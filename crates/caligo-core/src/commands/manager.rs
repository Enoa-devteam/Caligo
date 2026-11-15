//! Private multi-account core manager.
//!
//! This binary owns only core daemons and loopback OneBot listeners. It never
//! attaches QQ and never writes authentication material to stdout.

use crate::account_console::{connection_id, parse_account, recent_messages, verify_login};
use crate::account_manager::{AccountManager, ManagerConfig, PortConfig};
use crate::daemon::Daemon;
use crate::management_http::{ManagementHttpConfig, ManagementHttpServer};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static CTRL_STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> i32 {
    CTRL_STOP.store(true, Ordering::Release);
    1
}

struct Args {
    root: PathBuf,
    check: bool,
    set_config: Option<PathBuf>,
    management_bind: Option<SocketAddr>,
    select: bool,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut root = None;
    let mut check = false;
    let mut set_config = None;
    let mut management_bind = None;
    let mut select = false;
    let mut index = 0;
    while index < raw.len() {
        match raw[index].as_str() {
            "--root" => {
                index += 1;
                root = raw.get(index).map(PathBuf::from);
            }
            "--check" => check = true,
            "--select" => select = true,
            "--set-config" => {
                index += 1;
                set_config = raw.get(index).map(PathBuf::from);
            }
            "--management-bind" => {
                index += 1;
                let value = raw
                    .get(index)
                    .ok_or_else(|| "--management-bind <loopback> is required".to_string())?;
                let address: SocketAddr = value
                    .parse()
                    .map_err(|_| "invalid --management-bind address".to_string())?;
                if !address.ip().is_loopback() {
                    return Err("--management-bind must be a loopback address".into());
                }
                management_bind = Some(address);
            }
            value => return Err(format!("unknown option {value}")),
        }
        index += 1;
    }
    let root = root.ok_or_else(|| "--root <absolute-private-root> is required".to_string())?;
    if !root.is_absolute() {
        return Err("--root must be an absolute path".into());
    }
    if let Some(path) = &set_config {
        if !path.is_absolute() {
            return Err("--set-config must be an absolute path".into());
        }
    }
    Ok(Args {
        root,
        check,
        set_config,
        management_bind,
        select,
    })
}

fn read_config_file(path: &Path) -> Result<ManagerConfig, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "cannot read config input".to_string())?;
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read config input".to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("config input is too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid account configuration JSON".into())
}

fn endpoint(port: Option<u16>) -> Result<Option<(TcpListener, String)>, String> {
    let Some(port) = port else { return Ok(None) };
    let listener = crate::onebot_server::bind_loopback(&format!("127.0.0.1:{port}"))
        .map_err(|_| "loopback listener bind failed".to_string())?;
    let address = listener
        .local_addr()
        .map_err(|_| "listener address unavailable".to_string())?
        .to_string();
    Ok(Some((listener, address)))
}

struct BoundAccount {
    account: u64,
    daemon: Arc<Daemon>,
    token: String,
    http: Option<(TcpListener, String)>,
    websocket: Option<(TcpListener, String)>,
    sse: Option<(TcpListener, String)>,
    mcp: Option<(TcpListener, String)>,
}

fn bind_account(
    manager: &AccountManager,
    account: u64,
    ports: &PortConfig,
) -> Result<BoundAccount, String> {
    let daemon = manager
        .daemon(account)
        .map_err(|_| "account daemon is not available".to_string())?;
    let token = daemon.auth_token();
    Ok(BoundAccount {
        account,
        daemon,
        token,
        http: endpoint(ports.http)?,
        websocket: endpoint(ports.websocket)?,
        sse: endpoint(ports.sse)?,
        mcp: endpoint(ports.mcp)?,
    })
}

fn print_value(value: &Value) {
    println!("{value}");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
}

fn print_summary(bound: &[BoundAccount], config: &ManagerConfig) {
    print_value(&endpoint_summary(bound, config));
}

