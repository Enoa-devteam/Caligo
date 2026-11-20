//! Real TCP and AccountManager/Windows daemon tests. No QQ process is accessed.
#![cfg(windows)]

use caligo_core::account_manager::{AccountManager, AccountSpec, ManagerConfig, PortConfig};
use caligo_core::daemon::BridgeClient;
use caligo_core::ipc::{BridgeMsg, CoreToBridgeMsg, EventPayload};
use caligo_core::management_http::{ManagementHttpConfig, ManagementHttpServer};
use caligo_model::capability::{HostQuery, LoginInfo, QueryCapability, QueryData, QueryOutcome, QueryScope};
use caligo_model::ipc_v3::CAPABILITY_QUERY_REVISION;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(feature = "research")]
#[allow(dead_code)]
#[path = "../../caligo-bridge/tests/support/owner_fixture.rs"]
mod owner_fixture;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
struct Lab {
    server: ManagementHttpServer,
    manager: Arc<Mutex<AccountManager>>,
    root: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("caligo-management-http-{unique}"));
        let config = ManagerConfig {
            version: 1,
            accounts: [10001, 10002]
                .into_iter()
                .map(|account| AccountSpec {
                    account,
                    data_dir: format!("account-{account}"),
                    pipe_prefix: format!("caligo-ui-{unique}-{account}"),
                    module_baseline: "offline-http-lab".into(),
                    expect_client_pid: std::process::id(),
                    ports: PortConfig::default(),
                })
                .collect(),
        };
        config.save_atomic(&root).unwrap();
        let manager = Arc::new(Mutex::new(AccountManager::new(&root, config).unwrap()));
        let server = ManagementHttpServer::start(
            ManagementHttpConfig {
                bind: "127.0.0.1:0".parse().unwrap(),
                bearer_token: TOKEN.into(),
            },
            Arc::clone(&manager),
        )
        .unwrap();
        Self {
            server,
            manager,
            root,
        }
    }
    fn raw(&self, request: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(self.server.local_addr()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        response(&mut stream)
    }
    fn request(&self, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
        let payload = body.unwrap_or("");
        self.raw(&format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}", self.server.local_addr(), payload.len()))
    }
    fn api(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let body = body.map(Value::to_string);
        let (code, text) = self.request(method, path, body.as_deref());
        (code, serde_json::from_str(&text).unwrap())
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        self.server.shutdown().unwrap();
        self.manager.lock().unwrap().shutdown().unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn response(stream: &mut TcpStream) -> (u16, String) {
    let mut bytes = Vec::new();
    loop {
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..index]).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            if bytes.len() >= index + 4 + length {
                assert_eq!(bytes.len(), index + 4 + length);
                break;
            }
        }
        let mut block = [0u8; 4096];
        let count = stream.read(&mut block).unwrap();
        assert_ne!(count, 0, "response closed before Content-Length completed");
        bytes.extend_from_slice(&block[..count]);
    }
    let text = String::from_utf8(bytes).unwrap();
    let (headers, body) = text.split_once("\r\n\r\n").unwrap();
    assert!(headers.contains("Cache-Control: no-store"));
    assert!(headers.contains("frame-ancestors 'none'"));
    let code = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, body.into())
}

