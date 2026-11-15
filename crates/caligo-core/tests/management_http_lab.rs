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
