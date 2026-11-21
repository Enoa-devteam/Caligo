//! Research-only JS entry observations. Only the exact-build TID getter reads callback info.
//! Retained JS/native references require the bridge to remain pinned until exit.

use super::*;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const QQNT_HASH: &str = "75B19E58A123B0513E99DF4DF3EBC6524478535BA44651024D6A20821F5086E5";
const FUNCTION_NEW: &str = "?New@Function@v8@@SA?AV?$MaybeLocal@VFunction@v8@@@2@V?$Local@VContext@v8@@@2@P6AXAEBV?$FunctionCallbackInfo@VValue@v8@@@2@@ZV?$Local@VData@v8@@@2@HW4ConstructorBehavior@2@W4SideEffectType@2@@Z";
const CONTEXT_GLOBAL: &str = "?Global@Context@v8@@QEAA?AV?$Local@VObject@v8@@@2@XZ";
const CREATE_PROPERTY: &str = "?CreateDataProperty@Object@v8@@QEAA?AV?$Maybe@_N@2@V?$Local@VContext@v8@@@2@V?$Local@VName@v8@@@2@V?$Local@VValue@v8@@@2@@Z";
const INTEGER_NEW_UNSIGNED: &str = "?NewFromUnsigned@Integer@v8@@SA?AV?$Local@VInteger@v8@@@2@PEAVIsolate@2@I@Z";

type Callback = unsafe extern "C" fn(*const c_void);
type FunctionNew = unsafe extern "C" fn(*mut usize, usize, Callback, usize, i32, i32, i32);
type ContextGlobal = unsafe extern "C" fn(usize, *mut usize);
// Exact QQNT C1BBF0 uses this=RCX, sret=RDX, context=R8, key=R9,
// value on the stack, then returns the sret address (C1BE46).
type CreateProperty = unsafe extern "C" fn(usize, *mut u8, usize, usize, usize);
// Windows x64 static sret: RCX=&Local<Integer>, RDX=isolate, R8D=value.
type IntegerNewUnsigned = unsafe extern "C" fn(*mut usize, *mut c_void, u32) -> *mut usize;

struct TidGetterBinding {
    current: AtomicUsize,
    integer_new: AtomicUsize,
    isolate: AtomicPtr<c_void>,
}

impl TidGetterBinding {
    const fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
            integer_new: AtomicUsize::new(0),
            isolate: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    fn publish(&self, current: usize, integer_new: usize, isolate: *mut c_void) -> bool {
        if current == 0 || integer_new == 0 || isolate.is_null() {
            return false;
        }
        // Keep the first binding for any retained V8 function. Isolate is the
        // release/acquire publication marker for both verified addresses.
        if self
            .integer_new
            .compare_exchange(0, integer_new, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.current.store(current, Ordering::Relaxed);
            self.isolate.store(isolate, Ordering::Release);
            return true;
        }
        self.isolate.load(Ordering::Acquire) == isolate
            && self.current.load(Ordering::Relaxed) == current
            && self.integer_new.load(Ordering::Relaxed) == integer_new
    }

    // Safety: only a live V8 FunctionCallbackInfo<Value> for the verified image
    // may be passed here. The active callback supplies the handle scope.
    unsafe fn write_tid(&self, info: *const c_void) {
        let expected = self.isolate.load(Ordering::Acquire);
        if info.is_null() || expected.is_null() {
            return;
        }
        let current = self.current.load(Ordering::Relaxed);
        let integer_new = self.integer_new.load(Ordering::Relaxed);
        if current == 0 || integer_new == 0 {
            return;
        }
        // The exact-image GetStatus/GetError callbacks establish info+0x00,
        // implicit+0x08 and implicit+0x18. ValidateCallbackInfo can trap on TLS
        // mismatch, so use the exact GetCurrent/TryGetCurrent export instead.
        let implicit = unsafe { info.cast::<*mut u8>().read() };
        if implicit.is_null() {
            return;
        }
        let isolate = unsafe { implicit.add(0x08).cast::<*mut c_void>().read() };
        let current: FnGetCurrent = unsafe { std::mem::transmute(current) };
        if isolate != expected || unsafe { current() } != expected {
            return;
        }
        let integer_new: IntegerNewUnsigned = unsafe { std::mem::transmute(integer_new) };
        let tid = unsafe { GetCurrentThreadId() };
        let mut handle = 0usize;
        unsafe { integer_new(&mut handle, isolate, tid) };
        if handle != 0 {
            let tagged = unsafe { (handle as *const usize).read() };
            unsafe { implicit.add(0x18).cast::<usize>().write(tagged) };
        }
    }
}

