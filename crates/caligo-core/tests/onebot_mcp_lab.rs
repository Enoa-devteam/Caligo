use caligo_core::daemon::{Daemon, DaemonConfig};
use caligo_core::onebot::{ActionResponse, ACTION_REGISTRY};
use caligo_core::onebot_mcp::{handle_json_rpc, serve_mcp};
use caligo_core::onebot_server::{action_handler, ActionHandler};
use caligo_core::runtime::RuntimeConfig;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

fn test_daemon(tag: &str) -> Arc<Daemon> {
    let mut journal = std::env::temp_dir();
    journal.push(format!("caligo-mcp-{tag}-{}.journal", std::process::id()));
    let _ = std::fs::remove_file(&journal);
    let _ = std::fs::remove_file(journal.with_extension("messages.jsonl"));
    Daemon::start(DaemonConfig {
        pipe_prefix: format!("mcp-{tag}-{}", std::process::id()),
        journal_path: journal,
        module_baseline: "test".into(),
        account: "10001".into(),
        core_build: caligo_core::CORE_BUILD.into(),
        expect_client_pid: 0,
        runtime: RuntimeConfig::default(),
        test_fault_break_bridge_after_hello: false,
        test_fault_break_bridge_after_result: false,
    })
    .unwrap()
}

fn rpc(daemon: &Arc<Daemon>, request: Value) -> Value {
    handle_json_rpc(&request, &action_handler(Arc::clone(daemon))).unwrap()
}

#[test]
fn initialize_tool_registry_and_annotations_match_shared_actions() {
    let daemon = test_daemon("protocol");
    let initialized = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":1, "method":"initialize",
            "params":{"protocolVersion":"2025-03-26", "capabilities":{}, "clientInfo":{"name":"lab", "version":"1"}}
        }),
    );
    assert_eq!(initialized["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(
        initialized["result"]["capabilities"]["tools"]["listChanged"],
        false
    );

    let listed = rpc(
        &daemon,
        json!({"jsonrpc":"2.0", "id":"list", "method":"tools/list"}),
    );
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), ACTION_REGISTRY.len());
    for action in ACTION_REGISTRY {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == action.name)
            .unwrap();
        assert_eq!(tool["inputSchema"], action.input_schema.to_json_schema());
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            action.permission == caligo_core::onebot::ActionPermission::Read
        );
        assert_eq!(
            tool["annotations"]["idempotentHint"],
            action.permission == caligo_core::onebot::ActionPermission::Read
        );
    }
    assert!(tools
        .iter()
        .all(|tool| tool["name"].as_str().unwrap().contains('_')));
}

#[test]
fn tool_calls_share_handler_preserve_onebot_response_and_mark_errors() {
    let daemon = test_daemon("calls");
    let not_ready = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":2, "method":"tools/call",
            "params":{"name":"get_status", "arguments":{}}
        }),
    );
    let not_ready_full: Value =
        serde_json::from_str(not_ready["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(not_ready_full["retcode"], 0);
    assert_eq!(not_ready["result"]["isError"], false);

    let status = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":9, "method":"tools/call",
            "params":{"name":"get_msg", "arguments":{"message_id":123456}}
        }),
    );
    assert_eq!(status["result"]["isError"], true);
    let full: Value =
        serde_json::from_str(status["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(full["status"], "failed");
    assert_eq!(full["retcode"], 1440);
    assert!(status["result"].get("structuredContent").is_none());

    let unknown = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":3, "method":"tools/call",
            "params":{"name":"GetStatus", "arguments":{}}
        }),
    );
    assert_eq!(unknown["error"]["code"], -32602);

    let invalid = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":4, "method":"tools/call",
            "params":{"name":"get_msg", "arguments":{"message_id":"bad"}}
        }),
    );
    assert_eq!(invalid["error"]["code"], -32602);

    let minimum_message_id = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":8, "method":"tools/call",
            "params":{"name":"get_msg", "arguments":{"message_id":i32::MIN}}
        }),
    );
    assert_eq!(minimum_message_id["result"]["isError"], true);
    let minimum_response: Value = serde_json::from_str(
        minimum_message_id["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(minimum_response["retcode"], 1440);
    assert_eq!(minimum_response["wording"], "message 不存在");

    let invalid_inferred_route = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":5, "method":"tools/call",
            "params":{"name":"send_msg", "arguments":{"message":"hello"}}
        }),
    );
    assert_eq!(invalid_inferred_route["error"]["code"], -32602);
}

#[test]
fn async_accepted_action_is_not_an_mcp_error() {
    let handler: ActionHandler = Arc::new(|_, _, echo| ActionResponse::async_accepted(echo));
    let response = handle_json_rpc(
        &json!({
            "jsonrpc":"2.0", "id":7, "method":"tools/call",
            "params":{"name":"send_group_msg_async", "arguments":{"group_id":1, "message":"hello"}}
        }),
        &handler,
    )
    .unwrap();
    assert_eq!(response["result"]["isError"], false);
    let full: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(full["status"], "async");
    assert_eq!(full["retcode"], 1);
}

