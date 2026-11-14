//! K4-D4 常驻生命周期契约测试(计划 §7-D4 / §8.1:L07/L16/L17)。
//!
//! 自建宿主 = [`SharedMockHost`]:可控 owner 线程标识、env 有效性、
//! current 上下文、假会话服务语义(计数型)。全部用例零 QQ 依赖;
//! 当前测试线程的分配由计数分配器监控(L17 资源平衡)，不把并行测试的
//! 临时分配混入本用例的快照。
//!
//! 注意:本测试二进制安装了自己的 `#[global_allocator]`(计数分配器),
//! 仅用于本文件的资源增长断言。

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{
    CloseReport, OwnedEvent, OwnedRequest, Resident, ResidentLimits, SendOutcome, SubmitVerdict,
};

#[test]
fn private_voice_queue_rechecks_scope_deadline_and_deduplicates_before_owner_call() {
    use caligo_model::capability::QueryScope;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};
    struct VoiceHost(Arc<AtomicUsize>);
    impl HostAdapter for VoiceHost {
        fn owner_thread_id(&self) -> u64 { 1 }
        fn current_thread_id(&self) -> u64 { 1 }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn supports_private_voice(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
            match op {
                HostOp::ListenerAdd => Ok(HostOpResult::ListenerAdded { token: 1 }),
                HostOp::ListenerRemove { .. } => Ok(HostOpResult::ListenerRemoved),
                HostOp::SendVoice { request_id, wire_seq, peer_uin, chat_type, timeout_ms, silk } => {
                    assert_eq!(chat_type, 1);
                    assert_eq!(wire_seq, 7);
                    assert_eq!(peer_uin, "20002");
                    assert!((1..=5000).contains(&timeout_ms));
                    assert!(caligo_model::voice::validate_short_tencent_silk(&silk));
                    self.0.fetch_add(1, Ordering::Relaxed);
                    Ok(HostOpResult::SendFired { mid: request_id })
                }
                _ => panic!("fixture is voice-only"),
            }
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let resident = Resident::new(VoiceHost(calls.clone()), ResidentLimits::default());
    resident.bootstrap().unwrap();
    let scope = QueryScope {
        account: "10001".into(), host_nonce: 1, session_generation: 1, connection_epoch: 2,
    };
    let request = |id: &str, deadline| OwnedRequest::SendVoice {
        request_id: id.into(), wire_seq: 7, deadline, scope: scope.clone(),
        peer_uin: "20002".into(), chat_type: 1, silk: b"\x02#!SILK_V3\x01\x00\x42".to_vec(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    resident.set_bridge_scope(Some(scope.clone()));
    assert_eq!(resident.submit(request("stale", deadline)), SubmitVerdict::Accepted);
    resident.set_bridge_scope(None);
    resident.drain().unwrap();
    resident.set_bridge_scope(Some(scope.clone()));
    assert_eq!(resident.submit(request("expired", Instant::now())), SubmitVerdict::Accepted);
    resident.drain().unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let sent = request("one", deadline);
    assert_eq!(resident.submit(sent.clone()), SubmitVerdict::Accepted);
    assert_eq!(resident.submit(sent), SubmitVerdict::Accepted);
    resident.drain().unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    resident.close().unwrap();
}

#[test]
fn image_queue_rechecks_scope_capability_deadline_and_input_before_native_call() {
    use caligo_model::capability::QueryScope;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::{Duration, Instant};
    struct ImageHost {
        calls: Arc<AtomicUsize>,
        enabled: Arc<AtomicBool>,
    }
    impl HostAdapter for ImageHost {
        fn owner_thread_id(&self) -> u64 {
            1
        }
        fn current_thread_id(&self) -> u64 {
            1
        }
        fn env_valid(&self) -> bool {
            true
        }
        fn current_context_ok(&self) -> bool {
            true
        }
        fn supports_image(&self, chat: u32) -> bool {
            matches!(chat, 1 | 2) && self.enabled.load(Ordering::Relaxed)
        }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
            match op {
                HostOp::ListenerAdd => Ok(HostOpResult::ListenerAdded { token: 1 }),
                HostOp::ListenerRemove { .. } => Ok(HostOpResult::ListenerRemoved),
                HostOp::SendImage {
                    request_id,
                    wire_seq,
                    peer_uin,
                    chat_type,
                    timeout_ms,
                    png,
                } => {
                    assert_eq!(wire_seq, 7);
                    assert_eq!(peer_uin, "20002");
                    assert_eq!(chat_type, 1);
                    assert!((1..=5000).contains(&timeout_ms));
                    assert_eq!(caligo_model::png::dimensions(&png), Some((1, 1)));
                    self.calls.fetch_add(1, Ordering::Relaxed);
                    Ok(HostOpResult::SendFired { mid: request_id })
                }
                _ => panic!("image-only fixture"),
            }
        }
    }
    let png_hex = "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
    let png: Vec<u8> = png_hex
        .as_bytes()
        .chunks_exact(2)
        .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
        .collect();
    let calls = Arc::new(AtomicUsize::new(0));
    let enabled = Arc::new(AtomicBool::new(true));
    let resident = Resident::new(
        ImageHost {
            calls: calls.clone(),
            enabled: enabled.clone(),
        },
        ResidentLimits::default(),
    );
    resident.bootstrap().unwrap();
    let scope = QueryScope {
        account: "10001".into(),
        host_nonce: 1,
        session_generation: 1,
        connection_epoch: 2,
    };
    resident.set_bridge_scope(Some(scope.clone()));
    let request = |id: &str, deadline| OwnedRequest::SendImage {
        request_id: id.into(),
        wire_seq: 7,
        scope: scope.clone(),
        deadline,
        peer_uin: "20002".into(),
        chat_type: 1,
        png: png.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut invalid = request("invalid", deadline);
    if let OwnedRequest::SendImage { png, .. } = &mut invalid {
        png[0] = 0;
    }
    assert_eq!(resident.submit(invalid), SubmitVerdict::Invalid);
    assert_eq!(
        resident.submit(request("stale", deadline)),
        SubmitVerdict::Accepted
    );
    resident.set_bridge_scope(None);
    resident.drain().unwrap();
    resident.set_bridge_scope(Some(scope.clone()));
    assert_eq!(
        resident.submit(request("expired", Instant::now())),
        SubmitVerdict::Accepted
    );
    resident.drain().unwrap();
    assert_eq!(
        resident.submit(request("revoked", deadline)),
        SubmitVerdict::Accepted
    );
    enabled.store(false, Ordering::Relaxed);
    resident.drain().unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    enabled.store(true, Ordering::Relaxed);
    let sent = request("once", deadline);
    assert_eq!(resident.submit(sent.clone()), SubmitVerdict::Accepted);
    assert_eq!(resident.submit(sent), SubmitVerdict::Accepted);
    resident.drain().unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    resident.close().unwrap();
}

// ---- 计数分配器(L17:分配不随调度次数增长 / 关闭后回基线) ----

struct AllocationStats {
    enabled: Cell<bool>,
    live: Cell<usize>,
    total: Cell<usize>,
}

thread_local! {
    static ALLOCATION_STATS: AllocationStats = const {
        AllocationStats {
            enabled: Cell::new(false),
            live: Cell::new(0),
            total: Cell::new(0),
        }
    };
}

struct AllocationTracking;

impl AllocationTracking {
    fn start() -> Self {
        ALLOCATION_STATS.with(|stats| {
            stats.live.set(0);
            stats.total.set(0);
            stats.enabled.set(true);
        });
        Self
    }
}

impl Drop for AllocationTracking {
    fn drop(&mut self) {
        ALLOCATION_STATS.with(|stats| stats.enabled.set(false));
    }
}

fn record_alloc() {
    let _ = ALLOCATION_STATS.try_with(|stats| {
        if stats.enabled.get() {
            stats.live.set(stats.live.get() + 1);
            stats.total.set(stats.total.get() + 1);
        }
    });
}

fn record_dealloc() {
    let _ = ALLOCATION_STATS.try_with(|stats| {
        if stats.enabled.get() {
            stats.live.set(stats.live.get().saturating_sub(1));
        }
    });
}

struct CountingAlloc;

unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: 前置转发到系统分配器。
        let p = unsafe { std::alloc::System.alloc(layout) };
        if !p.is_null() {
            record_alloc();
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: 前置转发到系统分配器。
        unsafe { std::alloc::System.dealloc(ptr, layout) };
        record_dealloc();
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn live_allocs() -> usize {
    ALLOCATION_STATS.with(|stats| stats.live.get())
}

fn total_allocs() -> usize {
    ALLOCATION_STATS.with(|stats| stats.total.get())
}

// ---- SharedMockHost:自建假宿主(测试夹具,无 QQ) ----

#[derive(Default)]
struct MockHostInner {
    env_valid: bool,
    ctx_ok: bool,
    current_thread: u64,
    native_calls: u64,
    failed_ops: u64,
    /// 操作级 unknown:env/上下文检查通过,但操作中途环境失效
    /// (模拟 precheck 后、执行中的环境死亡;映射 DeliveryUnknown)。
    unknown_ops: u64,
    next_listener_token: u64,
    listener: Option<u64>,
    op_threads: Vec<u64>,
}

struct SharedMockHost {
    inner: Arc<std::sync::Mutex<MockHostInner>>,
    owner_thread: u64,
}

/// 测试侧控制面(与 resident 持有的 adapter 共享同一内部状态)。
#[derive(Clone)]
struct SharedControl {
    inner: Arc<std::sync::Mutex<MockHostInner>>,
}

impl SharedControl {
    fn set_env_valid(&self, ok: bool) {
        self.inner.lock().unwrap().env_valid = ok;
    }
    fn set_ctx_ok(&self, ok: bool) {
        self.inner.lock().unwrap().ctx_ok = ok;
    }
    /// 模拟外来线程执行 drain(错误线程)。
    fn masquerade_foreign_thread(&self) {
        self.inner.lock().unwrap().current_thread = 0x1BAD; // 非 owner
    }
    fn back_to_owner(&self) {
        self.inner.lock().unwrap().current_thread = OWNER;
    }
    fn native_calls(&self) -> u64 {
        self.inner.lock().unwrap().native_calls
    }
    fn listener_token(&self) -> Option<u64> {
        self.inner.lock().unwrap().listener
    }
    fn fail_next_native(&self) {
        self.inner.lock().unwrap().failed_ops += 1;
    }
    fn fail_next_op_unknown(&self) {
        self.inner.lock().unwrap().unknown_ops += 1;
    }
    fn all_native_on_owner(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .op_threads
            .iter()
            .all(|t| *t == OWNER)
    }
}

const OWNER: u64 = 0xF00D;

impl SharedMockHost {
    fn new() -> (Self, SharedControl) {
        let inner = Arc::new(std::sync::Mutex::new(MockHostInner {
            env_valid: true,
            ctx_ok: true,
            current_thread: OWNER,
            next_listener_token: 5000,
            ..Default::default()
        }));
        (
            Self {
                inner: inner.clone(),
                owner_thread: OWNER,
            },
            SharedControl { inner },
        )
    }
}

impl HostAdapter for SharedMockHost {
    fn owner_thread_id(&self) -> u64 {
        self.owner_thread
    }
    fn current_thread_id(&self) -> u64 {
        self.inner.lock().unwrap().current_thread
    }
    fn env_valid(&self) -> bool {
        self.inner.lock().unwrap().env_valid
    }
    fn current_context_ok(&self) -> bool {
        self.inner.lock().unwrap().ctx_ok
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        let mut inner = self.inner.lock().unwrap();
        // 防御纵深:实现内部自检(与 resident 层检查独立,两道防线)。
        if inner.current_thread != self.owner_thread {
            return Err(HostError::NotOwnerThread);
        }
        if !inner.env_valid {
            return Err(HostError::EnvInvalid);
        }
        if !inner.ctx_ok {
            return Err(HostError::NoCurrentContext);
        }
        inner.native_calls += 1;
        let tid = inner.current_thread;
        inner.op_threads.push(tid);
        if inner.failed_ops > 0 {
            inner.failed_ops -= 1;
            return Err(HostError::Native { code: 9 });
        }
        if inner.unknown_ops > 0 {
            inner.unknown_ops -= 1;
            return Err(HostError::EnvInvalid);
        }
        match op {
            HostOp::ListenerAdd => {
                inner.next_listener_token += 1;
                inner.listener = Some(inner.next_listener_token);
                Ok(HostOpResult::ListenerAdded {
                    token: inner.next_listener_token,
                })
            }
            HostOp::ListenerRemove { .. } => {
                inner.listener = None;
                Ok(HostOpResult::ListenerRemoved)
            }
            HostOp::SendVoice { .. } | HostOp::SendImage { .. } => panic!("fixture does not send voice"),
            HostOp::Probe => Ok(HostOpResult::ProbeDone),
            HostOp::SendText { text, .. } if text == "accepted-without-id" => {
                Ok(HostOpResult::Sent { native_id: None })
            }
            HostOp::SendText { .. } => Ok(HostOpResult::Sent {
                native_id: Some(format!("NM-{}", inner.native_calls)),
            }),
        }
    }
}

fn fixture() -> (Arc<Resident<SharedMockHost>>, SharedControl) {
    let (host, control) = SharedMockHost::new();
    let r = Arc::new(Resident::new(host, ResidentLimits::default()));
    (r, control)
}

struct CorrelationHost {
    owner: u64,
    pending: Arc<std::sync::Mutex<Option<String>>>,
    response_id: Option<String>,
}

impl HostAdapter for CorrelationHost {
    fn owner_thread_id(&self) -> u64 {
        self.owner
    }
    fn current_thread_id(&self) -> u64 {
        self.owner
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        Ok(match op {
            HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
            HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
            HostOp::SendVoice { .. } | HostOp::SendImage { .. } => panic!("fixture does not send voice"),
            HostOp::Probe => HostOpResult::ProbeDone,
            HostOp::SendText { request_id, .. } => {
                *self.pending.lock().unwrap() = Some(request_id);
                HostOpResult::SendFired {
                    mid: "MID-correlation".into(),
                }
            }
        })
    }
    fn drain_send_responses(&mut self) -> Vec<(String, SendOutcome)> {
        self.pending
            .lock()
            .unwrap()
            .take()
            .map(|request_id| {
                vec![(
                    self.response_id
                        .clone()
                        .unwrap_or_else(|| request_id.clone()),
                    SendOutcome::Success {
                        request_id: self.response_id.clone().unwrap_or(request_id),
                        native_id: "NATIVE-correlation".into(),
                    },
                )]
            })
            .unwrap_or_default()
    }
}

struct LateReceiptHost {
    owner: u64,
    outcomes: Arc<std::sync::Mutex<VecDeque<(String, SendOutcome)>>>,
}

impl HostAdapter for LateReceiptHost {
    fn owner_thread_id(&self) -> u64 {
        self.owner
    }
    fn current_thread_id(&self) -> u64 {
        self.owner
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        Ok(match op {
            HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
            HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
            HostOp::SendVoice { .. } | HostOp::SendImage { .. } => panic!("fixture does not send voice"),
            HostOp::Probe => HostOpResult::ProbeDone,
            HostOp::SendText { .. } => HostOpResult::SendFired {
                mid: "MID-late".into(),
            },
        })
    }
    fn drain_send_responses(&mut self) -> Vec<(String, SendOutcome)> {
        self.outcomes.lock().unwrap().drain(..).collect()
    }
}

#[test]
fn unknown_holds_bounded_slot_for_late_strong_identity_then_releases_it() {
    let outcomes = Arc::new(std::sync::Mutex::new(VecDeque::new()));
    let resident = Resident::new(
        LateReceiptHost {
            owner: OWNER,
            outcomes: outcomes.clone(),
        },
        ResidentLimits {
            results_max: 2,
            ..ResidentLimits::default()
        },
    );
    resident.bootstrap().unwrap();
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-late".into(),
            wire_seq: 9,
            timeout_ms: 5,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "x".into(),
        }),
        SubmitVerdict::Accepted
    );
    assert_eq!(resident.drain().unwrap(), 1);
    assert!(resident.take_results().iter().any(|notice| matches!(
        notice,
        caligo_bridge::resident::ResidentNotice::NativeStarted { request_id }
            if request_id == "R-late"
    )));
    assert!(resident.ack_notice("R-late", true));

    outcomes.lock().unwrap().push_back((
        "R-late".into(),
        SendOutcome::Unknown {
            request_id: "R-late".into(),
            reason: Some("timeout".into()),
            late_receipt_possible: true,
        },
    ));
    assert_eq!(resident.drain_send_responses(), 1);
    let unknown = resident.take_results();
    assert!(unknown.iter().any(|notice| matches!(
        notice,
        caligo_bridge::resident::ResidentNotice::Outcome(SendOutcome::Unknown { request_id, .. })
            if request_id == "R-late"
    )));
    assert!(resident.ack_result_notice("R-late", true));
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "blocked-by-correction-slot".into(),
            wire_seq: 10,
            timeout_ms: 5,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "x".into(),
        }),
        SubmitVerdict::QueueFull
    );

    outcomes.lock().unwrap().push_back((
        "R-late".into(),
        SendOutcome::Success {
            request_id: "R-late".into(),
            native_id: "N-late".into(),
        },
    ));
    assert_eq!(resident.drain_send_responses(), 1);
    assert!(matches!(
        resident.take_results().as_slice(),
        [caligo_bridge::resident::ResidentNotice::Outcome(SendOutcome::Success {
            request_id,
            native_id,
        })] if request_id == "R-late" && native_id == "N-late"
    ));
    assert!(resident.ack_result_notice("R-late", true));
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "after-correction".into(),
            wire_seq: 11,
            timeout_ms: 5,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "x".into(),
        }),
        SubmitVerdict::Accepted
    );
}

