//! In-memory fake owner and libuv fixture shared by both Windows labs.

use caligo_bridge::qq_entry::EntrySymbols;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());
pub(crate) static WAKE_NOTICE: Mutex<Option<std::sync::mpsc::Sender<()>>> = Mutex::new(None);
pub(crate) static BLOCK_NEXT_SEND: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
pub(crate) static SEND_HELD: AtomicBool = AtomicBool::new(false);
pub(crate) static CLOSE_OVERLAPPED_SEND: AtomicBool = AtomicBool::new(false);
pub(crate) const LAB_WAIT: Duration = Duration::from_secs(5);

/// 用例隔离:机器静态 + 假符号计数器一并清零。
pub(crate) fn lab_reset() {
    *WAKE_NOTICE.lock().unwrap() = None;
    *BLOCK_NEXT_SEND.lock().unwrap() = None;
    SEND_HELD.store(false, Ordering::Release);
    CLOSE_OVERLAPPED_SEND.store(false, Ordering::Release);
    caligo_bridge::qq_entry::test_reset_state();
    FAKE_LOOP.store(0, Ordering::Release);
    FAKE_EXPECTED_ISOLATE.store(0, Ordering::Release);
    FAKE_CURRENT.store(0, Ordering::Release);
    FAKE_INIT_CALLS.store(0, Ordering::Relaxed);
    FAKE_SEND_CALLS.store(0, Ordering::Relaxed);
    FAKE_CLOSE_CALLS.store(0, Ordering::Relaxed);
    FAKE_RI_CALLS.store(0, Ordering::Relaxed);
    FAKE_INIT_CB.store(0, Ordering::Release);
    FAKE_CLOSE_CB.store(0, Ordering::Release);
    DRAIN_CALLS.store(0, Ordering::Relaxed);
}

// ---- 假 QQNT 内存布局(env / IsolateData / isolate) ----

pub(crate) struct FakeQq {
    // 保持分配存活(页校验要求真实可读内存)。
    _env_mem: Box<[u8; 0x200]>,
    _isolate_data: Box<[u8; 0x1200]>,
    _isolate_ptr: Box<u64>,
}

impl FakeQq {
    pub(crate) fn new() -> (Self, usize, usize, usize) {
        let mut env_mem = Box::new([0u8; 0x200]);
        let mut isolate_data = Box::new([0u8; 0x1200]);
        let isolate_inner = Box::into_raw(Box::new(0x1DEA_0001u64));
        let qqnt_base = 0x0000_5100_0000_0000usize; // 任意假基址
        let env = env_mem.as_mut_ptr() as usize;
        let idata = isolate_data.as_mut_ptr() as usize;
        let isolate = isolate_inner as usize;
        // 新鲜度门:env[0] = qqnt_base + 主 vtable RVA。
        let vfptr =
            (qqnt_base as u64).wrapping_add(caligo_bridge::exec::EXPECTED_VTABLE_RVA as u64);
        env_mem[0..8].copy_from_slice(&vfptr.to_le_bytes());
        // env+0xA0 = isolate;env+0xB0 = IsolateData*;IsolateData+0x11E8 = loop。
        env_mem[0xA0..0xA8].copy_from_slice(&(isolate as u64).to_le_bytes());
        env_mem[0xB0..0xB8].copy_from_slice(&(idata as u64).to_le_bytes());
        let loop_ptr = 0x5E17_0002usize;
        isolate_data[0x11E8..0x11F0].copy_from_slice(&(loop_ptr as u64).to_le_bytes());
        FAKE_LOOP.store(loop_ptr, Ordering::Release);
        FAKE_EXPECTED_ISOLATE.store(isolate, Ordering::Release);
        FAKE_CURRENT.store(isolate, Ordering::Release);
        (
            Self {
                _env_mem: env_mem,
                _isolate_data: isolate_data,
                _isolate_ptr: unsafe { Box::from_raw(isolate_inner) },
            },
            qqnt_base,
            env,
            loop_ptr,
        )
    }
}

// ---- 假符号(签名与真实导出一致;行为由静态记录) ----

pub(crate) static FAKE_LOOP: AtomicUsize = AtomicUsize::new(0);
pub(crate) static FAKE_EXPECTED_ISOLATE: AtomicUsize = AtomicUsize::new(0);
pub(crate) static FAKE_CURRENT: AtomicUsize = AtomicUsize::new(0);
pub(crate) static FAKE_INIT_CALLS: AtomicU64 = AtomicU64::new(0);
pub(crate) static FAKE_SEND_CALLS: AtomicU64 = AtomicU64::new(0);
pub(crate) static FAKE_CLOSE_CALLS: AtomicU64 = AtomicU64::new(0);
pub(crate) static FAKE_RI_CALLS: AtomicU64 = AtomicU64::new(0);
pub(crate) static FAKE_INIT_CB: AtomicUsize = AtomicUsize::new(0);
pub(crate) static FAKE_CLOSE_CB: AtomicUsize = AtomicUsize::new(0);
pub(crate) static DRAIN_CALLS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn fake_uv_handle_size(_t: i32) -> usize {
    64
}

