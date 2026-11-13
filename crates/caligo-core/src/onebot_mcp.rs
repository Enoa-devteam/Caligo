//! Stateless, bounded MCP Streamable HTTP POST transport over the shared
//! OneBot action registry and daemon handler.

use crate::daemon::Daemon;
use crate::onebot::{resolve_action, ActionPermission, ACTION_REGISTRY};
use crate::onebot_server::{action_handler, ActionHandler};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_HEADER: usize = 64 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const MAX_READ_DURATION: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_WORKERS: usize = 16;
const MCP_DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";
const MCP_PROTOCOL_VERSIONS: &[&str] = &["2025-03-26", "2025-06-18"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct McpServerStats {
    pub accepted: usize,
    pub completed: usize,
    pub rejected: usize,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    content_length: usize,
    content_type: Option<String>,
    accept: Option<String>,
    origin: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ReadRequestError {
    BadRequest,
    HeaderTooLarge,
    BodyTooLarge,
    Unauthorized,
    ForbiddenOrigin,
    UnsupportedProtocolVersion,
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message.into()}})
}

fn valid_id(id: &Value) -> bool {
    id.is_null() || id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()
}

fn validate_schema(value: &Value, schema: &Value) -> Result<(), String> {
    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        if !variants
            .iter()
            .any(|variant| validate_schema(value, variant).is_ok())
        {
            return Err("value does not match any allowed schema".into());
        }
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return Err(format!("value must equal {expected}"));
        }
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.contains(value) {
            return Err("value is not an allowed enum member".into());
        }
    }
    let object_constraints = schema.get("required").is_some() || schema.get("properties").is_some();
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let object = value.as_object().ok_or("must be an object")?;
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for name in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(name) {
                        return Err(format!("{name} is required"));
                    }
                }
            }
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, field_schema) in properties {
                    if let Some(field) = object.get(name) {
                        validate_schema(field, field_schema)
                            .map_err(|error| format!("{name}: {error}"))?;
                    }
                }
            }
        }
        Some("integer") => {
            let number = value
                .as_i64()
                .map(i128::from)
                .or_else(|| value.as_u64().map(i128::from))
                .ok_or("must be an integer")?;
            if let Some(minimum) = schema.get("minimum").and_then(Value::as_i64) {
                if number < i128::from(minimum) {
                    return Err(format!("must be at least {minimum}"));
                }
            }
            if let Some(maximum) = schema.get("maximum").and_then(Value::as_u64) {
                if number > i128::from(maximum) {
                    return Err(format!("must be at most {maximum}"));
                }
            }
        }
        Some("string") if !value.is_string() => return Err("must be a string".into()),
        None if object_constraints => {
            let object = value.as_object().ok_or("must be an object")?;
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for name in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(name) {
                        return Err(format!("{name} is required"));
                    }
                }
            }
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (name, field_schema) in properties {
                    if let Some(field) = object.get(name) {
                        validate_schema(field, field_schema)
                            .map_err(|error| format!("{name}: {error}"))?;
                    }
                }
            }
        }
        Some("array") => {
            let array = value.as_array().ok_or("must be an array")?;
            if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64) {
                if array.len() < minimum as usize {
                    return Err(format!("must contain at least {minimum} item(s)"));
                }
            }
            if let Some(item_schema) = schema.get("items") {
                for item in array {
                    validate_schema(item, item_schema)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_tool_error(response: &crate::onebot::ActionResponse) -> bool {
    response.status == "failed" || !matches!(response.retcode, 0 | 1)
}

fn tool_list() -> Vec<Value> {
    ACTION_REGISTRY
        .iter()
        .map(|action| {
            json!({
                "name": action.name,
                "description": format!("OneBot V11 action {}", action.name),
                "inputSchema": action.input_schema.to_json_schema(),
                "annotations": {
                    "readOnlyHint": action.permission == ActionPermission::Read,
                    "idempotentHint": action.permission == ActionPermission::Read
                }
            })
        })
        .collect()
}

/// Handle one JSON-RPC request without HTTP framing using the shared daemon handler.
pub fn handle_json_rpc(request: &Value, handler: &ActionHandler) -> Option<Value> {
    let Some(object) = request.as_object() else {
        return Some(rpc_error(Value::Null, -32600, "Invalid Request"));
    };
    let id_present = object.contains_key("id");
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || !object.get("method").is_some_and(Value::is_string)
        || (id_present && !valid_id(&id))
    {
        return Some(rpc_error(
            if id_present { id } else { Value::Null },
            -32600,
            "Invalid Request",
        ));
    }
    let method = object["method"].as_str().unwrap_or_default();
    let params = object.get("params").unwrap_or(&Value::Null);
    if method == "notifications/initialized" {
        return if id_present {
            Some(rpc_error(id, -32600, "Invalid Request"))
        } else {
            None
        };
    }
    if !id_present {
        return None;
    }
    let result = match method {
        "initialize" => {
            let requested_version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(MCP_DEFAULT_PROTOCOL_VERSION);
            if !MCP_PROTOCOL_VERSIONS.contains(&requested_version) {
                return Some(rpc_error(
                    id,
                    -32602,
                    format!(
                        "Unsupported protocol version; supported: {}",
                        MCP_PROTOCOL_VERSIONS.join(", ")
                    ),
                ));
            }
            json!({
                "protocolVersion": requested_version,
                "capabilities": {"tools":{"listChanged":false}},
                "serverInfo":{"name":"caligo-onebot-mcp", "version":env!("CARGO_PKG_VERSION")},
                "instructions":"Stateless JSON-RPC over HTTP POST /mcp. Sessions and resumability are not supported."
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":tool_list()}),
        "tools/call" => {
            let Some(call) = params.as_object() else {
                return Some(rpc_error(id, -32602, "params must be an object"));
            };
            let Some(name) = call.get("name").and_then(Value::as_str) else {
                return Some(rpc_error(id, -32602, "tools/call requires a string name"));
            };
            let Some(action) = resolve_action(name) else {
                return Some(rpc_error(id, -32602, format!("Unknown tool: {name}")));
            };
            let arguments = call.get("arguments").cloned().unwrap_or_else(|| json!({}));
            if let Err(error) = validate_schema(&arguments, &action.input_schema.to_json_schema()) {
                return Some(rpc_error(id, -32602, format!("Invalid arguments: {error}")));
            }
            let response = handler(name, &arguments, Value::Null);
            let full_response = serde_json::to_value(&response).unwrap_or(Value::Null);
            json!({
                "content":[{"type":"text", "text":serde_json::to_string(&full_response).unwrap_or_default()}],
                "isError":is_tool_error(&response)
            })
        }
        _ => return Some(rpc_error(id, -32601, format!("Method not found: {method}"))),
    };
    Some(json!({"jsonrpc":"2.0", "id":id, "result":result}))
}

/// Stateless MCP POST listener; the caller supplies a loopback-bound listener.
pub fn serve_mcp(
    daemon: Arc<Daemon>,
    listener: TcpListener,
    access_token: Option<String>,
) -> io::Result<McpServerStats> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "MCP listener must bind to loopback",
        ));
    }
    if access_token.as_deref().is_some_and(str::is_empty) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "MCP bearer token must not be empty",
        ));
    }
    listener.set_nonblocking(true)?;
    let handler = action_handler(Arc::clone(&daemon));
    let active = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    let mut stats = McpServerStats::default();
    while !daemon.stop_flag().load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stats.accepted += 1;
                if active.fetch_add(1, Ordering::AcqRel) >= MAX_WORKERS {
                    active.fetch_sub(1, Ordering::AcqRel);
                    stats.rejected += 1;
                    let _ = stream.set_nonblocking(true);
                    let _ = stream.write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let handler = Arc::clone(&handler);
                let active = Arc::clone(&active);
                let token = access_token.clone();
                workers.push(std::thread::spawn(move || {
                    let completed = serve_connection(stream, token.as_deref(), &handler);
                    active.fetch_sub(1, Ordering::AcqRel);
                    completed
                }));
                if workers.len() > MAX_WORKERS * 2 {
                    let mut retained = Vec::with_capacity(MAX_WORKERS);
                    for worker in workers.drain(..) {
                        if worker.is_finished() {
                            if worker.join().unwrap_or(false) {
                                stats.completed += 1;
                            }
                        } else {
                            retained.push(worker);
                        }
                    }
                    workers = retained;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(error) => return Err(error),
        }
    }
    for worker in workers {
        if worker.join().unwrap_or(false) {
            stats.completed += 1;
        }
    }
    Ok(stats)
}

