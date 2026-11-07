//! Caligo 进程内 bridge:K1 最小加载/握手 probe。
//!
//! 安全边界(计划 §5/§6,K1 阶段):
//! - 本 crate 是项目内允许集中 `unsafe` 的位置之一;K1 阶段**不调用任何 QQ
//!   私有入口、不安装任何钩挂**,只报告自身加载状态与宿主进程模块快照。
//! - `DllMain` 不做实质工作:不创建线程、不等待、不获取 loader lock 之外的
//!   资源;实际探测由自有 loader 通过导出函数 [`caligo_probe_run`] 显式触发。
//! - 探测只读:枚举宿主模块、读自身路径,不写宿主内存、不读取消息数据。
//! - 报告写入由 loader 指定的路径完成;该写入发生在 loader 的远程线程上下文,
//!   不在 QQ 敏感回调内(计划 §3.1:回调内不做 IO 的约束不适用于本阶段,
//!   因为本阶段尚无任何 QQ 回调)。
//!
//! 版本拒绝行为:见 `docs/contracts/version-adapter-manifest.json`;
//! `caligo_bridge_protocol_version` 供 loader 与 manifest/核对握手使用。

use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

/// bridge C ABI 版本。导出函数集合或签名变化时递增。
pub const BRIDGE_ABI_VERSION: u32 = 1;
/// bridge 侧 IPC 协议版本,须与 `caligo-core::ipc::PROTOCOL_VERSION` 一致
/// (由 caligo-cli 的测试交叉断言)。
pub const PROTOCOL_VERSION: u32 = 1;
pub const BRIDGE_BUILD: &str = concat!("caligo-bridge ", env!("CARGO_PKG_VERSION"));

/// `DllMain` 在 DLL_PROCESS_ATTACH 时存入的自身模块句柄,
/// 用于 `GetModuleFileNameW` 取自身路径。
static OWN_INSTANCE: AtomicUsize = AtomicUsize::new(0);

// DLL_PROCESS_ATTACH(未引入 SystemServices feature,常量按 Win32 SDK 固定值)
const DLL_PROCESS_ATTACH: u32 = 1;

/// 探测结果码。
pub mod probe_code {
    /// 报告写入成功。
    pub const OK: u32 = 0;
    /// 报告路径指针无效(空)。
    pub const ERR_NULL_PATH: u32 = 1;
    /// 报告路径不是合法 UTF-16(到 NUL 为止无法构造)。
    pub const ERR_BAD_PATH: u32 = 2;
    /// 报告写入失败(路径不可写/磁盘错误)。
    pub const ERR_WRITE_FAILED: u32 = 3;
}

pub mod asyncrun;
#[cfg(feature = "research")]
pub mod consumer_probe;
#[cfg(feature = "research")]
pub mod daemon_client;
pub mod envrun;
pub mod exec;
pub mod g1;
pub mod g2;
pub mod g3;
pub mod g3_ops;
pub mod g3_session;
pub mod gate;
pub mod host_adapter;
pub mod intr;
pub mod native_abi;
pub mod native_handle;
pub mod native_msf;
pub mod native_receive;
#[cfg(feature = "research")]
pub mod native_resident;
pub mod native_response;
pub mod native_send_sink;
pub mod native_session;
pub mod obs;
#[cfg(feature = "research")]
pub mod qq_entry;
#[cfg(feature = "research")]
pub mod qq_v8;
pub mod register;
pub mod resident;
#[cfg(feature = "research")]
pub mod uid_map_probe;
#[cfg(feature = "research")]
pub mod wirecap;

/// 注册入口结果码。
#[no_mangle]
/// 注册自有 linked binding(EXP-K2-00;详见 register.rs 与方案文档)。
/// 无参导出,由 loader 远程调用。返回 register_code 结果码。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`],不触碰宿主。
pub extern "system" fn caligo_register_entry() -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    register::register_entry()
}

#[no_mangle]
/// 本 DLL 实例是否已执行注册。
pub extern "system" fn caligo_entry_registered() -> u32 {
    u32::from(register::entry_state().0)
}

#[no_mangle]
/// 回调是否已被 Node 调用(0/1)。
pub extern "system" fn caligo_entry_fired() -> u32 {
    u32::from(register::entry_state().1)
}

#[no_mangle]
/// 启动自建 Environment 链路线程(envrun;EXP-K2-01)。立即返回(线程异步执行),
/// 结果以 JSONL 报告文件呈现(每阶段一行,崩溃亦保留已完成阶段)。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`],不触碰宿主。
///
/// # Safety
///
/// `report_path` 必须指向有效的 NUL 结尾 UTF-16 缓冲区(由 loader 写入)。
pub unsafe extern "system" fn caligo_env_start(report_path: *const u16) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    let len = match wide_string_len(report_path) {
        Some(len) => len,
        None => return probe_code::ERR_NULL_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(unsafe { std::slice::from_raw_parts(report_path, len) })
    {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };
    envrun::env_start(&path_str)
}

