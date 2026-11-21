//! K4-D8 V8 执行阶梯(仅 research 构建):在 owner 轮转点执行固定语义 JS。
//!
//! ABI 全部按 K2-03 §12.2.1 反汇编实证规则(Local/MaybeLocal 的 sret 约定):
//! - 静态函数:sret 在 RCX,参数自 RDX 后移;
//! - 成员函数:this 在 RCX,sret 在 RDX,其余自 R8 起;
//! - ScriptOrigin(0x28)零构造安全;Run 用双参重载、data 传空 Local;
//! - 入口拒绝空 isolate 与非零、异 isolate current。轮转点 current=0 只在
//!   `GetEnteredOrMicrotaskContext`/`GetIncumbentContext` 至少返回一个有效 Local 时放行。
//!   这不等于允许无上下文执行；owner/新鲜 env 检查仍由上层入口负责。
//!
//! 符号全部可注入(LAB 假机器驱动同一阶梯);`resolve(qqnt)` 为现场路线。

use core::ffi::c_void;

use crate::qq_entry::append_stage;

#[cfg(feature = "research")]
mod callback_threads;

const NAME_HANDLE_SCOPE_CTOR: &str = "??0HandleScope@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_HANDLE_SCOPE_DTOR: &str = "??1HandleScope@v8@@QEAA@XZ";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_ISOLATE_ENTERED_CTX: &str =
    "?GetEnteredOrMicrotaskContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_ISOLATE_INCUMBENT_CTX: &str =
    "?GetIncumbentContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_STRING_NEW_FROM_UTF8: &str = "?NewFromUtf8@String@v8@@SA?AV?$MaybeLocal@VString@v8@@@2@PEAVIsolate@2@PEBDW4NewStringType@2@H@Z";
const NAME_SCRIPT_COMPILE: &str = "?Compile@Script@v8@@SA?AV?$MaybeLocal@VScript@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VString@v8@@@2@PEAVScriptOrigin@2@@Z";
const NAME_SCRIPT_RUN: &str = "?Run@Script@v8@@QEAA?AV?$MaybeLocal@VValue@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VData@v8@@@2@@Z";
const NAME_UTF8_CTOR: &str = "??0Utf8Value@String@v8@@QEAA@PEAVIsolate@2@V?$Local@VValue@v8@@@2@@Z";
const NAME_UTF8_DTOR: &str = "??1Utf8Value@String@v8@@QEAA@XZ";
const NAME_UTF8_DEREF: &str = "??DUtf8Value@String@v8@@QEAAPEADXZ";
const NAME_CTX_ENTER: &str = "?Enter@Context@v8@@QEAAXXZ";
const NAME_CTX_EXIT: &str = "?Exit@Context@v8@@QEAAXXZ";
const NAME_TRYCATCH_CTOR: &str = "??0TryCatch@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_TRYCATCH_DTOR: &str = "??1TryCatch@v8@@QEAA@XZ";
const NAME_TRYCATCH_CAUGHT: &str = "?HasCaught@TryCatch@v8@@QEBA_NXZ";
const NAME_TRYCATCH_EXC: &str = "?Exception@TryCatch@v8@@QEBA?AV?$Local@VValue@v8@@@2@XZ";

type FnScopeCtor = unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnScopeDtor = unsafe extern "C" fn(this: *mut c_void);
type FnGetCurrent = unsafe extern "C" fn() -> *mut c_void;
type FnIsolateCtx = unsafe extern "C" fn(isolate: *mut c_void, sret: *mut usize);
type FnStringNewFromUtf8 = unsafe extern "C" fn(
    sret: *mut usize,
    isolate: *mut c_void,
    data: *const u8,
    ty: i32,
    len: i32,
);
type FnScriptCompile =
    unsafe extern "C" fn(sret: *mut usize, ctx: usize, src: usize, origin: *mut c_void);
type FnScriptRun = unsafe extern "C" fn(this: usize, sret: *mut usize, ctx: usize, data: usize);
type FnUtf8Ctor =
    unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void, value: usize) -> *mut c_void;
type FnUtf8Dtor = unsafe extern "C" fn(this: *mut c_void);
type FnUtf8Deref = unsafe extern "C" fn(this: *mut c_void) -> *const u8;
type FnCtxEnterExit = unsafe extern "C" fn(this: *mut c_void);
type FnTcCtor = unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnTcDtor = unsafe extern "C" fn(this: *mut c_void);
type FnTcCaught = unsafe extern "C" fn(this: *const c_void) -> bool;
type FnTcExc = unsafe extern "C" fn(this: *const c_void, sret: *mut usize);

#[derive(Debug, Clone, Copy)]
pub struct V8Symbols {
    pub scope_ctor: usize,
    pub scope_dtor: usize,
    pub get_current: usize,
    pub entered_ctx: usize,
    pub incumbent_ctx: usize,
    pub string_new_from_utf8: usize,
    pub script_compile: usize,
    pub script_run: usize,
    pub utf8_ctor: usize,
    pub utf8_dtor: usize,
    pub utf8_deref: usize,
    pub ctx_enter: usize,
    pub ctx_exit: usize,
    pub trycatch_ctor: usize,
    pub trycatch_dtor: usize,
    pub trycatch_caught: usize,
    pub trycatch_exc: usize,
}

impl V8Symbols {
    pub fn resolve(qqnt: usize, report: &str) -> Option<Self> {
        let get = |name: &str| -> Option<usize> {
            // SAFETY: obs::export_addr 只读导出表。
            let a = unsafe { crate::obs::export_addr(qqnt, name) };
            if a.is_none() {
                append_stage(
                    report,
                    "v8_resolve",
                    false,
                    &format!("export missing: {name}"),
                );
            }
            a
        };
        Some(Self {
            scope_ctor: get(NAME_HANDLE_SCOPE_CTOR)?,
            scope_dtor: get(NAME_HANDLE_SCOPE_DTOR)?,
            get_current: get(NAME_ISOLATE_GETCURRENT)?,
            entered_ctx: get(NAME_ISOLATE_ENTERED_CTX)?,
            incumbent_ctx: get(NAME_ISOLATE_INCUMBENT_CTX)?,
            string_new_from_utf8: get(NAME_STRING_NEW_FROM_UTF8)?,
            script_compile: get(NAME_SCRIPT_COMPILE)?,
            script_run: get(NAME_SCRIPT_RUN)?,
            utf8_ctor: get(NAME_UTF8_CTOR)?,
            utf8_dtor: get(NAME_UTF8_DTOR)?,
            utf8_deref: get(NAME_UTF8_DEREF)?,
            ctx_enter: get(NAME_CTX_ENTER)?,
            ctx_exit: get(NAME_CTX_EXIT)?,
            trycatch_ctor: get(NAME_TRYCATCH_CTOR)?,
            trycatch_dtor: get(NAME_TRYCATCH_DTOR)?,
            trycatch_caught: get(NAME_TRYCATCH_CAUGHT)?,
            trycatch_exc: get(NAME_TRYCATCH_EXC)?,
        })
    }

    /// LAB 注入。
    pub fn from_raw(parts: [usize; 17]) -> Self {
        Self {
            scope_ctor: parts[0],
            scope_dtor: parts[1],
            get_current: parts[2],
            entered_ctx: parts[3],
            incumbent_ctx: parts[4],
            string_new_from_utf8: parts[5],
            script_compile: parts[6],
            script_run: parts[7],
            utf8_ctor: parts[8],
            utf8_dtor: parts[9],
            utf8_deref: parts[10],
            ctx_enter: parts[11],
            ctx_exit: parts[12],
            trycatch_ctor: parts[13],
            trycatch_dtor: parts[14],
            trycatch_caught: parts[15],
            trycatch_exc: parts[16],
        }
    }