#[test]
fn selected_account_logs_follow_durable_messages_and_login_verifies_actual_result() {
    let lab = Lab::new();
    let eq = json!({});
    let accounts = "/v1/accounts";
    let first = "/v1/accounts/10001";
    let messages = format!("{first}/messages");
    let verify = format!("{first}/verify-login");
    let start = format!("{first}/start");
    let stop = format!("{first}/stop");
    let exact_account = "9007199254740993";
    let registered = lab.api("POST", accounts, Some(&json!({"account":exact_account})));
    assert_eq!(registered.0, 200);
    assert_eq!(registered.1["account"], exact_account);
    assert_eq!(ManagerConfig::load(&lab.root).unwrap().accounts[2].account.to_string(), exact_account);
    assert_eq!(lab.api("POST", accounts, Some(&json!({"account":exact_account}))).0, 409);
    assert_eq!(lab.api("GET", &messages, None).0, 409);
    assert_eq!(lab.api("POST", &start, Some(&eq)).0, 200);
    assert_eq!(lab.manager.lock().unwrap().running_accounts(), vec![10001]);
    assert_eq!(lab.api("POST", &verify, Some(&eq)).0, 409);
    assert_eq!(lab.api("GET", &messages, None).1["messages"], json!([]));
    let daemon = lab.manager.lock().unwrap().daemon(10001).unwrap();
    let mut bridge = BridgeClient::connect(
        &daemon.pipe_names().0, &daemon.auth_token(), "management-lab", 1, "10001",
        "offline-http-lab", 12, "2026-11-15T00:00:00Z",
    ).unwrap();
    let scope = QueryScope {
        account: "10001".into(), host_nonce: 12, session_generation: 1,
        connection_epoch: daemon.connection_epoch(),
    };
    bridge.send(&BridgeMsg::QueryCapabilities {
        revision: CAPABILITY_QUERY_REVISION, scope,
        capabilities: vec![QueryCapability::LoginInfo],
    }).unwrap();
    bridge.send(&BridgeMsg::Event { event: EventPayload {
        event_seq: 1,
        session_generation: 1,
        session: json!({"account":"10001","kind":"group","peer":"20001"}),
        direction: "incoming".into(),
        sender: "synthetic".into(),
        native_id: "never-exposed-native-id".into(),
        text: "<img src=x onerror=alert(1)> 测试消息".into(),
        platform_time: Some(1700000000),
        wrapper_message: None,
        onebot_projection: Some(caligo_model::OneBotMessageProjection {
            peer_id: 20001, sender_user_id: 9007199254740993, real_id: 77,
            time: 1700000000, sender_nickname: Some("测试发送者".into()),
            sub_type: Some("normal".into()), font: Some(0),
        }),
        observed_at_unix_ms: 1700000000000,
        source: "recv".into(),
    }}).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::EventAck { suppressed: false, .. }));
    let (code, snapshot) = lab.api("GET", &messages, None);
    assert_eq!(code, 200);
    assert_eq!(snapshot, json!({"account":"10001", "messages":[{
        "message_id":1, "time":1700000000, "chat_type":"group", "peer":"20001",
        "sender":exact_account, "nickname":"测试发送者",
        "preview":"<img src=x onerror=alert(1)> 测试消息"
    }]}));
    let address = lab.server.local_addr();
    assert_eq!(lab.raw(&format!("GET {messages} HTTP/1.1\r\nHost: {address}\r\n\r\n")).0, 401);
    let other = "/v1/accounts/10002";
    assert_eq!(lab.api("POST", &format!("{other}/start"), Some(&eq)).0, 200);
    assert_eq!(lab.api("GET", &format!("{other}/messages"), None).1["messages"], json!([]));
    let host = std::thread::spawn(move || {
        for user_id in ["10002", "10001"] {
            let CoreToBridgeMsg::Query { request_id, scope, query: HostQuery::LoginInfo, .. } = bridge.recv().unwrap()
            else { panic!("expected current-login query") };
            bridge.send(&BridgeMsg::QueryResult {
                request_id, scope, outcome: QueryOutcome::Success(QueryData::LoginInfo(LoginInfo {
                    user_id: user_id.into(), nickname: "合成 QQ".into(), online: true
                })),
            }).unwrap();
        }
        bridge
    });
    assert_eq!(lab.api("POST", &verify, Some(&eq)).0, 409);
    let (code, verified) = lab.api("POST", &verify, Some(&eq));
    assert_eq!(code, 200);
    assert_eq!(verified["nickname"], "合成 QQ");
    assert_eq!(verified["connection_id"], lab.api("GET", accounts, None).1["accounts"][0]["connection_id"]);
    drop(host.join().unwrap());
    assert_eq!(lab.api("POST", &stop, Some(&eq)).0, 200);
    assert_eq!(lab.api("GET", &messages, None).0, 409);
    assert_eq!(lab.api("POST", &start, Some(&eq)).0, 200);
    assert_eq!(lab.api("GET", &messages, None).1, snapshot, "the log replays the account ledger");
    assert!(lab.api("GET", accounts, None).1["accounts"][0]["connection_id"].is_null());
}

