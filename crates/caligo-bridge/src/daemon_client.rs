//! K4-D7b bridge 侧 daemon 客户端(仅 research 构建)。
//!
//! 把 daemon_chain 的 LAB 链延伸进 QQ 进程:常驻 worker 线程(自有线程,
//! 只做管道 I/O 与 resident 提交,**零 QQ/V8 API** —— 计划 §3.1)连接
//! caligod 的 bridge 管道,说 IPC v3:
//! - Hello(认证/基线/账号/会话代次)→ HelloAck;
//! - Dispatch → NativeStarted → `resident.submit(SendText)` → `wake()`
//!   (uv_async_send,唯一合法跨线程面)→ owner pump drain → 结果经
//!   result 通道回传 → SendResult;
//! - Event 上行(带未确认环形窗口;EventAck 前移;重连重放未确认项);
//! - 心跳 5s(3 次未响应即断开重连);重连退避 1/2/4…30s;
//! - **重连绝不重新 bootstrap**(`bootstraps_attempted` 恒 0,由计数证明);
//! - 认证/身份被拒 → 不可重试,立即退出(不暴力重连)。

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use caligo_model::framing::{encode_frame, FrameDecoder, FrameError};
use caligo_model::ipc_v3::{
    BridgeMsg, CoreToBridgeMsg, EventPayload, OutcomePayload, Role, BUSINESS_PROTOCOL_REVISION,
    PROTOCOL_VERSION_V3,
};

use crate::resident::{Resident, ResidentNotice, SendOutcome};

/// 宿主实例身份:进程内一次生成,保持到宿主进程退出(C9)。
fn process_created_utc() -> String {
    // SAFETY: 只读查询当前进程;缓冲由调用方提供。
    unsafe {
        use windows_sys::Win32::Foundation::FILETIME;
        let mut created = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut exited = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut user = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let handle = windows_sys::Win32::System::Threading::GetCurrentProcess();
        if windows_sys::Win32::System::Threading::GetProcessTimes(
            handle,
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return String::new(); // 取不到必须如实为空(不得伪造)
        }
        let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        // FILETIME → unix 秒(1601-01-01 起 100ns;偏移 11644473600s)。
        let unix_secs = (ticks / 10_000_000).wrapping_sub(11_644_473_600);
        format!("{unix_secs}")
    }
}

/// 宿主 nonce:PID 与进程存活期锚定;同进程恒定,跨进程不重复。
fn host_instance_nonce() -> u64 {
    static NONCE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *NONCE.get_or_init(|| {
        let pid = std::process::id() as u64;
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        (pid << 32) | (ms & 0xffff_ffff)
    })
}

/// worker 配置。
#[derive(Debug, Clone)]
pub struct DaemonClientConfig {
    pub pipe_name: String,
    pub auth_token: String,
    pub bridge_build: String,
    pub session_generation: u64,
    pub account: String,
    pub module_baseline: String,
    /// 宿主实例 nonce(C9;默认进程内一次生成)。
    pub host_nonce: u64,
    /// 宿主进程创建时间(取不到为空;默认 GetProcessTimes 的 unix 秒)。
    pub host_process_created_utc: String,
    /// 本账号 uid(u_* 形态;可选)。CPB 记录的 sender 为 uid 形态,
    /// 与 account(uin)不同域 —— 提供时 direction 判别才能识别自发回流;
    /// 空 = 一律 incoming(不猜映射)。
    pub self_uid: String,
    /// 心跳间隔(计划 §6.4 建议 5s)。
    pub heartbeat_ms: u64,
    /// 重连退避起点(计划 §6.4:1/2/4…上限 30s)。
    pub reconnect_backoff_ms: u64,
}

impl Default for DaemonClientConfig {
    fn default() -> Self {
        Self {
            pipe_name: String::new(),
            auth_token: String::new(),
            bridge_build: concat!("caligo-bridge ", env!("CARGO_PKG_VERSION")).into(),
            session_generation: 0,
            account: String::new(),
            module_baseline: String::new(),
            host_nonce: host_instance_nonce(),
            host_process_created_utc: process_created_utc(),
            self_uid: String::new(),
            heartbeat_ms: 5_000,
            reconnect_backoff_ms: 1_000,
        }
    }
}

/// worker 计数(资源与行为审计;`bootstraps_attempted` 恒 0 是重连判据)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WorkerCounters {
    pub connects: u64,
    pub connect_failures: u64,
    pub reconnects: u64,
    pub hellos_accepted: u64,
    pub hello_rejected: u64,
    pub dispatches: u64,
    pub results_sent: u64,
    pub events_sent: u64,
    pub events_acked: u64,
    pub events_gap_dropped: u64,
    pub heartbeats_sent: u64,
    pub heartbeat_timeouts: u64,
    pub protocol_errors: u64,
    pub clean_stops: u64,
    pub bootstraps_attempted: u64,
}

