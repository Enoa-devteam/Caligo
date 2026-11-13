//! Loopback OneBot SSE transport, independently implemented from the observable
//! `7cf025f` SnowCat contract: GET /_events and `data: JSON\n\n`.
//!
//! Subscribes to Daemon's general/WS bus, so HTTP POST quick operations finish
//! before SSE fanout when POST ordering is enabled. There is no replay cursor:
//! Last-Event-ID is rejected and frames never invent an `id`. A subscription
//! queue overflow closes that client; metrics distinguish it from socket errors.
//! All socket work, including incomplete headers and writes, has a total deadline.

use crate::daemon::Daemon;
use serde_json::Value;
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug)]
pub struct SseConfig {
    /// Includes clients still sending their HTTP headers.
    pub max_clients: usize,
    pub max_header_bytes: usize,
    pub header_timeout: Duration,
    pub write_timeout: Duration,
    pub heartbeat_interval: Duration,
    /// Reject oversized JSON instead of allocating an unbounded SSE frame.
    pub max_event_bytes: usize,
    pub metrics: Arc<SseMetrics>,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            max_clients: 32,
            max_header_bytes: 8192,
            header_timeout: Duration::from_secs(3),
            write_timeout: Duration::from_secs(2),
            heartbeat_interval: Duration::from_secs(15),
            max_event_bytes: 1024 * 1024,
            metrics: Arc::new(SseMetrics::default()),
        }
    }
}