#[test]
fn unknown_correction_slot_expires_and_unblocks_new_dispatch() {
    let outcomes = Arc::new(std::sync::Mutex::new(VecDeque::new()));
    let resident = Resident::new(
        LateReceiptHost {
            owner: OWNER,
            outcomes: outcomes.clone(),
        },
        ResidentLimits {
            results_max: 2,
            ..ResidentLimits::default()
        },
    );
    resident.bootstrap().unwrap();
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-expiring".into(),
            wire_seq: 13,
            timeout_ms: 1,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "x".into(),
        }),
        SubmitVerdict::Accepted
    );
    resident.drain().unwrap();
    resident.take_results();
    assert!(resident.ack_notice("R-expiring", true));
    outcomes.lock().unwrap().push_back((
        "R-expiring".into(),
        SendOutcome::Unknown {
            request_id: "R-expiring".into(),
            reason: Some("timeout".into()),
            late_receipt_possible: true,
        },
    ));
    resident.drain_send_responses();
    resident.take_results();
    assert!(resident.ack_result_notice("R-expiring", true));
    assert_eq!(
        resident.expire_late_receipts(
            std::time::Instant::now()
                + std::time::Duration::from_millis(caligo_model::LATE_RECEIPT_GRACE_MS + 1),
        ),
        1
    );
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-after-grace".into(),
            wire_seq: 14,
            timeout_ms: 1,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "x".into(),
        }),
        SubmitVerdict::Accepted
    );
}

