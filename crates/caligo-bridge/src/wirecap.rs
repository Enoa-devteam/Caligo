//! wire 捕获钩子(N3 研究;research 构建)。
//!
//! 目的:抓 hash-bound send function(MSFService::SendSSORequestInternal)的**真实 wire 实参**
//! —— 原生路径(SnowLuma/手点)与我们裸调的 (cmd, body) 逐字节对照。
//! @ 调查:v6–v9 body 已对齐存储记录仍 1006501(30.5s 无 ack),唯一
//! 未知 = 真实 wire body 与存储记录的差异(存储 ≈ 但 ≠ wire)。
//!
//! 机制:补丁 profile 的两个直接调用点(各 5B E8)
//! → trampoline(g3::trampoline_bytes,field-proven:call capture(a1..a6)
//! 后 jmp 732A80 入口 —— 调用点补丁,入口不动,无循环)。
//!
//! 纪律:
//! - capture 内零 QQ 调用、零堆分配(栈上定长缓冲)、RPM-self 安全读
//!   (坏指针丢行不崩)、文件追加持进程级锁;
//! - 补丁 = 5B E8 rel32;先读回校验(非 E8 = 已被补过/意外 → 跳过,
//!   幂等);VirtualProtect 仅覆盖 5B 所在页,写后恢复 + FlushInstructionCache;
//! - 风险面:补丁窗口内若他线程正执行该调用点(发送极低频)可能踩空 ——
//!   已知残余风险,campaign 纪律下接受。

use core::ffi::c_void;

/// 捕获上下文(进程生存期 leaked)。
#[repr(C)]
pub struct WirecapCtx {
    pub wrapper_base: u64,
    pub report: [u16; 180],
    pub hits: u64,
    pub magic: u64,
}

const CTX_MAGIC: u64 = 0xC0A1_5E09_1DEA;

static INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static CAP_CTX: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

fn rpm_read(addr: usize, buf: &mut [u8]) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut got = 0usize;
    // SAFETY: 自进程 RPM;栈缓冲。
    unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            addr as *const c_void,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut got,
        )
    };
    got == buf.len()
}

/// 栈上定长缓冲(捕获行 ~1.4KB:512B body 的 hex)。
struct CapBuf {
    data: [u8; 1600],
    len: usize,
}

impl CapBuf {
    fn new() -> Self {
        Self { data: [0; 1600], len: 0 }
    }
    fn push_str(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(self.data.len() - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&b[..n]);
        self.len += n;
    }
    fn push_hex_byte(&mut self, b: u8) {
        if self.data.len() - self.len < 4 {
            return;
        }
        let hex = b"0123456789abcdef";
        self.data[self.len] = hex[(b >> 4) as usize];
        self.data[self.len + 1] = hex[(b & 0xf) as usize];
        self.len += 2;
    }
    fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn append_line(ctx: &WirecapCtx, line: &[u8]) {
    use std::os::windows::ffi::OsStringExt;
    let end = ctx.report.iter().position(|c| *c == 0).unwrap_or(180);
    let path =
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&ctx.report[..end]));
    use std::io::Write;
    if let Ok(_g) = FILE_LOCK.lock() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line);
            let _ = f.write_all(b"\n");
        }
    }
}

/// 捕获回调(732A80 调用点 trampoline 调;实参 = 732A80 的 (svc, cmd, body,
/// flag, cb))。读 cmd(__qq string)+ body(vector{begin,end,cap}),JSONL
/// 落盘后返回;trampoline 随后 jmp 原 732A80。
///
/// # Safety
/// 发送线程调用;不得 unwind;坏指针静默丢行。
pub unsafe extern "system" fn wirecap_capture(
    svc: *mut c_void,
    cmd: *mut c_void,
    body: *mut c_void,
    flag: *mut c_void,
    _cb: *mut c_void,
) {
    unsafe {
        let p = *CAP_CTX.get_or_init(|| 0);
        if p <= 0x10000 {
            return;
        }
        let ctx = &*(p as *const WirecapCtx);
        let mut b = CapBuf::new();
        b.push_str("{\"kind\":\"wire_send\",\"ts\":");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        push_dec(&mut b, ts);
        b.push_str(",\"svc\":\"");
        push_hex_addr(&mut b, svc as usize);
        b.push_str("\",\"flag\":");
        push_dec(&mut b, (flag as usize & 0xff) as u64);
        // cmd = __qq string(短形态 byte0=len<<1, data@+1;长形态 {cap|1,size,ptr})。
        b.push_str(",\"cmd\":\"");
        let mut hdr = [0u8; 24];
        if rpm_read(cmd as usize, &mut hdr) {
            if hdr[0] & 1 == 0 {
                let n = (hdr[0] >> 1) as usize;
                let n = n.min(22);
                for &c in &hdr[1..1 + n] {
                    push_json_byte(&mut b, c);
                }
            } else {
                // N3R49 教训:此处曾读 16B 头再切 lh[16..24] → panic → abort
                // → 进程死亡(732A80 = 通用 SSO 通道,长 cmd 一发即触)。
                // 修正:头统一读 24B;size 上限校验后再读 data。
                let size = u64::from_le_bytes(hdr[8..16].try_into().unwrap()) as usize;
                let d = u64::from_le_bytes(hdr[16..24].try_into().unwrap()) as usize;
                let mut big = [0u8; 64];
                if d > 0x10000 && size > 0 && size <= 63 && rpm_read(d, &mut big[..size]) {
                    for &c in &big[..size] {
                        push_json_byte(&mut b, c);
                    }
                }
            }
        }
        b.push_str("\",\"body_len\":");
        // body = libc++ vector<uint8> {begin, end, cap}。
        let mut vh = [0u8; 24];
        let mut blen = 0usize;
        let mut bbegin = 0usize;
        if rpm_read(body as usize, &mut vh) {
            let begin = u64::from_le_bytes(vh[0..8].try_into().unwrap()) as usize;
            let end = u64::from_le_bytes(vh[8..16].try_into().unwrap()) as usize;
            bbegin = begin;
            blen = end.saturating_sub(begin);
        }
        push_dec(&mut b, blen as u64);
        b.push_str(",\"body_hex\":\"");
        if blen > 0 && blen <= 512 && bbegin > 0x10000 {
            let n = blen.min(512);
            let mut buf = [0u8; 512];
            if rpm_read(bbegin, &mut buf[..n]) {
                for (i, byte) in buf[..n].iter().enumerate() {
                    if i > 0 && i % 32 == 0 {
                        b.push_str("_");
                    }
                    b.push_hex_byte(*byte);
                }
            }
        }
        b.push_str("\"}");
        append_line(ctx, b.as_slice());
    }
}

