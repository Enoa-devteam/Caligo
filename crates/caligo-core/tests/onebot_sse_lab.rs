//! Offline TCP/real Daemon bus tests; no QQ process or reference code executes.
#![cfg(windows)]

use caligo_core::daemon::{BridgeClient, Daemon, DaemonConfig};
use caligo_core::ipc::{BridgeMsg, CoreToBridgeMsg, EventPayload};
use caligo_core::onebot_sse::{serve_sse_with_config, SseConfig, SseMetrics, SseStats};
use caligo_core::runtime::RuntimeConfig;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Lab {
    daemon: Arc<Daemon>,
    address: SocketAddr,
    metrics: Arc<SseMetrics>,
    server: Option<JoinHandle<std::io::Result<SseStats>>>,
    runner: Option<JoinHandle<Result<(), caligo_core::daemon::SessionError>>>,
    directory: PathBuf,
}

impl Lab {
    fn start(mut config: SseConfig) -> Self {
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(format!("caligo-sse-{unique}"));
        std::fs::create_dir(&directory).unwrap();
        let daemon = Daemon::start(DaemonConfig {
            pipe_prefix: format!("caligo-sse-{unique}"),
            journal_path: directory.join("journal.jsonl"),
            module_baseline: "sse-offline-fixture".into(),
            account: "10001".into(),
            core_build: caligo_core::CORE_BUILD.into(),
            expect_client_pid: std::process::id(),
            runtime: RuntimeConfig::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        })
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        config.metrics = Arc::new(SseMetrics::default());
        let metrics = Arc::clone(&config.metrics);
        let server_daemon = Arc::clone(&daemon);
        let server = thread::spawn(move || {
            serve_sse_with_config(server_daemon, listener, Some("lab-token".into()), config)
        });
        Self {
            daemon,
            address,
            metrics,
            server: Some(server),
            runner: None,
            directory,
        }
    }

