//! K4-D7 候选 B 首入机器(计划 §6.1/§7-D7;bootstrap-contract §2)。
//!
//! **仅 research 构建存在**(`--features research`);普通构建整体不编译。
//! 版本绑定 9.9.33-52230(vtable RVA 见 [`EXPECTED_VTABLE_RVA`])。
//!
//! 序列(D2 契约 §2 候选 B):
//! 1. 解析符号(QQNT.dll 导出面;LAB 可注入假符号驱动同一机器);
//! 2. env 新鲜度门(`[env+0] == qqnt+主vtableRVA`,I-03 修复语义);
//! 3. uv_loop 链解析(env+0xB0 → IsolateData;+0x11E8 → loop,K2-03 布局);
//! 4. **首入**:`RequestInterrupt` 载荷 —— 回调在 QQ 的 JS/loop 线程执行;
//!    回调内先核 `Isolate::GetCurrent()`(**零 current 不放行**),通过后
//!    才在该线程 `uv_async_init` 常驻句柄并发布 ready(修复旧 asyncrun
//!    从远程线程 init 的 libuv 契约违例);
//! 5. 此后:工作线程仅 `uv_async_send` 唤醒(合法跨线程面);pump 回调在
//!    owner 轮转点执行有界 drain;关闭经 CLOSE_REQ → owner 线程
//!    `uv_close` → 关闭回调确认(不释放在用句柄)。
//!
//! 全程阶段化 JSONL 取证(每阶段可独立 PASS/FAIL,供 D7 报告逐行引用)。
//! 单进程单实例(计划 §6.1:每宿主代次初始化一次)。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::exec::EXPECTED_VTABLE_RVA;

// —— 符号名(与 asyncrun 同源;QQNT.dll 导出面)——

const NAME_UV_ASYNC_INIT: &str = "uv_async_init";
const NAME_UV_ASYNC_SEND: &str = "uv_async_send";
const NAME_UV_HANDLE_SIZE: &str = "uv_handle_size";
const NAME_UV_LOOP_ALIVE: &str = "uv_loop_alive";
const NAME_UV_CLOSE: &str = "uv_close";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_REQUEST_INTERRUPT: &str = "?RequestInterrupt@node@@YAXPEAVEnvironment@1@P6AXPEAX@Z1@Z";

type FnUvAsyncInit = unsafe extern "C" fn(
    loop_: *mut core::ffi::c_void,
    async_: *mut core::ffi::c_void,
    cb: unsafe extern "C" fn(*mut core::ffi::c_void),
) -> i32;
type FnUvAsyncSend = unsafe extern "C" fn(async_: *mut core::ffi::c_void) -> i32;
type FnUvHandleSize = unsafe extern "C" fn(t: i32) -> usize;
type FnUvLoopAlive = unsafe extern "C" fn(loop_: *mut core::ffi::c_void) -> i32;
type FnUvClose = unsafe extern "C" fn(
    handle: *mut core::ffi::c_void,
    cb: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
);
type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut core::ffi::c_void;
type FnRequestInterrupt = unsafe extern "C" fn(
    env: *mut core::ffi::c_void,
    cb: unsafe extern "system" fn(*mut core::ffi::c_void),
    ctx: *mut core::ffi::c_void,
);

/// 五个首入所需符号(地址形式;调用方按上述类型别名保证 ABI)。
#[derive(Debug, Clone, Copy)]
pub struct EntrySymbols {
    pub uv_async_init: usize,
    pub uv_async_send: usize,
    pub uv_handle_size: usize,
    pub uv_close: usize,
    pub uv_loop_alive: usize,
    pub isolate_get_current: usize,
    pub request_interrupt: usize,
}

impl EntrySymbols {
    /// 从 QQNT.dll 基址解析(真实路线;obs::export_addr 走导出表)。
    pub fn resolve(qqnt: usize, report: &str) -> Option<Self> {
        let get = |name: &str| -> Option<usize> {
            // SAFETY: obs::export_addr 只读导出表。
            let a = unsafe { crate::obs::export_addr(qqnt, name) };
            if a.is_none() {
                append_stage(report, "resolve", false, &format!("export missing: {name}"));
            }
            a
        };
        Some(Self {
            uv_async_init: get(NAME_UV_ASYNC_INIT)?,
            uv_async_send: get(NAME_UV_ASYNC_SEND)?,
            uv_handle_size: get(NAME_UV_HANDLE_SIZE)?,
            uv_loop_alive: get(NAME_UV_LOOP_ALIVE)?,
            uv_close: get(NAME_UV_CLOSE)?,
            isolate_get_current: get(NAME_ISOLATE_GETCURRENT)?,
            request_interrupt: get(NAME_REQUEST_INTERRUPT)?,
        })
    }

    /// LAB 注入(假符号;机器逻辑与真实路线共用)。
    pub fn from_raw(
        uv_async_init: usize,
        uv_async_send: usize,
        uv_handle_size: usize,
        uv_close: usize,
        uv_loop_alive: usize,
        isolate_get_current: usize,
        request_interrupt: usize,
    ) -> Self {
        Self {
            uv_async_init,
            uv_async_send,
            uv_handle_size,
            uv_close,
            uv_loop_alive,
            isolate_get_current,
            request_interrupt,
        }
    }