fn endpoint_summary(bound: &[BoundAccount], config: &ManagerConfig) -> Value {
    let mut accounts = Vec::with_capacity(config.accounts.len());
    for spec in &config.accounts {
        let mut endpoints = Map::new();
        if let Some(item) = bound.iter().find(|item| item.account == spec.account) {
            for (name, value) in [
                ("http", item.http.as_ref()),
                ("websocket", item.websocket.as_ref()),
                ("sse", item.sse.as_ref()),
                ("mcp", item.mcp.as_ref()),
            ] {
                if let Some((_, address)) = value {
                    endpoints.insert(name.to_string(), Value::String(address.clone()));
                }
            }
        } else {
            for (name, port) in [
                ("http", spec.ports.http),
                ("websocket", spec.ports.websocket),
                ("sse", spec.ports.sse),
                ("mcp", spec.ports.mcp),
            ] {
                if let Some(port) = port {
                    endpoints.insert(name.to_string(), json!(format!("127.0.0.1:{port}")));
                }
            }
        }
        accounts.push(json!({
            "account": spec.account,
            "endpoints": endpoints,
        }));
    }
    json!({ "accounts": accounts })
}

fn spawn_listener_threads(bound: Vec<BoundAccount>) -> Vec<ListenerThread> {
    let mut threads = Vec::new();
    for account in bound {
        let BoundAccount {
            account: account_id,
            daemon,
            token,
            http,
            websocket,
            sse,
            mcp,
        } = account;
        if let Some((listener, _)) = http {
            let daemon = Arc::clone(&daemon);
            let token = token.clone();
            threads.push(ListenerThread {
                account: account_id,
                kind: "http",
                handle: thread::spawn(move || {
                    crate::onebot_server::serve_http(daemon, &listener, Some(&token));
                    Ok(())
                }),
            });
        }
        if let Some((listener, _)) = websocket {
            let daemon = Arc::clone(&daemon);
            let token = token.clone();
            threads.push(ListenerThread {
                account: account_id,
                kind: "websocket",
                handle: thread::spawn(move || {
                    crate::onebot_transport::serve_forward_ws(daemon, listener, Some(token));
                    Ok(())
                }),
            });
        }
        if let Some((listener, _)) = sse {
            let daemon = Arc::clone(&daemon);
            let token = token.clone();
            threads.push(ListenerThread {
                account: account_id,
                kind: "sse",
                handle: thread::spawn(move || {
                    crate::onebot_sse::serve_sse(daemon, listener, Some(token))
                        .map(|_| ())
                        .map_err(|_| "listener stopped with an error".to_string())
                }),
            });
        }
        if let Some((listener, _)) = mcp {
            let daemon = Arc::clone(&daemon);
            let token = token.clone();
            threads.push(ListenerThread {
                account: account_id,
                kind: "mcp",
                handle: thread::spawn(move || {
                    crate::onebot_mcp::serve_mcp(daemon, listener, Some(token))
                        .map(|_| ())
                        .map_err(|_| "listener stopped with an error".to_string())
                }),
            });
        }
    }
    threads
}

struct ListenerThread {
    account: u64,
    kind: &'static str,
    handle: JoinHandle<Result<(), String>>,
}

struct ListenerSet {
    account: u64,
    daemon: Arc<Daemon>,
    threads: Vec<ListenerThread>,
}

fn join_listeners(threads: Vec<ListenerThread>) -> bool {
    let mut ok = true;
    for thread in threads {
        match thread.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => {
                eprintln!(
                    "[caligo-manager] {} listener stopped for account {}",
                    thread.kind, thread.account
                );
                ok = false;
            }
        }
    }
    ok
}

fn join_set(set: ListenerSet) -> bool {
    join_listeners(set.threads)
}

fn cleanup_startup(manager: &Arc<Mutex<AccountManager>>, listeners: BTreeMap<u64, ListenerSet>) {
    let _ = manager.lock().expect("manager mutex poisoned").shutdown();
    for (_, set) in listeners {
        let _ = join_set(set);
    }
}

fn bind_account_listeners(
    manager: &AccountManager,
    account: u64,
) -> Result<ListenerSet, String> {
    let spec = manager
        .config()
        .accounts
        .iter()
        .find(|spec| spec.account == account)
        .ok_or_else(|| "account is not configured".to_string())?;
    let bound = bind_account(manager, account, &spec.ports)?;
    let daemon = Arc::clone(&bound.daemon);
    Ok(ListenerSet {
        account,
        daemon,
        threads: spawn_listener_threads(vec![bound]),
    })
}

fn install_ctrl_handler() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_handler), 1);
    }
}