#[test]
fn original_request_id_survives_async_response_correlation() {
    let pending = Arc::new(std::sync::Mutex::new(None));
    let resident = Resident::new(
        CorrelationHost {
            owner: OWNER,
            pending: pending.clone(),
            response_id: None,
        },
        ResidentLimits::default(),
    );
    resident.bootstrap().unwrap();
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-origin".into(),
            wire_seq: 1,
            timeout_ms: 30_000,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "hello".into(),
        }),
        SubmitVerdict::Accepted
    );

    assert!(
        resident.take_results().is_empty(),
        "submit acceptance is not NativeStarted"
    );
    resident.drain().unwrap();
    assert_eq!(*pending.lock().unwrap(), Some("R-origin".into()));
    resident.drain_send_responses();
    assert!(matches!(resident.take_results().as_slice(), [
        caligo_bridge::resident::ResidentNotice::NativeStarted { request_id: started },
        caligo_bridge::resident::ResidentNotice::Outcome(SendOutcome::Success {
            request_id,
            native_id,
        }),
    ] if started == "R-origin" && request_id == "R-origin" && native_id == "NATIVE-correlation"));
}

#[test]
fn unknown_native_response_id_is_audited_and_not_forwarded() {
    let pending = Arc::new(std::sync::Mutex::new(None));
    let resident = Resident::new(
        CorrelationHost {
            owner: OWNER,
            pending,
            response_id: Some("N-inner".into()),
        },
        ResidentLimits::default(),
    );
    resident.bootstrap().unwrap();
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-origin".into(),
            wire_seq: 1,
            timeout_ms: 30_000,
            chat_type: 2,
            peer_uid: "123".into(),
            text: "hello".into(),
        }),
        SubmitVerdict::Accepted
    );
    resident.drain().unwrap();
    assert_eq!(
        resident.drain_send_responses(),
        0,
        "unknown identity is not associated"
    );
    assert!(matches!(resident.take_results().as_slice(), [
        caligo_bridge::resident::ResidentNotice::NativeStarted { request_id }
    ] if request_id == "R-origin"));
    assert_eq!(resident.counters().orphan_results_ignored, 1);
}