    fn to_fn<T: Copy>(a: usize) -> T {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
        // SAFETY: 调用方保证地址与 T 的 ABI 匹配(真实:导出表;LAB:测试注入)。
        unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
    }
}

/// 首入配置。
#[derive(Debug, Clone)]
pub struct EntryConfig {
    pub qqnt_base: usize,
    pub env: usize,
    pub report_path: String,
}

/// 首入错误(与阶段日志对应)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryError {
    EnvNotFresh,
    LoopChainUnreadable,
    InterruptNoCurrentContext,
    UvInitFailed { r: i32 },
    HandleSizeInvalid { size: usize },
    Timeout,
    AlreadyBootstrapped,
}

// —— 共享状态(单实例;extern 回调经静态访问) ——

struct EntryShared {
    // 配置(bootstrap 成功前由发布方写入,ready 后只读)。
    qqnt_base: AtomicUsize,
    env: AtomicUsize,
    isolate: AtomicUsize,
    loop_ptr: AtomicUsize,
    // 符号地址。
    sym_uv_async_init: AtomicUsize,
    sym_uv_async_send: AtomicUsize,
    sym_uv_handle_size: AtomicUsize,
    sym_uv_loop_alive: AtomicUsize,
    sym_uv_close: AtomicUsize,
    sym_isolate_get_current: AtomicUsize,
    // 机器状态。
    tid: AtomicUsize,        // owner 线程(首入回调线程)
    handle: AtomicUsize,     // uv_async 句柄
    ready: AtomicUsize,      // 1 = 首入完成
    error_code: AtomicUsize, // ERR_*;0 = 无
    error_aux: AtomicUsize,  // 附带数值(size/r)
    quarantine: AtomicUsize, // pump 发现错误线程
    close_req: AtomicUsize,
    closed: AtomicUsize,
    pumps: AtomicU64,
    wake_sends: AtomicU64,
}

/// owner 轮转点 drain 钩子(boxed 闭包;接线 worker 结果通道等捕获)。
/// 单宿主代次单钩子;shutdown 清空(资源随代次收尾,非泄漏)。
static DRAIN_HOOK: std::sync::Mutex<Option<Box<dyn Fn() -> usize + Send>>> =
    std::sync::Mutex::new(None);

/// 已启动的 daemon worker 停止标志(shutdown 先停 worker 再走关闭协议)。
static DAEMON_STOP: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

/// 自由函数版钩子安装(导出侧在句柄 move 后仍可调用)。
pub fn install_drain_hook(f: Box<dyn Fn() -> usize + Send>) {
    if let Ok(mut g) = DRAIN_HOOK.lock() {
        *g = Some(f);
    }
}

/// Install a one-shot research hook only when no resident/daemon hook owns the pump.
/// This prevents an isolated probe from replacing the active production seam.
pub fn try_install_drain_hook(f: Box<dyn Fn() -> usize + Send>) -> bool {
    match DRAIN_HOOK.lock() {
        Ok(mut g) if g.is_none() => {
            *g = Some(f);
            true
        }
        _ => false,
    }
}

/// Clear a completed one-shot hook. The pump mutex serializes this with any active callback.
pub fn clear_drain_hook() {
    if let Ok(mut g) = DRAIN_HOOK.lock() {
        *g = None;
    }
}

/// Wake the owner pump through the sole cross-thread libuv operation.
pub fn wake_owner_pump() -> bool {
    wake_trampoline()
}

/// daemon 启动产物(worker 停止标志;导出侧保存)。
#[derive(Debug, Clone)]
pub struct DaemonStartup {
    pub stop: Arc<AtomicBool>,
}

/// bootstrap 成功的 EntryHandle 存档(daemon 导出取用;取走即空)。
static STORED_HANDLE: std::sync::Mutex<Option<EntryHandle>> = std::sync::Mutex::new(None);
/// 唤醒函数地址(uv_async_send 包装;worker 侧跨线程面)。
static WAKE_ADDR: AtomicUsize = AtomicUsize::new(0);
/// 报告路径指针(bootstrap 登记;shutdown/daemon 阶段日志)。
static REPORT_PATH_PTR: AtomicUsize = AtomicUsize::new(0);

/// daemon 导出侧:存档 bootstrap 句柄(bootstrap 成功后由导出调用)。
pub fn store_entry_handle(h: EntryHandle, wake_addr: usize) {
    WAKE_ADDR.store(wake_addr, Ordering::Release);
    if let Ok(mut g) = STORED_HANDLE.lock() {
        *g = Some(h);
    }
}

/// daemon 导出侧:取走句柄(每代次一次)。
pub fn take_stored_entry_handle() -> Option<EntryHandle> {
    STORED_HANDLE.lock().ok()?.take()
}

/// worker 唤醒闭包地址(uv_async_send;0 = 不可用)。
pub fn peek_stored_wake() -> Option<fn() -> bool> {
    let addr = WAKE_ADDR.load(Ordering::Acquire);
    if addr == 0 {
        return None;
    }
    // SAFETY: 地址来自 EntryHandle::wake 的 fn 指针(bootstrap 成功后登记)。
    Some(unsafe { std::mem::transmute::<usize, fn() -> bool>(addr) })
}

/// daemon/D8 侧:isolate 地址(首入时解析;ready 前 0)。
pub fn isolate_addr() -> *mut core::ffi::c_void {
    ENTRY.isolate.load(Ordering::Acquire) as *mut core::ffi::c_void
}

