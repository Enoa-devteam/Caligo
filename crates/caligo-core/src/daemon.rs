//! K4-D5 常驻 core 会话层:管道 server、双角色会话与账号 actor 的接线
//! (计划 §7-D5:`host harness → resident → pipe → runtime actor → journal
//! → test client`)。
//!
//! 线程模型:daemon 持有 `Mutex<AccountActor>`(单 owner 语义由互斥串行化);
//! bridge 与 control 各一条连接一个线程,请求/应答式 + 出站泵。
//! 停止:任一入口 Stop → 停止标志 → accept 取消 → actor.stop/close(Closed
//! 前在途项显式 unknown)。core 非正常退出由断连恢复(测试在 recovery_contract)。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::capability_broker::CapabilityBroker;
use crate::media_tool::MediaTool;
use crate::ipc::{
    encode_frame, validate_auth, validate_bridge_hello_v3, validate_hello_v3, BridgeMsg,
    ControlMsg, CoreToBridgeMsg, CoreToControlMsg, EventPayload, FrameDecoder, OutcomePayload,
    Role, BUSINESS_PROTOCOL_REVISION, PROTOCOL_VERSION_V3,
};
use crate::message_store::MessageStore;
use crate::onebot::{StoreReader, StoredMessageView};
use crate::onebot_events::EventBus;
use crate::runtime::{host_identity, AccountActor, NativeResult, RuntimeConfig};
use crate::transport::{
    connect_client, generate_token, token_hex, PipeConnection, PipeServer, TransportError,
};
use caligo_model::capability::{HostQuery, QueryOutcome, QueryScope};
use caligo_model::{
    Direction, EventSource, MsgEvent, NativeMessageId, SendTextRequest, SessionIdentity,
};

/// 会话帧读取错误。
#[derive(Debug)]
pub enum SessionError {
    Transport(TransportError),
    Frame(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
    Closed,
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SessionError::Transport(e) => write!(f, "transport: {e}"),
            SessionError::Frame(e) => write!(f, "frame: {e}"),
            SessionError::Io(e) => write!(f, "io: {e}"),
            SessionError::Serde(e) => write!(f, "json: {e}"),
            SessionError::Closed => write!(f, "session closed"),
        }
    }
}

/// 每连接长寿命帧流:增量 decoder + 完整帧 FIFO(C2/C3 修复)。
///
/// 合同:
/// - 唤醒/取消(`TimedOut`)不丢失已消费字节 —— 半帧前缀留在 decoder,
///   完整帧留在 FIFO,下一轮调用从原点继续(反例 T02);
/// - 单次读合并的多帧按序全交付一次,不丢不重(反例 T01);
/// - 新连接必须新建实例,不得复用旧连接前缀;
/// - 魔数/超长为致命错误:解码器状态不可信,调用方结束该连接。
struct FrameStream {
    dec: FrameDecoder,
    queue: VecDeque<Vec<u8>>,
}

impl FrameStream {
    fn new() -> Self {
        Self {
            dec: FrameDecoder::new(),
            queue: VecDeque::new(),
        }
    }

    fn recv_raw(
        &mut self,
        conn: &PipeConnection,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>, SessionError> {
        loop {
            if let Some(payload) = self.queue.pop_front() {
                return Ok(payload);
            }
            let mut buf = [0u8; 4096];
            match conn.read_some(&mut buf, Some(cancel)) {
                Ok(0) => return Err(SessionError::Transport(TransportError::BrokenPipe)),
                Ok(n) => {
                    let mut out = Vec::new();
                    self.dec
                        .push(&buf[..n], &mut out)
                        .map_err(|e| SessionError::Frame(e.to_string()))?;
                    self.queue.extend(out);
                }
                Err(TransportError::TimedOut) => {
                    return Err(SessionError::Transport(TransportError::TimedOut))
                }
                Err(e) => return Err(SessionError::Transport(e)),
            }
        }
    }

    fn recv_json<T: for<'de> Deserialize<'de>>(
        &mut self,
        conn: &PipeConnection,
        cancel: &AtomicBool,
    ) -> Result<T, SessionError> {
        let payload = self.recv_raw(conn, cancel)?;
        serde_json::from_slice(&payload).map_err(SessionError::Serde)
    }
}

fn send_json<T: Serialize>(conn: &PipeConnection, msg: &T) -> Result<(), SessionError> {
    let payload = serde_json::to_vec(msg).map_err(SessionError::Serde)?;
    let frame = encode_frame(&payload).map_err(|e| SessionError::Frame(e.to_string()))?;
    conn.write_all(&frame).map_err(SessionError::Transport)
}

/// daemon 配置。`expect_client_pid` 为指定进程校验(0 = 不校验)。
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub pipe_prefix: String,
    pub journal_path: PathBuf,
    pub module_baseline: String,
    pub account: String,
    pub core_build: String,
    pub expect_client_pid: u32,
    pub runtime: RuntimeConfig,
    /// LAB 故障注入(research):bridge Hello 受理后不读在途入站字节、
    /// 不发 Stopped 直接断开 —— 模拟"worker 已写入但 core 未消费即崩溃"
    /// 的窗口(T07 反例的确定性构造点)。仅触发一次;生产路径恒 false。
    pub test_fault_break_bridge_after_hello: bool,
    /// LAB 故障注入(research):SendResult 已由 actor/journal 接受但 ACK
    /// 尚未写回时断开;用于验证持久结果可重放且不会重新调用 native。
    pub test_fault_break_bridge_after_result: bool,
}

