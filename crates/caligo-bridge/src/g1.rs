//! G1 首次原生调用探针(P6 单实例准入;计划 §7-P6"只接入和观察")。
//!
//! 本模块在被注入的 QQ 进程内执行,做**一次**受控原生调用:
//! 1. 调用服务 getter `72DE38`(guard 保护的线程安全单例初始化;R2 §4/
//!    §7 的调用契约;线程准入即本实验要回答的问题);
//! 2. 只读核验返回 pair:vptr 锚点、引用计数 +1、transport(+0x60)、
//!    内联连接 pair(transport+0x60);
//! 3. **不发送、不注册监听、不释放** —— 释放家族未定证(P6 ABI 冻结项),
//!    按寿命合同以"租约"形式持有到进程退出(capability profile
//!    `gates.attach: admitted-conditional` 的第一次真实接线)。
//!
//! 失败纪律:任何阶段失败即停,写报告正常返回;绝不追加第二次调用。

use core::ffi::c_void;

/// loader 传入的上下文(repr(C);基址来自加载器侧快照)。
#[repr(C)]
pub struct G1Ctx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
}

/// 锚点 RVA(与 capability profile `resolver.anchors` 一致)。
const GETTER_RVA: usize = 0x72DE38;
const ANCHOR_SVC_VTBL_RVA: usize = 0x3F6DED8;
const ANCHOR_TRANSPORT_VTBL_RVA: usize = 0x3EBEA48;
/// MSFService 单例槽(lifetime contract:getter 输出写入处)。
const SVC_OBJ_SLOT: usize = 0x674D2C0;
const SVC_CTRL_SLOT: usize = 0x674D2C8;

pub const OK: u32 = 0;
pub const ERR_NULL_PAIR: u32 = 1;
pub const ERR_ANCHOR_MISMATCH: u32 = 2;
pub const ERR_NO_TRANSPORT: u32 = 3;
pub const ERR_NO_INNER: u32 = 4;
pub const ERR_REPORT_IO: u32 = 0x10;

type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;

/// getter `72DE38` 的输出 pair(lifetime contract §1 几何)。
#[repr(C)]
#[derive(Clone, Copy)]
struct ServicePair {
    obj: *mut c_void,
    ctrl: *mut c_void,
}

/// 阶段日志(qq_entry::append_stage 同格式;进程内安全,单线程调用)。
fn stage(path: *const u16, stage: &str, ok: bool, detail: &str) {
    // SAFETY: path 由 loader 写入,NUL 结尾 UTF-16,进程生存期。
    let text = unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        if len >= 32 * 1024 {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(path, len)))
        }
    };
    if let Some(p) = text {
        append_stage_line(&p, stage, ok, detail);
    }
}

