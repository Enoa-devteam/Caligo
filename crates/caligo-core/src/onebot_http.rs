//! Bounded OneBot HTTP action transport. Parsing is byte-boundary based and
//! routes every request through the shared action handler.

use crate::onebot::{parse_action, ActionResponse};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

pub const DEFAULT_MAX_BODY: usize = 1024 * 1024;
const MAX_HEADER: usize = 64 * 1024;
const MAX_READ_DURATION: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_HTTP_WORKERS: usize = 16;

#[derive(Debug, PartialEq, Eq)]
struct RequestHead {
    method: String,
    path: String,
    content_length: Option<usize>,
    authorization: Option<String>,
    content_type: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum HeadError {
    BadRequest,
    HeaderTooLarge,
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_request_head(buf: &[u8]) -> Result<RequestHead, HeadError> {
    let end = header_end(buf).ok_or(HeadError::HeaderTooLarge)?;
    if end > MAX_HEADER {
        return Err(HeadError::HeaderTooLarge);
    }
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| HeadError::BadRequest)?;
    let mut lines = head.split("\r\n");
    let mut request = lines
        .next()
        .ok_or(HeadError::BadRequest)?
        .split_whitespace();
    let method = request.next().ok_or(HeadError::BadRequest)?.to_string();
    let path = request.next().ok_or(HeadError::BadRequest)?.to_string();
    let version = request.next().ok_or(HeadError::BadRequest)?;
    if request.next().is_some()
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || !path.starts_with('/')
    {
        return Err(HeadError::BadRequest);
    }
    let mut content_length = None;
    let mut authorization = None;
    let mut content_type = None;
    let mut transfer_encoding = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(HeadError::BadRequest)?;
        let value = value.trim();
        if name.is_empty() || value.contains('\0') {
            return Err(HeadError::BadRequest);
        }
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                if content_length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(HeadError::BadRequest);
                }
                content_length = Some(value.parse().map_err(|_| HeadError::BadRequest)?);
            }
            "authorization" => {
                if authorization.replace(value.to_string()).is_some() {
                    return Err(HeadError::BadRequest);
                }
            }
            "content-type" => {
                if content_type.replace(value.to_string()).is_some() {
                    return Err(HeadError::BadRequest);
                }
            }
            "transfer-encoding" => transfer_encoding = true,
            _ => {}
        }
    }
    if transfer_encoding {
        return Err(HeadError::BadRequest);
    }
    Ok(RequestHead {
        method,
        path,
        content_length,
        authorization,
        content_type,
    })
}

fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err("invalid percent escape".into());
                }
                let hi = (bytes[i + 1] as char)
                    .to_digit(16)
                    .ok_or("invalid percent escape")? as u8;
                let lo = (bytes[i + 2] as char)
                    .to_digit(16)
                    .ok_or("invalid percent escape")? as u8;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "form/query value is not UTF-8".into())
}