/// 探测报告。K1 阶段字段只覆盖"已加载、可握手"这一层证据;
/// 真实账号与会话信息必须等入口契约确认后由确认入口提供,不在此伪造。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeReport {
    pub protocol_version: u32,
    pub bridge_abi_version: u32,
    pub bridge_build: String,
    /// bridge 自身在本进程内的加载路径。
    pub bridge_path: String,
    /// 宿主进程 PID。
    pub host_pid: u32,
    /// 宿主进程加载的 Tencent 目录模块与 *.node 模块快照(路径)。
    pub module_snapshot: Vec<String>,
    /// 快照采集是否完整(false = Toolhelp 失败,列表不完整)。
    pub module_snapshot_complete: bool,
}

/// 纯函数:把探测报告序列化为 JSON(K1 握手载体)。单测覆盖。
pub fn build_report_json(report: &ProbeReport) -> serde_json::Result<String> {
    serde_json::to_string_pretty(report)
}

fn wide_string_len(ptr: *const u16) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: 由 loader 约定传入 NUL 结尾的 UTF-16 缓冲区;最多扫描 32 KiB 防失控。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 {
            if *ptr.add(len) == 0 {
                return Some(len);
            }
            len += 1;
        }
        None
    }
}

fn collect_host_snapshot() -> (Vec<String>, bool) {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    };
    // SAFETY: Toolhelp 快照为只读枚举;句柄在所有路径上都会被 CloseHandle 释放。
    unsafe {
        let handle = CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE,
            windows_sys::Win32::System::Threading::GetCurrentProcessId(),
        );
        if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return (Vec::new(), false);
        }
        let mut entries = Vec::new();
        let mut entry: MODULEENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        if Module32FirstW(handle, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExePath
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExePath.len());
                let path = String::from_utf16_lossy(&entry.szExePath[..len]);
                let lowered = path.to_ascii_lowercase();
                if lowered.contains("\\tencent\\") || lowered.ends_with(".node") {
                    entries.push(path);
                }
                if Module32NextW(handle, &mut entry) == 0 {
                    break;
                }
            }
        }
        windows_sys::Win32::Foundation::CloseHandle(handle);
        (entries, true)
    }
}

#[no_mangle]
/// 返回 bridge C ABI 版本,供 loader 在调用其他导出前核对。
pub extern "system" fn caligo_bridge_abi_version() -> u32 {
    BRIDGE_ABI_VERSION
}

#[no_mangle]
/// 返回 IPC 协议版本,供 loader 与 core 期望值核对(握手第一层)。
pub extern "system" fn caligo_bridge_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

#[no_mangle]
/// 构建 probe 报告并写入 `report_path`(UTF-16,NUL 结尾;由 loader 提供)。
///
/// 返回 [`probe_code`] 中的结果码。只读宿主信息;不做其他副作用。
///
/// K4-D0 门控:probe 属旧注入链的远程线程入口,普通构建拒绝。
///
/// # Safety
///
/// `report_path` 必须指向有效的 NUL 结尾 UTF-16 缓冲区(由 loader 写入)。
pub unsafe extern "system" fn caligo_probe_run(report_path: *const u16) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    let len = match wide_string_len(report_path) {
        Some(len) => len,
        None => return probe_code::ERR_NULL_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(
        // SAFETY: 同上,读取范围 [0, len) 均在 loader 写入的缓冲区内。
        unsafe { std::slice::from_raw_parts(report_path, len) },
    ) {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };

    let own = OWN_INSTANCE.load(Ordering::Acquire);
    let bridge_path = if own != 0 {
        // SAFETY: own 是 DllMain 存入的本模块句柄;缓冲区由我们提供并按返回长度截断。
        unsafe {
            let mut buf = [0u16; 1024];
            let n = windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(
                own as _,
                buf.as_mut_ptr(),
                buf.len() as u32,
            ) as usize;
            if n == 0 || n >= buf.len() {
                String::new()
            } else {
                String::from_utf16_lossy(&buf[..n])
            }
        }
    } else {
        String::new()
    };

    let (module_snapshot, module_snapshot_complete) = collect_host_snapshot();
    let report = ProbeReport {
        protocol_version: PROTOCOL_VERSION,
        bridge_abi_version: BRIDGE_ABI_VERSION,
        bridge_build: BRIDGE_BUILD.to_string(),
        bridge_path,
        host_pid: current_pid(),
        module_snapshot,
        module_snapshot_complete,
    };
    match build_report_json(&report) {
        Ok(json) => match std::fs::write(&path_str, json) {
            Ok(()) => probe_code::OK,
            Err(_) => probe_code::ERR_WRITE_FAILED,
        },
        Err(_) => probe_code::ERR_WRITE_FAILED,
    }
}