pub fn guided_entry(args: &[String]) {
    let mut terminal = false;
    let mut root = None;
    let mut args = args.iter().map(String::as_str);
    while let Some(arg) = args.next() {
        match arg {
            "-h" | "--help" => {
                println!("用法：caligo.exe manage [--terminal] [--root <私有目录的绝对路径>]");
                println!("默认使用 Web UI。使用 --terminal 在终端选择账号和查看消息。");
                return;
            }
            "--terminal" => terminal = true,
            "--root" => {
                root = args.next().map(PathBuf::from);
                if root.is_none() {
                    eprintln!("--root 需要私有目录的绝对路径。");
                    std::process::exit(2);
                }
            }
            _ => {
                eprintln!("未知选项。请运行 caligo.exe manage --help。");
                std::process::exit(2);
            }
        }
    }
    let Some(root) = root.or_else(|| {
        std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("Caligo"))
    }) else {
        eprintln!("无法找到本机数据目录。请用 --root 指定私有目录。");
        std::process::exit(2);
    };
    let mut options = vec!["--select".into(), "--root".into(), root.to_string_lossy().into_owned()];
    if !terminal {
        options.extend(["--management-bind".into(), "127.0.0.1:0".into()]);
    }
    entry(&options);
}

fn select_account(manager: &mut AccountManager) -> io::Result<Option<u64>> {
    let mut input = io::stdin().lock();
    let mut line = String::new();
    loop {
        eprintln!("\nCaligo · 选择要托管的 QQ");
        if manager.config().accounts.is_empty() {
            eprintln!("还没有账号。输入 a 添加 QQ。");
        }
        for (index, spec) in manager.config().accounts.iter().enumerate() {
            eprintln!("  {}. QQ {}", index + 1, spec.account);
        }
        eprint!("输入列表序号启动，a 添加账号，q 退出 > ");
        io::stderr().flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 || line.trim().eq_ignore_ascii_case("q") {
            return Ok(None);
        }
        if line.trim().eq_ignore_ascii_case("a") {
            eprint!("输入 QQ 号，留空返回 > ");
            io::stderr().flush()?;
            line.clear();
            if input.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            match parse_account(line.trim()) {
                Some(account) if manager.config().accounts.iter().any(|spec| spec.account == account) => {
                    eprintln!("此 QQ 账号已添加。");
                }
                Some(account) if manager.register_account(account).is_ok() => {
                    eprintln!("QQ {account} 已添加。请选择要启动的账号。");
                }
                Some(_) => eprintln!("无法保存账号配置。"),
                None => eprintln!("请输入有效的 QQ 号。"),
            }
            continue;
        }
        let selected = line.trim().parse::<usize>().ok().and_then(|index| index.checked_sub(1));
        if let Some(spec) = selected.and_then(|index| manager.config().accounts.get(index)) {
            return Ok(Some(spec.account));
        }
        eprintln!("请选择列表中的序号。");
    }
}

struct TerminalLog {
    account: u64,
    verify: Receiver<()>,
    next_refresh: Instant,
    connection: Option<String>,
    last_message: i32,
    initialized: bool,
    read_error: bool,
}

impl TerminalLog {
    fn new(account: u64) -> Self {
        eprintln!("QQ {account} 的核心服务已启动。请等待已准入的 QQ 桥接连接。");
        eprintln!("输入 v 回车核验当前登录。按 Ctrl+C 停止服务。");
        eprintln!("该账号最近 100 条 Caligo 消息记录将显示在下方。");
        let (tx, verify) = mpsc::sync_channel(1);
        thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                if line.trim().eq_ignore_ascii_case("v") {
                    let _ = tx.try_send(());
                }
            }
        });
        Self {
            account,
            verify,
            next_refresh: Instant::now(),
            connection: None,
            last_message: 0,
            initialized: false,
            read_error: false,
        }
    }

    fn poll(&mut self, daemon: &Daemon) {
        if Instant::now() < self.next_refresh {
            return;
        }
        self.next_refresh = Instant::now() + Duration::from_millis(750);
        let connection = connection_id(daemon);
        if connection != self.connection {
            if connection.is_some() {
                eprintln!("QQ 桥接已连接，输入 v 回车核验登录。");
            } else {
                eprintln!("QQ 桥接已断开，之前的登录核验已失效。");
            }
            self.connection = connection;
        }
        if self.verify.try_recv().is_ok() {
            match verify_login(daemon, &CTRL_STOP) {
                Ok((_, nickname)) => eprintln!("QQ {} 登录已核验 · {}", self.account, nickname.escape_debug()),
                Err(error) => eprintln!("{error}"),
            }
        }
        let messages = match recent_messages(daemon) {
            Ok(messages) => messages,
            Err(error) => {
                if !self.read_error {
                    eprintln!("{error}");
                }
                self.read_error = true;
                return;
            }
        };
        self.read_error = false;
        if !self.initialized && messages.is_empty() {
            eprintln!("暂无消息记录。");
        }
        if self.initialized && messages.last().is_some_and(|row| row.message_id > self.last_message.saturating_add(1)) {
            eprintln!("日志更新较快，当前显示最近 100 条。");
        }
        let previous = self.last_message;
        for row in messages.iter().rev().filter(|row| row.message_id > previous) {
            let chat = if row.chat_type == "group" { "群聊" } else { "私聊" };
            let sender = row.nickname.as_ref().or(row.sender.as_ref()).map_or("未知", String::as_str);
            println!("#{} {chat} {} · {} · {}", row.message_id, row.peer.escape_debug(), sender.escape_debug(), row.preview.escape_debug());
            self.last_message = row.message_id;
        }
        self.initialized = true;
    }
}