#[cfg(feature = "research")]
#[test]
fn quiesced_account_rejects_new_sends_and_acks_inflight_result_across_reconnect() {
    use caligo_core::daemon::ControlClient;
    use caligo_core::ipc::{ControlMsg, CoreToControlMsg};
    use caligo_core::journal::{Journal, JournalRecord, SyncMode};
    use caligo_model::ipc_v3::OutcomePayload;
    use caligo_model::{SessionKey, SessionKind};

    let lab = Lab::new();
    let quiesce = "/v1/accounts/10001/quiesce-sends";
    assert_eq!(lab.api("POST", "/v1/accounts/10001/start", Some(&json!({}))).0, 200);
    let daemon = lab.manager.lock().unwrap().daemon(10001).unwrap();
    let (bridge_pipe, control_pipe) = daemon.pipe_names();
    let auth = daemon.auth_token();
    let connect_bridge = || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match BridgeClient::connect(
                &bridge_pipe, &auth, "quiesce-lab", 1, "10001",
                "offline-http-lab", 22, "2026-11-15T00:00:00Z",
            ) {
                Ok(bridge) => break bridge,
                Err(error) => {
                    assert!(Instant::now() < deadline, "bridge connection failed: {error}");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    };
    let mut bridge = connect_bridge();
    let connection = || lab.api("GET", "/v1/accounts", None).1["accounts"][0]["connection_id"].clone();
    let first = connection();
    assert!(first.is_string());
    assert_eq!(lab.api("POST", quiesce, Some(&json!({}))).0, 400);
    assert_eq!(lab.api("POST", quiesce, Some(&json!({"connection_id":"stale"}))).0, 409);

    let mut control = ControlClient::connect(&control_pipe, &auth, "quiesce-lab").unwrap();
    let send = |request_id: &str| ControlMsg::SendText {
        request_id: request_id.into(),
        target: json!({"account":"10001","kind":"private","peer":"20002"}),
        text: "synthetic quiesce sample".into(),
        deadline_ms: 30000,
    };
    assert!(matches!(control.request(send("already-dispatched")).unwrap(),
        CoreToControlMsg::SendAccepted { .. }));
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::Dispatch { request_id, wire_seq: 1, .. }
        if request_id == "already-dispatched"));

    let fence = json!({"connection_id":first});
    let address = lab.server.local_addr();
    let text = fence.to_string();
    assert_eq!(lab.raw(&format!("POST {quiesce} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}", text.len())).0, 401);
    for _ in 0..2 {
        let (code, body) = lab.api("POST", quiesce, Some(&fence));
        assert_eq!(code, 200);
        assert_eq!(body["status"], "send_admission_closed");
        assert_eq!(body["connection_id"], first);
    }
    let assert_rejected = |control: &mut ControlClient, request_id| {
        assert!(matches!(control.request(send(request_id)).unwrap(), CoreToControlMsg::Reject { reason }
            if reason == "SendAdmissionClosed"));
    };
    assert_rejected(&mut control, "after-fence");
    let target = SessionKey::new("10001", SessionKind::Private, "20002");
    assert!(daemon.submit_onebot_text(target, "blocked OneBot sample".into(), 30000)
        .unwrap_err().contains("SendAdmissionClosed"));
    bridge.send(&BridgeMsg::NativeStarted { request_id: "already-dispatched".into() }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::NativeStartedAck { request_id }
        if request_id == "already-dispatched"));

    drop(bridge);
    let mut bridge = connect_bridge();
    let second = connection();
    assert!(second.is_string());
    assert_ne!(first, second);
    assert_eq!(lab.api("POST", quiesce, Some(&fence)).0, 409);
    assert_rejected(&mut control, "after-reconnect");
    bridge.send(&BridgeMsg::SendResult {
        request_id: "already-dispatched".into(),
        notice_id: 7,
        outcome: OutcomePayload::Success { native_id: "synthetic-native".into() },
    }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::SendResultAck { request_id, notice_id: 7, terminal: true }
        if request_id == "already-dispatched"));
    assert_eq!(lab.api("POST", "/v1/accounts/10001/stop", Some(&json!({}))).0, 200);

    let (_, history) = Journal::open(lab.root.join("account-10001/journal.jsonl"), 1024 * 1024, SyncMode::Always).unwrap();
    let requests: Vec<_> = history.records.iter().filter_map(|record| match record {
        JournalRecord::Request(request) => Some((request.request_id.as_str(), request.state.as_str())),
        _ => None,
    }).collect();
    assert_eq!(requests, [
        ("already-dispatched", "Queued"),
        ("already-dispatched", "DispatchIntent"),
        ("already-dispatched", "NativeStarted"),
        ("already-dispatched", "ConfirmedSuccess"),
    ]);

    drop(bridge);
    drop(control);
    assert_eq!(lab.api("POST", "/v1/accounts/10001/start", Some(&json!({}))).0, 200);
    let _new_bridge = connect_bridge();
    let new_actor_connection = connection();
    assert!(new_actor_connection.is_string());
    assert_ne!(second, new_actor_connection);
    assert_eq!(lab.api("POST", quiesce, Some(&json!({"connection_id": second}))).0, 409);
}

