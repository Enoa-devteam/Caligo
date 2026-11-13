//! Black-box smoke coverage for the real caligo-manager executable.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Guard {
    child: Child,
    root: PathBuf,
}

impl Guard {
    fn wait(mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match self.child.try_wait().expect("poll manager") {
                Some(status) => {
                    let _ = std::fs::remove_dir_all(&self.root);
                    return status;
                }
                None if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    let status = self.child.wait().expect("wait killed manager");
                    let _ = std::fs::remove_dir_all(&self.root);
                    panic!("manager did not exit before timeout: {status:?}");
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.child.try_wait().expect("poll manager cleanup").is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unique_tag() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

fn free_port() -> u16 {
    // Windows can immediately reissue a released ephemeral port. Keep a
    // process-wide set so parallel fixtures and one multi-port config never
    // select the same port before their child listeners have bound it.
    static PORTS: std::sync::Mutex<std::collections::BTreeSet<u16>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let mut used = PORTS.lock().unwrap();
    for _ in 0..256 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("free port");
        let port = listener.local_addr().expect("port address").port();
        if used.insert(port) {
            return port;
        }
    }
    panic!("could not allocate a distinct smoke-test port");
}

fn request_version(address: &str, token: &str) -> Value {
    let mut stream = TcpStream::connect(address).expect("connect HTTP listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("HTTP timeout");
    write!(
        stream,
        "GET /get_version_info HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    )
    .expect("write version request");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read version response");
    let response = String::from_utf8(response).expect("HTTP response UTF-8");
    assert!(response.starts_with("HTTP/1.1 200"), "unexpected response: {response}");
    let (_, body) = response.split_once("\r\n\r\n").expect("HTTP body");
    serde_json::from_str(body).expect("version JSON")
}

fn control_pipe(prefix: &str) -> String {
    format!("\\\\.\\pipe\\{prefix}-control")
}

fn connect_control(prefix: &str, token: &str) -> caligo_core::daemon::ControlClient {
    let pipe = control_pipe(prefix);
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match caligo_core::daemon::ControlClient::connect(&pipe, token, "manager-smoke") {
            Ok(client) => return client,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("control connection timed out: {error}"),
        }
    }
}

fn auth_token(root: &Path, account_dir: &str) -> String {
    std::fs::read_to_string(root.join(account_dir).join("auth.token"))
        .expect("private account token")
        .trim()
        .to_string()
}

fn management_token(root: &Path) -> String {
    std::fs::read_to_string(root.join("management.token"))
        .expect("private management token")
        .trim()
        .to_string()
}

fn management_request(
    address: &str,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, Value) {
    let mut stream = TcpStream::connect(address).expect("connect management listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("management HTTP timeout");
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write management request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read management response");
    let response = String::from_utf8(response).expect("management response UTF-8");
    let status = response
        .split_once(' ')
        .and_then(|(_, rest)| rest.split_once(' '))
        .and_then(|(code, _)| code.parse().ok())
        .expect("management status");
    let (_, body) = response.split_once("\r\n\r\n").expect("management body");
    (status, serde_json::from_str(body).expect("management JSON"))
}

fn write_manager_config(root: &Path, config: &Value) {
    let evidence = root.parent().expect("manager root parent");
    std::fs::create_dir_all(evidence).expect("evidence directory");
    let input = evidence.join(format!("manager-config-{}.json", unique_tag()));
    std::fs::write(
        &input,
        serde_json::to_vec_pretty(config).expect("serialize manager config"),
    )
    .expect("write manager config input");
    let output = Command::new(env!("CARGO_BIN_EXE_caligo-manager"))
        .args([
            "--root",
            root.to_str().unwrap(),
            "--set-config",
            input.to_str().unwrap(),
        ])
        .output()
        .expect("persist manager config through manager");
    let _ = std::fs::remove_file(input);
    assert!(output.status.success(), "config setup failed: {:?}", output.status);
}

fn wait_http_closed(address: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(address).is_err() {
            return;
        }
        assert!(Instant::now() < deadline, "listener remained bound at {address}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn management_http_config_lifecycle_and_shutdown() {
    let tag = unique_tag();
    let root = std::env::current_dir()
        .expect("workspace")
        .join("local-evidence")
        .join(format!("manager-management-smoke-{tag}"));
    let prefix = format!("manager-management-smoke-{tag}");
    let protocol_port = free_port();
    let management_port = free_port();
    let config = json!({
        "version": 1,
        "accounts": [{
            "account": 11001,
            "data_dir": "account",
            "pipe_prefix": prefix,
            "module_baseline": "offline-manager-smoke",
            "expect_client_pid": 0,
            "ports": {"http": protocol_port, "websocket": null, "sse": null, "mcp": null}
        }]
    });
    write_manager_config(&root, &config);

    let bin = env!("CARGO_BIN_EXE_caligo-manager");
    let mut child = Command::new(bin)
        .args([
            "--root",
            root.to_str().unwrap(),
            "--management-bind",
            &format!("127.0.0.1:{management_port}"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn management manager");
    let mut guard = Guard {
        child,
        root: root.clone(),
    };
    let mut stdout = String::new();
    let mut reader = std::io::BufReader::new(guard.child.stdout.take().expect("manager stdout"));
    std::io::BufRead::read_line(&mut reader, &mut stdout).expect("management metadata line");
    assert!(!stdout.is_empty(), "manager exited before metadata: {:?}", guard.child.try_wait().expect("poll manager"));
    let metadata: Value = serde_json::from_str(stdout.trim()).expect("management metadata JSON");
    assert_eq!(metadata.as_object().unwrap().len(), 2);
    assert_eq!(metadata["management_endpoint"], format!("http://127.0.0.1:{management_port}"));
    assert!(metadata.get("auth_token").is_none());
    let management_address = metadata["management_endpoint"]
        .as_str()
        .unwrap()
        .trim_start_matches("http://");
    let token = management_token(&root);
    assert_eq!(token.len(), 64);
    assert!(!stdout.contains(&token));

    let (status, current) = management_request(
        management_address,
        &token,
        "GET",
        "/v1/config",
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(current, config);

    let (status, initially_stopped) = management_request(
        management_address,
        &token,
        "POST",
        "/v1/accounts/11001/stop",
        Some("{}"),
    );
    assert_eq!(status, 200);
    assert_eq!(initially_stopped["running"], false);

    let mut edited = config.clone();
    edited["accounts"][0]["module_baseline"] = json!("offline-manager-smoke-edited");
    let body = edited.to_string();
    let (status, saved) = management_request(
        management_address,
        &token,
        "PUT",
        "/v1/config",
        Some(&body),
    );
    assert_eq!(status, 200);
    assert_eq!(saved, edited);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(root.join("accounts.json")).unwrap()).unwrap(),
        edited
    );

    let (status, started) = management_request(
        management_address,
        &token,
        "POST",
        "/v1/accounts/11001/start",
        Some("{}"),
    );
    assert_eq!(status, 200);
    assert_eq!(started["running"], true);
    let protocol_address = format!("127.0.0.1:{protocol_port}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(&protocol_address).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "API start did not bind protocol listener");
        std::thread::sleep(Duration::from_millis(25));
    }
    let account_token = auth_token(&root, "account");
    assert_eq!(request_version(&protocol_address, &account_token)["data"]["protocol_version"], "v11");

    let (status, stopped) = management_request(
        management_address,
        &token,
        "POST",
        "/v1/accounts/11001/stop",
        Some("{}"),
    );
    assert_eq!(status, 200);
    assert_eq!(stopped["running"], false);
    wait_http_closed(&protocol_address);

    let (status, restarted) = management_request(
        management_address,
        &token,
        "POST",
        "/v1/accounts/11001/start",
        Some("{}"),
    );
    assert_eq!(status, 200);
    assert_eq!(restarted["running"], true);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(&protocol_address).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "API restart did not rebind protocol listener");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(request_version(&protocol_address, &account_token)["data"]["protocol_version"], "v11");

    let (status, shutdown) = management_request(
        management_address,
        &token,
        "POST",
        "/v1/shutdown",
        Some("{}"),
    );
    assert_eq!(status, 200);
    assert_eq!(shutdown["status"], "stopped");
    assert_eq!(guard.wait().code(), Some(0));
}

#[test]
fn manager_startup_listener_bind_failure_exits_promptly() {
    let tag = unique_tag();
    let root = std::env::current_dir()
        .expect("workspace")
        .join("local-evidence")
        .join(format!("manager-bind-failure-smoke-{tag}"));
    let occupied = TcpListener::bind("127.0.0.1:0").expect("reserve protocol port");
    let port = occupied.local_addr().unwrap().port();
    let config = json!({
        "version": 1,
        "accounts": [{
            "account": 12001,
            "data_dir": "account",
            "pipe_prefix": format!("manager-bind-failure-{tag}"),
            "module_baseline": "offline-manager-smoke",
            "expect_client_pid": 0,
            "ports": {"http": port}
        }]
    });
    write_manager_config(&root, &config);

    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_caligo-manager"))
        .args(["--root", root.to_str().unwrap()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn manager with occupied port");
    let deadline = started + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("poll failed startup") {
            assert!(!status.success(), "occupied protocol port must fail startup");
            assert!(started.elapsed() < Duration::from_secs(10));
            break;
        }
        assert!(Instant::now() < deadline, "bind failure cleanup deadlocked");
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(occupied);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn manager_binary_runs_two_accounts_and_persists_config() {
    let tag = unique_tag();
    let evidence = std::env::current_dir()
        .expect("workspace")
        .join("local-evidence");
    let root = evidence.join(format!("manager-smoke-{tag}"));
    let input_dir = evidence.join(format!("manager-smoke-input-{tag}"));
    std::fs::create_dir_all(&input_dir).expect("config input directory");
    let config_input = input_dir.join("input.json");
    let prefixes = [format!("manager-smoke-{tag}-a"), format!("manager-smoke-{tag}-b")];
    let ports: Vec<[u16; 4]> = (0..2)
        .map(|_| [free_port(), free_port(), free_port(), free_port()])
        .collect();
    let config = json!({
        "version": 1,
        "accounts": [
            {
                "account": 10001,
                "data_dir": "account-a",
                "pipe_prefix": prefixes[0],
                "module_baseline": "offline-manager-smoke",
                "expect_client_pid": 0,
                "ports": {"http": ports[0][0], "websocket": ports[0][1], "sse": ports[0][2], "mcp": ports[0][3]}
            },
            {
                "account": 10002,
                "data_dir": "account-b",
                "pipe_prefix": prefixes[1],
                "module_baseline": "offline-manager-smoke",
                "expect_client_pid": 0,
                "ports": {"http": ports[1][0], "websocket": ports[1][1], "sse": ports[1][2], "mcp": ports[1][3]}
            }
        ]
    });
    std::fs::write(&config_input, serde_json::to_vec_pretty(&config).unwrap()).expect("input config");

    let bin = env!("CARGO_BIN_EXE_caligo-manager");
    let check = Command::new(bin)
        .args(["--root", root.to_str().unwrap(), "--set-config", config_input.to_str().unwrap()])
        .output()
        .expect("config edit command");
    assert!(check.status.success(), "config edit failed: {:?}; {}", check.status, String::from_utf8_lossy(&check.stderr));
    let persisted: Value = serde_json::from_slice(
        &std::fs::read(root.join("accounts.json")).expect("persisted config"),
    )
    .expect("persisted JSON");
    assert_eq!(persisted, config);
    let check = Command::new(bin)
        .args(["--root", root.to_str().unwrap(), "--check"])
        .output()
        .expect("config check command");
    assert!(check.status.success(), "config check failed: {:?}", check.status);

    let mut child = Command::new(bin)
        .args(["--root", root.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn manager");
    let mut guard = Guard {
        child,
        root: root.clone(),
    };
    let mut stdout = String::new();
    let mut reader = std::io::BufReader::new(guard.child.stdout.take().expect("manager stdout"));
    std::io::BufRead::read_line(&mut reader, &mut stdout).expect("metadata line");
    let metadata: Value = serde_json::from_str(stdout.trim()).expect("metadata JSON");
    assert_eq!(metadata.as_object().unwrap().len(), 1);
    assert!(metadata.get("auth_token").is_none());
    assert!(metadata.get("accounts").is_some());
    assert!(stdout.contains("endpoints"));

    let token_a = auth_token(&root, "account-a");
    let token_b = auth_token(&root, "account-b");
    assert_eq!(token_a.len(), 64);
    assert_eq!(token_b.len(), 64);
    assert!(!stdout.contains(&token_a));
    assert!(!stdout.contains(&token_b));

    let first = &metadata["accounts"][0];
    let second = &metadata["accounts"][1];
    let address_a = first["endpoints"]["http"].as_str().unwrap();
    let address_b = second["endpoints"]["http"].as_str().unwrap();
    assert_eq!(request_version(address_a, &token_a)["data"]["protocol_version"], "v11");
    assert_eq!(request_version(address_b, &token_b)["data"]["protocol_version"], "v11");

    let mut control_a = connect_control(&prefixes[0], &token_a);
    assert!(matches!(
        control_a
            .request(caligo_core::ipc::ControlMsg::Stop {})
            .expect("stop account a"),
        caligo_core::ipc::CoreToControlMsg::Stopped { .. }
    ));
    drop(control_a);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(request_version(address_b, &token_b)["data"]["protocol_version"], "v11");

    let mut control_b = connect_control(&prefixes[1], &token_b);
    assert!(matches!(
        control_b
            .request(caligo_core::ipc::ControlMsg::Stop {})
            .expect("stop account b"),
        caligo_core::ipc::CoreToControlMsg::Stopped { .. }
    ));
    drop(control_b);
    let status = guard.wait();
    assert_eq!(status.code(), Some(0), "manager must stop cleanly");
    let _ = std::fs::remove_dir_all(input_dir);
}