/// Counters are available while serving through SseConfig::metrics. They contain
/// no credentials or event text. Active gauges return to zero after server exit.
#[derive(Debug, Default)]
pub struct SseMetrics {
    accepted_connections: AtomicU64,
    active_connections: AtomicU64,
    subscriptions: AtomicU64,
    active_subscriptions: AtomicU64,
    unsubscribed: AtomicU64,
    auth_rejections: AtomicU64,
    rejected_requests: AtomicU64,
    capacity_rejections: AtomicU64,
    queue_overflows: AtomicU64,
    dropped_events: AtomicU64,
    invalid_events: AtomicU64,
    write_failures: AtomicU64,
    sent_events: AtomicU64,
    heartbeats: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SseStats {
    pub accepted_connections: u64,
    pub active_connections: u64,
    pub subscriptions: u64,
    pub active_subscriptions: u64,
    pub unsubscribed: u64,
    pub auth_rejections: u64,
    pub rejected_requests: u64,
    pub capacity_rejections: u64,
    pub queue_overflows: u64,
    pub dropped_events: u64,
    pub invalid_events: u64,
    pub write_failures: u64,
    pub sent_events: u64,
    pub heartbeats: u64,
}

impl SseMetrics {
    /// Each counter is atomic; concurrent snapshots are diagnostic, not a
    /// transaction across all counters. The final snapshot follows worker joins.
    pub fn snapshot(&self) -> SseStats {
        SseStats {
            accepted_connections: self.accepted_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            subscriptions: self.subscriptions.load(Ordering::Relaxed),
            active_subscriptions: self.active_subscriptions.load(Ordering::Relaxed),
            unsubscribed: self.unsubscribed.load(Ordering::Relaxed),
            auth_rejections: self.auth_rejections.load(Ordering::Relaxed),
            rejected_requests: self.rejected_requests.load(Ordering::Relaxed),
            capacity_rejections: self.capacity_rejections.load(Ordering::Relaxed),
            queue_overflows: self.queue_overflows.load(Ordering::Relaxed),
            dropped_events: self.dropped_events.load(Ordering::Relaxed),
            invalid_events: self.invalid_events.load(Ordering::Relaxed),
            write_failures: self.write_failures.load(Ordering::Relaxed),
            sent_events: self.sent_events.load(Ordering::Relaxed),
            heartbeats: self.heartbeats.load(Ordering::Relaxed),
        }
    }
}

/// Serve a caller-bound loopback listener until Daemon::stop_flag is set.
/// None permits unauthenticated loopback use; Some requires an exact Bearer
/// token (empty tokens are a configuration error). Query tokens are unsupported.
pub fn serve_sse(
    daemon: Arc<Daemon>,
    listener: TcpListener,
    access_token: Option<String>,
) -> io::Result<SseStats> {
    serve_sse_with_config(daemon, listener, access_token, SseConfig::default())
}

pub fn serve_sse_with_config(
    daemon: Arc<Daemon>,
    listener: TcpListener,
    access_token: Option<String>,
    config: SseConfig,
) -> io::Result<SseStats> {
    if !listener.local_addr()?.ip().is_loopback()
        || config.max_clients == 0
        || config.max_header_bytes < 4
        || config.max_event_bytes == 0
        || config.header_timeout.is_zero()
        || config.write_timeout.is_zero()
        || config.heartbeat_interval.is_zero()
        || [
            config.header_timeout,
            config.write_timeout,
            config.heartbeat_interval,
        ]
        .iter()
        .any(|duration| Instant::now().checked_add(*duration).is_none())
        || access_token.as_ref().is_some_and(|token| {
            token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic())
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid SSE configuration or non-loopback listener",
        ));
    }
    listener.set_nonblocking(true)?;
    let stop = daemon.stop_flag();
    let local_stop = Arc::new(AtomicBool::new(false));
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut accept_error = None;
    while !stop.load(Ordering::Relaxed) {
        reap_workers(&mut workers);
        match listener.accept() {
            Ok((mut stream, peer)) => {
                // Admission is owned by this accept loop, not a racy gauge.
                if !peer.ip().is_loopback() || workers.len() >= config.max_clients {
                    config
                        .metrics
                        .capacity_rejections
                        .fetch_add(1, Ordering::Relaxed);
                    // A busy peer cannot stall the accept loop: one nonblocking
                    // best-effort response, then close without allocating a worker.
                    if stream.set_nonblocking(true).is_ok() {
                        let _ = stream.write(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
                    }
                    continue;
                }
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                let _ = stream.set_nodelay(true);
                config
                    .metrics
                    .accepted_connections
                    .fetch_add(1, Ordering::Relaxed);
                config
                    .metrics
                    .active_connections
                    .fetch_add(1, Ordering::Relaxed);
                let daemon = Arc::clone(&daemon);
                let token = access_token.clone();
                let config = config.clone();
                let local_stop = Arc::clone(&local_stop);
                workers.push(thread::spawn(move || {
                    let _slot = ClientSlot(Arc::clone(&config.metrics));
                    serve_connection(&mut stream, &daemon, token.as_deref(), &config, local_stop);
                    let _ = stream.shutdown(Shutdown::Both);
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                accept_error = Some(error);
                break;
            }
        }
    }
    // Cancel workers on listener failure without stopping the shared daemon.
    local_stop.store(true, Ordering::Relaxed);
    for worker in workers {
        let _ = worker.join();
    }
    match accept_error {
        Some(error) => Err(error),
        None => Ok(config.metrics.snapshot()),
    }
}

fn reap_workers(workers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let _ = workers.swap_remove(index).join();
        } else {
            index += 1;
        }
    }
}

struct ClientSlot(Arc<SseMetrics>);
impl Drop for ClientSlot {
    fn drop(&mut self) {
        self.0.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Subscription<'a> {
    daemon: &'a Daemon,
    id: u64,
    metrics: Arc<SseMetrics>,
}
impl Drop for Subscription<'_> {
    fn drop(&mut self) {
        if let Some(dropped) = self.daemon.onebot_event_subscriber_dropped(self.id) {
            if dropped > 0 {
                self.metrics.queue_overflows.fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .dropped_events
                    .fetch_add(dropped, Ordering::Relaxed);
            }
        }
        self.daemon.unsubscribe_onebot_events(self.id);
        self.metrics
            .active_subscriptions
            .fetch_sub(1, Ordering::Relaxed);
        self.metrics.unsubscribed.fetch_add(1, Ordering::Relaxed);
    }
}

struct WorkerStop {
    daemon: Arc<AtomicBool>,
    local: Arc<AtomicBool>,
}

impl WorkerStop {
    fn is_stopped(&self) -> bool {
        self.daemon.load(Ordering::Relaxed) || self.local.load(Ordering::Relaxed)
    }
}

fn serve_connection(
    stream: &mut TcpStream,
    daemon: &Daemon,
    token: Option<&str>,
    config: &SseConfig,
    local: Arc<AtomicBool>,
) {
    let stop = WorkerStop {
        daemon: daemon.stop_flag(),
        local,
    };
    let request = read_request(stream, config, &stop);
    let status = match request {
        Ok(request) => validate_request(&request, token),
        Err(status) => Err(status),
    };
    if let Err(status) = status {
        config
            .metrics
            .rejected_requests
            .fetch_add(1, Ordering::Relaxed);
        if status == 401 {
            config
                .metrics
                .auth_rejections
                .fetch_add(1, Ordering::Relaxed);
        }
        let extra = match status {
            401 => "WWW-Authenticate: Bearer\r\n",
            405 => "Allow: GET\r\n",
            _ => "",
        };
        let reason = match status {
            401 => "Unauthorized",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            431 => "Request Header Fields Too Large",
            _ => "Bad Request",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nConnection: close\r\n{extra}Content-Length: 0\r\n\r\n"
        );
        let _ = write_bounded(
            stream,
            response.as_bytes(),
            config.write_timeout,
            &stop,
            None,
        );
        return;
    }
    if stop.is_stopped() {
        return;
    }
    let (id, events) = daemon.subscribe_onebot_events("http-sse");
    let _subscription = Subscription {
        daemon,
        id,
        metrics: Arc::clone(&config.metrics),
    };
    config.metrics.subscriptions.fetch_add(1, Ordering::Relaxed);
    config
        .metrics
        .active_subscriptions
        .fetch_add(1, Ordering::Relaxed);
    // This explicit header is informational, not a fabricated replay capability.
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\nX-Caligo-SSE-Replay: unsupported\r\n\r\n";
    if write_bounded(
        stream,
        response,
        config.write_timeout,
        &stop,
        Some((daemon, id)),
    )
    .is_err()
    {
        config
            .metrics
            .write_failures
            .fetch_add(1, Ordering::Relaxed);
        return;
    }
    pump_events(stream, daemon, id, events, config, &stop);
}

fn pump_events(
    stream: &mut TcpStream,
    daemon: &Daemon,
    id: u64,
    events: mpsc::Receiver<String>,
    config: &SseConfig,
    stop: &WorkerStop,
) {
    let mut last_write = Instant::now();
    while !stop.is_stopped() {
        match daemon.onebot_event_subscriber_dropped(id) {
            Some(0) => {}
            Some(_) => break, // Subscription::drop records loss even after a failed write.
            None => break,
        }
        // EOF unregisters idle clients promptly. Client bytes after the GET are
        // unsupported (no pipelining/request body); never parse a second request.
        let mut byte = [0];
        match stream.peek(&mut byte) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        let frame = match events.try_recv() {
            Ok(raw) => match event_frame(&raw, config.max_event_bytes) {
                Ok(frame) => Some((frame, false)),
                Err(_) => {
                    config
                        .metrics
                        .invalid_events
                        .fetch_add(1, Ordering::Relaxed);
                    break;
                }
            },
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) if last_write.elapsed() >= config.heartbeat_interval => {
                Some((b": heartbeat\n\n".to_vec(), true))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        };
        if let Some((frame, heartbeat)) = frame {
            if write_bounded(
                stream,
                &frame,
                config.write_timeout,
                stop,
                Some((daemon, id)),
            )
            .is_err()
            {
                config
                    .metrics
                    .write_failures
                    .fetch_add(1, Ordering::Relaxed);
                break;
            }
            if heartbeat {
                config.metrics.heartbeats.fetch_add(1, Ordering::Relaxed);
            } else {
                config.metrics.sent_events.fetch_add(1, Ordering::Relaxed);
            }
            last_write = Instant::now();
        } else {
            thread::sleep(POLL);
        }
    }
}

fn event_frame(raw: &str, limit: usize) -> io::Result<Vec<u8>> {
    if raw.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SSE event too large",
        ));
    }
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SSE JSON"))?;
    if !value.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SSE event must be an object",
        ));
    }
    // Preserve existing compact event bytes. Pretty JSON's physical line breaks
    // must be serialized away before adding the SSE envelope; escaped newlines
    // inside JSON strings stay escaped. Trailing injected SSE fields fail parsing.
    let json = if raw.contains(['\r', '\n']) {
        serde_json::to_string(&value)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid SSE JSON"))?
    } else {
        raw.to_owned()
    };
    if json.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SSE event too large",
        ));
    }
    let mut frame = Vec::with_capacity(json.len() + 8);
    frame.extend_from_slice(b"data: ");
    frame.extend_from_slice(json.as_bytes());
    frame.extend_from_slice(b"\n\n");
    Ok(frame)
}