#[cfg(feature = "research")]
#[test]
fn staged_stop_keeps_the_managed_pipe_open_until_owner_and_result_ack_complete() {
    use caligo_bridge::daemon_client::{run_worker, DaemonClientConfig, WorkerExit};
    use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
    use caligo_bridge::qq_entry::{
        append_stage_simple, arm_stop_observation_with_result_drain, bootstrap, close_daemon_owner,
        install_stop_observation_driver, retain_daemon_for_result_drain, wake_owner_pump, EntryConfig,
    };
    use caligo_bridge::resident::{Resident, ResidentLimits, ResultHandoffGate};
    use caligo_bridge::stop_observation::{Sample, StopIdentity};
    use caligo_core::daemon::ControlClient;
    use caligo_core::ipc::{ControlMsg, CoreToControlMsg};
    use owner_fixture::{close_once, fake_symbols, lab_reset, pump_once, FakeQq, FAKE_CLOSE_CALLS, LAB_WAIT, WAKE_NOTICE};
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::sync::mpsc;

    struct FiredHost {
        owner: std::thread::ThreadId,
        sends: Arc<AtomicUsize>,
    }
    impl HostAdapter for FiredHost {
        fn owner_thread_id(&self) -> u64 { 1 }
        fn current_thread_id(&self) -> u64 {
            if std::thread::current().id() == self.owner { 1 } else { 2 }
        }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
            Ok(match op {
                HostOp::ListenerAdd => HostOpResult::ListenerAdded { token: 7 },
                HostOp::ListenerRemove { token: 7 } => HostOpResult::ListenerRemoved,
                HostOp::SendText { request_id, .. } => {
                    self.sends.fetch_add(1, Ordering::Relaxed);
                    HostOpResult::SendFired { mid: request_id }
                }
                _ => panic!("unexpected fixture host call"),
            })
        }
    }

    lab_reset();
    let lab = Lab::new();
    let empty = json!({});
    assert_eq!(lab.api("POST", "/v1/accounts/10001/start", Some(&empty)).0, 200);
    let daemon = lab.manager.lock().unwrap().daemon(10001).unwrap();
    let (pipe_name, control_pipe) = daemon.pipe_names();
    let auth = daemon.auth_token();
    let cfg = DaemonClientConfig {
        pipe_name, auth_token: auth.clone(), account: "10001".into(),
        module_baseline: "offline-http-lab".into(), session_generation: 1,
        ..DaemonClientConfig::default()
    };
    let identity = StopIdentity {
        account: cfg.account.clone(), host_nonce: cfg.host_nonce, listener_generation: 1,
        wrapper_session: "nt_fixture".into(),
    };
    let report = lab.root.join("owner-entry.jsonl");
    let (_fake, qqnt_base, env, _) = FakeQq::new();
    let handle = bootstrap(&EntryConfig {
        qqnt_base, env, report_path: report.to_str().unwrap().into(),
    }, &fake_symbols()).unwrap();
    let sends = Arc::new(AtomicUsize::new(0));
    let resident = Arc::new(Resident::new(FiredHost {
        owner: std::thread::current().id(), sends: sends.clone(),
    }, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let stop = handle.daemon_stop_flag();
    stop.store(false, Ordering::Release);
    let worker_finished = Arc::new(AtomicBool::new(false));
    let (notice_tx, notice_rx) = mpsc::channel();
    let (_events_tx, events_rx) = mpsc::channel();
    retain_daemon_for_result_drain(
        resident.clone(), worker_finished.clone(), stop.clone(),
        Arc::new(ResultHandoffGate::default()), notice_tx,
    );
    let owner = resident.clone();
    handle.set_drain_hook(Box::new(move || close_daemon_owner(&owner)));
    let worker = {
        let r = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let result = run_worker(cfg, r, &wake_owner_pump, notice_rx, events_rx, stop);
            worker_finished.store(true, Ordering::Release);
            result
        })
    };

    let deadline = Instant::now() + LAB_WAIT;
    let connection_id = loop {
        let (_, accounts) = lab.api("GET", "/v1/accounts", None);
        if let Some(id) = accounts["accounts"][0]["connection_id"].as_str() {
            break id.to_string();
        }
        assert!(Instant::now() < deadline, "worker never bound to the account");
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut control = ControlClient::connect(&control_pipe, &auth, "staged-owner-lab").unwrap();
    let send = |id: &str| ControlMsg::SendText {
        request_id: id.into(), text: "synthetic staged stop".into(),
        target: json!({"account":"10001","kind":"private","peer":"20002"}), deadline_ms: 10000,
    };
    assert!(matches!(control.request(send("staged")).unwrap(), CoreToControlMsg::SendAccepted { .. }));
    let deadline = Instant::now() + LAB_WAIT;
    while resident.pending_ingress() == 0 {
        assert!(Instant::now() < deadline, "dispatch did not reach resident");
        std::thread::sleep(Duration::from_millis(5));
    }
    resident.drain().unwrap();
    assert_eq!(sends.load(Ordering::Relaxed), 1);
    append_stage_simple("text_send_trace", true, r#"{"events":[{"seq":1,"stage":"registered"},{"seq":1,"stage":"submit_enter"},{"seq":1,"stage":"submit_return"}],"generation":1,"incomplete":false}"#);
    let (code, response) = lab.api("POST", "/v1/accounts/10001/quiesce-sends", Some(&json!({"connection_id":connection_id})));
    assert_eq!(code, 200);
    assert_eq!(response["status"], "send_admission_closed");
    assert!(matches!(control.request(send("after-quiesce")).unwrap(),
        CoreToControlMsg::Reject { reason } if reason == "SendAdmissionClosed"));

    let journal = lab.root.join("account-10001/journal.jsonl");
    let before = lab.root.join("before-stop.bin");
    let after = lab.root.join("after-stop.bin");
    std::fs::copy(&journal, &before).unwrap();
    let observed = identity.clone();
    install_stop_observation_driver(Box::new(move || Some(observed.clone())), Box::new(|_| {
        // Synthetic active callback metadata. Actual STOP and sample timing
        // come from the driver. The fixture has no late callback.
        append_stage_simple("js_callback_observation", true, &json!({"events":[
            {"association":"matched","current":true,"generation":1,"kind":"matched_add","order":1,"phase":"active","seq":1},
            {"association":"none","current":null,"generation":1,"kind":"stop_begin","order":2,"phase":"stopping","seq":0},
            {"association":"none","current":null,"generation":1,"kind":"stop_end","order":3,"phase":"stopped","seq":0}
        ],"generation":1,"incomplete":false}).to_string());
        Ok(Sample { callbacks_seen: 3 })
    })).unwrap();
    arm_stop_observation_with_result_drain(identity, 1000, 5000).unwrap();
    let (wake_tx, wake_rx) = mpsc::channel();
    *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
    let closer = std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(5000));
    wake_rx.recv_timeout(LAB_WAIT).unwrap();
    assert!(!stop.load(Ordering::Acquire));
    pump_once();
    while FAKE_CLOSE_CALLS.load(Ordering::Acquire) == 0 {
        wake_rx.recv_timeout(LAB_WAIT).unwrap();
        pump_once();
    }
    close_once();
    assert_eq!(closer.join().unwrap(), 0);
    *WAKE_NOTICE.lock().unwrap() = None;
    let (counts, exit) = worker.join().unwrap();
    assert_eq!(exit, WorkerExit::LocalStop);
    assert_eq!(counts.results_sent, 1);
    assert_eq!(resident.request_state("staged"), Some("delivery_unknown"));
    assert_eq!(resident.counters().listener_removes, 1);
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Acquire), 1);
    assert_eq!(sends.load(Ordering::Relaxed), 1);
    assert!(!resident.has_unacked_results());
    assert!(lab.manager.lock().unwrap().running_accounts().contains(&10001));
    std::fs::copy(&journal, &after).unwrap();

    let log = std::fs::read_to_string(&report).unwrap();
    let rows: Vec<Value> = log.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let at = |label| rows.iter().find(|r| r["stage"] == "stop_observation" && r["detail"] == label)
        .unwrap()["ts"].as_u64().unwrap();
    let elapsed = at("final_snapshot") - at("listener_stopped");
    assert!(elapsed >= 1000);
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/analyze-text-stop-evidence.mjs");
    let analysis = std::process::Command::new("node")
        .arg(script).arg("--staged").arg(&report).arg(&after).arg(&before).output().unwrap();
    assert_eq!(analysis.status.code(), Some(2));
    let result: Value = serde_json::from_slice(&analysis.stdout).unwrap();
    assert_eq!(result["journal"], "staged_close", "{result}");
    assert_eq!(result["request_state"], "DeliveryUnknown");
    assert_eq!(result["reason"], "missing_late_closure");
    assert_eq!(lab.api("POST", "/v1/shutdown", Some(&empty)).0, 200);
    assert!(lab.manager.lock().unwrap().running_accounts().is_empty());
    assert_eq!(std::fs::read(&journal).unwrap(), std::fs::read(&after).unwrap());
    println!("fixture: manager quiesced, owner observed {elapsed} ms, one submission, worker ACKed staged_close, manager stopped last; late callback unknown");
}