// ---- 验收:bootstrap 一次;listener 注册在 owner;precheck 先于宿主 API ----

#[test]
fn bootstrap_once_and_listener_registered_on_owner() {
    let (r, ctl) = fixture();
    assert_eq!(ctl.native_calls(), 0, "bootstrap 前零宿主调用");
    let _tok = r.bootstrap().unwrap();
    assert_eq!(ctl.native_calls(), 1, "bootstrap = 恰一次 ListenerAdd");
    assert!(ctl.listener_token().is_some());
    // 二次 bootstrap:每宿主代次一次。
    assert!(r.bootstrap().is_err());
    assert_eq!(ctl.native_calls(), 1);
    assert!(ctl.all_native_on_owner());
}

// ---- L07:外来线程 / 零上下文 / env 失效 —— 一切宿主 API 之前拒绝 ----

#[test]
fn l07_foreign_thread_and_zero_context_rejected_before_any_host_api() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    r.submit(OwnedRequest::Probe { id: 1 });
    // 外来线程 drain:precheck 在任何 native 之前拒绝,队列原样保留。
    ctl.masquerade_foreign_thread();
    assert!(matches!(r.drain(), Err(HostError::NotOwnerThread)));
    assert_eq!(ctl.native_calls(), 1, "仅 bootstrap 一次调用,零业务 native");
    assert_eq!(r.pending_ingress(), 1, "被拒项不得静默丢弃");
    ctl.back_to_owner();

    // current 缺失(零上下文):同样在任何宿主 API 之前拒绝。
    r.submit(OwnedRequest::Probe { id: 2 });
    ctl.set_ctx_ok(false);
    assert!(matches!(r.drain(), Err(HostError::NoCurrentContext)));
    assert_eq!(ctl.native_calls(), 1);
    ctl.set_ctx_ok(true);

    // env 销毁:拒绝;恢复后可继续(队列未丢)。
    ctl.set_env_valid(false);
    assert!(matches!(r.drain(), Err(HostError::EnvInvalid)));
    ctl.set_env_valid(true);
    assert_eq!(r.drain().unwrap(), 2, "恢复后两条都执行");
    assert_eq!(ctl.native_calls(), 3);
    assert!(ctl.all_native_on_owner());
}