/// 登记由调用方保证进程生存期的报告路径缓冲(daemon 配置路径使用)。
pub fn register_report_path(p: *const u16) {
    REPORT_PATH_PTR.store(p as usize, Ordering::Release);
}

/// Whether the one-time K4 owner entry is ready.
pub fn is_ready() -> bool {
    ENTRY.ready.load(Ordering::Acquire) == 1
}

/// Frozen QQNT base captured by the successful owner bootstrap.
pub fn qqnt_base() -> usize {
    ENTRY.qqnt_base.load(Ordering::Acquire)
}

/// daemon 导出侧:读取 worker 停止标志(shutdown 先停 worker)。
pub fn peek_daemon_startup() -> Option<DaemonStartup> {
    DAEMON_STOP.get().map(|f| DaemonStartup { stop: f.clone() })
}

/// 唤醒蹦床:EntryHandle::wake 的稳定地址(daemon 导出登记用;
/// wake 内部读 ENTRY 静态,不捕获环境)。
pub fn wake_trampoline_addr() -> usize {
    // SAFETY: 仅取关联 fn 项地址。
    unsafe { std::mem::transmute::<fn() -> bool, usize>(wake_trampoline) }
}

/// 蹦床本体:读静态句柄执行 uv_async_send。
fn wake_trampoline() -> bool {
    // 句柄地址存于 ENTRY.handle;ready 后恒有效。
    if ENTRY.ready.load(Ordering::Acquire) != 1 {
        return false;
    }
    // SAFETY: 符号地址经 bootstrap 校验;uv_async_send 是唯一合法跨线程面。
    unsafe {
        let send: FnUvAsyncSend =
            EntrySymbols::to_fn(ENTRY.sym_uv_async_send.load(Ordering::Acquire));
        (send)(ENTRY.handle.load(Ordering::Acquire) as *mut core::ffi::c_void) == 0
    }
}

static ENTRY: EntryShared = EntryShared {
    qqnt_base: AtomicUsize::new(0),
    env: AtomicUsize::new(0),
    isolate: AtomicUsize::new(0),
    loop_ptr: AtomicUsize::new(0),
    sym_uv_async_init: AtomicUsize::new(0),
    sym_uv_async_send: AtomicUsize::new(0),
    sym_uv_handle_size: AtomicUsize::new(0),
    sym_uv_loop_alive: AtomicUsize::new(0),
    sym_uv_close: AtomicUsize::new(0),
    sym_isolate_get_current: AtomicUsize::new(0),
    tid: AtomicUsize::new(0),
    handle: AtomicUsize::new(0),
    ready: AtomicUsize::new(0),
    error_code: AtomicUsize::new(0),
    error_aux: AtomicUsize::new(0),
    quarantine: AtomicUsize::new(0),
    close_req: AtomicUsize::new(0),
    closed: AtomicUsize::new(0),
    pumps: AtomicU64::new(0),
    wake_sends: AtomicU64::new(0),
};

const ERR_NONE: usize = 0;
const ERR_ENV_NOT_FRESH: usize = 1;
const ERR_LOOP_CHAIN: usize = 2;
const ERR_NO_CURRENT: usize = 3;
const ERR_HANDLE_SIZE: usize = 4;
const ERR_UV_INIT: usize = 5;
const ERR_TIMEOUT: usize = 6;

fn set_error_code(code: usize, aux: usize) {
    ENTRY.error_code.store(code, Ordering::Release);
    ENTRY.error_aux.store(aux, Ordering::Release);
}

fn set_error(e: EntryError) {
    match e {
        EntryError::EnvNotFresh => set_error_code(ERR_ENV_NOT_FRESH, 0),
        EntryError::LoopChainUnreadable => set_error_code(ERR_LOOP_CHAIN, 0),
        EntryError::InterruptNoCurrentContext => set_error_code(ERR_NO_CURRENT, 0),
        EntryError::HandleSizeInvalid { size } => set_error_code(ERR_HANDLE_SIZE, size),
        EntryError::UvInitFailed { r } => set_error_code(ERR_UV_INIT, r as usize),
        EntryError::Timeout => set_error_code(ERR_TIMEOUT, 0),
        EntryError::AlreadyBootstrapped => set_error_code(ERR_TIMEOUT, 0),
    }
}

fn take_error() -> Option<EntryError> {
    let code = ENTRY.error_code.swap(ERR_NONE, Ordering::Acquire);
    let aux = ENTRY.error_aux.load(Ordering::Acquire);
    match code {
        ERR_NONE => None,
        ERR_ENV_NOT_FRESH => Some(EntryError::EnvNotFresh),
        ERR_LOOP_CHAIN => Some(EntryError::LoopChainUnreadable),
        ERR_NO_CURRENT => Some(EntryError::InterruptNoCurrentContext),
        ERR_HANDLE_SIZE => Some(EntryError::HandleSizeInvalid { size: aux }),
        ERR_UV_INIT => Some(EntryError::UvInitFailed { r: aux as i32 }),
        _ => Some(EntryError::Timeout),
    }
}