fn read_request(
    stream: &mut TcpStream,
    config: &SseConfig,
    stop: &WorkerStop,
) -> Result<Vec<u8>, u16> {
    let deadline = Instant::now() + config.header_timeout;
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        if stop.is_stopped() || Instant::now() >= deadline {
            return Err(408);
        }
        if request.len() >= config.max_header_bytes {
            return Err(431);
        }
        let room = buffer.len().min(config.max_header_bytes - request.len());
        match stream.read(&mut buffer[..room]) {
            Ok(0) => return Err(400),
            Ok(count) => {
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    if end + 4 != request.len() {
                        return Err(400);
                    }
                    return Ok(request);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(400),
        }
    }
}

fn validate_request(request: &[u8], token: Option<&str>) -> Result<(), u16> {
    let request = std::str::from_utf8(request).map_err(|_| 400u16)?;
    let mut lines = request
        .strip_suffix("\r\n\r\n")
        .ok_or(400u16)?
        .split("\r\n");
    let first = lines.next().ok_or(400u16)?;
    let parts: Vec<_> = first.split(' ').collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" || parts.iter().any(|part| part.is_empty()) {
        return Err(400);
    }
    let mut names = HashSet::new();
    let mut authorization = None;
    let mut replay = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
            || value
                .bytes()
                .any(|byte| !(byte == b'\t' || (0x20..=0x7e).contains(&byte)))
            || names.len() >= 64
        {
            return Err(400);
        }
        let name = name.to_ascii_lowercase();
        if !names.insert(name.clone()) {
            return Err(400);
        }
        let value = value.trim_matches([' ', '\t']);
        match name.as_str() {
            "host" if value.is_empty() => return Err(400),
            "authorization" => authorization = Some(value),
            "last-event-id" => replay = true,
            "transfer-encoding" => return Err(400),
            "content-length" if value != "0" => return Err(400),
            _ => {}
        }
    }
    if !names.contains("host") {
        return Err(400);
    }
    if let Some(expected) = token {
        let authorized = authorization
            .and_then(|value| value.split_once(' '))
            .is_some_and(|(scheme, supplied)| {
                scheme.eq_ignore_ascii_case("bearer") && supplied == expected
            });
        if !authorized {
            return Err(401);
        }
    }
    if parts[0] != "GET" {
        return Err(405);
    }
    if parts[1] != "/_events" {
        return Err(404);
    }
    if replay {
        return Err(400);
    }
    Ok(())
}