fn current_pid() -> u32 {
    // SAFETY: GetCurrentProcessId 无副作用。
    unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() }
}

#[no_mangle]
/// 只读运行时观测(EXP-K1-02):解析 QQNT.dll 导出定位 node_module 注册链表头,
/// 遍历输出全部已注册原生模块的 JS 名字。详见 [`obs`] 模块文档。
///
/// 返回 [`probe_code`] 结果码。不调用任何 QQ/Node 函数;唯一写动作是报告文件。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
///
/// # Safety
///
/// `report_path` 必须指向有效的 NUL 结尾 UTF-16 缓冲区(由 loader 写入)。
pub unsafe extern "system" fn caligo_obs_run(report_path: *const u16) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    let len = match wide_string_len(report_path) {
        Some(len) => len,
        None => return probe_code::ERR_NULL_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(unsafe { std::slice::from_raw_parts(report_path, len) })
    {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };
    let report = obs::observe();
    match obs::build_report_json(&report) {
        Ok(json) => match std::fs::write(&path_str, json) {
            Ok(()) => probe_code::OK,
            Err(_) => probe_code::ERR_WRITE_FAILED,
        },
        Err(_) => probe_code::ERR_WRITE_FAILED,
    }
}

#[no_mangle]
/// 只读运行时观测 v2(EXP-F1-R4 修复版):基址由加载器侧经 [`obs::ObsCtx`]
/// 传入,远程线程内**不调用任何加载器锁敏感 API**,只做裸内存读 + 写文件。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`obs::ObsCtx`](结构体布局见 obs.rs):
/// wrapper/qqnt/major 基址来自加载器侧 Toolhelp 快照,report_path 指向
/// 远程已写入的 NUL 结尾 UTF-16 缓冲。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。

/// G1 首次原生调用探针(P6;`g1.rs`):getter 72DE38 + 只读核验,不发送。
/// 上下文/报告纪律同 [`caligo_obs_run2`]。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`g1::G1Ctx`]。
#[no_mangle]
pub unsafe extern "system" fn caligo_g1_probe_run(ctx: *const g1::G1Ctx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { g1::g1_probe_run(ctx) }
}

/// G1 常驻观测(P6 收口;`g1.rs`):租约 + 周期只读探测 + 内部停止链,
/// 有界同步返回(observe_ms 0.5s..120s)。零发送。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`g1::G1NativeCtx`]。
#[no_mangle]
pub unsafe extern "system" fn caligo_g1_native_run(ctx: *const g1::G1NativeCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { g1::g1_native_run(ctx) }
}

/// G2 接收监听(P6;`g2.rs`):Manager+0x170 通知树 begin_node 交换,
/// 有界窗口(路线 B;+0x140 交换为弃用备份)。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`g2::G2Ctx`]。
#[no_mangle]
pub unsafe extern "system" fn caligo_g2_listen_run(ctx: *const g2::G2Ctx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { g2::g2_listen_run(ctx) }
}

/// N3 会话现场验证入口(research 构建;注入纪律与 g2 同)。
///
/// `ctx` 必须指向本进程内有效的 [`g3_session::G3SessionCtx`]。
#[no_mangle]
pub unsafe extern "system" fn caligo_g3_session_run(ctx: *const g3_session::G3SessionCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { g3_session::g3_session_run(ctx) }
}

/// N3 常驻 native 后端入口(research 构建;注入纪律与 g2/g3-session 同)。
/// attach → 泵/接收/daemon-client 三线程 → observe_ms 有界窗口或停止。
///
/// `ctx` 必须指向本进程内有效的
/// [`native_resident::NativeResidentCtx`]。
#[no_mangle]
#[cfg(feature = "research")]
pub unsafe extern "system" fn caligo_native_resident_run(
    ctx: *const native_resident::NativeResidentCtx,
) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { native_resident::native_resident_run(ctx) }
}

/// G3 发送探测 P1(P7;`g3.rs`):transport vtable slot7 捕获交换,
/// 有界窗口,零发送。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`g3::G3Ctx`]。
#[no_mangle]
pub unsafe extern "system" fn caligo_g3_probe_run(ctx: *const g3::G3Ctx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { g3::g3_probe_run(ctx) }
}

/// G3-P1 zero-send probe for the exact QQ 9.9.36-53644 wrapper profile.
/// The caller must have matched the wrapper SHA-256 before dispatch.
#[no_mangle]
pub unsafe extern "system" fn caligo_g3_probe_run_9936(ctx: *const g3::G3Ctx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;CLI 已按 wrapper hash 选择 profile。
    unsafe { g3::g3_probe_run_9936(ctx) }
}

/// G3-P2 受控发送(P7;`g3.rs`):单次调用 SendSSORequestInternal,
/// 零插桩、无常驻修改。发送结果异步,验证 = 对端收到消息。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`g3::G3SendCtx`]。
#[no_mangle]
pub unsafe extern "system" fn caligo_g3_send_run(ctx: *const g3::G3SendCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程。
    unsafe { g3::g3_send_run(ctx) }
}

