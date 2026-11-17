use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{OwnedRequest, Resident, ResidentLimits, ResidentNotice, ResultHandoffGate, HandoffError, SendOutcome, SubmitVerdict};
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
            HostOp::Probe => Ok(HostOpResult::ProbeDone),
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
    assert_eq!(ResultHandoffGate::default().forward(&r, &tx), Err(HandoffError::Full(2)));
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
fn capacity_backpressure_resumes_in_fifo_order_after_ack() {
    let (r, calls) = setup(true);
    r.complete_sends(&[("r".into(), String::new())], &[], &[]);
    r.complete_sends(&[], &[], &[("r".into(), "unknown".into())]);
    r.complete_sends(&[("r".into(), "late-native".into())], &[], &[]);
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(2)));
    assert!(!gate.is_closed(), "capacity pressure must not permanently close handoff");
    assert_eq!(r.retained_notices().len(), 2);
    let started = rx.try_recv().unwrap();
    let accepted = rx.try_recv().unwrap();
    assert!(r.ack_retained_notice(&started));
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(1)));
    assert!(!gate.is_closed());
    let unknown = rx.try_recv().unwrap();
    assert!(matches!(&unknown.notice, ResidentNotice::Outcome(SendOutcome::Unknown { .. })));
    assert!(r.ack_retained_notice(&accepted));
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(1)));
    let success = rx.try_recv().unwrap();
    assert!(matches!(&success.notice, ResidentNotice::Outcome(SendOutcome::Success { .. })));
    assert!(started.notice_id < accepted.notice_id && accepted.notice_id < unknown.notice_id && unknown.notice_id < success.notice_id);
    assert!(r.ack_retained_notice(&unknown));
    assert_eq!(gate.forward(&r, &tx), Ok(0));
    assert_eq!(calls.load(Ordering::Relaxed), 1, "recovering notices must not repeat native send");
}
#[test]
fn channel_closed_while_full_latches_after_capacity_returns() {
    let (r, _) = setup(true);
    r.complete_sends(&[("r".into(), String::new())], &[], &[]);
    r.complete_sends(&[], &[], &[("r".into(), "unknown".into())]);
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(2)));
    let started = rx.try_recv().unwrap();
    drop(rx);
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(0)));
    assert!(!gate.is_closed());
    assert!(r.ack_retained_notice(&started));
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::ChannelClosed(0)));
    assert!(gate.is_closed());
    assert!(r.retained_notices().iter().any(|item| matches!(item.notice, ResidentNotice::Outcome(SendOutcome::Unknown { .. }))));
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::ChannelClosed(0)));
}

#[test]
fn full_preflight_pauses_dispatch_until_ack_frees_capacity() {
    let (r, calls) = setup(false);
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(2)));
    assert_eq!(r.submit(OwnedRequest::Probe { id: 7 }), SubmitVerdict::Accepted);
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(0)));
    assert_eq!(r.pending_ingress(), 1, "preflight must run before dispatch");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(r.ack_retained_notice(&rx.try_recv().unwrap()));
    assert_eq!(gate.forward(&r, &tx), Ok(0));
    assert_eq!(r.drain().unwrap(), 1);
    assert_eq!(r.pending_ingress(), 0);
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
    assert_eq!(gate.forward(&r, &tx), Err(HandoffError::Full(2)));
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