/// 无路径场景的阶段日志(shutdown 导出用):写到进程内环形缓冲不可行,
/// 直接落到与 bootstrap 相同的报告文件(路径存于 ENTRY 之外 —— shutdown
/// 在 bootstrap 之后调用,路径经 bootstrap 记录;此处从静态取)。
pub fn append_stage_simple(stage: &str, ok: bool, detail: &str) {
    // bootstrap 将报告路径复制到 bridge 进程生存期存储；不得保留 loader 的临时远程缓冲指针。
    let path = REPORT_PATH_PTR.load(Ordering::Acquire);
    if path == 0 {
        return;
    }
    // SAFETY: bootstrap 已将该指针登记为 NUL 结尾 UTF-16(进程生存期)。
    let text = unsafe { wide_ptr_to_string_pub(path as *const u16) };
    if let Some(p) = text {
        append_stage(&p, stage, ok, detail);
    }
}

/// SAFETY: p 须指向 NUL 结尾 UTF-16 缓冲。
unsafe fn wide_ptr_to_string_pub(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: 同 wide_ptr_to_string。
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

// —— 页校验读(与 asyncrun/intr 同语义)——

fn page_readable(addr: usize) -> bool {
    // SAFETY: VirtualQuery 无锁查询。
    unsafe {
        let mut mbi: windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION =
            std::mem::zeroed();
        let n = windows_sys::Win32::System::Memory::VirtualQuery(
            addr as *const core::ffi::c_void,
            &mut mbi,
            std::mem::size_of::<windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION>(),
        );
        if n == 0 {
            return false;
        }
        const MEM_COMMIT: u32 = 0x1000;
        const PAGE_NOACCESS: u32 = 0x01;
        const PAGE_GUARD: u32 = 0x100;
        mbi.State == MEM_COMMIT && (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)) == 0
    }
}

fn read_usize_checked(p: usize) -> Option<usize> {
    if !page_readable(p) || !page_readable(p + 7) {
        return None;
    }
    // SAFETY: 页已校验。
    unsafe { Some((p as *const usize).read_unaligned()) }
}

// —— 阶段日志 ——

