//! G3 发送探测 P1-v3:transport **对象 vptr** 交换 + 零分配捕获(零发送)。
//!
//! 静态证据链(r4-g3 树 + thread-contract §10,详见
//! `docs/research/qq-native-send-probe-design.md` §1):
//! - MSFService(this)+0x60 = transport,对象头 vptr = 原版 vtable RVA
//!   `0x41403B8`;slot7(+0x38)= `0x1B4E4EC`,六参:
//!   `(transport, string& uin, unique_ptr<task>*, policy48&, i32* token_out, callback&)`;
//!   **全部 SSO 发送(心跳、推送 ACK、用户消息)都经此槽**。
//!
//! v3 与 r1(镜像 vtable 补丁)的差别(2026-10-09 事故后修订,见台账):
//! - r1 改 wrapper .rdata 映像页(COW)= 可检测篡改面,11s 后进程以
//!   伪装 C++ length_error 终止 → **镜像补丁路线弃用**;
//! - v3 只交换 transport **对象头**的 vptr(单个堆 qword,原子,仅该
//!   对象受影响):伪造 vtable = 自分配 RW 内存中 16 qword 原表副本,
//!   slot7 指向蹦床,其余槽 = 原函数。QQ 自身即有堆垫片 vtable 先例
//!   (G2 发现的 Manager 垫片),对象级 vptr 多样性是 QQ 原生模式;
//! - 捕获处理器 **零分配/零锁/零 format**:固定 96-qword 记录 × 64 槽,
//!   seqlock 语义([0] 最低位 = 完成位,Release 发布),编码在窗口结束后
//!   由我们线程统一做。
//!
//! 纪律(承 3 次实例损失教训):
//! - 蹦床页/伪 vtable 页 **永不释放**(恢复后迟到调用仍完整工作);
//! - 捕获处理器全部读取经 safe_read(RPM-self,坏页=None;硬件 AV 不可捕获,
//!   禁 read_volatile 直读);有界(uin ≤128B、候选 ≤4、环 64 条);
//! - 座位(对象 vptr)被第三方覆写则不回写(同 G2 座位丢失语义),如实记录。

use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

/// getter 机器级原型(各 wrapper profile 同 ABI,作身份核验 + 租约)。
type GetterFn = unsafe extern "system" fn(out: *mut ServicePair) -> *mut ServicePair;

pub const G3_PROFILE_ID_9933: u32 = 0;
pub const G3_PROFILE_ID_9936: u32 = 1;
pub const G3_PROFILE_9933_WRAPPER_SHA256: &str =
    "63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0";
pub const G3_PROFILE_9936_WRAPPER_SHA256: &str =
    "0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C";

#[derive(Clone, Copy)]
struct G3BuildProfile {
    getter_rva: usize,
    anchor_svc_vtbl_rva: usize,
    transport_vtbl_rva: usize,
    slot7_orig_rva: usize,
    send_sso_rva: usize,
    qq_operator_new_rva: usize,
    empty_fn_callable_rva: Option<usize>,
    empty_fn_dtor_rva: Option<usize>,
    wirecap_call_sites: [usize; 2],
    requires_consumer: bool,
}

const G3_PROFILE_9933: G3BuildProfile = G3BuildProfile {
    getter_rva: 0x72DE38,
    anchor_svc_vtbl_rva: 0x3F6DED8,
    transport_vtbl_rva: 0x41403B8,
    slot7_orig_rva: 0x1B4E4EC,
    send_sso_rva: 0x732A80,
    qq_operator_new_rva: 0xB75000,
    empty_fn_callable_rva: Some(0x72D596),
    empty_fn_dtor_rva: Some(0x72D5C8),
    wirecap_call_sites: [0x732A3E, 0x7333E6],
    requires_consumer: false,
};

// Exact wrapper 0D8B…534C. Entries were matched to current-hash code and RTTI;
// G3-send requires the research consumer because this build's empty-delegate
// helper RVAs have not been mapped.
const G3_PROFILE_9936: G3BuildProfile = G3BuildProfile {
    getter_rva: 0x74F214,
    anchor_svc_vtbl_rva: 0x40FFF58,
    transport_vtbl_rva: 0x42DEF48,
    slot7_orig_rva: 0x1C21344,
    send_sso_rva: 0x753FE6,
    qq_operator_new_rva: 0xBC0FB4,
    empty_fn_callable_rva: None,
    empty_fn_dtor_rva: None,
    wirecap_call_sites: [0x753FA4, 0x75494C],
    requires_consumer: true,
};

/// transport vtable slot 7 位于 vtable +0x38。
const SLOT7_OFF: usize = 0x38;
/// transport+0x60 = conn pair(b7ce8a 的状态检查对象)。
const TRANSPORT_FIELD_CONN: usize = 0x60;
/// MSFService+0x60 字段(transport 指针)。
const SVC_FIELD_TRANSPORT: usize = 0x60;
/// vtable 拷贝槽数(前 10 槽为函数指针,其后为非指针数据,原样拷贝)。
const VTBL_COPY_QWORDS: usize = 16;

/// 任务对象 0x88 字节(0x72ca0d:alloc(0x88)+ctor)。
const TASK_BYTES: usize = 0x88;
/// 捕获环容量(条)。
const RING_CAP: usize = 64;
/// 候选解引用上限(每调用)。
const CAND_CAP: usize = 4;
/// 字符串捕获上限(字节)。
const STR_CAP: usize = 128;
/// 每槽 qword 数(记录布局见下)。
const SLOT_QWORDS: usize = 96;

#[cfg(feature = "research")]
static G3_CALLBACK_NONCE: AtomicU64 = AtomicU64::new(1);

// 记录布局(槽内偏移):
//  [0]       seq|1(完成位;0=未完成)
//  [2..8]    a1..a6
//  [8..11]   uin string 原始 24B
//  [11..27]  uin 数据 ≤128B
//  [27]      task_ptr(*task_slot)
//  [28..45]  task 0x88B
//  [45]      n_cands
//  [46..58]  候选 4×{off,ptr,first}
//  [58..74]  候选头 4×0x20B
//  [74..80]  policy 0x30B
//  [80..84]  callback 0x20B
const OFF_ARGS: usize = 2;
const OFF_UIN_RAW: usize = 8;
const OFF_UIN_DATA: usize = 11;
const OFF_TASK_PTR: usize = 27;
const OFF_TASK: usize = 28;
const OFF_NCAND: usize = 45;
const OFF_CANDS: usize = 46;
const OFF_HEADS: usize = 58;
const OFF_POLICY: usize = 74;
const OFF_CB: usize = 80;

pub const OK: u32 = 0;
pub const ERR_NO_OBSERVER: u32 = 21; // getter/锚点失败(沿用 v1 语义)
pub const ERR_SWAP_STATE: u32 = 22;
pub const ERR_NO_TRANSPORT: u32 = 31;

#[repr(C)]
pub struct G3Ctx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
    pub observe_ms: u32,
}

/// 固定捕获环(回调侧零分配写入;窗口结束单次 drain)。
static mut RING_SLOTS: [u64; RING_CAP * SLOT_QWORDS] = [0; RING_CAP * SLOT_QWORDS];
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 蹦床页地址(交换期间写入伪 vtable;恢复后仍有效,页不释放)。
static TRAMPOLINE: AtomicU64 = AtomicU64::new(0);
/// 伪 vtable 页地址(交换期间写入对象头;恢复后仍有效,页不释放)。
static FAKE_VTBL: AtomicU64 = AtomicU64::new(0);
/// 原对象 vptr(= wrapper_base + 0x41403B8)。
static ORIGINAL_VPTR: AtomicU64 = AtomicU64::new(0);
/// 捕获激活闸(交换后置 1,恢复前置 0;0 时蹦床仍安全:直接进原函数)。
static ACTIVE: AtomicU64 = AtomicU64::new(0);

fn hx(v: u64) -> String {
    format!("{v:#x}")
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn bytes_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(HEX_DIGITS[(x >> 4) as usize] as char);
        s.push(HEX_DIGITS[(x & 0xF) as usize] as char);
    }
    s
}

/// RPM-self 安全读:坏页返回 None 而非异常(g2 同源实现)。
fn safe_read_qword(addr: usize) -> Option<u64> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = 0u64;
    let mut got = 0usize;
    // SAFETY: 自进程 RPM;缓冲在栈上。
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            (&mut out as *mut u64).cast(),
            8,
            &mut got,
        )
    };
    if ok == 0 || got != 8 {
        None
    } else {
        Some(out)
    }
}

/// 逐 qword 读入固定缓冲(零分配)。返回成功读取的 qword 数。
fn read_qwords_into(addr: usize, out: &mut [u64]) -> usize {
    for (i, o) in out.iter_mut().enumerate() {
        match safe_read_qword(addr + i * 8) {
            Some(v) => *o = v,
            None => return i,
        }
    }
    out.len()
}

pub(crate) fn addr_plausible(v: u64) -> bool {
    (0x1_0000..0x7FFF_FFFE_FFFF).contains(&v) && v & 7 == 0
}

/// libc++ 同源 string 判形(纯函数,可测试)。
/// 短形态:byte0 = size<<1(≤22,内联 +1);长形态:{cap|1, size@8, data*@0x10}。
fn string_form(q0: u64) -> Option<(bool, usize)> {
    if q0 & 1 == 0 {
        let size = ((q0 & 0xFF) >> 1) as usize;
        if size > 22 {
            return None;
        }
        Some((true, size)) // short, size
    } else {
        Some((false, 0)) // long,size 由 +8 处读取
    }
}