pub fn entry(args: &[String]) {
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("[caligo-manager] {error}");
            std::process::exit(2);
        }
    };
    if let Some(input) = &args.set_config {
        let config = match read_config_file(input) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("[caligo-manager] {error}");
                std::process::exit(3);
            }
        };
        match config.save_atomic(&args.root) {
            Ok(readback) => {
                print_summary(&[], &readback);
                return;
            }
            Err(_) => {
                eprintln!("[caligo-manager] configuration save failed");
                std::process::exit(3);
            }
        }
    }
    let manager = if args.select && !args.check {
        AccountManager::load_for_selection(&args.root).map_err(|_| {
            eprintln!("[caligo-manager] 无法读取账号配置或打开私有目录。");
            3
        })
    } else {
        let config = match ManagerConfig::load(&args.root) {
            Ok(config) => config,
            Err(_) => {
                eprintln!("[caligo-manager] configuration load or validation failed");
                std::process::exit(3);
            }
        };
        if args.check {
            print_summary(&[], &config);
            return;
        }
        AccountManager::new(&args.root, config).map_err(|_| {
            eprintln!("[caligo-manager] manager initialization failed");
            4
        })
    };
    let mut manager = match manager {
        Ok(manager) => manager,
        Err(code) => std::process::exit(code),
    };
    let terminal_account = if args.select && args.management_bind.is_none() {
        match select_account(&mut manager) {
            Ok(Some(account)) => Some(account),
            Ok(None) => return,
            Err(error) => {
                eprintln!("[caligo-manager] 无法读取终端输入：{error}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };
    CTRL_STOP.store(false, Ordering::Release);
    install_ctrl_handler();
    let started = match terminal_account {
        Some(account) => manager.start(account).map(|_| ()),
        None if !args.select => manager.start_all(),
        None => Ok(()),
    };
    if started.is_err() {
        eprintln!("[caligo-manager] account startup failed");
        let _ = manager.shutdown();
        std::process::exit(5);
    }
    let config = manager.config().clone();
    let manager = Arc::new(Mutex::new(manager));

    let mut listeners = BTreeMap::new();
    let initial_metadata;
    {
        let manager_guard = manager.lock().expect("manager mutex poisoned");
        let mut bound = Vec::new();
        let running = manager_guard.running_accounts();
        for spec in config.accounts.iter().filter(|spec| running.contains(&spec.account)) {
            match bind_account(&manager_guard, spec.account, &spec.ports) {
                Ok(account) => bound.push(account),
                Err(_) => {
                    eprintln!("[caligo-manager] listener startup failed");
                    drop(bound);
                    drop(manager_guard);
                    cleanup_startup(&manager, listeners);
                    std::process::exit(6);
                }
            }
        }
        initial_metadata = if args.select {
            let mut active = config.clone();
            active.accounts.retain(|spec| running.contains(&spec.account));
            endpoint_summary(&bound, &active)
        } else {
            endpoint_summary(&bound, &config)
        };
        for account in bound {
            let account_id = account.account;
            let daemon = Arc::clone(&account.daemon);
            listeners.insert(
                account_id,
                ListenerSet {
                    account: account_id,
                    daemon,
                    threads: spawn_listener_threads(vec![account]),
                },
            );
        }
    }

    let mut management = match args.management_bind {
        Some(bind) => {
            let token_result = {
                let manager_guard = manager.lock().expect("manager mutex poisoned");
                crate::account_manager::management_token(manager_guard.root())
            };
            let token = match token_result {
                Ok(token) => token,
                Err(_) => {
                    eprintln!("[caligo-manager] management credential unavailable");
                    cleanup_startup(&manager, listeners);
                    std::process::exit(6);
                }
            };
            match ManagementHttpServer::start(
                ManagementHttpConfig {
                    bind,
                    bearer_token: token,
                },
                Arc::clone(&manager),
            ) {
                Ok(server) => Some(server),
                Err(_) => {
                    eprintln!("[caligo-manager] management listener startup failed");
                    cleanup_startup(&manager, listeners);
                    std::process::exit(6);
                }
            }
        }
        None => None,
    };

    let mut metadata = initial_metadata;
    if let Some(server) = &management {
        metadata["management_endpoint"] = json!(format!("http://{}", server.local_addr()));
        if args.select {
            eprintln!("在浏览器打开 http://{}/", server.local_addr());
            eprintln!("用记事本打开 {}，将文件内容粘贴到网页的「管理凭据」。", args.root.join("management.token").display());
            eprintln!("按 Ctrl+C 可关闭管理器。");
        }
    }
    // Endpoint metadata never contains account or management credentials.
    print_value(&metadata);
    let mut terminal = terminal_account.map(TerminalLog::new);

    let mut listener_failure = false;
    loop {
        let management_stop = management
            .as_ref()
            .is_some_and(|server| server.stop_flag().load(Ordering::Acquire));
        if CTRL_STOP.load(Ordering::Acquire) || management_stop {
            break;
        }
        if let Some(terminal) = &mut terminal {
            if let Some(set) = listeners.get(&terminal.account) {
                terminal.poll(&set.daemon);
            }
        }

        let running: Vec<u64> = {
            let manager_guard = manager.lock().expect("manager mutex poisoned");
            manager_guard.running_accounts()
        };
        let configured: Vec<u64> = {
            let manager_guard = manager.lock().expect("manager mutex poisoned");
            manager_guard.config().accounts.iter().map(|spec| spec.account).collect()
        };

        // A stop/start through the API can replace the daemon between polls. Join
        // the old generation before binding the replacement ports.
        let replaced: Vec<u64> = listeners
            .iter()
            .filter_map(|(account, set)| {
                let current = manager
                    .lock()
                    .expect("manager mutex poisoned")
                    .daemon(*account)
                    .ok();
                match current {
                    Some(current) if !Arc::ptr_eq(&current, &set.daemon) => Some(*account),
                    _ => None,
                }
            })
            .collect();
        for account in replaced {
            if let Some(set) = listeners.remove(&account) {
                let _ = join_set(set);
            }
        }

        // API-started accounts are attached to their configured protocol ports here.
        for account in configured.iter().copied().filter(|account| running.contains(account)) {
            if !listeners.contains_key(&account) {
                let result = {
                    let manager_guard = manager.lock().expect("manager mutex poisoned");
                    bind_account_listeners(&manager_guard, account)
                };
                match result {
                    Ok(set) => {
                        listeners.insert(account, set);
                    }
                    Err(_) => {
                        eprintln!("[caligo-manager] listener startup failed for account {account}");
                        let _ = manager.lock().expect("manager mutex poisoned").stop(account);
                    }
                }
            }
        }

        // Stop/join accounts that were stopped through the API or their control pipe.
        let stale: Vec<u64> = listeners
            .keys()
            .copied()
            .filter(|account| !running.contains(account))
            .collect();
        for account in stale {
            if let Some(set) = listeners.remove(&account) {
                let _ = manager.lock().expect("manager mutex poisoned").stop(account);
                let _ = join_set(set);
            }
        }

        // A failed protocol worker is isolated to its account. The daemon stop
        // flag then releases the other protocol workers for the same account.
        let failed: Vec<u64> = listeners
            .iter()
            .filter(|(_, set)| set.threads.iter().any(|thread| thread.handle.is_finished()))
            .map(|(account, _)| *account)
            .collect();
        for account in failed {
            eprintln!("[caligo-manager] listener failure; stopping account {account}");
            listener_failure = true;
            if let Some(set) = listeners.remove(&account) {
                let _ = manager.lock().expect("manager mutex poisoned").stop(account);
                let _ = join_set(set);
            }
        }

        let empty = manager
            .lock()
            .expect("manager mutex poisoned")
            .running_accounts()
            .is_empty();
        if empty && management.is_none() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    let _ = manager.lock().expect("manager mutex poisoned").shutdown();
    for (_, set) in listeners {
        let _ = join_set(set);
    }
    if let Some(mut server) = management.take() {
        let _ = server.shutdown();
    }
    if listener_failure {
        // A runtime listener failure is isolated and does not fail the manager.
    }
}
