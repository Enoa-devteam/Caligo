//! caligod — K4-D5 常驻 core 进程(计划 §7-D5)。
//!
//! 职责:打开 journal、创建双管道 server(bridge/control)、运行会话循环;
//! Ctrl+C 与 control Stop 走同一真实 Stopping 协议(actor.stop→close)。
//! 启动参数不携带认证凭据:token 每次启动由 OS 随机生成,打印到 stdout
//! 首行 JSON(仅供本地启动方读取;不进命令行、不进日志文件)。
//!
//! 用法:caligod --pipe-prefix <name> --journal <path> --baseline <id> --account <u>
//!            [--expect-pid <n>] [--core-build <s>] [--media-config <absolute-json>]

use caligo_core::media_tool::{MediaTool, MediaToolConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Ctrl+C 处理器可见的进程级停止标志(与 control Stop 汇合)。
static CTRL_STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> i32 {
    // 约束:控制台处理器内只做原子写,不分配/不等待(计划 §3.1 同源纪律)。
    CTRL_STOP.store(true, Ordering::Relaxed);
    1 // TRUE:已处理
}

#[derive(Debug)]
struct Args {
    pipe_prefix: String,
    journal: String,
    baseline: String,
    account: String,
    expect_pid: u32,
    core_build: String,
    /// 凭据文件:存在则读取(重启连续性),不存在则生成并写入(user-only)。
    /// 计划 §6.4:凭据不进 argv/日志 —— 文件是本地私有通道。
    token_file: Option<String>,
    media_config: Option<PathBuf>,
    onebot_http_bind: String,
    onebot_ws_bind: Option<String>,
    onebot_sse_bind: Option<String>,
    onebot_mcp_bind: Option<String>,
    onebot_http_post_url: Option<String>,
    onebot_http_post_secret_env: Option<String>,
    onebot_http_post_timeout_ms: u64,
    onebot_ws_reverse_url: Option<String>,
    onebot_ws_reverse_token_env: Option<String>,
    onebot_ws_reverse_reconnect_ms: u64,
}

fn parse_args() -> Option<Args> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args {
        pipe_prefix: String::new(),
        journal: String::new(),
        baseline: "qq-9.9.33-52230".into(),
        account: String::new(),
        expect_pid: 0,
        core_build: caligo_core::CORE_BUILD.to_string(),
        token_file: None,
        media_config: None,
        onebot_http_bind: "127.0.0.1:5700".into(),
        onebot_ws_bind: None,
        onebot_sse_bind: None,
        onebot_mcp_bind: None,
        onebot_http_post_url: None,
        onebot_http_post_secret_env: None,
        onebot_http_post_timeout_ms: 5_000,
        onebot_ws_reverse_url: None,
        onebot_ws_reverse_token_env: None,
        onebot_ws_reverse_reconnect_ms: 3_000,
    };
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--pipe-prefix" => a.pipe_prefix = next(&mut i)?,
            "--journal" => a.journal = next(&mut i)?,
            "--baseline" => a.baseline = next(&mut i)?,
            "--account" => a.account = next(&mut i)?,
            "--expect-pid" => a.expect_pid = next(&mut i)?.parse().ok()?,
            "--core-build" => a.core_build = next(&mut i)?,
            "--token-file" => a.token_file = next(&mut i),
            "--media-config" => a.media_config = Some(PathBuf::from(next(&mut i)?)),
            "--onebot-http-bind" => a.onebot_http_bind = next(&mut i)?,
            "--onebot-ws-bind" => a.onebot_ws_bind = Some(next(&mut i)?),
            "--onebot-sse-bind" => a.onebot_sse_bind = Some(next(&mut i)?),
            "--onebot-mcp-bind" => a.onebot_mcp_bind = Some(next(&mut i)?),
            "--onebot-http-post-url" => a.onebot_http_post_url = Some(next(&mut i)?),
            "--onebot-http-post-secret-env" => a.onebot_http_post_secret_env = Some(next(&mut i)?),
            "--onebot-http-post-timeout-ms" => {
                a.onebot_http_post_timeout_ms = next(&mut i)?.parse().ok()?
            }
            "--onebot-ws-reverse-url" => a.onebot_ws_reverse_url = Some(next(&mut i)?),
            "--onebot-ws-reverse-token-env" => a.onebot_ws_reverse_token_env = Some(next(&mut i)?),
            "--onebot-ws-reverse-reconnect-ms" => {
                a.onebot_ws_reverse_reconnect_ms = next(&mut i)?.parse().ok()?
            }
            _ => {}
        }
        i += 1;
    }
    if a.pipe_prefix.is_empty() || a.journal.is_empty() || a.account.is_empty() {
        eprintln!(
            "caligod --pipe-prefix <name> --journal <path> --baseline <id> --account <u> [--expect-pid <n>] [--core-build <s>] [--media-config <absolute-json>] [--onebot-http-bind 127.0.0.1:5700] [--onebot-ws-bind 127.0.0.1:6700] [--onebot-sse-bind 127.0.0.1:6701] [--onebot-mcp-bind 127.0.0.1:6702] [--onebot-http-post-url <url>] [--onebot-http-post-secret-env <name>] [--onebot-ws-reverse-url <ws-url>] [--onebot-ws-reverse-token-env <name>]"
        );
        return None;
    }
    Some(a)
}