// ---- L16:关闭协议 + 迟回调拒绝 + 幂等 double close ----

#[test]
fn l16_close_protocol_and_late_callbacks() {
    let (r, ctl) = fixture();
    let tok = r.bootstrap().unwrap();
    r.submit(OwnedRequest::SendText {
        request_id: "a".into(),
        wire_seq: 1,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".repeat(2),
    });
    r.submit(OwnedRequest::SendText {
        request_id: "b".into(),
        wire_seq: 2,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".repeat(3),
    });
    assert_eq!(r.drain().unwrap(), 2);

    let report = r.close().unwrap();
    assert_eq!(
        report,
        CloseReport {
            cancelled_unsent: 0,
            marked_unknown: 0,
            listener_removed: Some(true),
            late_callbacks_rejected: 0,
            already_closed: false,
        }
    );
    assert!(ctl.listener_token().is_none(), "监听器对称移除并确认");
    assert_eq!(
        r.submit(OwnedRequest::Probe { id: 99 }),
        SubmitVerdict::Closed
    );

    // 迟到回调(已关闭):拒绝且零 native。
    let before = ctl.native_calls();
    assert!(r
        .deliver_callback(
            tok,
            OwnedEvent {
                source: caligo_bridge::resident::EventSourceKind::Recv,
                chat_type: 1,
                peer_uid: String::new(),
                peer_uin: String::new(),
                sender_uin: String::new(),
                native_id: "late".into(),
                text: String::new(),
                msg_time: None,
                wrapper_message: None,
            },
        )
        .is_err());
    assert_eq!(ctl.native_calls(), before, "迟回调零宿主触碰");

    // double close:幂等。
    let again = r.close().unwrap();
    assert!(again.already_closed);
}

#[test]
fn close_cancels_queued_and_reports_failure_vs_unknown() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    // native 服务明确报错(调用完成但失败)→ Failure 结果,不是 unknown。
    ctl.fail_next_native();
    r.submit(OwnedRequest::SendText {
        request_id: "inflight".into(),
        wire_seq: 3,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".repeat(1),
    });
    assert_eq!(r.drain().unwrap(), 1);
    assert_eq!(r.request_state("inflight"), Some("done"));
    let outcomes = r.take_results();
    assert_eq!(
        outcomes,
        vec![caligo_bridge::resident::ResidentNotice::Outcome(
            caligo_bridge::resident::SendOutcome::Failure {
                request_id: "inflight".into(),
                reason: "native code 9".into(),
            }
        )]
    );
    // 操作级环境失效(precheck 过、执行中死)→ DeliveryUnknown,不冒充失败。
    ctl.fail_next_op_unknown();
    r.submit(OwnedRequest::SendText {
        request_id: "ghost".into(),
        wire_seq: 4,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".repeat(1),
    });
    assert_eq!(r.drain().unwrap(), 1);
    assert_eq!(r.request_state("ghost"), Some("delivery_unknown"));
    let outcomes = r.take_results();
    assert_eq!(outcomes.len(), 1);
    assert!(matches!(
        outcomes[0],
        caligo_bridge::resident::ResidentNotice::Outcome(
            caligo_bridge::resident::SendOutcome::Unknown { .. }
        )
    ));
    // 排队未执行项随 close 取消。
    r.submit(OwnedRequest::SendText {
        request_id: "queued".into(),
        wire_seq: 5,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".repeat(1),
    });
    let report = r.close().unwrap();
    assert_eq!(report.cancelled_unsent, 1);
    assert_eq!(r.request_state("queued"), Some("cancelled_unsent"));
    assert_eq!(report.marked_unknown, 0, "前两项已显式化");
}

#[test]
fn listener_removal_failure_quarantines_instead_of_fake_success() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    ctl.fail_next_native(); // close 内的 ListenerRemove 将失败
    assert!(r.close().is_err());
    assert!(
        r.quarantine_reason(),
        "无法证明清理 → Quarantined,不当作正常关闭"
    );
    // 隔离态不可洗白为正常关闭。
    assert!(r.close().is_err());
}

