//! Offline stop-window model only. No OS, V8, sending, or native lifetime proof.
#![cfg(feature = "research")]

pub const SAMPLE_INTERVAL_MS: u64 = 50;
pub const MAX_ACTIONS: u16 = 104;

/// Independent owner binding; construction validates bounds, every event compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopIdentity {
    pub account: String,
    pub host_nonce: u64,
    pub wrapper_session: String,
    pub listener_generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    InvalidConfig,
    IdentityMismatch,
    ClockReversed,
    Timeout,
    InvalidReceipt,
    UnknownOutcome,
    ActionBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    New,
    Stopping,
    InitialObservation,
    Observing,
    FinalObservation,
    ReadyToClose,
    Closing,
    Closed,
    Cancelled,
    Quarantined(Failure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    StopListener,
    Observe { final_sample: bool },
    ClosePump,
}

/// Issued once, never replayed by polling. Receipts must echo the entire action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    pub serial: u16,
    pub kind: ActionKind,
}

/// Counts only: contains no message or callback bytes. Not a quiescence claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub callbacks_seen: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receipt {
    ListenerStopped,
    Observed(Sample),
    PumpClosed,
    /// Includes sample errors, failed/ambiguous stop, and failed/ambiguous close.
    Unknown,
}

/// Explicit creation opts into a model window. Do not clone/reset after failure.
/// `now_ms` is elapsed monotonic time, not wall time. The overall deadline starts
/// at creation; a delayed initial observation must still leave the full window.
#[derive(Debug)]
pub struct StopObservationWindow {
    identity: StopIdentity,
    phase: Phase,
    last_now: u64,
    deadline: u64,
    duration_ms: u64,
    end: u64,
    next_sample: u64,
    pending: Option<Action>,
    actions: u16,
    cancelled: bool,
    last_sample: Option<Sample>,
}

impl StopObservationWindow {
    pub fn new(
        identity: StopIdentity,
        now_ms: u64,
        duration_ms: u64,
        wait_ms: u64,
    ) -> Result<Self, Failure> {
        let bounded = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control);
        if identity.host_nonce == 0
            || !bounded(&identity.account)
            || !bounded(&identity.wrapper_session)
            || !(1..=i32::MAX as u32).contains(&identity.listener_generation)
            || !(1..=5000).contains(&duration_ms)
            || wait_ms < duration_ms
        {
            return Err(Failure::InvalidConfig);
        }
        let deadline = now_ms.checked_add(wait_ms).ok_or(Failure::InvalidConfig)?;
        Ok(Self {
            identity,
            phase: Phase::New,
            last_now: now_ms,
            deadline,
            duration_ms,
            end: 0,
            next_sample: 0,
            pending: None,
            actions: 0,
            cancelled: false,
            last_sample: None,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn actions_issued(&self) -> u16 {
        self.actions
    }
    pub fn last_sample(&self) -> Option<Sample> {
        self.last_sample
    }

    fn quarantine<T>(&mut self, reason: Failure) -> Result<T, Failure> {
        self.phase = Phase::Quarantined(reason);
        self.pending = None;
        Err(reason)
    }

    fn validate(&mut self, now_ms: u64, identity: &StopIdentity) -> Result<(), Failure> {
        if let Phase::Quarantined(reason) = self.phase {
            return Err(reason);
        }
        if identity != &self.identity {
            return self.quarantine(Failure::IdentityMismatch);
        }
        if now_ms < self.last_now {
            return self.quarantine(Failure::ClockReversed);
        }
        self.last_now = now_ms;
        if now_ms > self.deadline {
            return self.quarantine(Failure::Timeout);
        }
        Ok(())
    }

    /// Stops admission immediately. Before any stop action, cancellation needs no
    /// close. After stop issuance, finish its receipt and initial/final samples.
    pub fn cancel(&mut self, now_ms: u64, identity: &StopIdentity) -> Result<(), Failure> {
        self.validate(now_ms, identity)?;
        self.cancelled = true;
        if self.phase == Phase::New {
            self.phase = Phase::Cancelled;
        }
        Ok(())
    }

    /// Returns each action at most once. While a receipt is outstanding returns
    /// None, even at expiry/cancellation; it never retries unknown operations.
    pub fn next_action(
        &mut self,
        now_ms: u64,
        identity: &StopIdentity,
    ) -> Result<Option<Action>, Failure> {
        self.validate(now_ms, identity)?;
        if self.pending.is_some() {
            return Ok(None);
        }
        let kind = match self.phase {
            Phase::New => {
                self.phase = Phase::Stopping;
                ActionKind::StopListener
            }
            Phase::InitialObservation => ActionKind::Observe {
                final_sample: false,
            },
            Phase::Observing if self.cancelled || now_ms >= self.end => {
                self.phase = Phase::FinalObservation;
                ActionKind::Observe { final_sample: true }
            }
            Phase::Observing if now_ms >= self.next_sample => ActionKind::Observe {
                final_sample: false,
            },
            Phase::ReadyToClose => {
                self.phase = Phase::Closing;
                ActionKind::ClosePump
            }
            _ => return Ok(None),
        };
        if self.actions >= MAX_ACTIONS {
            return self.quarantine(Failure::ActionBudget);
        }
        self.actions += 1;
        let action = Action {
            serial: self.actions,
            kind,
        };
        self.pending = Some(action);
        Ok(Some(action))
    }

    /// Exact serial/kind and typed outcome required. A final sample only permits
    /// closing after its receipt; a close receipt alone marks model completion.
    pub fn accept_receipt(
        &mut self,
        now_ms: u64,
        identity: &StopIdentity,
        action: Action,
        receipt: Receipt,
    ) -> Result<(), Failure> {
        self.validate(now_ms, identity)?;
        if self.pending != Some(action) {
            return self.quarantine(Failure::InvalidReceipt);
        }
        self.pending = None;
        match (action.kind, receipt) {
            (_, Receipt::Unknown) => return self.quarantine(Failure::UnknownOutcome),
            (ActionKind::StopListener, Receipt::ListenerStopped) => {
                self.phase = Phase::InitialObservation
            }
            (ActionKind::Observe { final_sample }, Receipt::Observed(sample)) => {
                self.last_sample = Some(sample);
                if final_sample {
                    self.phase = Phase::ReadyToClose;
                } else {
                    if self.phase == Phase::InitialObservation {
                        let Some(end) = now_ms.checked_add(self.duration_ms) else {
                            return self.quarantine(Failure::Timeout);
                        };
                        if end > self.deadline {
                            return self.quarantine(Failure::Timeout);
                        }
                        self.end = end;
                    }
                    // Saturation is safe: the bounded end/deadline still wins.
                    self.next_sample = now_ms.saturating_add(SAMPLE_INTERVAL_MS);
                    self.phase = Phase::Observing;
                }
            }
            (ActionKind::ClosePump, Receipt::PumpClosed) => self.phase = Phase::Closed,
            _ => return self.quarantine(Failure::InvalidReceipt),
        }
        Ok(())
    }
}