fn read_secret_env(name: Option<&str>) -> Result<Option<String>, String> {
    let Some(name) = name else { return Ok(None) };
    std::env::var(name)
        .map(Some)
        .map_err(|_| format!("configured credential environment variable {name} is unavailable"))
}

fn main() {
    let Some(args) = parse_args() else {
        std::process::exit(2);
    };
    let reverse_self_id = if args.onebot_ws_reverse_url.is_some() {
        match args.account.parse::<u64>() {
            Ok(id) if id > 0 => Some(id),
            _ => {
                eprintln!(
                    "[caligod] reverse OneBot WebSocket requires a positive numeric account ID"
                );
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    let http_post_secret = match read_secret_env(args.onebot_http_post_secret_env.as_deref()) {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!("[caligod] {error}");
            std::process::exit(2);
        }
    };
    let reverse_token = match read_secret_env(args.onebot_ws_reverse_token_env.as_deref()) {
        Ok(token) => token,
        Err(error) => {
            eprintln!("[caligod] {error}");
            std::process::exit(2);
        }
    };
    let voice_media = args.media_config.as_deref().map(|path| {
        MediaToolConfig::read_from(path).and_then(MediaTool::for_voice_records)
            .unwrap_or_else(|error| {
                eprintln!("[caligod] voice media initialization failed: {error}");
                std::process::exit(2);
            })
    });
    let config = caligo_core::daemon::DaemonConfig {
        pipe_prefix: args.pipe_prefix,
        journal_path: args.journal.into(),
        module_baseline: args.baseline,
        account: args.account,
        core_build: args.core_build,
        expect_client_pid: args.expect_pid,
        runtime: Default::default(),
        test_fault_break_bridge_after_hello: false,
        test_fault_break_bridge_after_result: false,
    };
    // 凭据:token-file 优先(重启连续性;文件 user-only,不进 argv/日志)。
    // Daemon::start 自身生成随机 token —— token-file 模式下用文件内容覆盖。
    let daemon = match caligo_core::daemon::Daemon::start_with_voice_media(config, voice_media) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[caligod] journal open failed(不静默重建): {e}");
            std::process::exit(3);
        }
    };
    if let Some(tf) = &args.token_file {
        let token = match std::fs::read_to_string(tf) {
            Ok(t) if t.trim().len() == 64 => t.trim().to_string(),
            _ => {
                let t = daemon.auth_token();
                let _ = std::fs::write(tf, &t);
                t
            }
        };
        daemon.set_auth_token(&token);
    }
    let listener = match caligo_core::onebot_server::bind_loopback(&args.onebot_http_bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("[caligod] OneBot HTTP bind failed: {error}");
            std::process::exit(6);
        }
    };
    let onebot_http_addr: SocketAddr = listener.local_addr().expect("bound listener address");
    let ws_listener = match args.onebot_ws_bind.as_deref() {
        Some(address) => match caligo_core::onebot_server::bind_loopback(address) {
            Ok(listener) => Some(listener),
            Err(error) => {
                eprintln!("[caligod] OneBot WebSocket bind failed: {error}");
                std::process::exit(6);
            }
        },
        None => None,
    };
    let onebot_ws_addr = ws_listener
        .as_ref()
        .map(|listener| listener.local_addr().expect("bound websocket address"));
    let bind_optional = |address: Option<&str>, protocol: &str| {
        address.map(|address| {
            caligo_core::onebot_server::bind_loopback(address).unwrap_or_else(|error| {
                eprintln!("[caligod] {protocol} bind failed: {error}");
                std::process::exit(6);
            })
        })
    };
    let sse_listener = bind_optional(args.onebot_sse_bind.as_deref(), "OneBot SSE");
    let mcp_listener = bind_optional(args.onebot_mcp_bind.as_deref(), "MCP");
    let onebot_sse_addr = sse_listener
        .as_ref()
        .map(|listener| listener.local_addr().expect("bound SSE address"));
    let onebot_mcp_addr = mcp_listener
        .as_ref()
        .map(|listener| listener.local_addr().expect("bound MCP address"));
    let (bridge_name, control_name) = daemon.pipe_names();
    // 启动信息首行(JSON):管道名 + 认证 token。启动方读取后应立即吞掉该输出。
    println!(
        "{}",
        serde_json::json!({
            "bridge_pipe": bridge_name,
            "control_pipe": control_name,
            "auth_token": daemon.auth_token(),
            "onebot_http": onebot_http_addr,
            "onebot_ws": onebot_ws_addr,
            "onebot_sse": onebot_sse_addr,
            "onebot_mcp": onebot_mcp_addr,
            "http_post_enabled": args.onebot_http_post_url.is_some(),
            "reverse_ws_enabled": args.onebot_ws_reverse_url.is_some(),
        })
    );

    let onebot_server = {
        let d = daemon.clone();
        let token = daemon.auth_token();
        std::thread::spawn(move || {
            caligo_core::onebot_server::serve_http(d, &listener, Some(&token))
        })
    };
    let onebot_ws_server = ws_listener.map(|listener| {
        let d = daemon.clone();
        let token = daemon.auth_token();
        std::thread::spawn(move || {
            caligo_core::onebot_transport::serve_forward_ws(d, listener, Some(token))
        })
    });
    let onebot_sse_server = sse_listener.map(|listener| {
        let d = daemon.clone();
        let token = daemon.auth_token();
        std::thread::spawn(move || caligo_core::onebot_sse::serve_sse(d, listener, Some(token)))
    });
    let onebot_mcp_server = mcp_listener.map(|listener| {
        let d = daemon.clone();
        let token = daemon.auth_token();
        std::thread::spawn(move || caligo_core::onebot_mcp::serve_mcp(d, listener, Some(token)))
    });
    let http_post_worker = args.onebot_http_post_url.map(|url| {
        daemon.enable_onebot_http_post_ordering();
        let d = daemon.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            caligo_core::onebot_transport::run_http_post_ready(
                d,
                caligo_core::onebot_transport::HttpPostConfig {
                    url,
                    secret: http_post_secret,
                    timeout: std::time::Duration::from_millis(args.onebot_http_post_timeout_ms),
                },
                ready_tx,
            )
        });
        if ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_err()
        {
            eprintln!("[caligod] OneBot HTTP POST worker did not become ready");
            daemon.stop_flag().store(true, Ordering::Relaxed);
            let _ = worker.join();
            std::process::exit(7);
        }
        worker
    });
    let reverse_ws_worker = args.onebot_ws_reverse_url.map(|url| {
        let d = daemon.clone();
        let self_id = reverse_self_id.expect("validated before daemon startup");
        let reconnect_interval =
            std::time::Duration::from_millis(args.onebot_ws_reverse_reconnect_ms);
        std::thread::spawn(move || {
            caligo_core::onebot_transport::run_reverse_ws(
                d,
                caligo_core::onebot_transport::ReverseWsConfig {
                    url,
                    access_token: reverse_token,
                    self_id,
                    reconnect_interval,
                },
            )
        })
    });

    // SAFETY: 处理器为进程生存期静态 fn;仅置原子标志。
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_handler), 1);
    }

    // daemon.run() 阻塞(accept 循环以 100ms 粒度轮询停止标志);
    // 主线程汇合 Ctrl+C 与 control Stop 两个来源。
    let runner = {
        let d = daemon.clone();
        std::thread::spawn(move || d.run())
    };
    loop {
        if CTRL_STOP.load(Ordering::Relaxed) {
            daemon.stop_flag().store(true, Ordering::Relaxed);
        }
        if runner.is_finished() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    match runner.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("[caligod] session error: {e}");
            std::process::exit(4);
        }
        Err(_) => {
            eprintln!("[caligod] run thread panicked");
            std::process::exit(5);
        }
    }
    let served = onebot_server.join().expect("OneBot HTTP thread panicked");
    println!("[caligod] OneBot HTTP 已停止，共处理 {served} 个请求");
    if let Some(server) = onebot_ws_server {
        println!(
            "[caligod] OneBot WebSocket 已停止，共接入 {} 个连接",
            server.join().expect("OneBot WS thread panicked")
        );
    }
    if let Some(server) = onebot_sse_server {
        match server.join().expect("OneBot SSE thread panicked") {
            Ok(stats) => println!("[caligod] OneBot SSE stopped: {stats:?}"),
            Err(error) => eprintln!("[caligod] OneBot SSE failed: {error}"),
        }
    }
    if let Some(server) = onebot_mcp_server {
        match server.join().expect("MCP thread panicked") {
            Ok(stats) => println!("[caligod] MCP stopped: {stats:?}"),
            Err(error) => eprintln!("[caligod] MCP failed: {error}"),
        }
    }
    if let Some(worker) = http_post_worker {
        println!(
            "[caligod] OneBot HTTP POST 已停止，共上报 {} 条事件",
            worker.join().expect("OneBot HTTP POST thread panicked")
        );
    }
    if let Some(worker) = reverse_ws_worker {
        println!(
            "[caligod] OneBot 反向 WebSocket 已停止，共建立 {} 次连接",
            worker.join().expect("OneBot reverse WS thread panicked")
        );
    }
    println!("[caligod] stopped(closed;journal 保留)");
}