// ---- L17:10,000 次非发送调度 + 100 轮关闭/重建,资源回基线 ----

#[test]
fn l17_ten_thousand_probes_no_growth_and_counters_balance() {
    let _tracking = AllocationTracking::start();
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    assert_eq!(
        r.counters().native_ops_total,
        1,
        "bootstrap 的 ListenerAdd 计入 native 总数"
    );
    // 预热(排除首次建表的一次性分配)。
    for i in 0..500u64 {
        r.submit(OwnedRequest::Probe { id: i });
        assert_eq!(r.drain().unwrap(), 1);
    }
    let warm_live = live_allocs();
    let warm_total = total_allocs();

    for i in 500..10_500u64 {
        r.submit(OwnedRequest::Probe { id: i });
        assert_eq!(r.drain().unwrap(), 1);
    }
    let total_delta = total_allocs() - warm_total;
    // 稳态:每调度 ≤ 2 次分配(排队元组),且 live 不随轮数增长。
    assert!(
        total_delta <= 2 * 10_000 + 1_000,
        "稳态分配异常增长: {total_delta}"
    );
    assert!(
        live_allocs() <= warm_live + 64,
        "live 分配随调度增长: {} -> {}",
        warm_live,
        live_allocs()
    );
    let c = r.counters();
    assert_eq!(c.dispatched_total, 10_500);
    assert_eq!(c.native_ops_total, 10_501);
    assert_eq!(r.pending_ingress(), 0);
    let report = r.close().unwrap();
    assert_eq!(report.listener_removed, Some(true));
    assert_eq!(ctl.listener_token(), None);
}

#[test]
fn l17_hundred_close_rebuild_cycles_return_to_baseline() {
    let _tracking = AllocationTracking::start();
    let live_before = live_allocs();
    for cycle in 0..100u64 {
        let (r, ctl) = fixture();
        r.bootstrap().unwrap();
        r.submit(OwnedRequest::Probe { id: cycle });
        assert_eq!(r.drain().unwrap(), 1);
        let report = r.close().unwrap();
        assert_eq!(report.listener_removed, Some(true));
        assert!(ctl.listener_token().is_none());
        let c = r.counters();
        assert_eq!(c.listener_adds, 1);
        assert_eq!(c.listener_removes, 1);
    }
    // 100 轮后 live 分配回基线(允许少量常量残差)。
    let after = live_allocs();
    assert!(
        after <= live_before + 128,
        "100 轮后资源未回基线: before={live_before} after={after}"
    );
}

// ---- 跨线程:任意线程提交、owner 执行(TSFN 语义) ----

#[test]
fn submit_from_worker_threads_executes_only_on_owner() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    let mut handles = Vec::new();
    // 总提交量 = 队列上限(32):生产在 worker,消费在主线程的 drain 之外。
    for w in 0..4u64 {
        let rr = r.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..8u64 {
                assert_eq!(
                    rr.submit(OwnedRequest::Probe { id: w * 8 + i }),
                    SubmitVerdict::Accepted
                );
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let mut drained = 0;
    loop {
        let n = r.drain().unwrap();
        drained += n;
        if n == 0 {
            break;
        }
    }
    assert_eq!(drained, 32);
    assert!(ctl.all_native_on_owner(), "全部 native 调用都在 owner 线程");
    r.close().unwrap();
}

// ---- 有界队列:满即明确拒绝;drain 分批,有剩余可继续调度 ----

#[test]
fn bounded_queues_reject_explicitly_and_drain_batches() {
    let (host, _ctl) = SharedMockHost::new();
    // 队列上限(40)> 单轮批次(16):验证分批与"不依赖新消息唤醒"。
    let r = Arc::new(Resident::new(
        host,
        ResidentLimits {
            ingress_max: 40,
            events_max: 256,
            results_max: 256,
            drain_batch: 16,
        },
    ));
    r.bootstrap().unwrap();
    for i in 0..40u64 {
        assert_eq!(
            r.submit(OwnedRequest::Probe { id: i }),
            SubmitVerdict::Accepted
        );
    }
    assert_eq!(
        r.submit(OwnedRequest::Probe { id: 9999 }),
        SubmitVerdict::QueueFull,
        "队列满必须显式拒绝"
    );
    assert_eq!(r.drain().unwrap(), 16);
    assert_eq!(r.pending_ingress(), 24);
    assert_eq!(r.drain().unwrap(), 16, "有剩余时不依赖新消息唤醒");
    assert_eq!(r.drain().unwrap(), 8);
    assert_eq!(r.drain().unwrap(), 0);
}

// ---- 重复提交同 ID send:查询语义,不重复派发 ----

#[test]
fn duplicate_send_request_id_is_not_redispatched() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    let req = OwnedRequest::SendText {
        request_id: "dup".into(),
        wire_seq: 6,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".repeat(5),
    };
    assert_eq!(r.submit(req.clone()), SubmitVerdict::Accepted);
    assert_eq!(
        r.submit(req),
        SubmitVerdict::Accepted,
        "重复 ID 接受但不重复入队"
    );
    assert_eq!(r.pending_ingress(), 1);
    assert_eq!(r.drain().unwrap(), 1);
    let calls = ctl.native_calls();
    assert_eq!(r.request_state("dup"), Some("done"));
    r.drain().unwrap();
    assert_eq!(ctl.native_calls(), calls, "第二次 drain 不会再次执行 dup");
}