/// G3-P2 one-shot send for the exact QQ 9.9.36-53644 wrapper profile.
/// Requires the research response consumer; the empty-delegate RVAs are unknown.
#[cfg(feature = "research")]
#[no_mangle]
pub unsafe extern "system" fn caligo_g3_send_run_9936(ctx: *const g3::G3SendCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;CLI 已按 wrapper hash 选择 profile。
    unsafe { g3::g3_send_run_9936(ctx) }
}

pub unsafe extern "system" fn caligo_obs_run2(ctx: *const obs::ObsCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return probe_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;字段按 obs::ObsCtx 布局解读。
    let ctx_view = unsafe { &*ctx };
    let report = obs::observe_ctx(ctx_view);
    let path_ptr = ctx_view.report_path;
    if path_ptr == 0 {
        return probe_code::ERR_NULL_PATH;
    }
    let len = match wide_string_len(path_ptr as *const u16) {
        Some(len) => len,
        None => return probe_code::ERR_BAD_PATH,
    };
    // SAFETY: len 来自 wide_string_len 的 NUL 扫描,切片不越过缓冲区。
    let path_str = match String::from_utf16(unsafe {
        std::slice::from_raw_parts(path_ptr as *const u16, len)
    }) {
        Ok(s) => s,
        Err(_) => return probe_code::ERR_BAD_PATH,
    };
    match obs::build_report_json(&report) {
        Ok(json) => match std::fs::write(&path_str, json) {
            Ok(()) => probe_code::OK,
            Err(_) => probe_code::ERR_WRITE_FAILED,
        },
        Err(_) => probe_code::ERR_WRITE_FAILED,
    }
}

#[no_mangle]
/// K2-02 WU3:对候选 Environment 调用 `node::RequestInterrupt`(详见 intr.rs)。
/// 同步执行:解析导出 → 校验 env → 调用 → 轮询回调 → JSONL 报告。
///
/// 返回 [`intr::intr_code`] 结果码。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`intr::IntrCtx`](布局见 intr.rs):
/// env/expected_vftable/qqnt_base 来自加载器侧解析与 WU2 扫描结果,
/// report_path 指向远程已写入的 NUL 结尾 UTF-16 缓冲。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
pub unsafe extern "system" fn caligo_interrupt_run(ctx: *mut intr::IntrCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return intr::intr_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;字段按 intr::IntrCtx 布局解读。
    let ctx_view = unsafe { &*ctx };
    intr::interrupt_run(ctx_view)
}

#[no_mangle]
/// K2-03:主 Environment 事件循环点载荷(详见 asyncrun.rs 与设计记录)。
/// 同步执行:解析导出 → env 布局链校验 → uv_async_init/send → 轮询 → JSONL。
///
/// 返回 [`asyncrun::async_code`] 结果码。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`asyncrun::AsyncCtx`](布局见 asyncrun.rs):
/// env 来自 K2-02 扫描结果,report_path 指向远程已写入的 NUL 结尾 UTF-16 缓冲。
///
/// K4-D0 门控:普通构建返回 [`gate::ERR_RESEARCH_DISABLED`]。
pub unsafe extern "system" fn caligo_async_run(ctx: *mut asyncrun::AsyncCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return asyncrun::async_code::ERR_NULL_PATH;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;字段按 asyncrun::AsyncCtx 布局解读。
    let ctx_view = unsafe { &*ctx };
    asyncrun::async_run(ctx_view)
}

#[no_mangle]
/// K3-E 通用 JS 执行器(生产 primitive,详见 exec.rs):在主 env 执行调用方
/// 提供的 JS,结果写 JSONL 报告。返回 [`exec::exec_code`] 结果码。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`exec::ExecCtx`](js 缓冲由加载器写入)。
///
/// K4-D0 门控:K3-E 已判负的通用执行器,普通构建返回
/// [`gate::ERR_RESEARCH_DISABLED`](计划 §2.2:文档暂停落实为默认拒绝)。
pub unsafe extern "system" fn caligo_exec_run(ctx: *mut exec::ExecCtx) -> u32 {
    if let Some(code) = gate::reject_legacy() {
        return code;
    }
    if ctx.is_null() {
        return exec::exec_code::ERR_NULL;
    }
    // SAFETY: ctx 由 loader 写入且位于本进程;字段按 exec::ExecCtx 布局解读。
    let ctx_view = unsafe { &*ctx };
    exec::exec_run(ctx_view)
}

// ---------------------------------------------------------------------------
// K4-D7 候选 B 首入导出(仅 research 构建;普通构建不存在这些符号)。
// ctx 布局与 CLI 加载器的写入序列一致(repr(C),8 字节对齐)。
// ---------------------------------------------------------------------------