fn parse_pairs(raw: &str) -> Result<Vec<(String, String)>, String> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            Ok((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}

fn form_action(raw: &str, path_action: Option<&str>) -> Result<(String, Value, Value), String> {
    let pairs = parse_pairs(raw)?;
    let mut action = path_action.map(str::to_owned);
    let mut params = Map::new();
    for (key, value) in pairs {
        match key.as_str() {
            "action" => action = Some(value),
            "access_token" => {}
            "params" => {
                let parsed: Value = serde_json::from_str(&value)
                    .map_err(|e| format!("params JSON invalid: {e}"))?;
                if let Value::Object(obj) = parsed {
                    params.extend(obj);
                } else {
                    return Err("params must be a JSON object".into());
                }
            }
            _ => {
                params.insert(key, Value::String(value));
            }
        }
    }
    let action = action
        .filter(|s| !s.is_empty())
        .ok_or("action is required")?;
    let mut params = Value::Object(params);
    coerce_http_params(&mut params)?;
    Ok((action, params, Value::Null))
}

fn coerce_http_params(params: &mut Value) -> Result<(), String> {
    let Some(object) = params.as_object_mut() else {
        return Err("params must be an object".into());
    };
    for key in ["user_id", "group_id", "message_id", "time", "real_id"] {
        if let Some(Value::String(raw)) = object.get(key) {
            let value = raw
                .parse::<i64>()
                .map_err(|_| format!("{key} must be an integer"))?;
            object.insert(key.to_string(), Value::from(value));
        }
    }
    if let Some(Value::String(raw)) = object.get("auto_escape") {
        let value = raw
            .parse::<bool>()
            .map_err(|_| "auto_escape must be true or false")?;
        object.insert("auto_escape".into(), Value::Bool(value));
    }
    Ok(())
}

fn action_from_request(head: &RequestHead, body: &[u8]) -> Result<(String, Value, Value), String> {
    let (route, query) = head.path.split_once('?').unwrap_or((&head.path, ""));
    let path_action = route
        .trim_matches('/')
        .split('/')
        .next()
        .filter(|s| !s.is_empty());
    let content_type = head
        .content_type
        .as_deref()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if head.method == "GET" {
        return form_action(query, path_action);
    }
    match content_type.as_str() {
        "application/json" => {
            let text = std::str::from_utf8(body).map_err(|_| "request body is not UTF-8")?;
            let parsed: Value =
                serde_json::from_str(text).map_err(|e| format!("动作 JSON 解析失败: {e}"))?;
            if parsed.get("action").is_some() {
                let (action, mut params, echo) = parse_action(text)?;
                coerce_http_params(&mut params)?;
                Ok((action, params, echo))
            } else {
                let action = path_action.ok_or("action path is required")?.to_string();
                let mut params = parsed;
                coerce_http_params(&mut params)?;
                Ok((action, params, Value::Null))
            }
        }
        "application/x-www-form-urlencoded" => {
            let text = std::str::from_utf8(body).map_err(|_| "request body is not UTF-8")?;
            form_action(text, path_action)
        }
        _ => {
            Err("Content-Type must be application/json or application/x-www-form-urlencoded".into())
        }
    }
}

/// Pure JSON-body helper retained for callers that already validated HTTP
/// framing. All body lengths are checked before parsing.
pub fn handle_http_body(
    body: &[u8],
    max_body: usize,
    handler: impl FnOnce(&str, &Value, Value) -> ActionResponse,
) -> (u16, String) {
    if body.len() > max_body {
        return (
            413,
            r#"{"status":"failed","retcode":1413,"wording":"请求体超限"}"#.into(),
        );
    }
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            return (
                400,
                r#"{"status":"failed","retcode":1440,"wording":"非 UTF-8 请求体"}"#.into(),
            )
        }
    };
    match parse_action(text) {
        Ok((action, params, echo)) => {
            let response = handler(&action, &params, echo);
            (200, serde_json::to_string(&response).unwrap_or_default())
        }
        Err(error) => {
            let response =
                ActionResponse::failed(crate::onebot::RET_BAD_REQUEST, &error, Value::Null);
            (400, serde_json::to_string(&response).unwrap_or_default())
        }
    }
}

fn token_ok(authorization: &Option<String>, path: &str, token: Option<&str>) -> bool {
    let Some(expected) = token else {
        return true;
    };
    if let Some(auth) = authorization {
        if let Some((scheme, value)) = auth.split_once(' ') {
            if scheme.eq_ignore_ascii_case("Bearer") && value.trim() == expected {
                return true;
            }
        }
        if auth == expected {
            return true;
        }
    }
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    parse_pairs(query).ok().is_some_and(|pairs| {
        pairs
            .iter()
            .any(|(k, v)| k == "access_token" && v == expected)
    })
}