/// 进程级原子计数(worker 在 QQ 内运行,导出侧可随时取证)。
#[derive(Debug, Default)]
pub struct AtomicCounters {
    pub connects: AtomicU64,
    pub connect_failures: AtomicU64,
    pub reconnects: AtomicU64,
    pub hellos_accepted: AtomicU64,
    pub hello_rejected: AtomicU64,
    pub dispatches: AtomicU64,
    pub results_sent: AtomicU64,
    pub events_sent: AtomicU64,
    pub events_acked: AtomicU64,
    pub events_gap_dropped: AtomicU64,
    pub heartbeats_sent: AtomicU64,
    pub heartbeat_timeouts: AtomicU64,
    pub protocol_errors: AtomicU64,
    pub clean_stops: AtomicU64,
    pub bootstraps_attempted: AtomicU64,
}

static GLOBAL_COUNTERS: AtomicCounters = AtomicCounters {
    connects: AtomicU64::new(0),
    connect_failures: AtomicU64::new(0),
    reconnects: AtomicU64::new(0),
    hellos_accepted: AtomicU64::new(0),
    hello_rejected: AtomicU64::new(0),
    dispatches: AtomicU64::new(0),
    results_sent: AtomicU64::new(0),
    events_sent: AtomicU64::new(0),
    events_acked: AtomicU64::new(0),
    events_gap_dropped: AtomicU64::new(0),
    heartbeats_sent: AtomicU64::new(0),
    heartbeat_timeouts: AtomicU64::new(0),
    protocol_errors: AtomicU64::new(0),
    clean_stops: AtomicU64::new(0),
    bootstraps_attempted: AtomicU64::new(0),
};

/// 计数快照(导出侧取证;字段与顺序即 JSONL 输出顺序)。
pub fn counters_snapshot() -> WorkerCounters {
    let g = &GLOBAL_COUNTERS;
    WorkerCounters {
        connects: g.connects.load(Ordering::Relaxed),
        connect_failures: g.connect_failures.load(Ordering::Relaxed),
        reconnects: g.reconnects.load(Ordering::Relaxed),
        hellos_accepted: g.hellos_accepted.load(Ordering::Relaxed),
        hello_rejected: g.hello_rejected.load(Ordering::Relaxed),
        dispatches: g.dispatches.load(Ordering::Relaxed),
        results_sent: g.results_sent.load(Ordering::Relaxed),
        events_sent: g.events_sent.load(Ordering::Relaxed),
        events_acked: g.events_acked.load(Ordering::Relaxed),
        events_gap_dropped: g.events_gap_dropped.load(Ordering::Relaxed),
        heartbeats_sent: g.heartbeats_sent.load(Ordering::Relaxed),
        heartbeat_timeouts: g.heartbeat_timeouts.load(Ordering::Relaxed),
        protocol_errors: g.protocol_errors.load(Ordering::Relaxed),
        clean_stops: g.clean_stops.load(Ordering::Relaxed),
        bootstraps_attempted: g.bootstraps_attempted.load(Ordering::Relaxed),
    }
}

impl WorkerCounters {
    fn publish(&self) {
        let g = &GLOBAL_COUNTERS;
        g.connects.store(self.connects, Ordering::Relaxed);
        g.connect_failures
            .store(self.connect_failures, Ordering::Relaxed);
        g.reconnects.store(self.reconnects, Ordering::Relaxed);
        g.hellos_accepted
            .store(self.hellos_accepted, Ordering::Relaxed);
        g.hello_rejected
            .store(self.hello_rejected, Ordering::Relaxed);
        g.dispatches.store(self.dispatches, Ordering::Relaxed);
        g.results_sent.store(self.results_sent, Ordering::Relaxed);
        g.events_sent.store(self.events_sent, Ordering::Relaxed);
        g.events_acked.store(self.events_acked, Ordering::Relaxed);
        g.events_gap_dropped
            .store(self.events_gap_dropped, Ordering::Relaxed);
        g.heartbeats_sent
            .store(self.heartbeats_sent, Ordering::Relaxed);
        g.heartbeat_timeouts
            .store(self.heartbeat_timeouts, Ordering::Relaxed);
        g.protocol_errors
            .store(self.protocol_errors, Ordering::Relaxed);
        g.clean_stops.store(self.clean_stops, Ordering::Relaxed);
        g.bootstraps_attempted
            .store(self.bootstraps_attempted, Ordering::Relaxed);
    }

    pub fn as_json(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("{\"connects\":");
        out.push_str(&self.connects.to_string());
        out.push_str(",\"connect_failures\":");
        out.push_str(&self.connect_failures.to_string());
        out.push_str(",\"reconnects\":");
        out.push_str(&self.reconnects.to_string());
        out.push_str(",\"hellos_accepted\":");
        out.push_str(&self.hellos_accepted.to_string());
        out.push_str(",\"hello_rejected\":");
        out.push_str(&self.hello_rejected.to_string());
        out.push_str(",\"dispatches\":");
        out.push_str(&self.dispatches.to_string());
        out.push_str(",\"results_sent\":");
        out.push_str(&self.results_sent.to_string());
        out.push_str(",\"events_sent\":");
        out.push_str(&self.events_sent.to_string());
        out.push_str(",\"events_acked\":");
        out.push_str(&self.events_acked.to_string());
        out.push_str(",\"events_gap_dropped\":");
        out.push_str(&self.events_gap_dropped.to_string());
        out.push_str(",\"heartbeats_sent\":");
        out.push_str(&self.heartbeats_sent.to_string());
        out.push_str(",\"heartbeat_timeouts\":");
        out.push_str(&self.heartbeat_timeouts.to_string());
        out.push_str(",\"protocol_errors\":");
        out.push_str(&self.protocol_errors.to_string());
        out.push_str(",\"clean_stops\":");
        out.push_str(&self.clean_stops.to_string());
        out.push_str(",\"bootstraps_attempted\":");
        out.push_str(&self.bootstraps_attempted.to_string());
        out.push('}');
        out
    }
}

