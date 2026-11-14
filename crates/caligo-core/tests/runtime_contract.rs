//! K4-D3 运行时契约测试(计划 §8.1:L03/L04/L08/L10/L11 及状态机验收)。
//!
//! "native 侧"由 `FakeBridge` 扮演:它持有 native 调用计数,是"旧代次触碰
//! 原生计数为 0 / 零 native 调用"断言的被测对象。所有用例零 QQ 依赖。

use caligo_core::runtime::{
    host_identity, AccountActor, CancelOutcome, EventReceipt, NativeResult, ReceiptOutcome,
    RuntimeConfig,
};
use caligo_model::{
    AccountId, ActionState, ConnectionIdentity, Direction, EventSource, HostIdentity, MsgEvent,
    NativeMessageId, Phase, RejectReason, SendTextRequest, SessionIdentity, SessionKey,
    SessionKind,
};

fn tmp_journal(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("caligo-runtime-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("journal.log")
}

fn host(nonce: u64) -> HostIdentity {
    host_identity(
        1000,
        "2026-10-07T00:00:00Z",
        "qq-9.9.33-52230",
        "caligo-bridge 0.1.0",
        nonce,
    )
}

fn session(gen: u64) -> SessionIdentity {
    SessionIdentity {
        host: host(7),
        account: AccountId("10001".into()),
        session_generation: gen,
    }
}

fn conn(epoch: u64) -> ConnectionIdentity {
    ConnectionIdentity {
        core_instance_nonce: 99,
        connection_epoch: epoch,
        protocol_version: 2,
    }
}

fn actor(tag: &str) -> AccountActor {
    let path = tmp_journal(tag);
    let _ = std::fs::remove_file(&path);
    let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();
    a
}

fn send_req(id: &str, gen: u64, target: &SessionKey, text: &str) -> SendTextRequest {
    let deadline = 30_000;
    SendTextRequest {
        request_id: id.into(),
        session_identity: session(gen),
        target: target.clone(),
        text: text.into(),
        payload_hash: SendTextRequest::compute_payload_hash(gen, target, text, deadline),
        deadline_ms: deadline,
    }
}

fn private_target(peer: &str) -> SessionKey {
    SessionKey::new("10001", SessionKind::Private, peer)
}

fn event(
    gen: u64,
    target: &SessionKey,
    native_id: &str,
    text: &str,
    source: EventSource,
) -> MsgEvent {
    MsgEvent {
        event_seq: 0,
        session_generation: gen,
        session: target.clone(),
        direction: Direction::Incoming,
        sender: "friend-b".into(),
        native_id: NativeMessageId(native_id.into()),
        text: text.into(),
        platform_time: Some(1_000),
        wrapper_message: None,
        onebot_projection: None,
        observed_at_unix_ms: 2_000,
        source,
    }
}

/// FakeBridge:模拟"bridge→native"边界;计数即验收探针。
#[derive(Default)]
struct FakeBridge {
    native_calls: u64,
    started: Vec<String>,
}

impl FakeBridge {
    fn dispatch(&mut self, actor: &mut AccountActor, id: &str) {
        actor.dispatch_begin(id).unwrap();
        actor.bridge_started(id).unwrap();
        self.native_calls += 1;
        self.started.push(id.into());
    }
}

// ---- L03:同数字 private/group peer 必须是两个会话 ----

#[test]
fn l03_same_numeric_peer_private_and_group_stay_distinct() {
    let mut a = actor("l03");
    let ev_private = event(
        1,
        &private_target("123456"),
        "N1",
        "hello",
        EventSource::Recv,
    );
    let ev_group = event(
        1,
        &SessionKey::new("10001", SessionKind::Group, "123456"),
        "N2",
        "hello",
        EventSource::Recv,
    );
    assert!(matches!(
        a.on_event(ev_private),
        Ok(EventReceipt::Published { .. })
    ));
    assert!(matches!(
        a.on_event(ev_group),
        Ok(EventReceipt::Published { .. })
    ));
    let delivered = a.drain_deliverables(10);
    assert_eq!(delivered.len(), 2);
    assert_ne!(delivered[0].session, delivered[1].session);
    // 同正文不去重:不同原生 ID 两条都保留(L04 前半)。
    assert_eq!(delivered[0].text, delivered[1].text);
}

// ---- L04:同正文不同 ID 保留;重复接收/重复更新不重复发布 ----

#[test]
fn l04_same_text_different_ids_kept_and_duplicates_suppressed() {
    let mut a = actor("l04");
    let t = private_target("123456");
    assert!(matches!(
        a.on_event(event(1, &t, "N1", "dup-text", EventSource::Recv)),
        Ok(EventReceipt::Published { .. })
    ));
    // 同 ID 再次接收 → 抑制。
    assert_eq!(
        a.on_event(event(1, &t, "N1", "dup-text", EventSource::Recv))
            .unwrap(),
        EventReceipt::DuplicateSuppressed
    );
    // 不同 ID 同正文 → 保留。
    assert!(matches!(
        a.on_event(event(1, &t, "N2", "dup-text", EventSource::Recv)),
        Ok(EventReceipt::Published { .. })
    ));
    // 同 ID 的 update → 作为更新发布一次(不是新 incoming)。
    assert!(matches!(
        a.on_event(event(1, &t, "N1", "dup-text", EventSource::Update)),
        Ok(EventReceipt::Published { .. })
    ));
    // 同 ID 的不同修订必须发布；相同修订重放才抑制。
    assert!(matches!(
        a.on_event(event(1, &t, "N1", "edited-text", EventSource::Update)),
        Ok(EventReceipt::Published { .. })
    ));
    assert_eq!(
        a.on_event(event(1, &t, "N1", "edited-text", EventSource::Update))
            .unwrap(),
        EventReceipt::DuplicateSuppressed
    );
    let delivered = a.drain_deliverables(10);
    assert_eq!(delivered.len(), 4, "recv N1, recv N2, and two distinct N1 updates");
    assert_eq!(delivered[0].source, EventSource::Recv);
    assert_eq!(delivered[2].source, EventSource::Update);
    assert_eq!(delivered[3].text, "edited-text");
    assert_eq!(a.counters.events_duplicate_suppressed, 2);
}

// ---- 状态机准入:未 Active 拒绝;stale 会话拒绝且零 native ----

#[test]
fn admission_requires_active_phase_and_current_session() {
    // Bound(未 IPC ready)→ 拒绝。
    let path = tmp_journal("adm");
    let _ = std::fs::remove_file(&path);
    let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    a.attach_session(session(1)).unwrap();
    assert_eq!(
        a.submit_send(send_req("r1", 1, &private_target("123"), "x")),
        Err(RejectReason::NotActive {
            phase: Phase::Bound
        })
    );
    a.mark_connection_ready(conn(1)).unwrap();

    // 旧代次身份 → SessionStale。
    let bridge = FakeBridge::default();
    assert_eq!(
        a.submit_send(send_req("r2", 0, &private_target("123"), "x")),
        Err(RejectReason::SessionStale)
    );
    assert_eq!(bridge.native_calls, 0, "拒绝必须发生在任何 native 之前");
}

// ---- L08:排队取消与 native_started 竞争 ----

#[test]
fn l08_cancel_vs_native_started_race() {
    let mut a = actor("l08");
    let mut bridge = FakeBridge::default();
    a.submit_send(send_req("c1", 1, &private_target("123"), "m1"))
        .unwrap();
    // 未派发:可取消,结果 cancelled_unsent,零 native。
    assert_eq!(
        a.cancel_queued("c1").unwrap(),
        CancelOutcome::CancelledUnsent
    );
    assert_eq!(a.query_request("c1"), Some(ActionState::CancelledUnsent));
    assert_eq!(bridge.native_calls, 0);

    // 已派发(native_started):取消必须 TooLate,不得宣称"未发送"。
    a.submit_send(send_req("c2", 1, &private_target("123"), "m2"))
        .unwrap();
    bridge.dispatch(&mut a, "c2");
    assert_eq!(a.cancel_queued("c2").unwrap(), CancelOutcome::TooLate);
    a.bridge_result(
        "c2",
        NativeResult::Success {
            native_id: NativeMessageId("NM2".into()),
        },
    )
    .unwrap();
    assert_eq!(
        a.query_request("c2"),
        Some(ActionState::ConfirmedSuccess {
            native_id: NativeMessageId("NM2".into())
        })
    );
    // 终态后取消 → TooLate(不重开、不撤销)。
    assert_eq!(a.cancel_queued("c2").unwrap(), CancelOutcome::TooLate);
}

// ---- L11:两个同正文请求 OK;同 ID 不同 payload 冲突拒绝 ----

#[test]
fn l11_request_identity_semantics() {
    let mut a = actor("l11");
    let t = private_target("123");
    a.submit_send(send_req("k1", 1, &t, "same-text")).unwrap();
    a.submit_send(send_req("k2", 1, &t, "same-text")).unwrap();
    assert_eq!(a.counters.sends_accepted, 2, "同正文不同 ID 各自成立");

    // 相同 ID + 相同 payload → 查询语义,不重复入队。
    let st = a.submit_send(send_req("k1", 1, &t, "same-text")).unwrap();
    assert_eq!(st, ActionState::Queued);
    assert_eq!(a.counters.requests_duplicate_query, 1);
    assert_eq!(a.counters.sends_accepted, 2);

    // 相同 ID + 不同 payload → 冲突拒绝,零执行。
    let mut r3 = send_req("k1", 1, &t, "different-text");
    r3.payload_hash = SendTextRequest::compute_payload_hash(1, &t, "different-text", 30_000);
    assert_eq!(
        a.submit_send(r3),
        Err(RejectReason::RequestIdPayloadConflict)
    );
}

// ---- L10:重复/迟到/未知/旧代次回执 → 幂等审计,不重开 ----

#[test]
fn l10_late_duplicate_and_unknown_receipts_are_idempotent_audit() {
    let mut a = actor("l10");
    let mut bridge = FakeBridge::default();
    a.submit_send(send_req("q1", 1, &private_target("123"), "m"))
        .unwrap();
    bridge.dispatch(&mut a, "q1");
    // 结果丢失:Unknown(不是失败)。
    assert_eq!(
        a.bridge_result(
            "q1",
            NativeResult::Unknown {
                reason: Some("response callback errCode=1006501; provenance unverified".into()),
            },
        )
        .unwrap(),
        ReceiptOutcome::Applied
    );
    assert_eq!(a.query_request("q1"), Some(ActionState::DeliveryUnknown));
    assert!(a
        .receipt_audits()
        .iter()
        .any(|entry| entry.detail.contains("errCode=1006501")));
    // 迟到明确回执:更正当前已知结果,不重开动作。
    assert_eq!(
        a.bridge_result(
            "q1",
            NativeResult::Success {
                native_id: NativeMessageId("NM9".into())
            }
        )
        .unwrap(),
        ReceiptOutcome::CorrectedFromUnknown
    );
    assert!(matches!(
        a.query_request("q1"),
        Some(ActionState::ConfirmedSuccess { .. })
    ));
    // 终态上的重复回执:幂等忽略 + 审计。
    assert_eq!(
        a.bridge_result(
            "q1",
            NativeResult::Success {
                native_id: NativeMessageId("NM9".into())
            }
        )
        .unwrap(),
        ReceiptOutcome::DuplicateIgnored
    );
    assert!(a.request_history("q1").unwrap().len() >= 4, "审计留痕");
    // 未知请求的回执:仅审计,不创建请求。
    assert_eq!(
        a.bridge_result("ghost", NativeResult::AcceptedUnconfirmed)
            .unwrap(),
        ReceiptOutcome::AuditedOnly
    );
    assert!(a.query_request("ghost").is_none());
    let audits = a.receipt_audits();
    assert_eq!(audits.len(), 3);
    assert!(audits
        .iter()
        .any(|entry| entry.detail.contains("errCode=1006501")));
    assert!(audits.iter().any(|entry| entry.request_id == "ghost"));
}

#[test]
fn business_acceptance_without_native_identity_is_not_confirmed_success() {
    let mut a = actor("accepted-unconfirmed");
    a.submit_send(send_req("R-ACK", 1, &private_target("123"), "accepted"))
        .unwrap();
    a.dispatch_begin("R-ACK").unwrap();
    a.bridge_started("R-ACK").unwrap();

    assert_eq!(
        a.bridge_result("R-ACK", NativeResult::AcceptedUnconfirmed)
            .unwrap(),
        ReceiptOutcome::Applied,
    );
    assert_eq!(
        a.query_request("R-ACK"),
        Some(ActionState::AcceptedUnconfirmed),
    );
    assert!(!a.query_request("R-ACK").unwrap().is_terminal());
    assert!(a.pending_queries().contains(&"R-ACK".to_string()));

    // A later strong native identity corrects the accepted-but-unconfirmed
    // fact without redispatching the original request.
    assert_eq!(
        a.bridge_result(
            "R-ACK",
            NativeResult::Success {
                native_id: NativeMessageId("NM-R-ACK".into()),
            }
        )
        .unwrap(),
        ReceiptOutcome::Applied,
    );
    assert_eq!(
        a.query_request("R-ACK"),
        Some(ActionState::ConfirmedSuccess {
            native_id: NativeMessageId("NM-R-ACK".into())
        }),
    );
    assert!(!a.pending_queries().contains(&"R-ACK".to_string()));
}

// ---- 动作队列满:明确未受理 ----

#[test]
fn action_queue_overflow_is_explicit_rejection() {
    let path = tmp_journal("qfull");
    let _ = std::fs::remove_file(&path);
    let cfg = RuntimeConfig {
        action_queue_max: 3,
        ..RuntimeConfig::default()
    };
    let (mut a, _) = AccountActor::open(&path, cfg).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();
    for i in 0..3 {
        a.submit_send(send_req(&format!("q{i}"), 1, &private_target("123"), "m"))
            .unwrap();
    }
    assert_eq!(
        a.submit_send(send_req("q3", 1, &private_target("123"), "m")),
        Err(RejectReason::CapacityExhausted { queue: "actions" })
    );
}

// ---- 事件队列溢出:Gap + Degraded(停止新发送),事件不静默丢 ----

#[test]
fn event_queue_overflow_records_gap_and_degrades() {
    let path = tmp_journal("efull");
    let _ = std::fs::remove_file(&path);
    let cfg = RuntimeConfig {
        event_queue_max_items: 2,
        ..RuntimeConfig::default()
    };
    let (mut a, _) = AccountActor::open(&path, cfg).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();
    let t = private_target("123");
    a.on_event(event(1, &t, "E1", "a", EventSource::Recv))
        .unwrap();
    a.on_event(event(1, &t, "E2", "b", EventSource::Recv))
        .unwrap();
    assert_eq!(
        a.on_event(event(1, &t, "E3", "c", EventSource::Recv)),
        Err(RejectReason::CapacityExhausted { queue: "events" })
    );
    assert_eq!(a.phase(), Phase::Degraded);
    assert_eq!(a.gaps().len(), 1);
    assert!(a.gaps()[0].to_event_seq.is_none(), "不得编造缺失范围");
    // Degraded 下发送入口关闭。
    assert_eq!(
        a.submit_send(send_req("s1", 1, &t, "m")),
        Err(RejectReason::NotActive {
            phase: Phase::Degraded
        })
    );
}

// ---- 停止协议:排队项取消;在途项 close 时显式 unknown;不回 Active ----

#[test]
fn stop_close_protocol_marks_states_honestly() {
    let mut a = actor("stop");
    let mut bridge = FakeBridge::default();
    a.submit_send(send_req("s1", 1, &private_target("123"), "queued"))
        .unwrap();
    a.submit_send(send_req("s2", 1, &private_target("123"), "inflight"))
        .unwrap();
    bridge.dispatch(&mut a, "s2");

    let cancelled = a.stop().unwrap();
    assert_eq!(cancelled, 1, "只有未派发项被取消");
    assert_eq!(a.query_request("s1"), Some(ActionState::CancelledUnsent));
    assert_eq!(a.phase(), Phase::Stopping);
    // Stopping 下不接受新动作,迟到回执不能把状态拉回 Active。
    assert_eq!(
        a.submit_send(send_req("s3", 1, &private_target("123"), "late")),
        Err(RejectReason::NotActive {
            phase: Phase::Stopping
        })
    );
    assert!(matches!(
        a.bridge_result("s2", NativeResult::AcceptedUnconfirmed),
        Ok(ReceiptOutcome::Applied) | Ok(ReceiptOutcome::CorrectedFromUnknown)
    ));
    let unknown = a.close().unwrap();
    assert_eq!(unknown, 1, "无稳定身份的 accepted 关闭时归 unknown");
    assert_eq!(a.query_request("s2"), Some(ActionState::DeliveryUnknown));
    assert_eq!(a.phase(), Phase::Closed);

    // 重开同一 journal:closed 后的动作仍可查询(审计持久)。
    let path = tmp_journal("stop");
    let (a2, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    assert!(matches!(
        a2.query_request("s2"),
        Some(ActionState::DeliveryUnknown)
    ));
}

// ---- 旧代次事件:只进审计,不进业务链 ----

#[test]
fn old_generation_events_are_audited_not_published() {
    let mut a = actor("oldgen");
    a.attach_session(session(2)).unwrap_err(); // 非 Detached → 拒绝,当前代次不变
                                               // 代次 0(旧)事件进来 → AuditedOldGeneration,不出现在交付队列。
    let r = a
        .on_event(event(
            0,
            &private_target("123"),
            "OLD1",
            "old",
            EventSource::Recv,
        ))
        .unwrap();
    assert!(matches!(r, EventReceipt::AuditedOldGeneration { .. }));
    assert_eq!(a.pending_event_queue_len(), 0);
    assert_eq!(a.counters.events_old_generation_audited, 1);
}