/// 常驻 core(LAB 形态;计划 §7-D5)。
pub struct Daemon {
    pub(crate) voice_media: Option<MediaTool>,
    pub config: DaemonConfig,
    auth_hex: std::sync::Mutex<String>,
    core_nonce: u64,
    pub(crate) actor: Mutex<AccountActor>,
    onebot_events: Mutex<EventBus>,
    onebot_http_post_events: Mutex<EventBus>,
    onebot_http_post_ordered: AtomicBool,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) epoch: AtomicU64,
    /// 待派发队列(control 受理后经此转 bridge)。
    pub(crate) dispatch_out: Mutex<Vec<CoreToBridgeMsg>>,
    pub(crate) bridge_hello_seen: AtomicBool,
    /// LAB 故障注入的一次性触发闸(与 config.test_fault_break_bridge_after_hello 配合)。
    test_fault_armed: AtomicBool,
    test_result_fault_armed: AtomicBool,
    /// bridge 会话读唤醒:control 推入派发后置位,使其阻塞读立即返回
    /// (read 以 100ms 粒度轮询该标志)。
    pub(crate) bridge_wake: Arc<AtomicBool>,
    api_request_seq: AtomicU64,
    query_broker: CapabilityBroker,
}

/// Every return path (including wire errors) releases ephemeral query waiters.
struct BridgeQuerySession<'a> {
    daemon: &'a Daemon,
    epoch: u64,
    open: Arc<AtomicBool>,
}

impl Drop for BridgeQuerySession<'_> {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Relaxed);
        self.daemon.query_broker.disconnect(self.epoch);
    }
}

impl Daemon {
    pub fn start(config: DaemonConfig) -> Result<Arc<Self>, crate::runtime::OpenError> {
        Self::start_with_voice_media(config, None)
    }

    pub fn start_with_voice_media(
        config: DaemonConfig,
        voice_media: Option<MediaTool>,
    ) -> Result<Arc<Self>, crate::runtime::OpenError> {
        let message_store = match config.account.parse::<u64>() {
            Ok(account_id) if account_id > 0 => {
                let mut path = config.journal_path.clone();
                path.set_extension("messages.jsonl");
                Some(
                    MessageStore::open(path, account_id, None)
                        .map_err(|e| crate::runtime::OpenError::MessageStore(e.to_string()))?,
                )
            }
            _ => None,
        };
        let (actor, _report) = match message_store {
            Some(store) => AccountActor::open_with_message_store(
                config.journal_path.clone(),
                config.runtime.clone(),
                store,
            )?,
            None => AccountActor::open(&config.journal_path, config.runtime.clone())?,
        };
        let token = generate_token().map_err(|_| {
            crate::runtime::OpenError::Journal(crate::journal::JournalError::Io(
                std::io::Error::new(std::io::ErrorKind::Other, "no os rng"),
            ))
        })?;
        Ok(Arc::new(Self {
            voice_media,
            auth_hex: std::sync::Mutex::new(token_hex(&token)),
            core_nonce: u64::from_le_bytes(token[0..8].try_into().unwrap()),
            actor: Mutex::new(actor),
            onebot_events: Mutex::new(EventBus::new()),
            onebot_http_post_events: Mutex::new(EventBus::new()),
            onebot_http_post_ordered: AtomicBool::new(false),
            stop: Arc::new(AtomicBool::new(false)),
            epoch: AtomicU64::new(1),
            dispatch_out: Mutex::new(Vec::new()),
            bridge_hello_seen: AtomicBool::new(false),
            test_fault_armed: AtomicBool::new(false),
            test_result_fault_armed: AtomicBool::new(false),
            bridge_wake: Arc::new(AtomicBool::new(false)),
            api_request_seq: AtomicU64::new(1),
            query_broker: CapabilityBroker::default(),
            config,
        }))
    }

    /// (bridge, control) 管道名。前缀可以是短名(自动补 `\.\pipe\`)
    /// 或完整路径 —— 规避跨 shell 传参的反斜杠转换问题。
    pub fn pipe_names(&self) -> (String, String) {
        // 前缀可以是短名(自动补 Win32 命名管道根)或完整路径;
        // 规避跨 shell 传参的反斜杠转换问题。
        const ROOT: &str = "\\\\.\\pipe\\";
        let raw = self.config.pipe_prefix.as_str();
        let prefix = if raw.starts_with(ROOT) {
            raw.to_string()
        } else {
            format!("{ROOT}{}", raw.trim_start_matches('\\'))
        };
        (format!("{prefix}-bridge"), format!("{prefix}-control"))
    }

    /// 认证凭据(启动时随机;token-file 模式下以文件内容为准)。
    pub fn auth_token(&self) -> String {
        self.auth_hex.lock().unwrap().clone()
    }

    /// token-file 模式:以文件内容为准(caligod 重启连续性;§6.4 私有通道)。
    pub fn set_auth_token(&self, token: &str) {
        *self.auth_hex.lock().unwrap() = token.to_string();
    }

