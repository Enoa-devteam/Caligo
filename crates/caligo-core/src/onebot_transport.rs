//! OneBot V11 event transports. Each transport has its own bounded EventBus
//! subscription and invokes the same action handler used by HTTP API calls.

use crate::daemon::Daemon;
use crate::onebot::ActionResponse;
use crate::onebot_server::{action_handler, ActionHandler};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WsError, Message, WebSocket};
use url::Url;

const MAX_WS_CLIENTS: usize = 16;
const WS_READ_POLL: Duration = Duration::from_millis(100);
const WS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_POST_REPLY_CAP: u64 = 64 * 1024;

type HmacSha1 = Hmac<Sha1>;

#[derive(Clone)]
pub struct HttpPostConfig {
    pub url: String,
    pub secret: Option<String>,
    pub timeout: Duration,
}

pub struct ReverseWsConfig {
    pub url: String,
    pub access_token: Option<String>,
    pub self_id: u64,
    pub reconnect_interval: Duration,
}

/// Run one bounded HTTP POST event consumer until the daemon stops. Network
/// waits happen on this worker and cannot block bridge/native event capture.
pub fn run_http_post(daemon: Arc<Daemon>, config: HttpPostConfig) -> usize {
    run_http_post_inner(daemon, config, None)
}

/// Variant used by caligod to make sure the HTTP POST subscriber is ready
/// before the daemon starts accepting native events.
pub fn run_http_post_ready(
    daemon: Arc<Daemon>,
    config: HttpPostConfig,
    ready: std::sync::mpsc::SyncSender<()>,
) -> usize {
    run_http_post_inner(daemon, config, Some(ready))
}

fn run_http_post_inner(
    daemon: Arc<Daemon>,
    config: HttpPostConfig,
    ready: Option<std::sync::mpsc::SyncSender<()>>,
) -> usize {
    daemon.enable_onebot_http_post_ordering();
    let (subscription, events) = daemon.subscribe_onebot_http_post_events("http-post");
    if let Some(ready) = ready {
        let _ = ready.send(());
    }
    let handler = action_handler(Arc::clone(&daemon));
    let stop = daemon.stop_flag();
    let mut delivered = 0usize;
    let mut failed = 0usize;
    while !stop.load(Ordering::Relaxed) {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                match post_event_once(&config, &event) {
                    Ok(Some(quick_operation)) => {
                        execute_http_quick_operation(&handler, &event, quick_operation);
                        delivered += 1;
                    }
                    Ok(None) => delivered += 1,
                    Err(_) => {
                        failed += 1;
                        eprintln!("[onebot] HTTP POST event delivery failed");
                    }
                }
                // OneBot V11 specifies POST handling before forwarding the
                // same event to connected forward/reverse WebSocket clients.
                daemon.publish_onebot_ws_event_json(&event);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let dropped = daemon
        .onebot_http_post_event_subscriber_dropped(subscription)
        .unwrap_or(0);
    daemon.unsubscribe_onebot_http_post_events(subscription);
    if failed > 0 || dropped > 0 {
        eprintln!(
            "[onebot] HTTP POST transport stopped with loss: delivered={delivered} failed={failed} dropped={dropped}"
        );
    }
    delivered
}

/// POST one already-serialized event. The response is bounded because the
/// OneBot quick-operation body is optional and must not exhaust the worker.
pub fn post_event_once(config: &HttpPostConfig, event_json: &str) -> Result<Option<Value>, String> {
    let endpoint = Url::parse(&config.url).map_err(|_| "invalid HTTP POST URL".to_string())?;
    if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
        return Err("HTTP POST URL must use http or https".into());
    }
    let event: Value = serde_json::from_str(event_json).map_err(|e| e.to_string())?;
    let self_id = event
        .get("self_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "OneBot event is missing numeric self_id".to_string())?;
    let timeout = config
        .timeout
        .max(Duration::from_millis(100))
        .min(Duration::from_secs(60));
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build();
    let mut request = agent
        .post(&config.url)
        .set("Content-Type", "application/json")
        .set("X-Self-ID", &self_id.to_string());
    if let Some(secret) = config.secret.as_deref().filter(|value| !value.is_empty()) {
        request = request.set(
            "X-Signature",
            &http_post_signature(secret, event_json.as_bytes())?,
        );
    }
    let response = match request.send_bytes(event_json.as_bytes()) {
        Ok(response) => response,
        Err(ureq::Error::Status(_, _)) => {
            return Err("HTTP POST application returned a non-success status".into())
        }
        Err(_) => return Err("HTTP POST request failed".into()),
    };
    if !(200..300).contains(&response.status()) {
        return Err("HTTP POST application returned a non-success status".into());
    }
    let mut body = Vec::new();
    response
        .into_reader()
        .take(HTTP_POST_REPLY_CAP + 1)
        .read_to_end(&mut body)
        .map_err(|_| "HTTP POST response read failed".to_string())?;
    if body.len() as u64 > HTTP_POST_REPLY_CAP {
        return Err("HTTP POST quick-operation body exceeded 64 KiB".into());
    }
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|_| "HTTP POST quick-operation body was not JSON".into())
}

