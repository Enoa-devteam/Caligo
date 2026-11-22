//! Session revocation contract: network changes re-verify the same identity;
//! identity changes require a newer generation and old events stay audited.

use caligo_core::runtime::{
    host_identity, AccountActor, EventReceipt, RevokeReason, RuntimeConfig,
};
use caligo_model::{
    AccountId, ConnectionIdentity, Direction, EventSource, MsgEvent, NativeMessageId, Phase,
    SendTextRequest, SessionIdentity, SessionKey, SessionKind,
};

fn tmp_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("caligo-revoke-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("journal.log")
}

fn host() -> caligo_model::HostIdentity {
    host_identity(1000, "2026-10-07T00:00:00Z", "qq-9.9.33-52230", "caligo-bridge 0.1.0", 7)
}

fn session(gen: u64) -> SessionIdentity {
    SessionIdentity { host: host(), account: AccountId("10001".into()), session_generation: gen }
}

fn other_account(gen: u64) -> SessionIdentity {
    SessionIdentity { host: host(), account: AccountId("20002".into()), session_generation: gen }
}

fn conn(epoch: u64) -> ConnectionIdentity {
    ConnectionIdentity { core_instance_nonce: 99, connection_epoch: epoch, protocol_version: 2 }
}

fn req(id: &str, generation: u64) -> SendTextRequest {
    let target = SessionKey::new("10001", SessionKind::Private, "123456");
    let deadline = 30_000;
    SendTextRequest {
        request_id: id.into(),
        session_identity: session(generation),
        target: target.clone(),
        text: format!("revoke-{id}"),
        payload_hash: SendTextRequest::compute_payload_hash(1, &target, &format!("revoke-{id}"), deadline),
        deadline_ms: deadline,
    }
}

fn event(native_id: &str, generation: u64) -> MsgEvent {
    MsgEvent {
        event_seq: 0,
        session_generation: generation,
        session: SessionKey::new("10001", SessionKind::Private, "123456"),
        direction: Direction::Incoming,
        sender: "friend-b".into(),
        native_id: NativeMessageId(native_id.into()),
        text: "revoked-event".into(),
        platform_time: None,
        wrapper_message: None,
        onebot_projection: None,
        observed_at_unix_ms: 1,
        source: EventSource::Recv,
    }
}

fn active(tag: &str) -> AccountActor {
    let path = tmp_path(tag);
    let _ = std::fs::remove_file(&path);
    let (mut actor, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    actor.attach_session(session(1)).unwrap();
    actor.mark_connection_ready(conn(1)).unwrap();
    actor
}

#[test]
fn network_revoke_cancels_unsent_and_requires_same_identity() {
    let mut actor = active("network");
    actor.submit_send(req("queued", 1)).unwrap();
    actor.submit_send(req("dispatched", 1)).unwrap();
    actor.dispatch_begin("dispatched").unwrap();

    let report = actor.revoke_session(RevokeReason::NetworkChanged).unwrap();
    assert_eq!(report.cancelled_unsent, 1);
    assert_eq!(report.delivery_unknown, 1);
    assert_eq!(report.session_generation, 1);
    assert_eq!(actor.phase(), Phase::Detached);
    assert_eq!(
        actor.query_request("queued"),
        Some(caligo_model::ActionState::CancelledUnsent)
    );
    assert_eq!(
        actor.query_request("dispatched"),
        Some(caligo_model::ActionState::DeliveryUnknown)
    );
    assert!(actor.pending_queries().is_empty(), "revocation must not requeue requests");
    assert!(actor.submit_send(req("late", 1)).is_err(), "detached actor rejects sends");

    assert_eq!(actor.attach_session(other_account(1)), Err(caligo_model::RejectReason::SessionStale));
    assert_eq!(actor.attach_session(session(2)), Err(caligo_model::RejectReason::SessionStale));
    actor.attach_session(session(1)).unwrap();
    actor.mark_connection_ready(conn(2)).unwrap();
    assert_eq!(actor.phase(), Phase::Active);
    assert_eq!(
        actor.query_request("dispatched"),
        Some(caligo_model::ActionState::DeliveryUnknown),
        "recovery must not replay or reset a dispatched request"
    );
}

#[test]
fn new_generation_uses_a_fresh_ledger() {
    let old_path = tmp_path("gen-old");
    let (mut old, _) = AccountActor::open(&old_path, RuntimeConfig::default()).unwrap();
    old.attach_session(session(1)).unwrap();
    old.mark_connection_ready(conn(1)).unwrap();
    old.submit_send(req("old-request", 1)).unwrap();
    old.on_event(event("old-event", 1)).unwrap();
    let old_bytes = std::fs::read(&old_path).unwrap();

    let new_path = tmp_path("gen-new");
    let (mut new, _) = AccountActor::open(&new_path, RuntimeConfig::default()).unwrap();
    new.attach_session(session(2)).unwrap();
    new.mark_connection_ready(conn(1)).unwrap();
    assert_eq!(new.session().unwrap().session_generation, 2);
    assert!(new.query_request("old-request").is_none(), "old request must not enter the new ledger");
    assert_eq!(new.pending_event_queue_len(), 0, "old event must not enter the new ledger");
    assert_eq!(std::fs::read(&old_path).unwrap(), old_bytes, "old journal must stay untouched");
}

#[test]
fn identity_revoke_requires_new_generation_and_audits_old_events() {
    let mut actor = active("identity");
    actor.submit_send(req("pending", 1)).unwrap();
    let report = actor.revoke_session(RevokeReason::IdentityChanged).unwrap();
    assert_eq!(report.cancelled_unsent, 1);
    assert_eq!(actor.attach_session(session(1)), Err(caligo_model::RejectReason::SessionStale));
    actor.attach_session(session(2)).unwrap();
    actor.mark_connection_ready(conn(2)).unwrap();
    assert_eq!(actor.session().unwrap().session_generation, 2);

    let receipt = actor.on_event(event("old-1", 1)).unwrap();
    assert!(matches!(receipt, EventReceipt::AuditedOldGeneration { .. }));
    assert_eq!(actor.counters.events_old_generation_audited, 1);
    assert_eq!(actor.pending_event_queue_len(), 0);

    let receipt = actor.on_event(event("new-1", 2)).unwrap();
    assert!(matches!(receipt, EventReceipt::Published { .. } | EventReceipt::DuplicateSuppressed));
    assert_eq!(
        actor.query_request("pending"),
        Some(caligo_model::ActionState::CancelledUnsent),
        "old-generation request must stay cancelled"
    );
}
