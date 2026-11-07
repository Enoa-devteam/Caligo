//! G3 原生调用封装为 [`NativeOps`](crate::native_session::NativeOps)(N3 现场验证;
//! 计划 §7-N3:把已证 g3 FFI 序列封装为 NativeSession 的派发后端)。
//!
//! 安全边界:一个 G3NativeOps 实例持有单个 getter pair；提交前核对锚点与
//! transport。732A80 同步复制 body 后由本侧经 QQ 配对 delete 释放输入缓冲；
//! pair 在异步回调排空/服务失效寿命未证前保留，不在 Drop 中释放。

use crate::native_response::Cookie;
use crate::native_session::{NativeOps, OpsError};
use core::ffi::c_void;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("the frozen QQ wrapper ABI is Windows x64 only");

/// g3 原生调用所需 wrapper 内 RVA(与 g3.rs 同源)。
mod rva {
    pub const GETTER: usize = 0x72DE38;
    pub const ANCHOR_SVC_VTBL: usize = 0x3F6DED8;
    pub const SEND_SSO: usize = 0x732A80;
    pub const OPERATOR_NEW: usize = 0xB75000;
    pub const OPERATOR_DELETE: usize = 0xB75029;
    pub const PAIR_RELEASE: usize = 0x1DB2;
    pub const EMPTY_FN_CALLABLE: usize = 0x72D596;
    pub const EMPTY_FN_DTOR: usize = 0x72D5C8;
    pub const SVC_TRANSPORT: usize = 0x60;
}

type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;
type SendSsoFn = unsafe extern "system" fn(usize, usize, usize, u8, usize);
type OperatorNewFn = unsafe extern "system" fn(usize) -> *mut c_void;
type OperatorDeleteFn = unsafe extern "system" fn(*mut c_void);
type PairReleaseFn = unsafe extern "system" fn(*mut ServicePair);

#[repr(C)]
#[derive(Default)]
struct ServicePair {
    // The frozen target is Windows x64: these two pointer-width words have the
    // same ABI layout as the QQ pair, while keeping this retained lease Send.
    obj: usize,
    ctrl: usize,
}

struct QqOwnedBuffer {
    ptr: *mut c_void,
    delete_fn: OperatorDeleteFn,
}

impl QqOwnedBuffer {
    fn as_ptr(&self) -> *mut c_void {
        self.ptr
    }
}

impl Drop for QqOwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: ptr came from this exact wrapper's B75000 and B75029 is its
        // statically verified paired QQ delete wrapper.
        unsafe { (self.delete_fn)(self.ptr) };
    }
}

/// g3 原生调用 ops(每个实例绑定一个 wrapper 基址)。
pub struct G3NativeOps {
    wrapper_base: usize,
    service_pair: Option<ServicePair>,
    submitted: bool,
}

impl G3NativeOps {
    pub fn new(wrapper_base: usize) -> Self {
        Self {
            wrapper_base,
            service_pair: None,
            submitted: false,
        }
    }

    pub fn wrapper_base(&self) -> usize {
        self.wrapper_base
    }

    /// 获取并缓存本实例唯一的 getter pair。重复探测复用同一个 lease，避免
    /// 每次 env check/submit 都增加一次 QQ 强引用。成功返回真实服务对象地址。
    pub fn probe_service(&mut self) -> Result<usize, OpsError> {
        if let Some(pair) = self.service_pair.as_ref() {
            let svc = pair.obj;
            if self.validate_service(svc).is_ok() {
                return Ok(svc);
            }
            if !self.submitted {
                self.release_unsubmitted_service_pair();
            }
            return Err(OpsError::NotReady(
                "probe: cached service is not ready".into(),
            ));
        }

        let wb = self.wrapper_base;
        // SAFETY: getter writes the fixed-profile {obj, ctrl} pair to its out
        // argument and returns that same address.
        unsafe {
            let getter: GetterFn = core::mem::transmute(wb + rva::GETTER);
            let mut pair = ServicePair::default();
            let ret = getter(&mut pair as *mut ServicePair);
            if ret as usize != &mut pair as *mut ServicePair as usize
                || pair.obj == 0
                || pair.ctrl == 0
            {
                return Err(OpsError::NotReady("probe: getter failed".into()));
            }
            let svc = pair.obj;
            if let Err(error) = self.validate_service(svc) {
                // No native request has been submitted with this lease yet.
                // 1DB2 is the fixed-hash pair release helper, distinct from the
                // callback-manager operation-3 disposer at 2632.
                let release: PairReleaseFn = core::mem::transmute(wb + rva::PAIR_RELEASE);
                release(&mut pair as *mut ServicePair);
                return Err(error);
            }
            self.service_pair = Some(pair);
            Ok(svc)
        }
    }

    /// 只在本代尚未尝试任何 native submit 时显式清理 getter lease。
    /// 成功提交后保留 pair：回调/dispatcher quiescence 尚未证实。
    pub fn release_unsubmitted_service_pair(&mut self) {
        if self.submitted {
            return;
        }
        if let Some(mut pair) = self.service_pair.take() {
            // SAFETY: 1DB2 consumes the address of this exact {obj,ctrl} pair.
            unsafe {
                let release: PairReleaseFn =
                    core::mem::transmute(self.wrapper_base + rva::PAIR_RELEASE);
                release(&mut pair as *mut ServicePair);
            }
        }
    }