    fn to_fn<T: Copy>(a: usize) -> T {
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<usize>());
        // SAFETY: 调用方保证地址与 T 的 ABI 匹配(现场:导出表;LAB:注入)。
        unsafe { std::ptr::read(core::ptr::addr_of!(a).cast::<T>()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8Error {
    /// 零 current 或 current 与 isolate 不符:轮转点不具备 JS 执行条件。
    NoCurrentContext,
    StringEmpty,
    CompileFailed,
    RunEmpty,
    Utf8Failed,
}

fn to_fn<T: Copy>(a: usize) -> T {
    V8Symbols::to_fn::<T>(a)
}

/// 取执行上下文:entered 优先,incumbent 兜底;皆空 → None。
unsafe fn ladder_ctx(syms: &V8Symbols, isolate: *mut c_void) -> Option<usize> {
    // SAFETY: 符号 ABI 见类型别名;sret 槽在栈上。
    unsafe {
        let entered: FnIsolateCtx = to_fn(syms.entered_ctx);
        let mut v = 0usize;
        entered(isolate, &mut v);
        if v != 0 {
            return Some(v);
        }
        let incumbent: FnIsolateCtx = to_fn(syms.incumbent_ctx);
        let mut v2 = 0usize;
        incumbent(isolate, &mut v2);
        if v2 != 0 {
            return Some(v2);
        }
        None
    }
}

/// 在轮转点执行 JS 并取回字符串。
///
/// 检查顺序:零/异 current 拒绝(先于 HandleScope)→ ladder ctx →
/// HandleScope → String → Compile → Run → Utf8Value → 回收。
pub unsafe fn exec_script(
    syms: &V8Symbols,
    isolate: *mut c_void,
    js: &str,
) -> Result<String, V8Error> {
    // SAFETY: 符号 ABI 见类型别名;各 sret 槽在栈上;缓冲生命周期受控。
    unsafe {
        // Do not call even the context getters with a missing isolate. The QQ
        // owner bootstrap normally publishes a non-null pointer before ready,
        // but this boundary is also used by injected LAB symbols and must fail
        // closed if that invariant is ever broken.
        if isolate.is_null() {
            return Err(V8Error::NoCurrentContext);
        }
        let get_current: FnGetCurrent = to_fn(syms.get_current);
        let current = (get_current)() as usize;
        // K2-03 r3 实证语义(uv 轮转点):GetCurrent 经常合法为 0,
        // entered/incumbent 才是轮转点的有效上下文。拒绝条件 = current 与
        // isolate 不一致(非 0 且不等)+ 阶梯上下文为空。§5.1 的"零 current
        // 不能自行解释为可进入"由此满足:仍须通过上下文阶梯方可进入,
        // 且 env 新鲜度门 + owner 线程检查在阶梯之前(QqOwnerAdapter)。
        if current != 0 && current != isolate as usize {
            return Err(V8Error::NoCurrentContext);
        }
        let Some(ctx) = ladder_ctx(syms, isolate) else {
            return Err(V8Error::NoCurrentContext);
        };
        let scope_ctor: FnScopeCtor = to_fn(syms.scope_ctor);
        let scope_dtor: FnScopeDtor = to_fn(syms.scope_dtor);
        let mut scope = [0usize; 8];
        scope_ctor(scope.as_mut_ptr().cast(), isolate);

        // Context::Enter(V8 规范:Script::Run 需要已进入的上下文;安静轮转点
        // 上 QQ 侧可能未持有 entered 状态 → 不 Enter 直接 Run 得到空 Local,
        // D9 现场实证 RunEmpty)。Exit 严格逆序。
        let ctx_enter: FnCtxEnterExit = to_fn(syms.ctx_enter);
        let ctx_exit: FnCtxEnterExit = to_fn(syms.ctx_exit);
        (ctx_enter)(ctx as *mut c_void);

        // TryCatch:捕获未捕获 JS 异常并读取消息(RunEmpty 确切原因)。
        let tc_ctor: FnTcCtor = to_fn(syms.trycatch_ctor);
        let tc_dtor: FnTcDtor = to_fn(syms.trycatch_dtor);
        // QQNT.dll SHA-256 75B19E58A123B0513E99DF4DF3EBC6524478535BA44651024D6A20821F5086E5
        // writes TryCatch state at this+0x28; reserve the full 0x30-byte storage.
        let mut tc = [0usize; 6];
        tc_ctor(tc.as_mut_ptr().cast(), isolate);

        let r = exec_in_scope(syms, isolate, ctx, js, &mut scope);

        // RunEmpty + TryCatch 已捕获 → 读取确切 JS 异常消息。
        let caught: FnTcCaught = to_fn(syms.trycatch_caught);
        if r == Err(V8Error::RunEmpty) && (caught)(tc.as_ptr().cast() as *const c_void) {
            let exc: FnTcExc = to_fn(syms.trycatch_exc);
            let mut exc_local = 0usize;
            exc(tc.as_ptr().cast() as *const c_void, &mut exc_local);
            if exc_local != 0 {
                let utf8_ctor: FnUtf8Ctor = to_fn(syms.utf8_ctor);
                let utf8_dtor: FnUtf8Dtor = to_fn(syms.utf8_dtor);
                let utf8_deref: FnUtf8Deref = to_fn(syms.utf8_deref);
                let mut u8b = [0usize; 3];
                utf8_ctor(u8b.as_mut_ptr().cast(), isolate, exc_local);
                let p = utf8_deref(u8b.as_mut_ptr().cast());
                if !p.is_null() {
                    let msg = core::str::from_utf8(core::slice::from_raw_parts(
                        p,
                        strlen_bounded(p, 4096),
                    ))
                    .unwrap_or("?")
                    .to_string();
                    crate::qq_entry::append_stage_simple(
                        "v8_exc",
                        false,
                        &format!("uncaught: {}", &msg[..msg.len().min(400)]),
                    );
                }
                utf8_dtor(u8b.as_mut_ptr().cast());
            }
        }
        tc_dtor(tc.as_mut_ptr().cast());

        // RunEmpty 诊断(D9 现场):区分"上下文不可执行"与"脚本自身问题"。
        if matches!(r, Err(V8Error::RunEmpty)) {
            let probe = exec_in_scope(syms, isolate, ctx, "1+1", &mut scope);
            crate::qq_entry::append_stage_simple(
                "v8_probe",
                probe.is_ok(),
                &format!(
                    "probe={:?} script_len={}",
                    probe.as_ref().map(|s| s.as_str()).unwrap_or("<empty>"),
                    js.len()
                ),
            );
        }

        (ctx_exit)(ctx as *mut c_void);
        scope_dtor(scope.as_mut_ptr().cast());
        r
    }
}

/// SAFETY: 须持有效 HandleScope;ctx 经阶梯取得。
unsafe fn exec_in_scope(
    syms: &V8Symbols,
    isolate: *mut c_void,
    ctx: usize,
    js: &str,
    _scope: &mut [usize],
) -> Result<String, V8Error> {
    // SAFETY: 同上。
    unsafe {
        let new_utf8: FnStringNewFromUtf8 = to_fn(syms.string_new_from_utf8);
        let compile: FnScriptCompile = to_fn(syms.script_compile);
        let run: FnScriptRun = to_fn(syms.script_run);
        let utf8_ctor: FnUtf8Ctor = to_fn(syms.utf8_ctor);
        let utf8_dtor: FnUtf8Dtor = to_fn(syms.utf8_dtor);
        let utf8_deref: FnUtf8Deref = to_fn(syms.utf8_deref);

        let mut src_local = 0usize;
        new_utf8(
            &mut src_local,
            isolate,
            js.as_ptr(),
            0, // kNormal
            js.len() as i32,
        );
        if src_local == 0 {
            return Err(V8Error::StringEmpty);
        }
        // ScriptOrigin(0x28 字节)零构造安全(K2-03 §12.1)。
        let mut origin = [0usize; 5];
        let mut script_local = 0usize;
        compile(
            &mut script_local,
            ctx,
            src_local,
            origin.as_mut_ptr().cast(),
        );
        if script_local == 0 {
            return Err(V8Error::CompileFailed);
        }
        let mut result_local = 0usize;
        run(
            script_local,
            &mut result_local,
            ctx,
            0, /*空 data Local*/
        );
        if result_local == 0 {
            return Err(V8Error::RunEmpty);
        }
        let mut utf8 = [0usize; 3];
        utf8_ctor(utf8.as_mut_ptr().cast(), isolate, result_local);
        let p = utf8_deref(utf8.as_mut_ptr().cast());
        if p.is_null() {
            utf8_dtor(utf8.as_mut_ptr().cast());
            return Err(V8Error::Utf8Failed);
        }
        let n = strlen_bounded(p, 1 << 20);
        let out = core::str::from_utf8(core::slice::from_raw_parts(p, n))
            .unwrap_or_default()
            .to_string();
        utf8_dtor(utf8.as_mut_ptr().cast());
        Ok(out)
    }
}

fn strlen_bounded(p: *const u8, cap: usize) -> usize {
    // SAFETY: Utf8Value 缓冲 NUL 结尾;上限防失控。
    unsafe {
        let mut n = 0usize;
        while n < cap && *p.add(n) != 0 {
            n += 1;
        }
        n
    }
}

// —— D8 固定语义脚本(源自 js/resident-*.js,K3 现场验证版本;移植为常量) ——

/// 注册监听器(活会话扫描 + addKernelMsgListener;每会话代次一次)。
/// 返回 "ARMED sid=<n> lidRet=<id>" / "ALREADY ..." / "ERR:..."。
pub const D8_START_JS: &str = r"
(function () {
  try {
    var G = globalThis;
    if (G.__caligo_res) {
      if (G.__caligo_res.stopped) return 'ERR:stopped_listener_retained';
      G.__caligo_d8 = G.__caligo_res; return 'ALREADY sid=' + G.__caligo_res.sid;
    }
    var q = process._linkedBinding('QQNT');
    var S = q.NodeIQQNTWrapperSession;
    var live = null, sidName = null;
    for (var i = 0; i < 10; i++) {
      try {
        var ls = S.getNTWrapperSession('nt_' + i);
        if (ls && typeof ls === 'object') {
          var sid = ls.getSessionId();
          if (sid && String(sid) !== '0') {
            var ms = ls.getMsgService();
            if (ms && ms.sendMsg) { live = ls; sidName = 'nt_' + i; break; }
          }
        }
      } catch (e) {}
    }
    if (!live) return 'ERR:no live session';
    var ms = live.getMsgService();
    var R = { sid: sidName, session: live, ms: ms, recv: [], recvBytes: 0,
      rejected: 0, overflow: 0, imageShapes: [], listenerId: null, sendResults: {}, sends: {} };
    var traceText = typeof caligoSendTrace !== 'undefined' && caligoSendTrace === true;
    R.textSendTrace = [];
    var traceGeneration = 0;
    if (traceText) {
      try {
        var lastGeneration = G.__caligo_text_trace_generation;
        if (lastGeneration === undefined) lastGeneration = 0;
        if (Number.isSafeInteger(lastGeneration) && lastGeneration >= 0 && lastGeneration < 2147483647) {
          traceGeneration = lastGeneration + 1;
          G.__caligo_text_trace_generation = traceGeneration;
          if (G.__caligo_text_trace_generation !== traceGeneration) traceGeneration = 0;
        }
      } catch (_) { traceGeneration = 0; }
    }
    // The JS order belongs to this listener only. Polling never resets it.
    var observationOrder = 0, observationPhase = 'active';
    var currentTid = null;
    if (traceText && typeof caligoTidGetterReady !== 'undefined' && caligoTidGetterReady === true) {
      try {
        var installedTidGetter = G.__caligo_current_tid;
        if (typeof installedTidGetter === 'function') currentTid = installedTidGetter;
      } catch (_) {}
    }
    var observeCallback = function(kind, association, key, pending) {
      try {
        if (!traceText || R.syntheticStopProbe) return;
        var seq = pending ? pending.traceSeq : 0;
        if (!traceGeneration || observationOrder >= 128 ||
            (pending && (!Number.isSafeInteger(seq) || seq < 1 || seq > 2147483647))) {
          R.callbackObservationsOverflow = true;
          return;
        }
        var callbackTid = null;
        try {
          var sampledTid = currentTid ? currentTid() : null;
          if (Number.isInteger(sampledTid) && sampledTid > 0 && sampledTid <= 4294967295) callbackTid = sampledTid;
        } catch (_) {}
        var row = {generation:traceGeneration, order:++observationOrder,
          seq:seq, kind:kind, phase:observationPhase, association:association,
          current:pending ? R.sends[key] === pending : null, callback_tid:callbackTid};
        R.callbackObservations.push(row);
        return row;
      } catch (_) { try { R.callbackObservationsOverflow = true; } catch (_) {} }
    };
    // Capture only the text closure's business fields. References and values
    // stay in this invocation; the observation row stores a fixed label only.
    var captureTextClosureState = function(key, pending) {
      if (R.stopped !== true || G.__caligo_d8 !== R || !R.sends || !R.sendResults ||
          !Array.isArray(R.recv) || !Array.isArray(R.imageShapes)) return null;
      var result = R.sendResults[key], readback = pending.readback;
      return [R.sends, R.sends[key], R.sendResults, result,
        result && result.status, result && result.msgId, result && result.reason,
        pending.promiseDone, pending.promiseStatus, pending.unknownReason, pending.failed,
        pending.timedOut, pending.done, pending.addId, pending.updateId, pending.readbackStarted,
        readback, readback && readback.status, readback && readback.msgId, readback && readback.reason,
        R.recv, R.recv.length, R.recvBytes, R.rejected, R.overflow, R.imageShapes, R.imageShapes.length];
    };
    var beginTextClosureObservation = function(kind, key, pending) {
      var row = observeCallback(kind, 'closure', key, pending);
      if (!row || row.phase !== 'stopped') return null;
      row.text_state = 'unverified';
      try {
        var before = captureTextClosureState(key, pending);
        if (!before) return null;
        return function () {
          try {
            var after = captureTextClosureState(key, pending);
            if (after) row.text_state = before.every(function(value, index) {
              return Object.is(value, after[index]);
            }) ? 'unchanged' : 'changed';
          } catch (_) {}
        };
      } catch (_) { return null; }
    };
    if (traceText) {
      R.beginTextClosureObservation = beginTextClosureObservation;
      R.callbackObservations = [];
      R.callbackObservationsOverflow = traceGeneration === 0;
      R.observeCallback = observeCallback;
      R.observeStopPhase = function(phase, kind) {
        observationPhase = phase;
        observeCallback(kind, 'none');
      };
    }
    // Capture the observers once per listener generation. The native no-argument
    // functions only record the current OS thread; their return value is ignored.
    var threadObservers = null;
    if (traceText && typeof caligoThreadObserversReady !== 'undefined' && caligoThreadObserversReady === true) {
      try {
        var candidates = {
          recv:G.__caligo_thread_recv, add:G.__caligo_thread_add,
          update:G.__caligo_thread_update, promise:G.__caligo_thread_promise,
          readback:G.__caligo_thread_readback
        };
        if (Object.keys(candidates).every(function(k) { return typeof candidates[k] === 'function'; }))
          threadObservers = candidates;
      } catch (_) {}
    }
    var noteCallbackThread = R.noteCallbackThread = function(kind) {
      try {
        if (R.syntheticStopProbe || !threadObservers) return;
        var observer = threadObservers[kind];
        if (typeof observer === 'function') observer();
      } catch (_) {}
    };
    R.textTraceGeneration = traceGeneration;
    R.textTraceSequence = 0;
    R.textTraceOverflow = traceText && traceGeneration === 0;
    R.allocateTextTrace = function() {
      try {
        if (!traceText) return 0;
        if (!traceGeneration || R.textTraceSequence >= 2147483647) { R.textTraceOverflow = true; return 0; }
        return ++R.textTraceSequence;
      } catch (_) { try { R.textTraceOverflow = true; } catch (_) {} return 0; }
    };
    R.noteTextSend = function(stage, seq) {
      try {
        if (!traceText) return;
        if (!traceGeneration || !Number.isSafeInteger(seq) || seq <= 0 || R.textSendTrace.length >= 32) {
          R.textTraceOverflow = true; return;
        }
        R.textSendTrace.push({seq:seq, stage:stage});
      } catch (_) { try { R.textTraceOverflow = true; } catch (_) {} }
    };
    var voiceAccount = typeof caligoVoiceAccount === 'string' ? caligoVoiceAccount : null;
    R.voiceAccount = voiceAccount;
    // The pinned wrapper serializes absent numeric mention IDs as '0' and the optional NT UID as null.
    var emptyNumericMention = function (id) {
      return id === undefined || id === '' || (voiceAccount !== null && id === '0');
    };
    var emptyNtMention = function (id) {
      return id === undefined || id === '' || (voiceAccount !== null && id === null);
    };
    var utf8Bytes = function (s) { return Buffer.byteLength(s, 'utf8'); };
    var settleSendResult = function (R, key, pending) {
      if (R.stopped || !pending || R.sends[key] !== pending || pending.done ||
          !pending.promiseDone || !pending.readback) return;
      if (!pending.voice && !pending.image) R.noteTextSend('settled', pending.traceSeq);
      if (pending.ambiguous || pending.promiseStatus !== 'accepted' || pending.readback.status !== 'readback') {
        R.sendResults[key] = {status:'unknown', reason:pending.ambiguous ? 'multiple_add_send_ids' :
          (pending.promiseStatus !== 'accepted' ? (pending.unknownReason || 'promise_event_conflict') : pending.readback.reason)};
      } else if ((pending.voice || pending.image) && !push('add_send', pending.readback.record, true)) {
        R.sendResults[key] = {status:'unknown', reason:'verified_voice_projection_rejected'};
      } else {
        R.sendResults[key] = {status:'done', msgId:pending.readback.msgId};
      }
      pending.done = true;
      if (R.sends[key] === pending) delete R.sends[key];
    };
    // Diagnostic failures must never affect callback admission. Only fixed labels and booleans escape.
    var noteImageShape = function (src, m) {
      try {
        if (!voiceAccount || R.imageShapes.length >= 8 || !m || !Array.isArray(m.elements)) return;
        var e = m.elements.slice(0, 128).find(function (e) {
          return e && (e.elementType === 2 || e.elementType === '2');
        });
        if (!e) return;
        var p = e.picElement || {}, size = p.fileSize;
        if (typeof size === 'string' && /^[1-9][0-9]{0,7}$/.test(size)) size = Number(size);
        R.imageShapes.push({source:src, chat:m.chatType === 1 ? 'private' : (m.chatType === 2 ? 'group' : 'other'),
          arity:m.elements.length === 1 ? 'single' : 'multiple', numeric_element_type:e.elementType === 2,
          pic_type:p.picType === 1001 ? 'png' : (p.picType === 1000 ? 'default' : 'other'),
          subtype:p.picSubType === 0 ? 'ordinary' : 'other',
          wire_strings_valid:['msgId','msgSeq','msgRandom','msgTime','guildId','peerUid','peerUin','senderUid','senderUin']
            .every(function (f) { return typeof m[f] === 'string'; }),
          message_types_valid:Number.isInteger(m.msgType) && m.msgType >= 0 && Number.isInteger(m.subMsgType) && m.subMsgType >= 0,
          optional_fields_valid:(m.clientSeq === undefined || typeof m.clientSeq === 'string') &&
            (m.sendStatus === undefined || Number.isInteger(m.sendStatus)),
          self_sent:m.senderUin === voiceAccount,
          peer_matches:m.chatType === 1 ? m.senderUid === m.peerUid && m.senderUin === m.peerUin : m.peerUid === m.peerUin,
          element_id_valid:typeof e.elementId === 'string' && /^[0-9]{1,128}$/.test(e.elementId) && /[1-9]/.test(e.elementId),
          size_valid:Number.isInteger(size) && size >= 33 && size <= 16777216,
          dimensions_valid:Number.isInteger(p.picWidth) && p.picWidth >= 1 && p.picWidth <= 32768 &&
            Number.isInteger(p.picHeight) && p.picHeight >= 1 && p.picHeight <= 32768 && p.picWidth * p.picHeight <= 100000000,
          md5_valid:typeof p.md5HexStr === 'string' && /^[0-9a-fA-F]{32}$/.test(p.md5HexStr), text_present:!!e.textElement});
      } catch (_) {}
    };
    var push = function (src, m, verifiedVoice) {
      if (!verifiedVoice) noteImageShape(src, m);
      try {
        if (!m || typeof m !== 'object' || !Array.isArray(m.elements) ||
            !m.elements.length || m.elements.length > 128) { R.rejected++; return; }
        var required = ['msgId','msgSeq','msgRandom','msgTime','guildId',
          'peerUid','peerUin','senderUid','senderUin'];
        for (var j = 0; j < required.length; j++) {
          if (typeof m[required[j]] !== 'string') { R.rejected++; return; }
        }
        if ((m.chatType !== 1 && m.chatType !== 2) ||
            !Number.isInteger(m.msgType) || m.msgType < 0 ||
            !Number.isInteger(m.subMsgType) || m.subMsgType < 0 ||
            (m.clientSeq !== undefined && typeof m.clientSeq !== 'string') ||
            (m.sendStatus !== undefined && !Number.isInteger(m.sendStatus))) { R.rejected++; return; }
        // The unverified self-sent callback only feeds D9 correlation.
        if (!verifiedVoice && (src === 'add_send' || src === 'update') && voiceAccount &&
            m.senderUin === voiceAccount &&
            m.elements.length === 1 && m.elements[0] && (m.elements[0].elementType === 4 || m.elements[0].elementType === 2)) {
          correlateSend(R, m, src);
          return;
        }
        var elements = [], textBytes = 0;
        for (var j = 0; j < m.elements.length; j++) {
          var e = m.elements[j], t = e && e.textElement;
          if (e && e.elementType === 2 && voiceAccount && m.elements.length === 1 &&
              (verifiedVoice || (src === 'recv' && m.senderUin !== voiceAccount &&
              ((m.chatType === 1 && m.senderUid === m.peerUid && m.senderUin === m.peerUin) ||
               (m.chatType === 2 && m.peerUid === m.peerUin))))) {
            var pic = e.picElement, size = pic && pic.fileSize;
            if (typeof size === 'string' && /^[1-9][0-9]{0,7}$/.test(size)) size = Number(size);
            if (t || !pic || typeof e.elementId !== 'string' || !/^[0-9]{1,128}$/.test(e.elementId) ||
                !/[1-9]/.test(e.elementId) || (pic.picType !== 1001 && !(verifiedVoice && pic.picType === 1000)) || pic.picSubType !== 0 ||
                !Number.isInteger(size) || size < 33 || size > 16777216 ||
                !Number.isInteger(pic.picWidth) || pic.picWidth < 1 || pic.picWidth > 32768 ||
                !Number.isInteger(pic.picHeight) || pic.picHeight < 1 || pic.picHeight > 32768 ||
                pic.picWidth * pic.picHeight > 100000000 || typeof pic.md5HexStr !== 'string' ||
                !/^[0-9a-fA-F]{32}$/.test(pic.md5HexStr)) { R.rejected++; return; }
            elements.push({elementType:2,image:{chat_type:m.chatType,peer_uid:m.peerUid,
              sender_uin:m.senderUin,message_id:m.msgId,element_id:e.elementId,size:size,
              md5:pic.md5HexStr.toLowerCase(),width:pic.picWidth,height:pic.picHeight,pic_type:pic.picType}});
            continue;
          }
          if (e && e.elementType === 4 && voiceAccount && m.elements.length === 1 &&
              (verifiedVoice || (src === 'recv' && m.senderUin !== voiceAccount &&
                ((m.chatType === 1 && m.senderUid === m.peerUid && m.senderUin === m.peerUin) ||
                 (m.chatType === 2 && m.peerUid === m.peerUin))))) {
            var ptt = e.pttElement;
            if (t || !ptt || typeof e.elementId !== 'string' || !/^[0-9]{1,128}$/.test(e.elementId) ||
                !/[1-9]/.test(e.elementId) || typeof ptt.fileSize !== 'string' ||
                !/^[1-9][0-9]{0,7}$/.test(ptt.fileSize) || Number(ptt.fileSize) > 16777216 ||
                typeof ptt.md5HexStr !== 'string' || !/^[0-9a-fA-F]{32}$/.test(ptt.md5HexStr)) { R.rejected++; return; }
            elements.push({elementType:4, voice:{element_id:e.elementId,
              size:Number(ptt.fileSize),md5:ptt.md5HexStr.toLowerCase()}});
            continue;
          }
          if (!e || e.elementType !== 1 || !t || typeof t.content !== 'string' || t.atType !== 0 ||
              !emptyNumericMention(t.atUid) || !emptyNtMention(t.atNtUid) ||
              !emptyNumericMention(t.atTinyId)) { R.rejected++; return; }
          textBytes += utf8Bytes(t.content);
          if (textBytes > 49152) { R.overflow++; return; }
          elements.push({elementType:1, textElement:{content:t.content, atType:0,
            atUid:'', atNtUid:'', atTinyId:''}});
        }
        var record = {msgId:m.msgId, msgSeq:m.msgSeq, msgRandom:m.msgRandom,
          msgTime:m.msgTime, clientSeq:m.clientSeq, guildId:m.guildId, chatType:m.chatType,
          msgType:m.msgType, subMsgType:m.subMsgType,
          peerUid:m.peerUid, peerUin:m.peerUin, senderUid:m.senderUid, senderUin:m.senderUin,
          sendStatus:m.sendStatus, elements:elements};
        if (!verifiedVoice && (src === 'add_send' || src === 'update')) correlateSend(R, m, src);
        var item = {source:src, observed_at_unix_ms:Date.now(), record:record};
        var bytes = utf8Bytes(JSON.stringify(item));
        if (bytes > 65536 || R.recv.length >= 128 || R.recvBytes + bytes > 393216) {
          R.overflow++; return;
        }
        R.recv.push(item); R.recvBytes += bytes;
        return true;
      } catch (err) { R.rejected++; }
    };
    var matchesSendElement = function (record, send) {
      if (!record || !Array.isArray(record.elements) || record.elements.length !== 1 ||
          !record.elements[0]) return false;
      var element = record.elements[0];
      if (send.image) {
        var pic=element.picElement;
        return element.elementType === 2 && !element.textElement && pic &&
          typeof element.elementId === 'string' && /^[0-9]{1,128}$/.test(element.elementId) && /[1-9]/.test(element.elementId) &&
          (pic.picType === 1000 || pic.picType === 1001) && pic.picSubType === 0 &&
          (pic.fileSize === String(send.image.size) || pic.fileSize === send.image.size) &&
          pic.picWidth === send.image.width && pic.picHeight === send.image.height &&
          typeof pic.md5HexStr === 'string' && pic.md5HexStr.toLowerCase() === send.image.md5;
      }
      if (!send.voice) return element.textElement && element.textElement.content === send.text;
      var ptt = element.pttElement;
      return element.elementType === 4 && !element.textElement && ptt &&
        typeof element.elementId === 'string' && /^[0-9]{1,128}$/.test(element.elementId) &&
        /[1-9]/.test(element.elementId) && ptt.fileSize === String(send.voice.size) &&
        typeof ptt.md5HexStr === 'string' && ptt.md5HexStr.toLowerCase() === send.voice.md5;
    };
    var correlateSend = function (R, m, src) {
      if (R.stopped) return;
      var keys = Object.keys(R.sends);
      for (var k = 0; k < keys.length; k++) {
        var key = keys[k], send = R.sends[key];
        if (!send || send.done || send.failed || m.guildId !== send.cookie || m.chatType !== send.chat ||
            m.peerUid !== send.peer || typeof m.msgId !== 'string' || !m.msgId || m.msgId === '0' ||
            m.senderUin !== send.selfUin || !matchesSendElement(m, send) ||
            ((send.voice || send.image) && (m.peerUin !== send.peerUin ||
              !/^[0-9]{1,128}$/.test(m.msgId) || !/[1-9]/.test(m.msgId)))) continue;
        if (src === 'add_send') {
          if (!send.voice && !send.image) {
            if (traceText) observeCallback('matched_add', 'matched', key, send);
            R.noteTextSend('matched_add', send.traceSeq);
          }
          if (send.addId && (send.addId !== m.msgId ||
              ((send.voice || send.image) && send.addElementId !== m.elements[0].elementId))) {
            send.ambiguous = true;
            send.readback = {status:'unknown', reason:'multiple_add_send_ids'};
            R.sendResults[key] = {status:'unknown', reason:'multiple_add_send_ids'};
            settleSendResult(R, key, send);
          } else {
            send.addId = m.msgId;
            if (send.voice || send.image) send.addElementId = m.elements[0].elementId;
          }
        }
        if (src === 'update' && send.addId === m.msgId && m.sendStatus === 2 && !send.readbackStarted &&
            (!(send.voice || send.image) || send.addElementId === m.elements[0].elementId)) {
          if (!send.voice && !send.image) {
            if (traceText) observeCallback('matched_update', 'matched', key, send);
            R.noteTextSend('matched_update', send.traceSeq);
          }
          send.updateId = m.msgId; send.readbackStarted = true;
          (function (key, pending, service) {
            var queryEntered = false;
            Promise.resolve().then(function () {
              if (R.stopped || R.sends[key] !== pending) return null;
              queryEntered = true;
              return service.getMsgsByMsgId({chatType:pending.chat, peerUid:pending.peer,
                guildId:pending.cookie}, [pending.updateId]);
            }).then(function (response) {
              noteCallbackThread('readback');
              var finishObservation = traceText && queryEntered && !pending.voice && !pending.image
                ? beginTextClosureObservation('readback_fulfilled', key, pending) : null;
              try {
                if (R.stopped || R.sends[key] !== pending) return;
                var list = response && response.msgList;
                if (!response || response.result !== 0 || !Array.isArray(list) || list.length !== 1) {
                  pending.readback = {status:'unknown', reason:'exact_query_miss'};
                } else {
                  var record = list[0];
                  var matched = record && record.msgId === pending.updateId && record.chatType === pending.chat &&
                    record.peerUid === pending.peer && record.senderUin === pending.selfUin &&
                    record.sendStatus === 2 && matchesSendElement(record, pending) &&
                    (!(pending.voice || pending.image) || (record.peerUin === pending.peerUin &&
                      record.elements[0].elementId === pending.addElementId));
                  var owned;
                  if (matched && (pending.voice || pending.image)) {
                    owned = {};
                    var fields = ['msgId','msgSeq','msgRandom','msgTime','clientSeq','guildId',
                      'peerUid','peerUin','senderUid','senderUin','chatType','msgType','subMsgType','sendStatus'];
                    for (var f = 0; f < fields.length; f++) {
                      var value = record[fields[f]];
                      if (typeof value === 'string' || typeof value === 'number') owned[fields[f]] = value;
                    }
                    owned.elements = pending.image
                      ? [{elementType:2,elementId:pending.addElementId,picElement:{fileSize:String(pending.image.size),
                        md5HexStr:pending.image.md5,picWidth:pending.image.width,picHeight:pending.image.height,
                        picType:record.elements[0].picElement.picType,picSubType:0}}]
                      : [{elementType:4, elementId:pending.addElementId,
                        pttElement:{fileSize:String(pending.voice.size),md5HexStr:pending.voice.md5}}];
                  }
                  pending.readback = matched
                      ? {status:'readback', msgId:record.msgId, record:owned}
                      : {status:'unknown', reason:'exact_query_identity_mismatch'};
                  if (!pending.voice && !pending.image) R.noteTextSend(matched ? 'readback_matched' : 'readback_mismatch', pending.traceSeq);
                }
                settleSendResult(R, key, pending);
              } finally {
                if (finishObservation) finishObservation();
              }
            }, function () {
              noteCallbackThread('readback');
              var finishObservation = traceText && queryEntered && !pending.voice && !pending.image
                ? beginTextClosureObservation('readback_rejected', key, pending) : null;
              try {
                if (R.stopped || R.sends[key] !== pending) return;
                pending.readback = {status:'unknown', reason:'exact_query_rejected'};
                settleSendResult(R, key, pending);
              } finally {
                if (finishObservation) finishObservation();
              }
            });
          })(key, send, R.ms);
        }
      }
    };
    R.settleSendResult = settleSendResult;
    var pushList = function (src, a) {
      if (!Array.isArray(a)) { R.rejected++; return; }
      for (var i = 0; i < a.length; i++) push(src, a[i]);
    };
    R.listener = {
      onRecvMsg: function (a) {
        noteCallbackThread('recv');
        if (traceText) observeCallback('recv', 'none');
        if (!R.stopped) pushList('recv', a);
      },
      onMsgInfoListUpdate: function (a) {
        noteCallbackThread('update');
        if (traceText) observeCallback('update', 'none');
        if (!R.stopped) pushList('update', a);
      },
      onAddSendMsg: function (m) {
        noteCallbackThread('add');
        if (traceText) observeCallback('add', 'none');
        if (!R.stopped) push('add_send', m);
      },
      // A late media completion can still release an owned temporary file.
      onRichMediaDownloadComplete: function (result) {
        if (typeof R.voiceDownload === 'function') R.voiceDownload(result);
      }
    };
    R.listenerId = ms.addKernelMsgListener(R.listener);
    G.__caligo_res = R;
    G.__caligo_d8 = R;
    return 'ARMED sid=' + sidName + ' lidRet=' + String(R.listenerId).slice(0, 40);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
";

/// 排空接收环:返回 JSON {sid,n,items:[...]} / "NOT_ARMED"。
pub const D8_POLL_JS: &str = r"
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    // Production passes the captured binding; reject before any mutation.
    if (typeof caligoExpectedSid !== 'undefined' && R.sid !== caligoExpectedSid) return 'ERR:session_mismatch';
    var keys = Object.keys(R.sends || {});
    for (var k = 0; k < keys.length; k++) {
      var key = keys[k], send = R.sends[key];
      if (send && !send.done && !send.timedOut && Date.now() >= send.deadline) {
        send.timedOut = true;
        R.sendResults[key] = {status:'unknown', reason:'timeout'};
      }
      if (send && Date.now() >= send.deadline + 30000) delete R.sends[key];
    }
    var leaseMode = typeof caligoResultLease !== 'undefined' && caligoResultLease === true;
    var sourceResults = R.sendResults || {}, offeredResults = sourceResults;
    var deferrals = R.resultDeferrals;
    if (leaseMode && deferrals instanceof Map && deferrals.size) {
      var retryNow = null;
      try { var n = process.hrtime.bigint(); if (typeof n === 'bigint' && n >= 0n) retryNow = n; } catch (_) {}
      if (retryNow === null) {
        // A missing clock must not hide an unacknowledged result.
        deferrals.clear();
      } else {
        deferrals.forEach(function(d, k) {
          if (!d || typeof d.since !== 'bigint' || typeof d.until !== 'bigint' ||
              d.until < d.since || d.until - d.since > 2000000000n ||
              !Number.isInteger(d.count) || d.count < 1 || d.count > 4 ||
              !Object.prototype.hasOwnProperty.call(sourceResults, k) || sourceResults[k] !== d.value)
            deferrals.delete(k);
        });
        offeredResults = Object.create(null);
        Object.keys(sourceResults).forEach(function(k) {
          var d = deferrals.get(k);
          if (!d || d.value !== sourceResults[k] || retryNow < d.since || retryNow >= d.until) offeredResults[k] = sourceResults[k];
        });
      }
    }
    var out = {schema:1, sid:R.sid, items:R.recv, rejected:R.rejected, overflow:R.overflow,
      sendResults:offeredResults, imageShapes:R.imageShapes || [], mediaDiagnostics:R.mediaDiagnostics || [],
      textSendTrace:R.textSendTrace || [], textSendTraceGeneration:R.textTraceGeneration || 0,
      textSendTraceOverflow:R.textTraceOverflow === true};
    var resultKeysForLease = Object.keys(offeredResults);
    var hasTerminal = resultKeysForLease.some(function(k) {
      var value = offeredResults[k];
      return value && (value.status === 'done' || value.status === 'unknown' || value.status === 'rejected');
    });
    var receipt = null;
    if (leaseMode && hasTerminal) {
      if (R.resultLeaseGeneration === undefined) {
        var crypto = process.getBuiltinModule('node:crypto');
        var generation = crypto.randomBytes(16).toString('hex');
        if (!/^[0-9a-f]{32}$/.test(generation)) return 'ERR:result_lease_unavailable';
        R.resultLeaseGeneration = generation;
        R.resultLeaseSequence = 0;
      }
      if (typeof R.resultLeaseGeneration !== 'string' || !/^[0-9a-f]{32}$/.test(R.resultLeaseGeneration) ||
          !Number.isSafeInteger(R.resultLeaseSequence) || R.resultLeaseSequence < 0 ||
          R.resultLeaseSequence >= Number.MAX_SAFE_INTEGER) return 'ERR:result_lease_unavailable';
      receipt = {generation:R.resultLeaseGeneration, sequence:++R.resultLeaseSequence};
      out.resultReceipt = receipt;
    }
    var json = JSON.stringify(out);
    // Preserve legacy D9 state for a later poll rather than truncate JSON or D8 records.
    if (Buffer.byteLength(json, 'utf8') > 524288) {
      out.sendResults = {};
      var keys = Object.keys(offeredResults);
      var isolated = {schema:1, sid:R.sid, items:[], rejected:0, overflow:0,
        sendResults:{}, imageShapes:[], mediaDiagnostics:[], textSendTrace:[],
        textSendTraceGeneration:R.textTraceGeneration || 0, textSendTraceOverflow:false};
      if (receipt) isolated.resultReceipt = receipt;
      for (var i = 0; i < keys.length; i++) {
        out.sendResults[keys[i]] = offeredResults[keys[i]];
        if (Buffer.byteLength(JSON.stringify(out), 'utf8') > 524288) {
          // Keep the omitted result for a later poll, but let smaller results
          // behind it make progress in this batch.
          delete out.sendResults[keys[i]];
          isolated.sendResults = {};
          isolated.sendResults[keys[i]] = offeredResults[keys[i]];
          if (offeredResults[keys[i]].status !== 'pending' &&
              Buffer.byteLength(JSON.stringify(isolated), 'utf8') > 524288) {
            // A terminal result that cannot fit even in an otherwise empty
            // batch cannot carry a usable identity; report unknown instead.
            out.sendResults[keys[i]] = {status:'unknown', reason:'result_exceeds_poll_budget'};
            if (Buffer.byteLength(JSON.stringify(out), 'utf8') > 524288)
              delete out.sendResults[keys[i]];
          }
          continue;
        }
      }
      json = JSON.stringify(out);
    }
    // Match the decoder's batch-level admission before discarding state. This
    // does not acknowledge delivery to Rust or its durable consumer.
    if (Buffer.byteLength(json, 'utf8') > 524288) return 'ERR:poll_batch_invalid';
    var wire = JSON.parse(json);
    if (wire.schema !== 1 || typeof wire.sid !== 'string' || !wire.sid ||
        Buffer.byteLength(wire.sid, 'utf8') > 128 || !Array.isArray(wire.items) ||
        !Number.isSafeInteger(wire.rejected) || wire.rejected < 0 ||
        !Number.isSafeInteger(wire.overflow) || wire.overflow < 0)
      return 'ERR:poll_batch_invalid';
    if (leaseMode && receipt) {
      var refs = Object.create(null);
      var emitted = Object.keys(wire.sendResults || {});
      for (var i = 0; i < emitted.length; i++) {
        var item = wire.sendResults[emitted[i]];
        if (item && (item.status === 'done' || item.status === 'unknown' || item.status === 'rejected'))
          refs[emitted[i]] = R.sendResults[emitted[i]];
      }
      R.resultLease = {generation:receipt.generation, sequence:receipt.sequence, refs:refs};
    }
    R.recv = []; R.recvBytes = 0; R.rejected = 0; R.overflow = 0; R.imageShapes = []; R.mediaDiagnostics = []; R.textSendTrace = []; R.textTraceOverflow = false;
    if (!leaseMode) {
      // Compatibility path; serialization is not downstream acknowledgement.
      var resultKeys = Object.keys(out.sendResults || {});
      for (var i = 0; i < resultKeys.length; i++) {
        if (R.sendResults[resultKeys[i]].status !== 'pending') delete R.sendResults[resultKeys[i]];
      }
    }
    return json;
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
";

/// 对称移除监听器 + 清理常驻对象:返回 "STOPPED sid=.. remove=.."。
pub const D8_STOP_JS: &str = r"
(function () {
  try {
    var G = globalThis;
    if (!G.__caligo_res) return 'NOT_ARMED';
    var R = G.__caligo_res;
    var r = 'no-listener';
    R.stopped = true;
    var noteStopPhase = function(phase, kind) {
      try { if (R.observeStopPhase) R.observeStopPhase(phase, kind); } catch (_) {}
    };
    noteStopPhase('stopping', 'stop_begin');
    // Research-only, synchronous synthetic callbacks. No QQ API or message data.
    var probe = '';
    if (typeof caligoStopProbe !== 'undefined' && caligoStopProbe === true) {
      try {
        var before = [R.recv, R.recv.length, R.recvBytes, R.rejected, R.overflow,
          R.imageShapes, R.imageShapes.length];
        R.syntheticStopProbe = true;
        try {
          R.listener.onRecvMsg(null);
          R.listener.onMsgInfoListUpdate(null);
          R.listener.onAddSendMsg(null);
        } finally { R.syntheticStopProbe = false; }
        var unchanged = R.recv === before[0] && R.recv.length === before[1] &&
          R.recvBytes === before[2] && R.rejected === before[3] && R.overflow === before[4] &&
          R.imageShapes === before[5] && R.imageShapes.length === before[6];
        probe = unchanged ? ' stop_probe=pass' : ' stop_probe=fail';
      } catch (_) { probe = ' stop_probe=error'; }
    }
    try { r = String(R.ms.removeKernelMsgListener(R.listenerId)); }
    catch (_) {
      noteStopPhase('remove_failed', 'stop_remove_failed');
      return 'ERR:listener_remove_failed' + probe;
    }
    noteStopPhase('stopped', 'stop_end');
    var sid = R.sid;
    delete G.__caligo_res;
    return 'STOPPED sid=' + sid + ' remove=' + r + probe;
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
";

/// Read the retained listener's metadata without clearing any queue. The owner
/// binds both fields at acquisition, including for the immediate post-STOP read.
#[cfg(feature = "research")]
#[doc(hidden)]
pub fn build_callback_observations_js(session: &str, generation: u32) -> Option<String> {
    if session.is_empty() || session.len() > 128 || generation > 2_147_483_647 {
        return None;
    }
    let sid = serde_json::to_string(session).ok()?;
    Some(format!(r#"(function(sid, generation) {{
      try {{
        var R = globalThis.__caligo_d8;
        if (!R || R.sid !== sid || R.textTraceGeneration !== generation ||
            !Array.isArray(R.callbackObservations) || typeof R.callbackObservationsOverflow !== 'boolean')
          return 'UNAVAILABLE';
        var rows = R.callbackObservations, events = [];
        var kinds = ['recv','add','update','matched_add','matched_update','promise_fulfilled',
          'promise_rejected','readback_fulfilled','readback_rejected','stop_begin','stop_end','stop_remove_failed'];
        var phases = ['active','stopping','stopped','remove_failed'];
        var associations = ['none','matched','closure'];
        var overflow = R.callbackObservationsOverflow || rows.length > 128;
        for (var i = 0; i < Math.min(rows.length, 129); i++) {{
          var e = rows[i];
          if (!e) {{ overflow = true; continue; }}
          var g = e.generation, order = e.order, seq = e.seq, kind = e.kind,
            phase = e.phase, association = e.association, current = e.current, callbackTid = e.callback_tid;
          if (g !== generation || !Number.isInteger(order) || order < 1 || order > 128 ||
              !Number.isInteger(seq) || seq < 0 || seq > 2147483647 || !kinds.includes(kind) ||
              !phases.includes(phase) || !associations.includes(association) ||
              (current !== null && typeof current !== 'boolean') ||
              (callbackTid !== null && (!Number.isInteger(callbackTid) || callbackTid < 1 || callbackTid > 4294967295))) {{ overflow = true; continue; }}
          var row = {{generation:g, order:order, seq:seq, kind:kind, phase:phase,
            association:association, current:current, callback_tid:callbackTid}};
          if (e.text_state !== undefined) {{
            if (phase !== 'stopped' || association !== 'closure' ||
                !['unchanged','changed','unverified'].includes(e.text_state)) {{ overflow = true; continue; }}
            row.text_state = e.text_state;
          }}
          events.push(row);
        }}
        var json = JSON.stringify({{generation:generation, overflow:overflow, events:events}});
        return json.length <= 24576 ? json : 'UNAVAILABLE';
      }} catch (_) {{ return 'UNAVAILABLE'; }}
    }})({sid}, {generation})"#))
}

/// Build a read-only, session-bound aggregate of the result table. No result
/// key, message content, reason, generation or account is returned.
pub fn build_result_pressure_js(session: &str) -> Option<String> {
    if session.is_empty() || session.len() > 128 { return None; }
    let sid = serde_json::to_string(session).ok()?;
    Some(format!(r#"(function(expected) {{
      try {{
        var R = globalThis.__caligo_res;
        if (!R || R.sid !== expected) return 'ERR:pressure_session';
        var results = R.sendResults;
        if (!results || typeof results !== 'object' || Array.isArray(results) ||
            (Object.getPrototypeOf(results) !== Object.prototype && Object.getPrototypeOf(results) !== null)) return 'ERR:pressure_state';
        var keys = Object.keys(results), overflow = keys.length > 128;
        var delayed = null, deferrals = R.resultDeferrals;
        if (!overflow && (deferrals === undefined || deferrals === null)) delayed = 0;
        else if (!overflow && deferrals instanceof Map) {{
          if (deferrals.size === 0) delayed = 0;
          else if (deferrals.size <= 128) {{
            var now = null;
            try {{ var t = process.hrtime.bigint(); if (typeof t === 'bigint' && t >= 0n) now = t; }} catch (_) {{}}
            if (now !== null) {{
              delayed = 0;
              for (var i = 0; i < keys.length; i++) {{
                var d = deferrals.get(keys[i]);
                if (d === undefined) continue;
                if (!d || typeof d.since !== 'bigint' || typeof d.until !== 'bigint' ||
                    d.until < d.since || d.until - d.since > 2000000000n ||
                    !Number.isInteger(d.count) || d.count < 1 || d.count > 4) {{ delayed = null; break; }}
                if (d.value === results[keys[i]] && now >= d.since && now < d.until) delayed++;
              }}
            }}
          }}
        }}
        return JSON.stringify({{entries:Math.min(keys.length,128), over_capacity:overflow, delayed:delayed}});
      }} catch (_) {{ return 'ERR:pressure_unavailable'; }}
    }})({sid})"#))
}

/// Validate and reconstruct only the fixed aggregate fields; arbitrary input
/// fields never reach the stage report. A missing sample is not zero pressure.
pub fn sanitize_result_pressure(input: &str) -> Option<String> {
    if input.len() > 1024 { return None; }
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let entries = value.get("entries")?.as_u64().filter(|n| *n <= 128)?;
    let over = value.get("over_capacity")?.as_bool()?;
    if over && entries != 128 { return None; }
    let delayed = match value.get("delayed")? {
        serde_json::Value::Null => None,
        v => Some(v.as_u64().filter(|n| *n <= entries)?),
    };
    if over && delayed.is_some() { return None; }
    Some(serde_json::json!({"entries":entries,"capacity":128,"saturated":entries == 128,
        "over_capacity":over,"delayed":delayed}).to_string())
}

pub fn read_result_pressure(session: &str) -> Result<Option<String>, V8Error> {
    let Some(source) = build_result_pressure_js(session) else { return Ok(None); };
    let output = run_fixed_query_script(&source)?;
    Ok(sanitize_result_pressure(&output))
}

/// A receipt identifies one JS listener's exact serialized result snapshot.
/// It only acknowledges resident processing, not daemon durability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollResultReceipt { pub generation: String, pub sequence: u64 }

/// Only the result keys accepted by the resident may be acknowledged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollResultAck {
    pub session: String,
    pub receipt: PollResultReceipt,
    pub keys: Vec<String>,
}

pub fn poll_result_receipt(json: &str) -> Result<Option<PollResultReceipt>, &'static str> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| "receipt_json")?;
    let Some(receipt) = value.get("resultReceipt") else { return Ok(None); };
    let generation = receipt.get("generation").and_then(|v| v.as_str())
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .ok_or("receipt_generation")?;
    let sequence = receipt.get("sequence").and_then(|v| v.as_u64())
        .filter(|n| *n > 0 && *n <= 9_007_199_254_740_991).ok_or("receipt_sequence")?;
    Ok(Some(PollResultReceipt { generation: generation.to_string(), sequence }))
}

/// Build an idempotent ack that only removes unchanged result objects.
pub fn build_poll_result_ack_js(session: &str, receipt: &PollResultReceipt, keys: &[String]) -> Option<String> {
    if session.is_empty() || session.len() > 128 || receipt.generation.len() != 32 ||
        !receipt.generation.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) ||
        receipt.sequence == 0 || receipt.sequence > 9_007_199_254_740_991 || keys.len() > 128 { return None; }
    let unique: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
    if unique.len() != keys.len() || serde_json::to_string(keys).ok()?.len() > 524288 { return None; }
    let args = serde_json::json!({"sid":session,"generation":receipt.generation,"sequence":receipt.sequence,"keys":keys});
    Some(format!(r#"(function(a) {{
      try {{
        var R = globalThis.__caligo_res;
        if (!R || R.sid !== a.sid) return 'ERR:receipt_session';
        if (!Array.isArray(a.keys) || a.keys.length > 128) return 'ERR:receipt_keys';
        var fingerprint = JSON.stringify(a.keys);
        var last = R.resultLeaseAcked;
        if (last && last.generation === a.generation && last.sequence === a.sequence)
          return last.keys === fingerprint ? 'ACKED' : 'ERR:receipt_replay_mismatch';
        var lease = R.resultLease;
        if (!lease || lease.generation !== a.generation || lease.sequence !== a.sequence) return 'ERR:receipt_stale';
        for (var i = 0; i < a.keys.length; i++) {{
          if (typeof a.keys[i] !== 'string' || !Object.prototype.hasOwnProperty.call(lease.refs, a.keys[i]))
            return 'ERR:receipt_keys';
        }}
        var now = null;
        try {{ var t = process.hrtime.bigint(); if (typeof t === 'bigint' && t >= 0n) now = t; }} catch (_) {{}}
        var deferred = R.resultDeferrals;
        if (now !== null && !(deferred instanceof Map)) {{ deferred = new Map(); R.resultDeferrals = deferred; }}
        if (now === null && deferred instanceof Map) {{ deferred.clear(); deferred = null; }}
        var selected = new Set(a.keys), snapshotKeys = Object.keys(lease.refs);
        for (var i = 0; i < snapshotKeys.length; i++) {{
          var k = snapshotKeys[i], original = lease.refs[k];
          if (selected.has(k)) {{
            if (R.sendResults[k] === original) delete R.sendResults[k];
            if (deferred) deferred.delete(k);
          }} else if (deferred && R.sendResults[k] === original) {{
            var prior = deferred.get(k);
            if (prior || deferred.size < 128) {{
              var count = prior && prior.value === original && typeof prior.since === 'bigint' &&
                Number.isInteger(prior.count) && prior.count >= 1 && prior.count <= 4 && now >= prior.since
                ? Math.min(prior.count + 1, 4) : 1;
              var delayMs = 250 * Math.pow(2, count - 1);
              deferred.set(k, {{value:original, count:count, since:now, until:now + BigInt(delayMs) * 1000000n}});
            }}
          }} else if (deferred) {{ deferred.delete(k); }}
        }}
        R.resultLeaseAcked = {{generation:a.generation, sequence:a.sequence, keys:fingerprint}};
        R.resultLease = null;
        return 'ACKED';
      }} catch (_) {{ return 'ERR:receipt_failed'; }}
    }})({args})"#))
}

/// Advance one pending receipt only after the caller observes an ACKED reply.
pub fn advance_poll_result_ack<F>(pending: &mut Option<PollResultAck>, ack: F) -> bool
where F: FnOnce(&PollResultAck) -> bool {
    if let Some(item) = pending.as_ref() {
        if !ack(item) { return false; }
        *pending = None;
    }
    true
}

pub fn ack_poll_result(item: &PollResultAck) -> Result<String, V8Error> {
    let Some(source) = build_poll_result_ack_js(&item.session, &item.receipt, &item.keys) else { return Ok("ERR:receipt_invalid".into()); };
    let Some(syms) = symbols() else { return Err(V8Error::NoCurrentContext); };
    // SAFETY: caller is the owner thread; exec_script checks the V8 ladder.
    unsafe { exec_script(&syms, isolate_addr(), &source) }
}

// —— 共享符号存取(daemon 导出解析一次;adapter/pump 共用) ——

static V8_STORE: std::sync::Mutex<Option<V8Symbols>> = std::sync::Mutex::new(None);

/// daemon 导出侧:解析并存储 V8 符号(每代次一次)。
pub fn store_symbols(qqnt: usize, report: &str) -> bool {
    let Some(syms) = V8Symbols::resolve(qqnt, report) else {
        return false;
    };
    if let Ok(mut g) = V8_STORE.lock() {
        *g = Some(syms);
        true
    } else {
        false
    }
}

/// LAB 注入(绕过导出表解析;仅 research 构建导出,真机禁用)。
#[doc(hidden)]
pub fn store_symbols_raw(s: V8Symbols) {
    if let Ok(mut g) = V8_STORE.lock() {
        *g = Some(s);
    }
}

fn symbols() -> Option<V8Symbols> {
    V8_STORE.lock().ok().and_then(|g| *g)
}

fn isolate_addr() -> *mut c_void {
    crate::qq_entry::isolate_addr()
}

/// 排空轮转点:执行 D8_POLL_JS 并返回原始 JSON("NOT_ARMED"/items JSON)。
pub fn poll_listener_json() -> Result<String, V8Error> {
    poll_listener_script(D8_POLL_JS)
}

/// Generate a poll tied to the listener session captured at acquisition.
pub fn build_bound_poll_js(session: &str) -> Option<String> {
    if session.is_empty() || session.len() > 128 { return None; }
    let quoted = serde_json::to_string(session).ok()?;
    Some(format!("(function(caligoExpectedSid, caligoResultLease) {{ return ({}); }})({quoted}, true)", D8_POLL_JS))
}

/// Production poll. A missing binding must not execute the destructive script.
pub fn poll_listener_bound_json(session: &str) -> Result<String, V8Error> {
    let Some(source) = build_bound_poll_js(session) else { return Ok("NOT_ARMED:session_unavailable".into()); };
    poll_listener_script(&source)
}

fn poll_listener_script(source: &str) -> Result<String, V8Error> {
    #[cfg(feature = "research")]
    callback_threads::drain();
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 阶梯内自管全部检查;调用方保证 owner 线程。
    let output = unsafe { exec_script(&syms, isolate_addr(), source) }?;
    if let Some(detail) = crate::receive_diagnostics::sanitize_image_shapes(&output) {
        // This is a per-process research report budget, independent of message admission.
        static IMAGE_REPORTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if IMAGE_REPORTS.fetch_update(std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed, |n| (n < 32).then_some(n + 1)).is_ok() {
            crate::qq_entry::append_stage_simple("receive_image_shape", true, &detail);
        }
    }
    if let Some(detail) = crate::receive_diagnostics::sanitize_media_download(&output) {
        static MEDIA_REPORTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if MEDIA_REPORTS.fetch_update(std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed, |n| (n < 32).then_some(n + 1)).is_ok() {
            crate::qq_entry::append_stage_simple("media_download_stage", true, &detail);
        }
    }
    #[cfg(feature = "research")]
    if let Some(detail) = crate::receive_diagnostics::sanitize_text_send_trace(&output) {
        static TEXT_REPORTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if TEXT_REPORTS.fetch_update(std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed, |n| (n < 32).then_some(n + 1)).is_ok() {
            crate::qq_entry::append_stage_simple("text_send_trace", true, &detail);
        }
    }
    #[cfg(feature = "research")]
    report_callback_observations(&syms);
    Ok(output)
}

#[cfg(feature = "research")]
#[derive(Default)]
struct CallbackObservationReport {
    source: Option<String>,
    generation: u32,
    previous: Option<String>,
    reports: u8,
}

#[cfg(feature = "research")]
static CALLBACK_OBSERVATION_REPORT: std::sync::Mutex<CallbackObservationReport> =
    std::sync::Mutex::new(CallbackObservationReport {
        source: None,
        generation: 0,
        previous: None,
        reports: 0,
    });

#[cfg(feature = "research")]
const OBSERVATION_REPORTS: u8 = 32;
/// Polling may use every slot but the last: the stop sample reserves it so a
/// long idle session can always record its final observation.
#[cfg(feature = "research")]
const POLL_OBSERVATION_REPORTS: u8 = OBSERVATION_REPORTS - 1;

#[cfg(feature = "research")]
fn poll_report_allowed(reports: u8) -> bool {
    reports < POLL_OBSERVATION_REPORTS
}

#[cfg(feature = "research")]
fn final_report_allowed(reports: u8) -> bool {
    reports < OBSERVATION_REPORTS
}

#[cfg(feature = "research")]
fn report_callback_observations(syms: &V8Symbols) {
    let (source, generation) = {
        let Ok(report) = CALLBACK_OBSERVATION_REPORT.lock() else { return; };
        if !poll_report_allowed(report.reports) { return; }
        let Some(source) = report.source.clone() else { return; };
        (source, report.generation)
    };
    // SAFETY: same owner and context checks as the preceding poll or STOP.
    let snapshot = unsafe { exec_script(syms, isolate_addr(), &source) }.unwrap_or_default();
    let detail = crate::receive_diagnostics::sanitize_callback_observations(&snapshot, generation);
    let Ok(mut report) = CALLBACK_OBSERVATION_REPORT.lock() else { return; };
    if !poll_report_allowed(report.reports) || report.generation != generation ||
        report.source.as_deref() != Some(source.as_str()) || report.previous.as_ref() == Some(&detail) {
        return;
    }
    report.reports += 1;
    crate::qq_entry::append_stage_simple("js_callback_observation", true, &detail);
    report.previous = Some(detail);
}

/// Cached binding only: safe to read before scheduling work on the owner.
#[cfg(feature = "research")]
pub fn stop_observation_identity(account: &str, host_nonce: u64) -> Option<crate::stop_observation::StopIdentity> {
    if account.is_empty() || account.len() > 128 || !account.bytes().all(|b| b.is_ascii_digit())
        || host_nonce == 0 || host_nonce != crate::daemon_client::host_instance_nonce() { return None; }
    let session = listener_session()?;
    let report = CALLBACK_OBSERVATION_REPORT.lock().ok()?;
    if report.source.is_none() || !(1..=i32::MAX as u32).contains(&report.generation) { return None; }
    Some(crate::stop_observation::StopIdentity {
        account:account.to_owned(), host_nonce, wrapper_session:session,
        listener_generation:report.generation,
    })
}

#[cfg(feature = "research")]
#[doc(hidden)]
pub fn build_stopped_observation_js(identity: &crate::stop_observation::StopIdentity) -> Option<String> {
    if identity.host_nonce == 0 || identity.account.is_empty() || identity.account.len() > 128
        || !identity.account.bytes().all(|b| b.is_ascii_digit()) || identity.listener_generation == 0 { return None; }
    let source = build_callback_observations_js(&identity.wrapper_session, identity.listener_generation)?;
    let account = serde_json::to_string(&identity.account).ok()?;
    Some(format!(r#"(function(account) {{
      try {{
        var R = globalThis.__caligo_d8;
        if (!R || R.voiceAccount !== account || R.stopped !== true) return 'UNAVAILABLE';
        return ({source});
      }} catch (_) {{ return 'UNAVAILABLE'; }}
    }})({account})"#))
}

/// Called only by the admitted owner driver; never dispatches or polls messages.
#[cfg(feature = "research")]
pub fn sample_stopped_observations(identity: &crate::stop_observation::StopIdentity) -> Result<crate::stop_observation::Sample, &'static str> {
    if stop_observation_identity(&identity.account, identity.host_nonce).as_ref() != Some(identity) {
        return Err("observation_binding_changed");
    }
    let source = build_stopped_observation_js(identity).ok_or("observation_binding_invalid")?;
    let syms = symbols().ok_or("observation_symbols_unavailable")?;
    // SAFETY: owner/context admission is checked by the driver and V8 ladder.
    let snapshot = unsafe { exec_script(&syms, isolate_addr(), &source) }.map_err(|_| "observation_v8_failed")?;
    let detail = crate::receive_diagnostics::sanitize_callback_observations(&snapshot, identity.listener_generation);
    let parsed: serde_json::Value = serde_json::from_str(&detail).map_err(|_| "observation_shape")?;
    if parsed.get("incomplete").and_then(|v| v.as_bool()) != Some(false) { return Err("observation_incomplete"); }
    let count = parsed.get("events").and_then(|v| v.as_array()).ok_or("observation_shape")?.len();
    let mut report = CALLBACK_OBSERVATION_REPORT.lock().map_err(|_| "observation_report_poisoned")?;
    if report.generation != identity.listener_generation { return Err("observation_binding_changed"); }
    // The stop sample never fails on the report budget: polling leaves it the
    // final slot, and a full budget only skips the detail line.
    if report.previous.as_ref() != Some(&detail) && final_report_allowed(report.reports) {
        report.reports += 1;
        crate::qq_entry::append_stage_simple("js_callback_observation", true, &detail);
        report.previous = Some(detail);
    }
    Ok(crate::stop_observation::Sample { callbacks_seen:count as u64 })
}

/// Read-only surface enumeration for the R2/R3 notification work. It prints
/// only property names and typeof values, never message data or identity
/// strings, so the report stays bounded and auditable.
#[cfg(feature = "research")]
pub const D8_SURFACE_PROBE_JS: &str = r"
(function () {
  try {
    var q = process._linkedBinding('QQNT');
    var S = q.NodeIQQNTWrapperSession;
    var s = S.getNTWrapperSession('nt_1');
    if (!s || typeof s !== 'object') return JSON.stringify({error:'no_session'});
    function names(o, limit) {
      try {
        var ks = Object.getOwnPropertyNames(o);
        if (!Array.isArray(ks)) return [];
        if (ks.length > limit) ks = ks.slice(0, limit);
        var out = [];
        for (var i = 0; i < ks.length; i++) {
          var k = ks[i];
          if (typeof k === 'string' && k.length > 0) out.push(k.slice(0, 64));
        }
        return out;
      } catch (e) { return ['ERR:' + String((e && e.name) || 'error').slice(0, 32)]; }
    }
    function typeOf(v) { return v === null ? 'null' : (Array.isArray(v) ? 'array' : typeof v); }
    function service(name) {
      try {
        if (typeof s[name] !== 'function') return {type:typeOf(s[name])};
        var v = s[name]();
        var out = {type:typeOf(v)};
        if (v && (typeof v === 'object' || typeof v === 'function')) {
          out.keys = names(v, 48);
          try { out.proto = names(Object.getPrototypeOf(v), 48); } catch (e) { out.proto = []; }
        }
        return out;
      } catch (e) { return {error:String((e && e.name) || 'error').slice(0, 32)}; }
    }
    var out = {session:names(s, 64), wrapper_class:names(S, 48), services:{}};
    try { out.session_proto = names(Object.getPrototypeOf(s), 64); } catch (e) { out.session_proto = []; }
    var probes = ['getLoginService','getNetworkService','getNetwork','getNTWrapperNetwork','getProfileService','getMsgService'];
    for (var i = 0; i < probes.length; i++) out.services[probes[i]] = service(probes[i]);
    return JSON.stringify(out);
  } catch (e) { return JSON.stringify({error:String((e && e.name) || 'error').slice(0, 32)}); }
})()
";

#[cfg(feature = "research")]
pub fn surface_probe() -> Result<String, V8Error> {
    let syms = symbols().ok_or(V8Error::NoCurrentContext)?;
    // SAFETY: owner-thread hook; the V8 ladder re-checks owner/context.
    let raw = unsafe { exec_script(&syms, isolate_addr(), D8_SURFACE_PROBE_JS) }?;
    let mut detail: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if detail.len() > 3000 { detail.truncate(3000); }
    let parsed: serde_json::Value = serde_json::from_str(&detail).map_err(|_| V8Error::RunEmpty)?;
    Ok(parsed.to_string())
}

#[cfg(all(test, feature = "research"))]
mod observation_report_budget_tests {
    use super::*;