/// 捕获处理器(蹦床调用;六参原样)。零分配、零锁、零 format。
///
/// SAFETY: 各参为 QQ 线程上 slot7 的实参;全部解引用经 safe_read。
unsafe extern "system" fn g3_capture(
    transport: usize,
    uin_str: usize,
    task_slot: usize,
    policy: usize,
    token_out: usize,
    callback: usize,
) {
    if ACTIVE.load(Ordering::Acquire) == 0 {
        return;
    }
    // SAFETY: 边界内只做固定缓冲写入 + safe_read;无 unwrap/分配/锁,
    // 无 panic 路径(catch_unwind 仅作纵深防御)。
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let idx64 = NEXT.fetch_add(1, Ordering::Relaxed);
        if idx64 as usize >= RING_CAP {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let seq = idx64;
        // SAFETY: idx < RING_CAP;单写者(原子取号);drain 在 ACTIVE=0 后。
        let slot = unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::addr_of_mut!(RING_SLOTS).cast::<u64>(),
                RING_CAP * SLOT_QWORDS,
            )
        };
        let s = &mut slot[idx64 as usize * SLOT_QWORDS..(idx64 as usize + 1) * SLOT_QWORDS];
        s[0] = 0; // 未完成标记(偶)
        s[1] = 0; // flags
        s[OFF_ARGS] = transport as u64;
        s[OFF_ARGS + 1] = uin_str as u64;
        s[OFF_ARGS + 2] = task_slot as u64;
        s[OFF_ARGS + 3] = policy as u64;
        s[OFF_ARGS + 4] = token_out as u64;
        s[OFF_ARGS + 5] = callback as u64;

        // uin string(双形态;数据 ≤128B)。
        if addr_plausible(uin_str as u64) {
            let mut raw = [0u64; 3];
            if read_qwords_into(uin_str, &mut raw) == 3 {
                s[OFF_UIN_RAW..OFF_UIN_RAW + 3].copy_from_slice(&raw);
                if let Some((short, size)) = string_form(raw[0]) {
                    if short {
                        let bytes = raw.map(|v| v.to_le_bytes());
                        let flat: [u8; 24] = {
                            let mut f = [0u8; 24];
                            let mut n = 0;
                            for b in bytes {
                                f[n..n + 8].copy_from_slice(&b);
                                n += 8;
                            }
                            f
                        };
                        if size <= 22 {
                            let mut data = [0u64; 16];
                            let mut fb = [0u8; 128];
                            fb[..24].copy_from_slice(&flat);
                            let mut n8 = 0;
                            while n8 * 8 < 128 && n8 < 16 {
                                let mut w = [0u8; 8];
                                w.copy_from_slice(&fb[n8 * 8..n8 * 8 + 8]);
                                data[n8] = u64::from_le_bytes(w);
                                n8 += 1;
                            }
                            s[OFF_UIN_DATA..OFF_UIN_DATA + 16].copy_from_slice(&data);
                            s[1] |= 1; // has uin(short)
                        }
                    } else {
                        let size = safe_read_qword(uin_str + 8).unwrap_or(0) as usize;
                        let data_ptr = safe_read_qword(uin_str + 0x10).unwrap_or(0);
                        s[OFF_UIN_RAW + 1] = size as u64;
                        s[OFF_UIN_RAW + 2] = data_ptr;
                        if addr_plausible(data_ptr) {
                            let take = size.min(STR_CAP);
                            let mut data = [0u64; 16];
                            let n =
                                read_qwords_into(data_ptr as usize, &mut data[..(take + 7) / 8]);
                            s[OFF_UIN_DATA..OFF_UIN_DATA + n].copy_from_slice(&data[..n]);
                            s[1] |= 1 | 2; // has uin(long)
                        }
                    }
                }
            }
        }

        // task(0x88B)+ 候选追踪。
        if addr_plausible(task_slot as u64) {
            if let Some(tp) = safe_read_qword(task_slot) {
                s[OFF_TASK_PTR] = tp;
                if addr_plausible(tp) {
                    let mut task = [0u64; TASK_BYTES / 8];
                    let n = read_qwords_into(tp as usize, &mut task);
                    s[OFF_TASK..OFF_TASK + n].copy_from_slice(&task[..n]);
                    let mut ncand = 0usize;
                    for (i, q) in task.iter().take(n).enumerate() {
                        if ncand >= CAND_CAP {
                            break;
                        }
                        if !addr_plausible(*q) {
                            continue;
                        }
                        // 对象资格:首 qword(疑似 vptr)必须可读。
                        let Some(first) = safe_read_qword(*q as usize) else {
                            continue;
                        };
                        let base = OFF_CANDS + ncand * 3;
                        s[base] = (i * 8) as u64;
                        s[base + 1] = *q;
                        s[base + 2] = first;
                        let hbase = OFF_HEADS + ncand * 4;
                        let mut head = [0u64; 4];
                        let hn = read_qwords_into(*q as usize, &mut head);
                        s[hbase..hbase + hn].copy_from_slice(&head[..hn]);
                        ncand += 1;
                    }
                    s[OFF_NCAND] = ncand as u64;
                    s[1] |= 4; // has task
                }
            }
        }

        // policy 48B。
        if addr_plausible(policy as u64) {
            let mut p = [0u64; 6];
            let n = read_qwords_into(policy, &mut p);
            s[OFF_POLICY..OFF_POLICY + n].copy_from_slice(&p[..n]);
            if n == 6 {
                s[1] |= 8;
            }
        }
        // callback 32B。
        if addr_plausible(callback as u64) {
            let mut c = [0u64; 4];
            let n = read_qwords_into(callback, &mut c);
            s[OFF_CB..OFF_CB + n].copy_from_slice(&c[..n]);
            if n == 4 {
                s[1] |= 16;
            }
        }

        // 发布(完成位 + seq;Release 保证先前写入可见)。
        s[0] = ((seq as u64) << 1) | 1;
    }));
}

/// drain:固定环 → JSON 行(窗口结束后由我们线程调用,可分配)。
pub fn drain_records() -> (Vec<String>, u64, u64) {
    let mut out = Vec::new();
    // SAFETY: 唯一读者在 ACTIVE=0 后运行;写者已停。
    let slots = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(RING_SLOTS).cast::<u64>(),
            RING_CAP * SLOT_QWORDS,
        )
    };
    for idx in 0..RING_CAP {
        let s = &slots[idx * SLOT_QWORDS..(idx + 1) * SLOT_QWORDS];
        let head = s[0];
        if head & 1 == 0 {
            continue;
        }
        let seq = (head >> 1) as usize;
        let flags = s[1];
        let args: Vec<String> = s[OFF_ARGS..OFF_ARGS + 6].iter().map(|v| hx(*v)).collect();
        let uin = decode_uin(s);
        let task_ptr = s[OFF_TASK_PTR];
        let task_hex = bytes_hex(&qw_slice_bytes(&s[OFF_TASK..OFF_TASK + TASK_BYTES / 8]));
        let ncand = s[OFF_NCAND] as usize;
        let mut cands = String::new();
        for c in 0..ncand.min(CAND_CAP) {
            let off = s[OFF_CANDS + c * 3];
            let ptr = s[OFF_CANDS + c * 3 + 1];
            let first = s[OFF_CANDS + c * 3 + 2];
            let head_hex = bytes_hex(&qw_slice_bytes(
                &s[OFF_HEADS + c * 4..OFF_HEADS + c * 4 + 4],
            ));
            if !cands.is_empty() {
                cands.push(',');
            }
            cands.push_str(&format!(
                "{{\"off\":{off:#x},\"ptr\":{},\"first\":{},\"head\":\"{head_hex}\"}}",
                hx(ptr),
                hx(first)
            ));
        }
        let policy_hex = bytes_hex(&qw_slice_bytes(&s[OFF_POLICY..OFF_POLICY + 6]));
        let cb_hex = bytes_hex(&qw_slice_bytes(&s[OFF_CB..OFF_CB + 4]));
        out.push(format!(
            "{{\"kind\":\"sso_call\",\"seq\":{seq},\"flags\":{flags},\"transport\":{},\
             \"uin_str\":{},\"uin\":{uin},\"task_slot\":{},\"task_ptr\":{},\"task\":\"{task_hex}\",\
             \"cands\":[{cands}],\"policy\":\"{policy_hex}\",\"cb\":\"{cb_hex}\",\
             \"token_out\":{}}}",
            args[0],
            args[1],
            args[2],
            hx(task_ptr),
            args[4],
        ));
    }
    let pushed = NEXT.load(Ordering::Relaxed);
    let dropped = DROPPED.load(Ordering::Relaxed);
    (out, pushed, dropped)
}

/// uin 字段解码为 JSON 字符串(hex + 尽力 UTF-8 文本)。
fn decode_uin(s: &[u64]) -> String {
    if s[1] & 1 == 0 {
        return "null".into();
    }
    let short = s[1] & 2 == 0;
    let (bytes, _n) = if short {
        (
            qw_slice_bytes(&s[OFF_UIN_RAW..OFF_UIN_RAW + 3])[1..23].to_vec(),
            0usize,
        )
    } else {
        (
            qw_slice_bytes(&s[OFF_UIN_DATA..OFF_UIN_DATA + 16]).to_vec(),
            0usize,
        )
    };
    let hexs = bytes_hex(&bytes);
    let text: String = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .replace('\\', "\\\\")
        .replace('"', "'");
    format!("{{\"t\":\"{text}\",\"hex\":\"{hexs}\"}}")
}

fn qw_slice_bytes(q: &[u64]) -> Vec<u8> {
    let mut v = Vec::with_capacity(q.len() * 8);
    for w in q {
        v.extend_from_slice(&w.to_le_bytes());
    }
    v
}