#[test]
fn two_real_accounts_start_stop_and_config_readback_follow_current_config() {
    let lab = Lab::new();
    let (code, accounts) = lab.api("GET", "/v1/accounts", None);
    assert_eq!(code, 200);
    assert_eq!(accounts["accounts"].as_array().unwrap().len(), 2);
    assert_eq!(accounts["accounts"][0]["running"], false);
    assert!(accounts["accounts"][0]["online"].is_null());
    let (code, mut cfg) = lab.api("GET", "/v1/config", None);
    assert_eq!(code, 200);
    cfg["accounts"][0]["module_baseline"] = json!("updated-before-start");
    let (code, saved) = lab.api("PUT", "/v1/config", Some(&cfg));
    assert_eq!(code, 200, "{saved}");
    assert_eq!(saved, cfg);
    assert_eq!(json!(ManagerConfig::load(&lab.root).unwrap()), cfg);
    for id in [10001, 10002] {
        let (code, result) = lab.api(
            "POST",
            &format!("/v1/accounts/{id}/start"),
            Some(&json!({})),
        );
        assert_eq!(code, 200, "{result}");
        assert_eq!(result["running"], true);
        assert!(
            result["online"].is_null(),
            "running must not imply QQ login"
        );
    }
    assert_eq!(
        lab.manager
            .lock()
            .unwrap()
            .daemon(10001)
            .unwrap()
            .config
            .module_baseline,
        "updated-before-start"
    );
    assert_eq!(lab.api("PUT", "/v1/config", Some(&cfg)).0, 409);
    assert_eq!(
        lab.api("POST", "/v1/accounts/10001/start", Some(&json!({})))
            .0,
        409
    );
    let (code, capabilities) = lab.api("GET", "/v1/accounts/10001/capabilities", None);
    assert_eq!(code, 200);
    assert_eq!(
        capabilities["declared"],
        json!(caligo_core::onebot::ACTION_REGISTRY)
    );
    assert_eq!(capabilities["backend_state"], "unknown");
    assert!(capabilities["advertised"].is_null());
    assert!(capabilities.get("enabled").is_none());
    assert_eq!(
        lab.api("POST", "/v1/accounts/10001/stop", Some(&json!({})))
            .0,
        200
    );
    assert_eq!(lab.manager.lock().unwrap().running_accounts(), vec![10002]);
    assert_eq!(
        lab.api("POST", "/v1/accounts/10002/stop", Some(&json!({})))
            .0,
        200
    );
    let (code, mut cfg) = lab.api("GET", "/v1/config", None);
    assert_eq!(code, 200);
    cfg["accounts"][1]["module_baseline"] = json!("updated-after-stop");
    assert_eq!(lab.api("PUT", "/v1/config", Some(&cfg)).0, 200);
    assert_eq!(
        lab.api("POST", "/v1/accounts/10002/start", Some(&json!({})))
            .0,
        200
    );
    assert_eq!(
        lab.manager
            .lock()
            .unwrap()
            .daemon(10002)
            .unwrap()
            .config
            .module_baseline,
        "updated-after-stop"
    );
    let (_, config_body) = lab.request("GET", "/v1/config", None);
    assert!(!config_body.contains(TOKEN));
    assert!(!config_body.contains("auth.token"));
    assert!(!config_body.contains("root"));
}