#[test]
fn result_notice_capacity_is_reserved_until_core_ack() {
    let (r, _) = fixture();
    r.bootstrap().unwrap();
    let mut notices = Vec::new();
    for n in 0..128 {
        let request_id = format!("bounded-{n}");
        assert_eq!(
            r.submit(OwnedRequest::SendText {
                request_id,
                wire_seq: n + 1,
                timeout_ms: 30_000,
                chat_type: 1,
                peer_uid: "p".into(),
                text: "x".into(),
            }),
            SubmitVerdict::Accepted
        );
        assert_eq!(r.drain().unwrap(), 1);
        notices.extend(r.take_results()); // ACK 故意延迟,模拟断线
    }
    assert_eq!(notices.len(), 256, "每个动作预留 started/result 两个槽");
    let overflow = OwnedRequest::SendText {
        request_id: "bounded-overflow".into(),
        wire_seq: 500,
        timeout_ms: 30_000,
        chat_type: 1,
        peer_uid: "p".into(),
        text: "x".into(),
    };
    assert_eq!(r.submit(overflow.clone()), SubmitVerdict::QueueFull);

    for notice in notices {
        match notice {
            caligo_bridge::resident::ResidentNotice::NativeStarted { request_id } => {
                assert!(r.ack_notice(&request_id, true));
            }
            caligo_bridge::resident::ResidentNotice::Outcome(outcome) => {
                let request_id = match outcome {
                    SendOutcome::Success { request_id, .. }
                    | SendOutcome::AcceptedUnconfirmed { request_id }
                    | SendOutcome::Failure { request_id, .. }
                    | SendOutcome::Unknown { request_id, .. } => request_id,
                };
                assert!(r.ack_result_notice(&request_id, true));
            }
        }
    }
    assert_eq!(r.submit(overflow), SubmitVerdict::Accepted);
}

#[test]
fn accepted_unconfirmed_ack_keeps_terminal_capacity_and_never_redispatches() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    for n in 0..255u32 {
        let request_id = format!("accepted-{n}");
        let req = OwnedRequest::SendText {
            request_id: request_id.clone(),
            wire_seq: n + 1,
            timeout_ms: 30_000,
            chat_type: 2,
            peer_uid: "group".into(),
            text: "accepted-without-id".into(),
        };
        assert_eq!(r.submit(req.clone()), SubmitVerdict::Accepted);
        assert_eq!(r.drain().unwrap(), 1);
        assert_eq!(r.request_state(&request_id), Some("identity_pending"));
        let notices = r.take_results();
        assert_eq!(notices.len(), 2);
        for notice in notices {
            match notice {
                caligo_bridge::resident::ResidentNotice::NativeStarted { request_id } => {
                    assert!(r.ack_notice(&request_id, true));
                }
                caligo_bridge::resident::ResidentNotice::Outcome(
                    SendOutcome::AcceptedUnconfirmed { request_id },
                ) => {
                    assert!(r.ack_result_notice(&request_id, false));
                }
                other => panic!("unexpected notice: {other:?}"),
            }
        }
        assert_eq!(
            r.submit(req),
            SubmitVerdict::Accepted,
            "相同 request id 只查询，不重派发"
        );
    }
    let overflow = OwnedRequest::SendText {
        request_id: "accepted-overflow".into(),
        wire_seq: 256,
        timeout_ms: 30_000,
        chat_type: 2,
        peer_uid: "group".into(),
        text: "accepted-without-id".into(),
    };
    assert_eq!(
        r.submit(overflow),
        SubmitVerdict::QueueFull,
        "中间态 ACK 不释放终态槽"
    );
    assert_eq!(
        ctl.native_calls(),
        256,
        "仅 bootstrap + 255 次首次 dispatch"
    );
}

// ---- env 销毁后:业务调度拒绝且零 native(仅 bootstrap 的 1 次存在) ----

#[test]
fn after_env_destroyed_business_dispatch_touches_nothing() {
    let (r, ctl) = fixture();
    r.bootstrap().unwrap();
    ctl.set_env_valid(false);
    r.submit(OwnedRequest::Probe { id: 1 });
    assert!(matches!(r.drain(), Err(HostError::EnvInvalid)));
    assert_eq!(ctl.native_calls(), 1, "仅 bootstrap;业务零触碰");
}

// ---- P4 锁范围改造:内联回调重入不死锁 + SendPending 关闭分类(T14) ----

use std::sync::OnceLock;

/// 测试线程 tid(与宿主 owner 一致:单线程执行语义)。
fn this_tid() -> u64 {
    // SAFETY: 无副作用。
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
}

/// 重入宿主:native_op 期间回调 resident 的 core 面板(deliver_callback/
/// token)—— 改造前 drain/close 持 core 锁调 native_op,同线程重入即死锁。
struct ReentrantHost {
    resident: Arc<OnceLock<Arc<Resident<ReentrantHost>>>>,
    remove_reenters: bool,
}