unsafe extern "C" fn fake_isolate_get_current() -> *mut core::ffi::c_void {
    FAKE_CURRENT.load(Ordering::Acquire) as *mut core::ffi::c_void
}

unsafe extern "C" fn fake_uv_async_init(
    loop_: *mut core::ffi::c_void,
    _handle: *mut core::ffi::c_void,
    cb: unsafe extern "C" fn(*mut core::ffi::c_void),
) -> i32 {
    FAKE_INIT_CALLS.fetch_add(1, Ordering::Relaxed);
    FAKE_INIT_CB.store(cb as usize, Ordering::Release);
    if loop_ as usize != FAKE_LOOP.load(Ordering::Acquire) {
        return -1; // loop 不符:机器必须报 UvInitFailed
    }
    0
}

unsafe extern "C" fn fake_uv_loop_alive(_loop_: *mut core::ffi::c_void) -> i32 {
    1
}

unsafe extern "C" fn fake_uv_async_send(_h: *mut core::ffi::c_void) -> i32 {
    FAKE_SEND_CALLS.fetch_add(1, Ordering::Relaxed);
    if let Some(sender) = WAKE_NOTICE.lock().unwrap().as_ref() {
        sender.send(()).unwrap();
    }
    let block = BLOCK_NEXT_SEND.lock().unwrap().take();
    if let Some((entered, release)) = block {
        SEND_HELD.store(true, Ordering::SeqCst);
        let released = entered.send(()).is_ok() && release.recv_timeout(LAB_WAIT).is_ok();
        SEND_HELD.store(false, Ordering::SeqCst);
        if !released {
            return -1;
        }
    }
    0
}

unsafe extern "C" fn fake_uv_close(
    _h: *mut core::ffi::c_void,
    cb: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
) {
    CLOSE_OVERLAPPED_SEND.fetch_or(SEND_HELD.load(Ordering::SeqCst), Ordering::SeqCst);
    FAKE_CLOSE_CALLS.fetch_add(1, Ordering::Relaxed);
    if let Some(cb) = cb {
        FAKE_CLOSE_CB.store(cb as usize, Ordering::Release);
    }
}

unsafe extern "system" fn fake_request_interrupt(
    _env: *mut core::ffi::c_void,
    cb: unsafe extern "system" fn(*mut core::ffi::c_void),
    ctx: *mut core::ffi::c_void,
) {
    FAKE_RI_CALLS.fetch_add(1, Ordering::Relaxed);
    // 假宿主:同步在当前线程执行(真实为 JS 线程异步;机器以状态机等待)。
    cb(ctx);
}

pub(crate) fn fake_symbols() -> EntrySymbols {
    // 先收敛为函数指针再转 usize(函数项是 ZST,不可直接转整型)。
    type FnUvAsyncInit = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        unsafe extern "C" fn(*mut core::ffi::c_void),
    ) -> i32;
    type FnUvAsyncSend = unsafe extern "C" fn(*mut core::ffi::c_void) -> i32;
    type FnUvHandleSize = unsafe extern "C" fn(i32) -> usize;
    type FnUvClose = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
    );
    type FnIsolateGetCurrent = unsafe extern "C" fn() -> *mut core::ffi::c_void;
    type FnRequestInterrupt = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        unsafe extern "system" fn(*mut core::ffi::c_void),
        *mut core::ffi::c_void,
    );
    let a1: FnUvAsyncInit = fake_uv_async_init;
    let a2: FnUvAsyncSend = fake_uv_async_send;
    let a3: FnUvHandleSize = fake_uv_handle_size;
    let a4: FnUvClose = fake_uv_close;
    let a5: FnIsolateGetCurrent = fake_isolate_get_current;
    let a6: FnRequestInterrupt = fake_request_interrupt;
    EntrySymbols::from_raw(
        a1 as usize,
        a2 as usize,
        a3 as usize,
        a4 as usize,
        fake_uv_loop_alive as *const () as usize,
        a5 as usize,
        a6 as usize,
    )
}

pub(crate) fn report_path(tag: &str) -> String {
    let p = std::env::temp_dir().join(format!("caligo-qqentry-{tag}-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

/// 模拟 QQ loop 的一个轮转点(真实由宿主事件循环调用;LAB 由测试直调)。
pub(crate) fn pump_once() {
    let cb = FAKE_INIT_CB.load(Ordering::Acquire);
    assert!(cb != 0, "pump 未注册");
    // SAFETY: 指针来自 fake_uv_async_init 记录的回调。
    unsafe {
        let f: unsafe extern "C" fn(*mut core::ffi::c_void) =
            std::mem::transmute::<usize, unsafe extern "C" fn(*mut core::ffi::c_void)>(cb);
        f(std::ptr::null_mut());
    }
}

pub(crate) fn close_once() {
    let cb = FAKE_CLOSE_CB.load(Ordering::Acquire);
    assert!(cb != 0, "close 回调未注册");
    // SAFETY: 指针来自 fake_uv_close 记录的回调。
    unsafe {
        let f: unsafe extern "C" fn(*mut core::ffi::c_void) =
            std::mem::transmute::<usize, unsafe extern "C" fn(*mut core::ffi::c_void)>(cb);
        f(std::ptr::null_mut());
    }
}