    #[test]
    fn polling_reserves_the_final_slot() {
        assert!(poll_report_allowed(0));
        assert!(poll_report_allowed(POLL_OBSERVATION_REPORTS - 1));
        assert!(!poll_report_allowed(POLL_OBSERVATION_REPORTS));
        assert!(!poll_report_allowed(OBSERVATION_REPORTS));
        assert!(final_report_allowed(POLL_OBSERVATION_REPORTS));
        assert!(!final_report_allowed(OBSERVATION_REPORTS));
        assert_eq!(POLL_OBSERVATION_REPORTS + 1, OBSERVATION_REPORTS);
    }
}

static D8_SESSION: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Captured at successful listener acquisition, not trusted from a later batch.
pub fn listener_session() -> Option<String> {
    D8_SESSION.lock().ok()?.clone()
}

/// 注册监听器(D8_START_JS)。返回 "ARMED sid=.. lidRet=N" / "ALREADY .." / "ERR:.."。
pub fn start_listener() -> Result<String, V8Error> {
    start_listener_with_voice(None)
}

pub(crate) fn start_listener_with_voice(account: Option<&str>) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    #[cfg(feature = "research")]
    let observers_ready = account.is_some() && unsafe { callback_threads::install(&syms, isolate_addr()) };
    #[cfg(not(feature = "research"))]
    let observers_ready = false;
    #[cfg(feature = "research")]
    if account.is_some() {
        crate::qq_entry::append_stage_simple("js_callback_thread_install", observers_ready,
            if observers_ready { "exact_runtime_observers_ready" } else { "observation_unavailable" });
    }
    let listener = account.map(listener_script_with_account).unwrap_or_else(|| {
        format!("(function(caligoSendTrace) {{ return ({}); }})(false)", D8_START_JS)
    });
    #[cfg(feature = "research")]
    let getter_ready = observers_ready && callback_threads::getter_installed();
    #[cfg(not(feature = "research"))]
    let getter_ready = false;
    let source = format!("(function(caligoThreadObserversReady, caligoTidGetterReady) {{ return ({listener}); }})({observers_ready}, {getter_ready})");
    #[cfg(feature = "research")]
    let source = if account.is_some() {
        format!(r#"(function() {{
          var result = ({source});
          try {{
            if (result.startsWith('ARMED sid=')) {{
              var generation = globalThis.__caligo_d8.textTraceGeneration;
              if (Number.isInteger(generation) && generation >= 0 && generation <= 2147483647)
                return generation + ' ' + result;
            }}
          }} catch (_) {{}}
          return result;
        }})()"#)
    } else { source };
    // SAFETY: 同上。
    let result = unsafe { exec_script(&syms, isolate_addr(), &source) }?;
    #[cfg(feature = "research")]
    let (generation, result) = match result.split_once(' ') {
        Some((prefix, rest)) if rest.starts_with("ARMED sid=") => match prefix.parse::<u32>() {
            Ok(generation @ 0..=2_147_483_647) => (generation, rest.to_owned()),
            _ => (0, result),
        },
        _ => (0, result),
    };
    if let Some(session) = result
        .strip_prefix("ARMED sid=")
        .and_then(|v| v.split_once(" lidRet=").map(|(s, _)| s))
    {
        if let Ok(mut bound) = D8_SESSION.lock() {
            *bound = Some(session.to_owned());
        }
        #[cfg(feature = "research")]
        if let Ok(mut report) = CALLBACK_OBSERVATION_REPORT.lock() {
            report.source = account.and_then(|_| build_callback_observations_js(session, generation));
            report.generation = generation;
            report.previous = None;
        }
    }
    Ok(result)
}

