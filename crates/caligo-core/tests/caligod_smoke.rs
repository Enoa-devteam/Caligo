//! K4-D5 caligod 二进制冒烟测试:子进程启动 → stdout 首行取管道名与
//! token → control 连接 → Health → Stop → 退出码 0(真实 Stopping 协议)。

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn caligod_binary_runs_stops_cleanly() {
    let bin = env!("CARGO_BIN_EXE_caligod");
    let tag = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let journal_dir = std::env::temp_dir().join(format!("caligo-caligod-{tag}"));
    std::fs::create_dir_all(&journal_dir).unwrap();
    let journal = journal_dir.join("journal.log");
    let _ = std::fs::remove_file(&journal);

    let mut child = Command::new(bin)
        .args([
            "--pipe-prefix",
            &format!("\\\\.\\pipe\\caligo-k5-smoke-{tag}"),
            "--journal",
            journal.to_str().unwrap(),
            "--baseline",
            "qq-9.9.33-52230",
            "--account",
            "10001",
            "--expect-pid",
            &std::process::id().to_string(),
            "--onebot-http-bind",
            "127.0.0.1:0",
            "--onebot-sse-bind",
            "127.0.0.1:0",
            "--onebot-mcp-bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn caligod");

    // 首行 JSON:管道名 + token。
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read startup line");
    let info: serde_json::Value = serde_json::from_str(line.trim()).expect("startup json");
    let control_pipe = info["control_pipe"]
        .as_str()
        .expect("control_pipe")
        .to_string();
    let auth = info["auth_token"].as_str().expect("auth_token").to_string();

    // 连接 control(轮询等待 server 就绪)。
    let mut client = None;
    for _ in 0..150 {
        match caligo_core::daemon::ControlClient::connect(&control_pipe, &auth, "smoke-client") {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut client = client.expect("control connect");

    // Exercise optional protocol listeners through the actual caligod process.
    use std::io::{Read, Write};
    let mut sse = std::net::TcpStream::connect(info["onebot_sse"].as_str().unwrap()).unwrap();
    sse.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write!(
        sse,
        "GET /_events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {auth}\r\n\r\n"
    )
    .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        sse.read_exact(&mut byte).unwrap();
        headers.push(byte[0]);
        assert!(headers.len() < 8192);
    }
    assert!(String::from_utf8(headers)
        .unwrap()
        .starts_with("HTTP/1.1 200"));
    let mut mcp = std::net::TcpStream::connect(info["onebot_mcp"].as_str().unwrap()).unwrap();
    mcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    write!(mcp, "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {auth}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    mcp.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    let rpc: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        rpc["result"]["tools"].as_array().unwrap().len(),
        caligo_core::onebot::ACTION_REGISTRY.len()
    );

    // Health 探测 + Stop。
    let r = client
        .request(caligo_core::ipc::ControlMsg::Health {})
        .unwrap();
    assert!(matches!(
        r,
        caligo_core::ipc::CoreToControlMsg::HealthAck {}
    ));
    let r = client
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    assert!(matches!(
        r,
        caligo_core::ipc::CoreToControlMsg::Stopped { .. }
    ));

    // 等待退出:退出码 0 = 真实 Stopping 协议收尾。
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().unwrap() {
            Some(s) => break s,
            None if std::time::Instant::now() > deadline => panic!("caligod did not exit"),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    assert_eq!(status.code(), Some(0), "clean stop must exit 0");
    let _ = reader;
    let _ = std::fs::remove_dir_all(&journal_dir);
}