/// 首入导出结果码(纯常量,两构建皆可引用;执行导出本身仍 research 门控)。
pub mod qq_entry_code {
    pub const OK: u32 = 0;
    pub const ERR_NULL_CTX: u32 = 1;
    pub const ERR_BAD_PATH: u32 = 2;
    pub const ERR_RESOLVE: u32 = 3;
    pub const ERR_ENV_NOT_FRESH: u32 = 0x10;
    pub const ERR_LOOP_CHAIN: u32 = 0x11;
    pub const ERR_NO_CURRENT: u32 = 0x12;
    pub const ERR_HANDLE_SIZE: u32 = 0x13;
    pub const ERR_UV_INIT: u32 = 0x14;
    pub const ERR_TIMEOUT: u32 = 0x15;
    pub const ERR_ALREADY: u32 = 0x16;
    /// 关闭协议:未就绪。
    pub const ERR_SHUTDOWN_NOT_READY: u32 = 0x20;
    /// 关闭协议:超时(句柄保留,不冒充成功)。
    pub const ERR_SHUTDOWN_TIMEOUT: u32 = 0x21;
}

/// 首入参数块(加载器写入;指针字段位于本进程)。
#[cfg(feature = "research")]
#[repr(C)]
pub struct QqEntryCtx {
    pub qqnt_base: usize,
    pub env: usize,
    /// NUL 结尾 UTF-16 报告路径。
    pub report_path: usize,
    pub wait_s: u32,
    pub _pad: u32,
}

/// 候选 B 首入(计划 §7-D7)。每宿主代次一次;重复调用返回 ERR_ALREADY。
///
/// # Safety
///
/// `ctx` 必须指向本进程内有效的 [`QqEntryCtx`](由加载器写入);
/// `qqnt_base`/`env` 须经 loader 侧解析与 envscan 提供。
#[cfg(feature = "research")]
#[no_mangle]
pub unsafe extern "system" fn caligo_qq_entry_bootstrap(ctx: *mut QqEntryCtx) -> u32 {
    use qq_entry::{bootstrap, EntryConfig, EntryError};
    if ctx.is_null() {
        return qq_entry_code::ERR_NULL_CTX;
    }
    // SAFETY: 调用方保证 ctx 有效(repr(C) 布局)。
    let view = unsafe { &*ctx };
    if view.report_path == 0 {
        return qq_entry_code::ERR_BAD_PATH;
    }
    let path = wide_ptr_to_string(view.report_path as *const u16);
    let Some(path) = path else {
        return qq_entry_code::ERR_BAD_PATH;
    };
    let cfg = EntryConfig {
        qqnt_base: view.qqnt_base,
        env: view.env,
        report_path: path,
    };
    // obs::export_addr 只读本进程导出表(resolve 内部自管 unsafe)。
    let symbols = qq_entry::EntrySymbols::resolve(view.qqnt_base, &cfg.report_path);
    let Some(symbols) = symbols else {
        return qq_entry_code::ERR_RESOLVE;
    };
    match bootstrap(&cfg, &symbols) {
        Ok(handle) => {
            // 存档句柄(daemon 导出取用)+ 唤醒面地址。
            // bootstrap 已复制并泄漏进程生存期路径；不要保存 loader 的临时远程缓冲。
            qq_entry::store_entry_handle(handle, qq_entry::wake_trampoline_addr());
            qq_entry_code::OK
        }
        Err(e) => match e {
            EntryError::EnvNotFresh => qq_entry_code::ERR_ENV_NOT_FRESH,
            EntryError::LoopChainUnreadable => qq_entry_code::ERR_LOOP_CHAIN,
            EntryError::InterruptNoCurrentContext => qq_entry_code::ERR_NO_CURRENT,
            EntryError::HandleSizeInvalid { .. } => qq_entry_code::ERR_HANDLE_SIZE,
            EntryError::UvInitFailed { .. } => qq_entry_code::ERR_UV_INIT,
            EntryError::Timeout => qq_entry_code::ERR_TIMEOUT,
            EntryError::AlreadyBootstrapped => qq_entry_code::ERR_ALREADY,
        },
    }
}

/// One-shot QQNT identity-map context. This is research-only, writes only to
/// the supplied local result path, and cannot send messages.
#[cfg(feature = "research")]
#[repr(C)]
pub struct QqUidMapProbeCtx {
    pub uin: u64,
    /// NUL-terminated UTF-16 path under the workspace's ignored local-evidence directory.
    pub output_path: usize,
    pub timeout_ms: u32,
    /// 0 = round-trip resolver query; 1 = GroupService surface only.
    pub mode: u32,
}