/// 蹦床字节编码(可测试)。
///
/// 入口(MSVC x64):rcx=a1 rdx=a2 r8=a3 r9=a4;[rsp]=ret;
/// [rsp+8..0x28]=0x20 字节 shadow;**a5=[rsp+0x28],a6=[rsp+0x30]**;
/// rsp≡8 (mod 16)。保存四个寄存器参 → 对齐 → 以完整六参调 `capture`
/// → 恢复 → 尾跳 `original`(栈面与直调完全一致)。rax 为易失,可占用。
pub fn trampoline_bytes(capture: usize, original: usize) -> Vec<u8> {
    let mut b = Vec::with_capacity(0x40);
    b.extend_from_slice(&[0x51]); // push rcx
    b.extend_from_slice(&[0x52]); // push rdx
    b.extend_from_slice(&[0x41, 0x50]); // push r8
    b.extend_from_slice(&[0x41, 0x51]); // push r9
    b.extend_from_slice(&[0x48, 0x83, 0xEC, 0x28]); // sub rsp,0x28(rsp=R-0x48)
                                                    // a5: 入口 [R+0x28] = [rsp+0x70];a6: [R+0x30] = [rsp+0x78]。
                                                    // capture 的栈参:call 前 [rsp+0x20]=a5,[rsp+0x28]=a6。
    b.extend_from_slice(&[0x48, 0x8B, 0x44, 0x24, 0x70]); // mov rax,[rsp+0x70]
    b.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x20]); // mov [rsp+0x20],rax
    b.extend_from_slice(&[0x48, 0x8B, 0x44, 0x24, 0x78]); // mov rax,[rsp+0x78]
    b.extend_from_slice(&[0x48, 0x89, 0x44, 0x24, 0x28]); // mov [rsp+0x28],rax
    b.extend_from_slice(&[0x48, 0xB8]); // movabs rax, capture
    b.extend_from_slice(&(capture as u64).to_le_bytes());
    b.extend_from_slice(&[0xFF, 0xD0]); // call rax
    b.extend_from_slice(&[0x48, 0x83, 0xC4, 0x28]); // add rsp,0x28
    b.extend_from_slice(&[0x41, 0x59]); // pop r9
    b.extend_from_slice(&[0x41, 0x58]); // pop r8
    b.extend_from_slice(&[0x5A]); // pop rdx
    b.extend_from_slice(&[0x59]); // pop rcx
    b.extend_from_slice(&[0xFF, 0x25, 0x00, 0x00, 0x00, 0x00]); // jmp qword [rip]
    b.extend_from_slice(&(original as u64).to_le_bytes());
    b
}

fn stage(path: *const u16, stage: &str, ok: bool, detail: &str) {
    // SAFETY: path 由 loader 写入,NUL 结尾 UTF-16。
    let text = unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        if len >= 32 * 1024 {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                path, len,
            )))
        }
    };
    if let Some(p) = text {
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
            .open(&p)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn report_path_str(path: *const u16) -> String {
    // SAFETY: loader 写入的 NUL 结尾 UTF-16。
    unsafe {
        let mut len = 0usize;
        while len < 32 * 1024 && *path.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(path, len))
    }
}

/// 单 qword 交换(VirtualProtect → 原子 8B 写 → 恢复保护;堆页本已 RW,
/// protect 为无害往返)。可测试:slot 可为进程内任意可读写地址。
fn swap_slot(slot_addr: usize, new_val: u64) -> Result<u64, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Memory::VirtualProtect;
    let mut old_prot = 0u32;
    // SAFETY: 自进程;8B 窗口;old_prot 由系统填写。
    if unsafe {
        VirtualProtect(
            slot_addr as *const c_void,
            8,
            0x04, /*PAGE_READWRITE*/
            &mut old_prot,
        )
    } == 0
    {
        return Err(format!("VirtualProtect(rw): Win32 {}", unsafe {
            GetLastError()
        }));
    }
    // SAFETY: 已确认可写;8B 对齐原子访问。
    let prev = unsafe { core::ptr::read_volatile(slot_addr as *const u64) };
    unsafe { core::ptr::write_volatile(slot_addr as *mut u64, new_val) };
    let mut restored = 0u32;
    // SAFETY: 恢复原保护。
    unsafe { VirtualProtect(slot_addr as *const c_void, 8, old_prot, &mut restored) };
    Ok(prev)
}

#[repr(C)]
struct ServicePair {
    obj: *mut c_void,
    ctrl: *mut c_void,
}

/// G3-P1(v3)探测主入口(loader 经 `caligo_g3_probe_run` 远程线程调用)。///
/// 返回 [`OK`] 时机制链(adopt→site→swap→restore→drain)已完整走完;
/// captured=0 属正常(窗口内无 SSO 发送),看报告 stages 判定。
///
/// SAFETY: ctx 必须指向本进程内有效的 [`G3Ctx`]。
pub unsafe fn g3_probe_run(ctx: *const G3Ctx) -> u32 {
    // This export remains pinned to the historic 9.9.33 profile.
    unsafe { g3_probe_run_with_profile(ctx, &G3_PROFILE_9933) }
}

/// Same bounded, zero-send probe for the exact 9.9.36-53644 wrapper hash.
///
/// SAFETY: ctx 必须指向本进程内有效的 [`G3Ctx`]。
pub unsafe fn g3_probe_run_9936(ctx: *const G3Ctx) -> u32 {
    unsafe { g3_probe_run_with_profile(ctx, &G3_PROFILE_9936) }
}

unsafe fn g3_probe_run_with_profile(ctx: *const G3Ctx, profile: &G3BuildProfile) -> u32 {
    // SAFETY: ctx 由 loader 写入。
    let (wrapper_base, report_path, observe_ms) =
        unsafe { ((*ctx).wrapper_base, (*ctx).report_path, (*ctx).observe_ms) };
    let observe_ms = observe_ms.min(300_000).max(500);

    // wrapper 身份核验(getter 租约 + 锚点;g1/g2 同机械)。
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + profile.getter_rva) };
    let mut pair = ServicePair {
        obj: core::ptr::null_mut(),
        ctrl: core::ptr::null_mut(),
    };
    // SAFETY: getter 契约(R2 §4)。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
        stage(report_path, "g3_adopt", false, "getter failed");
        return ERR_NO_OBSERVER;
    }
    let vptr = unsafe { core::ptr::read_volatile(pair.obj as *const u64) };
    let actual_vtbl_rva = (vptr as usize).wrapping_sub(wrapper_base);
    if actual_vtbl_rva != profile.anchor_svc_vtbl_rva {
        stage(
            report_path,
            "g3_adopt",
            false,
            &format!(
                "anchor mismatch actual_vtbl_rva=0x{:x} expected_vtbl_rva=0x{:x}",
                actual_vtbl_rva, profile.anchor_svc_vtbl_rva
            ),
        );
        return ERR_SWAP_STATE;
    }
    stage(
        report_path,
        "g3_adopt",
        true,
        &format!("svc={}", pair.obj as u64),
    );

    // 发送侧选址:transport = *(svc+0x60);conn pair = *(transport+0x60)。
    let Some(transport) = safe_read_qword(pair.obj as usize + SVC_FIELD_TRANSPORT) else {
        stage(report_path, "g3_site", false, "svc+0x60 unreadable");
        return ERR_NO_TRANSPORT;
    };
    if !addr_plausible(transport) {
        stage(
            report_path,
            "g3_site",
            false,
            &format!("transport implausible {}", hx(transport)),
        );
        return ERR_NO_TRANSPORT;
    }
    let orig_vptr = safe_read_qword(transport as usize).unwrap_or(0);
    let vptr_ok = orig_vptr == (wrapper_base + profile.transport_vtbl_rva) as u64;
    let conn = safe_read_qword(transport as usize + TRANSPORT_FIELD_CONN).unwrap_or(0);
    let conn_vptr = if addr_plausible(conn) {
        safe_read_qword(conn as usize).unwrap_or(0)
    } else {
        0
    };
    stage(
        report_path,
        "g3_site",
        vptr_ok,
        &format!(
            "transport={} vptr={} ({}) conn={} conn_vptr={}",
            hx(transport),
            hx(orig_vptr),
            if vptr_ok { "raw" } else { "MISMATCH" },
            hx(conn),
            hx(conn_vptr)
        ),
    );
    if !vptr_ok {
        return ERR_NO_TRANSPORT;
    }

    // 伪 vtable(自分配 RW 数据页;原表 16 qword 副本,slot7 → 蹦床)。
    let bytes = trampoline_bytes(
        g3_capture as usize,
        (wrapper_base + profile.slot7_orig_rva) as usize,
    );
    let tramp = unsafe {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
        };
        VirtualAlloc(
            std::ptr::null(),
            0x100,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        )
    };
    if tramp.is_null() {
        stage(
            report_path,
            "g3_swap",
            false,
            "VirtualAlloc(trampoline) failed",
        );
        return ERR_SWAP_STATE;
    }
    // SAFETY: 蹦床页为本进程新分配 RWX,大小 0x100 ≥ bytes.len()。
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        FlushInstructionCache(GetCurrentProcess(), tramp, bytes.len());
    }
    let mut fake_tbl = [0u64; VTBL_COPY_QWORDS];
    let n = read_qwords_into(orig_vptr as usize, &mut fake_tbl);
    if n < 10 {
        stage(
            report_path,
            "g3_swap",
            false,
            &format!("vtable copy short: {n}"),
        );
        return ERR_SWAP_STATE;
    }
    fake_tbl[SLOT7_OFF / 8] = tramp as u64;
    let fake_vtbl = unsafe {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
        };
        VirtualAlloc(
            std::ptr::null(),
            0x100,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if fake_vtbl.is_null() {
        stage(
            report_path,
            "g3_swap",
            false,
            "VirtualAlloc(fake vtable) failed",
        );
        return ERR_SWAP_STATE;
    }
    // SAFETY: 伪 vtable 页为本进程新分配 RW,大小 0x100 ≥ 16*8。
    unsafe {
        core::ptr::copy_nonoverlapping(
            fake_tbl.as_ptr().cast::<u8>(),
            fake_vtbl as *mut u8,
            VTBL_COPY_QWORDS * 8,
        )
    };

    TRAMPOLINE.store(tramp as u64, Ordering::Release);
    FAKE_VTBL.store(fake_vtbl as u64, Ordering::Release);
    ORIGINAL_VPTR.store(orig_vptr, Ordering::Release);

    // 交换对象头 vptr(纯堆 8B 原子;身份双门已在 site 阶段核验)。
    let prev = match swap_slot(transport as usize, fake_vtbl as u64) {
        Ok(p) => p,
        Err(e) => {
            stage(report_path, "g3_swap", false, &e);
            return ERR_SWAP_STATE;
        }
    };
    if prev != orig_vptr {
        let _ = swap_slot(transport as usize, orig_vptr);
        stage(
            report_path,
            "g3_swap",
            false,
            &format!("prev changed mid-swap: {}", hx(prev)),
        );
        return ERR_SWAP_STATE;
    }
    ACTIVE.store(1, Ordering::Release);
    stage(
        report_path,
        "g3_swap",
        true,
        &format!(
            "fake_vtbl={} trampoline={} prev={}",
            hx(fake_vtbl as u64),
            hx(tramp as u64),
            hx(prev)
        ),
    );

    // 观测窗口(心跳/推送 ACK/用户发送均经 transport slot7)。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(observe_ms as u64);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // 恢复:先关闸(迟到调用安全直入原函数),再写回。
    ACTIVE.store(0, Ordering::Release);
    let cur = safe_read_qword(transport as usize).unwrap_or(0);
    if cur == fake_vtbl as u64 {
        match swap_slot(transport as usize, orig_vptr) {
            Ok(_) => {
                let after = safe_read_qword(transport as usize).unwrap_or(0);
                if after == orig_vptr {
                    stage(
                        report_path,
                        "g3_restore",
                        true,
                        "transport vptr restored (fake/trampoline pages retained)",
                    );
                } else {
                    stage(
                        report_path,
                        "g3_restore",
                        false,
                        &format!("post-restore verify failed: {}", hx(after)),
                    );
                }
            }
            Err(e) => stage(report_path, "g3_restore", false, &e),
        }
    } else {
        stage(
            report_path,
            "g3_restore",
            false,
            &format!(
                "vptr changed during window (not rewriting stale): {}",
                hx(cur)
            ),
        );
    }

    // drain 固定环 → 报告(编码在本线程,可分配)。
    let (records, pushed, dropped) = drain_records();
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(report_path_str(report_path))
    {
        for rec in &records {
            let _ = writeln!(f, "{rec}");
        }
        let _ = writeln!(
            f,
            "{{\"kind\":\"summary\",\"pushed\":{pushed},\"dropped\":{dropped},\"encoded\":{}}}",
            records.len()
        );
    }
    stage(
        report_path,
        "g3_done",
        true,
        &format!("captured={pushed} dropped={dropped}"),
    );
    OK
}