static TID_GETTER: TidGetterBinding = TidGetterBinding::new();
static TID_GETTER_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn getter_installed() -> bool {
    TID_GETTER_INSTALLED.load(Ordering::Acquire)
}

unsafe extern "C" fn current_tid(info: *const c_void) {
    unsafe { TID_GETTER.write_tid(info) };
}

struct Slot {
    state: AtomicU32,
    tid: AtomicU32,
    kind: AtomicU32,
    sequence: AtomicU64,
}
impl Slot {
    const fn new() -> Self {
        Self { state: AtomicU32::new(0), tid: AtomicU32::new(0),
            kind: AtomicU32::new(0), sequence: AtomicU64::new(0) }
    }
}
static SLOTS: [Slot; 128] = [const { Slot::new() }; 128];
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static LOST: AtomicU32 = AtomicU32::new(0);

fn capture(kind: u32) {
    let Ok(previous) = SEQUENCE.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
        |v| (v < 128).then_some(v + 1)) else {
        LOST.fetch_or(1, Ordering::Relaxed);
        return;
    };
    for slot in &SLOTS {
        if slot.state.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            slot.tid.store(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }, Ordering::Relaxed);
            slot.kind.store(kind, Ordering::Relaxed);
            slot.sequence.store(previous + 1, Ordering::Relaxed);
            slot.state.store(2, Ordering::Release);
            return;
        }
    }
    LOST.fetch_or(1, Ordering::Relaxed);
}

// V8 supplies FunctionCallbackInfo by reference. These observers deliberately
// ignore it, leave the JS return value undefined, and perform only atomic writes.
unsafe extern "C" fn recv(_: *const c_void) { capture(1); }
unsafe extern "C" fn add(_: *const c_void) { capture(2); }
unsafe extern "C" fn update(_: *const c_void) { capture(3); }
unsafe extern "C" fn promise(_: *const c_void) { capture(4); }
unsafe extern "C" fn readback(_: *const c_void) { capture(5); }

const OBSERVERS: [(&str, Callback); 5] = [
    ("__caligo_thread_recv", recv), ("__caligo_thread_add", add),
    ("__caligo_thread_update", update), ("__caligo_thread_promise", promise),
    ("__caligo_thread_readback", readback),
];

pub(super) fn drain() {
    for slot in &SLOTS {
        if slot.state.compare_exchange(2, 3, Ordering::Acquire, Ordering::Relaxed).is_err() { continue; }
        let tid = slot.tid.load(Ordering::Relaxed);
        let sequence = slot.sequence.load(Ordering::Relaxed);
        let kind = match slot.kind.load(Ordering::Relaxed) {
            1 => "recv", 2 => "add", 3 => "update", 4 => "promise", 5 => "readback", _ => "unknown",
        };
        slot.state.store(0, Ordering::Release);
        crate::qq_entry::append_stage_simple("js_callback_thread", true, &format!(
            "sequence={sequence} callback_tid={tid} kind={kind} association=unverified source=js_entry"));
    }
    static LOSS_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if LOST.load(Ordering::Relaxed) != 0 && !LOSS_REPORTED.swap(true, Ordering::Relaxed) {
        crate::qq_entry::append_stage_simple("js_callback_thread", false, "capture_incomplete");
    }
}