fn serve_connection(mut stream: TcpStream, token: Option<&str>, handler: &ActionHandler) -> bool {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    match read_request(&mut stream, token) {
        Ok((request, body)) => {
            if request
                .origin
                .as_deref()
                .is_some_and(|origin| !loopback_origin(origin))
            {
                write_http(&mut stream, 403, "application/json", b"{}");
                return false;
            }
            if request.path.split('?').next() != Some("/mcp") {
                write_http(&mut stream, 404, "application/json", b"{}");
                return false;
            }
            if request.method != "POST" {
                write_http(&mut stream, 405, "application/json", b"{}");
                return false;
            }
            if !accepts_json(&request.accept) {
                write_http(&mut stream, 406, "application/json", b"{}");
                return false;
            }
            if !request
                .content_type
                .as_deref()
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
            {
                write_http(&mut stream, 415, "application/json", b"{}");
                return false;
            }
            let parsed: Value = match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => {
                    write_http(&mut stream, 400, "application/json", b"{}");
                    return false;
                }
            };
            if parsed.is_array() {
                let bytes = serde_json::to_vec(&rpc_error(
                    Value::Null,
                    -32600,
                    "Batch requests are not supported",
                ))
                .unwrap_or_default();
                write_http(&mut stream, 400, "application/json", &bytes);
                return false;
            }
            match handle_json_rpc(&parsed, handler) {
                Some(response) => {
                    let bytes = serde_json::to_vec(&response).unwrap_or_default();
                    write_http(&mut stream, 200, "application/json", &bytes);
                }
                None => write_http(&mut stream, 202, "application/json", b""),
            }
            true
        }
        Err(error) => {
            let (status, body) = match error {
                ReadRequestError::HeaderTooLarge => (431, b"request headers too large".as_slice()),
                ReadRequestError::BodyTooLarge => (413, b"request body too large".as_slice()),
                ReadRequestError::Unauthorized => (401, b"unauthorized".as_slice()),
                ReadRequestError::ForbiddenOrigin => (403, b"forbidden origin".as_slice()),
                ReadRequestError::UnsupportedProtocolVersion => {
                    (400, b"unsupported protocol version".as_slice())
                }
                ReadRequestError::BadRequest => (400, b"bad request".as_slice()),
            };
            write_http(&mut stream, status, "text/plain", body);
            false
        }
    }
}

