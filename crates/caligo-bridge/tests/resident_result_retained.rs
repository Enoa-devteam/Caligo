use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{OwnedRequest, Resident, ResidentLimits, ResidentNotice, ResultHandoffGate, SendOutcome, SubmitVerdict};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Host { calls: Arc<AtomicUsize>, async_send: bool }
impl HostAdapter for Host {
    fn owner_thread_id(&self) -> u64 { 1 }
    fn current_thread_id(&self) -> u64 { 1 }
    fn env_valid(&self) -> bool { true }
    fn current_context_ok(&self) -> bool { true }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        match op {
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
            HostOp::SendText { request_id, .. } => {
                self.calls.fetch_add(1, Ordering::Relaxed);
                if self.async_send { Ok(HostOpResult::SendFired { mid: request_id }) }
                else { Ok(HostOpResult::Sent { native_id: Some("native".into()) }) }
            }
            _ => panic!("unexpected host operation"),
        }
    }
}
fn setup(async_send: bool) -> (Resident<Host>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let r = Resident::new(Host { calls: calls.clone(), async_send }, ResidentLimits { results_max: 2, ..ResidentLimits::default() });
    r.bootstrap().unwrap();
    assert_eq!(r.submit(request("r")), SubmitVerdict::Accepted);
    assert_eq!(r.drain().unwrap(), 1);
    (r, calls)
}
fn request(id: &str) -> OwnedRequest {
    OwnedRequest::SendText { request_id: id.into(), wire_seq: 1, timeout_ms: 5000, chat_type: 2, peer_uid: "123".into(), text: "fixture".into() }
}
#[test]
fn resident_retains_order_and_identity_after_channel_and_worker_loss() {
    let (r, calls) = setup(false);
    let (tx, rx) = std::sync::mpsc::channel();
    assert_eq!(ResultHandoffGate::default().forward(&r, &tx), Ok(2));
    let before = r.retained_notices();
    assert_eq!(before.len(), 2);
    assert!(before[0].notice_id > 0 && before[0].notice_id < before[1].notice_id);
    assert!(matches!(&before[0].notice, ResidentNotice::NativeStarted { .. }));
    assert!(matches!(&before[1].notice, ResidentNotice::Outcome(SendOutcome::Success { .. })));
    let worker_copy = rx.try_recv().unwrap();
    drop(worker_copy); drop(rx); // one accepted but not read, one read then lost
    assert_eq!(r.retained_notices(), before);
    let mut forged = before[1].clone(); forged.notice_id += 1;
    assert!(!r.ack_retained_notice(&forged));
    let mut wrong_payload = before[1].clone();
    wrong_payload.notice = before[0].notice.clone();
    assert!(!r.ack_retained_notice(&wrong_payload));
    assert_eq!(r.retained_notices(), before);
    assert_eq!(r.submit(request("new")), SubmitVerdict::QueueFull);
    assert!(r.ack_retained_notice(&before[0]));
    assert!(!r.ack_retained_notice(&before[0]));
    assert_eq!(r.submit(request("new")), SubmitVerdict::QueueFull);
    assert!(r.ack_retained_notice(&before[1]));
    assert_eq!(r.submit(request("new")), SubmitVerdict::Accepted);
    assert_eq!(calls.load(Ordering::Relaxed), 1, "replay must not invoke host");
}
#[test]
fn capacity_exhaustion_retains_unforwarded_suffix_and_latches() {
    let (r, calls) = setup(true);
    r.complete_sends(&[("r".into(), String::new())], &[], &[]);
    r.complete_sends(&[], &[], &[("r".into(), "unknown".into())]);
    r.complete_sends(&[("r".into(), "late-native".into())], &[], &[]);
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&r, &tx), Err(2));
    assert!(gate.is_closed());
    assert_eq!(r.retained_notices().len(), 2);
    let remaining = r.take_results();
    assert_eq!(remaining.len(), 2);
    assert!(matches!(&remaining[0], ResidentNotice::Outcome(SendOutcome::Unknown { .. })));
    assert!(matches!(&remaining[1], ResidentNotice::Outcome(SendOutcome::Success { .. })));
    assert_eq!(rx.try_iter().count(), 2);
    assert_eq!(gate.forward(&r, &tx), Err(0));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
#[test]
fn retained_unknown_ack_does_not_release_new_success_slot() {
    let (r, calls) = setup(true);
    let gate = ResultHandoffGate::default();
    let (tx, rx) = std::sync::mpsc::channel();
    assert_eq!(gate.forward(&r, &tx), Ok(1));
    let started = rx.try_recv().unwrap();
    assert!(r.ack_retained_notice(&started));
    r.complete_sends(&[], &[], &[("r".into(), "unknown".into())]);
    r.complete_sends(&[("r".into(), "strong".into())], &[], &[]);
    assert_eq!(gate.forward(&r, &tx), Ok(2));
    let unknown = rx.try_recv().unwrap();
    let success = rx.try_recv().unwrap();
    assert!(matches!(&unknown.notice, ResidentNotice::Outcome(SendOutcome::Unknown { .. })));
    assert!(matches!(&success.notice, ResidentNotice::Outcome(SendOutcome::Success { .. })));
    assert!(r.ack_retained_notice(&unknown));
    assert_eq!(r.submit(request("next")), SubmitVerdict::QueueFull);
    assert!(r.ack_retained_notice(&success));
    assert_eq!(r.submit(request("next")), SubmitVerdict::Accepted);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