    fn validate_service(&self, svc: usize) -> Result<(), OpsError> {
        if svc < 0x10000 {
            return Err(OpsError::PreCall("probe: null service"));
        }
        let vptr = read_qword(svc).unwrap_or(0);
        if (vptr as usize).wrapping_sub(self.wrapper_base) != rva::ANCHOR_SVC_VTBL {
            return Err(OpsError::NotReady("probe: anchor mismatch".into()));
        }
        match read_qword(svc + rva::SVC_TRANSPORT) {
            Some(transport) if transport > 0x10000 => Ok(()),
            _ => Err(OpsError::NotReady("transport 未安装".into())),
        }
    }

    /// 完整 g3 发送序列: cached service → transport → allocate → send。
    /// 732A80 在本次同步调用内复制 body；返回后通过同一 QQ allocator pair 释放输入。
    fn native_submit(
        &mut self,
        service_addr: usize,
        _cookie: &Cookie,
        body: &[u8],
    ) -> Result<(), OpsError> {
        let wb = self.wrapper_base;
        let pair = self
            .service_pair
            .as_ref()
            .ok_or(OpsError::PreCall("g3 submit: service lease missing"))?;
        if pair.obj != service_addr {
            return Err(OpsError::PreCall("g3 submit: service lease mismatch"));
        }
        let svc = pair.obj;
        self.validate_service(svc)
            .map_err(|_| OpsError::NotReady("g3 submit: service/transport invalid".into()))?;

        // SAFETY: the exact fixed-profile service pair is retained in self;
        // local command/body/callback structures live through the synchronous
        // 732A80 call, whose body copy precedes its asynchronous dispatcher handoff.
        unsafe {
            let new_fn: OperatorNewFn = core::mem::transmute(wb + rva::OPERATOR_NEW);
            let delete_fn: OperatorDeleteFn = core::mem::transmute(wb + rva::OPERATOR_DELETE);
            let ptr = new_fn(body.len().max(1));
            if ptr.is_null() {
                return Err(OpsError::PreCall("g3 alloc: operator new null"));
            }
            let buf = QqOwnedBuffer { ptr, delete_fn };
            core::ptr::copy_nonoverlapping(body.as_ptr(), buf.as_ptr() as *mut u8, body.len());
            let begin = buf.as_ptr() as u64;
            let mut vec_hdr: [u64; 3] =
                [begin, begin + body.len() as u64, begin + body.len() as u64];

            let mut cmd = [0u8; 24];
            let cmd_name = b"MessageSvc.PbSendMsg";
            cmd[0] = (cmd_name.len() as u8) << 1;
            cmd[1..1 + cmd_name.len()].copy_from_slice(cmd_name);
            #[cfg(feature = "research")]
            let mut cb = crate::consumer_probe::build_cb_for_cookie(_cookie);
            #[cfg(not(feature = "research"))]
            let mut cb = {
                let mut cb = [0u64; 4];
                cb[2] = (wb + rva::EMPTY_FN_CALLABLE) as u64;
                cb[3] = (wb + rva::EMPTY_FN_DTOR) as u64;
                cb
            };

            let send: SendSsoFn = core::mem::transmute(wb + rva::SEND_SSO);
            // Conservatively quarantine the service lease from the moment native
            // entry is attempted; no callback drain/unload proof exists yet.
            self.submitted = true;
            send(
                svc,
                cmd.as_mut_ptr() as usize,
                vec_hdr.as_mut_ptr() as usize,
                1,
                cb.as_mut_ptr() as usize,
            );
            // QqOwnedBuffer drops here, after 732A80's synchronous body copy.
        }
        Ok(())
    }
}

impl NativeOps for G3NativeOps {
    fn probe_ready(&mut self, service_addr: usize) -> Result<(), OpsError> {
        match self.service_pair.as_ref() {
            Some(pair) if pair.obj == service_addr && pair.ctrl != 0 => {
                self.validate_service(service_addr)
            }
            _ => Err(OpsError::PreCall("probe: service lease mismatch")),
        }
    }

    fn submit(
        &mut self,
        service_addr: usize,
        cookie: &Cookie,
        body: &[u8],
    ) -> Result<(), OpsError> {
        self.native_submit(service_addr, cookie, body)
    }
}

/// RPM-self 单 qword 读取(g2 同款内核校验读)。
fn read_qword(addr: usize) -> Option<u64> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = 0u64;
    let mut got = 0usize;
    // SAFETY: 自进程 RPM;栈缓冲。
    unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            (&mut out as *mut u64).cast(),
            8,
            &mut got,
        )
    };
    if got == 8 {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_rejects_service_address_different_from_retained_lease() {
        let mut ops = G3NativeOps::new(0);
        ops.service_pair = Some(ServicePair {
            obj: 0x10000,
            ctrl: 0x20000,
        });
        let result = ops.submit(0x30000, &Cookie([0; 16]), b"BODY");
        assert_eq!(
            result,
            Err(OpsError::PreCall("g3 submit: service lease mismatch"))
        );
        assert!(!ops.submitted);
    }
}
