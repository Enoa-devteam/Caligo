//! Normal-build OneBot HTTP assembly for caligod.
//!
//! This layer translates a standard OneBot action into exactly one durable
//! AccountActor dispatch and waits for both a strong native receipt and the
//! matching persisted OneBot projection before reporting send success.

use crate::daemon::Daemon;
use crate::media_tool::{MediaTool, StopToken};
use crate::onebot::{handle_action_with_queries, QueryBackend};
use crate::onebot_http::{serve_parallel, DEFAULT_MAX_BODY};
use caligo_model::capability::{HostQuery, QueryData, QueryOutcome};
use caligo_model::onebot_traits::{SendDisposition, SendPayload, SendSink, SendTarget};
use caligo_model::{ActionState, SessionKey, SessionKind};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SEND_DEADLINE_MS: u64 = 30_000;
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Send adapter shared by all transports. Private targets require an explicit
/// send capability from the current authenticated bridge scope.
pub struct DaemonSendSink {
    daemon: Arc<Daemon>,
}

pub type ActionHandler = Arc<
    dyn Fn(&str, &serde_json::Value, serde_json::Value) -> crate::onebot::ActionResponse
        + Send
        + Sync,
>;

/// All network transports share this exact action registry and daemon sink.
pub fn action_handler(daemon: Arc<Daemon>) -> ActionHandler {
    let handler_daemon = Arc::clone(&daemon);
    Arc::new(move |action, params, echo| {
        let mut sink = DaemonSendSink::new(Arc::clone(&handler_daemon));
        let queries = DaemonQueryBackend::new(Arc::clone(&handler_daemon));
        handle_action_with_queries(
            action,
            params,
            echo,
            &mut sink,
            handler_daemon.as_ref(),
            &queries,
        )
    })
}

/// Normal-build query adapter. The daemon broker validates the response scope;
/// query errors never fall back to learned peers or store login placeholders.
pub struct DaemonQueryBackend {
    daemon: Arc<Daemon>,
}

impl DaemonQueryBackend {
    pub fn new(daemon: Arc<Daemon>) -> Self {
        Self { daemon }
    }
}

impl QueryBackend for DaemonQueryBackend {
    fn query_host(&self, query: HostQuery, timeout: Duration) -> QueryOutcome {
        // Default get_login_info must be available after a core restart even
        // when a locked QQ leaves the native LoginInfo query unanswered. The
        // durable cache is only a cache: no_cache/SelfProfile still queries.
        if matches!(query, HostQuery::LoginInfo) {
            if let Some(info) = self.daemon.cached_login_info() {
                return QueryOutcome::Success(QueryData::LoginInfo(info));
            }
        }
        let outcome = self.daemon.query_host(query, timeout);
        match &outcome {
            QueryOutcome::Success(QueryData::LoginInfo(info)) => {
                self.daemon.remember_login_info(&info.user_id, &info.nickname);
            }
            QueryOutcome::Success(QueryData::SelfProfile(info)) => {
                self.daemon.remember_login_info(&info.user_id, &info.nickname);
            }
            _ => {}
        }
        outcome
    }

    fn voice_media(&self) -> Option<(&MediaTool, StopToken)> {
        let media = self.daemon.voice_media.as_ref()?;
        media.config().silk_decoder.as_ref()?;
        Some((media, StopToken::from_flag(self.daemon.stop_flag())))
    }
}

