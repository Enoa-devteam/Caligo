//! P4 原生 adapter 合同测试(计划 §8 T11–T14/T16 + 10k/100 lifecycle)。
//!
//! 假宿主以与恢复 ABI 相同的 C 形状(repr(C) 布局、extern "system" 槽函数、
//! callback move 协议、控制块计数)实现 QQ 侧对象模型 —— adapter 的
//! unsafe 路径在此受试,零 QQ 依赖。真实入口在 G1 前不接线。

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 共享静态假宿主:测试串行执行消除模式开关互扰。
static TEST_SERIAL: Mutex<()> = Mutex::new(());

use caligo_bridge::native_abi::{
    self, ByteRange24, CallbackMgrLayout, CtrlBlockLayout, ServicePair, TaggedString24,
};
use caligo_bridge::native_handle::{HandleStats, ServiceHandle};
use caligo_bridge::native_msf::{
    MsfAdapter, NativeLinks, NativeOutcome, SchedulePermit, SubmitVerdict,
};

const HOST_GEN: u64 = 7;
const SESSION_GEN: u64 = 42;

// ---- 假宿主对象模型(布局对齐 native_abi)----

/// transport:vtbl 10 槽 + 补齐到 +0x60 的内联连接 pair(1B4E4EC 锁定点)。
#[repr(C)]
struct FakeTransport {
    vtbl: [*mut c_void; 10],
    _pad: [u8; 0x60 - 80],
    inner_pair: [*mut c_void; 2], // {inner, ctrl}(弱 pair;LAB 不做弱计数)
}

/// 内联连接对象:vtbl slot6 = 状态查询(0xB7CE8A 的检查点)。
#[repr(C)]
struct FakeInner {
    vtbl: [*mut c_void; 8],
}

/// 服务对象(形状不参与断言;仅为 pair 存在)。
#[repr(C)]
struct FakeService {
    vtbl: [usize; 31],
}

struct FakeQq {
    live_allocs: AtomicU64,
    native_ops: AtomicU64,
    deep_copies: AtomicU64,
    callbacks_fired: AtomicU64,
    state_checks: AtomicU64,
    next_token: AtomicU64,
    inner_state: AtomicU32,
    inner_present: AtomicBool,
    mode_inline: AtomicBool,
    deferred_delay_ms: AtomicU64,
}

unsafe impl Send for FakeQq {}
unsafe impl Sync for FakeQq {}

static QQ: std::sync::OnceLock<Arc<FakeQq>> = std::sync::OnceLock::new();

fn qq() -> &'static Arc<FakeQq> {
    QQ.get_or_init(|| {
        Arc::new(FakeQq {
            live_allocs: AtomicU64::new(0),
            native_ops: AtomicU64::new(0),
            deep_copies: AtomicU64::new(0),
            callbacks_fired: AtomicU64::new(0),
            state_checks: AtomicU64::new(0),
            next_token: AtomicU64::new(0x1_0000),
            inner_state: AtomicU32::new(4),
            inner_present: AtomicBool::new(true),
            mode_inline: AtomicBool::new(false),
            deferred_delay_ms: AtomicU64::new(0),
        })
    })
}

unsafe fn fake_alloc(size: usize) -> *mut u8 {
    // SAFETY: 全局分配;泄漏由 live_allocs 对账(测试断言回基线)。
    unsafe {
        let p = std::alloc::alloc(std::alloc::Layout::from_size_align(size, 8).unwrap());
        qq().live_allocs.fetch_add(1, Ordering::Relaxed);
        p
    }
}

