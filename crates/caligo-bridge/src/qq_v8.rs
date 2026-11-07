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

const NAME_HANDLE_SCOPE_CTOR: &str = "??0HandleScope@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_HANDLE_SCOPE_DTOR: &str = "??1HandleScope@v8@@QEAA@XZ";
const NAME_ISOLATE_GETCURRENT: &str = "?GetCurrent@Isolate@v8@@SAPEAV12@XZ";
const NAME_ISOLATE_ENTERED_CTX: &str =
    "?GetEnteredOrMicrotaskContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_ISOLATE_INCUMBENT_CTX: &str =
    "?GetIncumbentContext@Isolate@v8@@QEAA?AV?$Local@VContext@v8@@@2@XZ";
const NAME_STRING_NEW_FROM_UTF8: &str =
    "?NewFromUtf8@String@v8@@SA?AV?$MaybeLocal@VString@v8@@@2@PEAVIsolate@2@PEBDW4NewStringType@2@H@Z";
const NAME_SCRIPT_COMPILE: &str =
    "?Compile@Script@v8@@SA?AV?$MaybeLocal@VScript@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VString@v8@@@2@PEAVScriptOrigin@2@@Z";
const NAME_SCRIPT_RUN: &str =
    "?Run@Script@v8@@QEAA?AV?$MaybeLocal@VValue@v8@@@2@V?$Local@VContext@v8@@@2@V?$Local@VData@v8@@@2@@Z";
const NAME_UTF8_CTOR: &str =
    "??0Utf8Value@String@v8@@QEAA@PEAVIsolate@2@V?$Local@VValue@v8@@@2@@Z";
const NAME_UTF8_DTOR: &str = "??1Utf8Value@String@v8@@QEAA@XZ";
const NAME_UTF8_DEREF: &str = "??DUtf8Value@String@v8@@QEAAPEADXZ";
const NAME_CTX_ENTER: &str = "?Enter@Context@v8@@QEAAXXZ";
const NAME_CTX_EXIT: &str = "?Exit@Context@v8@@QEAAXXZ";
const NAME_TRYCATCH_CTOR: &str = "??0TryCatch@v8@@QEAA@PEAVIsolate@1@@Z";
const NAME_TRYCATCH_DTOR: &str = "??1TryCatch@v8@@QEAA@XZ";
const NAME_TRYCATCH_CAUGHT: &str = "?HasCaught@TryCatch@v8@@QEBA_NXZ";
const NAME_TRYCATCH_EXC: &str =
    "?Exception@TryCatch@v8@@QEBA?AV?$Local@VValue@v8@@@2@XZ";

type FnScopeCtor = unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void) -> *mut c_void;
type FnScopeDtor = unsafe extern "C" fn(this: *mut c_void);
type FnGetCurrent = unsafe extern "C" fn() -> *mut c_void;
type FnIsolateCtx = unsafe extern "C" fn(isolate: *mut c_void, sret: *mut usize);
type FnStringNewFromUtf8 =
    unsafe extern "C" fn(sret: *mut usize, isolate: *mut c_void, data: *const u8, ty: i32, len: i32);
type FnScriptCompile = unsafe extern "C" fn(sret: *mut usize, ctx: usize, src: usize, origin: *mut c_void);
type FnScriptRun = unsafe extern "C" fn(this: usize, sret: *mut usize, ctx: usize, data: usize);
type FnUtf8Ctor = unsafe extern "C" fn(this: *mut c_void, isolate: *mut c_void, value: usize) -> *mut c_void;
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
                append_stage(report, "v8_resolve", false, &format!("export missing: {name}"));
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
        compile(&mut script_local, ctx, src_local, origin.as_mut_ptr().cast());
        if script_local == 0 {
            return Err(V8Error::CompileFailed);
        }
        let mut result_local = 0usize;
        run(script_local, &mut result_local, ctx, 0 /*空 data Local*/);
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
    if (G.__caligo_res) return 'ALREADY sid=' + G.__caligo_res.sid;
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
    var R = { sid: sidName, session: live, ms: ms, recv: [], seq: 0, listenerId: null, sendResults: {} };
    var push = function (src, m) {
      try {
        var txt = '';
        var els = m.elements || [];
        for (var j = 0; j < els.length; j++) {
          if (els[j] && els[j].textElement) txt += els[j].textElement.content || '';
        }
        R.recv.push({
          seq: R.seq++, ts: Date.now(), src: src,
          msgId: String(m.msgId || '').slice(0, 40),
          chatType: m.chatType,
          peerUid: String(m.peerUid || '').slice(0, 48),
          peerUin: String(m.peerUin || '').slice(0, 20),
          senderUid: String(m.senderUid || '').slice(0, 48),
          senderUin: String(m.senderUin || '').slice(0, 20),
          msgTime: (typeof m.msgTime === 'number') ? m.msgTime : null,
          sendStatus: m.sendStatus, msgType: m.msgType, subMsgType: m.subMsgType,
          text: txt.slice(0, 8000)
        });
        if (R.recv.length > 200) R.recv.splice(0, R.recv.length - 200);
      } catch (err) {}
    };
    R.listener = {
      onRecvMsg: function (a) {
        try {
          var arr = Array.isArray(a) ? a : (a && a.msgList ? a.msgList : [a]);
          for (var i = 0; i < arr.length; i++) push('recv', arr[i]);
        } catch (err) {}
      },
      onMsgInfoListUpdate: function (a) {
        try {
          var arr = Array.isArray(a) ? a : (a && a.msgList ? a.msgList : [a]);
          for (var i = 0; i < arr.length; i++) push('update', arr[i]);
        } catch (err) {}
      }
    };
    R.listenerId = ms.addKernelMsgListener(R.listener);
    G.__caligo_res = R;
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
    var out = JSON.stringify({ sid: R.sid, n: R.recv.length, items: R.recv, sendResults: R.sendResults || {} });
    R.recv = [];
    return out.slice(0, 900000);
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
    try { r = String(R.ms.removeKernelMsgListener(R.listenerId)); } catch (e) { r = 'ERR:' + String(e).slice(0, 100); }
    var sid = R.sid;
    delete G.__caligo_res;
    return 'STOPPED sid=' + sid + ' remove=' + r;
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
";

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
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 阶梯内自管全部检查;调用方保证 owner 线程。
    unsafe { exec_script(&syms, isolate_addr(), D8_POLL_JS) }
}