impl DaemonSendSink {
    pub fn new(daemon: Arc<Daemon>) -> Self {
        Self { daemon }
    }
    fn wait_for_receipt(&self, request_id: &str, target: &SessionKey) -> Result<SendDisposition, String> {
        let deadline = Instant::now() + Duration::from_millis(SEND_DEADLINE_MS + 1_000);
        // NativeStarted records dispatch progress, not delivery confirmation.
        // Return success only after both the native receipt and persisted
        // OneBot message id are available.
        loop {
            if let Some((state, message_id)) = self.daemon.onebot_text_status(request_id, target)
            {
                match state {
                    ActionState::ConfirmedSuccess { .. } => {
                        if let Some(message_id) = message_id {
                            return Ok(SendDisposition::Confirmed { message_id });
                        }
                    }
                    ActionState::ConfirmedFailure { reason } => {
                        return Err(format!("native send rejected: {reason}"));
                    }
                    ActionState::DeliveryUnknown => {
                        return Ok(SendDisposition::DeliveryUnknown {
                            reason: "发送结果未知；未自动重发，请先查询现有记录".into(),
                        });
                    }
                    ActionState::CancelledUnsent => {
                        return Err("请求已确认未发送".into());
                    }
                    ActionState::Accepted
                    | ActionState::Queued
                    | ActionState::DispatchIntent
                    | ActionState::NativeStarted
                    | ActionState::AcceptedUnconfirmed => {}
                }
            }
            if Instant::now() >= deadline {
                return Ok(SendDisposition::DeliveryUnknown {
                    reason:
                        "等待 native identity 与持久 OneBot message_id 超时；结果未知，未自动重发"
                            .into(),
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl SendSink for DaemonSendSink {
    fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String> {
        let (kind, peer) = match &payload.target {
            SendTarget::Group { group_id } => (SessionKind::Group, *group_id),
            SendTarget::Private { user_id } if self.daemon.supports_private_text() => {
                (SessionKind::Private, *user_id)
            }
            SendTarget::Private { .. } => {
                return Ok(SendDisposition::Unsupported {
                    reason: "当前宿主未声明绑定会话的私聊文本发送能力".into(),
                })
            }
        };
        let account = self.daemon.config.account.clone();
        // Persist the public UIN; the owner adapter resolves a native UID.
        let target = SessionKey::new(account, kind, peer.to_string());
        let request_id = self.daemon.submit_onebot_text(
            target.clone(), payload.text.clone(), SEND_DEADLINE_MS,
        )?;
        self.wait_for_receipt(&request_id, &target)
    }

    fn send_image(&mut self, target: SendTarget, png: Vec<u8>) -> Result<SendDisposition, String> {
        let (kind, peer, chat_type) = match target {
            SendTarget::Private { user_id } => (SessionKind::Private, user_id, 1),
            SendTarget::Group { group_id } => (SessionKind::Group, group_id, 2),
        };
        if !self.daemon.supports_image(chat_type) {
            return Ok(SendDisposition::Unsupported {
                reason: "当前宿主未声明绑定会话的图片发送能力".into(),
            });
        }
        let peer = peer.to_string();
        let target = SessionKey::new(&self.daemon.config.account, kind, &peer);
        let request_id = self
            .daemon
            .submit_image(kind, &peer, png, SEND_DEADLINE_MS)?;
        self.wait_for_receipt(&request_id, &target)
    }

    fn send_private_voice(&mut self, user_id: u64, silk: Vec<u8>) -> Result<SendDisposition, String> {
        if !self.daemon.supports_private_voice() {
            return Ok(SendDisposition::Unsupported {
                reason: "当前宿主未声明绑定会话的私聊语音发送能力".into(),
            });
        }
        let peer = user_id.to_string();
        let target = SessionKey::new(&self.daemon.config.account, SessionKind::Private, &peer);
        let request_id = self.daemon.submit_private_voice(&peer, silk, SEND_DEADLINE_MS)?;
        self.wait_for_receipt(&request_id, &target)
    }

    fn send_group_voice(&mut self, group_id: u64, silk: Vec<u8>) -> Result<SendDisposition, String> {
        if !self.daemon.supports_group_voice() {
            return Ok(SendDisposition::Unsupported {
                reason: "当前宿主未声明绑定会话的群语音发送能力".into(),
            });
        }
        let peer = group_id.to_string();
        let target = SessionKey::new(&self.daemon.config.account, SessionKind::Group, &peer);
        let request_id = self.daemon.submit_voice(SessionKind::Group, &peer, silk, SEND_DEADLINE_MS)?;
        self.wait_for_receipt(&request_id, &target)
    }
}

/// Blocking normal-build listener. The bind address is chosen and validated
/// by caligod; this function shares the existing action registry and parser.
pub fn serve_http(
    daemon: Arc<Daemon>,
    listener: &TcpListener,
    access_token: Option<&str>,
) -> usize {
    let stop = daemon.stop_flag();
    let handler = action_handler(daemon);
    serve_parallel(
        listener,
        DEFAULT_MAX_BODY,
        access_token,
        move || stop.load(std::sync::atomic::Ordering::Relaxed),
        handler,
    )
}

/// Bind only to a loopback address so OneBot credentials are not exposed to a
/// network interface by an accidental command-line setting.
pub fn bind_loopback(address: &str) -> std::io::Result<TcpListener> {
    let address = address.parse::<std::net::SocketAddr>().map_err(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
    })?;
    if !address.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "OneBot HTTP must bind to a loopback address",
        ));
    }
    TcpListener::bind(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_login_short_circuits_the_native_query() {
        let dir = std::env::temp_dir().join(format!(
            "caligo-login-backend-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let config = crate::daemon::DaemonConfig {
            pipe_prefix: format!("caligo-login-backend-{}", std::process::id()),
            journal_path: dir.join("journal.jsonl"),
            module_baseline: "test".into(),
            account: "1694717255".into(),
            core_build: "test".into(),
            expect_client_pid: 0,
            runtime: crate::runtime::RuntimeConfig::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        };
        let daemon = Daemon::start(config).expect("daemon");
        daemon.remember_login_info("1694717255", "nick");
        let backend = DaemonQueryBackend::new(daemon.clone());
        match backend.query_host(HostQuery::LoginInfo, Duration::from_millis(1)) {
            QueryOutcome::Success(QueryData::LoginInfo(info)) => {
                assert_eq!(info.user_id, "1694717255");
                assert_eq!(info.nickname, "nick");
            }
            other => panic!("cached login was not served: {other:?}"),
        }
        drop(backend);
        drop(daemon);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn listener_accepts_ephemeral_loopback_and_rejects_external_bind() {
        let listener = bind_loopback("127.0.0.1:0").unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
        assert_eq!(
            bind_loopback("192.0.2.1:0").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
}
