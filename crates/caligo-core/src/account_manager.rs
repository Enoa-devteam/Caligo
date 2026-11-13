//! Account-scoped daemon ownership and private, durable configuration.
//!
//! This starts core threads only. QQ attachment/login and transport listeners
//! remain explicit callers' responsibilities; a configured account is not online.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::daemon::{Daemon, DaemonConfig, SessionError};
use crate::onebot_server::{self, ActionHandler};

#[derive(Debug)]
pub struct ManagerError(String);
impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ManagerError {}
impl From<std::io::Error> for ManagerError {
    fn from(error: std::io::Error) -> Self {
        Self(format!("filesystem operation failed: {error}"))
    }
}
type Result<T> = std::result::Result<T, ManagerError>;
fn invalid(message: &str) -> ManagerError {
    ManagerError(message.into())
}

/// Private management credential, separate from every account's pipe token.
/// Callers must deliver it through a private channel, never stdout/API output.
pub fn management_token(root: impl AsRef<Path>) -> Result<String> {
    let root = prepare_root(root.as_ref())?;
    let path = root.join("management.token");
    if path.try_exists()? {
        check_private_file(&path)?;
        return parse_token(&read_bounded(&path, 64)?);
    }
    let random = crate::transport::generate_token().map_err(|_| invalid("OS RNG failed"))?;
    let token = crate::transport::token_hex(&random);
    atomic_write(&path, token.as_bytes())?;
    check_private_file(&path)?;
    let readback = parse_token(&read_bounded(&path, 64)?)?;
    if readback != token {
        return Err(invalid("management token readback mismatch"));
    }
    Ok(readback)
}