/// Installs only into the exact V8 runtime already admitted by qq-entry.
/// Failure disables observation, never changes message admission or results.
pub(super) unsafe fn install(syms: &V8Symbols, isolate: *mut c_void) -> bool {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetModuleHandleExW,
        GetModuleFileNameW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN};
    use sha2::{Digest, Sha256};
    TID_GETTER_INSTALLED.store(false, Ordering::Release);
    let qqnt = unsafe { GetModuleHandleW("QQNT.dll\0".encode_utf16().collect::<Vec<_>>().as_ptr()) };
    if qqnt.is_null() || isolate.is_null() { return false; }
    let mut path = [0u16; 32768];
    let len = unsafe { GetModuleFileNameW(qqnt, path.as_mut_ptr(), path.len() as u32) } as usize;
    if len == 0 || len >= path.len() { return false; }
    let Ok(path) = String::from_utf16(&path[..len]) else { return false; };
    let Ok(bytes) = std::fs::read(path) else { return false; };
    if format!("{:X}", Sha256::digest(bytes)) != QQNT_HASH { return false; }
    let base = qqnt as usize;
    let Some(new) = (unsafe { crate::obs::export_addr(base, FUNCTION_NEW) }) else { return false; };
    let Some(global) = (unsafe { crate::obs::export_addr(base, CONTEXT_GLOBAL) }) else { return false; };
    let Some(property) = (unsafe { crate::obs::export_addr(base, CREATE_PROPERTY) }) else { return false; };
    if (new - base, global - base, property - base) != (0xC1E600, 0x4299920, 0xC1BBF0) { return false; }
    let tid_integer = if syms.get_current.checked_sub(base) == Some(0x42940B0) {
        unsafe { crate::obs::export_addr(base, INTEGER_NEW_UNSIGNED) }
            .filter(|address| address.checked_sub(base) == Some(0x4299F20))
    } else {
        None
    };
    let current: FnGetCurrent = to_fn(syms.get_current);
    let current = unsafe { current() };
    if !current.is_null() && current != isolate { return false; }
    let Some(ctx) = (unsafe { ladder_ctx(syms, isolate) }) else { return false; };
    let mut module = std::ptr::null_mut();
    if unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        recv as *const () as *const u16, &mut module) } == 0 { return false; }
    let scope_ctor: FnScopeCtor = to_fn(syms.scope_ctor);
    let scope_dtor: FnScopeDtor = to_fn(syms.scope_dtor);
    let enter: FnCtxEnterExit = to_fn(syms.ctx_enter);
    let exit: FnCtxEnterExit = to_fn(syms.ctx_exit);
    let tc_ctor: FnTcCtor = to_fn(syms.trycatch_ctor);
    let tc_dtor: FnTcDtor = to_fn(syms.trycatch_dtor);
    let new: FunctionNew = to_fn(new);
    let global: ContextGlobal = to_fn(global);
    let property: CreateProperty = to_fn(property);
    let string: FnStringNewFromUtf8 = to_fn(syms.string_new_from_utf8);
    let mut scope = [0usize; 8];
    let mut tc = [0usize; 6];
    unsafe {
        scope_ctor(scope.as_mut_ptr().cast(), isolate);
        enter(ctx as *mut c_void);
        tc_ctor(tc.as_mut_ptr().cast(), isolate);
        let mut target = 0usize;
        global(ctx, &mut target);
        let install_one = |name: &str, callback: Callback| {
            let mut function = 0usize;
            let mut key = 0usize;
            // ConstructorBehavior::kThrow=1, SideEffectType::kHasSideEffect=0.
            new(&mut function, ctx, callback, 0, 0, 1, 0);
            string(&mut key, isolate, name.as_ptr(), 0, name.len() as i32);
            let mut result = [0u8; 8];
            if function != 0 && key != 0 {
                property(target, result.as_mut_ptr(), ctx, key, function);
            }
            function != 0 && key != 0 && result[..2] == [1, 1]
        };
        let ok = target != 0
            && OBSERVERS.into_iter().all(|(name, callback)| install_one(name, callback));
        // The bridge is already pinned. Publish the verified binding before
        // Function::New can retain the getter. Aggregate readiness stays tied
        // to the original five observers.
        if ok && tid_integer.is_some_and(|addr| TID_GETTER.publish(syms.get_current, addr, isolate)) {
            TID_GETTER_INSTALLED.store(install_one("__caligo_current_tid", current_tid), Ordering::Release);
        }
        tc_dtor(tc.as_mut_ptr().cast());
        exit(ctx as *mut c_void);
        scope_dtor(scope.as_mut_ptr().cast());
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, ptr};

    const CANARY: usize = 0xA5A5_A5A5_A5A5_A5A5;
    const TAGGED: usize = 0x7654_3210;

    struct FakeIsolate {
        tagged: usize,
        tid: u32,
        factory_calls: u32,
        empty_local: bool,
    }

    thread_local! {
        static CURRENT: Cell<*mut c_void> = const { Cell::new(ptr::null_mut()) };
    }

    unsafe extern "C" fn fake_current() -> *mut c_void {
        CURRENT.get()
    }

    unsafe extern "C" fn fake_integer(out: *mut usize, isolate: *mut c_void, tid: u32) -> *mut usize {
        let state = unsafe { &mut *isolate.cast::<FakeIsolate>() };
        state.tid = tid;
        state.factory_calls += 1;
        if !state.empty_local {
            unsafe { out.write(ptr::addr_of!(state.tagged) as usize) };
        }
        out
    }

    #[test]
    fn tid_getter_writes_only_the_tagged_return_slot_with_the_calling_thread_id() {
        std::thread::spawn(|| {
            let mut state = FakeIsolate { tagged: TAGGED, tid: 0, factory_calls: 0, empty_local: false };
            let isolate = ptr::from_mut(&mut state).cast::<c_void>();
            CURRENT.set(isolate);
            let binding = TidGetterBinding::new();
            assert!(binding.publish(fake_current as FnGetCurrent as usize,
                fake_integer as IntegerNewUnsigned as usize, isolate));
            let mut implicit = [CANARY, CANARY, isolate as usize, CANARY, CANARY, CANARY];
            let info = [CANARY, unsafe { implicit.as_mut_ptr().add(1) } as usize, CANARY];
            let expected_info = info;
            let expected_tid = unsafe { GetCurrentThreadId() };
            unsafe { binding.write_tid(info.as_ptr().add(1).cast()) };
            assert_eq!(state.factory_calls, 1);
            assert_eq!(state.tid, expected_tid);
            assert_eq!(implicit, [CANARY, CANARY, isolate as usize, CANARY, TAGGED, CANARY]);
            assert_eq!(info, expected_info);
            assert_eq!(state.tagged, TAGGED);
        }).join().unwrap();
    }

    #[test]
    fn tid_getter_leaves_canaries_on_missing_or_mismatched_callback_state_and_empty_local() {
        for (scenario, expect_factory_calls) in [
            ("unpublished", 0),
            ("missing current export", 0),
            ("missing integer export", 0),
            ("null info", 0),
            ("null implicit", 0),
            ("null isolate", 0),
            ("wrong callback isolate", 0),
            ("no current isolate", 0),
            ("wrong current isolate", 0),
            ("empty local", 1),
        ] {
            let mut state = FakeIsolate { tagged: TAGGED, tid: 0, factory_calls: 0, empty_local: true };
            let isolate = ptr::from_mut(&mut state).cast::<c_void>();
            let mut other = 0usize;
            let other_isolate = ptr::from_mut(&mut other).cast::<c_void>();
            let binding = TidGetterBinding::new();
            if scenario != "unpublished" {
                let current = if scenario == "missing current export" {
                    0
                } else {
                    fake_current as FnGetCurrent as usize
                };
                let integer = if scenario == "missing integer export" {
                    0
                } else {
                    fake_integer as IntegerNewUnsigned as usize
                };
                assert_eq!(binding.publish(current, integer, isolate), current != 0 && integer != 0);
            }
            CURRENT.set(match scenario {
                "no current isolate" => ptr::null_mut(),
                "wrong current isolate" => other_isolate,
                _ => isolate,
            });
            let callback_isolate = match scenario {
                "null isolate" => ptr::null_mut(),
                "wrong callback isolate" => other_isolate,
                _ => isolate,
            };
            let mut implicit = [CANARY, CANARY, callback_isolate as usize, CANARY, CANARY, CANARY];
            let expected_implicit = implicit;
            let implicit_ptr = if scenario == "null implicit" {
                ptr::null_mut()
            } else {
                unsafe { implicit.as_mut_ptr().add(1) }
            };
            let info = [CANARY, implicit_ptr as usize, CANARY];
            let expected_info = info;
            let info_ptr = if scenario == "null info" {
                ptr::null()
            } else {
                unsafe { info.as_ptr().add(1).cast() }
            };
            unsafe { binding.write_tid(info_ptr) };
            assert_eq!(implicit, expected_implicit, "{scenario}");
            assert_eq!(info, expected_info, "{scenario}");
            assert_eq!(state.factory_calls, expect_factory_calls, "{scenario}");
        }
        CURRENT.set(ptr::null_mut());
    }

    #[test]
    fn capture_is_bounded_and_records_the_calling_os_thread() {
        for slot in &SLOTS { slot.state.store(0, Ordering::Relaxed); }
        LOST.store(0, Ordering::Relaxed);
        let expected = std::thread::spawn(|| {
            let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
            unsafe { promise(std::ptr::null()); }
            tid
        }).join().unwrap();
        assert_eq!(SLOTS[0].tid.load(Ordering::Acquire), expected);
        assert_eq!(SLOTS[0].kind.load(Ordering::Relaxed), 4);
        for _ in 1..128 { capture(2); }
        capture(3);
        assert_eq!(SLOTS.iter().filter(|s| s.state.load(Ordering::Acquire) == 2).count(), 128);
        assert_ne!(LOST.load(Ordering::Relaxed), 0);
        for slot in &SLOTS { slot.state.store(0, Ordering::Relaxed); }
    }
}