/// Run the selected read-only UID-map operation on the owner pump.
///
/// The remote loader thread only queues this fixed operation and waits. V8 and
/// the selected QQNT wrapper methods run from the established K4 owner loop.
/// Round-trip results contain raw identity values; all output stays under
/// `local-evidence/`.
#[cfg(feature = "research")]
#[no_mangle]
pub unsafe extern "system" fn caligo_qq_uid_map_probe(ctx: *mut QqUidMapProbeCtx) -> u32 {
    if ctx.is_null() {
        return uid_map_probe::code::ERR_NOT_BOOTSTRAPPED;
    }
    // SAFETY: loader supplies this fixed repr(C) context in target process memory.
    let view = unsafe { &*ctx };
    let surface_only = match view.mode {
        0 if view.uin > 0 => false,
        1 if view.uin == 0 => true,
        _ => return uid_map_probe::code::ERR_BAD_MODE,
    };
    if view.output_path == 0 {
        return uid_map_probe::code::ERR_REPORT;
    }
    let Some(path) = wide_ptr_to_string(view.output_path as *const u16) else {
        return uid_map_probe::code::ERR_REPORT;
    };
    let output = std::path::PathBuf::from(path);
    if surface_only {
        uid_map_probe::run_surface_only(output, view.timeout_ms)
    } else {
        uid_map_probe::run(view.uin, output, view.timeout_ms)
    }
}

