//! Private multi-account core manager.
//!
//! This binary owns only core daemons and loopback OneBot listeners. It never
//! attaches QQ and never writes authentication material to stdout.

use caligo_core::account_manager::{AccountManager, ManagerConfig, PortConfig};
use caligo_core::daemon::Daemon;
use caligo_core::management_http::{ManagementHttpConfig, ManagementHttpServer};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut root = None;
    let mut check = false;
    let mut set_config = None;
    let mut management_bind = None;
    let mut index = 0;
    while index < raw.len() {
        match raw[index].as_str() {
            "--root" => {
                index += 1;
                root = raw.get(index).map(PathBuf::from);
            }
            "--check" => check = true,
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
    let listener = caligo_core::onebot_server::bind_loopback(&format!("127.0.0.1:{port}"))
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
                    caligo_core::onebot_server::serve_http(daemon, &listener, Some(&token));
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
                    caligo_core::onebot_transport::serve_forward_ws(daemon, listener, Some(token));
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
                    caligo_core::onebot_sse::serve_sse(daemon, listener, Some(token))
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
                    caligo_core::onebot_mcp::serve_mcp(daemon, listener, Some(token))
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

fn main() {
    install_ctrl_handler();
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("[caligo-manager] {error}");
            std::process::exit(2);
        }
    };
    let config = if let Some(input) = &args.set_config {
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
    } else {
        match ManagerConfig::load(&args.root) {
            Ok(config) => config,
            Err(_) => {
                eprintln!("[caligo-manager] configuration load or validation failed");
                std::process::exit(3);
            }
        }
    };
    if args.check {
        print_summary(&[], &config);
        return;
    }

    let manager = match AccountManager::new(&args.root, config.clone()) {
        Ok(manager) => Arc::new(Mutex::new(manager)),
        Err(_) => {
            eprintln!("[caligo-manager] manager initialization failed");
            std::process::exit(4);
        }
    };
    if manager.lock().expect("manager mutex poisoned").start_all().is_err() {
        eprintln!("[caligo-manager] account startup failed");
        let _ = manager.lock().expect("manager mutex poisoned").shutdown();
        std::process::exit(5);
    }

    let mut listeners = BTreeMap::new();
    let initial_metadata;
    {
        let manager_guard = manager.lock().expect("manager mutex poisoned");
        let mut bound = Vec::new();
        for spec in &config.accounts {
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
        initial_metadata = endpoint_summary(&bound, &config);
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
                caligo_core::account_manager::management_token(manager_guard.root())
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
    }
    // Endpoint metadata never contains account or management credentials.
    print_value(&metadata);
    CTRL_STOP.store(false, Ordering::Release);

    let mut listener_failure = false;
    loop {
        let management_stop = management
            .as_ref()
            .is_some_and(|server| server.stop_flag().load(Ordering::Acquire));
        if CTRL_STOP.load(Ordering::Acquire) || management_stop {
            break;
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