/// JSONL 阶段行(与 qq_entry::append_stage 同格式;独立实现以保持本模块
/// 不依赖 research 门控的 qq_entry —— 普通构建编译通过,运行时由 gate 拒绝)。
fn append_stage_line(report: &str, stage: &str, ok: bool, detail: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let line = format!(
        "{{\"ts\":{},\"tid\":{},\"stage\":\"{}\",\"ok\":{},\"detail\":\"{}\"}}
",
        ts,
        tid,
        stage,
        ok,
        detail.replace("\\", "\\\\").replace('"', "'")
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(report) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn read_u64(addr: usize) -> Option<u64> {
    if addr == 0 || addr % 8 != 0 {
        return None;
    }
    // SAFETY: 单例槽位于 wrapper 数据段;obj/transport 位于 QQ 堆 —— 均为
    // 进程内有效地址(observe-msf 已外部核验);volatile 读无副作用。
    unsafe { Some(core::ptr::read_volatile(addr as *const u64)) }
}

fn read_i32(addr: usize) -> Option<i32> {
    if addr == 0 {
        return None;
    }
    // SAFETY: 同 read_u64。
    unsafe { Some(core::ptr::read_volatile(addr as *const i32)) }
}

fn hex(v: u64) -> String {
    format!("{v:#x}")
}

/// 执行 G1 探针。返回码见常量;每个阶段落一行 JSONL。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`G1Ctx`](loader 写入;wrapper_base 来自
/// 加载器侧快照;report_path 指向远程已写入的 NUL 结尾 UTF-16 缓冲)。
pub unsafe fn g1_probe_run(ctx: *const G1Ctx) -> u32 {
    // SAFETY: ctx 由 loader 写入且位于本进程。
    let (wrapper_base, report_path) = unsafe { ((*ctx).wrapper_base, (*ctx).report_path) };
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };

    // 观测:getter 调用前的单例状态(observe-msf 外部证据的进程内对照)。
    let pre_obj = read_u64(wrapper_base + SVC_OBJ_SLOT).unwrap_or(0);
    let pre_ctrl = read_u64(wrapper_base + SVC_CTRL_SLOT).unwrap_or(0);
    let pre_strong = if pre_ctrl != 0 { read_i32(pre_ctrl as usize + 8).unwrap_or(i32::MIN) } else { 0 };
    stage(report_path, "pre_state", true, &format!("tid={tid} obj={} ctrl={} strong={pre_strong}", hex(pre_obj), hex(pre_ctrl)));

    if pre_obj == 0 || pre_ctrl == 0 {
        stage(report_path, "pre_state", false, "singleton not constructed (login incomplete?)");
        return ERR_NULL_PAIR;
    }

    // —— 受控原生调用 #1:getter(强引用 +1)——
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + GETTER_RVA) };
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: getter 契约(R2 §4):RCX=输出地址,返回同地址;guard 线程安全;
    // 单例已构造,本次调用只做引用递增。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    let called_ok = ret as usize == &mut pair as *mut ServicePair as usize
        && !pair.obj.is_null()
        && !pair.ctrl.is_null();
    stage(report_path, "getter_called", called_ok, &format!("out_obj={} out_ctrl={}", hex(pair.obj as u64), hex(pair.ctrl as u64)));
    if !called_ok {
        return ERR_NULL_PAIR;
    }

    // 核验 1:obj vptr 锚点。
    let vptr = read_u64(pair.obj as usize).unwrap_or(0);
    let vptr_rva = (vptr as usize).wrapping_sub(wrapper_base);
    let anchor_ok = vptr_rva == ANCHOR_SVC_VTBL_RVA;
    stage(report_path, "anchor", anchor_ok, &format!("vptr_rva={vptr_rva:#x} expect={ANCHOR_SVC_VTBL_RVA:#x}"));
    if !anchor_ok {
        return ERR_ANCHOR_MISMATCH;
    }

    // 核验 2:引用计数 +1(getter 契约)。
    let post_strong = read_i32(pair.ctrl as usize + 8).unwrap_or(i32::MIN);
    let inc_ok = post_strong == pre_strong + 1;
    stage(report_path, "refcount", inc_ok, &format!("pre={pre_strong} post={post_strong} (lease: no release; family unpinned)"));

    // 核验 3:transport(+0x60)。
    let transport = read_u64(pair.obj as usize + FIELD_TRANSPORT_OFFSET).unwrap_or(0);
    if transport == 0 {
        stage(report_path, "transport", false, "null (not installed)");
        return ERR_NO_TRANSPORT;
    }
    let t_vptr = read_u64(transport as usize).unwrap_or(0);
    let t_rva = (t_vptr as usize).wrapping_sub(wrapper_base);
    stage(report_path, "transport", true, &format!("addr={} vptr_rva={t_rva:#x} expect={ANCHOR_TRANSPORT_VTBL_RVA:#x}", hex(transport)));

    // 核验 4:内联连接 pair(transport+0x60;1B4E4EC 锁定点)。
    let inner = read_u64(transport as usize + FIELD_TRANSPORT_OFFSET).unwrap_or(0);
    let inner_ok = inner != 0;
    stage(report_path, "inner_pair", inner_ok, &format!("addr={}", hex(inner)));
    if !inner_ok {
        return ERR_NO_INNER;
    }

    stage(report_path, "g1_probe_done", true, "getter admitted on loader thread; no send; lease held");
    OK
}

const FIELD_TRANSPORT_OFFSET: usize = 0x60;

// ---- G1 收口:常驻观测(租约 + 探测循环 + 内部停止链) ----

/// 常驻观测上下文(repr(C);loader 传入)。
#[repr(C)]
pub struct G1NativeCtx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
    /// 观测窗口毫秒(有界;线程在此窗口内周期探测后自停)。
    pub observe_ms: u32,
}