fn listener_script_with_account(account: &str) -> String {
    format!(
        "(function(caligoVoiceAccount, caligoSendTrace) {{ return ({}); }})({}, {})",
        D8_START_JS,
        serde_json::to_string(account).expect("serializable account"),
        cfg!(feature = "research")
    )
}

/// 对称移除监听器(D8_STOP_JS)。返回 "STOPPED .. remove=.." / "NOT_ARMED"。
pub fn stop_listener() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    #[cfg(feature = "research")]
    let source = format!("(function(caligoStopProbe) {{ return ({}); }})(true)", D8_STOP_JS);
    #[cfg(not(feature = "research"))]
    let source = format!("(function(caligoStopProbe) {{ return ({}); }})(false)", D8_STOP_JS);
    // SAFETY: 同上。
    let output = unsafe { exec_script(&syms, isolate_addr(), &source) };
    #[cfg(feature = "research")]
    report_callback_observations(&syms);
    let output = output?;
    #[cfg(feature = "research")]
    callback_threads::drain();
    #[cfg(feature = "research")]
    if output.starts_with("STOPPED ") || output.starts_with("ERR:listener_remove_failed ") {
        if let Some(label) = output.rsplit_once(" stop_probe=").map(|(_, label)| label) {
            let ok = label == "pass";
            let detail = match label { "pass" => "pass", "fail" => "fail", _ => "error" };
            crate::qq_entry::append_stage_simple("synthetic_stopped_callbacks", ok, detail);
        }
    }
    Ok(output)
}