/// worker 退出原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerExit {
    /// 收到 Stopped(core 侧真实停止)。
    CoreStopped,
    /// stop 标志置位(本侧关闭协议)。
    LocalStop,
    /// Hello 被拒(认证/身份):不可重试。
    HelloRejected,
}

/// 上行事件未确认窗口:满(项数或字节)→ 记 Gap 丢最旧(计数如实)。
const EVENT_WINDOW_MAX_ITEMS: usize = 256;
const EVENT_WINDOW_MAX_BYTES: usize = 8 * 1024 * 1024;
const RESULT_OUTBOX_MAX_ITEMS: usize = 256;

struct PendingNotice {
    notice: ResidentNotice,
    sent_epoch: Option<u64>,
}

fn notice_request_id(notice: &ResidentNotice) -> &str {
    match notice {
        ResidentNotice::NativeStarted { request_id } => request_id,
        ResidentNotice::Outcome(
            SendOutcome::Success { request_id, .. }
            | SendOutcome::AcceptedUnconfirmed { request_id }
            | SendOutcome::Failure { request_id, .. }
            | SendOutcome::Unknown { request_id, .. },
        ) => request_id,
    }
}

fn notice_message(notice: &ResidentNotice) -> (BridgeMsg, bool) {
    match notice {
        ResidentNotice::NativeStarted { request_id } => (
            BridgeMsg::NativeStarted {
                request_id: request_id.clone(),
            },
            false,
        ),
        ResidentNotice::Outcome(outcome) => (
            match outcome {
                SendOutcome::Success {
                    request_id,
                    native_id,
                } => BridgeMsg::SendResult {
                    request_id: request_id.clone(),
                    outcome: OutcomePayload::Success {
                        native_id: native_id.clone(),
                    },
                },
                SendOutcome::AcceptedUnconfirmed { request_id } => BridgeMsg::SendResult {
                    request_id: request_id.clone(),
                    outcome: OutcomePayload::AcceptedUnconfirmed,
                },
                SendOutcome::Failure { request_id, reason } => BridgeMsg::SendResult {
                    request_id: request_id.clone(),
                    outcome: OutcomePayload::Failure {
                        reason: reason.clone(),
                    },
                },
                SendOutcome::Unknown {
                    request_id, reason, ..
                } => BridgeMsg::SendResult {
                    request_id: request_id.clone(),
                    outcome: OutcomePayload::Unknown {
                        reason: reason.clone(),
                    },
                },
            },
            true,
        ),
    }
}

/// 最小管道客户端:OVERLAPPED 读写字节模式管道(与 core::transport 同语义;
/// bridge 不能依赖 core —— 独立实现,CLG1 帧取自 model)。
struct PipeConn {
    handle: *mut core::ffi::c_void,
}
// SAFETY: 管道句柄非线程亲和;worker 单线程独占使用。
unsafe impl Send for PipeConn {}

#[derive(Debug)]
enum PipeError {
    /// 附操作名与 Win32 错误码(计数器/日志用)。
    Win32 {
        op: &'static str,
        code: u32,
    },
    TimedOut,
    BrokenPipe,
}
impl PipeError {
    #[allow(dead_code)]
    fn describe(&self) -> String {
        match self {
            PipeError::Win32 { op, code } => format!("{op}: Win32 error {code}"),
            PipeError::TimedOut => "timed out".into(),
            PipeError::BrokenPipe => "pipe broken".into(),
        }
    }
}