// ===== G3-P2 受控发送(单次调用;hash-bound profiles)=====
//
// 静态依据(r4-g3 phaseF + send-probe-design §1):
// - 9.9.33 入口 = 0x732A80; 9.9.36-53644 入口 = 0x753FE6;五参:
//   (this, const string& cmd, vector<uint8>& body /*移转*/, bool, callback32B& /*移转*/);
//   1b4e4ec(transport slot7)语义:未连接同步失败(token=0 不排队),已连接
//   组闭包经 D32138 提交全局 dispatcher(跨线程唤醒)——单次直接调用,
//   **零插桩、零常驻修改**;
// - body = libc++ vector<uint8>{begin,end,cap}(phaseF 0x21866d0 定案:
//   BaseMsgMgr 经 vtbl+0x100 取得,内容 = 编码好的请求体);task 析构由 QQ
//   侧释放 → **data 必须来自 QQ 分配器**(profile 内绑定 operator new RVA);
// - cmd = "MessageSvc.PbSendMsg"(chat_type 常规);9.9.33 回调 = QQ 自身空函数
//   (0x72D596/0x72D5C8);9.9.36 profile 仅允许 research consumer delegate。
//   MSF 层回调本就被 b7f6f6 置空,结果路由走
//   BaseMsgMgr token 注册表,我们无消费者,空回调 = QQ 原生 inert 表示;
// - param_2(uin 串)从活对象 svc+0x1f0 逐字节复制(短形态内联;长形态
//   头部原样保留、data 换为我们 QQ 分配的副本)——零编码猜测。
//
// 请求体:最小 SendMsgReq protobuf(字段号与 caligo-core
// qq_protocol::send 同源,verified=false——本次发送同时是该编码的首次
// 真实核验;桥不依赖 core,内联同款 writer)。

/// MSFService+0x1f0 = 本机账号串(send function param_2 的源)。
const SVC_FIELD_SELF_STRING: usize = 0x1F0;

// 字段号权威源 = caligo_model::qq_wire::send(N1 起);此处仅保留 @ 研究
// 分支仍在使用的子集(值与 model 同源)。
const PB_F_ROUTING_HEAD: u32 = 1;
const PB_F_BODY: u32 = 3;
const PB_F_CLIENT_SEQ: u32 = 4;
const PB_F_RANDOM: u32 = 5;
const PB_F_BODY_RICH: u32 = 1;
const PB_F_ELEM_TEXT: u32 = 1;
const PB_F_TEXT_STR: u32 = 1;

pub const ERR_SEND_BODY: u32 = 41;
pub const ERR_SEND_ALLOC: u32 = 42;

#[repr(C)]
pub struct G3SendCtx {
    pub wrapper_base: usize,
    pub report_path: *const u16,
    pub peer_uin: u64,
    pub text_ptr: *const u16, // NUL 结尾 UTF-16(远程缓冲,loader 写入)
    pub uid_ptr: *const u16,  // 对端 uid("u_xxx";可空 = 无 uid 字段)
    pub group_code: u64,      // 非 0 = 群聊发送(路由 Grp,type=82)
    pub client_seq: u32,
    pub random: u32,
    pub at_uid_ptr: *const u8, // NUL 结尾 ASCII:被 @ 人 peerUid(null = 无 @)
    pub at_uin: u32,           // 被 @ 人 uin(attr 用;v1 由调用方提供)
    /// N3R27 形状二分:0 = legacy(无 CH);非 0 = 群请求带 ContentHead
    /// {1: 该值, 2: 0, 3: 0}(调查发送漂移的服务端形状策略翻转假设)。
    pub use_ch: u32,
    /// N3R30 消费者实验:0 = QQ 空函数 delegate(现状);非 0 = 我们的
    /// 响应消费者 delegate(v4 = consumer_probe L1 {ctx,1,manager,invoke},
    /// manager op=4 答 0 = 非空可调用)—— 响应经 73bd6c 门后以 5 参 invoke
    /// (inner,0x10,errCode,errStr,responseUid)到达。responseUid 保持 opaque。
    pub use_consumer: u32,
}

/// 读远程 NUL 结尾 ASCII(空/不可读 → None)。
unsafe fn read_ascii_opt(ptr: *const u8) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 512 && *ptr.add(len) != 0 {
        len += 1;
    }
    if len == 0 || len >= 512 {
        return None;
    }
    Some(String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned())
}

/// 读远程 NUL 结尾 UTF-16(空/不可读 → None)。
unsafe fn read_utf16_opt(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 512 && *ptr.add(len) != 0 {
        len += 1;
    }
    if len == 0 || len >= 512 {
        return None;
    }
    // SAFETY: loader 写入的 NUL 结尾缓冲。
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(ptr, len)
    }))
}

fn pb_len_field_vec(field: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    pb_len_field(&mut out, field, body);
    out
}

fn pb_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

fn pb_tag(out: &mut Vec<u8>, field: u32, wire: u32) {
    pb_varint(out, (((field as u64) << 3) | wire as u64) & 0xFFFF_FFFF);
}

fn pb_varint_field(out: &mut Vec<u8>, field: u32, v: u64) {
    pb_tag(out, field, 0);
    pb_varint(out, v);
}

fn pb_len_field(out: &mut Vec<u8>, field: u32, body: &[u8]) {
    pb_tag(out, field, 2);
    pb_varint(out, body.len() as u64);
    out.extend_from_slice(body);
}