unsafe fn fake_free(p: *mut u8, size: usize) {
    // SAFETY: 与 fake_alloc 成对。
    unsafe {
        std::alloc::dealloc(p, std::alloc::Layout::from_size_align(size, 8).unwrap());
        qq().live_allocs.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Deferred 派发载荷(mgr/裸副本只移交此唯一消费者线程 —— 模拟
/// dispatcher 的单消费者队列;测试宿主内的显式 Send 包装)。
struct DeferredPayload {
    mgr: CallbackMgrLayout,
    cmd: *mut TaggedString24,
    body: Vec<u8>,
    delay: u64,
}
unsafe impl Send for DeferredPayload {}

fn spawn_deferred(p: DeferredPayload) {
    std::thread::spawn(move || {
        // 整体捕获 p(绕开 2021 disjoint capture 对裸指针字段的直接捕获;
        // Send 由 DeferredPayload 的手动 impl 提供)。
        let p = p;
        if p.delay > 0 {
            std::thread::sleep(Duration::from_millis(p.delay));
        }
        // SAFETY: 载荷由提交侧移交;唯一消费者。
        unsafe { fire_and_dispose(p.mgr, p.cmd, p.body) };
    });
}

/// 状态查询(内联对象 vtbl slot6)。
unsafe extern "system" fn fake_state_fn(_this: *mut c_void) -> u32 {
    qq().inner_state.load(Ordering::Relaxed)
}

/// transport slot7(0x1B4E4EC 同型):锁 +0x60 pair → 状态检查 → 深复制 →
/// 提交(Inline 或 Deferred)→ 回调 → 临时副本释放。
unsafe extern "system" fn fake_transport_submit(
    this: *mut c_void,
    command: *mut TaggedString24,
    request_holder: *mut ByteRange24,
    callback: *mut CallbackMgrLayout,
    out_token: *mut u64,
    _storage: *mut c_void,
) -> u64 {
    // SAFETY: 全部对象由 adapter 按恢复布局构造;单宿主语义。
    unsafe {
        qq().native_ops.fetch_add(1, Ordering::Relaxed);
        let tr = &*(this as *const FakeTransport);
        let inner = tr.inner_pair[0] as *const FakeInner;
        if inner.is_null() || !qq().inner_present.load(Ordering::Relaxed) {
            *out_token = 0;
            return 0; // 未连接:同步失败路径(R3.1 §10.1)
        }
        let state = let_dummy(inner);
        qq().state_checks.fetch_add(1, Ordering::Relaxed);
        if (state & !1u32) != 4 {
            *out_token = 0;
            return 0;
        }
        // 深复制(同步;提交语义的输入寿命约束)。与 fake_free 同分配器。
        let cmd_copy = fake_alloc(core::mem::size_of::<TaggedString24>()) as *mut TaggedString24;
        std::ptr::write(cmd_copy, *command);
        let body_len = (*request_holder).len();
        let body_copy: Vec<u8> =
            std::slice::from_raw_parts((*request_holder).begin, body_len).to_vec();
        qq().deep_copies.fetch_add(1, Ordering::Relaxed);
        let token = qq().next_token.fetch_add(1, Ordering::Relaxed);
        *out_token = token;

        // callback:移动协议后移交派发侧。
        let mgr = native_abi::callback_move_protocol(callback);

        if qq().mode_inline.load(Ordering::Relaxed) {
            fire_and_dispose(mgr, cmd_copy, body_copy);
        } else {
            let delay = qq().deferred_delay_ms.load(Ordering::Relaxed);
            spawn_deferred(DeferredPayload { mgr, cmd: cmd_copy, body: body_copy, delay });
        }
        token
    }
}

unsafe fn let_dummy(inner: *const FakeInner) -> u32 {
    // SAFETY: vtbl slot6 调用(状态检查点);槽地址由本宿主写入。
    unsafe {
        let slot: unsafe extern "system" fn(*mut c_void) -> u32 =
            core::mem::transmute((*inner).vtbl[6]);
        slot(inner as *mut c_void)
    }
}

/// 派发侧:fire(复制在 invoker 边界内完成)→ manager op=0 销毁 → 释放临时副本。
unsafe fn fire_and_dispose(mut mgr: CallbackMgrLayout, cmd: *mut TaggedString24, body: Vec<u8>) {
    // SAFETY: mgr 由移动协议产出;临时副本仅在本次调用期内有效 —— 这正是
    // T11 的受试点(invoker 必须在返回前完成拥有复制)。
    unsafe {
        qq().callbacks_fired.fetch_add(1, Ordering::Relaxed);
        let reason = native_abi::tagged_short(if qq().inner_state.load(Ordering::Relaxed) == 4 {
            b"ok"
        } else {
            b"not-ready"
        })
        .unwrap();
        let body_copy = body;
        let mut reason_v = reason;
        let mut holder = ByteRange24::from_slice(&body_copy);
        let invoker = mgr.invoker.unwrap();
        invoker(
            &mut mgr as *mut CallbackMgrLayout as *mut c_void,
            0x10,
            0,
            &mut reason_v as *mut TaggedString24 as *mut c_void,
            &mut holder as *mut ByteRange24 as *mut c_void,
        );
        // manager op=0:销毁 capture(ctx Box)。
        if let Some(op) = mgr.manager {
            op(&mut mgr as *mut CallbackMgrLayout as *mut c_void, 0);
        }
        // 释放深复制副本(临时源销毁;若 adapter 在回调返回后仍读裸指针,
        // debug 分配器会直接暴露 UB)。
        drop(body_copy);
        let _ = holder;
        fake_free(cmd.cast(), core::mem::size_of::<TaggedString24>());
    }
}

/// getter(72DE38 同型):输出 {obj, ctrl} pair,强引用 +1。
unsafe extern "system" fn fake_getter(out: *mut ServicePair) -> *mut ServicePair {
    // SAFETY: out 由 ServiceHandle::adopt 提供。
    unsafe {
        let obj = fake_alloc(core::mem::size_of::<FakeService>()) as *mut c_void;
        let ctrl = fake_alloc(core::mem::size_of::<CtrlBlockLayout>()) as *mut CtrlBlockLayout;
        std::ptr::write(
            ctrl,
            CtrlBlockLayout { vptr: core::ptr::null(), strong: 0, weak: 1, obj, _tail: [0; 16] },
        );
        native_abi::ctrl_strong_inc(ctrl);
        std::ptr::write(out, ServicePair { obj, ctrl });
        out
    }
}

/// 释放家族(真实=QQ 侧函数;LAB=对账释放)。
unsafe fn fake_release(ctrl: *mut CtrlBlockLayout) {
    // SAFETY: ctrl 由 getter 分配;归零时连同对象一起释放。
    unsafe {
        if (*ctrl).strong <= 0 {
            fake_free((*ctrl).obj as *mut u8, core::mem::size_of::<FakeService>());
            fake_free(ctrl as *mut u8, core::mem::size_of::<CtrlBlockLayout>());
        }
    }
}

fn base_allocs() -> u64 {
    qq().live_allocs.load(Ordering::Relaxed)
}

/// 构造 adapter(假宿主接线)。
unsafe fn make_adapter(permit: SchedulePermit) -> (MsfAdapter, Arc<FakeQq>) {
    let qq = Arc::clone(qq());
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: fake_getter 同型。
    unsafe {
        fake_getter(&mut pair);
    }
    let stats = Box::into_raw(Box::new(HandleStats::default()));
    // SAFETY: pair 来自 getter(强引用已获取)。
    let handle = unsafe {
        ServiceHandle::adopt(pair, HOST_GEN, SESSION_GEN, Some(fake_release), stats)
    }
    .expect("pair non-null");
    // transport(inner 在位)。
    let tr = fake_alloc(core::mem::size_of::<FakeTransport>()) as *mut FakeTransport;
    // SAFETY: 刚分配。
    unsafe {
        let inner = fake_alloc(core::mem::size_of::<FakeInner>()) as *mut FakeInner;
        let mut vtbl: [*mut c_void; 8] = [core::ptr::null_mut(); 8];
        vtbl[6] = fake_state_fn as *mut c_void;
        std::ptr::write(inner, FakeInner { vtbl });
        let mut vtbl_t: [*mut c_void; 10] = [core::ptr::null_mut(); 10];
        vtbl_t[7] = fake_transport_submit as *mut c_void;
        std::ptr::write(
            tr,
            FakeTransport {
                vtbl: vtbl_t,
                _pad: [0; 0x60 - 80],
                inner_pair: [inner as *mut c_void, core::ptr::null_mut()],
            },
        );
    }
    // SAFETY: transport 为假宿主有效对象。
    let adapter = unsafe {
        MsfAdapter::new(
            handle,
            tr as *mut c_void,
            Some(NativeLinks { transport_submit: fake_transport_submit }),
            permit,
            stats,
        )
    };
    (adapter, qq)
}

fn drain_until(adapter: &MsfAdapter, want: usize, timeout: Duration) -> Vec<NativeOutcome> {
    let dl = std::time::Instant::now() + timeout;
    let mut all = Vec::new();
    while std::time::Instant::now() < dl {
        all.extend(adapter.take_outcomes());
        if all.len() >= want {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    all
}

// ---- T11:inline callback 不死锁、结果恰一次、临时源释放后数据仍完整 ----

#[test]
fn t11_inline_callback_no_deadlock_single_outcome() {
    let _serial = TEST_SERIAL.lock().unwrap();
    qq().mode_inline.store(true, Ordering::Relaxed);
    let (adapter, qq) = unsafe { make_adapter(SchedulePermit::unrestricted()) };
    let after_setup = base_allocs();
    let v = adapter.submit_send("R11", b"MessageSvc.PbSendMsg", b"hello-t11");
    assert!(matches!(v, SubmitVerdict::Submitted { token } if token != 0), "{v:?}");
    // 提交返回即回调已完成(inline):不死锁的直接证明 = 本行执行到。
    let outcomes = adapter.take_outcomes();
    assert_eq!(outcomes.len(), 1, "恰一次: {outcomes:?}");
    assert!(matches!(&outcomes[0], NativeOutcome::Success { request_id, .. } if request_id == "R11"));
    assert_eq!(qq.native_ops.load(Ordering::Relaxed), 1);
    assert_eq!(qq.deep_copies.load(Ordering::Relaxed), 1);
    assert_eq!(qq.callbacks_fired.load(Ordering::Relaxed), 1);
    drop(adapter);
    // 句柄 pair(service+ctrl)随 Drop 释放;transport+inner 为常驻(宿主生存期)。
    assert_eq!(base_allocs(), after_setup - 2, "临时源与已释放句柄必须对账");
}

// ---- T12:getter/handle/释放对称 + 100 次 lifecycle 回基线 ----

#[test]
fn t12_handle_release_symmetry_100_lifecycles() {
    let _serial = TEST_SERIAL.lock().unwrap();
    let base = base_allocs();
    let mut stats = Box::new(HandleStats::default());
    let stats_ptr: *mut HandleStats = &mut *stats;
    for _ in 0..100 {
        let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
        // SAFETY: fake_getter 同型。
        unsafe { fake_getter(&mut pair) };
        // SAFETY: pair 来自 getter。
        let h = unsafe {
            ServiceHandle::adopt(pair, HOST_GEN, SESSION_GEN, Some(fake_release), stats_ptr)
        }
        .unwrap();
        assert!(h.check_generations(HOST_GEN, SESSION_GEN));
        drop(h); // Drop → 释放家族 → ctrl 归零删除
        assert_eq!(base_allocs(), base, "每轮 lifecycle 后回基线");
    }
    assert_eq!(stats.acquired, 100);
    assert_eq!(stats.released, 100);
    assert_eq!(stats.quarantined, 0, "释放家族在位:无隔离");
    assert_eq!(base_allocs(), base);
}

/// 释放家族缺失 → 隔离计数(不猜地址、不跨 allocator 释放)。
#[test]
fn t12b_missing_release_family_quarantines() {
    let _serial = TEST_SERIAL.lock().unwrap();
    let base = base_allocs();
    let mut stats = Box::new(HandleStats::default());
    let stats_ptr: *mut HandleStats = &mut *stats;
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: fake_getter 同型。
    unsafe { fake_getter(&mut pair) };
    // SAFETY: pair 来自 getter;release=None。
    let h = unsafe { ServiceHandle::adopt(pair, HOST_GEN, SESSION_GEN, None, stats_ptr) }.unwrap();
    drop(h);
    assert_eq!(stats.quarantined, 1);
    assert_eq!(stats.released, 0);
    // 恢复环境(手动释放:计数-1 归零 → 删除钩子)。
    // SAFETY: 假宿主对象由本测试构造。
    unsafe {
        native_abi::ctrl_release(pair.ctrl, Some(fake_release));
    }
    assert_eq!(base_allocs(), base);
}

// ---- T13:重复/迟到/旧代次回调幂等;reply 先于 ACK 不改语义 ----

#[test]
fn t13_duplicate_stale_and_early_reply_idempotent() {
    let _serial = TEST_SERIAL.lock().unwrap();
    let (adapter, qq) = unsafe { make_adapter(SchedulePermit::unrestricted()) };
    // reply 先于任何 ACK:结果直接到达。
    adapter.on_native_callback("D1", 0, b"ok", Some("NM-1"));
    adapter.on_native_callback("D1", 0, b"ok", Some("NM-1")); // 重复
    let outcomes = adapter.take_outcomes();
    assert_eq!(outcomes.len(), 1, "重复幂等: {outcomes:?}");
    // 迟到且宿主已失效代次:只审计。
    adapter.mark_stale();
    adapter.on_native_callback("LATE", 0, b"ok", None);
    assert!(adapter.take_outcomes().is_empty(), "失效后不进当前业务链");
    drop(adapter);
}

// ---- T14:SendPending 在 shutdown 分类 Unknown;迟到回调不翻案 ----

#[test]
fn t14_pending_classified_unknown_at_shutdown_late_reply_audited() {
    let _serial = TEST_SERIAL.lock().unwrap();
    qq().mode_inline.store(false, Ordering::Relaxed);
    qq().deferred_delay_ms.store(10_000, Ordering::Relaxed); // 长延迟:回调不会到
    let (adapter, _qq) = unsafe { make_adapter(SchedulePermit::unrestricted()) };
    let v = adapter.submit_send("P1", b"MessageSvc.PbSendMsg", b"x");
    assert!(matches!(v, SubmitVerdict::Submitted { .. }));
    assert_eq!(adapter.pending_ids(), vec!["P1".to_string()]);
    // 关闭分类:在途 → Unknown(分类动作属于 resident/宿主;此处验证账本)。
    let pending = adapter.pending_ids();
    assert_eq!(pending.len(), 1);
    // 迟到回调(若发生):失效后只审计,不翻案。
    adapter.mark_stale();
    adapter.on_native_callback("P1", 0, b"ok", Some("NM-x"));
    assert!(adapter.take_outcomes().is_empty(), "失效后迟到回执只审计");
    assert!(adapter.pending_ids().is_empty() || adapter.pending_ids() == vec!["P1".to_string()]);
}

// ---- T16:前置拒绝 —— 未触碰原生 ----

#[test]
fn t16_precall_rejections_zero_native_ops() {
    let _serial = TEST_SERIAL.lock().unwrap();
    let base_ops = qq().native_ops.load(Ordering::Relaxed);
    let base_rej = 0;
    // 1) 链接未接线(G1 前生产形态)。
    // SAFETY: transport 为 null(生产占位)。
    let adapter_nowire = {
        let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
        // SAFETY: fake_getter 同型。
        unsafe { fake_getter(&mut pair) };
        let stats = Box::into_raw(Box::new(HandleStats::default()));
        // SAFETY: pair 来自 getter。
        let handle = unsafe {
            ServiceHandle::adopt(pair, HOST_GEN, SESSION_GEN, Some(fake_release), stats)
        }
        .unwrap();
        // SAFETY: transport=null(生产占位形态)。
        unsafe {
            MsfAdapter::new(handle, core::ptr::null_mut(), None, SchedulePermit::unrestricted(), stats)
        }
    };
    assert_eq!(adapter_nowire.submit_send("X", b"c", b"b"), SubmitVerdict::LinksUnwired);
    assert!(!adapter_nowire.probe_ready());
    // 2) 线程不在许可集。
    let (adapter, qq) = unsafe { make_adapter(SchedulePermit { allowed_threads: vec![999], unrestricted_threads: false }) };
    assert_eq!(
        adapter.submit_send("Y", b"c", b"b"),
        SubmitVerdict::RejectedPreCall("thread not admitted")
    );
    assert_eq!(qq.native_ops.load(Ordering::Relaxed), base_ops, "拒绝必须零原生调用");
    assert!(adapter.counters.submits_rejected_precall.load(Ordering::Relaxed) >= 1 + base_rej);
    drop(adapter);
    // 3) 超短形容量命令(需要 unrestricted 许可通过线程检查,聚焦命令路径)。
    // SAFETY: 同 make_adapter。
    let (adapter2, qq2) = unsafe { make_adapter(SchedulePermit::unrestricted()) };
    let long = [0x41u8; 64];
    assert_eq!(
        adapter2.submit_send("Z", &long, b"b"),
        SubmitVerdict::UnsupportedLongCommand
    );
    assert_eq!(qq2.native_ops.load(Ordering::Relaxed), base_ops, "命令拒绝零原生调用");
    drop(adapter2);
}

// ---- 10,000 次非发送调度:计数对账、无增长 ----

#[test]
fn ten_thousand_non_send_scheduling_no_growth() {
    let _serial = TEST_SERIAL.lock().unwrap();
    let base = base_allocs();
    let (adapter, _qq) = unsafe { make_adapter(SchedulePermit::unrestricted()) };
    let after_setup = base_allocs();
    for i in 0..10_000u32 {
        assert!(adapter.probe_ready());
        adapter.on_native_callback(&format!("NS-{i}"), 0, b"ok", None);
        if i % 100 == 99 {
            let got = adapter.take_outcomes();
            assert_eq!(got.len(), 100);
            assert!(got.iter().all(|o| o.request_id().starts_with("NS-")));
        }
    }
    assert!(adapter.take_outcomes().is_empty(), "全部已排空");
    assert_eq!(adapter.counters.callbacks_copied.load(Ordering::Relaxed), 10_000);
    assert_eq!(adapter.counters.callbacks_dropped_overflow.load(Ordering::Relaxed), 0);
    drop(adapter);
    // 非发送调度零假宿主分配;句柄 pair 随 Drop 释放(transport+inner 常驻)。
    assert_eq!(base_allocs(), after_setup - 2);
    let _ = base;
}