pub fn append_stage(report: &str, stage: &str, ok: bool, detail: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let line = format!(
        "{{\"ts\":{},\"tid\":{},\"stage\":\"{}\",\"ok\":{},\"detail\":\"{}\"}}\n",
        ts,
        tid,
        stage,
        ok,
        detail.replace('\\', "\\\\").replace('"', "'")
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(report)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

// —— 首入回调与 pump(libuv 轮转点)——

/// 首入载荷:RequestInterrupt 的处理点(QQ 的 JS/loop 线程)。
/// 检查顺序固定:线程记录 → **current isolate 核对(零不放行)** →
/// uv_handle_size → uv_async_init。任一失败即置 error 并中止,**不触碰宿主**。
unsafe extern "system" fn first_entry_cb(_ctx: *mut core::ffi::c_void) {
    // SAFETY: 各符号地址由 bootstrap 校验后发布;调用约定见类型别名。
    unsafe {
        let tid = windows_sys::Win32::System::Threading::GetCurrentThreadId();
        ENTRY.tid.store(tid as usize, Ordering::Release);
        let get_current: FnIsolateGetCurrent =
            EntrySymbols::to_fn(ENTRY.sym_isolate_get_current.load(Ordering::Acquire));
        let current = (get_current)() as usize;
        let expected = ENTRY.isolate.load(Ordering::Acquire);
        if current == 0 || current != expected {
            set_error(EntryError::InterruptNoCurrentContext);
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        let handle_size: FnUvHandleSize =
            EntrySymbols::to_fn(ENTRY.sym_uv_handle_size.load(Ordering::Acquire));
        let size = (handle_size)(1 /* UV_ASYNC */);
        if size == 0 || size > 0x1000 {
            set_error(EntryError::HandleSizeInvalid { size });
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        let layout = std::alloc::Layout::from_size_align(size, 16).unwrap();
        let handle = std::alloc::alloc(layout);
        if handle.is_null() {
            set_error(EntryError::HandleSizeInvalid { size });
            return;
        }
        core::ptr::write_bytes(handle, 0, size);
        let init: FnUvAsyncInit =
            EntrySymbols::to_fn(ENTRY.sym_uv_async_init.load(Ordering::Acquire));
        let r = (init)(
            ENTRY.loop_ptr.load(Ordering::Acquire) as *mut core::ffi::c_void,
            handle as *mut core::ffi::c_void,
            pump_cb,
        );
        if r != 0 {
            std::alloc::dealloc(handle, layout);
            set_error(EntryError::UvInitFailed { r });
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        ENTRY.handle.store(handle as usize, Ordering::Release);
        ENTRY.ready.store(1, Ordering::Release);
    }
}

/// 常驻 pump:owner 轮转点执行。线程不符 → quarantine(保留证据,不继续)。
unsafe extern "C" fn pump_cb(_handle: *mut core::ffi::c_void) {
    // SAFETY: 静态原子量;drain_hook 由本 crate 注册。
    unsafe {
        let tid = windows_sys::Win32::System::Threading::GetCurrentThreadId() as usize;
        if tid != ENTRY.tid.load(Ordering::Acquire) {
            ENTRY.quarantine.store(1, Ordering::Release);
            return;
        }
        ENTRY.pumps.fetch_add(1, Ordering::Relaxed);
        if ENTRY.close_req.load(Ordering::Acquire) == 1 {
            let close: FnUvClose = EntrySymbols::to_fn(ENTRY.sym_uv_close.load(Ordering::Acquire));
            (close)(
                ENTRY.handle.load(Ordering::Acquire) as *mut core::ffi::c_void,
                Some(close_cb),
            );
            return;
        }
        if let Ok(guard) = DRAIN_HOOK.lock() {
            if let Some(f) = guard.as_ref() {
                let _ = f();
            }
        }
    }
}

unsafe extern "C" fn close_cb(_handle: *mut core::ffi::c_void) {
    ENTRY.closed.store(1, Ordering::Release);
}

/// 首入结果句柄。
#[derive(Debug)]
pub struct EntryHandle;

impl EntryHandle {
    /// 工作线程唤醒(合法跨线程面:uv_async_send)。
    pub fn wake(&self) -> bool {
        if ENTRY.ready.load(Ordering::Acquire) != 1 {
            return false;
        }
        ENTRY.wake_sends.fetch_add(1, Ordering::Relaxed);
        // SAFETY: 地址来自导出解析/注入;uv_async_send 是 libuv 唯一
        // 明确允许跨线程调用的接口。
        unsafe {
            let send: FnUvAsyncSend =
                EntrySymbols::to_fn(ENTRY.sym_uv_async_send.load(Ordering::Acquire));
            (send)(ENTRY.handle.load(Ordering::Acquire) as *mut core::ffi::c_void) == 0
        }
    }

    /// 注册 owner 轮转点 drain 钩子(返回处理数;shutdown 时随代次清空)。
    pub fn set_drain_hook(&self, f: Box<dyn Fn() -> usize + Send>) {
        install_drain_hook(f);
    }

    /// 关闭后清空钩子(资源收尾语义,非泄漏)。
    pub fn clear_drain_hook(&self) {
        if let Ok(mut g) = DRAIN_HOOK.lock() {
            *g = None;
        }
    }

    /// daemon worker 停止标志(shutdown 接线用;首次调用创建)。
    pub fn daemon_stop_flag(&self) -> Arc<AtomicBool> {
        DAEMON_STOP
            .get_or_init(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// 关闭协议:置 CLOSE_REQ → 唤醒 → owner pump 执行 uv_close →
    /// 等关闭回调。超时不证明关闭完成:返回 Timeout,**不释放句柄内存**
    /// (计划 §5.3;留证据,随进程回收)。
    pub fn close(&self, wait_ms: u32) -> Result<(), EntryError> {
        shutdown(wait_ms)
    }

    pub fn owner_tid(&self) -> usize {
        ENTRY.tid.load(Ordering::Acquire)
    }

    pub fn pumps(&self) -> u64 {
        ENTRY.pumps.load(Ordering::Relaxed)
    }

    pub fn quarantine(&self) -> bool {
        ENTRY.quarantine.load(Ordering::Acquire) == 1
    }
}

/// 执行首入(计划 §6.1:每宿主代次一次;重复调用拒绝)。
pub fn bootstrap(cfg: &EntryConfig, symbols: &EntrySymbols) -> Result<EntryHandle, EntryError> {
    if ENTRY.ready.load(Ordering::Acquire) == 1 {
        return Err(EntryError::AlreadyBootstrapped);
    }
    let report = &cfg.report_path;
    // 阶段 1:env 新鲜度门。
    let Some(vfptr) = read_usize_checked(cfg.env) else {
        append_stage(report, "env_fresh", false, "env page unreadable");
        return Err(EntryError::EnvNotFresh);
    };
    let expected = (cfg.qqnt_base as u64).wrapping_add(EXPECTED_VTABLE_RVA as u64);
    if vfptr as u64 != expected {
        append_stage(
            report,
            "env_fresh",
            false,
            &format!("vfptr {:#x} != expected {:#x}", vfptr as u64, expected),
        );
        return Err(EntryError::EnvNotFresh);
    }
    append_stage(report, "env_fresh", true, "env vtable matches frozen RVA");
    // 阶段 2:uv_loop 链(env+0xB0 → IsolateData;+0x11E8 → loop)。
    let Some(isolate_data) = read_usize_checked(cfg.env + 0xB0) else {
        append_stage(report, "loop_chain", false, "env+0xB0 unreadable");
        return Err(EntryError::LoopChainUnreadable);
    };
    let Some(isolate) = read_usize_checked(cfg.env + 0xA0) else {
        append_stage(report, "loop_chain", false, "env+0xA0 unreadable");
        return Err(EntryError::LoopChainUnreadable);
    };
    let Some(loop_ptr) = read_usize_checked(isolate_data + 0x11E8) else {
        append_stage(report, "loop_chain", false, "IsolateData+0x11E8 unreadable");
        return Err(EntryError::LoopChainUnreadable);
    };
    if isolate == 0 || loop_ptr == 0 {
        append_stage(report, "loop_chain", false, "isolate/loop null");
        return Err(EntryError::LoopChainUnreadable);
    }
    append_stage(
        report,
        "loop_chain",
        true,
        &format!("isolate={isolate:#x} loop={loop_ptr:#x}"),
    );
    // 发布(ready 之前;回调以 Acquire 读取)。必须登记**进程生存期**的宽字符
    // 副本:此前存的是局部 UTF-8 String 的指针,bootstrap 返回即悬空,
    // 后续 append_stage_simple 经悬空指针按 UTF-16 解析出乱码路径,在当前
    // 工作目录创建乱码名垃圾文件(D9 现场遗留的磁盘症状之一)。
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(&cfg.report_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let leaked: &'static mut [u16] = Box::leak(wide.into_boxed_slice());
        REPORT_PATH_PTR.store(leaked.as_ptr() as usize, Ordering::Release);
    }
    ENTRY.qqnt_base.store(cfg.qqnt_base, Ordering::Release);
    ENTRY.env.store(cfg.env, Ordering::Release);
    ENTRY.isolate.store(isolate, Ordering::Release);
    ENTRY.loop_ptr.store(loop_ptr, Ordering::Release);
    ENTRY
        .sym_uv_async_init
        .store(symbols.uv_async_init, Ordering::Release);
    ENTRY
        .sym_uv_async_send
        .store(symbols.uv_async_send, Ordering::Release);
    ENTRY
        .sym_uv_handle_size
        .store(symbols.uv_handle_size, Ordering::Release);
    ENTRY
        .sym_uv_loop_alive
        .store(symbols.uv_loop_alive, Ordering::Release);
    ENTRY
        .sym_uv_close
        .store(symbols.uv_close, Ordering::Release);
    ENTRY
        .sym_isolate_get_current
        .store(symbols.isolate_get_current, Ordering::Release);
    // 阶段 3:首入(RequestInterrupt;回调同步或异步执行均按状态机等待)。
    append_stage(
        report,
        "interrupt_posted",
        true,
        "posting first-entry payload",
    );
    // SAFETY: env 经新鲜度门;符号 ABI 见类型别名(K2/K3 反汇编实证)。
    unsafe {
        let ri: FnRequestInterrupt = EntrySymbols::to_fn(symbols.request_interrupt);
        ri(
            cfg.env as *mut core::ffi::c_void,
            first_entry_cb,
            std::ptr::null_mut(),
        );
    }
    // 等待首入完成(空闲 QQ 的 JS 间隙可拖延数分钟 —— K2-03 §12.3 记录;
    // 调用方按 D7 观察窗给足超时,超时不是失败证据,记录后由执行者决策)。
    let deadline = Instant::now() + Duration::from_secs(600);
    while ENTRY.ready.load(Ordering::Acquire) != 1 {
        if let Some(e) = take_error() {
            append_stage(report, "first_entry", false, &format!("{e:?}"));
            return Err(e);
        }
        if Instant::now() >= deadline {
            append_stage(
                report,
                "first_entry",
                false,
                "timeout(600s;空闲 QQ 首入延迟候选)",
            );
            return Err(EntryError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    append_stage(
        report,
        "first_entry",
        true,
        &format!("owner_tid={}", ENTRY.tid.load(Ordering::Acquire)),
    );
    Ok(EntryHandle)
}

/// 关闭协议(导出面用;无句柄实例依赖)。语义同 `EntryHandle::close`:
/// 超时返回 Timeout 且**不释放句柄**(计划 §5.3)。
pub fn shutdown(wait_ms: u32) -> Result<(), EntryError> {
    if ENTRY.ready.load(Ordering::Acquire) != 1 {
        return Err(EntryError::AlreadyBootstrapped);
    }
    ENTRY.close_req.store(1, Ordering::Release);
    // 唤醒 owner 轮转点(uv_async_send:唯一合法跨线程面)。
    let wake_ok = unsafe {
        let send: FnUvAsyncSend =
            EntrySymbols::to_fn(ENTRY.sym_uv_async_send.load(Ordering::Acquire));
        (send)(ENTRY.handle.load(Ordering::Acquire) as *mut core::ffi::c_void) == 0
    };
    let deadline = Instant::now() + Duration::from_millis(wait_ms as u64);
    while ENTRY.closed.load(Ordering::Acquire) != 1 {
        if Instant::now() >= deadline {
            append_stage_simple(
                "shutdown",
                false,
                &format!("timeout(wait_ms={wait_ms} wake_ok={wake_ok}); handle retained"),
            );
            return Err(EntryError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Ok(mut g) = DRAIN_HOOK.lock() {
        *g = None; // 关闭确认:钩子随代次收尾
    }
    Ok(())
}

/// libuv 面存活校验(pump 阶段的宿主状态 gate):uv_loop_alive(loop)。
/// 语义注记(计划 §5.1 范围界定):零 current 拒绝针对 V8/Node 表面;
/// D7 的 pump 阶段无任何 V8 调用(监听器延迟、发送未接线),libuv 面
/// 以 owner 线程 + env 新鲜 + loop 存活三重校验。D8/D9 触碰 V8 的操作
/// 必须另加 entered/current context 校验(见 QqOwnerAdapter 文档)。
pub fn uv_loop_alive() -> bool {
    if ENTRY.ready.load(Ordering::Acquire) != 1 {
        return false;
    }
    // SAFETY: 符号地址经 bootstrap 校验;调用发生在 loop 线程(pump)或
    // 只读查询。
    unsafe {
        let f: FnUvLoopAlive = EntrySymbols::to_fn(ENTRY.sym_uv_loop_alive.load(Ordering::Acquire));
        (f)(ENTRY.loop_ptr.load(Ordering::Acquire) as *mut core::ffi::c_void) != 0
    }
}

/// LAB 专用:重置首入静态状态(仅 research 构建导出;生产链路永不调用 ——
/// 真机上单宿主代次单实例,重置意味着允许二次首入,违反 §6.1)。
#[doc(hidden)]
pub fn test_reset_state() {
    ENTRY.qqnt_base.store(0, Ordering::Release);
    ENTRY.env.store(0, Ordering::Release);
    ENTRY.isolate.store(0, Ordering::Release);
    ENTRY.loop_ptr.store(0, Ordering::Release);
    ENTRY.sym_uv_async_init.store(0, Ordering::Release);
    ENTRY.sym_uv_async_send.store(0, Ordering::Release);
    ENTRY.sym_uv_handle_size.store(0, Ordering::Release);
    ENTRY.sym_uv_loop_alive.store(0, Ordering::Release);
    ENTRY.sym_uv_close.store(0, Ordering::Release);
    ENTRY.sym_isolate_get_current.store(0, Ordering::Release);
    ENTRY.tid.store(0, Ordering::Release);
    ENTRY.handle.store(0, Ordering::Release);
    ENTRY.ready.store(0, Ordering::Release);
    ENTRY.error_code.store(0, Ordering::Release);
    ENTRY.error_aux.store(0, Ordering::Release);
    ENTRY.quarantine.store(0, Ordering::Release);
    ENTRY.close_req.store(0, Ordering::Release);
    ENTRY.closed.store(0, Ordering::Release);
    if let Ok(mut g) = DRAIN_HOOK.lock() {
        *g = None;
    }
    ENTRY.pumps.store(0, Ordering::Relaxed);
    ENTRY.wake_sends.store(0, Ordering::Relaxed);
}

// —— HostAdapter 适配(QqOwnerAdapter;resident 的生产缝) ——

/// 候选 B 宿主适配器:D7 面向 Health/身份/停止;消息监听(D8)与
/// 发送(D9)显式拒绝,不用占位成果冒充能力。
pub struct QqOwnerAdapter {
    handle: EntryHandle,
    account: String,
    voice_profile: bool,
}

impl QqOwnerAdapter {
    /// 首入成功后构造。`f` 由 bootstrap 侧提供。
    pub fn new(handle: EntryHandle, account: String) -> Self {
        let (loaded_modules, snapshot_ok) = crate::collect_host_snapshot();
        let voice_profile = snapshot_ok && crate::qq_query::matches_voice_profile(&loaded_modules);
        Self {
            handle,
            account,
            voice_profile,
        }
    }

    pub fn entry(&self) -> &EntryHandle {
        &self.handle
    }
}

impl crate::host_adapter::HostAdapter for QqOwnerAdapter {
    fn owner_thread_id(&self) -> u64 {
        self.handle.owner_tid() as u64
    }
    /// 当前执行线程(owner 语义由调用方线程保证;真实线程标识逐次读取)。
    fn current_thread_id(&self) -> u64 {
        // SAFETY: 无副作用。
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
    }
    fn env_valid(&self) -> bool {
        // 每次进入宿主 API 前重读新鲜度门(不缓存 —— K3 教训)。
        let expected = (ENTRY.qqnt_base.load(Ordering::Acquire) as u64)
            .wrapping_add(EXPECTED_VTABLE_RVA as u64);
        match read_usize_checked(self.handle_env()) {
            Some(v) => {
                let ok = v as u64 == expected;
                if !ok {
                    append_stage_simple(
                        "env_valid_false",
                        false,
                        &format!("vfptr={v:#x} expected={expected:#x}"),
                    );
                }
                ok
            }
            None => {
                append_stage_simple("env_valid_false", false, "page unreadable");
                false
            }
        }
    }
    /// 范围界定(计划 §5.1 + K2-03 r3 实证):quiet uv 轮转点 GetCurrent
    /// 合法为 0,entered/incumbent 才是有效上下文证明。D7/D8/D9 的 pump 面
    /// (监听器/轮询/发送经 HandleScope)由 qq_v8::exec_script 的阶梯上下文
    /// 校验把守 —— 本方法承载 owner 线程 + env 新鲜之外的第三重:loop 存活。
    fn current_context_ok(&self) -> bool {
        uv_loop_alive()
    }
    fn native_op(
        &mut self,
        op: crate::host_adapter::HostOp,
    ) -> Result<crate::host_adapter::HostOpResult, crate::host_adapter::HostError> {
        use crate::host_adapter::{HostError, HostOp, HostOpResult};
        match op {
            HostOp::Probe => {
                if !self.env_valid() {
                    return Err(HostError::EnvInvalid);
                }
                Ok(HostOpResult::ProbeDone)
            }
            // D8:真实监听器接线(K3 现场验证脚本,经 V8 阶梯执行;
            // 零 current / 轮转点条件不满足时显式失败,由调用方重试)。
            HostOp::ListenerAdd => match crate::qq_v8::start_listener_with_voice(
                self.voice_profile.then_some(self.account.as_str()),
            ) {
                Ok(s) if s.starts_with("ARMED") => {
                    let token = s
                        .split("lidRet=")
                        .nth(1)
                        .and_then(|t| t.trim().parse::<u64>().ok())
                        .unwrap_or(0);
                    append_stage_simple("listener_armed", true, &s);
                    Ok(HostOpResult::ListenerAdded { token })
                }
                Ok(s) if s.starts_with("ALREADY") => {
                    append_stage_simple("listener_already", true, &s);
                    Ok(HostOpResult::ListenerAdded { token: 0 })
                }
                Ok(s) => {
                    append_stage_simple("listener_err", false, &s);
                    Err(HostError::Native { code: 1 })
                }
                Err(e) => {
                    append_stage_simple("listener_v8_err", false, &format!("{e:?}"));
                    match e {
                        crate::qq_v8::V8Error::NoCurrentContext => Err(HostError::NoCurrentContext),
                        _ => Err(HostError::EnvInvalid),
                    }
                }
            },
            HostOp::ListenerRemove { .. } => match crate::qq_v8::stop_listener() {
                Ok(s) if s.starts_with("STOPPED") || s.starts_with("NOT_ARMED") => {
                    Ok(HostOpResult::ListenerRemoved)
                }
                Ok(_) => Err(HostError::Native { code: 2 }),
                Err(crate::qq_v8::V8Error::NoCurrentContext) => Err(HostError::NoCurrentContext),
                Err(_) => Err(HostError::EnvInvalid),
            },
            // D9:参数化发送(K3 验证脚本经 V8 阶梯;fired 非终态,
            // Promise 结果经轮询 sendResults 回收 —— 见 daemon pump 钩子)。
            HostOp::SendPrivateVoice { request_id, wire_seq, timeout_ms, peer_uin, silk } => {
                if !self.voice_profile {
                    return Err(HostError::NoProvenRoute);
                }
                let script = crate::qq_v8::build_voice_send_js(
                    &self.account, &peer_uin, &request_id, wire_seq, timeout_ms, &silk,
                ).ok_or(HostError::Native { code: 3 })?;
                let output = crate::qq_v8::run_fixed_query_script(&script).map_err(|error| {
                    match error {
                        crate::qq_v8::V8Error::NoCurrentContext => HostError::NoCurrentContext,
                        _ => HostError::EnvInvalid,
                    }
                })?;
                let value: serde_json::Value =
                    serde_json::from_str(&output).map_err(|_| HostError::EnvInvalid)?;
                if value.get("fired").and_then(|v| v.as_bool()) == Some(true)
                    && value.get("mid").and_then(|v| v.as_str()) == Some(&request_id)
                {
                    Ok(HostOpResult::SendFired { mid: request_id })
                } else if value.get("err").and_then(|v| v.as_str()).is_some() {
                    Err(HostError::Native { code: 3 })
                } else {
                    Err(HostError::EnvInvalid)
                }
            }
            HostOp::SendText {
                request_id,
                wire_seq,
                timeout_ms,
                chat_type,
                peer_uid,
                text,
            } => {
                match crate::qq_v8::send_text(
                    chat_type,
                    &peer_uid,
                    &text,
                    &self.account,
                    &request_id,
                    wire_seq,
                    timeout_ms,
                ) {
                    Ok(out) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
                            if v.get("fired").and_then(|x| x.as_bool()) == Some(true) {
                                let mid = v
                                    .get("mid")
                                    .map(|x| x.to_string().trim_matches('"').to_string())
                                    .unwrap_or_default();
                                return Ok(HostOpResult::SendFired { mid });
                            }
                            // "ERR:..." / NOT_ARMED:宿主侧明确失败。
                            return Err(HostError::Native { code: 3 });
                        }
                        Err(HostError::Native { code: 3 })
                    }
                    Err(crate::qq_v8::V8Error::NoCurrentContext) => {
                        Err(HostError::NoCurrentContext)
                    }
                    Err(_) => Err(HostError::EnvInvalid),
                }
            }
        }
    }

    fn query_capabilities(&self) -> &'static [caligo_model::capability::QueryCapability] {
        if self.voice_profile {
            crate::qq_query::SUPPORTED_CAPABILITIES
        } else {
            crate::qq_query::supported_capabilities()
        }
    }

    fn send_capabilities(&self) -> (bool, bool) {
        (true, true)
    }

    fn supports_private_voice(&self) -> bool {
        self.voice_profile
    }

    fn cancel_query(&mut self, request_id: &str, scope: &caligo_model::capability::QueryScope) {
        let script = crate::qq_query::build_query_cancel_js(request_id, scope);
        let _ = crate::qq_v8::run_fixed_query_script(&script);
    }

    fn query(
        &mut self,
        request_id: &str,
        scope: &caligo_model::capability::QueryScope,
        query: &caligo_model::capability::HostQuery,
        timeout_ms: u64,
        poll_only: bool,
    ) -> Option<caligo_model::capability::QueryOutcome> {
        if !self.query_capabilities().contains(&query.capability()) {
            return Some(caligo_model::capability::QueryOutcome::Unsupported);
        }
        let script = if poll_only {
            crate::qq_query::build_query_poll_js(request_id, scope)
        } else {
            crate::qq_query::build_query_start_js(request_id, scope, query, timeout_ms)
        };
        let output = crate::qq_v8::run_fixed_query_script(&script).ok()?;
        match crate::qq_query::decode_query_poll(query, scope, &output) {
            Ok(Some(outcome)) => Some(outcome),
            Ok(None) => None,
            Err(_) => Some(caligo_model::capability::QueryOutcome::InvalidResponse),
        }
    }
}

impl QqOwnerAdapter {
    fn handle_env(&self) -> usize {
        ENTRY.env.load(Ordering::Acquire)
    }
}