#[test]
fn strict_config_validation_and_disk_failure_never_report_saved() {
    let lab = Lab::new();
    let (_, cfg) = lab.api("GET", "/v1/config", None);
    let mut invalid = cfg.clone();
    invalid["token"] = json!("forbidden");
    assert_eq!(lab.api("PUT", "/v1/config", Some(&invalid)).0, 400);
    let mut invalid = cfg.clone();
    invalid["accounts"][1]["data_dir"] = json!("../escape");
    assert_eq!(lab.api("PUT", "/v1/config", Some(&invalid)).0, 400);
    let mut invalid = cfg.clone();
    invalid["accounts"][1]["account"] = invalid["accounts"][0]["account"].clone();
    assert_eq!(lab.api("PUT", "/v1/config", Some(&invalid)).0, 400);
    let mut invalid = cfg.clone();
    invalid["accounts"][0]["ports"]["http"] = json!(lab.server.local_addr().port());
    assert_eq!(lab.api("PUT", "/v1/config", Some(&invalid)).0, 409);
    assert_eq!(
        lab.request(
            "PUT",
            "/v1/config",
            Some("{\"version\":1,\"version\":1,\"accounts\":[]}")
        )
        .0,
        400
    );
    assert_eq!(lab.api("GET", "/v1/config", None).1, cfg);
    let path = lab.root.join("accounts.json");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let mut changed = cfg.clone();
    changed["accounts"][0]["module_baseline"] = json!("must-not-be-applied");
    assert_eq!(lab.api("PUT", "/v1/config", Some(&changed)).0, 500);
    assert_eq!(lab.api("GET", "/v1/config", None).1, cfg);
    assert_eq!(json!(ManagerConfig::load(&lab.root).unwrap()), cfg);
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
}

