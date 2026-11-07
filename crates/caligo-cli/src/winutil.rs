//! Win32 进程/模块枚举与受门控的加载器(K1 最小实现)。
//!
//! 纪律(对应计划 §5/§8):
//! - 枚举只读:Toolhelp 快照 + GetProcessTimes,不触碰目标进程内存。
//! - 加载器是**本仓库自有的实现**(无第三方 loader、无授权服务、无文件修补);
//!   它把 bridge cdylib 用 `LoadLibraryW` 装入执行者明确指定的测试实例,
//!   然后调用 bridge 的 `caligo_probe_run` 取回探测报告。
//! - 门控在 CLI 层完成:manifest 摘要核对通过 + 执行者显式确认参数,两者缺一不可。
//! - 本模块的 unsafe 注入序列在 K1 尚未经真实验证;首次使用必须按
//!   docs/research/recovery-notes.md §2 的恢复路径执行。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{FILETIME, HANDLE};

pub struct ProcessInfo {
    pub pid: u32,
    pub exe_path: String,
    /// 进程创建时间(UTC,取不到时为 None)。
    pub started_utc: Option<String>,
    pub modules: Vec<String>,
}

pub fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 远端留存缓冲记录(K4-D1 资源账本)。
///
/// 计划 §5.3:`WAIT_TIMEOUT` 表示等待未满足,不等于远程线程已终止——
/// 其仍可能引用远端 ctx/参数缓冲。等待超时(或无法证明线程终止)时,
/// 相关分配**不释放**,登记在此账本,随宿主进程生存期回收;
/// 生产链路(K4 起)不再逐次远程调用,本账本仅服务研究构建的取证。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedRemoteAlloc {
    pub pid: u32,
    pub addr: usize,
    pub bytes: usize,
    pub what: &'static str,
    /// 留存原因("wait-timeout" / "async-worker-outlives-wait")。
    pub reason: &'static str,
}

static RETAINED_REMOTE: std::sync::Mutex<Vec<RetainedRemoteAlloc>> =
    std::sync::Mutex::new(Vec::new());

/// 把远端分配登记进留存账本(不释放)。
fn retain_remote(pid: u32, addr: usize, bytes: usize, what: &'static str, reason: &'static str) {
    if let Ok(mut ledger) = RETAINED_REMOTE.lock() {
        ledger.push(RetainedRemoteAlloc {
            pid,
            addr,
            bytes,
            what,
            reason,
        });
    }
}

/// 留存账本登记(CLI 其他命令的远程缓冲超时面;D0 账本语义复用)。
pub fn retain_remote_pub(
    pid: u32,
    addr: usize,
    bytes: usize,
    what: &'static str,
    reason: &'static str,
) {
    retain_remote(pid, addr, bytes, what, reason);
}

/// 读取留存账本快照(测试/取证用;非测试构建下由测试模块独占使用)。
#[allow(dead_code)]
pub fn retained_remote_snapshot() -> Vec<RetainedRemoteAlloc> {
    RETAINED_REMOTE
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default()
}

fn filetime_to_utc(ft: &FILETIME) -> Option<String> {
    let raw = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    if raw == 0 {
        return None;
    }
    // 1601-01-01 → 1970-01-01 的 100ns 区间数。
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    if raw < EPOCH_DIFF_100NS {
        return None;
    }
    let unix_secs = ((raw - EPOCH_DIFF_100NS) / 10_000_000) as i64;
    let days = unix_secs.div_euclid(86_400);
    let rem = unix_secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z"))
}

fn u16sz(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 枚举名字等于 `image_name`(不区分大小写)的进程及其模块路径。只读。
pub fn list_processes(image_name: &str) -> Result<Vec<ProcessInfo>, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
        MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let want = image_name.to_ascii_lowercase();

    // SAFETY: Toolhelp 快照是只读枚举;每个句柄都在对应块内关闭。
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err("CreateToolhelp32Snapshot(PROCESS) failed".into());
        }
        let mut pids: Vec<(u32, String)> = Vec::new();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let exe = u16sz(&entry.szExeFile);
                if exe.eq_ignore_ascii_case(&want) {
                    pids.push((entry.th32ProcessID, exe));
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);

        let mut out = Vec::new();
        for (pid, exe_path) in pids {
            // 创建时间:查询受限信息权即可,不申请写权限。
            let mut started_utc = None;
            let proc_h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !proc_h.is_null() {
                let mut created: FILETIME = std::mem::zeroed();
                let mut exit_t: FILETIME = std::mem::zeroed();
                let mut kernel_t: FILETIME = std::mem::zeroed();
                let mut user_t: FILETIME = std::mem::zeroed();
                if GetProcessTimes(
                    proc_h,
                    &mut created,
                    &mut exit_t,
                    &mut kernel_t,
                    &mut user_t,
                ) != 0
                {
                    started_utc = filetime_to_utc(&created);
                }
                CloseHandle(proc_h);
            }

            let mut modules = Vec::new();
            let msnap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
            if msnap != INVALID_HANDLE_VALUE {
                let mut ment: MODULEENTRY32W = std::mem::zeroed();
                ment.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
                if Module32FirstW(msnap, &mut ment) != 0 {
                    loop {
                        modules.push(u16sz(&ment.szExePath));
                        if Module32NextW(msnap, &mut ment) == 0 {
                            break;
                        }
                    }
                }
                CloseHandle(msnap);
            }
            out.push(ProcessInfo {
                pid,
                exe_path,
                started_utc,
                modules,
            });
        }
        Ok(out)
    }
}

/// 解析目标进程内模块短名 → 基址(Toolhelp 快照在本进程执行;只读)。
///
/// 供 obs v2 把基址传给远程线程,避免远程线程内调用加载器锁敏感 API。
pub fn module_bases_in(pid: u32) -> Result<std::collections::HashMap<String, usize>, String> {
    Ok(modules_in(pid)?
        .into_iter()
        .map(|m| (m.name, m.base))
        .collect())
}

pub struct ModuleInfo {
    pub name: String,
    pub base: usize,
    pub size: u32,
}