fn read_request(
    stream: &mut TcpStream,
    token: Option<&str>,
) -> Result<(HttpRequest, Vec<u8>), ReadRequestError> {
    let deadline = Instant::now() + MAX_READ_DURATION;
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(end) = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|p| p + 4)
        {
            break end;
        }
        if bytes.len() >= MAX_HEADER {
            return Err(ReadRequestError::HeaderTooLarge);
        }
        set_read_timeout(stream, deadline)?;
        let limit = (MAX_HEADER - bytes.len()).min(chunk.len());
        let count = stream
            .read(&mut chunk[..limit])
            .map_err(|_| ReadRequestError::BadRequest)?;
        if count == 0 {
            return Err(ReadRequestError::BadRequest);
        }
        bytes.extend_from_slice(&chunk[..count]);
    };
    if header_end > MAX_HEADER {
        return Err(ReadRequestError::HeaderTooLarge);
    }
    let text =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| ReadRequestError::BadRequest)?;
    let mut lines = text.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or(ReadRequestError::BadRequest)?
        .split_whitespace();
    let method = request_line
        .next()
        .ok_or(ReadRequestError::BadRequest)?
        .to_owned();
    let path = request_line
        .next()
        .ok_or(ReadRequestError::BadRequest)?
        .to_owned();
    let version = request_line.next().ok_or(ReadRequestError::BadRequest)?;
    if request_line.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(ReadRequestError::BadRequest);
    }
    let (
        mut content_length,
        mut authorization,
        mut content_type,
        mut accept,
        mut mcp_protocol_version,
        mut origin,
        mut transfer_encoding,
    ) = (None, None, None, None, None, None, false);
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(ReadRequestError::BadRequest)?;
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                if content_length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(ReadRequestError::BadRequest);
                }
                content_length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| ReadRequestError::BadRequest)?,
                );
            }
            "authorization" => {
                if authorization.replace(value.to_owned()).is_some() {
                    return Err(ReadRequestError::BadRequest);
                }
            }
            "content-type" => {
                if content_type.replace(value.to_owned()).is_some() {
                    return Err(ReadRequestError::BadRequest);
                }
            }
            "accept" => {
                if accept.replace(value.to_owned()).is_some() {
                    return Err(ReadRequestError::BadRequest);
                }
            }
            "mcp-protocol-version" => {
                if mcp_protocol_version.replace(value.to_owned()).is_some() {
                    return Err(ReadRequestError::BadRequest);
                }
            }
            "origin" => {
                if origin.replace(value.to_owned()).is_some() {
                    return Err(ReadRequestError::BadRequest);
                }
            }
            "transfer-encoding" => transfer_encoding = true,
            _ => {}
        }
    }
    if transfer_encoding {
        return Err(ReadRequestError::BadRequest);
    }
    let content_length = match content_length {
        Some(length) => length,
        None if method == "GET" => 0,
        None => return Err(ReadRequestError::BadRequest),
    };
    if content_length > MAX_BODY {
        return Err(ReadRequestError::BodyTooLarge);
    }
    if origin
        .as_deref()
        .is_some_and(|value| !loopback_origin(value))
    {
        return Err(ReadRequestError::ForbiddenOrigin);
    }
    if mcp_protocol_version
        .as_deref()
        .is_some_and(|value| !MCP_PROTOCOL_VERSIONS.contains(&value))
    {
        return Err(ReadRequestError::UnsupportedProtocolVersion);
    }
    if let Some(expected) = token {
        let valid = authorization
            .as_deref()
            .and_then(|value| value.split_once(' '))
            .is_some_and(|(scheme, value)| {
                scheme.eq_ignore_ascii_case("Bearer") && value == expected
            });
        if !valid {
            return Err(ReadRequestError::Unauthorized);
        }
    }
    let request = HttpRequest {
        method,
        path,
        content_length,
        content_type,
        accept,
        origin,
    };
    let total = header_end
        .checked_add(request.content_length)
        .ok_or(ReadRequestError::BadRequest)?;
    while bytes.len() < total {
        set_read_timeout(stream, deadline)?;
        let read_len = (total - bytes.len()).min(chunk.len());
        let count = stream
            .read(&mut chunk[..read_len])
            .map_err(|_| ReadRequestError::BadRequest)?;
        if count == 0 {
            return Err(ReadRequestError::BadRequest);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok((request, bytes[header_end..total].to_vec()))
}

fn set_read_timeout(stream: &TcpStream, deadline: Instant) -> Result<(), ReadRequestError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(ReadRequestError::BadRequest)?;
    stream
        .set_read_timeout(Some(remaining.min(Duration::from_secs(1))))
        .map_err(|_| ReadRequestError::BadRequest)
}

fn loopback_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let Some(host) = url.host() else {
        return false;
    };
    matches!(host, url::Host::Ipv4(ip) if ip.is_loopback())
        || matches!(host, url::Host::Ipv6(ip) if ip.is_loopback())
        || matches!(host, url::Host::Domain(domain) if domain.eq_ignore_ascii_case("localhost"))
}

fn accepts_json(accept: &Option<String>) -> bool {
    let Some(accept) = accept else {
        return false;
    };
    let has_media_type = |expected: &str| {
        accept.split(',').any(|entry| {
            entry
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case(expected)
        })
    };
    has_media_type("application/json") && has_media_type("text/event-stream")
}

fn write_http(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let deadline = Instant::now() + WRITE_TIMEOUT;
    let _ = stream.set_nonblocking(true);
    let _ = write_all_until(stream, head.as_bytes(), deadline)
        .and_then(|_| write_all_until(stream, body, deadline));
}

fn write_all_until(stream: &mut TcpStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP response write deadline elapsed",
            ));
        }
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "HTTP response write returned zero",
                ));
            }
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