/// 最小 SendMsgReq(私聊/群聊文本)。普通文本路径自 N1 起委托共享 codec
/// `caligo_model::qq_wire::send`(field-proven:r9 私聊/r18 群聊,形状与
/// 字节 golden 见 caligo-model tests/qq_wire_golden.rs);本函数保留为
/// 研究入口薄包装 + @ 研究分支宿主(r20–r28b 归档,静默拒,阻塞于响应
/// retCode —— @ 不是生产支持面,勿据此扩能力)。
fn encode_send_msg(
    peer_uin: u64,
    peer_uid: Option<&str>,
    group_code: u64,
    text: &str,
    client_seq: u32,
    random: u32,
    at_uid: Option<&str>,
    at_uin: u32,
    use_ch: u32,
) -> Vec<u8> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if group_code == 0 {
        // ===== 私聊(r9 形状;共享 codec)=====
        return caligo_model::qq_wire::send::encode_friend_text(
            peer_uin, peer_uid, text, client_seq, random, ts,
        )
        .unwrap_or_default();
    }
    if at_uid.is_none() {
        // ===== 群聊(r18 形状:varint 群号;use_ch>0 = 带 CH 调查变体)=====
        return caligo_model::qq_wire::send::encode_group_text_ch(
            group_code, text, client_seq, random, ts, use_ch,
        )
        .unwrap_or_default();
    }
    // ===== 群聊 @ 研究分支(r20–r28b 归档;v6 = N3R37 真实记录对齐)=====
    // 入口保证 at_uid.is_some()。
    let mut w = Vec::with_capacity(text.len() + 128);
    let mut routing = Vec::new();
    let mut grp = Vec::new();
    if use_ch == 92 {
        // v11(N3R47):路由串形式(r16 主账号 wire 捕获 = Grp{1: code 串};
        // varint 形式 plain 已证可过,@ 未测 —— wirecap 已证 SnowLuma 不走
        // 732A80,真实 @ wire 仍未见,串形式 = 最后未测结构差异)。
        pb_len_field(&mut grp, 1, group_code.to_string().as_bytes());
    } else {
        pb_varint_field(&mut grp, 1, group_code);
    }
    pb_len_field(&mut routing, 2, &grp);
    pb_len_field(&mut w, PB_F_ROUTING_HEAD, &routing);
    let mut rich = Vec::new();
    // @ 元素构造(v6;N3R37 SnowLuma 送达件堆转储 0x21ba203c387 字节级对齐):
    // elem(text) = {1: {1: "@<显示名> "(含 @ 前缀 + 尾空格),
    //   3: attr13B{00,01,len = at段 UTF-16 units,00,uin_be32,00,00},
    //   12: {3: 2, 4: uin, 5: 0, 9: peerUid}(主账号 CAPWIRE47 记录实测;
    //   N3R37/SnowLuma 异路径无 f5 —— 已回滚)}}。
    // at 文本 = "@显示名" 无尾空格(空格在 rest 头,主账号实测)。
    let mut at_parts: Option<(Vec<u8>, Vec<u8>)> = None;
    if let Some(atuid) = at_uid {
        if let Some(sp) = text.find(' ') {
            let (at_name, rest) = text.split_at(sp);
            let at_text = format!("@{at_name}");
            let mut at_txt = Vec::new();
            pb_len_field(&mut at_txt, 1, at_text.as_bytes());
            let mut attr = vec![0u8, 1u8];
            // len = at 段 UTF-16 code-unit 数(主账号实测 "@Stella" = 7)。
            attr.extend_from_slice(&(at_text.chars().count() as u32).to_be_bytes());
            attr.push(0u8);
            attr.extend_from_slice(&at_uin.to_be_bytes());
            attr.extend_from_slice(&[0u8, 0u8]);
            pb_len_field(&mut at_txt, 3, &attr);
            let mut f12 = Vec::new();
            pb_varint_field(&mut f12, 3, 2);
            pb_varint_field(&mut f12, 4, at_uin as u64);
            pb_varint_field(&mut f12, 5, 0);
            pb_len_field(&mut f12, 9, atuid.as_bytes());
            pb_len_field(&mut at_txt, 12, &f12);
            let mut rest_txt = Vec::new();
            pb_len_field(&mut rest_txt, 1, rest.as_bytes());
            at_parts = Some((at_txt, rest_txt));
        }
    }
    if use_ch == 91 {
        // 变体 91(最小 @ 隔离实验,N3R29):已证普通形状原样保留(elem37 用
        // 普通版),仅追加 at 元素 —— 无 Rich.Attr/元素9/元素16/AT_ELEM37。
        // 目的:判 @ 拒绝源自 at 元素本身还是 r28 手工仿件。
        if let Some((at_txt, rest_txt)) = &at_parts {
            pb_len_field(&mut rich, 2, caligo_model::qq_wire::send::GROUP_ELEM37);
            pb_len_field(&mut rich, 2, at_txt);
            pb_len_field(&mut rich, 2, rest_txt);
            let body_nested = pb_len_field_vec(PB_F_BODY_RICH, &rich);
            pb_len_field(&mut w, PB_F_BODY, &body_nested);
            pb_varint_field(&mut w, PB_F_CLIENT_SEQ, client_seq as u64);
            pb_varint_field(&mut w, PB_F_RANDOM, random as u64);
            let mut f6 = Vec::new();
            pb_varint_field(&mut f6, 1, ts);
            pb_len_field(&mut w, 6, &f6);
            return w;
        }
    }
    if let Some((at_txt, rest_txt)) = &at_parts {
        // v13:CH = 10 键 {1:82, 4:divSeq, 5:seq, 6:ts, 7:1, 12:divSeq|1<<63,
        // 29:395247, 30:1, 32:r64} —— 主账号 CAPWIRE47 记录实测**含 f12**
        // (N3R37 异路径 9 键无 f12;两路径 CH 不同,r16 规格对本路径正确)。
        let div_seq = random as u64;
        let mut ch = Vec::new();
        pb_varint_field(&mut ch, 1, 82);
        pb_varint_field(&mut ch, 4, div_seq);
        pb_varint_field(&mut ch, 5, client_seq as u64);
        pb_varint_field(&mut ch, 6, ts);
        pb_varint_field(&mut ch, 7, 1);
        pb_varint_field(&mut ch, 12, div_seq | (1u64 << 63));
        pb_varint_field(&mut ch, 29, 395247);
        pb_varint_field(&mut ch, 30, 1);
        let mut r64 = unsafe { core::arch::x86_64::_rdtsc() };
        r64 ^= r64 >> 32;
        pb_varint_field(&mut ch, 32, r64);
        pb_len_field(&mut w, 2, &ch);
        // Rich.Attr(真实 @ 件必有;f3 = divSeq;f9 = "宋体" 主账号实测)。
        {
            let mut attr = Vec::new();
            pb_varint_field(&mut attr, 1, 0);
            pb_varint_field(&mut attr, 2, ts);
            pb_varint_field(&mut attr, 3, div_seq);
            pb_varint_field(&mut attr, 4, 0);
            pb_varint_field(&mut attr, 5, 10);
            pb_varint_field(&mut attr, 6, 0);
            pb_varint_field(&mut attr, 7, 134);
            pb_varint_field(&mut attr, 8, 2);
            pb_len_field(&mut attr, 9, &[0xe5, 0xae, 0x8b, 0xe4, 0xbd, 0x93]);
            pb_len_field(&mut rich, 1, &attr);
        }
        {
            let mut attr = Vec::new();
            pb_varint_field(&mut attr, 1, 0);
            pb_varint_field(&mut attr, 2, ts);
            pb_varint_field(&mut attr, 3, div_seq);
            pb_varint_field(&mut attr, 4, 0);
            pb_varint_field(&mut attr, 5, 10);
            pb_varint_field(&mut attr, 6, 0);
            pb_varint_field(&mut attr, 7, 134);
            pb_varint_field(&mut attr, 8, 2);
            pb_len_field(&mut attr, 9, &[0xe5, 0xae, 0x8b, 0xe4, 0xbd, 0x93]);
            pb_len_field(&mut rich, 1, &attr);
        }
        // @ 场景 elem37(v13 = 回滚 r28 125B:f19 带 f1:6 是本路径真实形状,
        // CAPWIRE47 记录实测 08 06 开头;v7 按 N3R37 异路径换成无 f1 版系误判)。
        const AT_ELEM37: &[u8] = &[
            0xaa, 0x02, 0x7a, 0x08, 0x00, 0x50, 0x00, 0x60, 0x00, 0x68, 0x00, 0x80, 0x01, 0x00,
            0x88, 0x01, 0x00, 0x9a, 0x01, 0x69, 0x08, 0x06, 0x20, 0xcb, 0x50, 0x78, 0x00, 0xc8,
            0x01, 0x00, 0xf0, 0x01, 0x00, 0xf8, 0x01, 0x00, 0x90, 0x02, 0x00, 0xc8, 0x02, 0x00,
            0x98, 0x03, 0x00, 0xa0, 0x03, 0x00, 0xb0, 0x03, 0x00, 0xb8, 0x03, 0x00, 0xc0, 0x03,
            0x00, 0xd0, 0x03, 0x00, 0xe8, 0x03, 0x00, 0x8a, 0x04, 0x02, 0x10, 0x11, 0x90, 0x04,
            0x80, 0x01, 0xb8, 0x04, 0x00, 0xc0, 0x04, 0x00, 0xca, 0x04, 0x00, 0xf8, 0x04, 0xcc,
            0x84, 0x08, 0x80, 0x05, 0x4d, 0x88, 0x05, 0x00, 0x80, 0x06, 0x00, 0xb2, 0x06, 0x0a,
            0x0a, 0x08, 0x08, 0x00, 0x10, 0x00, 0x18, 0x00, 0x20, 0x00, 0xd8, 0x06, 0x8b, 0x95,
            0x18, 0xa0, 0x07, 0xcf, 0xfe, 0xe3, 0xc3, 0xee, 0xdd, 0xc9, 0xa1, 0xef, 0x01,
        ];
        // v13 顺序(= 主账号实测):Attr → elem37 → 元素9 → at → Rich.f11=0
        // → rest → 元素16(r28 顺序本来正确;v6/v7 按 N3R37 重排系误判)。
        pb_len_field(&mut rich, 2, AT_ELEM37);
        // 元素 9(6B 固定;主账号件 f1 = 动态 id,N3R37 = 0 —— 0 可过)。
        pb_len_field(&mut rich, 2, &[0x4a, 0x04, 0x08, 0x00, 0x40, 0x01]);
        pb_len_field(&mut rich, 2, at_txt);
        // Rich.f11 = 0(主账号件 at 与 rest 之间携带,异路径无 —— 补齐)。
        pb_varint_field(&mut rich, 11, 0);
        pb_len_field(&mut rich, 2, rest_txt);
        // 元素16(发送者信息;本账号真实值,N3R37 记录)。
        pb_len_field(
            &mut rich,
            16,
            &[
                0x12, 0x06, 0x53, 0x74, 0x65, 0x6c, 0x6c, 0x61, 0x18, 0x01, 0x20, 0x00, 0x3a, 0x08,
                0x41, 0x49, 0xe6, 0x9c, 0x8b, 0xe5, 0x8f, 0x8b,
            ],
        );
        let body_nested = pb_len_field_vec(PB_F_BODY_RICH, &rich);
        pb_len_field(&mut w, PB_F_BODY, &body_nested);
        pb_varint_field(&mut w, PB_F_CLIENT_SEQ, client_seq as u64);
        pb_varint_field(&mut w, PB_F_RANDOM, random as u64);
        let mut f6 = Vec::new();
        pb_varint_field(&mut f6, 1, ts);
        pb_len_field(&mut w, 6, &f6);
        return w;
    }
    // 非 "@名 正文" 形态(研究调用方错误):退回普通群文本元素。
    let mut elem = Vec::new();
    {
        let mut txt = Vec::new();
        pb_len_field(&mut txt, PB_F_TEXT_STR, text.as_bytes());
        pb_len_field(&mut elem, PB_F_ELEM_TEXT, &txt);
    }
    pb_len_field(&mut rich, 2, &elem);
    // Body = {1: richText}(真实报文 0a39 包装;缺失 → 对端找不到富文本)。
    let body_nested = pb_len_field_vec(PB_F_BODY_RICH, &rich);
    pb_len_field(&mut w, PB_F_BODY, &body_nested);
    // 键4/5/6:与共享 codec 尾部同构(键6 = {1: 发送时刻秒})。
    pb_varint_field(&mut w, PB_F_CLIENT_SEQ, client_seq as u64);
    pb_varint_field(&mut w, PB_F_RANDOM, random as u64);
    let mut f6 = Vec::new();
    pb_varint_field(&mut f6, 1, ts);
    pb_len_field(&mut w, 6, &f6);
    w
}