/// 枚举目标进程模块(短名/基址/大小)。Toolhelp 快照在本进程执行;只读。
pub fn modules_in(pid: u32) -> Result<Vec<ModuleInfo>, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    };

    let mut out = Vec::new();
    // SAFETY: Toolhelp 只读快照;句柄在退出前关闭。快照逻辑在本进程执行,
    // 不触碰目标进程的加载器锁。
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
        if snap == INVALID_HANDLE_VALUE {
            return Err("CreateToolhelp32Snapshot(MODULE) failed".into());
        }
        let mut ment: MODULEENTRY32W = std::mem::zeroed();
        ment.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        if Module32FirstW(snap, &mut ment) != 0 {
            loop {
                out.push(ModuleInfo {
                    name: u16sz(&ment.szModule),
                    base: ment.modBaseAddr as usize,
                    size: ment.modBaseSize,
                });
                if Module32NextW(snap, &mut ment) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    Ok(out)
}

pub struct InjectOutcome {
    /// bridge 在目标进程内的基址。
    pub remote_base: usize,
    /// `caligo_probe_run` 的返回码(见 caligo-bridge::probe_code)。
    pub probe_exit_code: u32,
    /// `caligo_obs_run` 的返回码(仅在 obs_report 提供时)。
    pub obs_exit_code: Option<u32>,
    /// `caligo_register_entry` 的返回码(仅在 register_entry 提供时)。
    pub register_exit_code: Option<u32>,
    /// `caligo_g1_probe_run` 的返回码(仅在 g1_report 提供时)。
    pub g1_exit_code: Option<u32>,
    /// `caligo_g1_native_run` 的返回码(仅在 native_run_report 提供时)。
    pub native_exit_code: Option<u32>,
    /// `caligo_g2_listen_run` 的返回码(仅在 g2_run_report 提供时)。
    pub g2_exit_code: Option<u32>,
    /// `caligo_g3_probe_run` 的返回码(仅在 g3_run_report 提供时)。
    pub g3_exit_code: Option<u32>,
    /// `caligo_g3_send_run` 的返回码(仅在 g3_send_report 提供时)。
    pub g3_send_exit_code: Option<u32>,
    /// `caligo_g3_session_run` 的返回码(仅在 g3_session_report 提供时)。
    pub g3_session_exit_code: Option<u32>,
    /// `caligo_env_start` 的返回码(仅在 env_report 提供时;链路线程异步执行)。
    pub env_exit_code: Option<u32>,
    /// `caligo_interrupt_run` 的返回码(仅在 intr 提供时;同步执行)。
    pub intr_exit_code: Option<u32>,
    /// `caligo_async_run` 的返回码(仅在 async 提供时;同步执行)。
    pub async_exit_code: Option<u32>,
    /// `caligo_exec_run` 的返回码(仅在 exec 提供时;同步执行)。
    pub exec_exit_code: Option<u32>,
    /// `caligo_native_resident_run` 的返回码(仅在 native_resident 提供时)。
    pub native_resident_exit_code: Option<u32>,
}

/// N3 常驻 native 后端请求(布局对应 caligo_bridge::native_resident::NativeResidentCtx;
/// 字符串均 NUL 结尾由 loader 侧写入)。
pub struct NativeResidentRequest {
    /// JSONL 报告路径。
    pub report: PathBuf,
    /// daemon bridge 管道全名(前缀 + "-bridge")。
    pub pipe_name: String,
    /// 认证 token(caligod 首行 JSON 输出)。
    pub auth_token: String,
    /// bot uin 十进制串(与 caligod --account 一致)。
    pub account: String,
    /// 模块基线(如 "qq-9.9.33-52230")。
    pub module_baseline: String,
    /// 会话代次。
    pub session_generation: u64,
    /// 逗号分隔群号(kind 判别白名单;空 = 全部 unknown)。
    pub group_codes: String,
    /// 本账号 uid(u_* 形态;可选,空 = direction 一律 incoming)。
    pub self_uid: String,
    /// CPB 差分轮询周期(ms)。
    pub poll_ms: u32,
    /// 研究有界窗口(ms;0 = 直到 daemon 停止)。
    pub observe_ms: u32,
}

/// WU3 RequestInterrupt 实验的请求参数(布局对应 caligo_bridge::intr::IntrCtx)。
pub struct IntrRequest {
    /// 候选 node::Environment*(0 = 干跑)。
    pub env: usize,
    /// 期望 vfptr(qqnt_base + vtable_rva;0 = 跳过核对)。
    pub expected_vftable: usize,
    /// JSONL 报告路径。
    pub report: PathBuf,
    /// 等待回调触发的毫秒数。
    pub wait_ms: u32,
}

/// K3-E 通用 JS 执行请求(布局对应 caligo_bridge::exec::ExecCtx)。
pub struct ExecRequest {
    /// 候选 node::Environment*(0 = 干跑)。
    pub env: usize,
    /// 上下文钉扎 hint(非零优先;来自 start 轮报告)。
    pub ctx_hint: usize,
    /// UTF-8 JS 源码(CLI 侧已读入)。
    pub js: Vec<u8>,
    /// JSONL 报告路径。
    pub report: PathBuf,
    /// 轮询上限毫秒。
    pub wait_ms: u32,
}

/// K2-03 事件循环点载荷请求(布局对应 caligo_bridge::asyncrun::AsyncCtx)。
pub struct AsyncRequest {
    /// 候选 node::Environment*(0 = 干跑)。
    pub env: usize,
    /// 0=干跑 1=原子载荷 2=JS 枚举 3=中断捕获+执行(备用)。
    pub mode: u32,
    /// mode 2 脚本选择:0=指纹 1=load 探针(K2-04)。
    pub script: u32,
    /// JSONL 报告路径。
    pub report: PathBuf,
    /// 等待回调触发的毫秒数。
    pub wait_ms: u32,
    /// K3-F 参数区 UTF-8 JSON(仅 PARAM_SEND 需要;空=无)。
    pub params: Vec<u8>,
}

pub type RemoteThreadFn = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;

/// 把 bridge 装入指定进程并调用其 `caligo_probe_run`(可选:`caligo_obs_run`)。
///
/// 步骤(每步失败即中止,不给"部分成功"):
/// 1. OpenProcess(创建远程线程 + 虚拟内存读写 + 查询权限);
/// 2. VirtualAllocEx + WriteProcessMemory 写入 bridge 路径(UTF-16);
/// 3. CreateRemoteThread 调 kernel32!LoadLibraryW;
/// 4. 在目标模块快照中定位 bridge 基址(x64 上线程退出码会截断,不可靠),
///    用 ReadProcessMemory 解析其远程 PE 导出表,找到 `caligo_probe_run`;
/// 5. 分配报告路径缓冲,CreateRemoteThread 调用 probe;
/// 6. `obs_report` 给出时,再以同样方式调用 `caligo_obs_run`(只读运行时观测);
/// 7. 释放远端内存与句柄。
///
/// 门控(manifest 摘要核对、执行者显式确认)在 CLI 入口完成,不在此重复。
///
/// # Safety
///
/// 仅可对执行者明确指定的测试实例调用;调用方负责目标选择的全部授权。
pub unsafe fn inject_and_probe(
    pid: u32,
    bridge_path: &Path,
    report_path: &Path,
    obs_report: Option<&Path>,
    env_report: Option<&Path>,
    register_entry: bool,
    wait_ms: u32,
    intr: Option<&IntrRequest>,
    async_req: Option<&AsyncRequest>,
    exec_req: Option<&ExecRequest>,
    g1_report: Option<&Path>,
    native_run_report: Option<&Path>,
    native_run_ms: u32,
    g2_run_report: Option<&Path>,
    g2_run_ms: u32,
    g3_run_report: Option<(&Path, u32)>,
    g3_run_ms: u32,
    g3_send: Option<(&Path, u64, &str, Option<&str>, u64, u32, u32, Option<std::path::PathBuf>, Option<(String, u32)>, u32, u32, u32)>, // previous fields + exact wrapper profile id
    g3_session: Option<(&Path, u64, &str, u32)>, // (report, group_code, text, observe_ms)
    native_resident: Option<&NativeResidentRequest>,
) -> Result<InjectOutcome, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, INFINITE,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };

    if !bridge_path.is_file() {
        return Err(format!("bridge not found: {}", bridge_path.display()));
    }
    let bridge_wide = to_wide(&bridge_path.to_string_lossy());
    let report_wide = to_wide(&report_path.to_string_lossy());

    // SAFETY: 文档化的远程加载序列;门控在 CLI 入口完成。
    // 两块远端缓冲用 Option 跟踪,收尾统一释放;句柄在函数出口关闭。
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() {
            return Err(format!("OpenProcess: Win32 error {}", GetLastError()));
        }
        let mut buf_bridge: Option<*mut core::ffi::c_void> = None;
        let mut buf_report: Option<*mut core::ffi::c_void> = None;
        let mut buf_obs: Option<*mut core::ffi::c_void> = None;
        let mut buf_env: Option<*mut core::ffi::c_void> = None;
        let mut buf_intr: Option<*mut core::ffi::c_void> = None;
        let mut buf_async: Option<*mut core::ffi::c_void> = None;
        // K4-D1:任一远程线程等待超时即置位;此后所有远端缓冲转入留存账本,
        // 不再释放(WAIT_TIMEOUT ≠ 线程终止,计划 §5.3)。
        let mut timed_out = false;

        let result = (|| -> Result<InjectOutcome, String> {
            let alloc_and_write = |bytes: &[u16],
                                   step: &str|
             -> Result<*mut core::ffi::c_void, String> {
                let size = bytes.len() * 2;
                let remote = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    size,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote.is_null() {
                    return Err(format!("{step}: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                let ok =
                    WriteProcessMemory(proc_h, remote, bytes.as_ptr().cast(), size, &mut written);
                if ok == 0 || written != size {
                    VirtualFreeEx(proc_h, remote, 0, MEM_RELEASE);
                    return Err(format!(
                        "{step}: WriteProcessMemory incomplete ({written}/{size})"
                    ));
                }
                Ok(remote)
            };
            // 字节版本(G1 ctx 等非 UTF-16 载荷)。
            let alloc_and_write_bytes = |bytes: &[u8],
                                          step: &str|
             -> Result<*mut core::ffi::c_void, String> {
                let remote = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    bytes.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote.is_null() {
                    return Err(format!("{step}: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                let ok =
                    WriteProcessMemory(proc_h, remote, bytes.as_ptr().cast(), bytes.len(), &mut written);
                if ok == 0 || written != bytes.len() {
                    VirtualFreeEx(proc_h, remote, 0, MEM_RELEASE);
                    return Err(format!(
                        "{step}: WriteProcessMemory incomplete ({written}/{})"
                    , bytes.len()));
                }
                Ok(remote)
            };

            let remote_bridge_path = alloc_and_write(&bridge_wide, "alloc bridge path")?;
            buf_bridge = Some(remote_bridge_path);

            let kernel32 = GetModuleHandleW(to_wide("kernel32.dll").as_ptr());
            let load_library_addr: Option<unsafe extern "system" fn(*const u16) -> isize> =
                if kernel32.is_null() {
                    None
                } else {
                    GetProcAddress(kernel32, c"LoadLibraryW".as_ptr() as *const u8).map(|f| {
                        std::mem::transmute::<
                            usize,
                            unsafe extern "system" fn(*const u16) -> isize,
                        >(f as usize)
                    })
                };
            let Some(load_library_addr) = load_library_addr else {
                return Err("kernel32!LoadLibraryW not resolved".into());
            };

            let start_load: RemoteThreadFn =
                std::mem::transmute::<usize, RemoteThreadFn>(load_library_addr as usize);
            let thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start_load),
                remote_bridge_path,
                0,
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                return Err(format!(
                    "CreateRemoteThread(LoadLibraryW): Win32 error {}",
                    GetLastError()
                ));
            }
            let wait = WaitForSingleObject(thread, if wait_ms > 0 { wait_ms } else { INFINITE });
            CloseHandle(thread);
            if wait != 0 {
                // WAIT_OBJECT_0 == 0;超时/放弃都按失败处理(不假设加载完成)。
                // LoadLibraryW 线程可能仍持 bridge 路径缓冲 → 标记留存。
                timed_out = true;
                return Err(format!(
                    "remote LoadLibraryW wait failed (remote buffers retained): code {wait:#x}"
                ));
            }

            // 在目标模块快照中定位 bridge 基址(x64 线程退出码截断,不可靠)。
            let remote_base = {
                // 路径分隔符规范化:执行者可能传正斜杠,快照恒为反斜杠。
                let want = bridge_path.to_string_lossy().replace('/', "\\");
                let msnap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
                if msnap == INVALID_HANDLE_VALUE {
                    return Err("CreateToolhelp32Snapshot(MODULE) failed after load".into());
                }
                let mut found: Option<usize> = None;
                let mut ment: MODULEENTRY32W = std::mem::zeroed();
                ment.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
                if Module32FirstW(msnap, &mut ment) != 0 {
                    loop {
                        let loaded = u16sz(&ment.szExePath).replace('/', "\\");
                        if loaded.eq_ignore_ascii_case(&want) {
                            found = Some(ment.modBaseAddr as usize);
                            break;
                        }
                        if Module32NextW(msnap, &mut ment) == 0 {
                            break;
                        }
                    }
                }
                CloseHandle(msnap);
                found.ok_or_else(|| {
                    "bridge module not found in target snapshot after load".to_string()
                })?
            };

            // 可选:注册自有 linked binding(caligo_register_entry,无参调用)。
            let mut register_exit: Option<u32> = None;
            if register_entry {
                let reg_addr =
                    match read_remote_export(proc_h, remote_base, "caligo_register_entry") {
                        Ok(Some(a)) => a,
                        Ok(None) => return Err("caligo_register_entry not found in remote".into()),
                        Err(e) => return Err(format!("remote export lookup (register): {e}")),
                    };
                let reg_fn: unsafe extern "system" fn() -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn() -> u32>(reg_addr);
                let start_reg: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(reg_fn as usize);
                let reg_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_reg),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                );
                if reg_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(register): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait =
                    WaitForSingleObject(reg_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(reg_thread, &mut code);
                CloseHandle(reg_thread);
                if wait != 0 {
                    return Err(format!(
                        "register remote thread wait failed: code {wait:#x}"
                    ));
                }
                register_exit = Some(code);
            }

            // 解析远程 PE 导出表,找 caligo_probe_run。
            let probe_addr = match read_remote_export(proc_h, remote_base, "caligo_probe_run") {
                Ok(Some(a)) => a,
                Ok(None) => return Err("caligo_probe_run not found in remote export table".into()),
                Err(e) => return Err(format!("remote export lookup: {e}")),
            };

            let remote_report_path = alloc_and_write(&report_wide, "alloc report path")?;
            buf_report = Some(remote_report_path);

            let probe_fn: unsafe extern "system" fn(*const u16) -> u32 = std::mem::transmute::<
                usize,
                unsafe extern "system" fn(*const u16) -> u32,
            >(probe_addr);
            let start_probe: RemoteThreadFn =
                std::mem::transmute::<usize, RemoteThreadFn>(probe_fn as usize);
            let probe_thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start_probe),
                remote_report_path,
                0,
                std::ptr::null_mut(),
            );
            if probe_thread.is_null() {
                return Err(format!(
                    "CreateRemoteThread(probe): Win32 error {}",
                    GetLastError()
                ));
            }
            let wait =
                WaitForSingleObject(probe_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
            let mut exit_code: u32 = u32::MAX;
            GetExitCodeThread(probe_thread, &mut exit_code);
            CloseHandle(probe_thread);
            if wait != 0 {
                timed_out = true;
                return Err(format!(
                    "probe remote thread wait failed (remote buffers retained): code {wait:#x}"
                ));
            }

            // 可选:G1 首次原生调用探针(caligo_g1_probe_run;P6)。ctx =
            // {wrapper_base, report_path},均在远程内存;线程准入即本实验目标。
            let mut g1_exit: Option<u32> = None;
            if let Some(g1_path) = g1_report {
                let g1_addr = match read_remote_export(proc_h, remote_base, "caligo_g1_probe_run") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_g1_probe_run not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (g1): {e}")),
                };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (g1)".into());
                }
                let g1_wide = to_wide(&g1_path.to_string_lossy());
                let remote_g1_path = alloc_and_write(&g1_wide, "alloc g1 report path")?;
                buf_obs = Some(remote_g1_path); // 复用回收登记(进程退出清理)
                // G1Ctx repr(C): {usize, *const u16} = 16 字节。
                let mut ctx_bytes = Vec::with_capacity(16);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_g1_path as usize).to_le_bytes());
                let remote_g1_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc g1 ctx")?;
                buf_report = Some(remote_g1_ctx); // 复用回收登记
                let g1_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(g1_addr);
                let start_g1: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(g1_fn as usize);
                let g1_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_g1),
                    remote_g1_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if g1_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(g1): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait_g1 =
                    WaitForSingleObject(g1_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(g1_thread, &mut code);
                CloseHandle(g1_thread);
                if wait_g1 != 0 {
                    return Err(format!("g1 remote thread wait failed: code {wait_g1:#x}"));
                }
                g1_exit = Some(code);
            }

            // 可选:G1 常驻观测(caligo_g1_native_run;P6 收口):租约 + 周期
            // 只读探测 + 内部停止链,有界同步返回。
            let mut native_exit: Option<u32> = None;
            if let Some(nr_path) = native_run_report {
                let nr_addr =
                    match read_remote_export(proc_h, remote_base, "caligo_g1_native_run") {
                        Ok(Some(a)) => a,
                        Ok(None) => {
                            return Err("caligo_g1_native_run not found in remote".into())
                        }
                        Err(e) => return Err(format!("remote export lookup (native): {e}")),
                    };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (native)".into());
                }
                let nr_wide = to_wide(&nr_path.to_string_lossy());
                let remote_nr_path = alloc_and_write(&nr_wide, "alloc native report path")?;
                buf_obs = Some(remote_nr_path);
                // G1NativeCtx repr(C): {usize, *const u16, u32(+pad)} = 24 字节。
                let mut ctx_bytes = Vec::with_capacity(24);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_nr_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&native_run_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&[0u8; 4]); // 对齐填充
                let remote_nr_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc native ctx")?;
                buf_report = Some(remote_nr_ctx);
                let nr_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(nr_addr);
                let start_nr: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(nr_fn as usize);
                let nr_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_nr),
                    remote_nr_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if nr_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(native): Win32 error {}",
                        GetLastError()
                    ));
                }
                // 观测窗口 + 余量。
                let wait_nr = WaitForSingleObject(
                    nr_thread,
                    if native_run_ms > 0 {
                        native_run_ms.saturating_add(15_000)
                    } else {
                        20_000
                    },
                );
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(nr_thread, &mut code);
                CloseHandle(nr_thread);
                if wait_nr != 0 {
                    return Err(format!(
                        "native remote thread wait failed (window may still be running): code {wait_nr:#x}"
                    ));
                }
                native_exit = Some(code);
            }

            // 可选:G2 接收监听(caligo_g2_listen_run;P6):begin_node 交换(路线 B),
            // 有界窗口同步返回。零发送。
            let mut g2_exit: Option<u32> = None;
            if let Some(g2_path) = g2_run_report {
                let g2_addr = match read_remote_export(proc_h, remote_base, "caligo_g2_listen_run") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_g2_listen_run not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (g2): {e}")),
                };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (g2)".into());
                }
                // 绝对化:远程线程按目标进程 CWD 解析相对路径,必须传全路径。
                let g2_abs = if g2_path.is_absolute() {
                    g2_path.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("current_dir: {e}"))?
                        .join(g2_path)
                };
                let g2_wide = to_wide(&g2_abs.to_string_lossy());
                let remote_g2_path = alloc_and_write(&g2_wide, "alloc g2 report path")?;
                buf_obs = Some(remote_g2_path);
                let mut ctx_bytes = Vec::with_capacity(24);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_g2_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&g2_run_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&[0u8; 4]);
                let remote_g2_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc g2 ctx")?;
                buf_report = Some(remote_g2_ctx);
                let g2_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(g2_addr);
                let start_g2: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(g2_fn as usize);
                let g2_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_g2),
                    remote_g2_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if g2_thread.is_null() {
                    return Err(format!("CreateRemoteThread(g2): Win32 error {}", GetLastError()));
                }
                // 宽限 60s:定位扫描(MEM_PRIVATE 全量)在繁忙实例上可达数十秒。
                let wait_g2 = WaitForSingleObject(
                    g2_thread,
                    if g2_run_ms > 0 { g2_run_ms.saturating_add(60_000) } else { 65_000 },
                );
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(g2_thread, &mut code);
                CloseHandle(g2_thread);
                if wait_g2 != 0 {
                    return Err(format!("g2 remote thread wait failed: code {wait_g2:#x}"));
                }
                g2_exit = Some(code);
            }

            // 可选:G3 发送探测 P1(caligo_g3_probe_run):transport slot7
            // 捕获交换,有界窗口同步返回。零发送。
            let mut g3_exit: Option<u32> = None;
            if let Some((g3_path, g3_profile_id)) = g3_run_report {
                let export = match g3_profile_id {
                    caligo_bridge::g3::G3_PROFILE_ID_9933 => "caligo_g3_probe_run",
                    caligo_bridge::g3::G3_PROFILE_ID_9936 => "caligo_g3_probe_run_9936",
                    _ => return Err(format!("unsupported G3 profile id {g3_profile_id}")),
                };
                let g3_addr = match read_remote_export(proc_h, remote_base, export) {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err(format!("{export} not found in remote")),
                    Err(e) => return Err(format!("remote export lookup (g3): {e}")),
                };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (g3)".into());
                }
                // 绝对化:远程线程按目标进程 CWD 解析相对路径,必须传全路径。
                let g3_abs = if g3_path.is_absolute() {
                    g3_path.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("current_dir: {e}"))?
                        .join(g3_path)
                };
                let g3_wide = to_wide(&g3_abs.to_string_lossy());
                let remote_g3_path = alloc_and_write(&g3_wide, "alloc g3 report path")?;
                buf_obs = Some(remote_g3_path);
                let mut ctx_bytes = Vec::with_capacity(24);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_g3_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&g3_run_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&[0u8; 4]);
                let remote_g3_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc g3 ctx")?;
                buf_report = Some(remote_g3_ctx);
                let g3_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(g3_addr);
                let start_g3: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(g3_fn as usize);
                let g3_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_g3),
                    remote_g3_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if g3_thread.is_null() {
                    return Err(format!("CreateRemoteThread(g3): Win32 error {}", GetLastError()));
                }
                // 宽限 60s:同 g2(adopt/site 读取在繁忙实例上可达数十秒)。
                let wait_g3 = WaitForSingleObject(
                    g3_thread,
                    if g3_run_ms > 0 { g3_run_ms.saturating_add(60_000) } else { 65_000 },
                );
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(g3_thread, &mut code);
                CloseHandle(g3_thread);
                if wait_g3 != 0 {
                    return Err(format!("g3 remote thread wait failed: code {wait_g3:#x}"));
                }
                g3_exit = Some(code);
            }

            // 可选:G3-P2 受控发送(caligo_g3_send_run):单次调用,
            // 零插桩。同步返回(发送本身异步)。
            let mut g3_send_exit: Option<u32> = None;
            if let Some((send_report, peer_uin, text, uid, group_code, seq, rand, _seq_state, at, ch_flag, consumer_flag, g3_profile_id)) = g3_send {
                let (at_uid_s, at_uin_v) = at.clone().unwrap_or_default();
                let export = match g3_profile_id {
                    caligo_bridge::g3::G3_PROFILE_ID_9933 => "caligo_g3_send_run",
                    caligo_bridge::g3::G3_PROFILE_ID_9936 => "caligo_g3_send_run_9936",
                    _ => return Err(format!("unsupported G3 profile id {g3_profile_id}")),
                };
                let send_addr = match read_remote_export(proc_h, remote_base, export) {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err(format!("{export} not found in remote")),
                    Err(e) => return Err(format!("remote export lookup (g3s): {e}")),
                };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (g3s)".into());
                }
                let send_abs = if send_report.is_absolute() {
                    send_report.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("current_dir: {e}"))?
                        .join(send_report)
                };
                let send_wide = to_wide(&send_abs.to_string_lossy());
                let remote_send_path = alloc_and_write(&send_wide, "alloc g3s report path")?;
                buf_obs = Some(remote_send_path);
                let mut text_u16: Vec<u16> = text.encode_utf16().collect();
                text_u16.push(0);
                let remote_text = alloc_and_write(&text_u16, "alloc g3s text")?;
                buf_report = Some(remote_text);
                let uid_u16: Vec<u16> = match uid {
                    Some(u) => {
                        let mut v = u.encode_utf16().collect::<Vec<u16>>();
                        v.push(0);
                        v
                    }
                    None => vec![0],
                };
                let remote_uid = alloc_and_write(&uid_u16, "alloc g3s uid")?;
                buf_obs = Some(remote_uid);
                let mut at_u8: Vec<u8> = if at_uid_s.is_empty() {
                    vec![0]
                } else {
                    let mut v = at_uid_s.into_bytes();
                    v.push(0);
                    v
                };
                let remote_at_uid = alloc_and_write_bytes(&at_u8, "alloc g3s at_uid")?;
                buf_obs = Some(remote_at_uid);
                let mut ctx_bytes = Vec::with_capacity(80);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_send_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&peer_uin.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_text as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_uid as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&group_code.to_le_bytes());
                ctx_bytes.extend_from_slice(&seq.to_le_bytes());
                ctx_bytes.extend_from_slice(&rand.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_at_uid as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&at_uin_v.to_le_bytes());
                // use_ch(N3R27 形状二分;0 = legacy 无 CH)+ use_consumer
                // (N3R30 消费者实验)+ 尾部对齐。
                ctx_bytes.extend_from_slice(&ch_flag.to_le_bytes());
                ctx_bytes.extend_from_slice(&consumer_flag.to_le_bytes());
                ctx_bytes.extend_from_slice(&[0u8; 4]);
                let remote_send_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc g3s ctx")?;
                buf_obs = Some(remote_send_ctx);
                let send_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(send_addr);
                let start_send: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(send_fn as usize);
                let send_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_send),
                    remote_send_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if send_thread.is_null() {
                    return Err(format!("CreateRemoteThread(g3s): Win32 error {}", GetLastError()));
                }
                let wait_send = WaitForSingleObject(send_thread, 30_000);
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(send_thread, &mut code);
                CloseHandle(send_thread);
                if wait_send != 0 {
                    return Err(format!("g3s remote thread wait failed: code {wait_send:#x}"));
                }
                g3_send_exit = Some(code);
            }

            // 可选:N3 会话现场验证(caligo_g3_session_run):attach → CPB 基线 →
            // 单条发送(已证形状)→ 等待 → fresh 差分 → 全字段报告。有界同步。
            let mut g3_session_exit: Option<u32> = None;
            if let Some((sess_report, group_code, sess_text, observe_ms)) = g3_session {
                let sess_addr =
                    match read_remote_export(proc_h, remote_base, "caligo_g3_session_run") {
                        Ok(Some(a)) => a,
                        Ok(None) => return Err("caligo_g3_session_run not found in remote".into()),
                        Err(e) => return Err(format!("remote export lookup (g3-session): {e}")),
                    };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (g3-session)".into());
                }
                // 绝对化:远程线程按目标进程 CWD 解析相对路径,必须传全路径。
                let sess_abs = if sess_report.is_absolute() {
                    sess_report.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("current_dir: {e}"))?
                        .join(sess_report)
                };
                let sess_wide = to_wide(&sess_abs.to_string_lossy());
                let remote_sess_path = alloc_and_write(&sess_wide, "alloc g3-session report path")?;
                buf_obs = Some(remote_sess_path);
                let mut text_u16: Vec<u16> = sess_text.encode_utf16().collect();
                text_u16.push(0);
                let remote_sess_text = alloc_and_write(&text_u16, "alloc g3-session text")?;
                buf_report = Some(remote_sess_text);
                // G3SessionCtx:wrapper_base u64 / report_path / group_code u64 /
                // text_ptr / observe_ms u32(+尾部对齐填充)。
                let mut ctx_bytes = Vec::with_capacity(40);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_sess_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&group_code.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_sess_text as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&observe_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&[0u8; 4]);
                let remote_sess_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc g3-session ctx")?;
                buf_obs = Some(remote_sess_ctx);
                let sess_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(sess_addr);
                let start_sess: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(sess_fn as usize);
                let sess_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_sess),
                    remote_sess_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if sess_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(g3-session): Win32 error {}",
                        GetLastError()
                    ));
                }
                // 宽限 60s:CPB 基线/差分扫描在繁忙实例上可达数十秒(g2 同款)。
                let wait_sess = WaitForSingleObject(
                    sess_thread,
                    observe_ms.saturating_add(60_000).max(65_000),
                );
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(sess_thread, &mut code);
                CloseHandle(sess_thread);
                if wait_sess != 0 {
                    return Err(format!(
                        "g3-session remote thread wait failed: code {wait_sess:#x}"
                    ));
                }
                g3_session_exit = Some(code);
            }

            // 可选:N3 常驻 native 后端(caligo_native_resident_run):
            // attach → 泵/接收/daemon-client 三线程 → observe_ms 有界窗口。
            let mut native_resident_exit: Option<u32> = None;
            if let Some(nr) = native_resident {
                let nr_addr =
                    match read_remote_export(proc_h, remote_base, "caligo_native_resident_run") {
                        Ok(Some(a)) => a,
                        Ok(None) => {
                            return Err("caligo_native_resident_run not found in remote".into())
                        }
                        Err(e) => return Err(format!("remote export lookup (n3res): {e}")),
                    };
                let wrapper_base = *module_bases_in(pid)?.get("wrapper.node").unwrap_or(&0);
                if wrapper_base == 0 {
                    return Err("wrapper.node base unresolved in target (n3res)".into());
                }
                // 绝对化:远程线程按目标进程 CWD 解析相对路径,必须传全路径。
                let nr_abs = if nr.report.is_absolute() {
                    nr.report.to_path_buf()
                } else {
                    std::env::current_dir()
                        .map_err(|e| format!("current_dir: {e}"))?
                        .join(&nr.report)
                };
                let nr_wide = to_wide(&nr_abs.to_string_lossy());
                let remote_nr_path = alloc_and_write(&nr_wide, "alloc n3res report path")?;
                buf_obs = Some(remote_nr_path);
                let pipe_wide: Vec<u16> = nr.pipe_name.encode_utf16().chain(std::iter::once(0)).collect();
                let remote_nr_pipe = alloc_and_write(&pipe_wide, "alloc n3res pipe")?;
                buf_obs = Some(remote_nr_pipe);
                // UTF-8 NUL 结尾串(token/account/baseline/groups/self_uid)。
                let alloc_u8 = |label: &'static str, s: &str| -> Result<*mut core::ffi::c_void, String> {
                    let mut v = s.as_bytes().to_vec();
                    v.push(0);
                    alloc_and_write_bytes(&v, label)
                };
                let remote_nr_token = alloc_u8("alloc n3res token", &nr.auth_token)?;
                buf_obs = Some(remote_nr_token);
                let remote_nr_account = alloc_u8("alloc n3res account", &nr.account)?;
                buf_obs = Some(remote_nr_account);
                let remote_nr_baseline = alloc_u8("alloc n3res baseline", &nr.module_baseline)?;
                buf_obs = Some(remote_nr_baseline);
                let remote_nr_groups = alloc_u8("alloc n3res groups", &nr.group_codes)?;
                buf_obs = Some(remote_nr_groups);
                let remote_nr_selfuid = alloc_u8("alloc n3res self_uid", &nr.self_uid)?;
                buf_obs = Some(remote_nr_selfuid);
                // NativeResidentCtx:80B(见 native_resident.rs ctx 布局)。
                let mut ctx_bytes = Vec::with_capacity(80);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes()); // +0x00
                ctx_bytes.extend_from_slice(&(remote_nr_path as usize).to_le_bytes()); // +0x08
                ctx_bytes.extend_from_slice(&(remote_nr_pipe as usize).to_le_bytes()); // +0x10
                ctx_bytes.extend_from_slice(&(remote_nr_token as usize).to_le_bytes()); // +0x18
                ctx_bytes.extend_from_slice(&(remote_nr_account as usize).to_le_bytes()); // +0x20
                ctx_bytes.extend_from_slice(&(remote_nr_baseline as usize).to_le_bytes()); // +0x28
                ctx_bytes.extend_from_slice(&nr.session_generation.to_le_bytes()); // +0x30
                ctx_bytes.extend_from_slice(&(remote_nr_groups as usize).to_le_bytes()); // +0x38
                ctx_bytes.extend_from_slice(&(remote_nr_selfuid as usize).to_le_bytes()); // +0x40
                ctx_bytes.extend_from_slice(&nr.poll_ms.to_le_bytes()); // +0x48
                ctx_bytes.extend_from_slice(&nr.observe_ms.to_le_bytes()); // +0x4c
                let remote_nr_ctx = alloc_and_write_bytes(&ctx_bytes, "alloc n3res ctx")?;
                buf_report = Some(remote_nr_ctx);
                let nr_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const core::ffi::c_void) -> u32>(nr_addr);
                let start_nr: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(nr_fn as usize);
                let nr_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_nr),
                    remote_nr_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if nr_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(n3res): Win32 error {}",
                        GetLastError()
                    ));
                }
                // 宽限 120s:窗口关闭后还有三轮汇合(泵/接收/daemon worker)。
                let wait_nr = WaitForSingleObject(
                    nr_thread,
                    nr.observe_ms.saturating_add(120_000).max(130_000),
                );
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(nr_thread, &mut code);
                CloseHandle(nr_thread);
                if wait_nr != 0 {
                    return Err(format!(
                        "n3res remote thread wait failed: code {wait_nr:#x}"
                    ));
                }
                native_resident_exit = Some(code);
            }

            // 可选:只读运行时观测 v2(caligo_obs_run2;基址由本加载器进程解析,
            // 远程线程不再调用 GetModuleHandleW/Toolhelp —— EXP-F1-R4 缺陷修复)。
            let mut obs_exit: Option<u32> = None;
            if let Some(obs_path) = obs_report {
                let obs_wide = to_wide(&obs_path.to_string_lossy());
                let obs_addr = match read_remote_export(proc_h, remote_base, "caligo_obs_run2") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_obs_run2 not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (obs): {e}")),
                };
                // 加载器侧解析目标进程内关键模块基址(快照在本进程执行,不碰目标锁)。
                let bases = module_bases_in(pid)?;
                let wrapper_base = *bases.get("wrapper.node").unwrap_or(&0);
                let qqnt_base = *bases.get("QQNT.dll").unwrap_or(&0);
                let major_base = *bases.get("major.node").unwrap_or(&0);
                if wrapper_base == 0 || qqnt_base == 0 {
                    return Err(format!(
                        "module bases unresolved in target: wrapper={wrapper_base:#x} qqnt={qqnt_base:#x}"
                    ));
                }
                let remote_obs_path = alloc_and_write(&obs_wide, "alloc obs report path")?;
                buf_obs = Some(remote_obs_path);
                // caligo_bridge::obs::ObsCtx 的进程内副本(#[repr(C)]:4 个 usize)。
                let mut ctx_bytes = Vec::with_capacity(32);
                ctx_bytes.extend_from_slice(&wrapper_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&qqnt_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&major_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_obs_path as usize).to_le_bytes());
                let remote_ctx = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    ctx_bytes.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote_ctx.is_null() {
                    return Err(format!("alloc obs ctx: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                if WriteProcessMemory(
                    proc_h,
                    remote_ctx,
                    ctx_bytes.as_ptr().cast(),
                    ctx_bytes.len(),
                    &mut written,
                ) == 0
                    || written != ctx_bytes.len()
                {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    return Err("alloc obs ctx: WriteProcessMemory incomplete".into());
                }
                let obs_fn: unsafe extern "system" fn(*const core::ffi::c_void) -> u32 =
                    std::mem::transmute::<
                        usize,
                        unsafe extern "system" fn(*const core::ffi::c_void) -> u32,
                    >(obs_addr);
                let start_obs: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(obs_fn as usize);
                let obs_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_obs),
                    remote_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if obs_thread.is_null() {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    return Err(format!(
                        "CreateRemoteThread(obs): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait =
                    WaitForSingleObject(obs_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(obs_thread, &mut code);
                CloseHandle(obs_thread);
                if wait != 0 {
                    // obs ctx 引用 obs 报告路径缓冲;线程未证实终止 → 双双留存。
                    timed_out = true;
                    retain_remote(
                        pid,
                        remote_ctx as usize,
                        ctx_bytes.len(),
                        "obs ctx",
                        "wait-timeout",
                    );
                    return Err(format!(
                        "obs remote thread wait failed (remote buffers retained): code {wait:#x}"
                    ));
                }
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                obs_exit = Some(code);
            }

            // 可选:自建 Environment 链路(caligo_env_start;执行线程异步运行,
            // 阶段结果增量写入 env 报告文件——中途崩溃亦保留已完成阶段)。
            let mut env_exit: Option<u32> = None;
            if let Some(env_path) = env_report {
                let env_wide = to_wide(&env_path.to_string_lossy());
                let env_addr = match read_remote_export(proc_h, remote_base, "caligo_env_start") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_env_start not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (env): {e}")),
                };
                let remote_env_path = alloc_and_write(&env_wide, "alloc env report path")?;
                buf_env = Some(remote_env_path);
                let env_fn: unsafe extern "system" fn(*const u16) -> u32 =
                    std::mem::transmute::<usize, unsafe extern "system" fn(*const u16) -> u32>(
                        env_addr,
                    );
                let start_env: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(env_fn as usize);
                let env_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_env),
                    remote_env_path,
                    0,
                    std::ptr::null_mut(),
                );
                if env_thread.is_null() {
                    return Err(format!(
                        "CreateRemoteThread(env): Win32 error {}",
                        GetLastError()
                    ));
                }
                let wait =
                    WaitForSingleObject(env_thread, if wait_ms > 0 { wait_ms } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(env_thread, &mut code);
                CloseHandle(env_thread);
                // env 链路线程内部再派生异步 worker,远端线程返回≠worker 结束:
                // 报告路径缓冲 worker 仍可能引用,一律留存登记(不释放)。
                if let Some(p) = buf_env.take() {
                    retain_remote(
                        pid,
                        p as usize,
                        env_wide.len() * 2,
                        "env report path",
                        "async-worker-outlives-wait",
                    );
                }
                if wait != 0 {
                    timed_out = true;
                    return Err(format!(
                        "env remote thread wait failed (remote buffers retained): code {wait:#x}"
                    ));
                }
                env_exit = Some(code);
            }

            // 可选:WU3 RequestInterrupt 实验(caligo_interrupt_run;同步执行,
            // ctx 含 env/期望 vfptr/QQNT 基址;env=0 为干跑)。回调侧只有原子写,
            // 本侧只负责写路径与 ctx 缓冲的远端分配。
            let mut intr_exit: Option<u32> = None;
            if let Some(intr) = intr {
                let intr_addr = match read_remote_export(proc_h, remote_base, "caligo_interrupt_run")
                {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_interrupt_run not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (interrupt): {e}")),
                };
                let bases = module_bases_in(pid)?;
                let qqnt_base = *bases.get("QQNT.dll").unwrap_or(&0);
                if qqnt_base == 0 {
                    return Err("QQNT.dll base unresolved in target (interrupt ctx)".into());
                }
                let intr_wide = to_wide(&intr.report.to_string_lossy());
                let remote_intr_path = alloc_and_write(&intr_wide, "alloc intr report path")?;
                buf_intr = Some(remote_intr_path);
                // caligo_bridge::intr::IntrCtx 的进程内副本(6 字段 ×8B)。
                let mut ctx_bytes: Vec<u8> = Vec::with_capacity(48);
                ctx_bytes.extend_from_slice(&intr.env.to_le_bytes());
                ctx_bytes.extend_from_slice(&intr.expected_vftable.to_le_bytes());
                ctx_bytes.extend_from_slice(&qqnt_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_intr_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&intr.wait_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&0u32.to_le_bytes());
                let remote_ctx = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    ctx_bytes.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote_ctx.is_null() {
                    return Err(format!("alloc intr ctx: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                if WriteProcessMemory(
                    proc_h,
                    remote_ctx,
                    ctx_bytes.as_ptr().cast(),
                    ctx_bytes.len(),
                    &mut written,
                ) == 0
                    || written != ctx_bytes.len()
                {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    return Err("alloc intr ctx: WriteProcessMemory incomplete".into());
                }
                let intr_fn: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 =
                    std::mem::transmute::<
                        usize,
                        unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
                    >(intr_addr);
                let start_intr: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(intr_fn as usize);
                let intr_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_intr),
                    remote_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if intr_thread.is_null() {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    return Err(format!(
                        "CreateRemoteThread(interrupt): Win32 error {}",
                        GetLastError()
                    ));
                }
                // 远程线程内部自带 wait_ms 轮询;本侧等待再加 5s 余量。
                let intr_wait = intr.wait_ms.saturating_add(5000).max(wait_ms);
                let wait = WaitForSingleObject(intr_thread, if intr_wait > 0 { intr_wait } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(intr_thread, &mut code);
                CloseHandle(intr_thread);
                if wait != 0 {
                    // RequestInterrupt 回调可能在 JS 空闲时挂起数分钟(K2-03 记录):
                    // 超时绝不证明回调链结束 → ctx 留存。
                    timed_out = true;
                    retain_remote(
                        pid,
                        remote_ctx as usize,
                        ctx_bytes.len(),
                        "intr ctx",
                        "wait-timeout",
                    );
                    return Err(format!(
                        "interrupt remote thread wait failed (remote buffers retained): code {wait:#x}"
                    ));
                }
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                intr_exit = Some(code);
            }

            // 可选:K3-E 通用 JS 执行(caligo_exec_run;JS 缓冲 + ctx 远端写入)。
            let mut exec_exit: Option<u32> = None;
            if let Some(ex) = exec_req {
                let addr = match read_remote_export(proc_h, remote_base, "caligo_exec_run") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_exec_run not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (exec): {e}")),
                };
                // 写 JS 缓冲(执行期间必须保持有效;线程 Wait 结束后才释放)。
                let remote_js = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    ex.js.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote_js.is_null() {
                    return Err(format!("alloc exec js: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                if WriteProcessMemory(proc_h, remote_js, ex.js.as_ptr().cast(), ex.js.len(), &mut written) == 0
                    || written != ex.js.len()
                {
                    VirtualFreeEx(proc_h, remote_js, 0, MEM_RELEASE);
                    return Err("alloc exec js: WriteProcessMemory incomplete".into());
                }
                let rep_wide = to_wide(&ex.report.to_string_lossy());
                let remote_rep = alloc_and_write(&rep_wide, "alloc exec report path")?;
                // caligo_bridge::exec::ExecCtx 副本(env, qqnt_base, ctx_hint, js,
                // js_len, report, wait, pad)。
                let mut ctx_bytes: Vec<u8> = Vec::with_capacity(56);
                let qqnt_base = *module_bases_in(pid)?.get("QQNT.dll").unwrap_or(&0);
                ctx_bytes.extend_from_slice(&ex.env.to_le_bytes());
                ctx_bytes.extend_from_slice(&qqnt_base.to_le_bytes());
                ctx_bytes.extend_from_slice(&ex.ctx_hint.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_js as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&ex.js.len().to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_rep as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&ex.wait_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&0u32.to_le_bytes());
                let remote_ctx = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    ctx_bytes.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote_ctx.is_null() {
                    return Err(format!("alloc exec ctx: Win32 error {}", GetLastError()));
                }
                let mut w2: usize = 0;
                if WriteProcessMemory(proc_h, remote_ctx, ctx_bytes.as_ptr().cast(), ctx_bytes.len(), &mut w2) == 0
                    || w2 != ctx_bytes.len()
                {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    VirtualFreeEx(proc_h, remote_js, 0, MEM_RELEASE);
                    return Err("alloc exec ctx: WriteProcessMemory incomplete".into());
                }
                let exec_fn: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 =
                    std::mem::transmute::<
                        usize,
                        unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
                    >(addr);
                let start_exec: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(exec_fn as usize);
                let exec_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_exec),
                    remote_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if exec_thread.is_null() {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    VirtualFreeEx(proc_h, remote_js, 0, MEM_RELEASE);
                    return Err(format!("CreateRemoteThread(exec): Win32 error {}", GetLastError()));
                }
                let wait = WaitForSingleObject(exec_thread, if ex.wait_ms.saturating_add(8000) > 0 { ex.wait_ms.saturating_add(8000) } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(exec_thread, &mut code);
                CloseHandle(exec_thread);
                if wait != 0 {
                    // exec 回调(uv_async)可能尚未执行;ctx/js/report 缓冲均被引用 → 留存。
                    timed_out = true;
                    retain_remote(pid, remote_ctx as usize, ctx_bytes.len(), "exec ctx", "wait-timeout");
                    retain_remote(pid, remote_js as usize, ex.js.len(), "exec js", "wait-timeout");
                    retain_remote(pid, remote_rep as usize, rep_wide.len() * 2, "exec report path", "wait-timeout");
                    return Err(format!(
                        "exec remote thread wait failed (remote buffers retained): code {wait:#x}"
                    ));
                }
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                VirtualFreeEx(proc_h, remote_rep, 0, MEM_RELEASE);
                VirtualFreeEx(proc_h, remote_js, 0, MEM_RELEASE);
                exec_exit = Some(code);
            }

            // 可选:K2-03 事件循环点载荷(caligo_async_run;同步执行,mode 0/1/2)。
            let mut async_exit: Option<u32> = None;
            if let Some(req) = async_req {
                let addr = match read_remote_export(proc_h, remote_base, "caligo_async_run") {
                    Ok(Some(a)) => a,
                    Ok(None) => return Err("caligo_async_run not found in remote".into()),
                    Err(e) => return Err(format!("remote export lookup (async): {e}")),
                };
                let req_wide = to_wide(&req.report.to_string_lossy());
                let remote_req_path = alloc_and_write(&req_wide, "alloc async report path")?;
                buf_async = Some(remote_req_path);
                // K3-F 参数区(可选;PARAM_SEND 需要)。
                let mut remote_params: Option<*mut core::ffi::c_void> = None;
                if !req.params.is_empty() {
                    let rp = VirtualAllocEx(
                        proc_h,
                        std::ptr::null(),
                        req.params.len(),
                        MEM_COMMIT | MEM_RESERVE,
                        PAGE_READWRITE,
                    );
                    if rp.is_null() {
                        return Err(format!("alloc async params: Win32 error {}", GetLastError()));
                    }
                    let mut wp: usize = 0;
                    if WriteProcessMemory(proc_h, rp, req.params.as_ptr().cast(), req.params.len(), &mut wp) == 0
                        || wp != req.params.len()
                    {
                        VirtualFreeEx(proc_h, rp, 0, MEM_RELEASE);
                        return Err("alloc async params: WriteProcessMemory incomplete".into());
                    }
                    remote_params = Some(rp);
                }
                // caligo_bridge::asyncrun::AsyncCtx 的进程内副本(env,mode,script,
                // report_path,wait_ms,pad,params_ptr,params_len,pad2)。
                let mut ctx_bytes: Vec<u8> = Vec::with_capacity(56);
                ctx_bytes.extend_from_slice(&req.env.to_le_bytes());
                ctx_bytes.extend_from_slice(&req.mode.to_le_bytes());
                ctx_bytes.extend_from_slice(&req.script.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_req_path as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&req.wait_ms.to_le_bytes());
                ctx_bytes.extend_from_slice(&0u32.to_le_bytes());
                ctx_bytes.extend_from_slice(&(remote_params.unwrap_or(std::ptr::null_mut()) as usize).to_le_bytes());
                ctx_bytes.extend_from_slice(&(req.params.len() as u32).to_le_bytes());
                ctx_bytes.extend_from_slice(&0u32.to_le_bytes());
                let remote_ctx = VirtualAllocEx(
                    proc_h,
                    std::ptr::null(),
                    ctx_bytes.len(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if remote_ctx.is_null() {
                    return Err(format!("alloc async ctx: Win32 error {}", GetLastError()));
                }
                let mut written: usize = 0;
                if WriteProcessMemory(
                    proc_h,
                    remote_ctx,
                    ctx_bytes.as_ptr().cast(),
                    ctx_bytes.len(),
                    &mut written,
                ) == 0
                    || written != ctx_bytes.len()
                {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    return Err("alloc async ctx: WriteProcessMemory incomplete".into());
                }
                let async_fn: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 =
                    std::mem::transmute::<
                        usize,
                        unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
                    >(addr);
                let start_async: RemoteThreadFn =
                    std::mem::transmute::<usize, RemoteThreadFn>(async_fn as usize);
                let async_thread = CreateRemoteThread(
                    proc_h,
                    std::ptr::null(),
                    0,
                    Some(start_async),
                    remote_ctx,
                    0,
                    std::ptr::null_mut(),
                );
                if async_thread.is_null() {
                    VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                    return Err(format!(
                        "CreateRemoteThread(async): Win32 error {}",
                        GetLastError()
                    ));
                }
                let async_wait = req.wait_ms.saturating_add(5000).max(wait_ms);
                let wait =
                    WaitForSingleObject(async_thread, if async_wait > 0 { async_wait } else { INFINITE });
                let mut code: u32 = u32::MAX;
                GetExitCodeThread(async_thread, &mut code);
                CloseHandle(async_thread);
                if wait != 0 {
                    // K4-D1 修复:原实现此处先释放 remote_ctx/params 再判 wait ——
                    // 超时时回调(async_cb)可能仍在 QQ loop 线程上引用这些缓冲。
                    // 现改为留存登记,绝不盲目释放;不用 TerminateThread。
                    timed_out = true;
                    retain_remote(pid, remote_ctx as usize, ctx_bytes.len(), "async ctx", "wait-timeout");
                    if let Some(rp) = remote_params {
                        retain_remote(pid, rp as usize, req.params.len(), "async params", "wait-timeout");
                    }
                    return Err(format!(
                        "async remote thread wait failed (remote buffers retained): code {wait:#x}"
                    ));
                }
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                if let Some(rp) = remote_params {
                    VirtualFreeEx(proc_h, rp, 0, MEM_RELEASE);
                }
                async_exit = Some(code);
            }

            Ok(InjectOutcome {
                remote_base,
                probe_exit_code: exit_code,
                obs_exit_code: obs_exit,
                register_exit_code: register_exit,
                g1_exit_code: g1_exit,
                native_exit_code: native_exit,
                g2_exit_code: g2_exit,
                g3_exit_code: g3_exit,
                g3_send_exit_code: g3_send_exit,
                g3_session_exit_code: g3_session_exit,
                native_resident_exit_code: native_resident_exit,
                env_exit_code: env_exit,
                intr_exit_code: intr_exit,
                async_exit_code: async_exit,
                exec_exit_code: exec_exit,
            })
        })();

        if timed_out {
            // K4-D1:存在未证实终止的远程线程 —— 全部已分配远端缓冲转入留存账本,
            // 不释放;生产链路不逐次远程调用,此留存随宿主进程生存期回收。
            let entries: [(&Option<*mut core::ffi::c_void>, &'static str); 6] = [
                (&buf_bridge, "bridge path"),
                (&buf_report, "probe report path"),
                (&buf_obs, "obs report path"),
                (&buf_env, "env report path"),
                (&buf_intr, "intr report path"),
                (&buf_async, "async report path"),
            ];
            for (buf, what) in entries {
                if let Some(p) = buf {
                    retain_remote(pid, *p as usize, 0, what, "wait-timeout");
                }
            }
        } else {
            if let Some(p) = buf_bridge {
                VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
            }
            if let Some(p) = buf_report {
                VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
            }
            if let Some(p) = buf_obs {
                VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
            }
            if let Some(p) = buf_env {
                VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
            }
            if let Some(p) = buf_intr {
                VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
            }
            if let Some(p) = buf_async {
                VirtualFreeEx(proc_h, p, 0, MEM_RELEASE);
            }
        }
        CloseHandle(proc_h);
        result
    }
}

/// 读远程进程内存并解析 PE 导出表,返回指定导出函数的绝对地址。
///
/// # Safety
///
/// `handle` 须具备 PROCESS_VM_READ,`base` 须是该进程内已加载映像基址。
/// 全部读取量有上界(头部 4 KiB;导出名 256 B)。
pub(crate) unsafe fn read_remote_export(
    handle: HANDLE,
    base: usize,
    want: &str,
) -> Result<Option<usize>, String> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    let read = |addr: usize, buf: &mut [u8]| -> Result<(), String> {
        let mut got: usize = 0;
        let ok = ReadProcessMemory(
            handle,
            addr as *const core::ffi::c_void,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut got,
        );
        if ok == 0 || got != buf.len() {
            return Err(format!("ReadProcessMemory failed at {addr:#x}"));
        }
        Ok(())
    };

    let mut dos = [0u8; 0x40];
    read(base, &mut dos)?;
    if &dos[0..2] != b"MZ" {
        return Err("remote image lacks MZ".into());
    }
    let e_lfanew = u32::from_le_bytes([dos[0x3C], dos[0x3D], dos[0x3E], dos[0x3F]]) as usize;

    // PE 头 + 段表整体读取(位于映像头页内,映射存在)。
    let mut hdr = [0u8; 4096];
    read(base + e_lfanew, &mut hdr)?;
    if &hdr[0..4] != b"PE\0\0" {
        return Err("remote PE signature missing".into());
    }
    let number_of_sections = u16::from_le_bytes([hdr[6], hdr[7]]) as usize;
    let size_of_optional = u16::from_le_bytes([hdr[20], hdr[21]]) as usize;
    let opt = 4 + 20;
    let magic = u16::from_le_bytes([hdr[opt], hdr[opt + 1]]);
    let data_dir_offset = match magic {
        0x20B => opt + 112, // PE32+
        0x10B => opt + 96,  // PE32
        other => return Err(format!("remote optional header magic 0x{other:04X}")),
    };
    if data_dir_offset + 8 > hdr.len() {
        return Err("remote header read too small for data directories".into());
    }
    let export_rva = u32::from_le_bytes(
        hdr[data_dir_offset..data_dir_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let export_size = u32::from_le_bytes(
        hdr[data_dir_offset + 4..data_dir_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    if export_rva == 0 || export_size == 0 || export_size > 16 * 1024 * 1024 {
        return Err("remote export directory absent or implausible".into());
    }

    // 段表范围检查(RVA 是否落在映像映射范围内)。
    let sec_base = opt + size_of_optional;
    let mut sections: Vec<(usize, usize)> = Vec::with_capacity(number_of_sections);
    for i in 0..number_of_sections {
        let off = sec_base + i * 40;
        if off + 40 > hdr.len() {
            return Err("remote section table beyond header read".into());
        }
        let vsize = u32::from_le_bytes(hdr[off + 8..off + 12].try_into().unwrap()) as usize;
        let va = u32::from_le_bytes(hdr[off + 12..off + 16].try_into().unwrap()) as usize;
        sections.push((va, va + vsize));
    }
    let rva_in_image = |rva: usize| {
        sections
            .iter()
            .any(|(start, end)| rva >= *start && rva < *end)
    };

    if !rva_in_image(export_rva) {
        return Err(format!(
            "export dir rva {export_rva:#x} outside mapped sections"
        ));
    }

    let dir_remote = base + export_rva;
    let mut dir = [0u8; 40];
    read(dir_remote, &mut dir)?;
    let number_of_names = u32::from_le_bytes(dir[24..28].try_into().unwrap()) as usize;
    let addr_names = u32::from_le_bytes(dir[32..36].try_into().unwrap()) as usize;
    let addr_ordinals = u32::from_le_bytes(dir[36..40].try_into().unwrap()) as usize;
    let addr_functions = u32::from_le_bytes(dir[28..32].try_into().unwrap()) as usize;
    if number_of_names > 100_000 {
        return Err("implausible export name count".into());
    }

    for i in 0..number_of_names {
        let mut name_ptr = [0u8; 4];
        read(base + addr_names + i * 4, &mut name_ptr)?;
        let name_rva = u32::from_le_bytes(name_ptr) as usize;
        let mut ordinal = [0u8; 2];
        read(base + addr_ordinals + i * 2, &mut ordinal)?;
        let oi = u16::from_le_bytes(ordinal) as usize;

        let mut name = [0u8; 256];
        read(base + name_rva, &mut name)?;
        let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        if std::str::from_utf8(&name[..len])
            .map(|s| s == want)
            .unwrap_or(false)
        {
            let mut fn_rva = [0u8; 4];
            read(base + addr_functions + oi * 4, &mut fn_rva)?;
            let fn_rva = u32::from_le_bytes(fn_rva) as usize;
            // probe 是本 DLL 的真实导出,不会是转发器。
            return Ok(Some(base + fn_rva));
        }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion_known_value() {
        // 2026-01-01T00:00:00Z = 1_767_225_600 unix 秒。
        // FILETIME 基准用硬编码正确值,避免与实现共用同一常量形成循环验证。
        let raw: u64 = 134_116_992_000_000_000;
        let ft = FILETIME {
            dwLowDateTime: (raw & 0xFFFF_FFFF) as u32,
            dwHighDateTime: (raw >> 32) as u32,
        };
        assert_eq!(
            filetime_to_utc(&ft).as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn own_process_creation_time_is_plausible() {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, GetCurrentProcessId());
            assert!(!h.is_null(), "OpenProcess(self) failed");
            let mut created: FILETIME = std::mem::zeroed();
            let mut exit_t: FILETIME = std::mem::zeroed();
            let mut kernel_t: FILETIME = std::mem::zeroed();
            let mut user_t: FILETIME = std::mem::zeroed();
            let ok = GetProcessTimes(h, &mut created, &mut exit_t, &mut kernel_t, &mut user_t);
            CloseHandle(h);
            assert_eq!(ok, 1, "GetProcessTimes(self) failed");
            let started = filetime_to_utc(&created).expect("creation time parseable");
            let year: i64 = started[0..4].parse().unwrap();
            assert!(
                (2024..=2036).contains(&year),
                "implausible creation year {year} in {started}"
            );
        }
    }

    #[test]
    fn retained_allocs_are_recorded_not_freed() {
        // K4-D1 资源账本:登记只追加、可快照;同一 (addr) 可重复登记
        // (多次超时),取证时按序呈现。
        let marker = 0xDEADBEEF_usize;
        retain_remote(1, marker, 64, "async ctx", "wait-timeout");
        retain_remote(2, marker + 8, 32, "async params", "wait-timeout");
        let snap = retained_remote_snapshot();
        assert!(snap.contains(&RetainedRemoteAlloc {
            pid: 1,
            addr: marker,
            bytes: 64,
            what: "async ctx",
            reason: "wait-timeout",
        }));
        assert!(snap.iter().any(|e| e.what == "async params" && e.pid == 2));
    }
}