pub const NATIVE_OK: u32 = 0;
pub const NATIVE_ERR_NULL_PAIR: u32 = 11;
pub const NATIVE_ERR_ANCHOR: u32 = 12;
pub const NATIVE_ERR_NO_INNER: u32 = 14;

/// 常驻观测(P6 G1 收口):
/// 1. getter 一次(租约;不重复获取 —— 引用计数零增长可审计);
/// 2. 观测窗口内周期 `probe_ready()`(只读 not-ready 语义,**零发送**),
///    采样 strong 计数(租约稳定性);
/// 3. 内部停止链:窗口结束 → 停止采样 → 汇总(探测数/首末 strong/transport
///    在位)→ 正常返回。全程单线程、有界、可从外部 Wait 观测。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`G1NativeCtx`]。
pub unsafe fn g1_native_run(ctx: *const G1NativeCtx) -> u32 {
    // SAFETY: ctx 由 loader 写入且位于本进程。
    let (wrapper_base, report_path, observe_ms) =
        unsafe { ((*ctx).wrapper_base, (*ctx).report_path, (*ctx).observe_ms) };
    let observe_ms = observe_ms.min(120_000).max(500); // 有界:0.5s..120s

    // —— 一次获取(租约)——
    let pre_ctrl = read_u64(wrapper_base + SVC_CTRL_SLOT).unwrap_or(0);
    let pre_strong = if pre_ctrl != 0 { read_i32(pre_ctrl as usize + 8).unwrap_or(i32::MIN) } else { 0 };
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + GETTER_RVA) };
    let mut pair = ServicePair { obj: core::ptr::null_mut(), ctrl: core::ptr::null_mut() };
    // SAFETY: getter 契约(R2 §4);单例已构造。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
        stage(report_path, "native_adopt", false, "getter failed");
        return NATIVE_ERR_NULL_PAIR;
    }
    let vptr = read_u64(pair.obj as usize).unwrap_or(0);
    if (vptr as usize).wrapping_sub(wrapper_base) != ANCHOR_SVC_VTBL_RVA {
        stage(report_path, "native_adopt", false, &format!("anchor mismatch {vptr:#x}"));
        return NATIVE_ERR_ANCHOR;
    }
    let first_strong = read_i32(pair.ctrl as usize + 8).unwrap_or(i32::MIN);
    stage(
        report_path,
        "native_adopt",
        true,
        &format!("obj={} strong_adapted={first_strong} (lease; no repeated gets)", hex(pair.obj as u64)),
    );

    // —— 观测循环(只读探测 + 计数采样)——
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(observe_ms as u64);
    let mut probes: u64 = 0;
    let mut transport_seen = 0u64;
    let mut inner_seen = 0u64;
    let mut last_strong = first_strong;
    let mut min_strong = first_strong;
    let mut max_strong = first_strong;
    while std::time::Instant::now() < deadline {
        // probe_ready 语义:transport 与内联 pair 在位(只读;零发送)。
        let t = read_u64(pair.obj as usize + FIELD_TRANSPORT_OFFSET).unwrap_or(0);
        let inner = if t != 0 { read_u64(t as usize + FIELD_TRANSPORT_OFFSET).unwrap_or(0) } else { 0 };
        if t != 0 {
            transport_seen += 1;
        }
        if inner != 0 {
            inner_seen += 1;
        }
        if let Some(s) = read_i32(pair.ctrl as usize + 8) {
            last_strong = s;
            min_strong = min_strong.min(s);
            max_strong = max_strong.max(s);
        }
        probes += 1;
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // —— 停止链:汇总 + 干净返回 ——
    let lease_stable = first_strong == read_i32(pair.ctrl as usize + 8).unwrap_or(i32::MIN);
    stage(
        report_path,
        "native_stop",
        true,
        &format!(
            "probes={probes} transport_present={transport_seen} inner_present={inner_seen} strong_first={first_strong} last={last_strong} min={min_strong} max={max_strong} lease_stable={lease_stable} stop=clean"
        ),
    );
    if transport_seen != probes || inner_seen != probes {
        // 观测窗口内 transport/inner 出现过缺失:如实降级返回(仍不算失败 ——
        // QQ 自身会话波动由 G2+ 观察;此处只记录)。
        stage(report_path, "native_stop", false, "transport/inner had gaps in window");
        return NATIVE_ERR_NO_INNER;
    }
    NATIVE_OK
}