#[test]
fn jsonrpc_notification_and_id_semantics_are_distinct() {
    let daemon = test_daemon("jsonrpc");
    let handler = action_handler(Arc::clone(&daemon));
    assert!(handle_json_rpc(
        &json!({"jsonrpc":"2.0", "method":"notifications/initialized"}),
        &handler
    )
    .is_none());
    let invalid_notification = handle_json_rpc(
        &json!({"jsonrpc":"2.0", "id":"unexpected", "method":"notifications/initialized"}),
        &handler,
    )
    .unwrap();
    assert_eq!(invalid_notification["error"]["code"], -32600);
    assert_eq!(invalid_notification["id"], "unexpected");
    assert_eq!(
        rpc(
            &daemon,
            json!({"jsonrpc":"2.0", "id":null, "method":"ping"})
        )["id"],
        Value::Null
    );
    assert!(handle_json_rpc(&json!({"jsonrpc":"2.0", "method":"ping"}), &handler).is_none());
    assert_eq!(
        rpc(
            &daemon,
            json!({"jsonrpc":"2.0", "id":8, "method":"no.such.method"})
        )["error"]["code"],
        -32601
    );
    assert_eq!(
        rpc(&daemon, json!({"jsonrpc":"1.0", "id":9, "method":"ping"}))["error"]["code"],
        -32600
    );
}

fn request(listener: &TcpListener, text: &str) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    stream.write_all(text.as_bytes()).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let header = std::str::from_utf8(&response[..split]).unwrap();
    let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, response[split..].to_vec())
}

fn start_server(tag: &'static str) -> (Arc<Daemon>, TcpListener, thread::JoinHandle<()>) {
    let daemon = test_daemon(tag);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = listener.try_clone().unwrap();
    let daemon_for_server = Arc::clone(&daemon);
    let join = thread::spawn(move || {
        let _ = serve_mcp(daemon_for_server, client, Some("lab-token".into()));
    });
    (daemon, listener, join)
}

#[test]
fn http_auth_malformed_oversize_and_batch_are_rejected_and_stop_returns() {
    let (daemon, listener, join) = start_server("http");
    let address = listener.local_addr().unwrap();

    let mut unauth = TcpStream::connect(address).unwrap();
    unauth.write_all(b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}").unwrap();
    let mut unauth_response = Vec::new();
    unauth.read_to_end(&mut unauth_response).unwrap();
    assert!(std::str::from_utf8(&unauth_response)
        .unwrap()
        .starts_with("HTTP/1.1 401"));

    let mut malformed = TcpStream::connect(address).unwrap();
    malformed.write_all(b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nContent-Length: 2\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}").unwrap();
    let mut malformed_response = Vec::new();
    malformed.read_to_end(&mut malformed_response).unwrap();
    assert!(std::str::from_utf8(&malformed_response)
        .unwrap()
        .starts_with("HTTP/1.1 400"));

    let mut oversized = TcpStream::connect(address).unwrap();
    oversized.write_all(b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nContent-Length: 1048577\r\nContent-Type: application/json\r\n\r\n").unwrap();
    let mut oversized_response = Vec::new();
    oversized.read_to_end(&mut oversized_response).unwrap();
    assert!(std::str::from_utf8(&oversized_response)
        .unwrap()
        .starts_with("HTTP/1.1 413"));

    let batch = b"[]";
    let batch_request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        batch.len(),
        std::str::from_utf8(batch).unwrap()
    );
    let (status, body) = request(&listener, &batch_request);
    assert_eq!(status, 400);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
        -32600
    );

    let make_request = |headers: &str, body: &str| {
        format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
    };
    let valid_body = r#"{"jsonrpc":"2.0","id":10,"method":"ping"}"#;
    let (status, _) = request(
        &listener,
        &make_request(
            "Origin: http://127.0.0.1:3000\r\nMCP-Protocol-Version: 2025-06-18\r\n",
            valid_body,
        ),
    );
    assert_eq!(status, 200);
    let (status, _) = request(
        &listener,
        &make_request("Origin: https://example.com\r\n", valid_body),
    );
    assert_eq!(status, 403);
    let (status, _) = request(
        &listener,
        &make_request("MCP-Protocol-Version: 2024-11-05\r\n", valid_body),
    );
    assert_eq!(status, 400);
    let (status, _) = request(
        &listener,
        &make_request("", valid_body).replace(
            "Accept: application/json, text/event-stream\r\n",
            "Accept: application/json\r\n",
        ),
    );
    assert_eq!(status, 406);
    let (status, _) = request(
        &listener,
        &format!(
            "GET /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer lab-token\r\nAccept: application/json, text/event-stream\r\n\r\n"
        ),
    );
    assert_eq!(status, 405);

    daemon
        .stop_flag()
        .store(true, std::sync::atomic::Ordering::Relaxed);
    join.join().unwrap();
}

#[test]
fn initialize_negotiates_supported_versions_and_rejects_legacy_version() {
    let daemon = test_daemon("version");
    for version in ["2025-03-26", "2025-06-18"] {
        let response = rpc(
            &daemon,
            json!({
                "jsonrpc":"2.0", "id":1, "method":"initialize",
                "params":{"protocolVersion":version, "capabilities":{}, "clientInfo":{"name":"lab", "version":"1"}}
            }),
        );
        assert_eq!(response["result"]["protocolVersion"], version);
    }
    let legacy = rpc(
        &daemon,
        json!({
            "jsonrpc":"2.0", "id":2, "method":"initialize",
            "params":{"protocolVersion":"2024-11-05"}
        }),
    );
    assert_eq!(legacy["error"]["code"], -32602);
    let default = rpc(
        &daemon,
        json!({"jsonrpc":"2.0", "id":3, "method":"initialize", "params":{"capabilities":{}}}),
    );
    assert_eq!(default["result"]["protocolVersion"], "2025-03-26");
}