/// 短形态 libc++ string 物化(≤22B;"MessageSvc.PbSendMsg" = 20B)。
fn make_short_string(dst: &mut [u8; 24], s: &[u8]) {
    *dst = [0; 24];
    dst[0] = (s.len() as u8) << 1;
    dst[1..1 + s.len()].copy_from_slice(s);
}

type OperatorNewFn = unsafe extern "system" fn(usize) -> *mut c_void;
type SendSsoFn = unsafe extern "system" fn(usize, usize, usize, u8, usize);

#[cfg(feature = "research")]
fn observe_g3_callback(cookie: crate::native_response::Cookie, report_path: *const u16) {
    use std::time::{Duration, Instant};

    const OBSERVE_WINDOW: Duration = Duration::from_secs(5);
    const POLL_INTERVAL: Duration = Duration::from_millis(10);

    let deadline = Instant::now() + OBSERVE_WINDOW;
    loop {
        let matches = crate::consumer_probe::drain_responses_for_cookie(&cookie);
        if let Some((_, err_code, err)) = matches.first() {
            stage(
                report_path,
                "g3s_callback_observation",
                true,
                &format!(
                    "matches={} err_code={} err_bytes={} delivery=unconfirmed",
                    matches.len(),
                    err_code,
                    err.len()
                ),
            );
            return;
        }

        let now = Instant::now();
        if now >= deadline {
            stage(
                report_path,
                "g3s_callback_observation",
                false,
                "matches=0 window_ms=5000 callback_and_delivery=unknown; no retry",
            );
            return;
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

/// G3-P2 受控发送主入口(loader 经 `caligo_g3_send_run` 远程线程调用)。
///
/// 单次调用 SendSSORequestInternal;零插桩、无常驻修改、零转发。
/// 缓冲所有权:body data 经移转归 QQ(本侧不释放);回调由 profile 绑定。
/// 9.9.36 research profile 使用受控 capture 并限时观察;callback 不证明送达。
/// 发送结果异步,delivery 仍须由受控对端确认。
///
/// SAFETY: ctx 必须指向本进程内有效的 [`G3SendCtx`]。
pub unsafe fn g3_send_run(ctx: *const G3SendCtx) -> u32 {
    unsafe { g3_send_run_with_profile(ctx, &G3_PROFILE_9933) }
}

/// Same one-shot send path, bound to wrapper SHA-256 0D8B…534C.
/// This profile requires the captured research consumer.
///
/// SAFETY: ctx 必须指向本进程内有效的 [`G3SendCtx`]。
pub unsafe fn g3_send_run_9936(ctx: *const G3SendCtx) -> u32 {
    unsafe { g3_send_run_with_profile(ctx, &G3_PROFILE_9936) }
}

unsafe fn g3_send_run_with_profile(ctx: *const G3SendCtx, profile: &G3BuildProfile) -> u32 {
    // SAFETY: ctx 由 loader 写入。
    let (
        wrapper_base,
        report_path,
        peer_uin,
        text_ptr,
        uid_ptr,
        group_code,
        client_seq,
        random,
        use_consumer,
    ) = unsafe {
        (
            (*ctx).wrapper_base,
            (*ctx).report_path,
            (*ctx).peer_uin,
            (*ctx).text_ptr,
            (*ctx).uid_ptr,
            (*ctx).group_code,
            (*ctx).client_seq,
            (*ctx).random,
            (*ctx).use_consumer,
        )
    };
    if profile.requires_consumer && (!cfg!(feature = "research") || use_consumer == 0) {
        stage(
            report_path,
            "g3s_profile",
            false,
            "9.9.36-53644 requires the research response consumer",
        );
        return ERR_SEND_BODY;
    }
    // v6 research wire capture is bound to the callsites in this exact profile.
    // (SnowLuma)的真实 @ wire body 与裸调形状逐字节对照。
    #[cfg(feature = "research")]
    if (*ctx).use_ch == 93 {
        // use_ch=93 = 卸载模式(N3R50 后停用 wirecap;还原调用点原始字节)。
        crate::wirecap::uninstall(
            wrapper_base,
            report_path,
            profile.send_sso_rva,
            profile.wirecap_call_sites,
        );
    } else {
        crate::wirecap::install(
            wrapper_base,
            report_path,
            profile.send_sso_rva,
            profile.wirecap_call_sites,
        );
    }
    // random = 0 时现场生成(每消息新鲜值;真实客户端同语义,服务器不校验来源)。
    let random = if random == 0 {
        let mut t = unsafe { core::arch::x86_64::_rdtsc() };
        t ^= t >> 32;
        (t as u32) | 1
    } else {
        random
    };

    // adopt(getter 租约;g1/g2/g3 探测同机械)。
    let getter: GetterFn = unsafe { core::mem::transmute(wrapper_base + profile.getter_rva) };
    let mut pair = ServicePair {
        obj: core::ptr::null_mut(),
        ctrl: core::ptr::null_mut(),
    };
    // SAFETY: getter 契约(R2 §4)。
    let ret = unsafe { getter(&mut pair as *mut ServicePair) };
    if ret as usize != &mut pair as *mut ServicePair as usize || pair.obj.is_null() {
        stage(report_path, "g3s_adopt", false, "getter failed");
        return ERR_NO_OBSERVER;
    }
    let vptr = unsafe { core::ptr::read_volatile(pair.obj as *const u64) };
    let actual_vtbl_rva = (vptr as usize).wrapping_sub(wrapper_base);
    if actual_vtbl_rva != profile.anchor_svc_vtbl_rva {
        stage(
            report_path,
            "g3s_adopt",
            false,
            &format!(
                "anchor mismatch actual_vtbl_rva=0x{:x} expected_vtbl_rva=0x{:x}",
                actual_vtbl_rva, profile.anchor_svc_vtbl_rva
            ),
        );
        return ERR_SWAP_STATE;
    }
    let svc = pair.obj as usize;
    stage(report_path, "g3s_adopt", true, &format!("svc={svc:x}"));

    // site:transport 在位(raw vptr)才发送。
    let Some(transport) = safe_read_qword(svc + SVC_FIELD_TRANSPORT) else {
        stage(report_path, "g3s_site", false, "svc+0x60 unreadable");
        return ERR_NO_TRANSPORT;
    };
    if !addr_plausible(transport) {
        stage(report_path, "g3s_site", false, "transport implausible");
        return ERR_NO_TRANSPORT;
    }
    let t_vptr = safe_read_qword(transport as usize).unwrap_or(0);
    let ok = (t_vptr as usize).wrapping_sub(wrapper_base) == profile.transport_vtbl_rva;
    stage(
        report_path,
        "g3s_site",
        ok,
        &format!("transport={transport:x} vptr={t_vptr:x}"),
    );
    if !ok {
        return ERR_NO_TRANSPORT;
    }

    // 正文(UTF-16 → UTF-8)。
    let text = {
        // SAFETY: loader 写入的 NUL 结尾 UTF-16。
        let mut len = 0usize;
        unsafe {
            while len < 16 * 1024 && *text_ptr.add(len) != 0 {
                len += 1;
            }
        }
        if len == 0 || len >= 16 * 1024 {
            stage(report_path, "g3s_text", false, "text empty/oversize");
            return ERR_SEND_BODY;
        }
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text_ptr, len) })
    };
    stage(
        report_path,
        "g3s_text",
        true,
        &format!("bytes={}", text.len()),
    );

    // 对端 uid(可空)。
    let peer_uid = read_utf16_opt(uid_ptr);
    if let Some(u) = &peer_uid {
        stage(report_path, "g3s_uid", true, &format!("len={}", u.len()));
    }
    // 编码请求体(最小 SendMsgReq;verified=false)。
    let at_uid = read_ascii_opt((*ctx).at_uid_ptr);
    let at_uin = (*ctx).at_uin;
    let use_ch = (*ctx).use_ch;
    let body = encode_send_msg(
        peer_uin,
        peer_uid.as_deref(),
        group_code,
        &text,
        client_seq,
        random,
        at_uid.as_deref(),
        at_uin,
        use_ch,
    );
    stage(
        report_path,
        "g3s_encode",
        true,
        &format!(
            "len={} ch={} head={}",
            body.len(),
            use_ch,
            bytes_hex(&body[..body.len().min(24)])
        ),
    );

    // body data:QQ 分配器(QQ 侧释放;本侧永不释放)。
    let new_fn: OperatorNewFn =
        unsafe { core::mem::transmute(wrapper_base + profile.qq_operator_new_rva) };
    // SAFETY: operator new 契约;失败返回 null(MSVC new 实际抛异常,
    // 小分配不触发;null 检查兜底)。
    let buf = unsafe { new_fn(body.len().max(1)) };
    if buf.is_null() {
        stage(
            report_path,
            "g3s_alloc",
            false,
            "operator new returned null",
        );
        return ERR_SEND_ALLOC;
    }
    // SAFETY: buf 容量 ≥ body.len()(new(size))。
    unsafe { core::ptr::copy_nonoverlapping(body.as_ptr(), buf as *mut u8, body.len()) };
    let begin = buf as u64;
    let end = begin + body.len() as u64;
    let mut vec_hdr: [u64; 3] = [begin, end, end];
    stage(
        report_path,
        "g3s_alloc",
        true,
        &format!("buf={begin:x} len={}", body.len()),
    );

    // param_2:本机账号串——逐字节复制活对象 svc+0x1f0(零编码猜测)。
    // 短形态:24B 原样;长形态:头部原样 + data 换 QQ 分配副本。
    let mut self_str = [0u8; 24];
    {
        let mut raw = [0u64; 3];
        if read_qwords_into(svc + SVC_FIELD_SELF_STRING, &mut raw) != 3 {
            stage(report_path, "g3s_selfstr", false, "svc+0x1f0 unreadable");
            return ERR_SEND_BODY;
        }
        let q0bytes = raw[0].to_le_bytes();
        if q0bytes[0] & 1 == 0 {
            for (i, q) in raw.iter().enumerate() {
                self_str[i * 8..i * 8 + 8].copy_from_slice(&q.to_le_bytes());
            }
            stage(
                report_path,
                "g3s_selfstr",
                true,
                &format!("short len={}", q0bytes[0] >> 1),
            );
        } else {
            let size = raw[1] as usize;
            let data_ptr = raw[2] as usize;
            if !addr_plausible(raw[2]) || size == 0 || size > 256 {
                stage(report_path, "g3s_selfstr", false, "long-form bad size/ptr");
                return ERR_SEND_BODY;
            }
            let copy = unsafe { new_fn(size + 1) };
            if copy.is_null() {
                stage(report_path, "g3s_selfstr", false, "selfstr alloc failed");
                return ERR_SEND_ALLOC;
            }
            let mut raw_bytes = [0u8; 256];
            let nq;
            {
                let mut tmp = [0u64; 32];
                nq = read_qwords_into(data_ptr, &mut tmp[..(size + 7) / 8]);
                for (i, q) in tmp.iter().take(nq).enumerate() {
                    raw_bytes[i * 8..i * 8 + 8].copy_from_slice(&q.to_le_bytes());
                }
            }
            if nq * 8 < size {
                stage(report_path, "g3s_selfstr", false, "selfstr readback short");
                return ERR_SEND_BODY;
            }
            let bytes = &raw_bytes[..size];
            // SAFETY: copy 容量 size+1。
            unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), copy as *mut u8, size) };
            // SAFETY: 末位写 NUL(字符串非二进制;QQ 串以长度计量)。
            unsafe { core::ptr::write((copy as *mut u8).add(size), 0) };
            self_str[0..8].copy_from_slice(&raw[0].to_le_bytes()); // cap|1 原样
            self_str[8..16].copy_from_slice(&(size as u64).to_le_bytes());
            self_str[16..24].copy_from_slice(&(copy as u64).to_le_bytes());
            stage(
                report_path,
                "g3s_selfstr",
                true,
                &format!("long len={size} copy={:#x}", copy as usize),
            );
        }
    }

    // cmd 串 + 回调块(空 delegate 或我们的响应消费者 delegate)。
    let mut cmd = [0u8; 24];
    make_short_string(&mut cmd, b"MessageSvc.PbSendMsg");
    let mut cb = [0u64; 4];
    #[cfg(feature = "research")]
    let mut callback_cookie = None;
    // 消费者分支 = research 专属(default 构建 E0433 自 r2 起预存,本提交
    // 补门;default 下 use_consumer 落回空 delegate,行为与 0 无异)。
    #[cfg(feature = "research")]
    if use_consumer != 0 && cfg!(feature = "research") {
        // v4(N3R34):响应消费者 delegate —— cb = L1 {ctx, 1, manager, invoker}。
        // 732A80 把 arg5 内层 delegate 移入捕获块,响应经 0x73C34C → 73bd6c
        // 门(manager op=4 答 0 = 非空)→ invoke(5 参:inner/tag/errCode/errStr/responseUid)。
        // 防御式实现(consumer_probe;RPM-self 安全读,布局误判不崩)。
        let callback_generation = G3_CALLBACK_NONCE.fetch_add(1, Ordering::Relaxed).max(1);
        let callback_nonce = G3_CALLBACK_NONCE.fetch_add(1, Ordering::Relaxed).max(1);
        let c =
            match crate::consumer_probe::leak_ctx(wrapper_base, report_path, callback_generation) {
                Some(c) => c,
                None => {
                    stage(
                        report_path,
                        "g3s_consumer",
                        false,
                        "callback module pin or generation context registry unavailable",
                    );
                    return ERR_SEND_ALLOC;
                }
            };
        cb = crate::consumer_probe::build_cb_for_generation(c, callback_nonce);
        let mut cookie = [0u8; 16];
        cookie[..8].copy_from_slice(&cb[0].to_le_bytes());
        cookie[8..].copy_from_slice(&cb[1].to_le_bytes());
        callback_cookie = Some(crate::native_response::Cookie(cookie));
        stage(
            report_path,
            "g3s_consumer",
            true,
            &format!(
                "generation={} manager={:x} invoke={:x}",
                c.generation, cb[2], cb[3]
            ),
        );
    }
    if use_consumer == 0 || !cfg!(feature = "research") {
        let (Some(callable), Some(dtor)) =
            (profile.empty_fn_callable_rva, profile.empty_fn_dtor_rva)
        else {
            stage(
                report_path,
                "g3s_consumer",
                false,
                "profile has no verified empty-delegate helpers",
            );
            return ERR_SEND_BODY;
        };
        cb[2] = (wrapper_base + callable) as u64;
        cb[3] = (wrapper_base + dtor) as u64;
    }

    // 单次调用(本线程;发送经全局 dispatcher 异步执行)。
    let send: SendSsoFn = unsafe { core::mem::transmute(wrapper_base + profile.send_sso_rva) };
    // SAFETY: 契约见模块注释;全部实参为 QQ 侧可消费的合法对象。
    unsafe {
        send(
            svc,
            cmd.as_mut_ptr() as usize,
            vec_hdr.as_mut_ptr() as usize,
            1,
            cb.as_mut_ptr() as usize,
        )
    };
    // 移转后:buf/cb/串副本所有权归 QQ 或已失效;本侧一律不再触碰。
    stage(
        report_path,
        "g3s_submitted",
        true,
        &format!(
            "send returned (async); peer={peer_uin} body_len={} seq={client_seq}",
            body.len()
        ),
    );
    #[cfg(feature = "research")]
    if let Some(cookie) = callback_cookie {
        observe_g3_callback(cookie, report_path);
    }
    stage(
        report_path,
        "g3s_done",
        true,
        "submission path returned; callback observation is separate; peer delivery remains unconfirmed",
    );
    OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex as SyncMutex;

    /// ACTIVE 闸/固定环是进程级共享静态,涉及它们的测试必须串行。
    static EXEC_LOCK: SyncMutex<()> = SyncMutex::new(());

    /// 编码形状:长度 66、分段字节、内嵌地址偏移。
    #[test]
    fn trampoline_encoding_exact() {
        let b = trampoline_bytes(0x1234_5678_9ABC_DEF0, 0x0FED_CBA9_8765_4321);
        // 4 push(6B)+sub(4)+4 mov(20)+movabs(10)+call(2)+add(4)+4 pop(6)+jmp(6)+imm64(8)
        assert_eq!(b.len(), 66);
        assert_eq!(&b[0..4], &[0x51, 0x52, 0x41, 0x50]);
        assert_eq!(&b[4..6], &[0x41, 0x51]);
        assert_eq!(&b[6..10], &[0x48, 0x83, 0xEC, 0x28]);
        assert_eq!(&b[30..32], &[0x48, 0xB8]);
        assert_eq!(&b[32..40], &0x1234_5678_9ABC_DEF0u64.to_le_bytes());
        assert_eq!(&b[40..42], &[0xFF, 0xD0]);
        assert_eq!(&b[42..46], &[0x48, 0x83, 0xC4, 0x28]);
        assert_eq!(&b[46..50], &[0x41, 0x59, 0x41, 0x58]);
        assert_eq!(&b[50..52], &[0x5A, 0x59]);
        assert_eq!(&b[52..58], &[0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(&b[58..66], &0x0FED_CBA9_8765_4321u64.to_le_bytes());
    }

    /// 蹦床执行:真实 VirtualAlloc RWX + 以 6 参调用,验证零分配捕获记录与
    /// 尾跳传参(第 5/6 参经栈)完整到达 original。
    #[test]
    fn trampoline_executes_capture_and_tailjump() {
        let _g = EXEC_LOCK.lock().unwrap();
        static ORIG_A5: AtomicUsize = AtomicUsize::new(0);
        static ORIG_A6: AtomicUsize = AtomicUsize::new(0);
        static ORIG_HITS: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "system" fn fake_orig(
            _a1: usize,
            _a2: usize,
            _a3: usize,
            _a4: usize,
            a5: usize,
            a6: usize,
        ) {
            ORIG_A5.store(a5, Ordering::Relaxed);
            ORIG_A6.store(a6, Ordering::Relaxed);
            ORIG_HITS.fetch_add(1, Ordering::Relaxed);
        }

        // 假 uin string(短形态 "3089665724")。
        let mut uin_buf = [0u8; 24];
        let s = b"3089665724";
        uin_buf[0] = (s.len() as u8) << 1;
        uin_buf[1..1 + s.len()].copy_from_slice(s);
        let uin = Box::leak(Box::new(uin_buf));

        // 假 task:0x88B,+0x00 放一个可解引用的候选(指向 blob 对象)。
        let blob: Box<[u64; 4]> = Box::new([0xDEAD_BEEF_0000_1000, 1, 2, 3]);
        let blob_ptr = Box::leak(blob) as *mut u64 as usize;
        let mut task = [0u64; TASK_BYTES / 8];
        task[0] = blob_ptr as u64;
        task[3] = 0x31;
        let task = Box::leak(Box::new(task));
        // slot7 的 a3 = 指向"任务指针槽"的指针(*a3 = task)。
        let task_slot = Box::leak(Box::new(task.as_mut_ptr() as u64));

        let policy = Box::leak(Box::new([180000u64, 1, 180000, 1, 2, 1]));
        let cb = Box::leak(Box::new([0x1111_2222_3333_4444u64, 0x5555_6666_7777_8888]));
        let token_out = Box::leak(Box::new(0u32));

        ACTIVE.store(1, Ordering::SeqCst);
        let before = NEXT.load(Ordering::Relaxed);

        let bytes = trampoline_bytes(g3_capture as usize, fake_orig as usize);
        let tramp = unsafe {
            use windows_sys::Win32::System::Memory::{
                VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
            };
            VirtualAlloc(
                std::ptr::null(),
                0x100,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };
        assert!(!tramp.is_null());
        // SAFETY: 测试进程内新分配 RWX 页。
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };

        let f: unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) =
            unsafe { core::mem::transmute(tramp as usize) };
        // SAFETY: 蹦床由本测试写入,目标假宿主无副作用。
        unsafe {
            f(
                0x1000,
                uin.as_mut_ptr() as usize,
                task_slot as *mut u64 as usize,
                policy.as_mut_ptr() as usize,
                token_out as *mut u32 as usize,
                cb.as_mut_ptr() as usize,
            )
        };

        assert_eq!(ORIG_HITS.load(Ordering::Relaxed), 1, "original not reached");
        assert_eq!(
            ORIG_A5.load(Ordering::Relaxed),
            token_out as *mut u32 as usize,
            "a5 mangled"
        );
        assert_eq!(
            ORIG_A6.load(Ordering::Relaxed),
            cb.as_mut_ptr() as usize,
            "a6 mangled"
        );

        let (records, pushed, _dropped) = drain_records();
        assert!(pushed > before, "no capture record");
        let rec = records.last().expect("no encoded record");
        assert!(rec.contains("\"kind\":\"sso_call\""));
        assert!(
            rec.contains("3089665724"),
            "uin short-form not parsed: {rec}"
        );
        assert!(rec.contains(&format!("\"task_ptr\":{}", hx(task.as_mut_ptr() as u64))));
        assert!(
            rec.contains(&format!("\"ptr\":{}", hx(blob_ptr as u64))),
            "blob candidate missing: {rec}"
        );
        // policy 以 hex 记录:180000 = 0x2BF20(LE "20bf02…")。
        assert!(
            rec.contains("\"policy\":\"20bf02"),
            "policy not captured: {rec}"
        );
        // callback 以 hex 记录(LE 4444 3333 2222 1111)。
        assert!(rec.contains("4444333322221111"), "cb not captured: {rec}");
        ACTIVE.store(0, Ordering::SeqCst);
    }

    /// ACTIVE=0 时蹦床仍安全直入原函数(恢复后迟到调用路径)。
    #[test]
    fn inactive_trampoline_skips_capture() {
        let _g = EXEC_LOCK.lock().unwrap();
        static HITS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "system" fn nop_orig(
            _: usize,
            _: usize,
            _: usize,
            _: usize,
            _: usize,
            _: usize,
        ) {
            HITS.fetch_add(1, Ordering::Relaxed);
        }
        ACTIVE.store(0, Ordering::SeqCst);
        let before = NEXT.load(Ordering::Relaxed);
        let bytes = trampoline_bytes(g3_capture as usize, nop_orig as usize);
        let tramp = unsafe {
            use windows_sys::Win32::System::Memory::{
                VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
            };
            VirtualAlloc(
                std::ptr::null(),
                0x100,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };
        assert!(!tramp.is_null());
        // SAFETY: 测试进程内新分配 RWX 页。
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len()) };
        let f: unsafe extern "system" fn(usize, usize, usize, usize, usize, usize) =
            unsafe { core::mem::transmute(tramp as usize) };
        // SAFETY: 假宿主。
        unsafe { f(0, 0, 0, 0, 0, 0) };
        assert_eq!(HITS.load(Ordering::Relaxed), 1);
        assert_eq!(
            NEXT.load(Ordering::Relaxed),
            before,
            "capture ran while inactive"
        );
    }

    /// swap_slot 往返对称(自进程 RW 页模拟对象头)。
    #[test]
    fn swap_slot_roundtrip_symmetric() {
        let mut table = [0u64; 16];
        table[7] = 0xAAAA_BBBB_CCCC_DDDD;
        let slot = &mut table[7] as *mut u64 as usize;
        let prev = swap_slot(slot, 0x1234_5678_9ABC_DEF0).expect("swap failed");
        assert_eq!(prev, 0xAAAA_BBBB_CCCC_DDDD);
        // SAFETY: 测试自有内存。
        assert_eq!(
            unsafe { core::ptr::read_volatile(slot as *const u64) },
            0x1234_5678_9ABC_DEF0
        );
        let prev2 = swap_slot(slot, 0xAAAA_BBBB_CCCC_DDDD).expect("restore failed");
        assert_eq!(prev2, 0x1234_5678_9ABC_DEF0);
        // SAFETY: 测试自有内存。
        assert_eq!(
            unsafe { core::ptr::read_volatile(slot as *const u64) },
            0xAAAA_BBBB_CCCC_DDDD
        );
    }

    /// libc++ string 判形(短/长)。
    #[test]
    fn libcxx_string_form() {
        assert_eq!(string_form((b"abc".len() as u64) << 1), Some((true, 3)));
        assert_eq!(string_form(0x1001), Some((false, 0)));
        assert_eq!(string_form(48), None); // size 24 > 22 → 非短形态且低位 0
    }

    /// 候选上限:满是指针对象的 task 也只解引用 ≤CAND_CAP 个。
    #[test]
    fn candidate_chasing_capped() {
        let mut task = [0u64; TASK_BYTES / 8];
        for (i, slot) in task.iter_mut().take(8).enumerate() {
            let blob: Box<[u64; 2]> = Box::new([0x7000_0000 + i as u64, 0]);
            *slot = Box::leak(blob) as *mut u64 as u64;
        }
        let mut n = 0usize;
        for (i, q) in task.iter().enumerate() {
            if n >= CAND_CAP {
                break;
            }
            if !addr_plausible(*q) {
                continue;
            }
            if safe_read_qword(*q as usize).is_none() {
                continue;
            }
            let _ = i;
            n += 1;
        }
        assert_eq!(n, CAND_CAP);
    }

    /// P2 编码:好友私聊 SendMsgReq 结构(与 core 同字段号;手工解析校验)。
    /// 结构 = r9 现场送达形状:顶层 {1: 路由, 2: ContentHead 三键,
    /// 3: Body{1: Rich{2: [元素37, 文本]}}, 4: seq, 5: rand, 6: {1: ts}}。
    #[test]
    fn send_msg_encoding_friend() {
        // 合成标识(不指向真实账号;uid 存在时 uin 不进 wire)。
        const SYNTH_PEER_UIN: u64 = 2_987_654_321;
        let body = encode_send_msg(
            SYNTH_PEER_UIN,
            Some("u_TESTPEER"),
            0,
            "G3P2",
            7,
            9,
            None,
            0,
            0,
        );
        // 顶层字段序:1(routing)2(content)3(body)4(seq)5(rand)6(ts)。
        let mut pos = 0usize;
        let mut fields = Vec::new();
        while pos < body.len() {
            // tag(varint,单字节足够:field≤5,wire≤2)
            let tag = body[pos];
            pos += 1;
            let field = (tag >> 3) as u32;
            let wire = tag & 7;
            if wire == 2 {
                let mut len = 0u64;
                let mut shift = 0;
                loop {
                    let b = body[pos];
                    pos += 1;
                    len |= ((b & 0x7F) as u64) << shift;
                    if b & 0x80 == 0 {
                        break;
                    }
                    shift += 7;
                }
                fields.push((field, wire, body[pos..pos + len as usize].to_vec()));
                pos += len as usize;
            } else {
                assert_eq!(wire, 0);
                let mut v = 0u64;
                let mut shift = 0;
                loop {
                    let b = body[pos];
                    pos += 1;
                    v |= ((b & 0x7F) as u64) << shift;
                    if b & 0x80 == 0 {
                        break;
                    }
                    shift += 7;
                }
                fields.push((field, wire, v.to_le_bytes().to_vec()));
            }
        }
        assert_eq!(pos, body.len());
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0].0, PB_F_ROUTING_HEAD);
        // routing = {1: C2C{2: uid}} — 解码回读;uid 叶子逐字节核对。
        let r = &fields[0].2;
        // r6 形状:routing = {1: C2C{2: uid}} → r = {0x0a, len, 0x12, len, uid...}
        assert_eq!(r[0], 0x0A, "C2C nested under key1");
        assert_eq!(r[2], 0x12, "uid under key2");
        assert_eq!(&r[4..], b"u_TESTPEER", "uid leaf bytes");
        assert_eq!(fields[1].0, caligo_model::qq_wire::send::PB_F_CONTENT_HEAD);
        // ContentHead 三键 = {1: type(1), 2: 0, 3: 0}(1360ebc/r5 现场实证)。
        assert_eq!(
            fields[1].2,
            &[0x08, 0x01, 0x10, 0x00, 0x18, 0x00],
            "friend ContentHead = {{1:1, 2:0, 3:0}}"
        );
        assert_eq!(fields[2].0, PB_F_BODY);
        // body = {1: rich} 一层包装
        assert_eq!(fields[2].2[0], 0x0A, "rich must nest under body field 1");
        assert_eq!(fields[3].0, PB_F_CLIENT_SEQ);
        assert_eq!(fields[4].0, PB_F_RANDOM);
        assert_eq!(fields[5].0, 6);
        // body 富文本里含元素37(attr 标记字节 aa0217…)
        assert!(
            fields[2]
                .2
                .windows(4)
                .any(|w| w == [0xaa, 0x02, 0x17, 0x88]),
            "general-flags elem missing"
        );
        // body.rich_text.elems.text.str == "G3P2"
        let body_bytes = &fields[2].2;
        let hex = bytes_hex(body_bytes);
        assert!(
            hex.ends_with(&bytes_hex(b"G3P2")),
            "text not at leaf: {hex}"
        );
    }

    /// 短形态串物化。
    #[test]
    fn short_string_materialize() {
        let mut s = [0u8; 24];
        make_short_string(&mut s, b"MessageSvc.PbSendMsg");
        assert_eq!(s[0], 40); // 20<<1
        assert_eq!(&s[1..21], b"MessageSvc.PbSendMsg");
        assert!(s[21..].iter().all(|b| *b == 0));
    }
}