fn http_post_signature(secret: &str, body: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    let mut signature = String::with_capacity(5 + bytes.len() * 2);
    signature.push_str("sha1=");
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(signature, "{byte:02x}");
    }
    Ok(signature)
}

fn execute_http_quick_operation(handler: &ActionHandler, event_json: &str, operation: Value) {
    let Some(reply) = operation.get("reply") else {
        return;
    };
    let Ok(event) = serde_json::from_str::<Value>(event_json) else {
        return;
    };
    let Some(message_type) = event.get("message_type").and_then(Value::as_str) else {
        return;
    };
    if message_type == "group" && operation.get("at_sender").and_then(Value::as_bool) != Some(false)
    {
        eprintln!(
            "[onebot] group quick reply with at_sender is not supported; no action dispatched"
        );
        return;
    }
    let (action, key) = match message_type {
        "group" => ("send_group_msg", "group_id"),
        "private" => ("send_private_msg", "user_id"),
        _ => return,
    };
    let Some(target) = event.get(key).cloned() else {
        return;
    };
    let mut params = serde_json::Map::new();
    params.insert(key.to_owned(), target);
    params.insert("message".into(), reply.clone());
    let params = Value::Object(params);
    let response = handler(action, &params, Value::Null);
    if response.retcode != 0 {
        eprintln!("[onebot] HTTP POST quick reply was not confirmed or is unsupported");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsRole {
    Api,
    Event,
    Universal,
}

impl WsRole {
    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/api" | "/api/" => Some(Self::Api),
            "/event" | "/event/" => Some(Self::Event),
            "/" => Some(Self::Universal),
            _ => None,
        }
    }

    fn api(self) -> bool {
        matches!(self, Self::Api | Self::Universal)
    }

    fn events(self) -> bool {
        matches!(self, Self::Event | Self::Universal)
    }
}

