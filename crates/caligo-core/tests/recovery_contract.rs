//! K4-D3 恢复契约测试(计划 §8.1:L09/L13 与 §7-D3 验收)。
//!
//! 覆盖:崩溃窗口(dispatch_intent 前后)不重发;journal 尾截断保守恢复;
//! 中段损坏 → 拒绝且原文件不动;重启后 pending 逐项 QueryRequest;
//! 已确认事件不重复业务发布;ack/去重跨重启有效。

use caligo_core::journal::{Journal, JournalRecord, RequestRecord, SyncMode};
use caligo_core::message_store::MessageStore;
use caligo_core::onebot::StoreReader;
use caligo_core::runtime::{
    host_identity, AccountActor, NativeResult, ReceiptOutcome, RuntimeConfig,
};
use caligo_model::{
    AccountId, ConnectionIdentity, Direction, EventSource, MsgEvent, NativeMessageId,
    OneBotMessageProjection, SendTextRequest, SessionIdentity, SessionKey, SessionKind,
};

fn tmp_path(tag: &str, name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("caligo-recovery-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn host() -> caligo_model::HostIdentity {
    host_identity(
        1000,
        "2026-10-07T00:00:00Z",
        "qq-9.9.33-52230",
        "caligo-bridge 0.1.0",
        7,
    )
}

fn session(gen: u64) -> SessionIdentity {
    SessionIdentity {
        host: host(),
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

fn req(id: &str, text: &str) -> SendTextRequest {
    let target = SessionKey::new("10001", SessionKind::Private, "123456");
    let deadline = 30_000;
    SendTextRequest {
        request_id: id.into(),
        session_identity: session(1),
        target: target.clone(),
        text: text.into(),
        payload_hash: SendTextRequest::compute_payload_hash(1, &target, text, deadline),
        deadline_ms: deadline,
    }
}

fn event(native_id: &str, text: &str) -> MsgEvent {
    MsgEvent {
        event_seq: 0,
        session_generation: 1,
        session: SessionKey::new("10001", SessionKind::Private, "123456"),
        direction: Direction::Incoming,
        sender: "friend-b".into(),
        native_id: NativeMessageId(native_id.into()),
        text: text.into(),
        platform_time: None,
        wrapper_message: None,
        onebot_projection: None,
        observed_at_unix_ms: 1,
        source: EventSource::Recv,
    }
}

/// 模拟"core 进程崩溃后重启":drop actor(不 stop/close),重开同一 journal。
fn crash_and_reopen(
    a: AccountActor,
    path: &std::path::Path,
) -> (AccountActor, caligo_core::runtime::OpenReport) {
    drop(a);
    AccountActor::open(path, RuntimeConfig::default()).unwrap()
}

fn open_or_panic(
    path: &std::path::Path,
) -> Result<(AccountActor, caligo_core::runtime::OpenReport), caligo_core::runtime::OpenError> {
    AccountActor::open(path, RuntimeConfig::default())
}

// ---- L09:dispatch_intent 前后崩溃 → pending 查询,不自动重发 ----

#[test]
fn l09_crash_around_dispatch_intent_never_redispatches() {
    let path = tmp_path("l09", "journal.log");
    let _ = std::fs::remove_file(&path);
    let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();

    // 请求 A:崩溃于 dispatch_intent 之前(已 queued)。
    a.submit_send(req("A", "before-intent")).unwrap();
    // 请求 B:崩溃于 dispatch_intent 之后、bridge 确认之前。
    a.submit_send(req("B", "after-intent")).unwrap();
    a.dispatch_begin("B").unwrap();

    let (mut a2, report) = crash_and_reopen(a, &path);
    // 恢复报告:两个请求都进入 pending_queries;不自动发送(模块无任何
    // 自动派发 API —— 由本文件编译期事实保证:不存在 re-dispatch 方法)。
    assert_eq!(report.recovered_requests, 2);
    assert_eq!(
        a2.query_request("A"),
        Some(caligo_model::ActionState::Queued)
    );
    assert_eq!(
        a2.query_request("B"),
        Some(caligo_model::ActionState::DispatchIntent)
    );

    // 规定恢复流:重新核实会话身份 → 连接就绪(新连接代次)。
    a2.attach_session(session(1)).unwrap();
    a2.mark_connection_ready(conn(2)).unwrap();
    let pending = a2.pending_queries();
    assert_eq!(pending.len(), 2, "两个请求都仍需 QueryRequest");
    // 重连(再次换新连接代次)→ 返回同样 pending;状态不变,仍不自动执行。
    let pending2 = a2.reconnect(conn(3)).unwrap();
    assert_eq!(pending2.len(), 2);
    assert_eq!(
        a2.query_request("B"),
        Some(caligo_model::ActionState::DispatchIntent)
    );

    // bridge 对 B 的真实状态答复"未知"(结果丢失)→ delivery_unknown,不重发。
    assert_eq!(
        a2.bridge_result("B", NativeResult::Unknown { reason: None })
            .unwrap(),
        ReceiptOutcome::Applied
    );
    assert_eq!(
        a2.query_request("B"),
        Some(caligo_model::ActionState::DeliveryUnknown)
    );
    // A 从未派发:bridge 侧确认从未执行 → cancelled_unsent 合法。
    assert_eq!(
        a2.cancel_queued("A").unwrap(),
        caligo_core::runtime::CancelOutcome::CancelledUnsent
    );
}

#[test]
fn received_projection_is_indexed_and_get_msg_id_survives_actor_reopen() {
    let journal_path = tmp_path("onebot-index", "journal.log");
    let _ = std::fs::remove_file(&journal_path);
    let mut store_path = journal_path.clone();
    store_path.set_extension("messages.jsonl");
    let _ = std::fs::remove_file(&store_path);

    let store = MessageStore::open(&store_path, 10001, None).unwrap();
    let (mut actor, _) =
        AccountActor::open_with_message_store(&journal_path, RuntimeConfig::default(), store)
            .unwrap();
    actor.attach_session(session(1)).unwrap();
    let mut incoming = event("native-message-7", "persist me");
    incoming.sender = "opaque-sender-uid".into();
    incoming.onebot_projection = Some(OneBotMessageProjection {
        peer_id: 123456,
        sender_user_id: 10002,
        real_id: 7007,
        time: 1_791_641_234,
        sender_nickname: Some("sender".into()),
        sub_type: Some("friend".into()),
        font: Some(0),
    });
    assert!(matches!(
        actor.on_event(incoming.clone()).unwrap(),
        caligo_core::runtime::EventReceipt::Published { .. }
    ));
    assert!(matches!(
        actor.on_event(incoming).unwrap(),
        caligo_core::runtime::EventReceipt::DuplicateSuppressed
    ));
    let first = actor
        .get_msg(1)
        .expect("first OneBot ID is queryable")
        .clone();
    assert_eq!(first.real_id, 7007);
    assert_eq!(
        first.sender["user_id"], 10002,
        "opaque sender is not parsed"
    );
    assert_eq!(first.time, 1_791_641_234);
    drop(actor);

    let store = MessageStore::open(&store_path, 10001, None).unwrap();
    let (reopened, report) =
        AccountActor::open_with_message_store(&journal_path, RuntimeConfig::default(), store)
            .unwrap();
    assert_eq!(report.recovered_events, 1);
    let recovered = reopened.get_msg(1).expect("message ID remains stable");
    assert_eq!(recovered.message_id, first.message_id);
    assert_eq!(recovered.real_id, first.real_id);
    assert_eq!(recovered.message, first.message);

    let _ = std::fs::remove_file(journal_path);
    let _ = std::fs::remove_file(store_path);
}

#[test]
fn event_without_verified_projection_does_not_allocate_onebot_id() {
    let journal_path = tmp_path("onebot-unmapped", "journal.log");
    let _ = std::fs::remove_file(&journal_path);
    let mut store_path = journal_path.clone();
    store_path.set_extension("messages.jsonl");
    let _ = std::fs::remove_file(&store_path);

    let store = MessageStore::open(&store_path, 10001, None).unwrap();
    let (mut actor, _) =
        AccountActor::open_with_message_store(&journal_path, RuntimeConfig::default(), store)
            .unwrap();
    actor.attach_session(session(1)).unwrap();
    assert!(matches!(
        actor.on_event(event("stable-native", "text")).unwrap(),
        caligo_core::runtime::EventReceipt::Published { .. }
    ));
    assert!(actor.get_msg(1).is_none());

    drop(actor);
    let _ = std::fs::remove_file(journal_path);
    let _ = std::fs::remove_file(store_path);
}

#[test]
fn l09_crash_after_native_started_unknown_not_failure() {
    let path = tmp_path("l09b", "journal.log");
    let _ = std::fs::remove_file(&path);
    let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();
    a.submit_send(req("N", "maybe-sent")).unwrap();
    a.dispatch_begin("N").unwrap();
    a.bridge_started("N").unwrap();
    // 崩溃:bridge 的执行结果永久丢失。
    let (mut a2, _) = crash_and_reopen(a, &path);
    assert_eq!(
        a2.query_request("N"),
        Some(caligo_model::ActionState::NativeStarted)
    );
    a2.attach_session(session(1)).unwrap();
    a2.mark_connection_ready(conn(2)).unwrap();
    // 关闭时显式标记 unknown —— 不是失败、不是"未发送"。
    a2.stop().unwrap();
    let marked = a2.close().unwrap();
    assert_eq!(marked, 1);
    assert_eq!(
        a2.query_request("N"),
        Some(caligo_model::ActionState::DeliveryUnknown)
    );
}

// ---- L13:journal 尾截断保守恢复;已确认事实保留 ----

#[test]
fn l13_tail_truncation_recovers_confirmed_prefix() {
    let path = tmp_path("l13", "journal.log");
    let _ = std::fs::remove_file(&path);
    {
        let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
        a.attach_session(session(1)).unwrap();
        a.mark_connection_ready(conn(1)).unwrap();
        a.submit_send(req("T1", "ok")).unwrap();
        a.dispatch_begin("T1").unwrap();
        a.bridge_started("T1").unwrap();
        a.bridge_result(
            "T1",
            NativeResult::Success {
                native_id: NativeMessageId("NM1".into()),
            },
        )
        .unwrap();
    }
    // 模拟崩溃半条:追加垃圾字节。
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"\x43\x4C\x47\x4A\x01\x00").unwrap(); // 帧头前 6 字节(半条)
    drop(f);

    let (a2, report) = open_or_panic(&path).unwrap();
    assert!(report.journal_tail_truncated_bytes > 0, "半条被截去并报告");
    assert_eq!(report.transition_anomalies, 0);
    assert!(matches!(
        a2.query_request("T1"),
        Some(caligo_model::ActionState::ConfirmedSuccess { .. })
    ));
    assert_eq!(
        a2.request_summary("T1").unwrap().state,
        caligo_model::ActionState::ConfirmedSuccess {
            native_id: NativeMessageId("NM1".into())
        }
    );
}

#[test]
fn l14_legacy_success_without_native_id_restores_unconfirmed_and_preserves_journal() {
    let path = tmp_path("l14-legacy", "journal.log");
    let _ = std::fs::remove_file(&path);
    let (mut journal, _) = Journal::open(
        &path,
        caligo_core::journal::DEFAULT_MAX_BYTES,
        SyncMode::Always,
    )
    .unwrap();
    journal
        .append(&JournalRecord::Request(RequestRecord {
            v: 1,
            request_id: "R-LEGACY".into(),
            payload_hash: 0xC411_60,
            state: "ConfirmedSuccess".into(),
            native_id: None,
            at_unix_ms: 1_797_091_200_000,
            session_generation: 7,
            wire_seq: None,
        }))
        .unwrap();
    drop(journal);
    let bytes_before = std::fs::read(&path).unwrap();

    let (actor, report) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    assert_eq!(report.recovered_requests, 1);
    assert_eq!(report.transition_anomalies, 0);
    assert_eq!(
        actor.query_request("R-LEGACY"),
        Some(caligo_model::ActionState::AcceptedUnconfirmed),
    );
    assert!(actor.pending_queries().contains(&"R-LEGACY".to_string()));
    drop(actor);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes_before,
        "旧记录保留,恢复不改写历史"
    );
}

#[test]
fn r3_wire_sequence_is_persisted_and_not_reused_after_restart() {
    let path = tmp_path("r3-seq", "journal.log");
    let _ = std::fs::remove_file(&path);
    let (mut first, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    first.attach_session(session(1)).unwrap();
    first.mark_connection_ready(conn(1)).unwrap();
    first.submit_send(req("R-SEQ-1", "one")).unwrap();
    assert_eq!(first.dispatch_begin("R-SEQ-1").unwrap(), 1);
    drop(first);

    let (mut recovered, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    recovered.attach_session(session(1)).unwrap();
    recovered.mark_connection_ready(conn(2)).unwrap();
    recovered.submit_send(req("R-SEQ-2", "two")).unwrap();
    assert_eq!(recovered.dispatch_begin("R-SEQ-2").unwrap(), 2);
    drop(recovered);

    let (journal, report) = Journal::open(
        &path,
        caligo_core::journal::DEFAULT_MAX_BYTES,
        SyncMode::Always,
    )
    .unwrap();
    let sequences: Vec<(String, Option<u32>)> = report
        .records
        .into_iter()
        .filter_map(|record| match record {
            JournalRecord::Request(record) if record.state == "DispatchIntent" => {
                Some((record.request_id, record.wire_seq))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        sequences,
        vec![("R-SEQ-1".into(), Some(1)), ("R-SEQ-2".into(), Some(2))]
    );
    drop(journal);
}

// ---- L13:中段损坏 → 拒绝打开;原文件不动;不做静默重建 ----

#[test]
fn l13_mid_file_corruption_refuses_open_and_preserves_file() {
    let path = tmp_path("l13b", "journal.log");
    let _ = std::fs::remove_file(&path);
    {
        let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
        a.attach_session(session(1)).unwrap();
        a.mark_connection_ready(conn(1)).unwrap();
        a.submit_send(req("C1", "one")).unwrap();
        a.submit_send(req("C2", "two")).unwrap();
        a.submit_send(req("C3", "three")).unwrap();
    }
    // 破坏第二条记录的魔数字节(其后仍有完好第三条 → 中段坏帧)。
    let mut bytes = std::fs::read(&path).unwrap();
    // 第一条记录长度 = 16 + payload;payload 长度在 [8..12)。
    let first_payload_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let corrupt_at = 16 + first_payload_len;
    bytes[corrupt_at] ^= 0xFF;
    std::fs::write(&path, &bytes).unwrap();
    let before = std::fs::read(&path).unwrap();

    let err = match open_or_panic(&path) {
        Err(e) => e,
        Ok(_) => panic!("mid-file corruption must refuse open"),
    };
    assert!(err.to_string().contains("corrupt"), "{err}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "损坏文件原样保留,不静默重建"
    );
}

// ---- 重启后:去重仍有效;已交付事件不重复发布 ----

#[test]
fn restart_preserves_dedup_and_delivered_cursor() {
    let path = tmp_path("dedup", "journal.log");
    let _ = std::fs::remove_file(&path);
    let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();
    a.on_event(event("E1", "hello")).unwrap();
    a.on_event(event("E2", "world")).unwrap();
    assert_eq!(a.drain_deliverables(10).len(), 2);
    assert_eq!(a.delivered_cursor(), 2);

    let (mut a2, report) = crash_and_reopen(a, &path);
    assert_eq!(report.recovered_events, 2);
    // 规定恢复流:重新核实会话与连接后,事件面恢复受理。
    a2.attach_session(session(1)).unwrap();
    a2.mark_connection_ready(conn(2)).unwrap();
    // 恢复后:重复投递同 ID → 抑制(去重跨重启)。
    assert_eq!(
        a2.on_event(event("E1", "hello")).unwrap(),
        caligo_core::runtime::EventReceipt::DuplicateSuppressed
    );
    // 已交付事件不重复进入业务队列(重启后队列为空)。
    assert_eq!(a2.pending_event_queue_len(), 0);
    // 新事件继续按序分配 event_seq(不回卷)。
    let r = a2.on_event(event("E3", "fresh")).unwrap();
    assert!(matches!(
        r,
        caligo_core::runtime::EventReceipt::Published { event_seq: 3 }
    ));
}

// ---- ACK 丢失/重放窗口:core 侧账本是 ACK 依据(先持久后 ACK) ----

#[test]
fn event_ack_is_backed_by_persisted_ledger_across_restart() {
    let path = tmp_path("ack", "journal.log");
    let _ = std::fs::remove_file(&path);
    let (mut a, _) = AccountActor::open(&path, RuntimeConfig::default()).unwrap();
    a.attach_session(session(1)).unwrap();
    a.mark_connection_ready(conn(1)).unwrap();
    a.on_event(event("A1", "acked-later")).unwrap();
    // 模拟 ACK 丢失:core 崩溃;重启后该事件仍在账本(未交付游标之前),
    // 可由重放窗口再次核对(此处以 query 账本事实表达:事件计数 + 游标)。
    let (mut a2, report) = crash_and_reopen(a, &path);
    assert_eq!(report.recovered_events, 1);
    a2.attach_session(session(1)).unwrap();
    a2.mark_connection_ready(conn(2)).unwrap();
    assert_eq!(a2.delivered_cursor(), 0, "未 drain ⇒ 未交付,ACK 不得前移");
    // 重新受理同一事件 → 抑制(账本已有),保证 bridge 重放不会重复业务发布。
    assert_eq!(
        a2.on_event(event("A1", "acked-later")).unwrap(),
        caligo_core::runtime::EventReceipt::DuplicateSuppressed
    );
    // 重启后事件不重新入队(已由 restart_preserves_dedup 覆盖),因此
    // 这里 drain 为空、游标保持 0 —— ACK 依据是持久账本本身,而非内存队列。
    assert!(a2.drain_deliverables(10).is_empty());
    assert_eq!(a2.delivered_cursor(), 0);
    // seq 编号不回卷:下一条新事件从 2 开始。
    assert!(matches!(
        a2.on_event(event("A2", "next")).unwrap(),
        caligo_core::runtime::EventReceipt::Published { event_seq: 2 }
    ));
}