/// 固定语义 UID/UIN 双向查询：直接复用 QQNT wrapper session 的服务。
/// 会话、服务、方法表面和 API 调用分 owner-pump 阶段执行，以便定位阻塞点。
const UID_MAP_SANITY_JS: &str = r#"
(function () { return typeof globalThis === 'object' ? 'V8_READY' : 'V8_MISSING'; })()
"#;

const UID_MAP_STATE_JS: &str = r#"
(function () {
  try {
    var G = globalThis;
    if (G.__caligo_uid_map_main) return 'BUSY';
    G.__caligo_uid_map_main = { phase: 'binding' };
    return 'STATE_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_BINDING_JS: &str = r#"
(function () {
  try {
    var S = globalThis.__caligo_uid_map_main;
    if (!S || S.phase !== 'binding') return 'ERR:binding stage is not ready';
    var q = process._linkedBinding('QQNT');
    if (!q) {
      S.phase = 'done'; S.status = 'qqnt_linked_binding_missing';
      return 'SURFACE_DONE';
    }
    S.qqnt = q;
    S.phase = 'wrappers';
    return 'BINDING_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_WRAPPERS_JS: &str = r#"
(function () {
  try {
    var S = globalThis.__caligo_uid_map_main;
    if (!S || S.phase !== 'wrappers') return 'ERR:wrapper stage is not ready';
    var Startup = S.qqnt.NodeIQQNTStartupSessionWrapper;
    var Sessions = S.qqnt.NodeIQQNTWrapperSession;
    if (!Startup || !Sessions) {
      S.phase = 'done'; S.status = 'qqnt_session_wrapper_missing';
      return 'SURFACE_DONE';
    }
    S.Startup = Startup;
    S.Sessions = Sessions;
    S.phase = 'session';
    return 'WRAPPERS_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_SESSION_JS: &str = r#"
(function () {
  try {
    var G = globalThis, S = G.__caligo_uid_map_main;
    if (!S || S.phase !== 'session') return 'ERR:session stage is not ready';
    var startup = S.Startup.create();
    var ids = startup.getSessionIdList();
    var name = ids && String(ids.get('nt') || '');
    if (!name || name === '0') {
      S.phase = 'done'; S.status = 'active_session_missing';
      return 'SURFACE_DONE';
    }
    S.sessionName = name;
    S.phase = 'service';
    return 'SESSION_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_GROUP_JS: &str = r#"
(function () {
  try {
    var G = globalThis, S = G.__caligo_uid_map_main;
    if (!S || S.phase !== 'service') return 'ERR:group stage is not ready';
    var session = S.Sessions.getNTWrapperSession(S.sessionName);
    if (!session || typeof session.getGroupService !== 'function') {
      S.phase = 'done'; S.status = 'group_service_accessor_missing';
      return 'SURFACE_DONE';
    }
    var group = session.getGroupService();
    if (!group) {
      S.phase = 'done'; S.status = 'group_service_missing';
      return 'SURFACE_DONE';
    }
    S.session = session;
    S.group = group;
    S.phase = 'methods';
    return 'GROUP_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

/// Complete a GroupService-only surface check without reading or calling resolver methods.
const UID_MAP_SURFACE_DONE_JS: &str = r#"
(function () {
  try {
    var S = globalThis.__caligo_uid_map_main;
    if (!S || S.phase !== 'methods') return 'ERR:group-service surface is not ready';
    S.route = 'QQNT NodeIQQNTWrapperSession.getGroupService';
    S.status = 'group_service_available';
    S.apiCallAttempted = false;
    S.phase = 'done';
    return 'SURFACE_DONE';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

// QQ 9.9.36-53644 late attachment resolves nt_1 from the loaded wrapper
// module. The QQNT linked binding's startup-session route is separate.
const UID_MAP_PROFILE_METHODS_JS: &str = r#"
function (wrapperPath) {
  var state = globalThis.__caligo_uid_map_main;
  if (!state || state.phase !== 'binding') return 'ERR:profile stage is not ready';
  state.route = 'wrapper.node nt_1 NodeIQQNTWrapperSession.getProfileService';
  function unavailable(status) {
    state.status = status;
    state.phase = 'done';
    return 'SURFACE_DONE';
  }
  try {
    var main = typeof process === 'object' && process && process.mainModule;
    var load = typeof globalThis.require === 'function' ? globalThis.require :
      (main && typeof main.require === 'function' ? main.require.bind(main) : null);
    if (!load) return unavailable('profile_require_missing');
    var wrapper;
    try { wrapper = load(wrapperPath); }
    catch (_) { return unavailable('profile_wrapper_require_failed'); }
    var sessions = wrapper && wrapper.NodeIQQNTWrapperSession;
    if (!sessions || typeof sessions.getNTWrapperSession !== 'function') {
      return unavailable('profile_wrapper_accessor_missing');
    }
    var session = sessions.getNTWrapperSession('nt_1');
    if (!session || typeof session.getProfileService !== 'function') {
      return unavailable('profile_service_accessor_missing');
    }
    // Keep the wrapper owner alive across the service/query/poll pump ticks.
    state.session = session;
    var service = session.getProfileService();
    if (!service) return unavailable('profile_service_missing');
    var forward = service.getUidByUin;
    var reverse = service.getUinByUid;
    if (typeof forward !== 'function' || typeof reverse !== 'function') {
      return unavailable('profile_methods_unavailable');
    }
    state.profile = service;
    state.forwardMethod = forward;
    state.reverseMethod = reverse;
    state.forward_arity = forward.length;
    state.reverse_arity = reverse.length;
    state.phase = 'profile_ready';
    return 'PROFILE_READY';
  } catch (_) {
    delete globalThis.__caligo_uid_map_main;
    return 'ERR:profile service acquisition failed';
  }
}
"#;

fn build_profile_uid_map_query_js(uin: &str) -> String {
    let argument = serde_json::to_string(uin).expect("string serialization is infallible");
    format!("({UID_MAP_PROFILE_QUERY_JS})({argument})")
}

const UID_MAP_PROFILE_QUERY_JS: &str = r#"
function (uin) {
  var state = globalThis.__caligo_uid_map_main;
  if (!state || state.phase !== 'profile_ready') return 'ERR:profile resolver is not ready';
  if (!/^[1-9][0-9]{0,19}$/.test(uin)) return 'ERR:invalid UIN';
  function finish(status) {
    state.status = status;
    state.phase = 'done';
    return 'DISPATCHED';
  }
  state.phase = 'querying';
  state.apiCallAttempted = true;
  var source = 'FriendsServiceImpl';
  function lookup(method, key) {
    var result = method.call(state.profile, source, [key]);
    if (!(result instanceof Map)) {
      // Assimilate foreign-context Promises/thenables before rejecting the
      // unsupported async shape, so a later rejection cannot escape to QQ.
      Promise.prototype.then.call(Promise.resolve(result), undefined, function () {});
      return null;
    }
    return { value: Map.prototype.get.call(result, key) };
  }
  var forward;
  try { forward = lookup(state.forwardMethod, uin); }
  catch (_) { return finish('forward_threw'); }
  if (!forward) return finish('forward_result_not_map');
  var uid = forward.value;
  if (uid === undefined) return finish('forward_no_uid');
  if (typeof uid !== 'string' || !/^u_[A-Za-z0-9_-]{1,128}$/.test(uid)) {
    return finish('forward_invalid_uid');
  }
  state.uidCandidates = [uid];
  var reverse;
  try { reverse = lookup(state.reverseMethod, uid); }
  catch (_) { return finish('reverse_threw'); }
  if (!reverse) return finish('reverse_result_not_map');
  var resolved = reverse.value;
  if (resolved === undefined) return finish('reverse_no_uin');
  if (typeof resolved !== 'string' || !/^[1-9][0-9]{0,19}$/.test(resolved)) {
    return finish('reverse_invalid_uin');
  }
  state.reverseUinValues = [resolved];
  return finish(resolved === uin ? 'roundtrip_match' : 'roundtrip_no_match');
}
"#;

pub(crate) fn start_uid_map_profile(wrapper_path: &str) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    let argument = serde_json::to_string(wrapper_path).expect("string serialization is infallible");
    let source = format!("({UID_MAP_PROFILE_METHODS_JS})({argument})");
    unsafe { exec_script(&syms, isolate_addr(), &source) }
}

pub(crate) fn query_uid_map_profile(uin: &str) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), &build_profile_uid_map_query_js(uin)) }
}

const CURRENT_LOGIN_START_JS: &str = r#"
(function () {
  var state = globalThis.__caligo_uid_map_main;
  if (!state || state.phase !== 'profile_ready') return 'ERR:profile stage is not ready';
  state.route = 'wrapper.node nt_1 NodeIKernelProfileService.getSelfStatus';
  function fail(status) {
    state.status = status;
    state.phase = 'done';
  }
  try {
    var id = state.session.getSessionId();
    if (!((typeof id === 'string' && /^[A-Za-z0-9_-]{1,80}$/.test(id) && id !== '0') ||
          (Number.isSafeInteger(id) && id > 0))) {
      fail('current_session_id_invalid');
      return 'SURFACE_DONE';
    }
    state.sessionId = id;
    if (typeof state.profile.getSelfStatus !== 'function' ||
        typeof state.profile.addKernelProfileListener !== 'function' ||
        typeof state.profile.removeKernelProfileListener !== 'function') {
      fail('self_status_listener_surface_missing');
      return 'SURFACE_DONE';
    }
    var listener = new Proxy({
      onSelfStatusChanged: function (info) {
        if (globalThis.__caligo_uid_map_main !== state ||
            (state.phase !== 'self_status_pending' && state.phase !== 'self_profile_pending' &&
             state.phase !== 'self_profile_ready' && state.phase !== 'profile_refresh_pending')) return;
        try {
          if (!info || typeof info.uid !== 'string' || !/^u_[A-Za-z0-9_-]{1,125}$/.test(info.uid) ||
              typeof info.uin !== 'string' || !/^[1-9][0-9]{4,12}$/.test(info.uin) ||
              !Number.isSafeInteger(info.status) || info.status <= 0) return;
          var prior = state.selfStatus;
          if (prior && (prior.uid !== info.uid || prior.uin !== info.uin)) {
            fail('self_status_conflict');
            return;
          }
          if (info.status === 20) {
            fail('self_status_offline');
            return;
          }
          state.selfStatus = {uid: info.uid, uin: info.uin, status: info.status};
        } catch (_) { fail('self_status_event_invalid'); }
      }
    }, {
      get: function (target, key) {
        if (typeof key !== 'string') return undefined;
        if (key === 'onSelfStatusChanged') return target[key];
        if (key === 'onUserDetailInfoChanged') return function (info) {
          if (typeof state.profileRefreshEvent === 'function') state.profileRefreshEvent(info);
        };
        return function () {};
      }
    });
    // This build returns a decimal string from the native add wrapper.
    state.listener = listener;
    state.listenerRegistration = 'unknown';
    var listenerId = state.profile.addKernelProfileListener(listener);
    if (typeof listenerId !== 'string' || !/^[1-9][0-9]{0,19}$/.test(listenerId)) {
      fail('self_status_listener_id_invalid');
      return 'SURFACE_DONE';
    }
    state.listenerId = listenerId;
    state.listenerRegistration = 'registered';
    state.phase = 'self_status_pending';
    state.apiCallAttempted = true;
    Promise.resolve(state.profile.getSelfStatus()).then(function (value) {
      if (globalThis.__caligo_uid_map_main !== state || state.phase !== 'self_status_pending') return;
      if (!value || !Number.isSafeInteger(value.result)) {
        fail('self_status_ack_invalid');
      } else {
        state.selfStatusResult = value.result;
        if (value.result !== 0) fail('self_status_ack_failed');
      }
    }, function () {
      if (globalThis.__caligo_uid_map_main === state && state.phase === 'self_status_pending') {
        fail('self_status_rejected');
      }
    });
  } catch (_) {
    fail('self_status_threw');
    if (!state.apiCallAttempted) return 'SURFACE_DONE';
  }
  return 'DISPATCHED';
})()
"#;

// Promise handlers record settlement. The owner tick validates identity and
// projects the exact profile row while the self-status listener is still armed.
const CURRENT_LOGIN_FINISH_JS: &str = r#"
(function () {
  var state = globalThis.__caligo_uid_map_main;
  if (!state) return 'ERR:no current-login state';
  var profileReady = state.phase === 'self_profile_ready';
  if (!profileReady &&
      (state.phase !== 'self_status_pending' || state.selfStatusResult !== 0 || !state.selfStatus)) {
    return 'POLL';
  }
  function finish(status) {
    state.status = status;
    state.phase = 'done';
    delete state.selfStatus;
    delete state.selfProfile;
    return 'POLL';
  }
  function completeIdentity(nicknameStatus, nickname) {
    if (state.phase === 'done') return 'POLL';
    var identity = state.selfStatus;
    state.nicknameStatus = nicknameStatus;
    state.currentLogin = {
      uin: identity.uin, uid: identity.uid, status: identity.status, session_id: state.sessionId
    };
    if (nicknameStatus === 'matched') state.currentLogin.nickname = nickname;
    return finish('current_login_match');
  }
  try {
    var identity = state.selfStatus;
    var uin = identity.uin, uid = identity.uid;
    if (state.session.getSessionId() !== state.sessionId) return finish('current_session_changed');
    if (state.expectedAccount !== undefined && state.expectedAccount !== uin) {
      return finish('current_account_mismatch');
    }
    if (profileReady) {
      if (state.nicknameStatus === 'query_rejected') return completeIdentity('query_rejected');
      if (Object.prototype.toString.call(state.selfProfile) !== '[object Map]') {
        return completeIdentity('result_not_map');
      }
      var item = Map.prototype.get.call(state.selfProfile, uid);
      var core = item && item.coreInfo;
      if (!core || core.uid !== uid || core.uin !== uin ||
          (item.uid !== undefined && item.uid !== uid) ||
          (item.uin !== undefined && item.uin !== uin)) {
        return completeIdentity('profile_identity_mismatch');
      }
      var nickname = core.nick;
      if (typeof nickname !== 'string' || !nickname.trim() || nickname.length > 4096 ||
          Buffer.byteLength(nickname, 'utf8') > 4096) return completeIdentity('nickname_invalid');
      if (state.session.getSessionId() !== state.sessionId) return finish('current_session_changed');
      return completeIdentity('matched', nickname);
    }
    var mapped = state.forwardMethod.call(state.profile, 'FriendsServiceImpl', [uin]);
    if (Object.prototype.toString.call(mapped) !== '[object Map]') {
      Promise.resolve(mapped).catch(function () {});
      return finish('self_forward_not_map');
    }
    if (Map.prototype.get.call(mapped, uin) !== uid) return finish('self_forward_mismatch');
    var reverse = state.reverseMethod.call(state.profile, 'FriendsServiceImpl', [uid]);
    if (Object.prototype.toString.call(reverse) !== '[object Map]') {
      Promise.resolve(reverse).catch(function () {});
      return finish('self_reverse_not_map');
    }
    if (Map.prototype.get.call(reverse, uid) !== uin) return finish('self_reverse_mismatch');
    if (state.session.getSessionId() !== state.sessionId) return finish('current_session_changed');
    if (typeof state.profile.getCoreAndBaseInfo !== 'function') return completeIdentity('accessor_missing');
    state.phase = 'self_profile_pending';
    try {
      Promise.resolve(state.profile.getCoreAndBaseInfo('nodeStore', [uid])).then(function (profile) {
        if (globalThis.__caligo_uid_map_main !== state || state.phase !== 'self_profile_pending') return;
        state.selfProfile = profile;
        state.phase = 'self_profile_ready';
      }, function () {
        if (globalThis.__caligo_uid_map_main !== state || state.phase !== 'self_profile_pending') return;
        state.nicknameStatus = 'query_rejected';
        state.phase = 'self_profile_ready';
      });
    } catch (_) {
      if (state.phase !== 'self_profile_pending') return 'POLL';
      return completeIdentity('query_threw');
    }
    return 'POLL';
  } catch (_) { return finish('current_login_check_failed'); }
})()
"#;

pub(crate) fn start_current_login_probe() -> Result<String, V8Error> {
    let syms = symbols().ok_or(V8Error::NoCurrentContext)?;
    unsafe { exec_script(&syms, isolate_addr(), CURRENT_LOGIN_START_JS) }
}

pub(crate) fn finish_current_login_probe() -> Result<String, V8Error> {
    let syms = symbols().ok_or(V8Error::NoCurrentContext)?;
    unsafe { exec_script(&syms, isolate_addr(), CURRENT_LOGIN_FINISH_JS) }
}

const CURRENT_PROFILE_REFRESH_JS: &str = include_str!("profile_refresh.js");

/// Research only: first establish current identity, then one bounded server-source
/// profile fetch. No public capability is declared by this probe.
pub(crate) fn finish_current_profile_probe() -> Result<String, V8Error> {
    run_fixed_query_script(&format!(
        "(function() {{ var step = ({CURRENT_LOGIN_FINISH_JS});
          if (step !== 'POLL') return step;
          return ({CURRENT_PROFILE_REFRESH_JS}); }})()"
    ))
}

/// Normal owner queries reuse the field-tested provider and temporary listener.
pub(crate) fn build_scoped_login_start(wrapper_path: &str, account: &str) -> String {
    let path = serde_json::to_string(wrapper_path).expect("string serialization");
    let account = serde_json::to_string(account).expect("string serialization");
    format!(
        "(function() {{
            if (({UID_MAP_STATE_JS}) !== 'STATE_READY') return 'BUSY';
            globalThis.__caligo_uid_map_main.expectedAccount = {account};
            var ready = ({UID_MAP_PROFILE_METHODS_JS})({path});
            if (ready !== 'PROFILE_READY') return ready;
            return ({CURRENT_LOGIN_START_JS});
        }})()"
    )
}

pub(crate) fn poll_scoped_login() -> Result<String, V8Error> {
    run_fixed_query_script(&build_scoped_login_poll())
}

pub(crate) fn build_scoped_self_profile_start(wrapper_path: &str, account: &str) -> String {
    let start = build_scoped_login_start(wrapper_path, account);
    format!("(function() {{ var result = ({start});
        if (result === 'DISPATCHED' || result === 'SURFACE_DONE') {{
            var state = globalThis.__caligo_uid_map_main;
            if (state) state.retainOnCleanupUnknown = true;
        }}
        return result; }})()")
}

pub(crate) fn poll_scoped_self_profile() -> Result<String, V8Error> {
    run_fixed_query_script(&build_scoped_self_profile_poll())
}

fn build_scoped_self_profile_poll() -> String {
    format!("(function() {{ var step = ({CURRENT_LOGIN_FINISH_JS});
        if (step !== 'POLL') return step;
        step = ({CURRENT_PROFILE_REFRESH_JS});
        if (step !== 'POLL') return step;
        return ({UID_MAP_POLL_JS}); }})()")
}

fn build_scoped_login_poll() -> String {
    format!(
        "(function() {{
            var step = ({CURRENT_LOGIN_FINISH_JS});
            if (step !== 'POLL') return step;
            return ({UID_MAP_POLL_JS});
        }})()"
    )
}

