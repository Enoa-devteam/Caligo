//! Opt-in research driver. Every sample runs on the admitted owner tick.
use super::*;
use crate::stop_observation::{ActionKind, Phase, Receipt, Sample, StopIdentity, StopObservationWindow};

type IdentityReader = Box<dyn Fn() -> Option<StopIdentity> + Send>;
type SnapshotReader = Box<dyn Fn(&StopIdentity) -> Result<Sample, &'static str> + Send>;
struct Driver {
    identity: IdentityReader,
    sample: SnapshotReader,
    window: Option<StopObservationWindow>,
    started: Option<Instant>,
    cancelled: bool,
}
static DRIVER: std::sync::Mutex<Option<Driver>> = std::sync::Mutex::new(None);
// Changes are serialized under DRIVER. The shutdown waiter reads this latch
// without taking locks held across an owner hook or sample.
static ARMED: AtomicBool = AtomicBool::new(false);
static DRAIN_RESULTS: AtomicBool = AtomicBool::new(false);
pub(super) static FAILED: AtomicBool = AtomicBool::new(false);

fn fail() -> EntryError {
    FAILED.store(true, Ordering::Release);
    quarantine_owner(None);
    append_stage_simple("stop_observation", false, "window_failed; roots retained");
    EntryError::StopObservationFailed
}

/// Fixed labels only: a stopped-session sample failure must stay diagnosable
/// without logging native detail. Unknown strings collapse to `other`.
const FAIL_REASONS: [&str; 9] = [
    "identity_unavailable",
    "observation_incomplete",
    "observation_binding_changed",
    "observation_binding_invalid",
    "observation_symbols_unavailable",
    "observation_v8_failed",
    "observation_shape",
    "observation_report_poisoned",
    "observation_report_budget",
];

