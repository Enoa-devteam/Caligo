//! Local management HTTP/UI over the real AccountManager. No QQ attachment.
//! The caller provisions the private manager credential; it is never config data.

use crate::account_manager::{AccountManager, ManagerConfig};
use crate::onebot::{StoreReader, ACTION_REGISTRY};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 256 * 1024;
const MAX_WORKERS: usize = 8;
const READ_DEADLINE: Duration = Duration::from_secs(5);
const IO_SLICE: Duration = Duration::from_millis(100);

/// Supply a 64-hex credential from a separately provisioned private token file.
/// Debug is deliberately not derived: it must not print credentials.
pub struct ManagementHttpConfig {
    pub bind: SocketAddr,
    pub bearer_token: String,
}

struct Context {
    manager: Arc<Mutex<AccountManager>>,
    stop: Arc<AtomicBool>,
    token: String,
    authority: String,
    origin: String,
    nonce: String,
    port: u16,
}

/// Owns only HTTP workers. Stopping this listener does not stop managed accounts.
pub struct ManagementHttpServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    runner: Option<JoinHandle<io::Result<()>>>,
}
impl ManagementHttpServer {
    pub fn start(
        config: ManagementHttpConfig,
        manager: Arc<Mutex<AccountManager>>,
    ) -> io::Result<Self> {
        if !config.bind.ip().is_loopback()
            || config.bearer_token.len() != 64
            || !config.bearer_token.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "management requires loopback and a 64-hex private credential",
            ));
        }
        let listener = TcpListener::bind(config.bind)?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let nonce = crate::transport::generate_token()
            .map_err(|_| io::Error::other("management nonce generation failed"))?;
        let context = Arc::new(Context {
            manager,
            stop: Arc::clone(&stop),
            token: config.bearer_token,
            authority: address.to_string(),
            origin: format!("http://{address}"),
            nonce: crate::transport::token_hex(&nonce),
            port: address.port(),
        });
        let runner = thread::Builder::new()
            .name("caligo-management-http".into())
            .spawn(move || serve(listener, context))?;
        Ok(Self {
            address,
            stop,
            runner: Some(runner),
        })
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(runner) = self.runner.take() {
            runner
                .join()
                .map_err(|_| io::Error::other("management listener panicked"))??;
        }
        Ok(())
    }
}
impl Drop for ManagementHttpServer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn serve(listener: TcpListener, context: Arc<Context>) -> io::Result<()> {
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut result = Ok(());
    while !context.stop.load(Ordering::Acquire) {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let _ = workers.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let _ = stream.set_write_timeout(Some(IO_SLICE));
                if workers.len() >= MAX_WORKERS {
                    // Overload never blocks the accept loop on a stalled client.
                    let _ = stream.set_nonblocking(true);
                    let _ = reply(&mut stream, &context, failure(503, "management busy"));
                    continue;
                }
                let worker_context = Arc::clone(&context);
                match thread::Builder::new()
                    .name("caligo-management-request".into())
                    .spawn(move || {
                        let response = handle(&mut stream, &worker_context)
                            .unwrap_or_else(|(code, message)| failure(code, message));
                        let _ = stream.set_write_timeout(Some(IO_SLICE));
                        let _ = reply(&mut stream, &worker_context, response);
                    }) {
                    Ok(worker) => workers.push(worker),
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    context.stop.store(true, Ordering::Release);
    for worker in workers {
        let _ = worker.join();
    }
    result
}

type HttpResult<T> = Result<T, (u16, &'static str)>;
struct Head {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    length: usize,
}
fn parse_head(bytes: &[u8]) -> HttpResult<Head> {
    let text = std::str::from_utf8(bytes).map_err(|_| (400, "invalid HTTP headers"))?;
    let mut lines = text.split("\r\n");
    let line = lines.next().ok_or((400, "missing request line"))?;
    let parts: Vec<_> = line.split(' ').collect();
    if parts.len() != 3
        || parts[2] != "HTTP/1.1"
        || !parts[1].starts_with('/')
        || parts[1].bytes().any(|b| b <= 32 || b >= 127)
        || parts[1].contains(['?', '#', '%', '\\'])
        || !parts[0].bytes().all(|b| b.is_ascii_uppercase())
    {
        return Err((400, "expected an unencoded HTTP/1.1 path without query"));
    }
    let mut headers = BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or((400, "invalid header"))?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127)
            || headers
                .insert(name.to_ascii_lowercase(), value.trim().to_string())
                .is_some()
        {
            return Err((400, "invalid or duplicate header"));
        }
    }
    if headers.contains_key("transfer-encoding") || headers.contains_key("expect") {
        return Err((400, "transfer encoding and expect are unsupported"));
    }
    let length = match headers.get("content-length") {
        None => 0,
        Some(value) => {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err((400, "invalid content length"));
            }
            value
                .parse::<usize>()
                .map_err(|_| (413, "body too large"))?
        }
    };
    if length > MAX_BODY {
        return Err((413, "body too large"));
    }
    Ok(Head {
        method: parts[0].to_string(),
        path: parts[1].to_string(),
        headers,
        length,
    })
}
fn read_more(
    stream: &mut TcpStream,
    bytes: &mut Vec<u8>,
    max: usize,
    context: &Context,
    deadline: Instant,
) -> HttpResult<()> {
    loop {
        if context.stop.load(Ordering::Acquire) {
            return Err((503, "management stopping"));
        }
        if Instant::now() >= deadline {
            return Err((408, "request deadline exceeded"));
        }
        let mut block = [0u8; 4096];
        let remaining = max.saturating_sub(bytes.len()).min(block.len());
        if remaining == 0 {
            return Err((413, "request too large"));
        }
        match stream.read(&mut block[..remaining]) {
            Ok(0) => return Err((400, "incomplete request")),
            Ok(count) => {
                bytes.extend_from_slice(&block[..count]);
                return Ok(());
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err((400, "request read failed")),
        }
    }
}
fn equal_token(actual: &str, expected: &str) -> bool {
    let mut mismatch = actual.len() ^ expected.len();
    for (index, byte) in expected.bytes().enumerate() {
        mismatch |= usize::from(byte ^ actual.as_bytes().get(index).copied().unwrap_or(0));
    }
    mismatch == 0
}
fn handle(stream: &mut TcpStream, context: &Context) -> HttpResult<Response> {
    stream
        .set_read_timeout(Some(IO_SLICE))
        .map_err(|_| (500, "request setup failed"))?;
    let deadline = Instant::now() + READ_DEADLINE;
    let mut bytes = Vec::new();
    let head_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() >= MAX_HEADER {
            return Err((431, "headers too large"));
        }
        read_more(stream, &mut bytes, MAX_HEADER, context, deadline)?;
    };
    let head = parse_head(&bytes[..head_end])?;
    if head.headers.get("host") != Some(&context.authority) {
        return Err((403, "unexpected management host"));
    }
    if head
        .headers
        .get("origin")
        .is_some_and(|origin| origin != &context.origin)
        || head
            .headers
            .get("sec-fetch-site")
            .is_some_and(|site| !matches!(site.as_str(), "same-origin" | "none"))
    {
        return Err((403, "cross-origin management request rejected"));
    }
    let homepage = head.method == "GET" && head.path == "/";
    if !homepage {
        let supplied = head
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or("");
        if !equal_token(supplied, &context.token) {
            return Err((401, "manager credential required"));
        }
    }
    let writing = matches!(head.method.as_str(), "PUT" | "POST");
    if writing {
        if !head.headers.contains_key("content-length") {
            return Err((411, "content length required"));
        }
        let content_type = head.headers.get("content-type").map(String::as_str);
        if !content_type.is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value.eq_ignore_ascii_case("application/json; charset=utf-8")
        }) {
            return Err((415, "application/json required"));
        }
    } else if head.length != 0 {
        return Err((400, "read request must have no body"));
    }
    let expected = head_end + head.length;
    if bytes.len() > expected {
        return Err((400, "pipelining and excess body bytes are unsupported"));
    }
    while bytes.len() < expected {
        read_more(stream, &mut bytes, expected, context, deadline)?;
    }
    if homepage {
        return Ok(Response {
            code: 200,
            mime: "text/html; charset=utf-8",
            body: UI.replace("__NONCE__", &context.nonce),
        });
    }
    let mut manager = loop {
        if context.stop.load(Ordering::Acquire) {
            return Err((503, "management stopping"));
        }
        match context.manager.try_lock() {
            Ok(manager) => {
                if context.stop.load(Ordering::Acquire) {
                    return Err((503, "management stopping"));
                }
                break manager;
            }
            Err(TryLockError::Poisoned(_)) => return Err((500, "manager unavailable")),
            Err(TryLockError::WouldBlock) => {
                if context.stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                    return Err((503, "manager busy"));
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    };
    route(&head, &bytes[head_end..], &mut manager, context)
}
fn account_view(manager: &AccountManager, account: u64) -> Value {
    let running = manager.running_accounts().contains(&account);
    // StoreReader reports actor session state, not a proven QQ login observation.
    let session_active = manager
        .daemon(account)
        .ok()
        .and_then(|daemon| daemon.online_status());
    json!({
        "account": account.to_string(), "running": running, "session_active": session_active,
        "online": null, "online_reason": "QQ login not observed",
        "status": if running { "running" } else { "stopped" }
    })
}
fn route(
    head: &Head,
    body: &[u8],
    manager: &mut AccountManager,
    context: &Context,
) -> HttpResult<Response> {
    match (head.method.as_str(), head.path.as_str()) {
        ("POST", "/v1/shutdown") => {
            empty_body(body)?;
            let stopped = manager.shutdown();
            context.stop.store(true, Ordering::Release);
            stopped.map_err(|_| (500, "manager shutdown completed with account errors"))?;
            return Ok(success(json!({"status":"stopped"})));
        }
        ("GET", "/v1/accounts") => {
            let accounts: Vec<_> = manager
                .config()
                .accounts
                .iter()
                .map(|spec| account_view(manager, spec.account))
                .collect();
            return Ok(success(json!({"accounts": accounts})));
        }
        ("GET", "/v1/config") => return Ok(success(json!(manager.config()))),
        ("PUT", "/v1/config") => {
            if !manager.running_accounts().is_empty() {
                return Err((
                    409,
                    "stop all running accounts before editing configuration",
                ));
            }
            // Deserializing the struct directly rejects unknown and duplicate fields.
            let config: ManagerConfig = serde_json::from_slice(body)
                .map_err(|_| (400, "invalid or unknown account configuration fields"))?;
            config
                .validate()
                .map_err(|_| (400, "invalid account configuration"))?;
            if config.accounts.iter().any(|account| {
                [
                    account.ports.http,
                    account.ports.websocket,
                    account.ports.sse,
                    account.ports.mcp,
                ]
                .into_iter()
                .flatten()
                .any(|port| port == context.port)
            }) {
                return Err((409, "account port conflicts with management listener"));
            }
            let root = manager.root().to_path_buf();
            let replacement = AccountManager::new(&root, config.clone())
                .map_err(|_| (422, "account paths rejected"))?;
            let readback = config
                .save_atomic(&root)
                .map_err(|_| (500, "configuration persistence or readback failed"))?;
            *manager = replacement;
            return Ok(success(json!(readback)));
        }
        _ => {}
    }
    let segments: Vec<_> = head.path.split('/').collect();
    if segments.len() != 5 || segments[1] != "v1" || segments[2] != "accounts" {
        return Err((404, "management route not found"));
    }
    let id = segments[3];
    if id.is_empty() || id.starts_with('0') || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err((400, "invalid account id"));
    }
    let account: u64 = id.parse().map_err(|_| (400, "invalid account id"))?;
    if !manager
        .config()
        .accounts
        .iter()
        .any(|spec| spec.account == account)
    {
        return Err((404, "account not configured"));
    }
    match (head.method.as_str(), segments[4]) {
        ("GET", "capabilities") => Ok(success(json!({
            "account": account.to_string(), "declared": ACTION_REGISTRY,
            "backend_state": "unknown", "advertised": null,
            "reason": "daemon does not expose an advertised capability snapshot"
        }))),
        ("POST", operation @ ("start" | "stop")) => {
            empty_body(body)?;
            if operation == "start" {
                if manager.running_accounts().contains(&account) {
                    return Err((409, "account already running"));
                }
                manager
                    .start(account)
                    .map_err(|_| (500, "account startup failed"))?;
            } else {
                manager
                    .stop(account)
                    .map_err(|_| (500, "account stop or join failed"))?;
            }
            Ok(success(account_view(manager, account)))
        }
        _ => Err((405, "method not allowed for management resource")),
    }
}
fn empty_body(body: &[u8]) -> HttpResult<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    let _: Empty = serde_json::from_slice(body)
        .map_err(|_| (400, "lifecycle body must be an empty JSON object"))?;
    Ok(())
}
struct Response {
    code: u16,
    mime: &'static str,
    body: String,
}
fn success(value: Value) -> Response {
    Response {
        code: 200,
        mime: "application/json; charset=utf-8",
        body: value.to_string(),
    }
}
fn failure(code: u16, message: &str) -> Response {
    Response {
        code,
        mime: "application/json; charset=utf-8",
        body: json!({"error": message}).to_string(),
    }
}
fn reply(stream: &mut TcpStream, context: &Context, response: Response) -> io::Result<()> {
    let reason = match response.code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let challenge = if response.code == 401 {
        "WWW-Authenticate: Bearer\r\n"
    } else {
        ""
    };
    let bytes = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'nonce-{}'; style-src 'nonce-{}'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n{}\r\n{}",
        response.code, reason, response.mime, response.body.len(), context.nonce, context.nonce,
        challenge, response.body
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut remaining = bytes.as_bytes();
    while !remaining.is_empty() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "response deadline exceeded",
            ));
        }
        match stream.write(remaining) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "response closed")),
            Ok(count) => remaining = &remaining[count..],
            // Nonblocking overload sockets fail here immediately; normal workers
            // also stop on timeout, so a stalled reader never retains a worker.
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

