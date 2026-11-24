use caligo_core::onebot::ActionResponse;
use caligo_core::onebot_http::{serve_loop, serve_parallel, DEFAULT_MAX_BODY};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn request_with_pause(address: SocketAddr, split: bool) -> Result<String, String> {
    let mut stream = TcpStream::connect(address).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    let body = br#"{"action":"get_status","params":{},"echo":"pause-test"}"#;
    if split {
        let head = format!("POST / HTTP/1.1\r\nHost: local\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len());
        stream.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
        stream.write_all(&body[..1]).map_err(|e| e.to_string())?;
    }
    std::thread::sleep(Duration::from_millis(250));
    if split {
        stream.write_all(&body[1..]).map_err(|e| e.to_string())?;
    } else {
        stream.write_all(b"GET /get_status HTTP/1.1\r\nHost: local\r\n\r\n").map_err(|e| e.to_string())?;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|e| e.to_string())?;
    Ok(response)
}

fn check_delayed_clients(parallel: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        if parallel {
            let handler: Arc<dyn Fn(&str, &Value, Value) -> ActionResponse + Send + Sync> =
                Arc::new(|_, _, echo| ActionResponse::ok(json!({"online":true}), echo));
            serve_parallel(&listener, DEFAULT_MAX_BODY, None,
                move || server_stop.load(Ordering::SeqCst), handler)
        } else {
            let mut handler = |_: &str, _: &Value, echo: Value| ActionResponse::ok(json!({"online":true}), echo);
            serve_loop(&listener, DEFAULT_MAX_BODY, None,
                move || server_stop.load(Ordering::SeqCst), &mut handler)
        }
    });
    let delayed_header = request_with_pause(address, false);
    let split_body = request_with_pause(address, true);
    stop.store(true, Ordering::SeqCst);
    let served = server.join().unwrap();
    assert_eq!(served, 2);
    for (case, result) in [("delayed header", delayed_header), ("split body", split_body)] {
        let response = result.unwrap_or_else(|e| panic!("{case} failed: {e}"));
        assert!(response.starts_with("HTTP/1.1 200"), "{case}: {response}");
        assert!(response.contains("\"retcode\":0"), "{case}: {response}");
    }
}

#[test]
fn parallel_http_accepts_delayed_headers_and_body_chunks() { check_delayed_clients(true); }

#[test]
fn sequential_http_accepts_delayed_headers_and_body_chunks() { check_delayed_clients(false); }