pub fn build_uid_map_methods_js(uin: &str) -> String {
    let uin_json = serde_json::to_string(uin).unwrap_or_else(|_| "\"\"".into());
    let mut js = String::with_capacity(1_200);
    js.push_str(
        r#"(function () {
  try {
    var G = globalThis, S = G.__caligo_uid_map_main;
    if (!S || S.phase !== 'methods') return 'ERR:method stage is not ready';
    function done(status, fields) {
      S.phase = 'done'; S.status = status;
      if (fields) Object.keys(fields).forEach(function (k) { S[k] = fields[k]; });
      return 'SURFACE_DONE';
    }
    var forwardMethod = S.group.getUidByUins;
    var reverseMethod = S.group.getUinByUids;
    if (typeof forwardMethod !== 'function' || typeof reverseMethod !== 'function') {
      return done('wrapper_methods_unavailable', {
        has_forward: typeof forwardMethod === 'function',
        has_reverse: typeof reverseMethod === 'function'
      });
    }
    if (forwardMethod.length > 1 || reverseMethod.length > 1) {
      return done('wrapper_arity_unsupported', {
        forward_arity: forwardMethod.length,
        reverse_arity: reverseMethod.length
      });
    }
    S.uin = "#,
    );
    js.push_str(&uin_json);
    js.push_str(
        r#";
    S.forwardMethod = forwardMethod;
    S.reverseMethod = reverseMethod;
    S.forward_arity = forwardMethod.length;
    S.reverse_arity = reverseMethod.length;
    S.route = 'QQNT NodeIQQNTWrapperSession.getGroupService';
    S.phase = 'ready';
    S.status = 'surface_ready';
    return 'METHODS_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()"#,
    );
    js
}

const UID_MAP_QUERY_JS: &str = r#"
(function () {
  try {
    var G = globalThis, S = G.__caligo_uid_map_main;
    if (!S || S.phase !== 'ready') return 'ERR:uid-map surface is not ready';
    function atoms(value, out, depth, seen) {
      if (depth > 12 || value == null) return;
      if (typeof value === 'string' || typeof value === 'number') { out.push(String(value)); return; }
      if (typeof value !== 'object' || seen.indexOf(value) >= 0) return;
      seen.push(value);
      if (value instanceof Map) {
        value.forEach(function (v, k) { atoms(k, out, depth + 1, seen); atoms(v, out, depth + 1, seen); });
      } else if (Array.isArray(value)) {
        for (var i = 0; i < value.length && i < 256; i++) atoms(value[i], out, depth + 1, seen);
      } else {
        Object.keys(value).slice(0, 256).forEach(function (k) { atoms(value[k], out, depth + 1, seen); });
      }
      seen.pop();
    }
    function clean(value, depth, seen, count) {
      if (depth > 12) return '[depth-limit]';
      if (value == null || typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') return value;
      if (typeof value !== 'object') return String(value);
      if (seen.indexOf(value) >= 0) return '[cycle]';
      if (++count.n > 4096) throw new Error('response item limit exceeded');
      seen.push(value);
      var out;
      if (value instanceof Map) {
        out = { '$map': [] };
        value.forEach(function (v, k) {
          if (out['$map'].length < 256) out['$map'].push([clean(k, depth + 1, seen, count), clean(v, depth + 1, seen, count)]);
        });
      } else if (Array.isArray(value)) {
        out = [];
        for (var i = 0; i < value.length && i < 256; i++) out.push(clean(value[i], depth + 1, seen, count));
      } else {
        out = {};
        Object.keys(value).slice(0, 256).forEach(function (k) {
          try { out[k] = clean(value[k], depth + 1, seen, count); } catch (e) { out[k] = '[unreadable]'; }
        });
      }
      seen.pop();
      return out;
    }
    function finish(status, fields) {
      S.status = status;
      if (fields) Object.keys(fields).forEach(function (k) { S[k] = fields[k]; });
      S.phase = 'done';
    }
    S.phase = 'querying';
    S.apiCallAttempted = true;
    var forward;
    try { forward = S.forwardMethod.call(S.group, [S.uin]); }
    catch (e) { finish('forward_threw', { error: String(e).slice(0, 300) }); return 'DISPATCHED'; }
    Promise.resolve(forward).then(function (value) {
      try {
        var found = [];
        atoms(value, found, 0, []);
        var uids = found.filter(function (v, i, a) { return /^u_.{1,128}$/.test(v) && a.indexOf(v) === i; });
        S.forward = clean(value, 0, [], { n: 0 });
        S.uidCandidates = uids;
        if (!uids.length) { finish('forward_no_uid'); return; }
        if (uids.length > 8) { finish('too_many_uid_candidates'); return; }
        var reverse;
        try { reverse = S.reverseMethod.call(S.group, [uids]); }
        catch (e) { finish('reverse_threw', { error: String(e).slice(0, 300) }); return; }
        Promise.resolve(reverse).then(function (reverseValue) {
          try {
            var foundUins = [];
            atoms(reverseValue, foundUins, 0, []);
            var uins = foundUins.filter(function (v, i, a) { return v === S.uin && a.indexOf(v) === i; });
            S.reverse = clean(reverseValue, 0, [], { n: 0 });
            S.reverseUinValues = uins;
            finish(uins.length > 0 ? 'roundtrip_match' : 'roundtrip_no_match');
          } catch (e) { finish('reverse_parse_error', { error: String(e).slice(0, 300) }); }
        }, function (e) { finish('reverse_rejected', { error: String(e).slice(0, 300) }); });
      } catch (e) { finish('forward_parse_error', { error: String(e).slice(0, 300) }); }
    }, function (e) { finish('forward_rejected', { error: String(e).slice(0, 300) }); });
    return 'DISPATCHED';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_POLL_JS: &str = r#"
(function () {
  try {
    var G = globalThis, S = G.__caligo_uid_map_main;
    if (!S) return 'ERR:no uid-map state';
    if (S.phase !== 'done') return 'PENDING';
    if (S.listenerRegistration === 'registered') {
      S.listenerRegistration = 'remove_unknown';
      try {
        S.profile.removeKernelProfileListener(S.listenerId);
        S.listenerRegistration = 'remove_returned';
      } catch (_) {}
    }
    if (S.listenerRegistration && S.listenerRegistration !== 'remove_returned') {
      S.status = 'self_status_listener_cleanup_unverified';
      delete S.currentLogin;
      delete S.profileRefresh;
    }
    var done = {
      status: S.status,
      route: S.route,
      apiCallAttempted: !!S.apiCallAttempted,
      forward_arity: S.forward_arity,
      reverse_arity: S.reverse_arity,
      uidCandidates: S.uidCandidates || [],
      reverseUinValues: S.reverseUinValues || [],
      forward: S.forward,
      reverse: S.reverse,
      error: S.error,
      currentLogin: S.currentLogin,
      nicknameStatus: S.nicknameStatus,
      selfStatusResult: S.selfStatusResult,
      listenerRegistration: S.listenerRegistration
    };
    if (S.profileRefreshStarted) {
      done.profileRefresh = S.profileRefresh;
      done.profileRefreshCallAttempted = !!S.profileRefreshCallAttempted;
    }
    if (!S.retainOnCleanupUnknown || !S.listenerRegistration || S.listenerRegistration === 'remove_returned') {
      delete G.__caligo_uid_map_main;
    }
    return JSON.stringify(done);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_CANCEL_JS: &str = r#"
(function () {
  var state = globalThis.__caligo_uid_map_main;
  if (state) {
    state.phase = 'cancelled';
    if (state.listenerRegistration === 'registered') {
      state.listenerRegistration = 'remove_unknown';
      try {
        state.profile.removeKernelProfileListener(state.listenerId);
        state.listenerRegistration = 'remove_returned';
      } catch (_) {}
    }
  }
  if (!state || !state.retainOnCleanupUnknown || !state.listenerRegistration || state.listenerRegistration === 'remove_returned') {
    delete globalThis.__caligo_uid_map_main;
  }
  return state && state.listenerRegistration && state.listenerRegistration !== 'remove_returned'
    ? 'CLEANUP_UNKNOWN' : 'CANCELLED';
})()
"#;

pub fn start_uid_map_sanity() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_SANITY_JS) }
}

pub fn start_uid_map_state() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_STATE_JS) }
}

pub fn start_uid_map_probe() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_BINDING_JS) }
}

pub fn start_uid_map_wrappers() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_WRAPPERS_JS) }
}

pub fn start_uid_map_session() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_SESSION_JS) }
}

pub fn start_uid_map_group() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_GROUP_JS) }
}

pub fn start_uid_map_methods(uin: &str) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), &build_uid_map_methods_js(uin)) }
}

pub fn finish_uid_map_surface_only() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_SURFACE_DONE_JS) }
}

pub fn start_uid_map_query() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_QUERY_JS) }
}
pub fn poll_uid_map_probe() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: fixed local state read; no user input or QQ data is mutated.
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_POLL_JS) }
}

pub fn cancel_uid_map_probe() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: fixed cleanup script only removes this probe's own temporary globals.
    unsafe { exec_script(&syms, isolate_addr(), UID_MAP_CANCEL_JS) }
}

#[cfg(test)]
mod exec_script_lab_tests {
    use super::{V8Error, V8Symbols, exec_script};
    use core::ffi::c_void;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static EVENTS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        static CONTEXT_SLOT: Cell<usize> = const { Cell::new(0) };
        static SCOPE_ISOLATE: Cell<usize> = const { Cell::new(0) };
    }

    fn mark(event: &'static str) {
        EVENTS.with(|events| events.borrow_mut().push(event));
    }

    unsafe extern "C" fn scope_ctor(this: *mut c_void, isolate: *mut c_void) -> *mut c_void {
        mark("scope_ctor");
        // The exact-build QQNT constructor stores the isolate in field zero.
        unsafe { *(this.cast::<usize>()) = isolate as usize };
        this
    }

    unsafe extern "C" fn scope_dtor(this: *mut c_void) {
        mark("scope_dtor");
        let isolate = unsafe { *(this.cast::<usize>()) };
        SCOPE_ISOLATE.with(|slot| slot.set(isolate));
    }

    unsafe extern "C" fn get_current() -> *mut c_void {
        mark("get_current");
        core::ptr::null_mut()
    }

    unsafe extern "C" fn entered_ctx(_isolate: *mut c_void, sret: *mut usize) {
        mark("entered_ctx");
        let value = CONTEXT_SLOT.with(Cell::get);
        unsafe { *sret = value };
    }

    unsafe extern "C" fn incumbent_ctx(_isolate: *mut c_void, sret: *mut usize) {
        mark("incumbent_ctx");
        let value = CONTEXT_SLOT.with(Cell::get);
        unsafe { *sret = value };
    }

    unsafe extern "C" fn string_new(
        sret: *mut usize,
        _isolate: *mut c_void,
        _data: *const u8,
        _kind: i32,
        _len: i32,
    ) {
        mark("string_new");
        unsafe { *sret = 0x101 };
    }

    unsafe extern "C" fn script_compile(
        sret: *mut usize,
        _ctx: usize,
        _src: usize,
        _origin: *mut c_void,
    ) {
        mark("script_compile");
        unsafe { *sret = 0x202 };
    }

    unsafe extern "C" fn script_run(_this: usize, sret: *mut usize, _ctx: usize, _data: usize) {
        mark("script_run");
        unsafe { *sret = 0x303 };
    }

    unsafe extern "C" fn utf8_ctor(
        this: *mut c_void,
        _isolate: *mut c_void,
        _value: usize,
    ) -> *mut c_void {
        mark("utf8_ctor");
        this
    }

    unsafe extern "C" fn utf8_dtor(_this: *mut c_void) {
        mark("utf8_dtor");
    }

    unsafe extern "C" fn utf8_deref(_this: *mut c_void) -> *const u8 {
        mark("utf8_deref");
        b"lab-ok\0".as_ptr()
    }

    unsafe extern "C" fn ctx_enter(_this: *mut c_void) {
        mark("ctx_enter");
    }

    unsafe extern "C" fn ctx_exit(_this: *mut c_void) {
        mark("ctx_exit");
    }

    unsafe extern "C" fn trycatch_ctor(this: *mut c_void, _isolate: *mut c_void) -> *mut c_void {
        mark("trycatch_ctor");
        this
    }

    unsafe extern "C" fn trycatch_dtor(_this: *mut c_void) {
        mark("trycatch_dtor");
    }

    unsafe extern "C" fn trycatch_caught(_this: *const c_void) -> bool {
        mark("trycatch_caught");
        false
    }

    unsafe extern "C" fn trycatch_exception(_this: *const c_void, sret: *mut usize) {
        mark("trycatch_exception");
        unsafe { *sret = 0 };
    }

    fn mock_symbols() -> V8Symbols {
        V8Symbols::from_raw([
            scope_ctor as *const () as usize,
            scope_dtor as *const () as usize,
            get_current as *const () as usize,
            entered_ctx as *const () as usize,
            incumbent_ctx as *const () as usize,
            string_new as *const () as usize,
            script_compile as *const () as usize,
            script_run as *const () as usize,
            utf8_ctor as *const () as usize,
            utf8_dtor as *const () as usize,
            utf8_deref as *const () as usize,
            ctx_enter as *const () as usize,
            ctx_exit as *const () as usize,
            trycatch_ctor as *const () as usize,
            trycatch_dtor as *const () as usize,
            trycatch_caught as *const () as usize,
            trycatch_exception as *const () as usize,
        ])
    }

    #[test]
    fn lab_exec_script_matches_sret_abi_and_tears_down_after_context_exit() {
        EVENTS.with(|events| events.borrow_mut().clear());
        CONTEXT_SLOT.with(|slot| slot.set(0x4560));
        SCOPE_ISOLATE.with(|slot| slot.set(0));
        let isolate = 0x1234usize as *mut c_void;

        let result = unsafe { exec_script(&mock_symbols(), isolate, "1+1") };

        assert_eq!(result, Ok("lab-ok".to_owned()));
        SCOPE_ISOLATE.with(|slot| assert_eq!(slot.get(), isolate as usize));
        EVENTS.with(|events| {
            let events = events.borrow();
            let expected = [
                "get_current",
                "entered_ctx",
                "scope_ctor",
                "ctx_enter",
                "trycatch_ctor",
                "string_new",
                "script_compile",
                "script_run",
                "utf8_ctor",
                "utf8_deref",
                "utf8_dtor",
                "trycatch_dtor",
                "ctx_exit",
                "scope_dtor",
            ];
            assert_eq!(events.as_slice(), expected);
        });
    }

    #[test]
    fn lab_exec_script_rejects_null_isolate_before_any_v8_symbol_call() {
        EVENTS.with(|events| events.borrow_mut().clear());
        let result = unsafe { exec_script(&mock_symbols(), core::ptr::null_mut(), "1+1") };
        assert_eq!(result, Err(V8Error::NoCurrentContext));
        EVENTS.with(|events| assert!(events.borrow().is_empty()));
    }

    #[test]
    fn lab_exec_script_does_not_construct_scope_without_a_context() {
        EVENTS.with(|events| events.borrow_mut().clear());
        CONTEXT_SLOT.with(|slot| slot.set(0));
        let result = unsafe { exec_script(&mock_symbols(), 0x1234usize as *mut c_void, "1+1") };
        assert_eq!(result, Err(V8Error::NoCurrentContext));
        EVENTS.with(|events| {
            assert_eq!(
                events.borrow().as_slice(),
                ["get_current", "entered_ctx", "incumbent_ctx"]
            )
        });
    }
}

#[cfg(test)]
mod uid_map_script_tests {
    use super::{
        CURRENT_LOGIN_FINISH_JS, CURRENT_LOGIN_START_JS, UID_MAP_BINDING_JS, UID_MAP_CANCEL_JS, UID_MAP_GROUP_JS,
        UID_MAP_POLL_JS, UID_MAP_PROFILE_METHODS_JS,
        UID_MAP_QUERY_JS, UID_MAP_SANITY_JS, UID_MAP_SESSION_JS, UID_MAP_STATE_JS,
        UID_MAP_SURFACE_DONE_JS, UID_MAP_WRAPPERS_JS, build_profile_uid_map_query_js,
        build_scoped_login_poll, build_scoped_login_start, build_uid_map_methods_js,
        listener_script_with_account,
    };