#[test]
fn auth_origin_host_query_and_write_contracts_are_enforced() {
    let lab = Lab::new();
    let address = lab.server.local_addr();
    assert_eq!(
        lab.raw(&format!(
            "GET /v1/accounts HTTP/1.1\r\nHost: {address}\r\n\r\n"
        ))
        .0,
        401
    );
    assert_eq!(
        lab.raw(&format!(
            "GET /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\n\r\n",
            "f".repeat(64)
        ))
        .0,
        401
    );
    for origin in ["null", "https://evil.example", "http://localhost:1"] {
        assert_eq!(lab.raw(&format!("GET /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nOrigin: {origin}\r\n\r\n")).0, 403);
    }
    assert_eq!(lab.raw(&format!("GET /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nOrigin: http://{address}\r\n\r\n")).0, 200);
    assert_eq!(lab.raw(&format!("GET /v1/config HTTP/1.1\r\nHost: evil.example\r\nAuthorization: Bearer {TOKEN}\r\n\r\n")).0, 403);
    assert_eq!(lab.raw(&format!("GET /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nSec-Fetch-Site: cross-site\r\n\r\n")).0, 403);
    assert_eq!(
        lab.request("GET", &format!("/v1/config?token={TOKEN}"), None)
            .0,
        400
    );
    assert_eq!(lab.request("GET", "/v1/accounts/10001/start", None).0, 405);
    assert_eq!(
        lab.api(
            "POST",
            "/v1/accounts/10001/start",
            Some(&json!({"token":TOKEN}))
        )
        .0,
        400
    );
    assert_eq!(
        lab.api("POST", "/v1/accounts/99999/start", Some(&json!({})))
            .0,
        404
    );
    assert_eq!(lab.raw(&format!("POST /v1/accounts/10001/start HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 2\r\nContent-Type: text/plain\r\n\r\n{{}}")).0, 415);
    assert_eq!(lab.raw(&format!("PUT /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\n\r\n")).0, 411);
    assert_eq!(lab.raw(&format!("GET /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n")).0, 400);
    assert_eq!(lab.raw(&format!("GET /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nTransfer-Encoding: chunked\r\n\r\n")).0, 400);
    assert_eq!(
        lab.manager.lock().unwrap().running_accounts(),
        Vec::<u64>::new()
    );
    assert!(ManagementHttpServer::start(
        ManagementHttpConfig {
            bind: "0.0.0.0:0".parse().unwrap(),
            bearer_token: TOKEN.into()
        },
        Arc::clone(&lab.manager)
    )
    .is_err());
    assert!(ManagementHttpServer::start(
        ManagementHttpConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            bearer_token: "short".into()
        },
        Arc::clone(&lab.manager)
    )
    .is_err());
}