fn write_bounded<W: Write>(
    stream: &mut W,
    mut bytes: &[u8],
    timeout: Duration,
    stop: &WorkerStop,
    subscription: Option<(&Daemon, u64)>,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    while !bytes.is_empty() {
        if subscription
            .is_some_and(|(daemon, id)| daemon.onebot_event_subscriber_dropped(id) != Some(0))
        {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "SSE event queue lost data",
            ));
        }
        if stop.is_stopped() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "SSE stopped"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "SSE write timeout"));
        }
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "SSE disconnected")),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BackpressuredWriter {
        writes: usize,
    }

    impl Write for BackpressuredWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.writes == 1 {
                return Ok(bytes.len().min(1));
            }
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn partial_write_then_backpressure_has_total_deadline_and_stop_interrupts() {
        let mut stream = BackpressuredWriter { writes: 0 };
        let stop = WorkerStop {
            daemon: Arc::new(AtomicBool::new(false)),
            local: Arc::new(AtomicBool::new(false)),
        };
        let started = Instant::now();
        let result = write_bounded(
            &mut stream,
            b"pending bytes",
            Duration::from_millis(40),
            &stop,
            None,
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(40));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(stream.writes > 1);
        let writes_before_stop = stream.writes;
        stop.daemon.store(true, Ordering::Relaxed);
        let result = write_bounded(&mut stream, b"x", Duration::from_secs(30), &stop, None);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(stream.writes, writes_before_stop);
    }

    #[test]
    fn prefilled_subscription_loss_aborts_before_write_and_unregisters() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(format!("caligo-sse-loss-{unique}"));
        std::fs::create_dir_all(&directory).unwrap();
        let daemon = Daemon::start(crate::daemon::DaemonConfig {
            pipe_prefix: format!("caligo-sse-loss-{unique}"),
            journal_path: directory.join("journal.jsonl"),
            module_baseline: "offline".into(),
            account: "10001".into(),
            core_build: crate::CORE_BUILD.into(),
            expect_client_pid: std::process::id(),
            runtime: crate::runtime::RuntimeConfig::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        })
        .unwrap();
        let (id, _unread) = daemon.subscribe_onebot_events("sse-loss-fixture");
        for _ in 0..crate::onebot_events::SUBSCRIBER_QUEUE_CAP + 3 {
            daemon.publish_onebot_ws_event_json(r#"{"post_type":"message"}"#);
        }
        assert_eq!(daemon.onebot_event_subscriber_dropped(id), Some(3));
        let metrics = Arc::new(SseMetrics::default());
        metrics.active_subscriptions.store(1, Ordering::Relaxed);
        let subscription = Subscription {
            daemon: &daemon,
            id,
            metrics: Arc::clone(&metrics),
        };
        let stop = WorkerStop {
            daemon: daemon.stop_flag(),
            local: Arc::new(AtomicBool::new(false)),
        };
        let mut output = Vec::new();
        let error = write_bounded(
            &mut output,
            b"data: {}\n\n",
            Duration::from_secs(1),
            &stop,
            Some((&daemon, id)),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert!(
            output.is_empty(),
            "a known gap must close before another frame"
        );
        drop(subscription);
        assert_eq!(daemon.onebot_event_subscriber_dropped(id), None);
        let stats = metrics.snapshot();
        assert_eq!(stats.queue_overflows, 1);
        assert_eq!(stats.dropped_events, 3);
        assert_eq!(stats.unsubscribed, 1);
        assert_eq!(stats.active_subscriptions, 0);
        drop(daemon);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn json_frame_preserves_compact_bytes_and_cannot_inject_sse_fields() {
        let raw = r#"{"raw_message":"hello\n\nid: forged\r\nevent: forged","message_id":17}"#;
        assert_eq!(
            event_frame(raw, 4096).unwrap(),
            format!("data: {raw}\n\n").as_bytes()
        );
        let pretty = "{\n  \"raw_message\": \"hello\\nworld\"\r\n}\r\n";
        let frame = event_frame(pretty, 4096).unwrap();
        assert_eq!(frame.iter().filter(|byte| **byte == b'\n').count(), 2);
        assert!(!frame.contains(&b'\r'));
        let json: Value = serde_json::from_slice(&frame[6..frame.len() - 2]).unwrap();
        assert_eq!(json["raw_message"], "hello\nworld");
        for invalid in [
            "{}\n\nid: injected",
            "{}\nevent: x",
            "{broken",
            "null",
            "[]",
            "{} {}",
        ] {
            assert!(event_frame(invalid, 4096).is_err(), "{invalid:?}");
        }
        assert!(event_frame("{}", 1).is_err());
    }
}