/// 关闭协议(计划 §6.7:owner 线程 uv_close,等待关闭回调)。
/// 超时返回 ERR_SHUTDOWN_TIMEOUT —— 句柄保留,不冒充成功。
#[cfg(feature = "research")]
#[no_mangle]
pub extern "system" fn caligo_qq_entry_shutdown(wait_ms: u32) -> u32 {
    use qq_entry::EntryError;
    // 先停 daemon worker(若已启动):关闭协议不与管道循环竞争。
    if let Some(startup) = qq_entry::peek_daemon_startup() {
        startup
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    match qq_entry::shutdown(wait_ms) {
        Ok(()) => {
            qq_entry::append_stage_simple("shutdown", true, "closed via owner pump");
            qq_entry_code::OK
        }
        Err(EntryError::AlreadyBootstrapped) => qq_entry_code::ERR_SHUTDOWN_NOT_READY,
        Err(EntryError::Timeout) => qq_entry_code::ERR_SHUTDOWN_TIMEOUT,
        Err(_) => qq_entry_code::ERR_SHUTDOWN_TIMEOUT,
    }
}

/// daemon client 启动结果码。
#[cfg(feature = "research")]
pub mod daemon_client_code {
    pub const OK: u32 = 0;
    pub const ERR_NULL_CFG: u32 = 1;
    pub const ERR_BAD_CFG: u32 = 2;
    pub const ERR_NOT_BOOTSTRAPPED: u32 = 3;
    pub const ERR_ALREADY_STARTED: u32 = 4;
    pub const ERR_SPAWN: u32 = 5;
}

/// daemon client 的 JSON 配置(caligo-cli 写入):
/// `{"pipe_name":"...","auth_token":"...","session_generation":1,"account":"...","module_baseline":"..."}`
#[cfg(feature = "research")]
static DAEMON_CFG: std::sync::Mutex<Option<qq_entry::DaemonStartup>> = std::sync::Mutex::new(None);

/// 启动常驻 daemon 客户端(bootstrap 成功后调用;每代次一次)。
/// worker:自有线程,只做管道 I/O + resident 提交(计划 §3.1);
/// drain 钩子接线 owner pump → 结果回传通道。
///
/// 参数:`cfg_json_ptr` 指向 NUL 结尾 UTF-8 JSON(CreateRemoteThread 单参):
/// `{"pipe_name":"...","auth_token":"...","session_generation":1,
///   "account":"...","module_baseline":"...","report_path":"..."}`
/// (report_path 可省;为 daemon/关闭阶段日志路径)。
///
/// # Safety
///
/// `cfg_json_ptr` 须指向本进程内有效 NUL 结尾 UTF-8 缓冲。
#[cfg(feature = "research")]
#[no_mangle]
pub unsafe extern "system" fn caligo_qq_daemon_client_start(cfg_json_ptr: *const u8) -> u32 {
    use daemon_client_code as c;
    use qq_entry::QqOwnerAdapter;
    use resident::{Resident, ResidentLimits};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    if cfg_json_ptr.is_null() {
        return c::ERR_NULL_CFG;
    }
    // SAFETY: 调用方保证 NUL 结尾;上限防失控。
    let mut len = 0usize;
    unsafe {
        while len < 64 * 1024 && *cfg_json_ptr.add(len) != 0 {
            len += 1;
        }
    }
    if len >= 64 * 1024 {
        return c::ERR_BAD_CFG;
    }
    // SAFETY: 范围已界定。
    let raw = unsafe { std::slice::from_raw_parts(cfg_json_ptr, len) };
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(raw);
    let Ok(v) = parsed else {
        return c::ERR_BAD_CFG;
    };
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    let (Some(pipe_name), Some(auth_token), Some(account), Some(module_baseline)) = (
        get("pipe_name"),
        get("auth_token"),
        get("account"),
        get("module_baseline"),
    ) else {
        return c::ERR_BAD_CFG;
    };
    let session_generation = v
        .get("session_generation")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);

    // V8 符号解析(D8 监听器/轮询;每代次一次)。
    let qqnt_base = v
        .get("qqnt_base")
        .and_then(|x| x.as_str())
        .and_then(|h| usize::from_str_radix(h.strip_prefix("0x").unwrap_or(h), 16).ok())
        .unwrap_or(0);
    if qqnt_base != 0 {
        let rp = get("report_path").unwrap_or_default();
        if !crate::qq_v8::store_symbols(qqnt_base, &rp) {
            eprintln!("[qq-daemon] V8 符号解析失败(监听器不可用)");
        }
    }

    // 报告路径登记(shutdown/daemon 阶段日志用;UTF-8 → 宽字符)。
    if let Some(rp) = get("report_path") {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(&rp)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // 进程生存期缓冲(每代次一次;随进程回收,如实记录)。
        let leaked = leaked_wide(wide);
        qq_entry::register_report_path(leaked.as_ptr());
    }

    let mut guard = match DAEMON_CFG.lock() {
        Ok(g) => g,
        Err(_) => return c::ERR_ALREADY_STARTED,
    };
    if guard.is_some() {
        return c::ERR_ALREADY_STARTED;
    }
    let Some(handle) = qq_entry::take_stored_entry_handle() else {
        return c::ERR_NOT_BOOTSTRAPPED;
    };
    let stop = handle.daemon_stop_flag();
    let resident = Arc::new(Resident::new(
        QqOwnerAdapter::new(handle),
        ResidentLimits::default(),
    ));
    let (tx, rx) = std::sync::mpsc::channel::<resident::ResidentNotice>();
    let (etx, erx) = std::sync::mpsc::channel::<resident::OwnedEvent>();
    // resident 初始化必须在 owner 线程执行(计划 §5.1:宿主 API 前的
    // owner 检查不是形式 —— 远程线程调用会被正确拒绝,D7-b 现场实证)。
    // 因此挂在 drain 钩子里,由 owner pump 的首次 tick 完成;失败即
    // Quarantined(写阶段日志,worker 退出)。
    {
        let r = resident.clone();
        qq_entry::install_drain_hook(Box::new(move || {
            // owner 初始化:安静 QQ 的轮转点上下文条件可能暂不满足 —— 每次泵
            // 重试直至成功;首次失败原因写阶段日志(不吞)。
            static OWNER_INIT: AtomicBool = AtomicBool::new(false);
            if !OWNER_INIT.load(Ordering::Relaxed) {
                match r.bootstrap() {
                    Ok(_) => {
                        OWNER_INIT.store(true, Ordering::Relaxed);
                    }
                    Err(e) => {
                        qq_entry::append_stage_simple("owner_init_retry", false, &format!("{e:?}"));
                        return 0;
                    }
                }
            }
            let _ = r.drain();
            for notice in r.take_results() {
                let _ = tx.send(notice);
            }
            // D8 轮询:排空 JS 接收环 → owned 事件注入(每泵一轮;
            // NOT_ARMED/未就绪静默跳过,Err 计数不吞)。
            if OWNER_INIT.load(Ordering::Relaxed) {
                match crate::qq_v8::poll_listener_json() {
                    Ok(json) if !json.starts_with("NOT_ARMED") => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                            // D9:sendResults 回收(fired 的 Promise 结果)。
                            if let Some(sr) = v.get("sendResults").and_then(|x| x.as_object()) {
                                let mut done: Vec<(String, String)> = Vec::new();
                                let mut rejected: Vec<(String, String)> = Vec::new();
                                for (mid, st) in sr {
                                    let status =
                                        st.get("status").and_then(|x| x.as_str()).unwrap_or("");
                                    if status == "done" {
                                        let nid = st
                                            .get("msgId")
                                            .and_then(|x| x.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        done.push((mid.clone(), nid));
                                    } else if status == "rejected" {
                                        let why = st
                                            .get("error")
                                            .and_then(|x| x.as_str())
                                            .unwrap_or("promise rejected")
                                            .to_string();
                                        rejected.push((mid.clone(), why));
                                    }
                                }
                                if !done.is_empty() || !rejected.is_empty() {
                                    r.complete_sends(&done, &rejected);
                                }
                            }
                            if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                                for it in items {
                                    let g = |k: &str| {
                                        it.get(k)
                                            .map(|x| {
                                                if x.is_string() {
                                                    x.as_str().unwrap_or("").to_string()
                                                } else {
                                                    x.to_string()
                                                }
                                            })
                                            .unwrap_or_default()
                                    };
                                    let ev = resident::OwnedEvent {
                                        source: if g("src") == "update" {
                                            resident::EventSourceKind::Update
                                        } else {
                                            resident::EventSourceKind::Recv
                                        },
                                        chat_type: it
                                            .get("chatType")
                                            .and_then(|x| x.as_u64())
                                            .unwrap_or(0)
                                            as u32,
                                        peer_uid: g("peerUid"),
                                        peer_uin: g("peerUin"),
                                        sender_uin: g("senderUin"),
                                        native_id: g("msgId"),
                                        text: g("text"),
                                        msg_time: it.get("msgTime").and_then(|x| x.as_u64()),
                                    };
                                    r.ingest_event(ev);
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // 首次/每 20 次记录轮询失败变体(不吞)。
                        static POLL_ERR_N: std::sync::atomic::AtomicU64 =
                            std::sync::atomic::AtomicU64::new(0);
                        let n = POLL_ERR_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if n % 20 == 0 {
                            qq_entry::append_stage_simple("poll_err", false, &format!("{e:?}"));
                        }
                    }
                }
            }
            for ev in r.take_events() {
                let _ = etx.send(ev); // 转交 worker(此前在此处被吞 —— D8 现场缺陷)
            }
            0
        }));
    }
    let cfg = daemon_client::DaemonClientConfig {
        pipe_name,
        auth_token,
        session_generation,
        account,
        module_baseline,
        ..Default::default()
    };
    let wake = {
        let h = qq_entry::peek_stored_wake();
        move || h.map(|w| w()).unwrap_or(false)
    };
    let worker_stop = stop.clone();
    let spawned = std::thread::Builder::new()
        .name("caligo-daemon-client".into())
        .spawn(move || {
            let (_counters, _exit) =
                daemon_client::run_worker(cfg, resident, &wake, rx, erx, worker_stop);
        });
    if spawned.is_err() {
        return c::ERR_SPAWN;
    }
    *guard = Some(qq_entry::DaemonStartup { stop });
    c::OK
}

/// 宽字符串常驻缓冲(每代次一次登记;随进程回收 —— 单份固定资源,非逐请求泄漏)。
#[cfg(feature = "research")]
fn leaked_wide(mut wide: Vec<u16>) -> &'static [u16] {
    if let Some(pos) = wide.iter().position(|c| *c == 0) {
        wide.truncate(pos + 1);
    }
    Box::leak(wide.into_boxed_slice())
}