    /// 停止标志(Ctrl+C 与 control Stop 汇合)。
    pub fn stop_flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.stop.clone()
    }

    /// 当前连接代次(观测/LAB;每条新 bridge 连接递增)。
    pub fn connection_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    /// Execute one fixed read-only query. This does not persist or retry it.
    pub fn query_host(&self, query: HostQuery, timeout: std::time::Duration) -> QueryOutcome {
        self.query_host_cancellable(query, timeout, &AtomicBool::new(false))
    }

    pub fn query_host_cancellable(
        &self,
        query: HostQuery,
        timeout: std::time::Duration,
        cancel: &AtomicBool,
    ) -> QueryOutcome {
        if self.stop.load(Ordering::Relaxed) {
            return QueryOutcome::Disconnected;
        }
        if cancel.load(Ordering::Relaxed) {
            return QueryOutcome::Rejected {
                code: -2,
                reason: "query cancelled".into(),
            };
        }
        if query.validate().is_err() {
            return QueryOutcome::Rejected {
                code: -1,
                reason: "invalid read-only query".into(),
            };
        }
        let Ok(sequence) =
            self.api_request_seq
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                    next.checked_add(1)
                })
        else {
            return QueryOutcome::NotReady;
        };
        let request_id = format!("query-{:016x}-{sequence}", self.core_nonce);
        let ticket = {
            let actor = self.actor.lock().unwrap();
            if actor.phase() != caligo_model::Phase::Active {
                return QueryOutcome::NotReady;
            }
            let (Some(session), Some(connection)) = (actor.session(), actor.connection()) else {
                return QueryOutcome::NotReady;
            };
            let scope = QueryScope {
                account: session.account.0.clone(),
                host_nonce: session.host.host_nonce,
                session_generation: session.session_generation,
                connection_epoch: connection.connection_epoch,
            };
            self.query_broker
                .begin(request_id, scope, query, timeout, &self.dispatch_out)
        };
        let ticket = match ticket {
            Ok(ticket) => ticket,
            Err(outcome) => return outcome,
        };
        self.bridge_wake.store(true, Ordering::Relaxed);
        // No actor, broker or dispatch mutex is held during the wait.
        ticket.wait(&self.stop, cancel)
    }

    pub fn pending_query_count(&self) -> usize {
        self.query_broker.pending_count()
    }
    pub fn stale_query_result_count(&self) -> u64 {
        self.query_broker.stale_result_count()
    }

    /// Whether the currently active, exactly bound bridge advertised private text send.
    pub fn supports_private_text(&self) -> bool {
        let Ok(actor) = self.actor.lock() else {
            return false;
        };
        if actor.phase() != caligo_model::Phase::Active {
            return false;
        }
        let (Some(session), Some(connection)) = (actor.session(), actor.connection()) else {
            return false;
        };
        let scope = QueryScope {
            account: session.account.0.clone(),
            host_nonce: session.host.host_nonce,
            session_generation: session.session_generation,
            connection_epoch: connection.connection_epoch,
        };
        self.query_broker.supports_private_text(&scope)
    }

    /// Submit one OneBot action through the same durable actor→bridge path as
    /// control clients. The request ID is generated once and never reused.
    pub fn submit_onebot_text(
        &self,
        target: caligo_model::SessionKey,
        text: String,
        deadline_ms: u64,
    ) -> Result<String, String> {
        let sequence = self
            .api_request_seq
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| "OneBot request ID sequence exhausted")?;
        let request_id = format!("onebot-{:016x}-{sequence}", self.core_nonce);
        let mut actor = self
            .actor
            .lock()
            .map_err(|_| "account actor lock poisoned")?;
        let session = actor
            .session()
            .cloned()
            .ok_or_else(|| "no active QQ session".to_string())?;
        let mut request = SendTextRequest {
            request_id: request_id.clone(),
            session_identity: session,
            target,
            text,
            payload_hash: 0,
            deadline_ms,
        };
        request.payload_hash = SendTextRequest::compute_payload_hash(
            request.session_identity.session_generation,
            &request.target,
            &request.text,
            deadline_ms,
        );
        actor
            .submit_send(request.clone())
            .map_err(|error| format!("send request rejected: {error:?}"))?;
        let wire_seq = actor
            .dispatch_begin(&request_id)
            .map_err(|error| format!("dispatch preparation failed: {error:?}"))?;
        let timeout_ms = actor.remaining_timeout_ms(&request_id).unwrap_or_default();
        self.dispatch_out
            .lock()
            .map_err(|_| "dispatch queue lock poisoned")?
            .push(CoreToBridgeMsg::Dispatch {
                request_id: request_id.clone(),
                wire_seq,
                timeout_ms,
                target: serde_json::json!({
                    "account": request.target.account.0,
                    "kind": request.target.kind.to_string(),
                    "peer": request.target.peer.0,
                }),
                text: request.text,
            });
        self.bridge_wake.store(true, Ordering::Relaxed);
        Ok(request_id)
    }

    /// State for the same OneBot request, plus a durable public ID if its
    /// matching projected event has reached MessageStore.
    pub fn onebot_text_status(
        &self,
        request_id: &str,
        target: &caligo_model::SessionKey,
    ) -> Option<(caligo_model::ActionState, Option<i32>)> {
        let actor = self.actor.lock().ok()?;
        let state = actor.query_request(request_id)?;
        let message_id = match &state {
            caligo_model::ActionState::ConfirmedSuccess { native_id } => {
                actor.onebot_message_id(target, &native_id.0)
            }
            _ => None,
        };
        Some((state, message_id))
    }

    /// Subscribe one independent bounded transport queue to accepted OneBot
    /// message events. Transports never drain the actor's durable IPC cursor.
    pub fn subscribe_onebot_events(&self, name: &str) -> (u64, std::sync::mpsc::Receiver<String>) {
        self.onebot_events.lock().unwrap().subscribe(name)
    }

    /// Enable the OneBot V11 ordering rule: finish HTTP POST event handling
    /// before forwarding that event to WebSocket event subscribers.
    pub fn enable_onebot_http_post_ordering(&self) {
        self.onebot_http_post_ordered.store(true, Ordering::Relaxed);
    }

    pub fn subscribe_onebot_http_post_events(
        &self,
        name: &str,
    ) -> (u64, std::sync::mpsc::Receiver<String>) {
        self.onebot_http_post_events.lock().unwrap().subscribe(name)
    }

    pub fn unsubscribe_onebot_http_post_events(&self, id: u64) -> bool {
        self.onebot_http_post_events.lock().unwrap().unsubscribe(id)
    }

    pub fn unsubscribe_onebot_events(&self, id: u64) -> bool {
        self.onebot_events.lock().unwrap().unsubscribe(id)
    }

    /// Observe queue loss without consuming the transport's events.
    pub fn onebot_event_subscriber_dropped(&self, id: u64) -> Option<u64> {
        self.onebot_events.lock().unwrap().subscriber_dropped(id)
    }

    pub(crate) fn publish_onebot_event_json(&self, event_json: &str) {
        if self.onebot_http_post_ordered.load(Ordering::Relaxed) {
            let _ = self
                .onebot_http_post_events
                .lock()
                .unwrap()
                .publish(event_json);
        } else {
            self.publish_onebot_ws_event_json(event_json);
        }
    }

    pub(crate) fn publish_onebot_ws_event_json(&self, event_json: &str) {
        let _ = self.onebot_events.lock().unwrap().publish(event_json);
    }

    fn publish_onebot_message(&self, mut event: MsgEvent) {
        if event.direction != Direction::Incoming || event.source != EventSource::Recv {
            return;
        }
        let message_id = {
            let actor = self.actor.lock().unwrap();
            if event.wrapper_message.is_some() {
                let Some(projection) = actor.wrapper_message_projection(&event) else {
                    return;
                };
                let message_id = projection.real_id;
                event.onebot_projection = Some(projection);
                message_id
            } else {
                let Some(message_id) = actor.onebot_message_id(&event.session, &event.native_id.0)
                else {
                    return;
                };
                message_id
            }
        };
        let Some(json) = self.config.account.parse::<u64>().ok().and_then(|self_id| {
            crate::onebot_events::message_event_json(self_id, message_id, &event)
        }) else {
            return;
        };
        self.publish_onebot_event_json(&json);
    }

    /// 阻塞运行:双 accept 循环;停止后收尾 actor(stop→close)。
    pub fn run(self: &Arc<Self>) -> Result<(), SessionError> {
        let (bridge_name, control_name) = self.pipe_names();
        eprintln!("[daemon] creating bridge pipe: {bridge_name}");
        let bridge_srv = PipeServer::create(&bridge_name).map_err(|e| {
            eprintln!("[daemon] create {bridge_name}: {e}");
            SessionError::Transport(e)
        })?;
        eprintln!("[daemon] bridge created ok; creating control pipe: {control_name}");
        let control_srv = PipeServer::create(&control_name).map_err(|e| {
            eprintln!("[daemon] create {control_name}: {e}");
            SessionError::Transport(e)
        })?;
        eprintln!("[daemon] both pipes created; spawning accept loops");
        let t_bridge = {
            let d = self.clone();
            std::thread::spawn(move || d.accept_loop(&bridge_srv, Role::Bridge))
        };
        let t_control = {
            let d = self.clone();
            std::thread::spawn(move || d.accept_loop(&control_srv, Role::Control))
        };
        let r1 = t_bridge.join().expect("bridge accept thread");
        let r2 = t_control.join().expect("control accept thread");
        // 收尾:取消未执行 → 在途显式 unknown → Closed。
        let mut actor = self.actor.lock().unwrap();
        let _ = actor.stop();
        let _ = actor.close();
        r1.and(r2)
    }

    fn accept_loop(self: &Arc<Self>, srv: &PipeServer, role: Role) -> Result<(), SessionError> {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let conn = match srv.accept(&self.stop) {
                Ok(c) => c,
                Err(TransportError::TimedOut) => continue,
                Err(e) => return Err(SessionError::Transport(e)),
            };
            // PID 校验:仅 bridge 连接(指定实例判据)。控制面客户端是
            // 本机的测试工具/执行者进程,凭 token 认证,不校验 PID。
            if role == Role::Bridge && self.config.expect_client_pid != 0 {
                match conn.client_pid() {
                    Ok(pid) if pid == self.config.expect_client_pid => {}
                    Ok(pid) => {
                        let err = SessionError::Frame(format!(
                            "bridge client pid {pid} != designated {}",
                            self.config.expect_client_pid
                        ));
                        eprintln!("[daemon] {err}");
                        // 仅拒此连接,不杀 accept 线程(否则控制面同被拖死)。
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }
                    Err(e) => return Err(SessionError::Transport(e)),
                }
            }
            let serve_r = match role {
                Role::Bridge => self.serve_bridge(conn),
                Role::Control => self.serve_control(conn),
            };
            if let Err(e) = &serve_r {
                eprintln!("[daemon] {role:?} session error: {e}");
            }
            serve_r?;
            // 排水窗:最终应答(Stopped/拒绝 ack)需要客户端先读走;
            // DisconnectNamedPipe 会丢弃未读数据,立即断开会吞掉收尾应答。
            std::thread::sleep(std::time::Duration::from_millis(200));
            srv.disconnect();
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
    }

    fn hello_ack(
        accepted: bool,
        reject: Option<String>,
        build: &str,
        epoch: u64,
    ) -> serde_json::Value {
        serde_json::json!({
            "t": "hello_ack",
            "d": {
                "protocol_version": PROTOCOL_VERSION_V3,
                "core_build": build,
                "connection_epoch": epoch,
                "accepted": accepted,
                "reject_reason": reject,
            }
        })
    }

    fn serve_bridge(self: &Arc<Self>, conn: PipeConnection) -> Result<(), SessionError> {
        // 每连接独立帧流:半帧前缀不跨连接(见 FrameStream 合同)。
        let mut stream = FrameStream::new();
        // 连接代次:每条新 bridge 连接分配新 epoch(C1);旧连接不得以
        // 相同/更小 epoch 触发重连恢复。
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        self.query_broker.reset(epoch);
        let session_open = Arc::new(AtomicBool::new(true));
        let _query_session = BridgeQuerySession {
            daemon: self,
            epoch,
            open: session_open.clone(),
        };
        // 握手:版本 → 认证 → 角色 → 基线/账号/代次。
        let hello: BridgeMsg = stream.recv_json(&conn, &self.stop)?;
        let BridgeMsg::Hello {
            protocol_version,
            business_protocol_revision,
            bridge_build,
            auth_token,
            role,
            session_generation,
            account,
            module_baseline,
            host_nonce,
            host_process_created_utc,
        } = hello
        else {
            send_json(&conn, &Self::reject_val("first message must be hello"))?;
            return Ok(());
        };
        let reject = |reason: String| {
            let _ = send_json(
                &conn,
                &Self::hello_ack(false, Some(reason), &self.config.core_build, epoch),
            );
        };
        if let Err(e) = validate_bridge_hello_v3(
            protocol_version,
            business_protocol_revision,
            role,
            Role::Bridge,
            &self.config.core_build,
        ) {
            reject(e.to_string());
            return Ok(());
        }
        let expected = self.auth_hex.lock().unwrap().clone();
        if !validate_auth(&auth_token, &expected) {
            reject("auth failed".into());
            return Ok(());
        }
        if module_baseline != self.config.module_baseline {
            reject(format!("baseline mismatch: {module_baseline}"));
            return Ok(());
        }
        if account != self.config.account {
            reject(format!("account mismatch: {account}"));
            return Ok(());
        }
        // 会话绑定(首次 bridge 接入 = Bound→Active)。accepted 只在完整
        // 绑定/恢复成功后返回(C1:重连失败必须拒绝,不得吞错后照常受理)。
        {
            let mut actor = self.actor.lock().unwrap();
            // C9:宿主身份来自 bridge Hello(nonce + 创建时间 + OS PID),
            // 不再由 core 代填;daemon 只核对,不发明。
            let host = host_identity(
                conn.client_pid().unwrap_or(0),
                &host_process_created_utc,
                &module_baseline,
                &bridge_build,
                host_nonce,
            );
            let session = SessionIdentity {
                host,
                account: caligo_model::AccountId(account.clone()),
                session_generation,
            };
            if let Err(e) = actor.attach_session(session) {
                // 已绑定同代次且同宿主 → 幂等(重连场景);宿主实例不同
                // (QQ 重启/换宿主)必须显式拒绝,不得沿用旧绑定(T05)。
                let same = actor
                    .session()
                    .map(|s| {
                        s.session_generation == session_generation
                            && s.account.0 == account
                            && s.host.host_nonce == host_nonce
                            && s.host.pid == conn.client_pid().unwrap_or(0)
                            && s.host.process_created_utc == host_process_created_utc
                    })
                    .unwrap_or(false);
                if !same {
                    reject(format!("attach rejected: {e:?}"));
                    return Ok(());
                }
                if let Err(e) = actor.reconnect(self.conn_identity(epoch)) {
                    // 同代次重连但连接代次不前进(如旧连接重放)→ 拒绝。
                    reject(format!("reconnect rejected: {e:?}"));
                    return Ok(());
                }
            } else if let Err(e) = actor.mark_connection_ready(self.conn_identity(epoch)) {
                reject(format!("connection rejected: {e:?}"));
                return Ok(());
            }
        }
        self.query_broker.bind(QueryScope {
            account: account.clone(),
            host_nonce,
            session_generation,
            connection_epoch: epoch,
        });
        send_json(
            &conn,
            &Self::hello_ack(true, None, &self.config.core_build, epoch),
        )?;
        self.bridge_hello_seen.store(true, Ordering::Relaxed);

        // LAB 故障注入(T07):受理后不给 Stopped、不读在途入站字节直接断开。
        // worker 在 HelloAck 之后写入的事件停留在管道缓冲,随断开被丢弃 ——
        // worker 侧必须靠跨连接未确认窗口在新连接上重放对账。
        if self.config.test_fault_break_bridge_after_hello
            && !self.test_fault_armed.swap(true, Ordering::Relaxed)
        {
            std::thread::sleep(std::time::Duration::from_millis(300));
            {
                let mut actor = self.actor.lock().unwrap();
                actor.degrade("lab fault: break after hello");
            }
            return Ok(());
        }

        // 组合唤醒观察者:stop 或 dispatch 到达都会打断 bridge 会话的阻塞读
        // (read 以 50ms 粒度轮询 recv_wake)。观察者随会话结束退出。
        let recv_wake = Arc::new(AtomicBool::new(false));
        {
            let s_open = session_open.clone();
            let s_stop = self.stop.clone();
            let s_dispatch = self.bridge_wake.clone();
            let s_wake = recv_wake.clone();
            std::thread::spawn(move || loop {
                if !s_open.load(Ordering::Relaxed) {
                    return;
                }
                if s_stop.load(Ordering::Relaxed) || s_dispatch.load(Ordering::Relaxed) {
                    s_wake.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            });
        }

        // 会话循环:泵出站(Dispatch)→ 阻塞等入站;派发/停止经 recv_wake
        // 唤醒阻塞读,消除"泵永不到来"的互等。
        loop {
            if self.stop.load(Ordering::Relaxed) {
                let _ = send_json(&conn, &CoreToBridgeMsg::Stopped {});
                return Ok(());
            }
            // 先清除唤醒,再泵出站:control 之后推入的派发会重新置位
            // bridge_wake(观察者随之置位 recv_wake),阻塞读最多 50ms 内返回。
            recv_wake.store(false, Ordering::Relaxed);
            let pending: Vec<CoreToBridgeMsg> =
                std::mem::take(&mut *self.dispatch_out.lock().unwrap());
            for mut m in pending {
                if let CoreToBridgeMsg::Query {
                    request_id,
                    scope,
                    timeout_ms,
                    ..
                } = &mut m
                {
                    let Some(remaining) =
                        self.query_broker.dispatch_timeout(epoch, request_id, scope)
                    else {
                        continue;
                    };
                    *timeout_ms = remaining;
                }
                send_json(&conn, &m)?;
            }
            match stream.recv_json::<BridgeMsg>(&conn, &recv_wake) {
                Ok(msg) => match msg {
                    BridgeMsg::QueryCapabilities {
                        revision,
                        scope,
                        capabilities,
                    } => {
                        if !self
                            .query_broker
                            .advertise(epoch, revision, &scope, capabilities)
                        {
                            send_json(
                                &conn,
                                &CoreToBridgeMsg::Reject {
                                    reason: "invalid query capability advertisement".into(),
                                },
                            )?;
                        }
                    }
                    BridgeMsg::SendCapabilities {
                        scope,
                        private_text,
                        group_text,
                    } => {
                        // A wrong-scope advertisement is ignored and cannot grant authority.
                        let _ = self.query_broker.advertise_send(
                            epoch,
                            &scope,
                            private_text,
                            group_text,
                        );
                    }
                    BridgeMsg::QueryResult {
                        request_id,
                        scope,
                        outcome,
                    } => {
                        self.query_broker
                            .complete(epoch, &request_id, &scope, outcome);
                    }
                    BridgeMsg::NativeStarted { request_id } => {
                        let mut actor = self.actor.lock().unwrap();
                        let applied = actor.bridge_started(&request_id).is_ok();
                        let already_applied = actor
                            .query_request(&request_id)
                            .map(|state| {
                                matches!(
                                    state,
                                    caligo_model::ActionState::NativeStarted
                                        | caligo_model::ActionState::ConfirmedSuccess { .. }
                                        | caligo_model::ActionState::AcceptedUnconfirmed
                                        | caligo_model::ActionState::ConfirmedFailure { .. }
                                        | caligo_model::ActionState::DeliveryUnknown
                                )
                            })
                            .unwrap_or(false);
                        drop(actor);
                        if applied || already_applied {
                            send_json(&conn, &CoreToBridgeMsg::NativeStartedAck { request_id })?;
                        }
                    }
                    BridgeMsg::SendResult {
                        request_id,
                        outcome,
                    } => {
                        let terminal = !matches!(&outcome, OutcomePayload::AcceptedUnconfirmed);
                        let result = match outcome {
                            OutcomePayload::Success { native_id } if !native_id.is_empty() => {
                                NativeResult::Success {
                                    native_id: NativeMessageId(native_id),
                                }
                            }
                            OutcomePayload::Success { .. }
                            | OutcomePayload::AcceptedUnconfirmed => {
                                NativeResult::AcceptedUnconfirmed
                            }
                            OutcomePayload::Failure { reason } => NativeResult::Failure { reason },
                            OutcomePayload::Unknown { reason } => NativeResult::Unknown { reason },
                        };
                        let applied = {
                            let mut actor = self.actor.lock().unwrap();
                            actor.bridge_result(&request_id, result).is_ok()
                        };
                        if applied {
                            if self.config.test_fault_break_bridge_after_result
                                && !self.test_result_fault_armed.swap(true, Ordering::Relaxed)
                            {
                                self.actor
                                    .lock()
                                    .unwrap()
                                    .degrade("lab fault: break after durable result before ack");
                                session_open.store(false, Ordering::Relaxed);
                                return Ok(());
                            }
                            send_json(
                                &conn,
                                &CoreToBridgeMsg::SendResultAck {
                                    request_id,
                                    terminal,
                                },
                            )?;
                        }
                    }
                    BridgeMsg::Event { event } => {
                        // wire seq = worker 本地序号(EventAck 的回显键;窗口
                        // 前移以此为准 —— 抑制回显 0 曾致重放风暴,D8 现场)。
                        let wire_seq = event.event_seq;
                        let accepted_event = self.to_event(event);
                        let receipt = self.actor.lock().unwrap().on_event(accepted_event.clone());
                        if matches!(&receipt, Ok(crate::runtime::EventReceipt::Published { .. })) {
                            // The actor journal and MessageStore append have both
                            // succeeded before any network transport sees it.
                            self.publish_onebot_message(accepted_event);
                        }
                        let (seq, suppressed) = match receipt {
                            Ok(r) => match r {
                                crate::runtime::EventReceipt::Published { .. } => (wire_seq, false),
                                crate::runtime::EventReceipt::DuplicateSuppressed => {
                                    (wire_seq, true)
                                }
                                crate::runtime::EventReceipt::AuditedOldGeneration { .. } => {
                                    (wire_seq, false)
                                }
                            },
                            Err(reason) => {
                                eprintln!("[daemon] event not durably accepted; leave it unacked: {reason:?}");
                                // The resident retains this event until EventAck.
                                // End this connection without an ACK so the replay
                                // path can retry a failed MessageStore append.
                                session_open.store(false, Ordering::Relaxed);
                                return Ok(());
                            }
                        };
                        send_json(
                            &conn,
                            &CoreToBridgeMsg::EventAck {
                                event_seq: seq,
                                suppressed,
                            },
                        )?;
                    }
                    BridgeMsg::Health { .. } => {
                        send_json(&conn, &CoreToBridgeMsg::HealthAck {})?;
                    }
                    BridgeMsg::Stop {} => {
                        let _ = send_json(&conn, &CoreToBridgeMsg::Stopped {});
                        session_open.store(false, Ordering::Relaxed);
                        return Ok(());
                    }
                    BridgeMsg::Hello { .. } => {
                        send_json(
                            &conn,
                            &CoreToBridgeMsg::Reject {
                                reason: "duplicate hello".into(),
                            },
                        )?;
                    }
                },
                Err(SessionError::Transport(TransportError::TimedOut)) => {
                    if self.stop.load(Ordering::Relaxed) {
                        let _ = send_json(&conn, &CoreToBridgeMsg::Stopped {});
                        session_open.store(false, Ordering::Relaxed);
                        return Ok(());
                    }
                    continue;
                }
                Err(SessionError::Transport(TransportError::BrokenPipe)) => {
                    // bridge 断连:停止派发准入;允许重连(recovery 契约)。
                    let mut actor = self.actor.lock().unwrap();
                    actor.degrade("bridge disconnected");
                    session_open.store(false, Ordering::Relaxed);
                    return Ok(());
                }
                Err(e) => {
                    session_open.store(false, Ordering::Relaxed);
                    return Err(e);
                }
            }
        }
    }

    fn serve_control(self: &Arc<Self>, conn: PipeConnection) -> Result<(), SessionError> {
        let mut stream = FrameStream::new();
        let hello: ControlMsg = stream.recv_json(&conn, &self.stop)?;
        let ControlMsg::Hello {
            protocol_version,
            auth_token,
            role,
            ..
        } = hello
        else {
            let _ = send_json(
                &conn,
                &CoreToControlMsg::Reject {
                    reason: "first message must be hello".into(),
                },
            );
            return Ok(());
        };
        let epoch = self.epoch.load(Ordering::Relaxed);
        let ack = |accepted: bool,
                   reject: Option<String>,
                   conn: &PipeConnection|
         -> Result<(), SessionError> {
            send_json(
                conn,
                &CoreToControlMsg::HelloAck {
                    protocol_version: PROTOCOL_VERSION_V3,
                    core_build: self.config.core_build.clone(),
                    connection_epoch: epoch,
                    accepted,
                    reject_reason: reject,
                },
            )
        };
        if let Err(e) = validate_hello_v3(
            protocol_version,
            role,
            Role::Control,
            &self.config.core_build,
        ) {
            ack(false, Some(e.to_string()), &conn)?;
            return Ok(());
        }
        let expected = self.auth_hex.lock().unwrap().clone();
        if !validate_auth(&auth_token, &expected) {
            ack(false, Some("auth failed".into()), &conn)?;
            return Ok(());
        }
        ack(true, None, &conn)?;

        loop {
            if self.stop.load(Ordering::Relaxed) {
                let _ = send_json(&conn, &CoreToControlMsg::Stopped {});
                return Ok(());
            }
            match stream.recv_json::<ControlMsg>(&conn, &self.stop) {
                Ok(msg) => match msg {
                    ControlMsg::SendText {
                        request_id,
                        target,
                        text,
                        deadline_ms,
                    } => {
                        let mut actor = self.actor.lock().unwrap();
                        let Some(session) = actor.session().cloned() else {
                            send_json(
                                &conn,
                                &CoreToControlMsg::Reject {
                                    reason: "no active session".into(),
                                },
                            )?;
                            continue;
                        };
                        let req = SendTextRequest {
                            request_id: request_id.clone(),
                            session_identity: session,
                            target: caligo_target(&target),
                            text,
                            payload_hash: 0,
                            deadline_ms,
                        };
                        let req = SendTextRequest {
                            payload_hash: SendTextRequest::compute_payload_hash(
                                req.session_identity.session_generation,
                                &req.target,
                                &req.text,
                                deadline_ms,
                            ),
                            ..req
                        };
                        match actor.submit_send(req.clone()) {
                            Ok(state) => {
                                // 受理即派发(dispatch_intent 窗口显式化)。
                                if let Ok(wire_seq) = actor.dispatch_begin(&request_id) {
                                    let timeout_ms =
                                        actor.remaining_timeout_ms(&request_id).unwrap_or_default();
                                    self.dispatch_out.lock().unwrap().push(
                                        CoreToBridgeMsg::Dispatch {
                                            request_id: request_id.clone(),
                                            wire_seq,
                                            timeout_ms,
                                            target,
                                            text: req.text,
                                        },
                                    );
                                    self.bridge_wake.store(true, Ordering::Relaxed);
                                }
                                send_json(
                                    &conn,
                                    &CoreToControlMsg::SendAccepted {
                                        request_id,
                                        state: format!("{state:?}"),
                                    },
                                )?;
                            }
                            Err(e) => {
                                send_json(
                                    &conn,
                                    &CoreToControlMsg::Reject {
                                        reason: format!("{e:?}"),
                                    },
                                )?;
                            }
                        }
                    }
                    ControlMsg::QueryRequest { request_id } => {
                        let actor = self.actor.lock().unwrap();
                        let (state, native_id) = match actor.request_summary(&request_id) {
                            Some(s) => (
                                Some(format!("{:?}", s.state)),
                                native_of_summary(&actor, &request_id),
                            ),
                            None => (None, None),
                        };
                        send_json(
                            &conn,
                            &CoreToControlMsg::QueryResult {
                                request_id,
                                state,
                                native_id,
                            },
                        )?;
                    }
                    ControlMsg::CancelQueued { request_id } => {
                        let mut actor = self.actor.lock().unwrap();
                        let verdict = match actor.cancel_queued(&request_id) {
                            Ok(v) => match v {
                                crate::runtime::CancelOutcome::CancelledUnsent => {
                                    "cancelled_unsent"
                                }
                                crate::runtime::CancelOutcome::TooLate => "too_late",
                                crate::runtime::CancelOutcome::NotFound => "not_found",
                            },
                            Err(_) => "rejected",
                        };
                        send_json(
                            &conn,
                            &CoreToControlMsg::CancelResult {
                                request_id,
                                verdict: verdict.into(),
                            },
                        )?;
                    }
                    ControlMsg::DrainEvents { max } => {
                        let mut actor = self.actor.lock().unwrap();
                        let events: Vec<EventPayload> = actor
                            .drain_deliverables(max)
                            .into_iter()
                            .map(daemon_event_payload)
                            .collect();
                        send_json(&conn, &CoreToControlMsg::EventsBatch { events })?;
                    }
                    ControlMsg::Health {} => {
                        send_json(&conn, &CoreToControlMsg::HealthAck {})?;
                    }
                    ControlMsg::Stop {} => {
                        self.stop.store(true, Ordering::Relaxed);
                        self.query_broker.disconnect(self.connection_epoch());
                        send_json(&conn, &CoreToControlMsg::Stopped {})?;
                        return Ok(());
                    }
                    ControlMsg::Hello { .. } => {
                        send_json(
                            &conn,
                            &CoreToControlMsg::Reject {
                                reason: "duplicate hello".into(),
                            },
                        )?;
                    }
                },
                Err(SessionError::Transport(TransportError::TimedOut)) => continue,
                Err(SessionError::Transport(TransportError::BrokenPipe)) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    fn conn_identity(&self, epoch: u64) -> caligo_model::ConnectionIdentity {
        caligo_model::ConnectionIdentity {
            core_instance_nonce: self.core_nonce,
            connection_epoch: epoch,
            protocol_version: PROTOCOL_VERSION_V3,
        }
    }

    fn to_event(&self, p: EventPayload) -> MsgEvent {
        let session = caligo_target(&p.session);
        MsgEvent {
            event_seq: 0,
            session_generation: p.session_generation,
            session,
            direction: match p.direction.as_str() {
                "self_sent" => Direction::SelfSent,
                _ => Direction::Incoming,
            },
            sender: p.sender,
            native_id: NativeMessageId(p.native_id),
            text: p.text,
            platform_time: p.platform_time,
            wrapper_message: p.wrapper_message,
            onebot_projection: p.onebot_projection,
            observed_at_unix_ms: p.observed_at_unix_ms,
            source: match p.source.as_str() {
                "update" => EventSource::Update,
                "history_backfill" => EventSource::HistoryBackfill,
                _ => EventSource::Recv,
            },
        }
    }

    fn reject_val(reason: &str) -> serde_json::Value {
        serde_json::json!({ "t": "reject", "d": { "reason": reason } })
    }
}

impl StoreReader for Daemon {
    fn voice_info(&self, file: &str) -> Option<caligo_model::capability::VoiceInfo> {
        self.actor.lock().ok()?.voice_info(file)
    }

    fn get_msg(&self, message_id: i32) -> Option<StoredMessageView> {
        self.actor.lock().ok()?.get_msg(message_id)
    }

    fn login_info(&self) -> Option<(u64, String)> {
        self.actor.lock().ok()?.login_info()
    }

    fn online_status(&self) -> Option<bool> {
        self.actor.lock().ok()?.online_status()
    }

    fn group_peers(&self) -> Vec<String> {
        self.actor
            .lock()
            .map(|a| a.group_peers())
            .unwrap_or_default()
    }
}

fn native_of_summary(actor: &AccountActor, id: &str) -> Option<String> {
    match actor.query_request(id) {
        Some(caligo_model::ActionState::ConfirmedSuccess { native_id }) => Some(native_id.0),
        _ => None,
    }
}

fn daemon_event_payload(ev: MsgEvent) -> EventPayload {
    EventPayload {
        event_seq: ev.event_seq,
        session_generation: ev.session_generation,
        session: serde_json::json!({
            "account": ev.session.account.0,
            "kind": ev.session.kind.to_string(),
            "peer": ev.session.peer.0,
        }),
        direction: match ev.direction {
            Direction::Incoming => "incoming".into(),
            Direction::SelfSent => "self_sent".into(),
        },
        sender: ev.sender,
        native_id: ev.native_id.0,
        text: ev.text,
        platform_time: ev.platform_time,
        wrapper_message: ev.wrapper_message,
        onebot_projection: ev.onebot_projection,
        observed_at_unix_ms: ev.observed_at_unix_ms,
        source: match ev.source {
            EventSource::Recv => "recv".into(),
            EventSource::Update => "update".into(),
            EventSource::HistoryBackfill => "history_backfill".into(),
        },
    }
}

fn caligo_target(v: &serde_json::Value) -> caligo_model::SessionKey {
    let account = v
        .get("account")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let peer = v
        .get("peer")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let kind = match v.get("kind").and_then(|x| x.as_str()) {
        Some("group") => caligo_model::SessionKind::Group,
        _ => caligo_model::SessionKind::Private,
    };
    caligo_model::SessionKey::new(account, kind, peer)
}

/// 控制客户端(LAB 测试用):连接、握手与请求/应答。
pub struct ControlClient {
    pub conn: PipeConnection,
    stream: FrameStream,
}

impl ControlClient {
    pub fn connect(name: &str, auth_token: &str, client_build: &str) -> Result<Self, SessionError> {
        let conn = connect_client(name).map_err(SessionError::Transport)?;
        send_json(
            &conn,
            &ControlMsg::Hello {
                protocol_version: PROTOCOL_VERSION_V3,
                client_build: client_build.to_string(),
                auth_token: auth_token.to_string(),
                role: Role::Control,
            },
        )?;
        // 消费并核对 HelloAck(否则它会被当作第一条请求的应答)。
        let mut stream = FrameStream::new();
        match stream.recv_json::<CoreToControlMsg>(&conn, &AtomicBool::new(false))? {
            CoreToControlMsg::HelloAck {
                accepted,
                reject_reason,
                ..
            } => {
                if !accepted {
                    return Err(SessionError::Frame(format!(
                        "hello rejected: {}",
                        reject_reason.unwrap_or_default()
                    )));
                }
            }
            other => {
                return Err(SessionError::Frame(format!(
                    "expected hello_ack, got {other:?}"
                )));
            }
        }
        Ok(Self { conn, stream })
    }

    pub fn request(&mut self, msg: ControlMsg) -> Result<CoreToControlMsg, SessionError> {
        send_json(&self.conn, &msg)?;
        self.stream.recv_json(&self.conn, &AtomicBool::new(false))
    }
}

/// bridge 侧 LAB 客户端(真机由 resident+worker 驱动;测试用同一管道语义)。
pub struct BridgeClient {
    pub conn: PipeConnection,
    stream: FrameStream,
}

impl BridgeClient {
    /// `host_nonce`/`host_created_utc`:宿主实例身份(v3;LAB 显式提供)。
    pub fn connect(
        name: &str,
        auth_token: &str,
        bridge_build: &str,
        session_generation: u64,
        account: &str,
        module_baseline: &str,
        host_nonce: u64,
        host_created_utc: &str,
    ) -> Result<Self, SessionError> {
        let conn = connect_client(name).map_err(SessionError::Transport)?;
        send_json(
            &conn,
            &BridgeMsg::Hello {
                protocol_version: PROTOCOL_VERSION_V3,
                business_protocol_revision: BUSINESS_PROTOCOL_REVISION,
                bridge_build: bridge_build.to_string(),
                auth_token: auth_token.to_string(),
                role: Role::Bridge,
                session_generation,
                account: account.to_string(),
                module_baseline: module_baseline.to_string(),
                host_nonce,
                host_process_created_utc: host_created_utc.to_string(),
            },
        )?;
        // 消费并核对 HelloAck。
        let mut stream = FrameStream::new();
        match stream.recv_json::<CoreToBridgeMsg>(&conn, &AtomicBool::new(false))? {
            CoreToBridgeMsg::HelloAck {
                accepted,
                reject_reason,
                ..
            } => {
                if !accepted {
                    return Err(SessionError::Frame(format!(
                        "hello rejected: {}",
                        reject_reason.unwrap_or_default()
                    )));
                }
            }
            other => {
                return Err(SessionError::Frame(format!(
                    "expected hello_ack, got {other:?}"
                )));
            }
        }
        Ok(Self { conn, stream })
    }

    pub fn send(&self, msg: &BridgeMsg) -> Result<(), SessionError> {
        send_json(&self.conn, msg)
    }

    pub fn recv(&mut self) -> Result<CoreToBridgeMsg, SessionError> {
        self.stream.recv_json(&self.conn, &AtomicBool::new(false))
    }
}