#[test]
fn static_ui_uses_real_routes_and_memory_only_credentials() {
    let lab = Lab::new();
    let (code, ui) = lab.raw(&format!(
        "GET / HTTP/1.1\r\nHost: {}\r\n\r\n",
        lab.server.local_addr()
    ));
    assert_eq!(code, 200);
    for expected in [
        "fetch(path, options)",
        "/v1/accounts",
        "/v1/config",
        "/v1/shutdown",
        "accountPath('start')",
        "accountPath('stop')",
        "accountPath('capabilities')",
        "accountPath('messages')",
        "accountPath('verify-login')",
        "id=\"save\"",
        "id=\"config\"",
        "Authorization: 'Bearer '+token",
        "textContent",
        "setInterval(() => void refresh()",
    ] {
        assert!(
            ui.contains(expected),
            "missing UI control or fetch route: {expected}"
        );
    }
    for forbidden in [
        "localStorage",
        "sessionStorage",
        "innerHTML",
        "https://",
        "http://",
        TOKEN,
        "__NONCE__",
    ] {
        assert!(
            !ui.contains(forbidden),
            "unexpected UI content: {forbidden}"
        );
    }
    assert!(!ui.contains(&lab.root.to_string_lossy().to_string()));
}

#[test]
fn bounded_headers_body_and_shutdown_cancel_stalled_workers() {
    let mut lab = Lab::new();
    let address = lab.server.local_addr();
    assert_eq!(lab.raw(&format!("PUT /v1/config HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 262145\r\n\r\n")).0, 413);
    let mut stalled = Vec::new();
    for _ in 0..8 {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(b"GET / HTTP/1.1\r\n").unwrap();
        stalled.push(stream);
    }
    // Wait for the accept loop to admit those eight in order before overload.
    std::thread::sleep(Duration::from_millis(150));
    let (code, _) = lab.request("GET", "/v1/accounts", None);
    assert_eq!(code, 503);
    let started = Instant::now();
    lab.server.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    drop(stalled);
}

#[test]
fn absolute_read_deadline_rejects_partial_request() {
    let lab = Lab::new();
    let mut stream = TcpStream::connect(lab.server.local_addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let started = Instant::now();
    let (code, _) = response(&mut stream);
    assert_eq!(code, 408);
    assert!(started.elapsed() < Duration::from_secs(7));
}

#[test]
fn authenticated_shutdown_joins_accounts_and_signals_binary_owner() {
    let mut lab = Lab::new();
    assert_eq!(
        lab.api("POST", "/v1/accounts/10001/start", Some(&json!({})))
            .0,
        200
    );
    let (code, body) = lab.api("POST", "/v1/shutdown", Some(&json!({})));
    assert_eq!(code, 200);
    assert_eq!(body["status"], "stopped");
    assert!(lab.server.stop_flag().load(Ordering::Acquire));
    lab.server.shutdown().unwrap();
    assert!(lab.manager.lock().unwrap().running_accounts().is_empty());
}