/// daemon worker 计数取证(写入 bootstrap 登记的报告 JSONL)。
/// 单参占位(CreateRemoteThread 需要参数;导出不读它)。
#[cfg(feature = "research")]
#[no_mangle]
pub extern "system" fn caligo_qq_daemon_status(_reserved: usize) -> u32 {
    let counters = daemon_client::counters_snapshot();
    qq_entry::append_stage_simple("daemon_counters", true, &counters.as_json());
    daemon_client_code::OK
}

/// 把 NUL 结尾 UTF-16 指针读为 String(上限 32 KiB;失败返回 None)。
#[cfg(feature = "research")]
fn wide_ptr_to_string(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: 加载器写入的 NUL 结尾缓冲;上限防失控。
    unsafe {
        while len < 32 * 1024 {
            if *p.add(len) == 0 {
                break;
            }
            len += 1;
        }
        if len == 32 * 1024 {
            return None;
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)))
    }
}

#[no_mangle]
/// 进程/线程入口。严格最小实现:仅记录自身句柄,不创建线程、不分配、不等待。
pub extern "system" fn DllMain(hinst: isize, reason: u32, _reserved: isize) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        OWN_INSTANCE.store(hinst as usize, Ordering::Release);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_json_contains_handshake_fields() {
        let report = ProbeReport {
            protocol_version: PROTOCOL_VERSION,
            bridge_abi_version: BRIDGE_ABI_VERSION,
            bridge_build: BRIDGE_BUILD.to_string(),
            bridge_path: "C:\\tmp\\caligo_bridge.dll".into(),
            host_pid: 4242,
            module_snapshot: vec!["C:\\t\\QQNT.dll".into()],
            module_snapshot_complete: true,
        };
        let json = build_report_json(&report).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["host_pid"], 4242);
        assert!(back["bridge_build"]
            .as_str()
            .unwrap()
            .starts_with("caligo-bridge"));
        assert_eq!(back["protocol_version"], 1);
        assert_eq!(back["module_snapshot_complete"], true);
        assert_eq!(back["bridge_abi_version"], BRIDGE_ABI_VERSION);
    }

    #[test]
    fn report_serialization_is_deterministic_for_same_input() {
        let report = ProbeReport {
            protocol_version: PROTOCOL_VERSION,
            bridge_abi_version: BRIDGE_ABI_VERSION,
            bridge_build: BRIDGE_BUILD.to_string(),
            bridge_path: "p".into(),
            host_pid: 1,
            module_snapshot: vec![],
            module_snapshot_complete: false,
        };
        assert_eq!(
            build_report_json(&report).unwrap(),
            build_report_json(&report).unwrap()
        );
    }
}