/// Serve one bounded request. The returned request body contains exactly the
/// declared body bytes (extra pipelined bytes are ignored as Connection closes).
pub fn serve_one(
    mut stream: TcpStream,
    max_body: usize,
    access_token: Option<&str>,
    handler: impl FnOnce(&str, &Value, Value) -> ActionResponse,
) -> (String, String, Option<String>, Vec<u8>) {
    // On Windows an accepted socket can inherit the listener's nonblocking
    // mode. Read timeouts only bound blocking reads; otherwise a client that
    // has not sent its complete request yet is rejected as incomplete.
    if stream.set_nonblocking(false).is_err() {
        return (String::new(), String::new(), None, Vec::new());
    }
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
    let read_deadline = std::time::Instant::now() + MAX_READ_DURATION;
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let end = loop {
        if let Some(end) = header_end(&buf) {
            break end;
        }
        if buf.len() >= MAX_HEADER {
            respond(
                &mut stream,
                431,
                "{\"status\":\"failed\",\"retcode\":1431,\"wording\":\"请求头超限\"}",
            );
            return (String::new(), String::new(), None, Vec::new());
        }
        let limit = (MAX_HEADER - buf.len()).min(tmp.len());
        if !set_remaining_read_timeout(&stream, read_deadline) {
            respond(
                &mut stream,
                400,
                "{\"status\":\"failed\",\"retcode\":1440,\"wording\":\"请求头读取超时\"}",
            );
            return (String::new(), String::new(), None, Vec::new());
        }
        match stream.read(&mut tmp[..limit]) {
            Ok(0) | Err(_) => {
                respond(
                    &mut stream,
                    400,
                    "{\"status\":\"failed\",\"retcode\":1440,\"wording\":\"请求头未完整\"}",
                );
                return (String::new(), String::new(), None, Vec::new());
            }
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    };
    let head = match parse_request_head(&buf[..end]) {
        Ok(head) => head,
        Err(HeadError::HeaderTooLarge) => {
            respond(
                &mut stream,
                431,
                "{\"status\":\"failed\",\"retcode\":1431,\"wording\":\"请求头超限\"}",
            );
            return (String::new(), String::new(), None, Vec::new());
        }
        Err(HeadError::BadRequest) => {
            respond(
                &mut stream,
                400,
                "{\"status\":\"failed\",\"retcode\":1440,\"wording\":\"请求头无效\"}",
            );
            return (String::new(), String::new(), None, Vec::new());
        }
    };
    let content_length = head.content_length.unwrap_or(0);
    if head.method == "POST" && head.content_length.is_none() {
        respond(
            &mut stream,
            411,
            "{\"status\":\"failed\",\"retcode\":1411,\"wording\":\"Content-Length 必填\"}",
        );
        return (head.method, head.path, head.authorization, Vec::new());
    }
    if content_length > max_body {
        respond(
            &mut stream,
            413,
            "{\"status\":\"failed\",\"retcode\":1413,\"wording\":\"请求体超限\"}",
        );
        return (head.method, head.path, head.authorization, Vec::new());
    }
    if !token_ok(&head.authorization, &head.path, access_token) {
        let (code, wording) =
            if head.authorization.is_none() && !head.path.contains("access_token=") {
                (401, "access token 缺失")
            } else {
                (403, "access token 无效")
            };
        respond(
            &mut stream,
            code,
            &format!(
                "{{\"status\":\"failed\",\"retcode\":{},\"wording\":{:?}}}",
                if code == 401 { 1401 } else { 1403 },
                wording
            ),
        );
        return (head.method, head.path, head.authorization, Vec::new());
    }
    if head.method != "POST" && head.method != "GET" {
        respond(
            &mut stream,
            405,
            "{\"status\":\"failed\",\"retcode\":1405,\"wording\":\"method not allowed\"}",
        );
        return (head.method, head.path, head.authorization, Vec::new());
    }
    if head.method == "POST" {
        let content_type = head
            .content_type
            .as_deref()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !content_type.eq_ignore_ascii_case("application/json")
            && !content_type.eq_ignore_ascii_case("application/x-www-form-urlencoded")
        {
            respond(
                &mut stream,
                406,
                "{\"status\":\"failed\",\"retcode\":1406,\"wording\":\"Content-Type 不支持\"}",
            );
            return (head.method, head.path, head.authorization, Vec::new());
        }
    }
    let total = match end.checked_add(content_length) {
        Some(total) => total,
        None => {
            respond(
                &mut stream,
                400,
                "{\"status\":\"failed\",\"retcode\":1440,\"wording\":\"请求长度溢出\"}",
            );
            return (head.method, head.path, head.authorization, Vec::new());
        }
    };
    while buf.len() < total {
        let remaining = total - buf.len();
        let want = remaining.min(tmp.len());
        if !set_remaining_read_timeout(&stream, read_deadline) {
            respond(
                &mut stream,
                400,
                "{\"status\":\"failed\",\"retcode\":1440,\"wording\":\"请求体读取超时\"}",
            );
            return (head.method, head.path, head.authorization, Vec::new());
        }
        match stream.read(&mut tmp[..want]) {
            Ok(0) | Err(_) => {
                respond(
                    &mut stream,
                    400,
                    "{\"status\":\"failed\",\"retcode\":1440,\"wording\":\"请求体未完整\"}",
                );
                return (head.method, head.path, head.authorization, Vec::new());
            }
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let body = buf[end..total].to_vec();
    let parsed = action_from_request(&head, &body);
    let (code, response) = match parsed {
        Ok((action, params, echo)) => {
            if !crate::onebot::is_supported_action(&action) {
                respond(
                    &mut stream,
                    404,
                    "{\"status\":\"failed\",\"retcode\":1404,\"wording\":\"API 不存在\"}",
                );
                return (head.method, head.path, head.authorization, body);
            }
            let result = handler(&action, &params, echo);
            (200, serde_json::to_string(&result).unwrap_or_default())
        }
        Err(error) => {
            let response =
                ActionResponse::failed(crate::onebot::RET_BAD_REQUEST, &error, Value::Null);
            (400, serde_json::to_string(&response).unwrap_or_default())
        }
    };
    respond(&mut stream, code, &response);
    (head.method, head.path, head.authorization, body)
}

fn set_remaining_read_timeout(stream: &TcpStream, deadline: std::time::Instant) -> bool {
    let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
        return false;
    };
    !remaining.is_zero() && stream.set_read_timeout(Some(remaining)).is_ok()
}

fn respond(stream: &mut TcpStream, code: u16, body: &str) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        406 => "Not Acceptable",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Concurrent, bounded HTTP accept loop. A slow reader occupies one worker
/// only, and the listener refuses excess work instead of creating unbounded
/// threads. On shutdown, remaining workers are joined after their read/action
/// deadlines expire.
pub fn serve_parallel(
    listener: &TcpListener,
    max_body: usize,
    access_token: Option<&str>,
    stop: impl Fn() -> bool,
    handler: std::sync::Arc<dyn Fn(&str, &Value, Value) -> ActionResponse + Send + Sync>,
) -> usize {
    let mut served = 0usize;
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    listener.set_nonblocking(true).ok();
    loop {
        let mut active = Vec::with_capacity(workers.len());
        for worker in workers.drain(..) {
            if worker.is_finished() {
                let _ = worker.join();
            } else {
                active.push(worker);
            }
        }
        workers = active;
        if stop() {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) if workers.len() >= MAX_HTTP_WORKERS => {
                let mut stream = stream;
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
                respond(
                    &mut stream,
                    503,
                    "{\"status\":\"failed\",\"retcode\":1503,\"wording\":\"HTTP workers are busy\"}",
                );
            }
            Ok((stream, _)) => {
                let handler = std::sync::Arc::clone(&handler);
                let token = access_token.map(str::to_owned);
                workers.push(std::thread::spawn(move || {
                    let _ = serve_one(
                        stream,
                        max_body,
                        token.as_deref(),
                        |action, params, echo| handler(action, params, echo),
                    );
                }));
                served += 1;
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    served
}

/// Blocking HTTP accept loop shared by application assembly.
pub fn serve_loop(
    listener: &TcpListener,
    max_body: usize,
    access_token: Option<&str>,
    stop: impl Fn() -> bool,
    handler: &mut dyn FnMut(&str, &Value, Value) -> ActionResponse,
) -> usize {
    let mut served = 0usize;
    listener.set_nonblocking(true).ok();
    loop {
        if stop() {
            return served;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                serve_one(stream, max_body, access_token, |a, p, e| handler(a, p, e));
                served += 1;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            Err(_) => return served,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onebot::StoreReader;
    use caligo_model::onebot_traits::{SendDisposition, SendPayload, SendSink, StoredMessageView};

    struct Sink;
    impl SendSink for Sink {
        fn send(&mut self, _: &SendPayload) -> Result<SendDisposition, String> {
            Ok(SendDisposition::AcceptedUnconfirmed)
        }
    }
    struct Store;
    impl StoreReader for Store {
        fn get_msg(&self, _: i32) -> Option<StoredMessageView> {
            None
        }
        fn login_info(&self) -> Option<(u64, String)> {
            Some((1, "bot".into()))
        }
        fn online_status(&self) -> Option<bool> {
            Some(true)
        }
    }

    fn serve_raw(req: Vec<Vec<u8>>, max_body: usize, token: Option<&str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let token = token.map(str::to_owned);
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_one(stream, max_body, token.as_deref(), |a, p, e| {
                crate::onebot::handle_action(a, p, e, &mut Sink, &Store)
            });
        });
        let mut client = TcpStream::connect(addr).unwrap();
        for chunk in req {
            client.write_all(&chunk).unwrap();
        }
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        response
    }

    #[test]
    fn handles_header_only_then_fragmented_body_exactly() {
        let body = br#"{}"#;
        let header = format!("POST /get_version_info HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len());
        let mut chunks = vec![header.into_bytes()];
        chunks.extend(body.iter().map(|b| vec![*b]));
        let response = serve_raw(chunks, DEFAULT_MAX_BODY, None);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("\"protocol_version\":\"v11\""));
    }

    #[test]
    fn rejects_duplicate_or_invalid_length_and_oversize_before_body() {
        for header in [
            b"POST / HTTP/1.1\r\nContent-Length: 0\r\ncontent-length: 0\r\n\r\n".to_vec(),
            b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n".to_vec(),
        ] {
            let response = serve_raw(vec![header], DEFAULT_MAX_BODY, None);
            assert!(response.starts_with("HTTP/1.1 400"));
        }
        let response = serve_raw(vec![b"POST /get_status HTTP/1.1\r\nContent-Length: 1025\r\nContent-Type: application/json\r\n\r\n".to_vec()], 1024, None);
        assert!(response.starts_with("HTTP/1.1 413"));
    }

    #[test]
    fn supports_get_route_query_and_form_post() {
        let response = serve_raw(
            vec![b"GET /get_version_info HTTP/1.1\r\n\r\n".to_vec()],
            DEFAULT_MAX_BODY,
            None,
        );
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("\"protocol_version\":\"v11\""));
        let body = b"group_id=123&message=hi";
        let header = format!("POST /send_group_msg_async HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n", body.len());
        let response = serve_raw(
            vec![header.into_bytes(), body.to_vec()],
            DEFAULT_MAX_BODY,
            None,
        );
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("\"status\":\"async\""));
        let query = serve_raw(
            vec![b"GET /send_group_msg_async?group_id=123&message=hi HTTP/1.1\r\n\r\n".to_vec()],
            DEFAULT_MAX_BODY,
            None,
        );
        assert!(query.starts_with("HTTP/1.1 200"));
        assert!(query.contains("\"status\":\"async\""));
    }

    #[test]
    fn auth_distinguishes_missing_and_invalid_token_and_get_remains_supported() {
        let req = b"GET /get_status HTTP/1.1\r\n\r\n".to_vec();
        assert!(
            serve_raw(vec![req.clone()], DEFAULT_MAX_BODY, Some("secret"))
                .starts_with("HTTP/1.1 401")
        );
        let bad = b"GET /get_status HTTP/1.1\r\nAuthorization: Bearer nope\r\n\r\n".to_vec();
        assert!(serve_raw(vec![bad], DEFAULT_MAX_BODY, Some("secret")).starts_with("HTTP/1.1 403"));
        let good = b"GET /get_status?access_token=secret HTTP/1.1\r\n\r\n".to_vec();
        assert!(serve_raw(vec![good], DEFAULT_MAX_BODY, Some("secret")).starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn request_parser_keeps_authorization_case_and_rejects_transfer_encoding() {
        let parsed = parse_request_head(
            b"POST / HTTP/1.1\r\nAuthorization: bEaReR token\r\nContent-Length: 2\r\n\r\n",
        )
        .unwrap();
        assert_eq!(parsed.authorization.as_deref(), Some("bEaReR token"));
        assert_eq!(
            parse_request_head(
                b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\n\r\n"
            ),
            Err(HeadError::BadRequest)
        );
    }

    #[test]
    fn slow_header_reader_does_not_block_a_valid_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_stop = std::sync::Arc::clone(&stop);
        let handler: std::sync::Arc<dyn Fn(&str, &Value, Value) -> ActionResponse + Send + Sync> =
            std::sync::Arc::new(|action, params, echo| {
                crate::onebot::handle_action(action, params, echo, &mut Sink, &Store)
            });
        let server = std::thread::spawn(move || {
            serve_parallel(
                &listener,
                DEFAULT_MAX_BODY,
                None,
                move || server_stop.load(std::sync::atomic::Ordering::Relaxed),
                handler,
            )
        });

        let mut slow = TcpStream::connect(address).unwrap();
        slow.write_all(b"GET /get_version_info HTTP/1.1\r\nHost: slow")
            .unwrap();

        let mut fast = TcpStream::connect(address).unwrap();
        fast.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        fast.write_all(b"GET /get_version_info HTTP/1.1\r\nHost: fast\r\n\r\n")
            .unwrap();
        fast.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        fast.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"protocol_version\":\"v11\""));

        let _ = slow.shutdown(std::net::Shutdown::Both);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(server.join().unwrap(), 2);
    }
}