    #[test]
    fn send_trace_is_enabled_only_in_research_listener_wrapper() {
        let js = listener_script_with_account("10001");
        assert!(js.ends_with(if cfg!(feature = "research") {
            ", true)"
        } else {
            ", false)"
        }));
    }

    #[test]
    fn profile_refresh_requires_request_reply_and_notification_and_retains_unknown_cleanup() {
        let scripts = serde_json::json!({
            "start": CURRENT_LOGIN_START_JS,
            "identity": CURRENT_LOGIN_FINISH_JS,
            "refresh": super::CURRENT_PROFILE_REFRESH_JS,
            "poll": UID_MAP_POLL_JS,
            "cancel": UID_MAP_CANCEL_JS,
            "state": UID_MAP_STATE_JS,
        });
        let fixture = include_str!("../tests/fixtures/profile-refresh-contract.cjs");
        let script = format!("globalThis.caligoRefreshScripts = {scripts};\n{fixture}");
        let result = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", &script]).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        println!("{}", String::from_utf8_lossy(&result.stdout));
    }

    #[test]
    fn scoped_self_profile_rejects_wrong_account_and_ignores_revoked_callbacks() {
        let scripts = serde_json::json!({
            "start": super::build_scoped_self_profile_start("synthetic-wrapper.node", "10001"),
            "poll": super::build_scoped_self_profile_poll(),
            "cancel": UID_MAP_CANCEL_JS,
        });
        let fixture = include_str!("../tests/fixtures/scoped-self-profile-contract.cjs");
        let script = format!("globalThis.caligoScopedProfileScripts = {scripts};\n{fixture}");
        let result = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", &script]).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        println!("{}", String::from_utf8_lossy(&result.stdout));
    }

    #[test]
    fn d8_account_wrapper_returns_the_listener_result() {
        let listener = serde_json::to_string(&listener_script_with_account("10001")).unwrap();
        let script = format!(
            "const assert = require('node:assert/strict');
             globalThis.__caligo_res = {{sid:'nt_1'}};
             assert.equal(eval({listener}), 'ALREADY sid=nt_1');"
        );
        let result = std::process::Command::new("node").args(["-e", &script]).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }

    #[test]
    fn d8_image_diagnostics_preserve_admission_and_omit_values() {
        use std::io::Write;
        let scripts = serde_json::json!({
            "pilot": listener_script_with_account("10001"),
            "poll": super::D8_POLL_JS,
        });
        let fixture = include_str!("../tests/fixtures/receive-image-diagnostics.cjs");
        let mut child = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", fixture])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        child.stdin.take().unwrap().write_all(scripts.to_string().as_bytes()).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }

    #[test]
    fn d8_pinned_wrapper_accepts_native_empty_mention_fields() {
        use std::io::Write;

        let scripts = serde_json::json!({
            "pilot": listener_script_with_account("10001"),
            "legacy": super::D8_START_JS,
            "poll": super::D8_POLL_JS,
        });
        let fixture = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const scripts = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
for (const [profile, expected] of [['pilot', 2], ['legacy', 0]]) {
  let listener;
  const service = {sendMsg() {}, addKernelMsgListener(value) { listener = value; return '1'; }};
  const session = {getSessionId: () => '1', getMsgService: () => service};
  const sandbox = vm.createContext({Buffer, process: {_linkedBinding: () => ({
    NodeIQQNTWrapperSession: {getNTWrapperSession: () => session}
  })}});
  assert.match(vm.runInContext(scripts[profile], sandbox), /^ARMED/);
  const common = {msgId:'5001',msgSeq:'2',msgRandom:'3',msgTime:'1794567890',guildId:'',
    peerUid:'u_peer',peerUin:'20002',senderUid:'u_peer',senderUin:'20002',msgType:2,subMsgType:1};
  const text = {content:'sample',atType:0,atUid:'0',atTinyId:'0',atNtUid:null};
  const row = (chatType, textElement) => ({...common,chatType,
    ...(chatType === 2 ? {peerUid:'30003',peerUin:'30003'} : {}),
    elements:[{elementType:1,textElement}]});
  listener.onRecvMsg([row(1, text), row(2, text), row(1, {...text,atType:2}),
    row(1, {...text,atUid:'20003'}), row(1, {...text,atNtUid:'u_other'})]);
  const batch = JSON.parse(vm.runInContext(scripts.poll, sandbox));
  assert.equal(batch.rejected, 5 - expected);
  assert.equal(batch.items.length, expected);
  for (const {record} of batch.items) {
    assert.deepEqual(record.elements[0].textElement,
      {content:'sample',atType:0,atUid:'',atNtUid:'',atTinyId:''});
  }
}
"#;
        let mut child = std::process::Command::new("node")
            .args(["-e", fixture])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        child.stdin.take().unwrap().write_all(scripts.to_string().as_bytes()).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }

    #[test]
    fn scoped_login_start_rejects_wrong_host_account_before_directory_lookup() {
        let start = build_scoped_login_start("D:\\QQ NT\\wrapper.node", "10001");
        let poll = build_scoped_login_poll();
        let fixture = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const calls = [];
let account = '10002';
const uid = 'u_synthetic';
let listener;
const profile = {
  addKernelProfileListener(value) {listener = value; return '1';},
  removeKernelProfileListener(id) {assert.equal(id, '1'); calls.push('remove');},
  getSelfStatus() {listener.onSelfStatusChanged({uid, uin: account, status: 10}); return {result: 0};},
  getUidByUin(_, keys) {calls.push('forward'); assert.equal(keys[0], account); return new Map([[account, uid]]);},
  getUinByUid(_, keys) {calls.push('reverse'); assert.equal(keys[0], uid); return new Map([[uid, account]]);},
  getCoreAndBaseInfo(_, keys) {
    calls.push('nickname');
    assert.equal(keys[0], uid);
    return new Map([[uid, {coreInfo: {uid, uin: account, nick: 'Synthetic'}}]]);
  }
};
const sandbox = vm.createContext({
  Buffer,
  require(path) {
    assert.equal(path, String.raw`D:\QQ NT\wrapper.node`);
    return {NodeIQQNTWrapperSession: {getNTWrapperSession(key) {
      assert.equal(key, 'nt_1');
      return {getSessionId: () => 'nt_1', getProfileService: () => profile};
    }}};
  }
});
const tick = () => new Promise(resolve => setImmediate(resolve));
(async () => {
  assert.equal(vm.runInContext(start, sandbox), 'DISPATCHED');
  await tick();
  assert.equal(JSON.parse(vm.runInContext(poll, sandbox)).status, 'current_account_mismatch');
  assert.deepEqual(calls, ['remove']);
  account = '10001';
  assert.equal(vm.runInContext(start, sandbox), 'DISPATCHED');
  await tick();
  assert.equal(vm.runInContext(poll, sandbox), 'PENDING');
  await tick();
  const result = JSON.parse(vm.runInContext(poll, sandbox));
  assert.equal(result.status, 'current_login_match');
  assert.equal(result.currentLogin.nickname, 'Synthetic');
  assert.deepEqual(calls, ['remove', 'forward', 'reverse', 'nickname', 'remove']);
})().catch(error => { console.error(error); process.exitCode = 1; });
"#;
        let start = serde_json::to_string(&start).unwrap();
        let poll = serde_json::to_string(&poll).unwrap();
        let script = format!("const start = {start}, poll = {poll};\n{fixture}");
        let result = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", &script])
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }

    #[test]
    fn current_login_uses_host_identity_and_rejects_drift_or_late_settlement() {
        let start = serde_json::to_string(CURRENT_LOGIN_START_JS).unwrap();
        let finish = serde_json::to_string(CURRENT_LOGIN_FINISH_JS).unwrap();
        let poll = serde_json::to_string(UID_MAP_POLL_JS).unwrap();
        let cancel = serde_json::to_string(UID_MAP_CANCEL_JS).unwrap();
        let fixture = r#"
const assert = require('node:assert/strict');
const uin = '1234567890', uid = 'u_synthetic';
const selfStatus = {uin, uid, status: 10};
const selfProfile = () => new Map([[uid, {coreInfo: {uid, uin, nick: 'Synthetic nickname'}}]]);
let sessionId = 'nt_1', mappings = 0, reverseUin = uin, listener, removed = 0;
let readProfile = selfProfile;
function prepare(getSelfStatus) {
  const profile = {
    getSelfStatus,
    addKernelProfileListener(value) {
      assert.equal(typeof value.onProfileDetailInfoChanged, 'function');
      listener = value;
      return '9007199254740993';
    },
    removeKernelProfileListener(id) {
      assert.equal(id, '9007199254740993');
      removed++;
    },
    getCoreAndBaseInfo(source, ids) {
      assert.equal(source, 'nodeStore');
      assert.deepEqual(ids, [uid]);
      return readProfile();
    }
  };
  const state = {
    phase: 'profile_ready', profile, session: {getSessionId: () => sessionId},
    forwardMethod(source, keys) {
      assert.equal(this, profile);
      assert.equal(source, 'FriendsServiceImpl');
      assert.deepEqual(keys, [uin]);
      mappings++;
      return new Map([[uin, uid]]);
    },
    reverseMethod(source, keys) {
      assert.equal(this, profile);
      assert.equal(source, 'FriendsServiceImpl');
      assert.deepEqual(keys, [uid]);
      mappings++;
      return new Map([[uid, reverseUin]]);
    }
  };
  globalThis.__caligo_uid_map_main = state;
  return state;
}
const tick = () => new Promise(resolve => setImmediate(resolve));
async function run(event, result = 0, eventFirst = true) {
  prepare(function () {
    assert.equal(arguments.length, 0);
    if (eventFirst && event) listener.onSelfStatusChanged(event);
    return Promise.resolve({result, errMsg: ''});
  });
  assert.equal(eval(start), 'DISPATCHED');
  assert.equal(eval(finish), 'POLL');
  assert.equal(eval(poll), 'PENDING');
  await tick();
  if (!eventFirst && event) listener.onSelfStatusChanged(event);
  assert.equal(eval(finish), 'POLL');
  await tick();
  assert.equal(eval(finish), 'POLL');
  return JSON.parse(eval(poll));
}
(async () => {
  let result = await run(selfStatus);
  assert.equal(result.status, 'current_login_match');
  assert.deepEqual(result.currentLogin, {...selfStatus, session_id: 'nt_1', nickname: 'Synthetic nickname'});
  assert.equal(result.nicknameStatus, 'matched');
  assert.equal(result.listenerRegistration, 'remove_returned');
  assert.equal(mappings, 2);
  assert.equal((await run(selfStatus, 0, false)).status, 'current_login_match');
  assert.equal((await run(selfStatus, 1)).status, 'self_status_ack_failed');
  assert.equal(mappings, 4);
  reverseUin = '1234567891';
  result = await run(selfStatus);
  assert.equal(result.status, 'self_reverse_mismatch');
  assert.equal(result.currentLogin, undefined);
  let resolveStatus;
  const old = prepare(() => new Promise(resolve => {resolveStatus = resolve;}));
  assert.equal(eval(start), 'DISPATCHED');
  listener.onSelfStatusChanged(selfStatus);
  sessionId = 'nt_2';
  resolveStatus({result: 0});
  await tick();
  eval(finish);
  assert.equal(JSON.parse(eval(poll)).status, 'current_session_changed');
  assert.equal(mappings, 6);
  const cancelled = prepare(() => new Promise(resolve => {resolveStatus = resolve;}));
  assert.equal(eval(start), 'DISPATCHED');
  const late = listener;
  assert.equal(eval(cancel), 'CANCELLED');
  const newer = prepare(() => selfStatus);
  late.onSelfStatusChanged(selfStatus);
  resolveStatus({result: 0});
  await tick();
  assert.equal(cancelled.phase, 'cancelled');
  assert.equal(cancelled.selfStatus, undefined);
  assert.equal(newer.phase, 'profile_ready');
  assert.equal(old.currentLogin, undefined);
  assert.equal(mappings, 6);
  assert.equal(removed, 6);
  prepare(() => Promise.resolve({result: 0}));
  assert.equal(eval(start), 'DISPATCHED');
  await tick();
  eval(finish);
  assert.equal(eval(poll), 'PENDING');
  assert.equal(eval(cancel), 'CANCELLED');
  assert.equal(removed, 7);
  reverseUin = uin;
  readProfile = () => new Map([['u_other', {coreInfo: {uid, uin, nick: 'Wrong key'}}]]);
  result = await run(selfStatus);
  assert.equal(result.nicknameStatus, 'profile_identity_mismatch');
  assert.equal(result.currentLogin.nickname, undefined);
  readProfile = () => new Map([[uid, {coreInfo: {uid, uin: '1234567891', nick: 'Wrong UIN'}}]]);
  result = await run(selfStatus);
  assert.equal(result.nicknameStatus, 'profile_identity_mismatch');
  assert.equal(result.currentLogin.nickname, undefined);
  readProfile = () => new Map([[uid, {coreInfo: {uid, uin, nick: '😀'.repeat(1025)}}]]);
  assert.equal((await run(selfStatus)).nicknameStatus, 'nickname_invalid');
  readProfile = () => Promise.reject(new Error('test rejection'));
  assert.equal((await run(selfStatus)).nicknameStatus, 'query_rejected');
  let resolveProfile;
  readProfile = () => new Promise(resolve => {resolveProfile = resolve;});
  prepare(() => {listener.onSelfStatusChanged(selfStatus); return {result: 0};});
  assert.equal(eval(start), 'DISPATCHED');
  await tick();
  eval(finish);
  sessionId = 'nt_3';
  resolveProfile(selfProfile());
  await tick();
  eval(finish);
  assert.equal(JSON.parse(eval(poll)).status, 'current_session_changed');
  prepare(() => {listener.onSelfStatusChanged(selfStatus); return {result: 0};});
  assert.equal(eval(start), 'DISPATCHED');
  await tick();
  eval(finish);
  listener.onSelfStatusChanged({uin: '1234567891', uid: 'u_other', status: 10});
  resolveProfile(selfProfile());
  await tick();
  result = JSON.parse(eval(poll));
  assert.equal(result.status, 'self_status_conflict');
  assert.equal(result.currentLogin, undefined);
  prepare(() => {listener.onSelfStatusChanged(selfStatus); return {result: 0};});
  assert.equal(eval(start), 'DISPATCHED');
  await tick();
  eval(finish);
  listener.onSelfStatusChanged({...selfStatus, status: 20});
  resolveProfile(selfProfile());
  await tick();
  result = JSON.parse(eval(poll));
  assert.equal(result.status, 'self_status_offline');
  assert.equal(result.currentLogin, undefined);
})().catch(error => { console.error(error); process.exitCode = 1; });
"#;
        let script =
            format!("const start = {start}, finish = {finish}, poll = {poll}, cancel = {cancel};\n{fixture}");
        let output = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", &script])
            .output()
            .expect("Node.js is required for the research JavaScript fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn uid_map_probe_uses_the_direct_qqnt_wrapper_surface_and_two_fixed_methods() {
        let binding = UID_MAP_BINDING_JS;
        let sanity = UID_MAP_SANITY_JS;
        let state = UID_MAP_STATE_JS;
        let wrappers = UID_MAP_WRAPPERS_JS;
        let session = UID_MAP_SESSION_JS;
        let group = UID_MAP_GROUP_JS;
        let methods = build_uid_map_methods_js("1234567890");
        assert!(!sanity.contains("process._linkedBinding"));
        assert!(!state.contains("process._linkedBinding"));
        assert!(binding.contains("process._linkedBinding('QQNT')"));
        assert!(!binding.contains("NodeIQQNTWrapperSession"));
        assert!(wrappers.contains("NodeIQQNTStartupSessionWrapper"));
        assert!(wrappers.contains("NodeIQQNTWrapperSession"));
        assert!(session.contains("getSessionIdList()"));
        assert!(group.contains("getGroupService()"));
        assert!(methods.contains("S.group.getUidByUins"));
        assert!(methods.contains("S.group.getUinByUids"));
        assert!(UID_MAP_QUERY_JS.contains("forwardMethod.call(S.group, [S.uin])"));
        assert!(UID_MAP_QUERY_JS.contains("reverseMethod.call(S.group, [uids])"));
        assert!(!UID_MAP_QUERY_JS.contains("ipcImpl"));
        assert!(!UID_MAP_QUERY_JS.contains("executeJavaScript"));
        assert!(!binding.contains("addKernelMsgListener"));
        assert!(!UID_MAP_QUERY_JS.contains("sendMsg("));
    }

    #[test]
    fn uid_map_probe_json_escapes_its_single_uin_argument() {
        let input = "123\";globalThis.bad=true;//";
        let js = build_uid_map_methods_js(input);
        let encoded = serde_json::to_string(input).unwrap();
        assert!(js.contains(&format!("S.uin = {encoded};")));
        assert!(!js.contains("S.uin = \"123\";globalThis.bad"));
    }

    #[test]
    fn uid_map_surface_only_completion_records_availability_without_resolver_dispatch() {
        assert!(UID_MAP_SURFACE_DONE_JS.contains("group_service_available"));
        assert!(UID_MAP_SURFACE_DONE_JS.contains("S.apiCallAttempted = false"));
        assert!(!UID_MAP_SURFACE_DONE_JS.contains("getUidByUins"));
        assert!(!UID_MAP_SURFACE_DONE_JS.contains("getUinByUids"));
        assert!(!UID_MAP_SURFACE_DONE_JS.contains(".call("));
    }

    #[test]
    fn profile_uid_map_uses_the_loaded_wrapper_and_reference_session_key() {
        let surface = serde_json::to_string(UID_MAP_PROFILE_METHODS_JS).unwrap();
        let fixture = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const path = String.raw`D:\QQ NT\wrapper.node`;
const calls = [];
const profile = {
  getUidByUin() { assert.fail('surface must not dispatch a resolver'); },
  getUinByUid() { assert.fail('surface must not dispatch a resolver'); }
};
const session = { getProfileService() { assert.equal(this, session); return profile; } };
const sessions = {
  getNTWrapperSession(key) {
    assert.equal(this, sessions);
    calls.push(key);
    return session;
  }
};
const host = {
  __caligo_uid_map_main: {phase: 'binding'},
  process: {
    _linkedBinding() { assert.fail('ProfileService must not use the startup wrapper'); },
    mainModule: {
      require(candidate) {
        assert.equal(this, host.process.mainModule);
        calls.push(candidate);
        return { NodeIQQNTWrapperSession: sessions };
      }
    }
  }
};
const run = () => vm.runInNewContext('(' + surface + ')(' + JSON.stringify(path) + ')', host);
assert.equal(run(), 'PROFILE_READY');
assert.deepEqual(calls, [path, 'nt_1']);
assert.equal(host.__caligo_uid_map_main.session, session);
assert.equal(host.__caligo_uid_map_main.profile, profile);

host.__caligo_uid_map_main = {phase: 'binding'};
host.require = candidate => {
  assert.equal(candidate, path);
  return {};
};
assert.equal(run(), 'SURFACE_DONE');
assert.equal(host.__caligo_uid_map_main.status, 'profile_wrapper_accessor_missing');
assert.equal(host.__caligo_uid_map_main.apiCallAttempted, undefined);

host.__caligo_uid_map_main = {phase: 'binding'};
delete host.require;
delete host.process.mainModule;
assert.equal(run(), 'SURFACE_DONE');
assert.equal(host.__caligo_uid_map_main.status, 'profile_require_missing');
"#;
        let script = format!("const surface = {surface};\n{fixture}");
        let output = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", &script])
            .output()
            .expect("Node.js is required for the research JavaScript fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn profile_uid_map_checks_exact_keys_and_rejects_unexpected_results() {
        let surface = serde_json::to_string(UID_MAP_PROFILE_METHODS_JS).unwrap();
        let query =
            serde_json::to_string(&build_profile_uid_map_query_js("9007199254740993")).unwrap();
        let poll = serde_json::to_string(UID_MAP_POLL_JS).unwrap();
        let fixture = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const uin = '9007199254740993';
const uid = 'u_synthetic-uid';
const wrapperPath = String.raw`D:\QQ NT\wrapper.node`;
function acquire(getSession) {
  globalThis.__caligo_uid_map_main = { phase: 'binding' };
  const sandbox = {
    __caligo_uid_map_main: globalThis.__caligo_uid_map_main,
    require(path) {
      assert.equal(path, wrapperPath);
      return { NodeIQQNTWrapperSession: { getNTWrapperSession: getSession } };
    }
  };
  const result = vm.runInNewContext('(' + surface + ')(' + JSON.stringify(wrapperPath) + ')', sandbox);
  if (sandbox.__caligo_uid_map_main === undefined) delete globalThis.__caligo_uid_map_main;
  return result;
}

function prepare(service) {
  const session = { getProfileService: () => service };
  const outcome = acquire(name => {
    assert.equal(name, 'nt_1');
    return session;
  });
  if (outcome === 'PROFILE_READY') {
    assert.equal(globalThis.__caligo_uid_map_main.session, session);
    assert.equal(globalThis.__caligo_uid_map_main.profile, service);
  }
  return outcome;
}

function verify(forward, reverse, expected, expectReverse) {
  const calls = [];
  const service = {
    getUidByUin(source, keys) {
      assert.equal(this, service);
      assert.equal(source, 'FriendsServiceImpl');
      assert.deepEqual(keys, [uin]);
      calls.push('forward');
      return forward();
    },
    getUinByUid(source, keys) {
      assert.equal(this, service);
      assert.equal(source, 'FriendsServiceImpl');
      assert.deepEqual(keys, [uid]);
      calls.push('reverse');
      return reverse();
    }
  };
  assert.equal(prepare(service), 'PROFILE_READY');
  assert.deepEqual(calls, []);
  assert.equal(eval(query), 'DISPATCHED');
  assert.equal(eval(query).startsWith('ERR:'), true);
  assert.deepEqual(calls, expectReverse ? ['forward', 'reverse'] : ['forward']);
  const result = JSON.parse(eval(poll));
  assert.equal(result.status, expected);
  assert.equal(result.apiCallAttempted, true);
  assert.deepEqual(result.uidCandidates, expectReverse ? [uid] : []);
  assert.equal(globalThis.__caligo_uid_map_main, undefined);
}

assert.equal(prepare(null), 'SURFACE_DONE');
assert.equal(eval(query).startsWith('ERR:'), true);
let result = JSON.parse(eval(poll));
assert.equal(result.status, 'profile_service_missing');
assert.equal(result.apiCallAttempted, false);
assert.equal(prepare({ getUidByUin() { throw new Error('must not call'); } }), 'SURFACE_DONE');
result = JSON.parse(eval(poll));
assert.equal(result.status, 'profile_methods_unavailable');
assert.equal(result.apiCallAttempted, false);

const found = () => new Map([[uin, uid]]);
const inverse = () => new Map([[uid, uin]]);
verify(found, inverse, 'roundtrip_match', true);
verify(() => new Map([['other', uid]]), inverse, 'forward_no_uid', false);
verify(() => new Map([[uin, 'u_masked*']]), inverse, 'forward_invalid_uid', false);
verify(() => Promise.reject(new Error('unexpected async provider')), inverse, 'forward_result_not_map', false);
verify(() => vm.runInNewContext('Promise.resolve(new Map())'), inverse, 'forward_result_not_map', false);
verify(() => vm.runInNewContext('Promise.reject(new Error("foreign rejection"))'), inverse, 'forward_result_not_map', false);
verify(() => ({ then(resolve, reject) { setImmediate(() => reject(new Error('late rejection'))); } }), inverse, 'forward_result_not_map', false);
verify(() => { throw new Error('lookup failed'); }, inverse, 'forward_threw', false);
verify(found, () => ({ [uid]: uin }), 'reverse_result_not_map', true);
verify(found, () => new Map([['u_other', uin]]), 'reverse_no_uin', true);
verify(found, () => new Map([[uid, Number(uin)]]), 'reverse_invalid_uin', true);
verify(found, () => new Map([[uid, '9007199254740992']]), 'roundtrip_no_match', true);
verify(found, () => vm.runInNewContext('Promise.resolve(new Map())'), 'reverse_result_not_map', true);
verify(found, () => vm.runInNewContext('Promise.reject(new Error("foreign rejection"))'), 'reverse_result_not_map', true);
verify(found, () => ({ then(resolve, reject) { setImmediate(() => reject(new Error('late rejection'))); } }), 'reverse_result_not_map', true);

function acquisitionThrows(getSession) {
  assert.equal(acquire(getSession), 'ERR:profile service acquisition failed');
  assert.equal(globalThis.__caligo_uid_map_main, undefined);
  // A failed acquisition must not poison the next state's BUSY guard.
  assert.equal(prepare({ getUidByUin() {}, getUinByUid() {} }), 'PROFILE_READY');
  delete globalThis.__caligo_uid_map_main;
}
function fail() { throw new Error('synthetic acquisition failure'); }
acquisitionThrows(fail);
acquisitionThrows(() => ({ get getProfileService() { return fail(); } }));
acquisitionThrows(() => ({ getProfileService: fail }));
for (const name of ['getUidByUin', 'getUinByUid']) {
  const service = { getUidByUin() {}, getUinByUid() {} };
  Object.defineProperty(service, name, { get: fail });
  acquisitionThrows(() => ({ getProfileService: () => service }));
  const arityService = { getUidByUin() {}, getUinByUid() {} };
  arityService[name] = new Proxy(function () {}, {
    get(target, key) { if (key === 'length') return fail(); return Reflect.get(target, key); }
  });
  acquisitionThrows(() => ({ getProfileService: () => arityService }));
}
"#;
        let script =
            format!("const surface = {surface}, query = {query}, poll = {poll};\n{fixture}");
        let output = std::process::Command::new("node")
            .args(["--unhandled-rejections=strict", "-e", &script])
            .output()
            .expect("Node.js is required for the research JavaScript fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// D9 voice submission consumes owned bytes. Its caller must use the exact
/// profile gate when arming the listener and the durable send path for dispatch.
pub fn build_voice_send_js(
    chat_type: u32,
    account: &str,
    peer_uin: &str,
    request_id: &str,
    wire_seq: u32,
    timeout_ms: u64,
    silk: &[u8],
) -> Option<String> {
    if !caligo_model::voice::validate_short_tencent_silk(silk) {
        return None;
    }
    let mut hex = String::with_capacity(silk.len() * 2);
    use std::fmt::Write;
    for byte in silk {
        write!(hex, "{byte:02x}").expect("writing to String is infallible");
    }
    let args = serde_json::json!({
        "chatType": chat_type, "account": account, "peerUin": peer_uin, "requestId": request_id,
        "wireSeq": wire_seq, "timeoutMs": timeout_ms, "silk": hex
    });
    Some(format!("({})({args})", include_str!("qq_voice_send.js")))
}

/// Fixed PNG preparation/submission contract used by scoped durable image dispatch.
pub fn build_image_send_js(
    chat_type: u32, account: &str, peer_uin: &str, request_id: &str,
    wire_seq: u32, timeout_ms: u64, png: &[u8],
) -> Option<String> {
    let (width,height) = caligo_model::png::dimensions(png)?;
    let mut hex = String::with_capacity(png.len()*2);
    use std::fmt::Write;
    for byte in png { write!(hex,"{byte:02x}").ok()?; }
    let args = serde_json::json!({"chatType":chat_type,"account":account,"peerUin":peer_uin,
        "requestId":request_id,"wireSeq":wire_seq,"timeoutMs":timeout_ms,"png":hex,"width":width,"height":height});
    Some(format!("({})({args})",include_str!("qq_image_send.js")))
}

/// D9 fixed wrapper text send. Correlation stays local; a native ID is returned
/// only after exact event identity and getMsgsByMsgId readback agree.
pub fn build_send_js(
    chat_type: u32,
    peer_uid: &str,
    text: &str,
    account: &str,
    request_id: &str,
    wire_seq: u32,
    timeout_ms: u64,
) -> String {
    let peer_json = serde_json::to_string(peer_uid).unwrap_or_default();
    let text_json = serde_json::to_string(text).unwrap_or_default();
    let request_json = serde_json::to_string(request_id).unwrap_or_default();
    let account_json = serde_json::to_string(account).unwrap_or_default();
    let settle = r#"function(R,key,pending){
      if (!pending || !pending.promiseDone || !pending.readback) return;
      if (pending.ambiguous || pending.promiseStatus !== 'accepted' || pending.readback.status !== 'readback') {
        R.sendResults[key] = {status:'unknown', reason:pending.ambiguous ? 'multiple_add_send_ids' :
          (pending.promiseStatus !== 'accepted' ? (pending.unknownReason || 'promise_event_conflict') : pending.readback.reason)};
      } else {
        R.sendResults[key] = {status:'done', msgId:pending.readback.msgId};
      }
      pending.done = true;
      if (R.sends[key] === pending) delete R.sends[key];
    }"#;
    let mut js = String::with_capacity(2048);
    js.push_str("(function () {\n");
    js.push_str("  try {\n");
    js.push_str("    var G = globalThis;\n");
    js.push_str("    if (!G.__caligo_res) return JSON.stringify({ err: 'NOT_ARMED' });\n");
    js.push_str("    var R = G.__caligo_res;\n");
    js.push_str("    R.sendResults = R.sendResults || {};\n");
    js.push_str("    var chat = ");
    js.push_str(&chat_type.to_string());
    js.push_str(";\n");
    js.push_str("    var peerUid = ");
    js.push_str(&peer_json);
    js.push_str(";\n");
    js.push_str("    var sv; try { sv = R.session.getMSFService().getServerTime(); } catch (e) { return JSON.stringify({err:'server_time_unavailable'}); }\n");
    js.push_str("    if (typeof sv !== 'string' || !sv || sv === '0') return JSON.stringify({err:'server_time_unavailable'});\n");
    js.push_str("    var cookie = R.ms.generateMsgUniqueId(chat, sv);\n");
    js.push_str(
        "    if (typeof cookie !== 'string' || !cookie || cookie === '0' || Object.values(R.sends).some(send => send.cookie === cookie)) return JSON.stringify({err:'invalid_cookie'});\n",
    );
    js.push_str("    var selfUin = ");
    js.push_str(&account_json);
    js.push_str(";\n");
    js.push_str("    var targetUin = peerUid;\n");
    js.push_str("    if (chat === 1) {\n");
    js.push_str("      if (!/^[1-9][0-9]{0,19}$/.test(targetUin)) return JSON.stringify({err:'private_target_uin_invalid'});\n");
    js.push_str("      var profile = R.session.getProfileService();\n");
    js.push_str("      if (!profile || typeof profile.getUidByUin !== 'function') return JSON.stringify({err:'private_uid_mapping_unavailable'});\n");
    js.push_str("      var mapped = profile.getUidByUin('FriendsServiceImpl', [targetUin]);\n");
    js.push_str("      if (!(mapped instanceof Map) || !Map.prototype.has.call(mapped, targetUin)) return JSON.stringify({err:'private_uid_mapping_missing'});\n");
    js.push_str("      peerUid = Map.prototype.get.call(mapped, targetUin);\n");
    js.push_str("      if (typeof peerUid !== 'string' || !/^u_[A-Za-z0-9_-]{1,128}$/.test(peerUid)) return JSON.stringify({err:'private_uid_mapping_invalid'});\n");
    js.push_str("    }\n");
    js.push_str("    var peer = { chatType: chat, guildId: cookie, peerUid: peerUid };\n");
    js.push_str("    var elems = [{ elementType: 1, textElement: { content: ");
    js.push_str(&text_json);
    js.push_str(" } }];\n");
    js.push_str("    var settleSendResult = ");
    js.push_str(settle);
    js.push_str(";\n");
    js.push_str("    var key = ");
    js.push_str(&request_json);
    js.push_str(";\n");
    js.push_str("    if (Object.prototype.hasOwnProperty.call(R.sends, key) || Object.prototype.hasOwnProperty.call(R.sendResults, key) || Object.keys(R.sends).length >= 128 || Object.keys(R.sendResults).length >= 128) return JSON.stringify({err:'send_capacity_or_duplicate'});\n");
    // Retain every text cookie in this JS environment: a late notification from a
    // completed send must not be able to confirm a later same-cookie send.
    // Bound memory and fail closed on exhaustion rather than evicting old cookies.
    js.push_str(r#"    var guardKey = '__caligo_text_cookie_guard_v1';
    if (!Object.prototype.hasOwnProperty.call(G, guardKey)) {
      var seenCookies = new Set();
      var claimCookie = function (value) {
        if (typeof value !== 'string' || !value || value === '0' || value.length > 256) return 'invalid_cookie';
        if (seenCookies.has(value)) return 'invalid_cookie';
        if (seenCookies.size >= 65536) return 'cookie_history_exhausted';
        seenCookies.add(value); return 'ok';
      };
      Object.defineProperty(G, guardKey, {value:claimCookie, writable:false, configurable:false, enumerable:false});
    }
    var guardDescriptor = Object.getOwnPropertyDescriptor(G, guardKey);
    if (!guardDescriptor || typeof guardDescriptor.value !== 'function' || guardDescriptor.writable !== false || guardDescriptor.configurable !== false) return JSON.stringify({err:'cookie_history_unavailable'});
    var cookieClaim = guardDescriptor.value(cookie);
    if (cookieClaim !== 'ok') return JSON.stringify({err:cookieClaim});
"#);
    js.push_str("    var pending = {cookie:cookie, chat:chat, peer:peerUid, text:");
    js.push_str(&text_json);
    js.push_str(", selfUin:selfUin, wireSeq:");
    js.push_str(&wire_seq.to_string());
    js.push_str(", traceSeq:typeof R.allocateTextTrace === 'function' ? R.allocateTextTrace() : 0, deadline:Date.now()+");
    js.push_str(&timeout_ms.to_string());
    js.push_str(", addId:null, updateId:null, done:false};\n");
    js.push_str("    R.sends[key] = pending; R.sendResults[key] = {status:'pending'};\n");
    js.push_str("    if (R.noteTextSend) R.noteTextSend('registered', pending.traceSeq);\n");
    js.push_str("    function result(status, detail) {\n");
    js.push_str("      if (R.stopped || R.sends[key] !== pending) return;\n");
    js.push_str("      if (R.noteTextSend) R.noteTextSend(status === 'accepted' ? 'promise_accepted' : (status === 'rejected' ? 'promise_rejected' : 'promise_unknown'), pending.traceSeq);\n");
    js.push_str("      pending.promiseDone=true; pending.promiseStatus=status;\n");
    js.push_str("      if (status === 'unknown') pending.unknownReason=detail.reason;\n");
    js.push_str("      if (pending.readback) settleSendResult(R,key,pending);\n");
    js.push_str("      else if (!pending.ambiguous && !pending.timedOut) R.sendResults[key]=detail;\n");
    js.push_str("      if (status !== 'accepted') pending.failed=true;\n");
    js.push_str("    }\n");
    js.push_str("    function unknown(reason) { result('unknown', {status:'unknown', reason:typeof reason === 'string' ? reason : 'text_submit_unknown'}); }\n");
    js.push_str("    function observePromise(kind) {\n");
    js.push_str("      try { if (R.beginTextClosureObservation) return R.beginTextClosureObservation(kind, key, pending); if (R.observeCallback) R.observeCallback(kind, 'closure', key, pending); } catch (_) {}\n");
    js.push_str("      return null;\n");
    js.push_str("    }\n");
    js.push_str("    try { if (R.noteTextSend) R.noteTextSend('submit_enter', pending.traceSeq); var pr = R.ms.sendMsg('0', peer, elems, new Map()); if (R.noteTextSend) R.noteTextSend('submit_return', pending.traceSeq); Promise.resolve(pr).then(function (res) {\n");
    js.push_str("      try { if (R.noteCallbackThread) R.noteCallbackThread('promise'); } catch (_) {}\n");
    js.push_str("      var finishObservation = observePromise('promise_fulfilled');\n");
    js.push_str("      try {\n");
    js.push_str("        if (res && res.result === 0) result('accepted', {status:'accepted'});\n");
    js.push_str("        else if (res && Number.isSafeInteger(res.result)) unknown('send_result_ambiguous=' + String(res.result));\n");
    js.push_str("        else unknown();\n");
    js.push_str("      } finally { try { if (finishObservation) finishObservation(); } catch (_) {} }\n");
    js.push_str("    }, function () {\n");
    js.push_str("      try { if (R.noteCallbackThread) R.noteCallbackThread('promise'); } catch (_) {}\n");
    js.push_str("      var finishObservation = observePromise('promise_rejected');\n");
    js.push_str("      try { unknown(); } finally { try { if (finishObservation) finishObservation(); } catch (_) {} }\n");
    js.push_str("    }); } catch (e) { unknown(); }\n");
    js.push_str("    return JSON.stringify({fired:true, mid:key});\n");
    js.push_str("  } catch (e) { return JSON.stringify({ err: String(e).slice(0, 300) }); }\n");
    js.push_str("})()\n");
    js
}

pub fn run_fixed_query_script(js: &str) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    unsafe { exec_script(&syms, isolate_addr(), js) }
}

/// D9 发送:返回原始输出(fired JSON / NOT_ARMED JSON / ERR JSON)。
pub fn send_text(
    chat_type: u32,
    peer_uid: &str,
    text: &str,
    account: &str,
    request_id: &str,
    wire_seq: u32,
    timeout_ms: u64,
) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    let js = build_send_js(
        chat_type, peer_uid, text, account, request_id, wire_seq, timeout_ms,
    );
    // SAFETY: 阶梯自管全部检查;调用方保证 owner 线程。
    unsafe { exec_script(&syms, isolate_addr(), &js) }
}