    fn request(&self, raw: &[u8]) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(raw).unwrap();
        BufReader::new(stream)
    }

    fn client(&self) -> BufReader<TcpStream> {
        let mut reader = self.request(
            b"GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\n\r\n",
        );
        let header = headers(&mut reader);
        assert!(header.starts_with("HTTP/1.1 200 OK\r\n"), "{header}");
        assert!(header.contains("Content-Type: text/event-stream\r\n"));
        assert!(header.contains("Cache-Control: no-cache\r\n"));
        assert!(header.contains("X-Caligo-SSE-Replay: unsupported\r\n"));
        reader
    }

    fn bridge(&mut self) -> BridgeClient {
        let daemon = Arc::clone(&self.daemon);
        self.runner = Some(thread::spawn(move || daemon.run()));
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Ok(bridge) = BridgeClient::connect(
                &self.daemon.pipe_names().0,
                &self.daemon.auth_token(),
                "sse-offline",
                1,
                "10001",
                "sse-offline-fixture",
                0x55,
                "2026-11-13T00:00:00Z",
            ) {
                return bridge;
            }
            assert!(
                Instant::now() < deadline,
                "offline bridge could not connect"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn finish(&mut self) -> SseStats {
        self.daemon.stop_flag().store(true, Ordering::Relaxed);
        let stats = self.server.take().unwrap().join().unwrap().unwrap();
        if let Some(runner) = self.runner.take() {
            runner.join().unwrap().unwrap();
        }
        stats
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        self.daemon.stop_flag().store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        if let Some(runner) = self.runner.take() {
            let _ = runner.join();
        }
        // Daemon may retain open journal handles on Windows; cleanup is best effort.
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn headers(reader: &mut BufReader<TcpStream>) -> String {
    let mut header = String::new();
    loop {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "EOF before response headers"
        );
        header.push_str(&line);
        if line == "\r\n" {
            return header;
        }
    }
}

fn frame(reader: &mut BufReader<TcpStream>) -> String {
    let mut result = String::new();
    loop {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "EOF before SSE frame"
        );
        result.push_str(&line);
        if line == "\n" {
            return result;
        }
    }
}

fn event_frame(reader: &mut BufReader<TcpStream>) -> String {
    loop {
        let value = frame(reader);
        if value.starts_with("data: ") {
            return value;
        }
        assert_eq!(value, ": heartbeat\n\n");
    }
}

fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !predicate() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

fn publish(bridge: &mut BridgeClient, sequence: u64, text: &str) {
    bridge
        .send(&BridgeMsg::Event {
            event: EventPayload {
                event_seq: sequence,
                session_generation: 1,
                session: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
                direction: "incoming".into(),
                sender: "synthetic-sender".into(),
                native_id: format!("sse-native-{sequence}"),
                text: text.into(),
                platform_time: Some(1234),
                wrapper_message: None,
                onebot_projection: Some(caligo_model::OneBotMessageProjection {
                    peer_id: 123456,
                    sender_user_id: 10002,
                    real_id: sequence as i32,
                    time: 1234,
                    sender_nickname: Some("synthetic".into()),
                    sub_type: Some("friend".into()),
                    font: Some(0),
                }),
                observed_at_unix_ms: 5678,
                source: "recv".into(),
            },
        })
        .unwrap();
    assert!(
        matches!(bridge.recv().unwrap(), CoreToBridgeMsg::EventAck { event_seq, suppressed: false } if event_seq == sequence)
    );
}

#[test]
fn bearer_auth_rejects_missing_wrong_raw_and_duplicate_credentials() {
    let mut lab = Lab::start(SseConfig::default());
    for authorization in [
        "",
        "Authorization: Bearer wrong\r\n",
        "Authorization: lab-token\r\n",
        "Authorization: Bearer lab-token\r\nAuthorization: Bearer lab-token\r\n",
    ] {
        let mut reader = lab.request(
            format!("GET /_events HTTP/1.1\r\nHost: localhost\r\n{authorization}\r\n").as_bytes(),
        );
        let status = if authorization.matches("Authorization:").count() == 2 {
            "400"
        } else {
            "401"
        };
        assert!(headers(&mut reader).starts_with(&format!("HTTP/1.1 {status} ")));
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        assert!(tail.is_empty());
    }
    let mut accepted = lab.request(
        b"GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: bearer lab-token\r\n\r\n",
    );
    assert!(headers(&mut accepted).starts_with("HTTP/1.1 200 "));
    accepted.get_mut().shutdown(Shutdown::Both).unwrap();
    wait_for(|| lab.metrics.snapshot().active_subscriptions == 0);
    let stats = lab.finish();
    assert_eq!(stats.auth_rejections, 3);
    assert_eq!(stats.subscriptions, 1);
    assert_eq!(stats.unsubscribed, 1);
}

#[test]
fn two_clients_receive_exact_general_bus_bytes_in_order_and_no_replay() {
    let mut lab = Lab::start(SseConfig::default());
    let mut bridge = lab.bridge();
    let (other_id, other_events) = lab.daemon.subscribe_onebot_events("sse-byte-comparison");
    let mut first = lab.client();
    let mut second = lab.client();
    for (sequence, text) in [
        (1, "literal\n\nid: forged\r\nevent: forged 中文"),
        (2, "second event"),
    ] {
        publish(&mut bridge, sequence, text);
        let raw = other_events.recv_timeout(Duration::from_secs(3)).unwrap();
        let expected = format!("data: {raw}\n\n");
        assert_eq!(event_frame(&mut first), expected);
        assert_eq!(event_frame(&mut second), expected);
        assert_eq!(expected.matches('\n').count(), 2);
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["raw_message"], text);
        assert!(value["message_id"].as_i64().unwrap() > 0);
    }
    first.get_mut().shutdown(Shutdown::Both).unwrap();
    second.get_mut().shutdown(Shutdown::Both).unwrap();
    wait_for(|| lab.metrics.snapshot().active_subscriptions == 0);
    // A new subscriber receives only future events, never a fabricated replay.
    let mut third = lab.client();
    third
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(80)))
        .unwrap();
    let mut byte = [0];
    assert!(matches!(
        third.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    third
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    publish(&mut bridge, 3, "future only");
    let raw = other_events.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(event_frame(&mut third), format!("data: {raw}\n\n"));
    assert!(lab.daemon.unsubscribe_onebot_events(other_id));
    drop(bridge);
    let stats = lab.finish();
    assert_eq!(stats.subscriptions, 3);
    assert_eq!(stats.unsubscribed, 3);
    assert_eq!(stats.sent_events, 5);
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.active_subscriptions, 0);
}

#[test]
fn malformed_methods_headers_paths_and_replay_are_rejected_without_subscribing() {
    let mut lab = Lab::start(SseConfig::default());
    let cases = [
        ("POST /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\n\r\n", 405),
        ("GET /other HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\n\r\n", 404),
        ("GET /_events?access_token=lab-token HTTP/1.1\r\nHost: localhost\r\n\r\n", 401),
        ("GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nLast-Event-ID: 42\r\n\r\n", 400),
        ("GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nlast-event-id:\r\n\r\n", 400),
        ("GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nTransfer-Encoding: chunked\r\n\r\n", 400),
        ("GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nContent-Length: 1\r\n\r\n", 400),
        ("GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\n Broken: folded\r\n\r\n", 400),
        ("GET /_events HTTP/1.1\r\nAuthorization: Bearer lab-token\r\n\r\n", 400),
        ("GET  /_events HTTP/1.1\r\nHost: localhost\r\n\r\n", 400),
        ("GET /_events HTTP/1.0\r\nHost: localhost\r\n\r\n", 400),
    ];
    for (request, status) in cases {
        let mut reader = lab.request(request.as_bytes());
        let response = headers(&mut reader);
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status} ")),
            "{request:?} => {response:?}"
        );
    }
    let stats = lab.finish();
    assert_eq!(stats.subscriptions, 0);
    assert_eq!(stats.rejected_requests, cases.len() as u64);
}