const UI: &str = r##"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Caligo · 本机管理</title><style nonce="__NONCE__">
:root{color-scheme:dark;font:15px/1.6 system-ui,sans-serif;background:#10151e;color:#e4eaf3}*{box-sizing:border-box}
body{margin:0}main{max-width:1080px;margin:40px auto;padding:0 24px}h1{font-size:30px;margin:0}h2{font-size:19px;margin:0 0 14px}
p{color:#aebcd0}section{background:#192230;border:1px solid #314158;border-radius:12px;padding:24px;margin:20px 0}
label{display:block;color:#c7d4e6;margin-bottom:8px}input,textarea,button{font:inherit;border:1px solid #50627c;border-radius:6px;padding:9px 13px}
input,textarea{background:#0e1723;color:#e4eaf3}input{width:min(100%,560px)}button{background:#2c486b;color:white;cursor:pointer;margin:4px 8px 4px 0}
button:hover{background:#3a5d87}button:disabled{opacity:.45;cursor:wait}textarea{width:100%;min-height:340px;font:13px/1.6 ui-monospace,monospace;tab-size:2;resize:vertical}
.row{padding:14px 0;border-top:1px solid #314158;display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap}
small{display:block;color:#aebcd0}#feedback{white-space:pre-wrap;overflow-wrap:anywhere;min-height:30px}#capabilities{white-space:pre-wrap;overflow-wrap:anywhere;color:#c7d4e6}
.good{color:#8cdbb6}.bad{color:#ffb4ad}.toolbar{display:flex;flex-wrap:wrap;gap:8px;align-items:center}@media(max-width:600px){main{margin:20px auto;padding:0 14px}section{padding:17px}h1{font-size:25px}}
</style></head><body><main><header><h1>Caligo 本机管理</h1><p>账号进程、配置与能力声明。运行状态与 QQ 登录状态分别显示。</p></header>
<section><h2>连接管理器</h2><label for="credential">管理凭据（本机私有 token 文件）</label><input id="credential" type="password" autocomplete="off" spellcheck="false" placeholder="输入 64 位十六进制凭据">
<div class="toolbar"><button id="connect">连接并读取</button><button id="forget">清除凭据</button><button id="refresh">刷新账号</button><button id="shutdown">关闭管理器</button></div>
<small>凭据仅在当前页面内存中使用，刷新页面后重新输入。</small><p id="feedback" role="status" aria-live="polite">尚未连接。请输入管理凭据。</p></section>
<section><h2>账号状态</h2><div id="accounts">尚未读取账号。</div><pre id="capabilities"></pre></section>
<section><h2>账号配置</h2><p>编辑实际 accounts.json 的 JSON 内容。所有账号停止后才可保存；保存成功后显示服务器落盘回读的配置。</p>
<label for="config">配置 JSON</label><textarea id="config" spellcheck="false" aria-label="账号配置 JSON"></textarea>
<div class="toolbar"><button id="load">读取配置</button><button id="save">保存并回读</button></div></section>
<footer><p>核心 daemon 启动不代表 QQ 已登录。能力声明不代表后端已启用或现场验收通过。</p></footer></main>
<script nonce="__NONCE__">
'use strict';
let token='';let busy=false;const byId=id=>document.getElementById(id);
function feedback(text,ok=false){byId('feedback').textContent=text;byId('feedback').className=ok?'good':'bad';}
async function api(path,method='GET',body){if(!token)throw new Error('请先输入管理凭据并连接。');const options={method,headers:{Authorization:'Bearer '+token},cache:'no-store',credentials:'omit',redirect:'error'};if(body!==undefined){options.headers['Content-Type']='application/json';options.body=body;}const response=await fetch(path,options);const data=await response.json();if(!response.ok)throw new Error('HTTP '+response.status+' · '+(data.error||'请求失败'));return data;}
function renderAccounts(data){const box=byId('accounts');box.replaceChildren();for(const account of data.accounts){const row=document.createElement('div');row.className='row';const label=document.createElement('div');const title=document.createElement('strong');title.textContent='账号 '+account.account;const state=document.createElement('small');state.textContent='核心 '+(account.running?'运行中':'已停止')+' · QQ '+(account.online===true?'在线':account.online===false?'离线':'未知');label.append(title,state);const controls=document.createElement('div');for(const [action,caption] of [['start','启动'],['stop','停止'],['capabilities','查看能力']]){const button=document.createElement('button');button.textContent=caption;button.disabled=busy;button.addEventListener('click',()=>run(async()=>{const path='/v1/accounts/'+encodeURIComponent(account.account)+'/'+action;if(action==='capabilities'){const result=await api(path);byId('capabilities').textContent='账号 '+result.account+' · 后端 '+result.backend_state+'\n'+result.declared.map(item=>item.name+' ['+item.permission+']').join('\n');feedback('能力声明已读取；后端广告状态为 '+result.backend_state,true);}else{const result=await api(path,'POST','{}');await refresh();feedback('账号 '+result.account+' · 核心 '+result.status+' · QQ '+(result.online===true?'在线':result.online===false?'离线':'未知'),true);}}));controls.append(button);}row.append(label,controls);box.append(row);}}
async function refresh(){renderAccounts(await api('/v1/accounts'));}
async function load(){byId('config').value=JSON.stringify(await api('/v1/config'),null,2);}
async function run(task){if(busy)return;busy=true;document.querySelectorAll('button').forEach(button=>button.disabled=true);try{await task();}catch(error){feedback(error.message);}finally{busy=false;document.querySelectorAll('button').forEach(button=>button.disabled=false);}}
byId('connect').addEventListener('click',()=>run(async()=>{token=byId('credential').value.trim();byId('credential').value='';await refresh();await load();feedback('已连接，账号与实际配置已读取。',true);}));
byId('forget').addEventListener('click',()=>{token='';byId('credential').value='';byId('config').value='';byId('accounts').textContent='凭据已清除。';byId('capabilities').textContent='';feedback('管理凭据已从页面内存清除。',true);});
byId('refresh').addEventListener('click',()=>run(async()=>{await refresh();feedback('账号状态已刷新。',true);}));
byId('shutdown').addEventListener('click',()=>run(async()=>{await api('/v1/shutdown','POST','{}');token='';byId('config').value='';byId('accounts').textContent='管理器已关闭，所有账号已停止。';byId('capabilities').textContent='';feedback('管理器已关闭。重新使用需从本机命令启动。',true);}));
byId('load').addEventListener('click',()=>run(async()=>{await load();feedback('实际配置已读取。',true);}));
byId('save').addEventListener('click',()=>run(async()=>{const body=byId('config').value;JSON.parse(body);const saved=await api('/v1/config','PUT',body);byId('config').value=JSON.stringify(saved,null,2);await load();await refresh();feedback('配置已原子保存并回读。',true);}));
</script></body></html>"##;