/// Port zero is rejected: persisted endpoints must be explicit and conflict-free.
/// These are validated assembly specifications, not automatically bound sockets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortConfig {
    pub http: Option<u16>,
    pub websocket: Option<u16>,
    pub sse: Option<u16>,
    pub mcp: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountSpec {
    pub account: u64,
    /// One portable directory component beneath the private manager root.
    pub data_dir: String,
    /// Short ASCII name; the daemon adds the Windows pipe root and role suffix.
    pub pipe_prefix: String,
    pub module_baseline: String,
    pub expect_client_pid: u32,
    pub ports: PortConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerConfig {
    pub version: u32,
    pub accounts: Vec<AccountSpec>,
}

impl ManagerConfig {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.accounts.is_empty() || self.accounts.len() > 128 {
            return Err(invalid("expected version 1 and 1..=128 accounts"));
        }
        let mut accounts = BTreeSet::new();
        let mut directories = BTreeSet::new();
        let mut pipes = BTreeSet::new();
        let mut ports = BTreeSet::new();
        for spec in &self.accounts {
            if spec.account == 0 || !accounts.insert(spec.account) {
                return Err(invalid("account must be positive and unique"));
            }
            if !portable_component(&spec.data_dir)
                || !directories.insert(spec.data_dir.to_ascii_lowercase())
            {
                return Err(invalid(
                    "account directories must be isolated portable components",
                ));
            }
            if spec.pipe_prefix.is_empty()
                || spec.pipe_prefix.len() > 160
                || !spec
                    .pipe_prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !pipes.insert(spec.pipe_prefix.to_ascii_lowercase())
            {
                return Err(invalid("pipe prefixes must be short, unique ASCII names"));
            }
            if spec.module_baseline.trim().is_empty()
                || spec.module_baseline.len() > 256
                || spec.module_baseline.chars().any(char::is_control)
            {
                return Err(invalid("module_baseline must be a nonempty bounded label"));
            }
            for port in [
                spec.ports.http,
                spec.ports.websocket,
                spec.ports.sse,
                spec.ports.mcp,
            ]
            .into_iter()
            .flatten()
            {
                if port == 0 || !ports.insert(port) {
                    return Err(invalid(
                        "listener ports must be nonzero and globally unique",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Fixed private path avoids arbitrary file destinations from configuration.
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = prepare_root(root.as_ref())?;
        let path = root.join("accounts.json");
        check_private_file(&path)?;
        let bytes = read_bounded(&path, 256 * 1024)?;
        // Do not include serde's value-bearing error text in management output.
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid account configuration JSON"))?;
        config.validate()?;
        Ok(config)
    }

    /// Atomic replace on the same volume, flush, then parse and compare readback.
    pub fn save_atomic(&self, root: impl AsRef<Path>) -> Result<Self> {
        self.validate()?;
        let root = prepare_root(root.as_ref())?;
        for spec in &self.accounts {
            validate_account_paths(&root, spec)?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|_| invalid("configuration serialization failed"))?;
        atomic_write(&root.join("accounts.json"), &bytes)?;
        let readback = Self::load(&root)?;
        if readback != *self {
            return Err(invalid("configuration readback mismatch"));
        }
        Ok(readback)
    }
}

struct RunningAccount {
    daemon: Arc<Daemon>,
    runner: JoinHandle<std::result::Result<(), SessionError>>,
    // Held until all daemon work joins. Also prevents another manager opening
    // the same journal/token directory, even under a different pipe prefix.
    _directory_lock: File,
}

/// Synchronous ownership API. Mutating lifecycle operations require &mut self,
/// while callers can clone a daemon/handler for protocol worker threads.
pub struct AccountManager {
    root: PathBuf,
    config: ManagerConfig,
    running: BTreeMap<u64, RunningAccount>,
}

impl AccountManager {
    pub fn new(root: impl AsRef<Path>, config: ManagerConfig) -> Result<Self> {
        config.validate()?;
        let root = prepare_root(root.as_ref())?;
        for spec in &config.accounts {
            validate_account_paths(&root, spec)?;
        }
        Ok(Self {
            root,
            config,
            running: BTreeMap::new(),
        })
    }

    pub fn config(&self) -> &ManagerConfig {
        &self.config
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn running_accounts(&self) -> Vec<u64> {
        self.running
            .iter()
            .filter(|(_, r)| {
                !r.runner.is_finished() && !r.daemon.stop_flag().load(Ordering::Acquire)
            })
            .map(|(account, _)| *account)
            .collect()
    }
    pub fn daemon(&self, account: u64) -> Result<Arc<Daemon>> {
        let running = self
            .running
            .get(&account)
            .ok_or_else(|| invalid("account is not started"))?;
        if running.runner.is_finished() || running.daemon.stop_flag().load(Ordering::Acquire) {
            return Err(invalid(
                "account daemon has stopped; stop/join it before restart",
            ));
        }
        Ok(Arc::clone(&running.daemon))
    }
    pub fn action_handler(&self, account: u64) -> Result<ActionHandler> {
        let daemon = self.daemon(account)?;
        let handler = onebot_server::action_handler(Arc::clone(&daemon));
        Ok(Arc::new(move |action, params, echo| {
            if daemon.stop_flag().load(Ordering::Acquire) {
                return crate::onebot::ActionResponse::failed(1400, "account stopped", echo);
            }
            handler(action, params, echo)
        }))
    }

    pub fn start(&mut self, account: u64) -> Result<Arc<Daemon>> {
        if self.running.contains_key(&account) {
            return Err(invalid("account already started; stop/join before restart"));
        }
        let spec = self
            .config
            .accounts
            .iter()
            .find(|s| s.account == account)
            .ok_or_else(|| invalid("unknown account"))?
            .clone();
        let dir = validate_account_paths(&self.root, &spec)?;
        let lock = lock_directory(&dir)?;
        // Validate/read existing credentials before opening the journal.
        let token_path = dir.join("auth.token");
        let token = if token_path.try_exists()? {
            check_private_file(&token_path)?;
            Some(parse_token(&read_bounded(&token_path, 64)?)?)
        } else {
            let random =
                crate::transport::generate_token().map_err(|_| invalid("OS RNG failed"))?;
            let token = crate::transport::token_hex(&random);
            atomic_write(&token_path, token.as_bytes())?;
            let readback = parse_token(&read_bounded(&token_path, 64)?)?;
            if readback != token {
                return Err(invalid("token readback mismatch"));
            }
            Some(readback)
        };
        let daemon = Daemon::start(DaemonConfig {
            pipe_prefix: spec.pipe_prefix,
            journal_path: dir.join("journal.jsonl"),
            module_baseline: spec.module_baseline,
            account: account.to_string(),
            core_build: crate::CORE_BUILD.into(),
            expect_client_pid: spec.expect_client_pid,
            runtime: Default::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        })
        .map_err(|_| invalid("account journal/message store open failed"))?;
        if let Some(token) = token {
            daemon.set_auth_token(&token);
        }
        let run_daemon = Arc::clone(&daemon);
        let runner = match std::thread::Builder::new()
            .name(format!("caligo-account-{account}"))
            .spawn(move || {
                let result = run_daemon.run();
                run_daemon.stop_flag().store(true, Ordering::Release);
                result
            }) {
            Ok(runner) => runner,
            Err(error) => {
                daemon.stop_flag().store(true, Ordering::Release);
                if let Ok(mut actor) = daemon.actor.lock() {
                    let _ = actor.stop();
                    let _ = actor.close();
                }
                return Err(error.into());
            }
        };
        let running = RunningAccount {
            daemon: Arc::clone(&daemon),
            runner,
            _directory_lock: lock,
        };
        self.running.insert(account, running);
        if let Err(error) = self.wait_ready(account) {
            let cleanup = self.stop(account);
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => ManagerError(format!("{error}; cleanup: {cleanup}")),
            });
        }
        Ok(daemon)
    }

    fn wait_ready(&self, account: u64) -> Result<()> {
        let running = &self.running[&account];
        let (bridge, control) = running.daemon.pipe_names();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if running.runner.is_finished() {
                return Err(invalid("account daemon exited during startup"));
            }
            if pipe_available(&bridge) && pipe_available(&control) {
                return probe_control(&running.daemon, &control);
            }
            if Instant::now() >= deadline {
                return Err(invalid("account pipe startup timed out"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Existing running accounts survive a failed batch; only this batch rolls back.
    pub fn start_all(&mut self) -> Result<()> {
        let accounts: Vec<_> = self.config.accounts.iter().map(|s| s.account).collect();
        let mut started = Vec::new();
        for account in accounts {
            if self.running.contains_key(&account) {
                continue;
            }
            if let Err(error) = self.start(account) {
                let mut cleanup_errors = Vec::new();
                for account in started.into_iter().rev() {
                    if let Err(error) = self.stop(account) {
                        cleanup_errors.push(error.to_string());
                    }
                }
                return Err(if cleanup_errors.is_empty() {
                    error
                } else {
                    ManagerError(format!(
                        "{error}; batch rollback: {}",
                        cleanup_errors.join("; ")
                    ))
                });
            }
            started.push(account);
        }
        Ok(())
    }

    pub fn stop(&mut self, account: u64) -> Result<()> {
        if !self.config.accounts.iter().any(|s| s.account == account) {
            return Err(invalid("unknown account"));
        }
        let Some(running) = self.running.remove(&account) else {
            return Ok(());
        };
        running.daemon.stop_flag().store(true, Ordering::Release);
        // run() cancels its accept/read loops and joins both pipe owners. If it
        // failed before spawning them, explicitly close the opened actor too.
        let joined = running.runner.join();
        if let Ok(mut actor) = running.daemon.actor.lock() {
            let _ = actor.stop();
            let _ = actor.close();
        }
        match joined {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(invalid("account daemon failed (all owned threads joined)")),
            Err(_) => Err(invalid("account daemon thread panicked")),
        }
    }
    pub fn shutdown(&mut self) -> Result<()> {
        let accounts: Vec<_> = self.running.keys().copied().collect();
        let mut errors = Vec::new();
        for account in accounts {
            if let Err(error) = self.stop(account) {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ManagerError(errors.join("; ")))
        }
    }
}
impl Drop for AccountManager {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("[account-manager] shutdown: {error}");
        }
    }
}

fn portable_component(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    !matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}
fn prepare_root(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute()
        || !matches!(root.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
        || root.as_os_str().to_string_lossy().contains('\0')
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid(
            "manager root must be an absolute normalized local disk path",
        ));
    }
    let parent = root
        .parent()
        .ok_or_else(|| invalid("manager root has no parent"))?;
    // The evidence parent is provisioned by the caller; do not mutate its ACL.
    reject_reparse_chain(parent)?;
    ensure_private_dir(root)?;
    reject_reparse_chain(root)?;
    Ok(root.to_path_buf())
}
fn validate_account_paths(root: &Path, spec: &AccountSpec) -> Result<PathBuf> {
    reject_reparse_chain(root)?;
    check_private(root)?;
    let dir = root.join(&spec.data_dir);
    ensure_private_dir(&dir)?;
    for name in [
        "journal.jsonl",
        "journal.messages.jsonl",
        "auth.token",
        "manager.lock",
    ] {
        let path = dir.join(name);
        if path.try_exists()? {
            check_private_file(&path)?;
        }
    }
    Ok(dir)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("private file exceeds size limit"));
    }
    Ok(bytes)
}
fn parse_token(bytes: &[u8]) -> Result<String> {
    if bytes.len() != 64 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid(
            "credential file must contain exactly 64 hexadecimal bytes",
        ));
    }
    Ok(String::from_utf8(bytes.to_vec()).map_err(|_| invalid("invalid credential encoding"))?)
}
fn check_private_file(path: &Path) -> Result<()> {
    reject_reparse_chain(path)?;
    if !fs::metadata(path)?.is_file() {
        return Err(invalid("private path must be a regular file"));
    }
    check_private(path)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.try_exists()? {
        check_private_file(path)?;
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("private file has no parent"))?;
    reject_reparse_chain(parent)?;
    check_private(parent)?;
    let nonce = crate::transport::generate_token().map_err(|_| invalid("OS RNG failed"))?;
    let temp = parent.join(format!(".pending-{}", crate::transport::token_hex(&nonce)));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        check_private_file(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        atomic_replace(&temp, path)?;
        check_private_file(path)?;
        if read_bounded(path, bytes.len() as u64)? != bytes {
            return Err(invalid("private file readback mismatch"));
        }
        Ok(())
    })();
    if temp.exists() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
#[cfg(windows)]
fn reject_reparse_chain(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid(
                "reparse points are forbidden in private data paths",
            ));
        }
    }
    Ok(())
}
#[cfg(windows)]
fn lock_directory(dir: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    let path = dir.join("manager.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&path)?;
    // Querying a locked file by path needs only READ_CONTROL, not file data.
    check_private_file(&path)?;
    Ok(file)
}
#[cfg(windows)]
fn pipe_available(name: &str) -> bool {
    // No connection is consumed: the real control/bridge clients own the first
    // authenticated Hello. Both pipes must have been created by Daemon::run.
    unsafe {
        windows_sys::Win32::System::Pipes::WaitNamedPipeW(wide(Path::new(name)).as_ptr(), 1) != 0
    }
}
#[cfg(windows)]
fn probe_control(daemon: &Arc<Daemon>, name: &str) -> Result<()> {
    use crate::ipc::{
        encode_frame, ControlMsg, CoreToControlMsg, FrameDecoder, Role, PROTOCOL_VERSION_V3,
    };
    use crate::transport::connect_client;
    let connection =
        connect_client(name).map_err(|_| invalid("control readiness connection failed"))?;
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let timer_cancel = Arc::clone(&cancel);
    let (finish, waiting) = std::sync::mpsc::channel();
    let timer = std::thread::Builder::new()
        .name("caligo-startup-timeout".into())
        .spawn(move || {
            if waiting.recv_timeout(Duration::from_secs(5)).is_err() {
                timer_cancel.store(true, Ordering::Release);
            }
        })?;
    let result = (|| {
        let message = ControlMsg::Hello {
            protocol_version: PROTOCOL_VERSION_V3,
            client_build: crate::CORE_BUILD.into(),
            auth_token: daemon.auth_token(),
            role: Role::Control,
        };
        let json =
            serde_json::to_vec(&message).map_err(|_| invalid("readiness serialization failed"))?;
        let frame = encode_frame(&json).map_err(|_| invalid("readiness frame failed"))?;
        connection
            .write_all(&frame)
            .map_err(|_| invalid("readiness write failed"))?;
        let mut decoder = FrameDecoder::new();
        loop {
            let mut bytes = [0u8; 4096];
            let count = connection
                .read_some(&mut bytes, Some(&cancel))
                .map_err(|_| invalid("readiness response failed or timed out"))?;
            if count == 0 {
                return Err(invalid("readiness pipe closed"));
            }
            let mut frames = Vec::new();
            decoder
                .push(&bytes[..count], &mut frames)
                .map_err(|_| invalid("invalid readiness frame"))?;
            if let Some(frame) = frames.first() {
                return match serde_json::from_slice::<CoreToControlMsg>(frame) {
                    Ok(CoreToControlMsg::HelloAck { accepted: true, .. }) => Ok(()),
                    _ => Err(invalid("readiness authentication rejected")),
                };
            }
        }
    })();
    // Break the pipe before telling the timer to join. Daemon's existing drain
    // window then releases this control slot for normal callers.
    drop(connection);
    let _ = finish.send(());
    let _ = timer.join();
    result?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !pipe_available(name) {
        if daemon.stop_flag().load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(invalid(
                "control pipe did not become available after readiness probe",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    if unsafe {
        MoveFileExW(
            wide(source).as_ptr(),
            wide(destination).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Build a protected DACL granting only the current user full control, with
/// inheritance for account files. Use existing windows-sys features only.
#[cfg(windows)]
fn with_user_sid<T>(
    operation: impl FnOnce(windows_sys::Win32::Security::PSID) -> Result<T>,
) -> Result<T> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TokenUser, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut length = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
        let mut data = vec![0u32; (length as usize + 3) / 4];
        let success = GetTokenInformation(
            token,
            TokenUser,
            data.as_mut_ptr().cast(),
            length,
            &mut length,
        );
        CloseHandle(token);
        if success == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let sid = *(data.as_ptr() as *const windows_sys::Win32::Security::PSID);
        operation(sid)
    }
}
#[cfg(windows)]
fn ensure_private_dir(path: &Path) -> Result<()> {
    if path.try_exists()? {
        reject_reparse_chain(path)?;
        if !fs::metadata(path)?.is_dir() {
            return Err(invalid("private directory path is not a directory"));
        }
        return check_private(path);
    }
    use windows_sys::Win32::Security::*;
    with_user_sid(|sid| unsafe {
        let mut acl_data = vec![0u32; 512];
        let acl = acl_data.as_mut_ptr().cast::<ACL>();
        let mut descriptor: SECURITY_DESCRIPTOR = std::mem::zeroed();
        if InitializeAcl(acl, 2048, ACL_REVISION) == 0
            || AddAccessAllowedAceEx(acl, ACL_REVISION, 3, 0x001f01ff, sid) == 0
            || InitializeSecurityDescriptor((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1)
                == 0
            || SetSecurityDescriptorDacl(
                (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                1,
                acl,
                0,
            ) == 0
            || SetSecurityDescriptorOwner(
                (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                sid,
                0,
            ) == 0
            || SetSecurityDescriptorControl(
                (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                SE_DACL_PROTECTED,
                SE_DACL_PROTECTED,
            ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
            bInheritHandle: 0,
        };
        if windows_sys::Win32::Storage::FileSystem::CreateDirectoryW(wide(path).as_ptr(), &security)
            == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        check_private(path)
    })
}
#[cfg(windows)]
fn check_private(path: &Path) -> Result<()> {
    use windows_sys::Win32::Security::*;
    with_user_sid(|sid| unsafe {
        let mut length = 0;
        GetFileSecurityW(
            wide(path).as_ptr(),
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut length,
        );
        if length == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut data = vec![0u32; (length as usize + 3) / 4];
        let descriptor = data.as_mut_ptr().cast();
        if GetFileSecurityW(
            wide(path).as_ptr(),
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            descriptor,
            length,
            &mut length,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut owner = std::ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut acl = std::ptr::null_mut();
        if GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) == 0
            || owner.is_null()
            || EqualSid(owner, sid) == 0
            || GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) == 0
            || present == 0
            || acl.is_null()
            || (*acl).AceCount == 0
        {
            return Err(invalid(
                "private path requires current-user owner and a nonempty user-only DACL",
            ));
        }
        for index in 0..(*acl).AceCount {
            let mut ace = std::ptr::null_mut();
            if GetAce(acl, index as u32, &mut ace) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let header = &*(ace as *const ACE_HEADER);
            // Fail closed on object, callback, deny, or unfamiliar ACE types.
            if header.AceType != 0
                || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
            {
                return Err(invalid("private path has an unsupported access entry"));
            }
            let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
            let ace_sid = (&allowed.SidStart as *const u32).cast_mut().cast();
            if EqualSid(ace_sid, sid) == 0 {
                return Err(invalid("private path grants another principal access"));
            }
        }
        Ok(())
    })
}