fn bounded_reason(reason: &'static str) -> &'static str {
    FAIL_REASONS
        .iter()
        .copied()
        .find(|known| *known == reason)
        .unwrap_or("other")
}

fn fail_reason(reason: &'static str) -> EntryError {
    append_stage_simple("stop_observation_reason", false, bounded_reason(reason));
    fail()
}

pub fn install(identity: IdentityReader, sample: SnapshotReader) -> Result<(), EntryError> {
    let _hook = lock_drain_hook()?;
    if ENTRY.close_req.load(Ordering::Acquire) != 0 { return Err(EntryError::AlreadyBootstrapped); }
    let mut guard = DRIVER.lock().map_err(|_| EntryError::StopObservationFailed)?;
    if guard.is_some() { return Err(EntryError::AlreadyBootstrapped); }
    *guard = Some(Driver { identity, sample, window:None, started:None, cancelled:false });
    Ok(())
}

/// Caller-thread binding check reads cached metadata only, never V8.
pub fn arm(identity: StopIdentity, duration_ms: u64, wait_ms: u64) -> Result<(), EntryError> {
    arm_inner(identity, duration_ms, wait_ms, false)
}

#[doc(hidden)]
pub fn arm_with_result_drain(identity: StopIdentity, duration_ms: u64, wait_ms: u64) -> Result<(), EntryError> {
    arm_inner(identity, duration_ms, wait_ms, true)
}

pub(super) fn drains_results() -> bool {
    DRAIN_RESULTS.load(Ordering::Acquire)
}

fn arm_inner(identity: StopIdentity, duration_ms: u64, wait_ms: u64, drain_results: bool) -> Result<(), EntryError> {
    let _hook = lock_drain_hook()?;
    if ENTRY.ready.load(Ordering::Acquire) != 1 || ENTRY.close_req.load(Ordering::Acquire) != 0 {
        return Err(EntryError::AlreadyBootstrapped);
    }
    let window = StopObservationWindow::new(identity.clone(), 0, duration_ms, wait_ms)
        .map_err(|_| EntryError::StopObservationFailed)?;
    let mut guard = DRIVER.lock().map_err(|_| EntryError::StopObservationFailed)?;
    let driver = guard.as_mut().ok_or(EntryError::StopObservationFailed)?;
    if driver.window.is_some() { return Err(EntryError::AlreadyBootstrapped); }
    if (driver.identity)().as_ref() != Some(&identity) { return Err(EntryError::StopObservationFailed); }
    driver.window = Some(window);
    driver.started = Some(Instant::now());
    DRAIN_RESULTS.store(drain_results, Ordering::Release);
    ARMED.store(true, Ordering::Release);
    append_stage_simple("stop_observation", true, "armed");
    Ok(())
}

pub fn cancel() -> Result<(), EntryError> {
    let _hook = lock_drain_hook()?;
    let mut guard = DRIVER.lock().map_err(|_| EntryError::StopObservationFailed)?;
    let driver = guard.as_mut().ok_or(EntryError::StopObservationFailed)?;
    if driver.window.is_none() { return Err(EntryError::StopObservationFailed); }
    if driver.window.as_ref().is_some_and(|window| window.phase() == Phase::New) {
        // A started managed close has deliberately left the worker running.
        // Disarming now would skip the owner side of that close.
        if drains_results() && owner_stop_requested() {
            return Err(EntryError::StopObservationFailed);
        }
        driver.window = None;
        driver.started = None;
        DRAIN_RESULTS.store(false, Ordering::Release);
        ARMED.store(false, Ordering::Release);
    } else {
        driver.cancelled = true;
    }
    Ok(())
}

/// HOOK is held before DRIVER throughout owner execution, including cleanup.
/// None is the unchanged ordinary close path; false waits, true permits close.
pub(super) fn drive(hook: &Option<Box<dyn Fn() -> usize + Send>>) -> Result<Option<bool>, EntryError> {
    let mut guard = DRIVER.lock().map_err(|_| fail())?;
    let Some(driver) = guard.as_mut() else { return Ok(None); };
    let Some(window) = driver.window.as_mut() else { return Ok(None); };
    let started = *driver.started.as_ref().ok_or_else(fail)?;
    let elapsed = || u64::try_from(started.elapsed().as_millis()).map_err(|_| fail());
    let now = elapsed()?;
    let identity = (driver.identity)().ok_or_else(|| fail_reason("identity_unavailable"))?;
    if driver.cancelled {
        window.cancel(now, &identity).map_err(|_| fail())?;
        driver.cancelled = false;
    }
    // Only Stop + initial sample + final sample + close can coalesce in one tick.
    for _ in 0..4 {
        let now = elapsed()?;
        let identity = (driver.identity)().ok_or_else(|| fail_reason("identity_unavailable"))?;
        let Some(action) = window.next_action(now, &identity).map_err(|_| fail())? else {
            return Ok(Some(false));
        };
        match action.kind {
            ActionKind::StopListener => {
                if !invoke_drain_hook(hook) { return Err(owner_fault().unwrap_or(EntryError::StopObservationFailed)); }
                let identity = (driver.identity)().ok_or_else(|| fail_reason("identity_unavailable"))?;
                window.accept_receipt(elapsed()?, &identity, action, Receipt::ListenerStopped).map_err(|_| fail())?;
                append_stage_simple("stop_observation", true, "listener_stopped");
            }
            ActionKind::Observe { final_sample } => {
                let expected = ENTRY.qqnt_base.load(Ordering::Acquire).checked_add(EXPECTED_VTABLE_RVA as usize);
                if expected.is_none() || read_usize_checked(ENTRY.env.load(Ordering::Acquire)) != expected || !uv_loop_alive() {
                    return Err(fail());
                }
                let sample = (driver.sample)(&identity);
                // The waiter can quarantine while the owner is inside a sample.
                if let Some(error) = owner_fault() {
                    return Err(error);
                }
                let sample = match sample {
                    Ok(sample) => sample,
                    Err(reason) => return Err(fail_reason(reason)),
                };
                let identity = (driver.identity)().ok_or_else(|| fail_reason("identity_unavailable"))?;
                window.accept_receipt(elapsed()?, &identity, action, Receipt::Observed(sample)).map_err(|_| fail())?;
                if final_sample { append_stage_simple("stop_observation", true, "final_snapshot"); }
            }
            ActionKind::ClosePump => {
                append_stage_simple("stop_observation", true, "close_permitted");
                return Ok(Some(true));
            }
        }
    }
    Err(fail())
}

/// Caller thread posts only a libuv wake; it never executes samples or V8.
pub(super) fn observation_wake() -> Result<(), EntryError> {
    if !ARMED.load(Ordering::Acquire) || ENTRY.close_started.load(Ordering::Acquire) {
        return Ok(());
    }
    let gate = WAKE_GATE.lock().map_err(|_| EntryError::StopObservationFailed)?;
    if ARMED.load(Ordering::Acquire)
        && ENTRY.quarantine.load(Ordering::Acquire) == 0
        && !ENTRY.close_started.load(Ordering::Acquire)
        && !send_wake(&gate)
    {
        drop(gate);
        return Err(fail());
    }
    Ok(())
}

pub(super) fn finish() -> Result<(), EntryError> {
    let mut guard = DRIVER.lock().map_err(|_| fail())?;
    if let Some(driver) = guard.as_mut() {
        if let Some(window) = driver.window.as_mut() {
            let identity = (driver.identity)().ok_or_else(fail)?;
            let now = u64::try_from(driver.started.as_ref().ok_or_else(fail)?.elapsed().as_millis()).map_err(|_| fail())?;
            if window.phase() == Phase::Closing {
                let action = crate::stop_observation::Action { serial:window.actions_issued(), kind:ActionKind::ClosePump };
                window.accept_receipt(now, &identity, action, Receipt::PumpClosed).map_err(|_| fail())?;
            }
            if window.phase() != Phase::Closed { return Err(fail()); }
        }
    }
    Ok(())
}

pub(super) fn shutdown_timeout() -> Result<(), EntryError> {
    if ARMED.load(Ordering::Acquire) {
        return Err(fail());
    }
    Ok(())
}

pub(super) fn reset() {
    ARMED.store(false, Ordering::Release);
    DRAIN_RESULTS.store(false, Ordering::Release);
    FAILED.store(false, Ordering::Release);
    DRIVER.clear_poison();
    if let Ok(mut guard) = DRIVER.lock() { *guard = None; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_reasons_are_fixed_labels_and_unknowns_collapse() {
        for reason in FAIL_REASONS {
            assert_eq!(bounded_reason(reason), reason, "allowlisted reason must pass through");
        }
        assert_eq!(bounded_reason("fixture"), "other");
        assert_eq!(bounded_reason("PUBLIC-ACCOUNT-123"), "other");
        assert_eq!(bounded_reason(""), "other");
    }
}