impl PipeConn {
    fn connect(name: &str) -> Result<Self, PipeError> {
        // 短名自动补 Win32 管道根(与 core::daemon::pipe_names 同规则)——
        // D7-b 现场教训:配置里的短名在 QQ 进程内解析失败,worker 永远
        // 退避重连一个不存在的名字。
        const ROOT: &str = "\\\\.\\pipe\\";
        let name = if name.starts_with(ROOT) {
            name.to_string()
        } else {
            format!("{ROOT}{}", name.trim_start_matches('\\'))
        };
        let wide: Vec<u16> = OsStr::new(&name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: name 在调用期间有效;句柄由 self 管理。
        let handle = unsafe {
            windows_sys::Win32::Storage::FileSystem::CreateFileW(
                wide.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE,
                windows_sys::Win32::Storage::FileSystem::FILE_SHARE_NONE,
                std::ptr::null(),
                windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING,
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(PipeError::Win32 {
                op: "CreateFileW",
                // SAFETY: GetLastError 无副作用。
                code: unsafe { windows_sys::Win32::Foundation::GetLastError() },
            });
        }
        Ok(Self { handle })
    }

    fn read_some(&self, buf: &mut [u8], cancel: &AtomicBool) -> Result<usize, PipeError> {
        // SAFETY: OVERLAPPED/事件在本函数内创建并关闭;取消后回收。
        unsafe {
            let event = windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
            );
            if event.is_null() || event == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return Err(PipeError::Win32 {
                    op: "CreateEventW",
                    code: windows_sys::Win32::Foundation::GetLastError(),
                });
            }
            let mut ov: windows_sys::Win32::System::IO::OVERLAPPED = std::mem::zeroed();
            ov.hEvent = event;
            let mut got = 0u32;
            let ok = windows_sys::Win32::Storage::FileSystem::ReadFile(
                self.handle,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut got,
                &mut ov,
            );
            if ok == 0 {
                let e = windows_sys::Win32::Foundation::GetLastError();
                if e != windows_sys::Win32::Foundation::ERROR_IO_PENDING {
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                        return Err(PipeError::BrokenPipe);
                    }
                    return Err(PipeError::Win32 {
                        op: "ReadFile",
                        code: e,
                    });
                }
            }
            loop {
                let mut transferred = 0u32;
                if windows_sys::Win32::System::IO::GetOverlappedResult(
                    self.handle,
                    &mut ov,
                    &mut transferred,
                    0,
                ) != 0
                {
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    return Ok(transferred as usize);
                }
                let e = windows_sys::Win32::Foundation::GetLastError();
                if e != windows_sys::Win32::Foundation::ERROR_IO_INCOMPLETE {
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                        return Err(PipeError::BrokenPipe);
                    }
                    return Err(PipeError::Win32 {
                        op: "GetOverlappedResult",
                        code: e,
                    });
                }
                let w = windows_sys::Win32::System::Threading::WaitForSingleObject(event, 100);
                if w == 0 {
                    continue;
                }
                if w == 0x00000102 {
                    // WAIT_TIMEOUT:轮询 cancel;置位 → 取消并回收(计划 §5.3)。
                    // 部分读必须交付(同 core::transport 的帧流撕毁修复)。
                    if cancel.load(Ordering::Relaxed) {
                        windows_sys::Win32::System::IO::CancelIoEx(self.handle, &mut ov);
                        let mut t = 0u32;
                        let ok = windows_sys::Win32::System::IO::GetOverlappedResult(
                            self.handle,
                            &mut ov,
                            &mut t,
                            1,
                        );
                        windows_sys::Win32::Foundation::CloseHandle(event);
                        if ok != 0 && t > 0 {
                            return Ok(t as usize);
                        }
                        return Err(PipeError::TimedOut);
                    }
                    continue;
                }
                windows_sys::Win32::Foundation::CloseHandle(event);
                return Err(PipeError::Win32 {
                    op: "WaitForSingleObject",
                    code: windows_sys::Win32::Foundation::GetLastError(),
                });
            }
        }
    }

    fn write_all(&self, mut bytes: &[u8]) -> Result<(), PipeError> {
        // SAFETY: 同 read_some(每次写独立 OVERLAPPED,写完即回收)。
        unsafe {
            while !bytes.is_empty() {
                let event = windows_sys::Win32::System::Threading::CreateEventW(
                    std::ptr::null(),
                    1,
                    0,
                    std::ptr::null(),
                );
                if event.is_null() || event == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
                {
                    return Err(PipeError::Win32 {
                        op: "CreateEventW",
                        code: windows_sys::Win32::Foundation::GetLastError(),
                    });
                }
                let mut ov: windows_sys::Win32::System::IO::OVERLAPPED = std::mem::zeroed();
                ov.hEvent = event;
                let mut wrote = 0u32;
                let ok = windows_sys::Win32::Storage::FileSystem::WriteFile(
                    self.handle,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    &mut wrote,
                    &mut ov,
                );
                if ok == 0 {
                    let e = windows_sys::Win32::Foundation::GetLastError();
                    if e != windows_sys::Win32::Foundation::ERROR_IO_PENDING {
                        windows_sys::Win32::Foundation::CloseHandle(event);
                        if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                            return Err(PipeError::BrokenPipe);
                        }
                        return Err(PipeError::Win32 {
                            op: "WriteFile",
                            code: e,
                        });
                    }
                }
                let mut transferred = 0u32;
                if windows_sys::Win32::System::IO::GetOverlappedResult(
                    self.handle,
                    &mut ov,
                    &mut transferred,
                    1,
                ) == 0
                {
                    let e = windows_sys::Win32::Foundation::GetLastError();
                    windows_sys::Win32::Foundation::CloseHandle(event);
                    if e == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE {
                        return Err(PipeError::BrokenPipe);
                    }
                    return Err(PipeError::Win32 {
                        op: "GetOverlappedResult(write)",
                        code: e,
                    });
                }
                windows_sys::Win32::Foundation::CloseHandle(event);
                bytes = &bytes[transferred as usize..];
            }
        }
        Ok(())
    }
}