static REENTRANT_SENDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl HostAdapter for ReentrantHost {
    fn owner_thread_id(&self) -> u64 {
        this_tid()
    }
    fn current_thread_id(&self) -> u64 {
        this_tid()
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(
        &mut self,
        op: HostOp,
    ) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
        use caligo_bridge::host_adapter::HostError;
        let r = self.resident.get().expect("resident slot set");
        match op {
            HostOp::SendVoice { .. } | HostOp::SendImage { .. } => panic!("fixture does not send voice"),
            HostOp::SendText { .. } => {
                // 重入点 1:token()(core 锁);重入点 2:deliver_callback(core 锁)。
                let tok = r.token();
                let _ = r.deliver_callback(
                    tok,
                    caligo_bridge::resident::OwnedEvent {
                        source: caligo_bridge::resident::EventSourceKind::Recv,
                        chat_type: 1,
                        peer_uid: "peer".into(),
                        peer_uin: "peer".into(),
                        sender_uin: "sender".into(),
                        native_id: format!(
                            "NM-R-{}",
                            REENTRANT_SENDS.fetch_add(1, Ordering::Relaxed)
                        ),
                        text: "inline".into(),
                        msg_time: None,
                        wrapper_message: None,
                    },
                );
                Ok(HostOpResult::Sent {
                    native_id: Some("NM-INLINE".into()),
                })
            }
            HostOp::ListenerRemove { .. } => {
                if self.remove_reenters {
                    let tok = r.token();
                    let _ = r.deliver_callback(
                        tok,
                        caligo_bridge::resident::OwnedEvent {
                            source: caligo_bridge::resident::EventSourceKind::Recv,
                            chat_type: 1,
                            peer_uid: "p".into(),
                            peer_uin: "p".into(),
                            sender_uin: "s".into(),
                            native_id: "NM-LATE-CLOSE".into(),
                            text: "late-during-remove".into(),
                            msg_time: None,
                            wrapper_message: None,
                        },
                    );
                }
                Ok(HostOpResult::ListenerRemoved)
            }
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerAdded { token: 1 }),
            HostOp::Probe => Ok(HostOpResult::ProbeDone),
        }
    }
}

/// 改造前:drain 持 core 锁调 native_op → 内联 token()/deliver_callback()
/// 同线程重入 core 锁 → 死锁。改造后:三段式(claim/invoke/complete)完成。
#[test]
fn inline_callback_reentrancy_no_deadlock() {
    let slot: Arc<OnceLock<Arc<Resident<ReentrantHost>>>> = Arc::new(OnceLock::new());
    let resident = Arc::new(Resident::new(
        ReentrantHost {
            resident: Arc::clone(&slot),
            remove_reenters: false,
        },
        ResidentLimits::default(),
    ));
    slot.set(Arc::clone(&resident)).ok().unwrap();
    resident.bootstrap().unwrap();
    assert_eq!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-INLINE".into(),
            wire_seq: 7,
            timeout_ms: 30_000,
            chat_type: 1,
            peer_uid: "peer".into(),
            text: "x".into(),
        }),
        SubmitVerdict::Accepted
    );
    let n = resident.drain().unwrap();
    assert_eq!(n, 1);
    // 内联回调送达 core 面板(经重入),结果与事件都在。
    let outcomes = resident.take_results();
    assert!(matches!(
        outcomes.as_slice(),
        [caligo_bridge::resident::ResidentNotice::NativeStarted { request_id: started },
         caligo_bridge::resident::ResidentNotice::Outcome(SendOutcome::Success { request_id, .. })]
            if started == "R-INLINE" && request_id == "R-INLINE"
    ));
    let events = resident.take_events();
    assert_eq!(events.len(), 1, "内联回调经重入面板送达");
}

/// close 的 ListenerRemove 期间迟到回调重入 —— 不死锁,且事件入 Stopping
/// 窗口(随后随 close 清空计数)。
#[test]
fn close_listener_remove_allows_reentrant_callback() {
    let slot: Arc<OnceLock<Arc<Resident<ReentrantHost>>>> = Arc::new(OnceLock::new());
    let resident = Arc::new(Resident::new(
        ReentrantHost {
            resident: Arc::clone(&slot),
            remove_reenters: true,
        },
        ResidentLimits::default(),
    ));
    slot.set(Arc::clone(&resident)).ok().unwrap();
    resident.bootstrap().unwrap();
    let report = resident.close().unwrap();
    assert_eq!(report.listener_removed, Some(true));
    // 存活到断言 = 无死锁。
}

/// T14(resident 级,C7):SendPending 在 close 时分类 DeliveryUnknown,
/// 不残留 send_pending。
#[test]
fn send_pending_classified_unknown_on_close() {
    // SendFired 宿主:drain 后停留在 SendPending(Promise 未回)。
    struct FiredHost;
    impl HostAdapter for FiredHost {
        fn owner_thread_id(&self) -> u64 {
            this_tid()
        }
        fn current_thread_id(&self) -> u64 {
            this_tid()
        }
        fn env_valid(&self) -> bool {
            true
        }
        fn current_context_ok(&self) -> bool {
            true
        }
        fn native_op(
            &mut self,
            op: HostOp,
        ) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
            Ok(match op {
                HostOp::ListenerAdd => HostOpResult::ListenerAdded { token: 1 },
                HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
                HostOp::SendVoice { .. } | HostOp::SendImage { .. } => panic!("fixture does not send voice"),
                HostOp::Probe => HostOpResult::ProbeDone,
                HostOp::SendText { .. } => HostOpResult::SendFired {
                    mid: "MID-1".into(),
                },
            })
        }
    }
    let resident = Arc::new(Resident::new(FiredHost, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    assert!(matches!(
        resident.submit(OwnedRequest::SendText {
            request_id: "R-PENDING".into(),
            wire_seq: 8,
            timeout_ms: 30_000,
            chat_type: 1,
            peer_uid: "peer".into(),
            text: "x".into(),
        }),
        SubmitVerdict::Accepted
    ));
    let _ = resident.drain().unwrap();
    assert_eq!(resident.request_state("R-PENDING"), Some("send_pending"));
    let report = resident.close().unwrap();
    assert!(
        report.marked_unknown >= 1,
        "SendPending 必须计入 marked_unknown"
    );
    assert_eq!(
        resident.request_state("R-PENDING"),
        Some("delivery_unknown")
    );
}