/// Forward WebSocket listener. The caller binds a loopback listener and owns
/// its lifetime; this loop caps concurrent sessions and polls stop promptly.
pub fn serve_forward_ws(
    daemon: Arc<Daemon>,
    listener: TcpListener,
    access_token: Option<String>,
) -> usize {
    if listener.set_nonblocking(true).is_err() {
        return 0;
    }
    let active = Arc::new(AtomicUsize::new(0));
    let stop = daemon.stop_flag();
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut accepted = 0usize;
    while !stop.load(Ordering::Relaxed) {
        reap_workers(&mut workers);
        match listener.accept() {
            Ok((stream, _)) => {
                if !acquire_slot(&active) {
                    reject_busy(stream);
                    continue;
                }
                accepted += 1;
                let daemon = Arc::clone(&daemon);
                let active = Arc::clone(&active);
                let token = access_token.clone();
                workers.push(thread::spawn(move || {
                    let _slot = ClientSlot(active);
                    let _ = stream.set_read_timeout(Some(WS_HANDSHAKE_TIMEOUT));
                    let _ = stream.set_write_timeout(Some(WS_WRITE_TIMEOUT));
                    serve_forward_connection(stream, daemon, token);
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    accepted
}

struct ClientSlot(Arc<AtomicUsize>);

impl Drop for ClientSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn acquire_slot(active: &AtomicUsize) -> bool {
    let mut current = active.load(Ordering::Relaxed);
    loop {
        if current >= MAX_WS_CLIENTS {
            return false;
        }
        match active.compare_exchange_weak(
            current,
            current + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

fn reap_workers(workers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            let _ = worker.join();
        } else {
            index += 1;
        }
    }
}

fn reject_busy(mut stream: TcpStream) {
    let _ = stream.write_all(
        b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
    );
}

fn token_matches(request: &Request, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    if expected.is_empty() {
        return true;
    }
    if let Some(value) = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
    {
        if let Some((scheme, token)) = value.split_once(' ') {
            if scheme.eq_ignore_ascii_case("bearer") && token == expected {
                return true;
            }
        }
        if value == expected {
            return true;
        }
    }
    request.uri().query().is_some_and(|query| {
        url::form_urlencoded::parse(query.as_bytes())
            .any(|(key, value)| key == "access_token" && value == expected)
    })
}

fn reject_handshake(status: u16, wording: &str) -> ErrorResponse {
    tungstenite::http::Response::builder()
        .status(status)
        .header("Connection", "close")
        .body(Some(wording.to_owned()))
        .expect("valid websocket rejection response")
}

fn serve_forward_connection(stream: TcpStream, daemon: Arc<Daemon>, token: Option<String>) {
    let role_slot = Arc::new(std::sync::Mutex::new(None));
    let role_for_handshake = Arc::clone(&role_slot);
    let token_for_handshake = token.clone();
    let socket = tungstenite::accept_hdr(stream, move |request: &Request, response: Response| {
        let Some(role) = WsRole::from_path(request.uri().path()) else {
            return Err(reject_handshake(404, "unsupported OneBot WebSocket path"));
        };
        if !token_matches(request, token_for_handshake.as_deref()) {
            return Err(reject_handshake(401, "unauthorized"));
        }
        *role_for_handshake.lock().unwrap() = Some(role);
        Ok(response)
    });
    let Ok(mut socket) = socket else {
        return;
    };
    let Some(role) = role_slot.lock().unwrap().take() else {
        return;
    };
    let (subscription, events) = if role.events() {
        let (id, receiver) = daemon.subscribe_onebot_events("forward-ws");
        (Some(id), Some(receiver))
    } else {
        (None, None)
    };
    let stream = socket.get_mut();
    let _ = stream.set_read_timeout(Some(WS_READ_POLL));
    let _ = stream.set_write_timeout(Some(WS_WRITE_TIMEOUT));
    let handler = action_handler(Arc::clone(&daemon));
    let outcome = serve_ws_socket(
        &mut socket,
        role,
        events.as_ref(),
        &handler,
        daemon.stop_flag(),
    );
    if let Some(subscription) = subscription {
        let dropped = daemon
            .onebot_event_subscriber_dropped(subscription)
            .unwrap_or(0);
        match outcome {
            Err(error) => eprintln!(
                "[onebot] forward WebSocket disconnected ({role:?}): {error}; dropped events={dropped}"
            ),
            Ok(()) if dropped > 0 => eprintln!(
                "[onebot] forward WebSocket closed with queue loss ({role:?}): dropped events={dropped}"
            ),
            Ok(()) => {}
        }
        daemon.unsubscribe_onebot_events(subscription);
    }
}

fn serve_ws_socket<S: Read + Write>(
    socket: &mut WebSocket<S>,
    role: WsRole,
    events: Option<&Receiver<String>>,
    handler: &ActionHandler,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    while !stop.load(Ordering::Relaxed) {
        match socket.read() {
            Ok(Message::Text(raw)) if role.api() => {
                let response = match crate::onebot::parse_action(raw.as_str()) {
                    Ok((action, params, echo)) => handler(&action, &params, echo),
                    Err(error) => {
                        ActionResponse::failed(crate::onebot::RET_BAD_REQUEST, &error, Value::Null)
                    }
                };
                let body = serde_json::to_string(&response).map_err(|e| e.to_string())?;
                socket
                    .send(Message::Text(body.into()))
                    .map_err(|e| e.to_string())?;
            }
            Ok(Message::Text(_)) => {}
            Ok(Message::Ping(payload)) => socket
                .send(Message::Pong(payload))
                .map_err(|e| e.to_string())?,
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(frame)) => {
                let _ = socket.close(frame);
                break;
            }
            Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => break,
            Err(WsError::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => break,
            Err(error) => return Err(error.to_string()),
        }
        if let Some(events) = events {
            for _ in 0..16 {
                match events.try_recv() {
                    Ok(event) => socket
                        .send(Message::Text(event.into()))
                        .map_err(|e| e.to_string())?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
        }
        socket.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Reverse WebSocket Universal client. Events remain in its own bounded
/// queue across reconnect attempts; a disconnected application never blocks
/// the daemon or another transport.
pub fn run_reverse_ws(daemon: Arc<Daemon>, config: ReverseWsConfig) -> usize {
    if config.self_id == 0 {
        return 0;
    }
    let (subscription, events) = daemon.subscribe_onebot_events("reverse-ws");
    let handler = action_handler(Arc::clone(&daemon));
    let stop = daemon.stop_flag();
    let interval = config
        .reconnect_interval
        .max(Duration::from_millis(100))
        .min(Duration::from_secs(300));
    let mut connected = 0usize;
    while !stop.load(Ordering::Relaxed) {
        match reverse_connect_request(&config).and_then(connect_reverse) {
            Ok(mut socket) => {
                connected += 1;
                set_reverse_timeouts(socket.get_mut());
                let outcome = serve_ws_socket(
                    &mut socket,
                    WsRole::Universal,
                    Some(&events),
                    &handler,
                    Arc::clone(&stop),
                );
                if let Err(error) = outcome {
                    let dropped = daemon
                        .onebot_event_subscriber_dropped(subscription)
                        .unwrap_or(0);
                    eprintln!(
                        "[onebot] reverse WebSocket delivery failed: {error}; dropped events={dropped}; reconnecting"
                    );
                }
            }
            Err(_) => eprintln!("[onebot] reverse WebSocket connection failed"),
        }
        sleep_while_running(&stop, interval);
    }
    daemon.unsubscribe_onebot_events(subscription);
    connected
}

fn reverse_connect_request(
    config: &ReverseWsConfig,
) -> Result<tungstenite::handshake::client::Request, String> {
    let self_id = config.self_id.to_string();
    let mut endpoint = Url::parse(&config.url).map_err(|e| e.to_string())?;
    if !matches!(endpoint.scheme(), "ws" | "wss") {
        return Err("reverse WebSocket URL must use ws or wss".into());
    }
    let mut request = endpoint
        .as_str()
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "user-agent",
        tungstenite::http::HeaderValue::from_static("OneBot/11 Caligo"),
    );
    request.headers_mut().insert(
        "x-self-id",
        tungstenite::http::HeaderValue::from_str(&self_id).map_err(|e| e.to_string())?,
    );
    request.headers_mut().insert(
        "x-client-role",
        tungstenite::http::HeaderValue::from_static("Universal"),
    );
    if let Some(token) = config
        .access_token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        match tungstenite::http::HeaderValue::from_str(&format!("Bearer {token}")) {
            Ok(value) => {
                request.headers_mut().insert("authorization", value);
            }
            Err(_) => {
                if endpoint.query_pairs().any(|(key, _)| key == "access_token") {
                    return Err("reverse WebSocket URL already contains access_token".into());
                }
                endpoint
                    .query_pairs_mut()
                    .append_pair("access_token", token);
                request = endpoint
                    .as_str()
                    .into_client_request()
                    .map_err(|e| e.to_string())?;
                request.headers_mut().insert(
                    "user-agent",
                    tungstenite::http::HeaderValue::from_static("OneBot/11 Caligo"),
                );
                request.headers_mut().insert(
                    "x-self-id",
                    tungstenite::http::HeaderValue::from_str(&self_id)
                        .map_err(|e| e.to_string())?,
                );
                request.headers_mut().insert(
                    "x-client-role",
                    tungstenite::http::HeaderValue::from_static("Universal"),
                );
            }
        }
    }
    Ok(request)
}

fn connect_reverse(
    request: tungstenite::handshake::client::Request,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    tungstenite::connect(request)
        .map(|(socket, _)| socket)
        .map_err(|_| "websocket handshake failed".into())
}

fn set_reverse_timeouts(stream: &mut MaybeTlsStream<TcpStream>) {
    match stream {
        MaybeTlsStream::Plain(tcp) => {
            let _ = tcp.set_read_timeout(Some(WS_READ_POLL));
            let _ = tcp.set_write_timeout(Some(WS_WRITE_TIMEOUT));
        }
        MaybeTlsStream::Rustls(tls) => {
            let _ = tls.sock.set_read_timeout(Some(WS_READ_POLL));
            let _ = tls.sock.set_write_timeout(Some(WS_WRITE_TIMEOUT));
        }
        _ => {}
    }
}

fn sleep_while_running(stop: &std::sync::atomic::AtomicBool, duration: Duration) {
    let until = std::time::Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < until {
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::DaemonConfig;
    use crate::runtime::RuntimeConfig;
    use std::io::{BufRead, BufReader};

    fn test_daemon(tag: &str) -> Arc<Daemon> {
        let mut journal = std::env::temp_dir();
        journal.push(format!(
            "caligo-onebot-{tag}-{}.journal",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&journal);
        let _ = std::fs::remove_file(journal.with_extension("messages.jsonl"));
        Daemon::start(DaemonConfig {
            pipe_prefix: format!("onebot-{tag}-{}", std::process::id()),
            journal_path: journal,
            module_baseline: "test".into(),
            account: "10001".into(),
            core_build: crate::CORE_BUILD.into(),
            expect_client_pid: 0,
            runtime: RuntimeConfig::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        })
        .unwrap()
    }

    #[test]
    fn http_post_signature_matches_onebot_hmac_sha1_contract() {
        assert_eq!(
            http_post_signature("key", b"The quick brown fox jumps over the lazy dog").unwrap(),
            "sha1=de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9"
        );
    }

    #[test]
    fn http_post_sends_signed_event_and_accepts_empty_204() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                headers.push_str(&line);
            }
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body).unwrap();
            assert!(headers.to_ascii_lowercase().contains("x-self-id: 10001"));
            assert!(headers.to_ascii_lowercase().contains("x-signature: sha1="));
            assert_eq!(body, br#"{"self_id":10001,"post_type":"message"}"#);
            let mut stream = reader.into_inner();
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let result = post_event_once(
            &HttpPostConfig {
                url: format!("http://{address}/events"),
                secret: Some("secret".into()),
                timeout: Duration::from_secs(2),
            },
            r#"{"self_id":10001,"post_type":"message"}"#,
        )
        .unwrap();
        assert!(result.is_none());
        receiver.join().unwrap();
    }

    #[test]
    fn http_post_finishes_before_websocket_event_fanout() {
        let daemon = test_daemon("post-order");
        daemon.enable_onebot_http_post_ordering();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_seen_tx, request_seen_rx) = std::sync::mpsc::channel();
        let (finish_response_tx, finish_response_rx) = std::sync::mpsc::channel();
        let receiver = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; content_length];
            std::io::Read::read_exact(&mut reader, &mut body).unwrap();
            request_seen_tx.send(()).unwrap();
            finish_response_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            let mut stream = reader.into_inner();
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });

        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
        let post_daemon = Arc::clone(&daemon);
        let worker = thread::spawn(move || {
            run_http_post_ready(
                post_daemon,
                HttpPostConfig {
                    url: format!("http://{address}/events"),
                    secret: None,
                    timeout: Duration::from_secs(2),
                },
                ready_tx,
            )
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (_subscription, ws_events) = daemon.subscribe_onebot_events("post-order-ws");
        let event = r#"{"self_id":10001,"post_type":"message"}"#;
        daemon.publish_onebot_event_json(event);

        request_seen_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(ws_events.try_recv().is_err());
        finish_response_tx.send(()).unwrap();
        assert_eq!(
            ws_events.recv_timeout(Duration::from_secs(2)).unwrap(),
            event
        );

        daemon.stop_flag().store(true, Ordering::Relaxed);
        receiver.join().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn forward_websocket_routes_shared_actions_and_publishes_events() {
        let daemon = test_daemon("forward");
        let listener = crate::onebot_server::bind_loopback("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_daemon = Arc::clone(&daemon);
        let server =
            thread::spawn(move || serve_forward_ws(server_daemon, listener, Some("token".into())));
        let mut request = format!("ws://{address}/").into_client_request().unwrap();
        request.headers_mut().insert(
            "authorization",
            tungstenite::http::HeaderValue::from_static("Bearer token"),
        );
        let (mut socket, _) = tungstenite::connect(request).unwrap();
        socket
            .send(Message::Text(
                r#"{"action":"get_version_info","params":{},"echo":{"id":7}}"#.into(),
            ))
            .unwrap();
        let action_response: Value =
            serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert_eq!(action_response["status"], "ok");
        assert_eq!(action_response["echo"]["id"], 7);

        daemon.publish_onebot_event_json(r#"{"post_type":"message","message_id":9}"#);
        let event: Value =
            serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert_eq!(event["message_id"], 9);
        socket.close(None).unwrap();
        daemon.stop_flag().store(true, Ordering::Relaxed);
        assert_eq!(server.join().unwrap(), 1);
    }

    #[test]
    fn reverse_websocket_uses_v11_headers_and_returns_echoed_action_result() {
        let daemon = test_daemon("reverse");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_daemon = Arc::clone(&daemon);
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                    assert_eq!(request.headers()["x-self-id"], "10001");
                    assert_eq!(request.headers()["x-client-role"], "Universal");
                    assert_eq!(request.headers()["authorization"], "Bearer app-token");
                    assert_eq!(request.headers()["user-agent"], "OneBot/11 Caligo");
                    assert!(!request.headers().contains_key("sec-websocket-protocol"));
                    Ok(response)
                })
                .unwrap();
            socket
                .send(Message::Text(
                    r#"{"action":"get_version_info","params":{},"echo":"reverse-1"}"#.into(),
                ))
                .unwrap();
            let reply: Value =
                serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_str()).unwrap();
            assert_eq!(reply["status"], "ok");
            assert_eq!(reply["echo"], "reverse-1");
            server_daemon.publish_onebot_event_json(r#"{"post_type":"message","message_id":12}"#);
            let event: Value =
                serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_str()).unwrap();
            assert_eq!(event["post_type"], "message");
            assert_eq!(event["message_id"], 12);
            server_daemon.stop_flag().store(true, Ordering::Relaxed);
            let _ = socket.close(None);
        });
        let client_daemon = Arc::clone(&daemon);
        let client = thread::spawn(move || {
            run_reverse_ws(
                client_daemon,
                ReverseWsConfig {
                    url: format!("ws://{address}/reverse"),
                    access_token: Some("app-token".into()),
                    self_id: 10001,
                    reconnect_interval: Duration::from_millis(100),
                },
            )
        });
        server.join().unwrap();
        assert!(client.join().unwrap() >= 1);
    }

    #[test]
    fn http_post_quick_reply_routes_target_and_plain_message_through_shared_action_handler() {
        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_handler = Arc::clone(&captured);
        let handler: ActionHandler = Arc::new(move |action, params, echo| {
            *captured_handler.lock().unwrap() =
                Some((action.to_owned(), params.clone(), echo.clone()));
            ActionResponse::ok(Value::Null, echo)
        });
        let event = r#"{"message_type":"group","group_id":20002,"user_id":30003}"#;
        execute_http_quick_operation(
            &handler,
            event,
            serde_json::json!({"reply":"hello","at_sender":false}),
        );
        let (action, params, echo) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(action, "send_group_msg");
        assert_eq!(params["group_id"], 20002);
        assert_eq!(params["message"], "hello");
        assert!(echo.is_null());

        *captured.lock().unwrap() = None;
        execute_http_quick_operation(&handler, event, serde_json::json!({"reply":"hello"}));
        assert!(captured.lock().unwrap().is_none(), "at_sender defaults true for group events and unsupported mention replies must not be sent as plain text");
    }

    /// Deterministic T17 stand-in for a stalled network client. Windows
    /// loopback absorbs real socket backpressure (see the SSE lab record), so
    /// the "slow client" is a transport writer that blocks after accepting the
    /// first frame. The publish path must stay non-blocking and the loss must
    /// stay observable instead of being silently retried.
    struct BlockingWriter {
        release: std::sync::mpsc::Receiver<()>,
        blocked_once: bool,
    }

    impl Read for BlockingWriter {
        fn read(&mut self, _buffer: &mut [u8]) -> Result<usize, std::io::Error> {
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "no inbound application data",
            ))
        }
    }

    impl Write for BlockingWriter {
        fn write(&mut self, buffer: &[u8]) -> Result<usize, std::io::Error> {
            if !self.blocked_once {
                self.blocked_once = true;
                let _ = self.release.recv();
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> Result<(), std::io::Error> {
            Ok(())
        }
    }

    #[test]
    fn stalled_websocket_writer_does_not_block_publish_and_drop_count_is_visible() {
        use crate::onebot_events::SUBSCRIBER_QUEUE_CAP;

        let daemon = test_daemon("slow-ws");
        let (subscription, events) = daemon.subscribe_onebot_events("slow-ws");
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let mut socket = WebSocket::from_raw_socket(
            BlockingWriter {
                release: release_rx,
                blocked_once: false,
            },
            tungstenite::protocol::Role::Server,
            None,
        );
        let worker_daemon = Arc::clone(&daemon);
        let worker_stop = daemon.stop_flag();
        let worker = thread::spawn(move || {
            let handler = action_handler(worker_daemon);
            serve_ws_socket(
                &mut socket,
                WsRole::Event,
                Some(&events),
                &handler,
                worker_stop,
            )
        });

        let started = std::time::Instant::now();
        for index in 0..(SUBSCRIBER_QUEUE_CAP * 2) {
            daemon.publish_onebot_ws_event_json(&format!(r#"{{"seq":{index}}}"#));
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a stalled transport writer must not block the fanout"
        );
        let dropped = daemon
            .onebot_event_subscriber_dropped(subscription)
            .expect("subscription is still registered while the writer is stalled");
        assert!(
            dropped > 0,
            "queue overflow behind a stalled writer must be counted"
        );

        release_tx.send(()).unwrap();
        // Let the released writer drain one bounded batch before shutdown.
        thread::sleep(Duration::from_millis(50));
        daemon.stop_flag().store(true, Ordering::Relaxed);
        assert!(worker.join().unwrap().is_ok());
    }

    #[test]
    fn http_post_failure_still_forwards_the_event_and_reports_loss() {
        let daemon = test_daemon("post-failure");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; content_length];
            std::io::Read::read_exact(&mut reader, &mut body).unwrap();
            let mut stream = reader.into_inner();
            stream
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
        let worker_daemon = Arc::clone(&daemon);
        let worker = thread::spawn(move || {
            run_http_post_ready(
                worker_daemon,
                HttpPostConfig {
                    url: format!("http://{address}/events"),
                    secret: None,
                    timeout: Duration::from_millis(500),
                },
                ready_tx,
            )
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (_subscription, ws_events) = daemon.subscribe_onebot_events("post-failure-ws");
        let event = r#"{"self_id":10001,"post_type":"message","message_id":21}"#;
        daemon.publish_onebot_event_json(event);
        assert_eq!(
            ws_events.recv_timeout(Duration::from_secs(2)).unwrap(),
            event,
            "a failed POST must not suppress the same event for WebSocket clients"
        );
        daemon.stop_flag().store(true, Ordering::Relaxed);
        receiver.join().unwrap();
        assert_eq!(worker.join().unwrap(), 0, "failed POST is not a delivery");
    }

    fn ws_request(url: &str, token: Option<&str>) -> tungstenite::handshake::client::Request {
        let mut request = url.into_client_request().unwrap();
        if let Some(token) = token {
            request
                .headers_mut()
                .insert("authorization", format!("Bearer {token}").parse().unwrap());
        }
        request
    }

    /// Assert that a control-plane client receives no frame within a short
    /// bounded wait; a timed-out read is tungstenite's no-message state.
    fn expect_no_message(socket: &mut WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>) {
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream
                .set_read_timeout(Some(Duration::from_millis(250)))
                .unwrap();
        }
        let result = socket.read();
        assert!(
            matches!(&result, Err(WsError::Io(error)) if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            )),
            "role must not receive this frame: {result:?}"
        );
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream.set_read_timeout(None).unwrap();
        }
    }

    #[test]
    fn forward_websocket_enforces_auth_paths_roles_and_echo() {
        let daemon = test_daemon("forward-matrix");
        let listener = crate::onebot_server::bind_loopback("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_daemon = Arc::clone(&daemon);
        let server =
            thread::spawn(move || serve_forward_ws(server_daemon, listener, Some("token".into())));

        // Unauthorized and unknown paths fail before a session exists.
        match tungstenite::connect(ws_request(&format!("ws://{address}/"), Some("wrong"))) {
            Err(WsError::Http(response)) => assert_eq!(response.status().as_u16(), 401),
            other => panic!("expected 401 handshake rejection, got {other:?}"),
        }
        match tungstenite::connect(ws_request(&format!("ws://{address}/nope"), Some("token"))) {
            Err(WsError::Http(response)) => assert_eq!(response.status().as_u16(), 404),
            other => panic!("expected 404 handshake rejection, got {other:?}"),
        }

        // /api: shared action layer with opaque echo, never events.
        let (mut api, _) =
            tungstenite::connect(ws_request(&format!("ws://{address}/api"), Some("token")))
                .unwrap();
        api.send(Message::Text(
            r#"{"action":"get_version_info","params":{},"echo":{"id":7,"nested":{"a":[1,2]}}}"#
                .into(),
        ))
        .unwrap();
        let response: Value =
            serde_json::from_str(api.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert_eq!(response["status"], "ok");
        assert_eq!(response["retcode"], 0);
        assert_eq!(response["data"]["protocol_version"], "v11");
        assert_eq!(
            response["echo"],
            serde_json::json!({"id":7,"nested":{"a":[1,2]}})
        );
        daemon.publish_onebot_event_json(r#"{"post_type":"message","message_id":31}"#);
        expect_no_message(&mut api);

        // /event: events only; an action frame earns no reply.
        let (mut event, _) =
            tungstenite::connect(ws_request(&format!("ws://{address}/event"), Some("token")))
                .unwrap();
        daemon.publish_onebot_event_json(r#"{"post_type":"message","message_id":32}"#);
        assert_eq!(
            serde_json::from_str::<Value>(event.read().unwrap().into_text().unwrap().as_str())
                .unwrap()["message_id"],
            32
        );
        event
            .send(Message::Text(
                r#"{"action":"get_version_info","params":{},"echo":1}"#.into(),
            ))
            .unwrap();
        expect_no_message(&mut event);

        // /: Universal carries both directions.
        let (mut universal, _) =
            tungstenite::connect(ws_request(&format!("ws://{address}/"), Some("token"))).unwrap();
        universal
            .send(Message::Text(
                r#"{"action":"get_version_info","params":{},"echo":"universal"}"#.into(),
            ))
            .unwrap();
        let reply: Value =
            serde_json::from_str(universal.read().unwrap().into_text().unwrap().as_str()).unwrap();
        assert_eq!(reply["echo"], "universal");
        daemon.publish_onebot_event_json(r#"{"post_type":"message","message_id":33}"#);
        assert_eq!(
            serde_json::from_str::<Value>(universal.read().unwrap().into_text().unwrap().as_str())
                .unwrap()["message_id"],
            33
        );

        let _ = api.close(None);
        let _ = event.close(None);
        let _ = universal.close(None);
        daemon.stop_flag().store(true, Ordering::Relaxed);
        assert!(server.join().unwrap() >= 3);
    }

    #[test]
    fn reverse_websocket_reconnects_after_application_close_and_delivers_again() {
        let daemon = test_daemon("reverse-reconnect");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_daemon = Arc::clone(&daemon);
        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut socket =
                    tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                        assert_eq!(request.headers()["x-self-id"], "10001");
                        assert_eq!(request.headers()["x-client-role"], "Universal");
                        assert_eq!(request.headers()["authorization"], "Bearer app-token");
                        assert_eq!(request.headers()["user-agent"], "OneBot/11 Caligo");
                        Ok(response)
                    })
                    .unwrap();
                socket
                    .send(Message::Text(
                        format!(
                            r#"{{"action":"get_version_info","params":{{}},"echo":"conn-{attempt}"}}"#
                        )
                        .into(),
                    ))
                    .unwrap();
                let reply: Value =
                    serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_str())
                        .unwrap();
                assert_eq!(reply["status"], "ok");
                assert_eq!(reply["echo"], format!("conn-{attempt}"));
                server_daemon.publish_onebot_event_json(&format!(
                    r#"{{"post_type":"message","message_id":{}}}"#,
                    40 + attempt
                ));
                let event: Value =
                    serde_json::from_str(socket.read().unwrap().into_text().unwrap().as_str())
                        .unwrap();
                assert_eq!(event["message_id"], 40 + attempt);
                if attempt == 0 {
                    // Application-side graceful close must be retried, not
                    // treated as a permanent transport failure.
                    let _ = socket.close(None);
                }
            }
            server_daemon.stop_flag().store(true, Ordering::Relaxed);
        });
        let client_daemon = Arc::clone(&daemon);
        let client = thread::spawn(move || {
            run_reverse_ws(
                client_daemon,
                ReverseWsConfig {
                    url: format!("ws://{address}/reverse"),
                    access_token: Some("app-token".into()),
                    self_id: 10001,
                    reconnect_interval: Duration::from_millis(100),
                },
            )
        });
        server.join().unwrap();
        assert!(
            client.join().unwrap() >= 2,
            "reverse client must reconnect after the application closes the socket"
        );
    }
}