/// 注册监听器(D8_START_JS)。返回 "ARMED sid=.. lidRet=N" / "ALREADY .." / "ERR:.."。
pub fn start_listener() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 同上。
    unsafe { exec_script(&syms, isolate_addr(), D8_START_JS) }
}

/// 对称移除监听器(D8_STOP_JS)。返回 "STOPPED .. remove=.." / "NOT_ARMED"。
pub fn stop_listener() -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    // SAFETY: 同上。
    unsafe { exec_script(&syms, isolate_addr(), D8_STOP_JS) }
}

/// 固定语义 UID/UIN 双向查询：直接复用 QQNT wrapper session 的 GroupService。
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
    S.phase = 'group';
    return 'SESSION_READY';
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_GROUP_JS: &str = r#"
(function () {
  try {
    var G = globalThis, S = G.__caligo_uid_map_main;
    if (!S || S.phase !== 'group') return 'ERR:group stage is not ready';
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
      error: S.error
    };
    delete G.__caligo_uid_map_main;
    return JSON.stringify(done);
  } catch (e) { return 'ERR:' + String(e).slice(0, 300); }
})()
"#;

const UID_MAP_CANCEL_JS: &str = r#"
(function () {
  delete globalThis.__caligo_uid_map_main;
  return 'CANCELLED';
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
    use super::{exec_script, V8Error, V8Symbols};
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
        build_uid_map_methods_js, UID_MAP_BINDING_JS, UID_MAP_GROUP_JS, UID_MAP_QUERY_JS,
        UID_MAP_SANITY_JS, UID_MAP_SESSION_JS, UID_MAP_STATE_JS, UID_MAP_WRAPPERS_JS,
    };

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
}

/// D9 发送模板:占位 {chat}(数字)、{peer}/{text}(JSON 字符串)。
/// fired 后 Promise 结果写入 R.sendResults[mid](done 带结果 / rejected 带错误),
/// 由轮询回收 —— §6.6:fired 非终态,mid 为 receipt 第一分类(候选 ID)。
pub fn build_send_js(chat_type: u32, peer_uid: &str, text: &str) -> String {
    let peer_json = serde_json::to_string(peer_uid).unwrap_or_default();
    let text_json = serde_json::to_string(text).unwrap_or_default();
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
    js.push_str("    var sv = 0;\n");
    js.push_str("    try { sv = R.session.getMSFService().getServerTime(); } catch (e) {}\n");
    js.push_str("    var mid = String(R.ms.generateMsgUniqueId(chat, sv));\n");
    js.push_str("    var peer = { chatType: chat, guildId: mid, peerUid: peerUid };\n");
    js.push_str("    var elems = [{ elementType: 1, textElement: { content: ");
    js.push_str(&text_json);
    js.push_str(" } }];\n");
    js.push_str("    var pr = R.ms.sendMsg('0', peer, elems, new Map());\n");
    js.push_str("    R.sendResults[mid] = { status: 'pending', ts: Date.now() };\n");
    js.push_str("    if (pr && typeof pr.then === 'function') {\n");
    js.push_str("      pr.then(function (res) {\n");
    js.push_str("        try {\n");
    js.push_str("          var rec = JSON.parse(JSON.stringify(res));\n");
    js.push_str("          var nid = '';\n");
    js.push_str("          try { nid = String(rec.msgId || (rec.msgList && rec.msgList[0] && rec.msgList[0].msgId) || ''); } catch (e) {}\n");
    js.push_str("          R.sendResults[mid] = { status: 'done', msgId: nid, result: JSON.stringify(rec).slice(0, 2000) };\n");
    js.push_str("        } catch (e) { R.sendResults[mid] = { status: 'done-raw' }; }\n");
    js.push_str("      }, function (err) {\n");
        js.push_str("        R.sendResults[mid] = { status: 'rejected', error: String(err).slice(0, 300) };\n");
    js.push_str("      });\n");
    js.push_str("    } else {\n");
    js.push_str("      R.sendResults[mid] = { status: 'done-raw' };\n");
    js.push_str("    }\n");
    js.push_str("    return JSON.stringify({ fired: true, mid: mid });\n");
    js.push_str("  } catch (e) { return JSON.stringify({ err: String(e).slice(0, 300) }); }\n");
    js.push_str("})()\n");
    js
}

/// D9 发送:返回原始输出(fired JSON / NOT_ARMED JSON / ERR JSON)。
pub fn send_text(chat_type: u32, peer_uid: &str, text: &str) -> Result<String, V8Error> {
    let Some(syms) = symbols() else {
        return Err(V8Error::NoCurrentContext);
    };
    let js = build_send_js(chat_type, peer_uid, text);
    // SAFETY: 阶梯自管全部检查;调用方保证 owner 线程。
    unsafe { exec_script(&syms, isolate_addr(), &js) }
}