#[test]
fn header_size_total_deadline_and_pending_connection_limit_are_bounded() {
    let mut lab = Lab::start(SseConfig {
        max_clients: 1,
        max_header_bytes: 128,
        header_timeout: Duration::from_millis(180),
        ..SseConfig::default()
    });
    let mut oversized =
        lab.request(format!("GET /_events HTTP/1.1\r\nX-Pad: {}", "x".repeat(128)).as_bytes());
    assert!(headers(&mut oversized).starts_with("HTTP/1.1 431 "));
    wait_for(|| lab.metrics.snapshot().active_connections == 0);
    let mut pending = lab.request(b"GET /_events HTTP/1.1\r\n");
    wait_for(|| lab.metrics.snapshot().active_connections == 1);
    let mut busy = lab.request(b"");
    assert!(headers(&mut busy).starts_with("HTTP/1.1 503 "));
    assert!(headers(&mut pending).starts_with("HTTP/1.1 408 "));
    wait_for(|| lab.metrics.snapshot().active_connections == 0);
    let client = lab.client();
    let stats = lab.finish();
    drop(client);
    assert_eq!(stats.capacity_rejections, 1);
    assert_eq!(stats.subscriptions, 1);
    assert_eq!(stats.unsubscribed, 1);
}

#[test]
fn idle_heartbeat_disconnect_unsubscribes_and_daemon_stop_cancels_partial_headers() {
    let mut lab = Lab::start(SseConfig {
        heartbeat_interval: Duration::from_millis(40),
        header_timeout: Duration::from_secs(30),
        ..SseConfig::default()
    });
    let mut client = lab.client();
    assert_eq!(frame(&mut client), ": heartbeat\n\n");
    client.get_mut().shutdown(Shutdown::Both).unwrap();
    wait_for(|| lab.metrics.snapshot().unsubscribed == 1);
    // IDs start at zero in this isolated daemon. This is an actual bus lookup,
    // not merely an assertion about our local active gauge.
    assert_eq!(lab.daemon.onebot_event_subscriber_dropped(0), None);
    let mut live = lab.client();
    let _partial = lab.request(b"GET /_events HTTP/1.1\r\n");
    wait_for(|| lab.metrics.snapshot().active_connections == 2);
    let started = Instant::now();
    let stats = lab.finish();
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut rest = String::new();
    live.read_to_string(&mut rest).unwrap();
    assert!(rest
        .lines()
        .all(|line| line.is_empty() || line == ": heartbeat"));
    assert_eq!(stats.active_subscriptions, 0);
    assert_eq!(stats.unsubscribed, 2);
    assert_eq!(lab.daemon.onebot_event_subscriber_dropped(1), None);
    assert!(stats.heartbeats >= 1);
}

#[test]
fn sse_waits_for_http_post_response_before_general_fanout() {
    use caligo_core::onebot_transport::{run_http_post_ready, HttpPostConfig};
    let mut lab = Lab::start(SseConfig::default());
    let mut bridge = lab.bridge();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let head = headers(&mut reader);
        let length: usize = head
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        seen_tx.send(body).unwrap();
        finish_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let daemon = Arc::clone(&lab.daemon);
    let post = thread::spawn(move || {
        run_http_post_ready(
            daemon,
            HttpPostConfig {
                url: format!("http://{address}/events"),
                secret: None,
                timeout: Duration::from_secs(3),
            },
            ready_tx,
        )
    });
    ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut client = lab.client();
    publish(&mut bridge, 1, "post before SSE");
    let body = seen_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    client
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(80)))
        .unwrap();
    assert!(matches!(
        client.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));
    finish_tx.send(()).unwrap();
    client
        .get_mut()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert_eq!(
        event_frame(&mut client),
        format!("data: {}\n\n", String::from_utf8(body).unwrap())
    );
    receiver.join().unwrap();
    drop(bridge);
    lab.finish();
    assert_eq!(post.join().unwrap(), 1);
}

#[test]
fn oversized_event_disconnects_and_unregisters_with_diagnostic_counter() {
    let mut lab = Lab::start(SseConfig {
        max_event_bytes: 16,
        ..SseConfig::default()
    });
    let mut bridge = lab.bridge();
    let mut client = lab.client();
    publish(&mut bridge, 1, "oversized fixture");
    wait_for(|| lab.metrics.snapshot().unsubscribed == 1);
    assert_eq!(lab.daemon.onebot_event_subscriber_dropped(0), None);
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
    drop(bridge);
    let stats = lab.finish();
    assert_eq!(stats.invalid_events, 1);
    assert_eq!(stats.sent_events, 0);
}

#[test]
fn non_loopback_and_empty_bearer_configuration_fail_closed() {
    let mut lab = Lab::start(SseConfig::default());
    for (address, token) in [("0.0.0.0:0", None), ("127.0.0.1:0", Some(String::new()))] {
        let result = serve_sse_with_config(
            Arc::clone(&lab.daemon),
            TcpListener::bind(address).unwrap(),
            token,
            SseConfig::default(),
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }
    lab.finish();
}