/// 无分配十进制格式化(回调纪律;to_string 族在 QQ 发送线程上禁用)。
fn push_dec(b: &mut CapBuf, v: u64) {
    if v == 0 {
        b.push_str("0");
        return;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    let mut v = v;
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // SAFETY: tmp[i..] 全为 ASCII 数字。
    b.push_str(unsafe { core::str::from_utf8_unchecked(&tmp[i..]) });
}

fn push_hex_addr(b: &mut CapBuf, v: usize) {
    b.push_str("0x");
    let mut tmp = [0u8; 16];
    let mut i = 16;
    let mut v = v as u64;
    if v == 0 {
        b.push_str("0");
        return;
    }
    while v > 0 {
        i -= 1;
        tmp[i] = b"0123456789abcdef"[(v & 0xf) as usize];
        v >>= 4;
    }
    // SAFETY: tmp[i..] 全为 hex ASCII。
    b.push_str(unsafe { core::str::from_utf8_unchecked(&tmp[i..]) });
}

fn push_json_byte(b: &mut CapBuf, c: u8) {
    match c {
        b'"' => b.push_str("\\\""),
        b'\\' => b.push_str("\\\\"),
        0x20..=0x7e => {
            if b.data.len() - b.len > 1 {
                b.data[b.len] = c;
                b.len += 1;
            }
        }
        _ => {
            // 非 ASCII 以 \u00XX 转义(诊断用途)。
            if b.data.len() - b.len > 6 {
                b.push_str("\\u00");
                b.push_hex_byte(c);
            }
        }
    }
}

fn stage(path: &str, stage: &str, ok: bool, detail: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let line = format!(
        "{{\"ts\":{ts},\"stage\":\"wc_{stage}\",\"ok\":{ok},\"detail\":\"{}\"}}\n",
        detail.replace('"', "'")
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 安装钩子(幂等;进程级一次)。补丁两个调用点 → trampoline。
/// 失败仅记录 stage,不影响发送流程。
pub fn install(
    wrapper_base: usize,
    report_path: *const u16,
    send_sso_rva: usize,
    call_sites: [usize; 2],
) {
    if INSTALLED.set(()).is_err() {
        return;
    }
    let mut ctx = WirecapCtx {
        wrapper_base: wrapper_base as u64,
        report: [0; 180],
        hits: 0,
        magic: CTX_MAGIC,
    };
    let path = {
        use std::os::windows::ffi::OsStringExt;
        // SAFETY: loader 写入的 NUL 结尾 UTF-16。
        unsafe {
            let mut v = Vec::new();
            let mut i = 0usize;
            while i < 179 && *report_path.add(i) != 0 {
                v.push(*report_path.add(i));
                i += 1;
            }
            let p = std::ffi::OsString::from_wide(&v)
                .to_string_lossy()
                .into_owned();
            for (i, c) in v.iter().take(179).enumerate() {
                ctx.report[i] = *c;
            }
            p
        }
    };
    if CAP_CTX
        .set(Box::leak(Box::new(ctx)) as *const WirecapCtx as usize)
        .is_err()
    {
        return;
    }
    match install_inner(wrapper_base, &path, send_sso_rva, call_sites) {
        Ok(n) => stage(&path, "install", true, &format!("patched={n}")),
        Err(e) => stage(&path, "install", false, &e),
    }
}

/// 卸载钩子(N3R50 教训后停用 wirecap):把 profile 的两个调用点写回原始
/// E8 rel32 target。幂等:已是原始字节则跳过。无 trampoline 依赖。
pub fn uninstall(
    wrapper_base: usize,
    report_path: *const u16,
    send_sso_rva: usize,
    call_sites: [usize; 2],
) {
    use windows_sys::Win32::System::Memory::VirtualProtect;
    let mut ctx_report = [0u16; 180];
    // SAFETY: loader 写入的 NUL 结尾 UTF-16。
    unsafe {
        let mut i = 0usize;
        while i < 179 && *report_path.add(i) != 0 {
            ctx_report[i] = *report_path.add(i);
            i += 1;
        }
    }
    let path = {
        use std::os::windows::ffi::OsStringExt;
        let end = ctx_report.iter().position(|c| *c == 0).unwrap_or(180);
        std::ffi::OsString::from_wide(&ctx_report[..end]).to_string_lossy().into_owned()
    };
    let mut restored = 0usize;
    for site in call_sites {
        let addr = wrapper_base + site;
        let orig_rel = (send_sso_rva as i64) - (site as i64) - 5;
        let want = {
            let mut v = vec![0xE8u8];
            v.extend_from_slice(&orig_rel.to_le_bytes());
            v
        };
        let mut cur = [0u8; 5];
        if !rpm_read(addr, &mut cur) {
            stage(&path, "uninstall", false, &format!("site {site:#x} unreadable"));
            continue;
        }
        if cur == want[..] {
            stage(&path, "uninstall", true, &format!("site {site:#x} already original"));
            continue;
        }
        let mut old_prot = 0u32;
        let mut back = 0u32;
        // SAFETY: 5B 写回已知原始字节(非 trampoline 目标已核实)。
        unsafe {
            if VirtualProtect(addr as *const c_void, 5, 0x40, &mut old_prot) == 0 {
                stage(&path, "uninstall", false, &format!("site {site:#x} VP fail"));
                continue;
            }
            core::ptr::copy_nonoverlapping(want.as_ptr(), addr as *mut u8, 5);
            let _ = VirtualProtect(addr as *const c_void, 5, old_prot, &mut back);
        }
        restored += 1;
        stage(&path, "uninstall", true, &format!("site {site:#x} restored"));
    }
    stage(&path, "uninstall_done", true, &format!("restored={restored}"));
}

fn install_inner(
    wrapper_base: usize,
    path: &str,
    send_sso_rva: usize,
    call_sites: [usize; 2],
) -> Result<usize, String> {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
        PAGE_READWRITE,
    };
    let target = wrapper_base + send_sso_rva;
    // SAFETY: RWX 一页放 trampoline(66B)。
    let tramp = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            0x1000,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        )
    };
    if tramp.is_null() {
        return Err("VirtualAlloc failed".into());
    }
    let bytes = crate::g3::trampoline_bytes(wirecap_capture as usize, target);
    // SAFETY: tramp 容量 0x1000 ≥ bytes.len()。
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), tramp as *mut u8, bytes.len());
    }
    let mut patched = 0usize;
    for site in call_sites {
        let addr = wrapper_base + site;
        let mut orig = [0u8; 5];
        if !rpm_read(addr, &mut orig) {
            stage(path, "read", false, &format!("site {site:#x} unreadable"));
            continue;
        }
        // 原始 5B 校验:E8 rel32 且 rel 指向 profile send RVA。已补丁站点(指向旧
        // trampoline)一律跳过 —— N3R47 教训:仅查 E8 会误重补已补丁站点。
        let orig_rel = i32::from_le_bytes(orig[1..5].try_into().unwrap());
        let orig_tgt = (addr as i64) + 5 + orig_rel as i64;
        if orig[0] != 0xE8 || orig_tgt != (wrapper_base + send_sso_rva) as i64 {
            stage(
                path,
                "verify",
                false,
                &format!(
                    "site {site:#x} not original call (tgt {orig_tgt:#x} vs {:#x}) — skip",
                    wrapper_base + send_sso_rva
                ),
            );
            continue;
        }
        let rel = (tramp as usize).wrapping_sub(addr + 5) as i32;
        let mut patch = Vec::with_capacity(5);
        patch.push(0xE8u8);
        patch.extend_from_slice(&rel.to_le_bytes());
        let mut old_prot = 0u32;
        let mut restored = 0u32;
        // SAFETY: wrapper .text 页;5B 原子性不足但发送低频,残余风险已评估。
        unsafe {
            if VirtualProtect(addr as *const c_void, 5, PAGE_READWRITE, &mut old_prot) == 0 {
                return Err(format!("VirtualProtect(site {site:#x}) failed"));
            }
            core::ptr::copy_nonoverlapping(patch.as_ptr(), addr as *mut u8, 5);
            let _ = VirtualProtect(addr as *const c_void, 5, old_prot, &mut restored);
        }
        patched += 1;
        stage(path, "patch", true, &format!("site {site:#x} -> tramp ok"));
    }
    Ok(patched)
}