impl Drop for PipeConn {
    fn drop(&mut self) {
        // SAFETY: 句柄唯一所有者。
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

fn send_json(conn: &PipeConn, msg: &BridgeMsg) -> Result<(), PipeError> {
    let payload = serde_json::to_vec(msg).map_err(|_| PipeError::BrokenPipe)?;
    let frame = encode_frame(&payload).map_err(|_| PipeError::BrokenPipe)?;
    conn.write_all(&frame)
}

/// 每连接长寿命帧管道:增量 decoder + 完整帧 FIFO(C3 修复)。
///
/// 合同(与 core 侧 FrameStream 一致):
/// - 单次读合并的多帧按序全交付一次,不丢不重(T01);
/// - 半帧跨唤醒/取消保留前缀(T02 客户端侧);
/// - 新连接新建实例,不复用旧前缀。
struct FramePipe {
    dec: FrameDecoder,
    queue: VecDeque<Vec<u8>>,
}

impl FramePipe {
    fn new() -> Self {
        Self {
            dec: FrameDecoder::new(),
            queue: VecDeque::new(),
        }
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<(), PipeError> {
        let mut out = Vec::new();
        match self.dec.push(chunk, &mut out) {
            Ok(()) => {
                self.queue.extend(out);
                Ok(())
            }
            // 流错位:断开重连(解码器状态不可信)。
            Err(FrameError::MagicMismatch(_) | FrameError::FrameTooLarge(_)) => {
                Err(PipeError::BrokenPipe)
            }
        }
    }

    /// FIFO 下一帧;`Ok(None)` = 尚无完整帧。载荷损坏为致命(BrokenPipe)。
    fn next_msg(&mut self) -> Result<Option<CoreToBridgeMsg>, PipeError> {
        match self.queue.pop_front() {
            None => Ok(None),
            Some(payload) => serde_json::from_slice(&payload)
                .map(Some)
                .map_err(|_| PipeError::BrokenPipe),
        }
    }
}

/// 阻塞收一帧;`tick` 置位时以 TimedOut 返回(已消费字节保留在 pipe)。
fn recv_json(
    conn: &PipeConn,
    pipe: &mut FramePipe,
    tick: &AtomicBool,
) -> Result<CoreToBridgeMsg, PipeError> {
    loop {
        if let Some(msg) = pipe.next_msg()? {
            return Ok(msg);
        }
        let mut buf = [0u8; 4096];
        match conn.read_some(&mut buf, tick) {
            Ok(0) => return Err(PipeError::BrokenPipe),
            Ok(n) => pipe.feed(&buf[..n])?,
            Err(e) => return Err(e),
        }
    }
}

/// worker 主循环(自有线程内运行)。`result_rx` 由 owner 侧 drain hook 供给;
/// `wake` = uv_async_send 包装。返回 (计数, 退出原因)。
pub fn run_worker<A: crate::host_adapter::HostAdapter>(
    cfg: DaemonClientConfig,
    resident: Arc<Resident<A>>,
    wake: &dyn Fn() -> bool,
    result_rx: mpsc::Receiver<ResidentNotice>,
    events_rx: mpsc::Receiver<crate::resident::OwnedEvent>,
    stop: Arc<AtomicBool>,
) -> (WorkerCounters, WorkerExit) {
    let mut c = WorkerCounters::default();
    let mut backoff = cfg.reconnect_backoff_ms.max(100);
    // 事件序号与确认游标:跨重连保持(worker 生存期内单调;wire 层身份)。
    let mut next_local_seq: u64 = 1;
    let mut acked_high: u64 = 0;
    // 未确认事件窗口:跨连接持有(C4 修复)——内容与 ACK 游标生命周期一致;
    // 断开前已发送未确认的事件在新连接上重放对账,不再随连接重建而丢失。
    let mut unacked: VecDeque<(u64, EventPayload)> = VecDeque::new();
    let mut unacked_bytes: usize = 0;
    // Native lifecycle notices survive connection teardown until the core
    // confirms durable application. Resident reserves at most two slots/send.
    let mut result_outbox: VecDeque<PendingNotice> = VecDeque::new();
    macro_rules! publish {
        () => {
            c.publish();
        };
    }
    loop {
        publish!(); // 实时计数:导出侧快照随时可取证(退出时再终发布)
        if stop.load(Ordering::Relaxed) {
            publish!();
            return (c, WorkerExit::LocalStop);
        }
        // —— 连接 ——
        let conn = match PipeConn::connect(&cfg.pipe_name) {
            Ok(conn) => conn,
            Err(_) => {
                c.connect_failures += 1;
                if c.connects > 0 {
                    c.reconnects += 1;
                }
                std::thread::sleep(Duration::from_millis(backoff));
                backoff = (backoff * 2).min(30_000);
                continue;
            }
        };
        c.connects += 1;
        backoff = cfg.reconnect_backoff_ms.max(100);
        // —— Hello(身份/认证;被拒不重试)——
        if send_json(
            &conn,
            &BridgeMsg::Hello {
                protocol_version: PROTOCOL_VERSION_V3,
                business_protocol_revision: BUSINESS_PROTOCOL_REVISION,
                bridge_build: cfg.bridge_build.clone(),
                auth_token: cfg.auth_token.clone(),
                role: Role::Bridge,
                session_generation: cfg.session_generation,
                account: cfg.account.clone(),
                module_baseline: cfg.module_baseline.clone(),
                host_nonce: cfg.host_nonce,
                host_process_created_utc: cfg.host_process_created_utc.clone(),
            },
        )
        .is_err()
        {
            c.connect_failures += 1;
            std::thread::sleep(Duration::from_millis(backoff));
            continue;
        }
        let mut pipe = FramePipe::new();
        let tick = Arc::new(AtomicBool::new(false));
        // 心跳看门狗:100ms 置位 tick,使阻塞 recv 周期返回。
        let watchdog_stop = Arc::new(AtomicBool::new(false));
        {
            let tick = tick.clone();
            let wstop = watchdog_stop.clone();
            std::thread::spawn(move || {
                while !wstop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    tick.store(true, Ordering::Relaxed);
                }
            });
        }
        let session = session_loop(
            &conn,
            &mut pipe,
            &cfg,
            &resident,
            wake,
            &result_rx,
            &events_rx,
            &tick,
            &stop,
            &mut c,
            &mut next_local_seq,
            &mut acked_high,
            &mut unacked,
            &mut unacked_bytes,
            &mut result_outbox,
        );
        watchdog_stop.store(true, Ordering::Relaxed);
        match session {
            SessionEnd::CoreStopped => {
                c.clean_stops += 1;
                publish!();
                return (c, WorkerExit::CoreStopped);
            }
            SessionEnd::LocalStop => {
                publish!();
                return (c, WorkerExit::LocalStop);
            }
            SessionEnd::HelloRejected => {
                c.hello_rejected += 1;
                publish!();
                return (c, WorkerExit::HelloRejected);
            }
            SessionEnd::Broken => {
                c.reconnects += 1;
                std::thread::sleep(Duration::from_millis(backoff));
                backoff = (backoff * 2).min(30_000);
            }
        }
    }
}

enum SessionEnd {
    CoreStopped,
    LocalStop,
    HelloRejected,
    Broken,
}

#[allow(clippy::too_many_arguments)]
fn session_loop<A: crate::host_adapter::HostAdapter>(
    conn: &PipeConn,
    pipe: &mut FramePipe,
    cfg: &DaemonClientConfig,
    resident: &Arc<Resident<A>>,
    wake: &dyn Fn() -> bool,
    result_rx: &mpsc::Receiver<ResidentNotice>,
    events_rx: &mpsc::Receiver<crate::resident::OwnedEvent>,
    tick: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    c: &mut WorkerCounters,
    next_local_seq: &mut u64,
    acked_high: &mut u64,
    unacked: &mut VecDeque<(u64, EventPayload)>,
    unacked_bytes: &mut usize,
    result_outbox: &mut VecDeque<PendingNotice>,
) -> SessionEnd {
    // 未确认事件窗口由 run_worker 持有(跨连接,C4);此处只借用。
    let mut last_heartbeat = Instant::now();
    let mut heartbeat_misses: u32 = 0;
    // resident 未就绪期的待提交队列(owner 初始化完成后补交)。
    let mut pending: VecDeque<(String, u32, u64, u32, String, String)> = VecDeque::new(); // id, wire seq, timeout, chat, peer, text

    // HelloAck(阻塞;被拒 → 不可重试)。
    let connection_epoch = loop {
        match recv_json(conn, pipe, tick) {
            Ok(CoreToBridgeMsg::HelloAck {
                accepted,
                reject_reason,
                connection_epoch,
                ..
            }) => {
                if accepted {
                    c.hellos_accepted += 1;
                    break connection_epoch;
                }
                let _ = reject_reason;
                return SessionEnd::HelloRejected;
            }
            Ok(_) => {
                c.protocol_errors += 1;
                return SessionEnd::HelloRejected;
            }
            Err(PipeError::TimedOut) => {
                if stop.load(Ordering::Relaxed) {
                    return SessionEnd::LocalStop;
                }
                continue;
            }
            Err(_) => return SessionEnd::Broken,
        }
    };

    loop {
        if stop.load(Ordering::Relaxed) {
            return SessionEnd::LocalStop;
        }
        c.publish(); // 实时计数(导出侧快照取证)
                     // 0) 待提交补交(resident 就绪后)。
        let plen = pending.len();
        for _ in 0..plen {
            let (id, wire_seq, timeout_ms, chat, peer, text) =
                pending.front().cloned().expect("nonempty");
            match resident.submit(crate::resident::OwnedRequest::SendText {
                request_id: id.clone(),
                wire_seq,
                timeout_ms,
                chat_type: chat,
                peer_uid: peer,
                text: text.clone(),
            }) {
                crate::resident::SubmitVerdict::Accepted => {
                    pending.pop_front();
                    wake();
                }
                _ => break, // 仍未就绪:保留队列,下轮再试
            }
        }
        // 1) 收入有界 outbox。其上限与 Resident 的默认预留结果槽对齐;
        // 已满则不再消费源队列,结果仍由 Resident 保留。
        while result_outbox.len() < RESULT_OUTBOX_MAX_ITEMS {
            match result_rx.try_recv() {
                Ok(notice) => result_outbox.push_back(PendingNotice {
                    notice,
                    sent_epoch: None,
                }),
                Err(_) => break,
            }
        }
        // 首次发送或新连接重放。FIFO 保证 NativeStarted 排在终态之前。
        for pending in result_outbox.iter_mut() {
            if pending.sent_epoch == Some(connection_epoch) {
                continue;
            }
            let (msg, is_result) = notice_message(&pending.notice);
            if send_json(conn, &msg).is_err() {
                return SessionEnd::Broken;
            }
            pending.sent_epoch = Some(connection_epoch);
            if is_result {
                c.results_sent += 1;
            }
        }
        // 2) 事件上行:泵钩子经通道转交的 owned 事件(D8)+ 窗口重放。
        while let Ok(ev) = events_rx.try_recv() {
            let wire_seq = *next_local_seq;
            *next_local_seq += 1;
            let payload = EventPayload {
                event_seq: wire_seq, // wire 身份 = worker 本地 seq(EventAck 回显键)
                session_generation: cfg.session_generation,
                session: serde_json::json!({
                    "account": cfg.account,
                    // kind 零猜测:2=群、1=私聊、其他(含 native 路线的
                    // unknown=0)如实上送,core 按"缺必需真实字段"处理。
                    "kind": match ev.chat_type {
                        2 => "group",
                        1 => "private",
                        _ => "unknown",
                    },
                    "peer": ev.peer_uid,
                }),
                // 自发判别:account(uin)或 self_uid(uid)双域比对;
                // 两者都未提供/不匹配 = incoming(不猜映射)。
                direction: if (!cfg.account.is_empty() && ev.sender_uin == cfg.account)
                    || (!cfg.self_uid.is_empty() && ev.sender_uin == cfg.self_uid)
                {
                    "self_sent"
                } else {
                    "incoming"
                }
                .into(),
                sender: ev.sender_uin.clone(),
                native_id: ev.native_id.clone(),
                text: ev.text.clone(),
                platform_time: ev.msg_time,
                // Native receive fields do not currently provide a proven
                // OneBot real_id/time/peer projection.
                onebot_projection: None,
                observed_at_unix_ms: 0,
                source: match ev.source {
                    crate::resident::EventSourceKind::Recv => "recv",
                    crate::resident::EventSourceKind::Update => "update",
                }
                .into(),
            };
            let seq = wire_seq;
            let bytes = payload.native_id.len() + payload.text.len() + 128;
            while unacked.len() + 1 > EVENT_WINDOW_MAX_ITEMS
                || *unacked_bytes + bytes > EVENT_WINDOW_MAX_BYTES
            {
                if let Some((s, _)) = unacked.pop_front() {
                    if let Some((_, e)) = unacked.front() {
                        *unacked_bytes -= e.native_id.len() + e.text.len() + 128;
                    }
                    let _ = s;
                    c.events_gap_dropped += 1;
                }
            }
            unacked.push_back((seq, payload.clone()));
            *unacked_bytes += bytes;
            if send_json(conn, &BridgeMsg::Event { event: payload }).is_err() {
                return SessionEnd::Broken;
            }
            c.events_sent += 1;
        }
        // 2b) 重放未确认事件(新连接对账;窗口满 → 记 Gap 丢最旧)。
        // 恰一处重放(此前重复的两处会每轮双发同一窗口)。
        let replay: Vec<EventPayload> = unacked
            .iter()
            .filter(|(s, _)| *s > *acked_high)
            .map(|(_, e)| e.clone())
            .collect();
        for e in replay {
            if send_json(conn, &BridgeMsg::Event { event: e }).is_err() {
                return SessionEnd::Broken;
            }
            c.events_sent += 1;
        }
        // 3) 心跳(3 次未响应 → 主动断开重连;计划 §6.4)。
        if last_heartbeat.elapsed() >= Duration::from_millis(cfg.heartbeat_ms) {
            if send_json(
                conn,
                &BridgeMsg::Health {
                    ingress_pending: resident.pending_ingress(),
                    native_ops_total: resident.counters().native_ops_total,
                },
            )
            .is_err()
            {
                return SessionEnd::Broken;
            }
            c.heartbeats_sent += 1;
            heartbeat_misses += 1;
            if heartbeat_misses >= 3 {
                c.heartbeat_timeouts += 1;
                return SessionEnd::Broken;
            }
            last_heartbeat = Instant::now();
            // 心跳兼作事件轮询唤醒源(5s 节奏;D8 接收环排空依赖周期性泵)。
            wake();
        }
        // 4) 阻塞收(tick 周期返回)。
        match recv_json(conn, pipe, tick) {
            Ok(CoreToBridgeMsg::Dispatch {
                request_id,
                wire_seq,
                timeout_ms,
                target,
                text,
            }) => {
                c.dispatches += 1;
                // resident 受理只代表排队。NativeStarted 由 owner drain
                // 在 native 调用确已发起后通过有序 notice 通道上报。
                match resident.submit(crate::resident::OwnedRequest::SendText {
                    request_id: request_id.clone(),
                    wire_seq,
                    timeout_ms,
                    chat_type: chat_type_from_target(&target),
                    peer_uid: peer_uid_from_target(&target),
                    text: text.clone(),
                }) {
                    crate::resident::SubmitVerdict::Accepted => {
                        wake();
                    }
                    // resident 未就绪(owner 初始化重试中):挂待提交队列。
                    _ => pending.push_back((
                        request_id,
                        wire_seq,
                        timeout_ms,
                        chat_type_from_target(&target),
                        peer_uid_from_target(&target),
                        text.clone(),
                    )),
                }
            }
            Ok(CoreToBridgeMsg::EventAck {
                event_seq,
                suppressed: _,
            }) => {
                // daemon 回显 wire seq(首次持久化与重复抑制均如此 —— 修复
                // 抑制回显 0 导致窗口永不前移的重放风暴,D8 现场实证)。
                *acked_high = (*acked_high).max(event_seq);
                while unacked
                    .front()
                    .map(|(s, _)| *s <= *acked_high)
                    .unwrap_or(false)
                {
                    if let Some((_, e)) = unacked.pop_front() {
                        *unacked_bytes -= e.native_id.len() + e.text.len() + 128;
                        c.events_acked += 1;
                    }
                }
                heartbeat_misses = 0;
            }
            Ok(CoreToBridgeMsg::NativeStartedAck { request_id }) => {
                resident.ack_notice(&request_id, true);
                if let Some(index) = result_outbox.iter().position(|item| {
                    matches!(&item.notice, ResidentNotice::NativeStarted { request_id: id } if id == &request_id)
                }) {
                    result_outbox.remove(index);
                }
                heartbeat_misses = 0;
            }
            Ok(CoreToBridgeMsg::SendResultAck {
                request_id,
                terminal,
            }) => {
                resident.ack_result_notice(&request_id, terminal);
                if let Some(index) = result_outbox.iter().position(|item| {
                    matches!(&item.notice, ResidentNotice::Outcome(_))
                        && notice_request_id(&item.notice) == request_id
                }) {
                    result_outbox.remove(index);
                }
                heartbeat_misses = 0;
            }
            Ok(CoreToBridgeMsg::HealthAck {}) => {
                heartbeat_misses = 0;
            }
            Ok(CoreToBridgeMsg::Stopped {}) => {
                return SessionEnd::CoreStopped;
            }
            Ok(CoreToBridgeMsg::Reject { .. } | CoreToBridgeMsg::HelloAck { .. }) => {
                c.protocol_errors += 1;
            }
            Err(PipeError::TimedOut) => {
                tick.store(false, Ordering::Relaxed);
            }
            Err(_) => return SessionEnd::Broken,
        }
    }
}

/// target JSON → chat_type(1=private 2=group;opaque 透传)。
fn chat_type_from_target(v: &serde_json::Value) -> u32 {
    if v.get("kind").and_then(|x| x.as_str()) == Some("group") {
        2
    } else {
        1
    }
}

/// target JSON → peer_uid(会话 peer;opaque)。
fn peer_uid_from_target(v: &serde_json::Value) -> String {
    v.get("peer")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(msg: &CoreToBridgeMsg) -> Vec<u8> {
        encode_frame(&serde_json::to_vec(msg).unwrap()).unwrap()
    }

    fn hello_ack() -> CoreToBridgeMsg {
        CoreToBridgeMsg::HelloAck {
            protocol_version: 2,
            core_build: "lab".into(),
            connection_epoch: 1,
            accepted: true,
            reject_reason: None,
        }
    }

    /// T01(C3): 单次读合并 HelloAck+Dispatch+EventAck → 三帧按序全交付,
    /// 恰好一次。修复前 out.pop() 只返回最后一帧,前两帧被静默丢弃。
    #[test]
    fn coalesced_frames_delivered_in_order_once() {
        let mut pipe = FramePipe::new();
        let msgs = vec![
            hello_ack(),
            CoreToBridgeMsg::Dispatch {
                request_id: "R1".into(),
                wire_seq: 1,
                timeout_ms: 30_000,
                target: serde_json::json!({"account":"a","kind":"private","peer":"p"}),
                text: "hi".into(),
            },
            CoreToBridgeMsg::EventAck {
                event_seq: 7,
                suppressed: false,
            },
        ];
        let mut merged = Vec::new();
        for m in &msgs {
            merged.extend(framed(m));
        }
        pipe.feed(&merged).unwrap();
        for m in &msgs {
            assert_eq!(pipe.next_msg().unwrap().as_ref(), Some(m));
        }
        assert!(pipe.next_msg().unwrap().is_none(), "交付必须恰好一次");
    }

    /// 半帧跨 feed 保留前缀(T02 客户端侧;新连接不得复用旧前缀由
    /// FramePipe::new 的每连接构造保证)。
    #[test]
    fn partial_frame_prefix_survives_across_feeds() {
        let mut pipe = FramePipe::new();
        let full = framed(&CoreToBridgeMsg::HealthAck {});
        pipe.feed(&full[..5]).unwrap();
        assert!(pipe.next_msg().unwrap().is_none());
        pipe.feed(&full[5..]).unwrap();
        assert!(matches!(
            pipe.next_msg().unwrap(),
            Some(CoreToBridgeMsg::HealthAck {})
        ));
    }

    /// 魔数错位是致命错误(feed → BrokenPipe,状态不可信)。
    #[test]
    fn magic_mismatch_is_fatal() {
        let mut pipe = FramePipe::new();
        let garbage = b"NOT-A-FRAME!!".repeat(2);
        assert!(pipe.feed(&garbage).is_err());
    }
}
