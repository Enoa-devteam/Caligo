//! K4-D7 候选 B 首入的 CLI 入口(research 运行时门控;普通构建拒绝)。
//!
//! 新路线专用命令:加载 bridge(仅 LoadLibrary+probe)→ 远程调用
//! `caligo_qq_entry_bootstrap` / `caligo_qq_entry_shutdown`。
//! 不使用旧 async/exec 载荷;门 1(manifest)+ 门 2(实例确认)与 inject 同源。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::{verify_manifest, winutil};

#[derive(Debug, Default, Clone)]
struct QqEntryArgs {
    pid: Option<u32>,
    bridge: Option<PathBuf>,
    manifest: Option<PathBuf>,
    report: Option<PathBuf>,
    env: Option<usize>,
    wait_s: u32,
    wait_ms: u32,
    confirmed: bool,
    /// D7b:daemon 客户端接线(可选;bootstrap 成功后启动)。
    daemon_pipe: Option<String>,
    daemon_auth: Option<String>,
    daemon_generation: Option<u64>,
    daemon_account: Option<String>,
    /// One-shot read-only QQNT identity-map round-trip; paired with uid_map_result.
    uid_map_uin: Option<u64>,
    /// Inspect only whether the active session exposes GroupService.
    uid_map_surface_only: bool,
    /// Use the hash-bound ProfileService round-trip contract.
    uid_map_profile_service: bool,
    uid_map_result: Option<PathBuf>,
    uid_map_timeout_ms: u32,
}

fn parse_args(args: &[String]) -> Result<QqEntryArgs, String> {
    let mut a = QqEntryArgs {
        wait_s: 600,
        wait_ms: 10_000,
        uid_map_timeout_ms: 60_000,
        ..Default::default()
    };
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--pid" => a.pid = next(&mut i).and_then(|s| s.parse().ok()),
            "--bridge" => a.bridge = next(&mut i).map(PathBuf::from),
            "--manifest" => a.manifest = next(&mut i).map(PathBuf::from),
            "--report" => a.report = next(&mut i).map(PathBuf::from),
            "--env" => {
                a.env = next(&mut i).and_then(|s| {
                    usize::from_str_radix(s.strip_prefix("0x").unwrap_or(&s), 16).ok()
                });
            }
            "--wait-s" => a.wait_s = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(600),
            "--wait-ms" => a.wait_ms = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(10_000),
            "--confirm-designated-test-instance" => a.confirmed = true,
            "--daemon-pipe" => a.daemon_pipe = next(&mut i),
            "--daemon-auth" => a.daemon_auth = next(&mut i),
            "--daemon-generation" => {
                a.daemon_generation = next(&mut i).and_then(|s| s.parse().ok());
            }
            "--daemon-account" => a.daemon_account = next(&mut i),
            "--uid-map-uin" => {
                let Some(value) = next(&mut i) else {
                    return Err("--uid-map-uin requires a decimal UIN".into());
                };
                a.uid_map_uin = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| "--uid-map-uin must be a decimal u64".to_string())?,
                );
            }
            "--uid-map-result" => a.uid_map_result = next(&mut i).map(PathBuf::from),
            "--uid-map-surface-only" => a.uid_map_surface_only = true,
            "--uid-map-profile-service" => a.uid_map_profile_service = true,
            "--uid-map-timeout-ms" => {
                a.uid_map_timeout_ms = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(60_000)
            }
            other => return Err(format!("未知参数: {other}")),
        }
        i += 1;
    }
    Ok(a)
}

/// 首入导出结果码 → 判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QqEntryExit {
    Ok,
    NullCtx,
    BadPath,
    Resolve,
    EnvNotFresh,
    LoopChain,
    NoCurrent,
    HandleSize,
    UvInit,
    Timeout,
    Already,
    ShutdownNotReady,
    ShutdownTimeout,
    Unknown,
}

fn map_code(code: u32) -> QqEntryExit {
    use caligo_bridge::qq_entry_code as c;
    match code {
        c::OK => QqEntryExit::Ok,
        c::ERR_NULL_CTX => QqEntryExit::NullCtx,
        c::ERR_BAD_PATH => QqEntryExit::BadPath,
        c::ERR_RESOLVE => QqEntryExit::Resolve,
        c::ERR_ENV_NOT_FRESH => QqEntryExit::EnvNotFresh,
        c::ERR_LOOP_CHAIN => QqEntryExit::LoopChain,
        c::ERR_NO_CURRENT => QqEntryExit::NoCurrent,
        c::ERR_HANDLE_SIZE => QqEntryExit::HandleSize,
        c::ERR_UV_INIT => QqEntryExit::UvInit,
        c::ERR_TIMEOUT => QqEntryExit::Timeout,
        c::ERR_ALREADY => QqEntryExit::Already,
        c::ERR_SHUTDOWN_NOT_READY => QqEntryExit::ShutdownNotReady,
        c::ERR_SHUTDOWN_TIMEOUT => QqEntryExit::ShutdownTimeout,
        _ => QqEntryExit::Unknown,
    }
}

fn usage() {
    eprintln!(
        "用法:qq-entry --pid <n> --bridge <dll> --manifest <json> --report <jsonl> --env <hex> [--wait-s n] --confirm-designated-test-instance\n      qq-entry [同上] --uid-map-uin <decimal> --uid-map-result <local-evidence.json> [--uid-map-profile-service] [--uid-map-timeout-ms n]\n      qq-entry [同上] --uid-map-surface-only --uid-map-result <local-evidence.json> [--uid-map-timeout-ms n]\n      qq-entry-stop --pid <n> --bridge <dll> --manifest <json> --report <jsonl> [--wait-ms n] --confirm-designated-test-instance"
    );
}

#[cfg(feature = "research")]
#[derive(Debug)]
enum UidMapRequest {
    RoundTrip { uin: u64, output: PathBuf },
    SurfaceOnly { output: PathBuf },
}

/// 公共前置:研究门控 + 必填参数 + 门 1(manifest)+ 门 2(实例确认)。
#[allow(clippy::type_complexity)]
fn gates(
    args: &[String],
    need_env: bool,
) -> Result<(u32, PathBuf, PathBuf, usize, u32, u32, QqEntryArgs), ExitCode> {
    if !caligo_bridge::gate::research_enabled() {
        eprintln!("{}", caligo_bridge::gate::DISABLED_NOTICE);
        return Err(ExitCode::from(3));
    }
    let a = match parse_args(args) {
        Ok(v) => v,
        Err(_) => {
            usage();
            return Err(ExitCode::FAILURE);
        }
    };
    if !need_env && (a.uid_map_surface_only || a.uid_map_profile_service) {
        eprintln!("[uid-map] surface-only and ProfileService modes require qq-entry");
        return Err(ExitCode::FAILURE);
    }
    let uid_map_mode_valid = if need_env {
        matches!(
            (
                a.uid_map_uin.is_some(),
                a.uid_map_surface_only,
                a.uid_map_result.is_some(),
            ),
            (true, false, true) | (false, true, true) | (false, false, false)
        )
    } else {
        a.uid_map_uin.is_some() == a.uid_map_result.is_some()
    };
    if !uid_map_mode_valid || (a.uid_map_profile_service && a.uid_map_uin.is_none()) {
        eprintln!("[uid-map] 请选择 round-trip(--uid-map-uin + --uid-map-result)、surface-only(--uid-map-surface-only + --uid-map-result)，或不启用 UID-map");
        return Err(ExitCode::FAILURE);
    }
    if need_env && a.uid_map_uin == Some(0) {
        eprintln!("[uid-map] round-trip UIN 必须大于零");
        return Err(ExitCode::FAILURE);
    }
    if (a.uid_map_uin.is_some() || a.uid_map_surface_only)
        && (a.daemon_pipe.is_some() || a.daemon_auth.is_some())
    {
        eprintln!("[uid-map] 一次性 UID-map 探针不能与 daemon hook 共用 owner pump");
        return Err(ExitCode::FAILURE);
    }
    if !(1_000..=180_000).contains(&a.uid_map_timeout_ms) {
        eprintln!("[uid-map] timeout 必须在 1000–180000 ms 之间");
        return Err(ExitCode::FAILURE);
    }
    let (pid, bridge, manifest, report, env, wait_s, wait_ms, confirmed) = (
        a.pid.clone(),
        a.bridge.clone(),
        a.manifest.clone(),
        a.report.clone(),
        a.env,
        a.wait_s,
        a.wait_ms,
        a.confirmed,
    );
    let (Some(pid), Some(bridge), Some(manifest), Some(report)) = (pid, bridge, manifest, report)
    else {
        usage();
        return Err(ExitCode::FAILURE);
    };
    if need_env && env.is_none() {
        eprintln!("--env <hex> 必填(envscan 的只读扫描结果;不猜测地址)");
        return Err(ExitCode::FAILURE);
    }
    println!("[gate 1] 核对模块基线 …");
    match verify_manifest(&manifest) {
        Ok(true) => println!("[gate 1] PASS"),
        Ok(false) => {
            eprintln!("[gate 1] REJECT — 基线不匹配,拒绝接入(不猜偏移)");
            return Err(ExitCode::from(2));
        }
        Err(e) => {
            eprintln!("[gate 1] error: {e}");
            return Err(ExitCode::FAILURE);
        }
    }
    if !confirmed {
        eprintln!("[gate 2] REJECT — 缺少 --confirm-designated-test-instance(test-scope §4)");
        return Err(ExitCode::from(2));
    }
    println!("[gate 2] PASS — 执行者已确认 PID {pid} 为指定测试实例");
    Ok((pid, bridge, report, env.unwrap_or(0), wait_s, wait_ms, a))
}

#[cfg(feature = "research")]
fn verify_uid_map_profile_build(pid: u32, manifest_path: &Path) -> Result<(), String> {
    // Bind this resolver to the host already investigated in Caligo and the
    // wrapper used by the reference probe. The general manifest gate runs first.
    const MODULES: [(&str, &str, &str); 3] = [
        (
            "node-runtime",
            "QQNT.dll",
            "75B19E58A123B0513E99DF4DF3EBC6524478535BA44651024D6A20821F5086E5",
        ),
        (
            "business-js-native",
            "major.node",
            "4564E402D1CF43A0F15982411596CF38B8D9B8FBC78F5E7CA717E27C24C74A07",
        ),
        (
            "session-shell-wrapper",
            "wrapper.node",
            caligo_bridge::g3::G3_PROFILE_9936_WRAPPER_SHA256,
        ),
    ];
    let manifest: super::Manifest =
        serde_json::from_slice(&std::fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut sizes = [0; MODULES.len()];
    for ((role, _, expected_hash), size) in MODULES.into_iter().zip(&mut sizes) {
        let mut entries = manifest.modules.iter().filter(|module| module.role == role);
        let entry = entries
            .next()
            .ok_or_else(|| format!("missing {role} in manifest"))?;
        if entries.next().is_some()
            || !entry.hash_checked
            || !entry.sha256.eq_ignore_ascii_case(expected_hash)
        {
            return Err(format!("unsupported {role} manifest for ProfileService"));
        }
        *size = entry.size_bytes;
    }
    let processes = winutil::list_processes("QQ.exe")?;
    let process = processes
        .iter()
        .find(|process| process.pid == pid)
        .ok_or("designated QQ process is unavailable")?;
    for ((_, name, expected_hash), expected_size) in MODULES.into_iter().zip(sizes) {
        let mut loaded = process.modules.iter().map(Path::new).filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|file_name| file_name.eq_ignore_ascii_case(name))
        });
        let path = loaded
            .next()
            .ok_or_else(|| format!("{name} is not loaded"))?;
        if loaded.next().is_some() {
            return Err(format!("multiple loaded modules named {name}"));
        }
        let (hash, size) = super::sha256_file(path)?;
        if !hash.eq_ignore_ascii_case(expected_hash) || size != expected_size {
            return Err(format!(
                "loaded {name} does not match the ProfileService build"
            ));
        }
    }
    Ok(())
}

fn print_report(report: &Path) {
    match std::fs::OpenOptions::new().read(true).open(report) {
        Ok(mut f) => {
            println!("=== entry log ({}) ===", report.display());
            let _ = std::io::stdout().flush();
            let _ = std::io::copy(&mut f, &mut std::io::stdout().lock());
            println!();
        }
        Err(e) => eprintln!("报告读取失败 {}: {e}", report.display()),
    }
}

/// 报告路径绝对化(文件可不存在:绝对化父目录 + 文件名)。
/// D7 缺陷修复:相对路径会按 QQ 进程工作目录解析,阶段日志因此丢失。
fn absolute_report_path(report: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(report) {
        return p;
    }
    match (report.parent(), report.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(p) => p.join(name),
            Err(_) => report.to_path_buf(),
        },
        _ => report.to_path_buf(),
    }
}

/// Reserve a fresh UID-map result only inside ignored local-evidence.
#[cfg(feature = "research")]
fn prepare_uid_map_result(path: &Path) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("current_dir: {e}"))?;
    let evidence_root = cwd
        .join("local-evidence")
        .canonicalize()
        .map_err(|e| format!("local-evidence directory unavailable: {e}"))?;
    let requested = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let parent = requested
        .parent()
        .ok_or_else(|| "result path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create result directory: {e}"))?;
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("canonicalize result directory: {e}"))?;
    if !parent.starts_with(&evidence_root) {
        return Err("UID-map result path must be under ignored local-evidence".into());
    }
    let name = requested
        .file_name()
        .ok_or_else(|| "result path has no file name".to_string())?;
    let output = parent.join(name);
    let _reservation = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|e| format!("result path must be new and writable: {e}"))?;
    Ok(output)
}

#[cfg(feature = "research")]
fn print_uid_map_summary(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("[uid-map] result file missing: {}", path.display());
        return false;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        eprintln!("[uid-map] result file is not valid JSON");
        return false;
    };
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let uid_count = v
        .get("uid_candidates")
        .and_then(|x| x.as_array())
        .map_or(0, Vec::len);
    let roundtrip = v
        .get("roundtrip_match")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    println!("[uid-map] status={status} uid_candidates={uid_count} reverse_matches_requested_uin={roundtrip}");
    println!(
        "[uid-map] raw identity values kept local at {}",
        path.display()
    );
    status == "roundtrip_match" && uid_count == 1 && roundtrip
}

#[cfg(feature = "research")]
fn print_uid_map_surface_summary(path: &Path, expected_pid: u32) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("[uid-map-surface] result file missing: {}", path.display());
        return false;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        eprintln!("[uid-map-surface] result file is not valid JSON");
        return false;
    };
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let available = v
        .get("group_service_available")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let api_call_attempted = v
        .get("api_call_attempted")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let raw_identity_values_written = v
        .get("raw_identity_values_written")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let raw_chat_text_written = v
        .get("raw_chat_text_written")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let pid_matches = v.get("pid").and_then(|x| x.as_u64()) == Some(u64::from(expected_pid));
    println!("[uid-map-surface] status={status} group_service_available={available} api_call_attempted={api_call_attempted} pid_matches={pid_matches}");
    println!(
        "[uid-map-surface] raw identity values written={raw_identity_values_written}; report={}",
        path.display()
    );
    available
        && !api_call_attempted
        && !raw_identity_values_written
        && !raw_chat_text_written
        && pid_matches
}

pub fn cmd_qq_entry(args: &[String]) -> ExitCode {
    let (pid, bridge, report, env, wait_s, _wait_ms, extra) = match gates(args, true) {
        Ok(v) => v,
        Err(c) => return c,
    };
    #[cfg(feature = "research")]
    if extra.uid_map_profile_service {
        let manifest = extra
            .manifest
            .as_deref()
            .expect("manifest checked by gates");
        if let Err(error) = verify_uid_map_profile_build(pid, manifest) {
            eprintln!("[uid-map-profile] exact-build gate rejected: {error}");
            return ExitCode::from(2);
        }
        println!("[uid-map-profile] exact loaded-module hashes verified");
    }
    #[cfg(feature = "research")]
    let uid_map = match (
        extra.uid_map_uin,
        extra.uid_map_surface_only,
        extra.uid_map_result.as_deref(),
    ) {
        (Some(uin), false, Some(path)) => match prepare_uid_map_result(path) {
            Ok(output) => Some(UidMapRequest::RoundTrip { uin, output }),
            Err(e) => {
                eprintln!("[uid-map] {e}");
                return ExitCode::FAILURE;
            }
        },
        (None, true, Some(path)) => match prepare_uid_map_result(path) {
            Ok(output) => Some(UidMapRequest::SurfaceOnly { output }),
            Err(e) => {
                eprintln!("[uid-map-surface] {e}");
                return ExitCode::FAILURE;
            }
        },
        _ => None,
    };
    println!("[qq-entry] QQNT 基址解析 …");
    let module_bases = match winutil::module_bases_in(pid) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("module_bases_in: {e}");
            return ExitCode::FAILURE;
        }
    };
    #[cfg(feature = "research")]
    if uid_map.is_some()
        && module_bases
            .keys()
            .any(|name| name.eq_ignore_ascii_case("caligo_bridge.dll"))
    {
        eprintln!("[uid-map] selected QQ process already has a Caligo bridge; use a clean restarted instance");
        return ExitCode::from(2);
    }
    let qqnt_base = *module_bases.get("QQNT.dll").unwrap_or(&0);
    if qqnt_base == 0 {
        eprintln!("QQNT.dll 基址未解析(主进程判据复核:test-scope §4)");
        return ExitCode::FAILURE;
    }
    println!("[qq-entry] qqnt_base={qqnt_base:#x} env={env:#x}");

    // 加载 bridge(LoadLibrary + probe;research 构建下 probe 可用)。
    println!("[qq-entry] 加载 bridge …");
    let probe_report = report.with_file_name("qq-entry-probe.json");
    let remote_base = match unsafe {
        winutil::inject_and_probe(
            pid,
            &bridge,
            &probe_report,
            None,
            None,
            false,
            20_000,
            None,
            None,
            None,
            None,
            None,
            10_000,
            None,
            10_000,
            None,
            0,
            None,
            None,
            None,
        )
    } {
        Ok(o) => o.remote_base,
        Err(e) => {
            eprintln!("[qq-entry] bridge 加载失败:{e}");
            return ExitCode::FAILURE;
        }
    };
    println!("[qq-entry] remote_base={remote_base:#x}");

    // 远程调用 caligo_qq_entry_bootstrap(ctx)。
    let code = unsafe { remote_bootstrap(pid, remote_base, qqnt_base, env, &report, wait_s) };
    let code = match code {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[qq-entry] {e}");
            return ExitCode::FAILURE;
        }
    };
    let verdict = map_code(code);
    println!("[qq-entry] bootstrap exit={code:#x} ({verdict:?})");
    #[cfg(feature = "research")]
    if let Some(request) = uid_map {
        // Both one-shot modes require a fresh owner generation so their pump hook
        // cannot collide with a pre-existing daemon hook.
        if verdict != QqEntryExit::Ok {
            eprintln!("[uid-map] bootstrap was not a fresh success; no UID-map operation was made");
            return ExitCode::from(2);
        }
        let (uin, mode, output, surface_only) = match request {
            UidMapRequest::RoundTrip { uin, output } => {
                let mode = if extra.uid_map_profile_service { 2 } else { 0 };
                (uin, mode, output, false)
            }
            UidMapRequest::SurfaceOnly { output } => (0, 1, output, true),
        };
        let query = unsafe {
            remote_uid_map_probe(
                pid,
                remote_base,
                uin,
                mode,
                &output,
                extra.uid_map_timeout_ms,
            )
        };
        let (query_code, may_run) = match query {
            Ok(code) => (Some(code), false),
            Err((e, may_run)) => {
                eprintln!("[uid-map] {e}");
                (None, may_run)
            }
        };
        if may_run {
            eprintln!("[uid-map] remote query thread completion is unknown; do not retry; close QQ normally to release the pinned bridge");
            return ExitCode::from(2);
        }
        let operation_ok = if query_code == Some(caligo_bridge::uid_map_probe::code::OK) {
            if surface_only {
                print_uid_map_surface_summary(&output, pid)
            } else {
                print_uid_map_summary(&output)
            }
        } else {
            false
        };
        let shutdown = unsafe { remote_shutdown(pid, remote_base, 10_000) };
        let shutdown_ok = match shutdown {
            Ok(code) if map_code(code) == QqEntryExit::Ok => {
                println!(
                    "[qq-entry] owner pump closed cleanly; bridge remains mapped until QQ exits"
                );
                true
            }
            Ok(code) => {
                eprintln!(
                    "[qq-entry] owner pump close exit={code:#x} ({:?})",
                    map_code(code)
                );
                false
            }
            Err(e) => {
                eprintln!("[qq-entry] owner pump close failed: {e}; do not hot-unload");
                false
            }
        };
        return if operation_ok && shutdown_ok {
            ExitCode::SUCCESS
        } else {
            if let Some(code) = query_code {
                if code != caligo_bridge::uid_map_probe::code::OK {
                    eprintln!("[uid-map] probe exit={code:#x}; no retry was attempted");
                }
            }
            ExitCode::from(2)
        };
    }
    print_report(&report);
    // D7b:bootstrap 成功后启动 daemon 客户端(可选)。
    if verdict == QqEntryExit::Ok {
        let (Some(pipe), Some(auth)) = (extra.daemon_pipe.clone(), extra.daemon_auth.clone())
        else {
            return ExitCode::SUCCESS;
        };
        let generation = extra.daemon_generation.unwrap_or(1);
        let account = extra
            .daemon_account
            .clone()
            .unwrap_or_else(|| "10001".to_string());
        let cfg_json = serde_json::json!({
            "pipe_name": pipe,
            "auth_token": auth,
            "session_generation": generation,
            "account": account,
            "module_baseline": "qq-9.9.33-52230",
            "report_path": absolute_report_path(&report).to_string_lossy(),
            "qqnt_base": format!("{qqnt_base:#x}"),
        });
        let code = unsafe { remote_daemon_start(pid, remote_base, &cfg_json.to_string()) };
        match code {
            Ok(0) => println!("[qq-entry] daemon 客户端已启动(管道 {pipe})"),
            Ok(c) => {
                eprintln!("[qq-entry] daemon 启动失败: {c:#x}");
                return ExitCode::from(2);
            }
            Err(e) => {
                eprintln!("[qq-entry] daemon 远程调用失败: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    match verdict {
        QqEntryExit::Ok => ExitCode::SUCCESS,
        QqEntryExit::Already => {
            println!("[qq-entry] 已初始化(每宿主代次一次)—— 单实例判据成立");
            ExitCode::SUCCESS
        }
        QqEntryExit::Timeout => {
            eprintln!("[qq-entry] 首入超时:空闲 QQ 的 JS 间隙延迟属已知候选(K2-03);非失败证据,由执行者决策");
            ExitCode::from(2)
        }
        _ => ExitCode::from(2),
    }
}

/// SAFETY: 前置:bridge 已加载。
/// daemon 启动导出为单指针签名(NUL 结尾 UTF-8 JSON)。
unsafe fn remote_daemon_start(pid: u32, remote_base: usize, cfg_json: &str) -> Result<u32, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };
    // SAFETY: 文档化远程调用序列。
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() || proc_h == INVALID_HANDLE_VALUE {
            return Err(format!("OpenProcess: Win32 error {}", GetLastError()));
        }
        let result = (|| -> Result<u32, String> {
            let addr =
                winutil::read_remote_export(proc_h, remote_base, "caligo_qq_daemon_client_start")?
                    .ok_or("caligo_qq_daemon_client_start not found(bridge 是否 research 构建?)")?;
            let mut bytes = cfg_json.as_bytes().to_vec();
            bytes.push(0); // NUL 结尾
            let remote_cfg = VirtualAllocEx(
                proc_h,
                std::ptr::null(),
                bytes.len(),
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote_cfg.is_null() {
                return Err(format!("alloc daemon cfg: Win32 error {}", GetLastError()));
            }
            let mut w: usize = 0;
            if WriteProcessMemory(
                proc_h,
                remote_cfg,
                bytes.as_ptr().cast(),
                bytes.len(),
                &mut w,
            ) == 0
                || w != bytes.len()
            {
                VirtualFreeEx(proc_h, remote_cfg, 0, MEM_RELEASE);
                return Err("write daemon cfg incomplete".into());
            }
            let start: winutil::RemoteThreadFn =
                std::mem::transmute::<usize, winutil::RemoteThreadFn>(addr);
            let thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start),
                remote_cfg,
                0,
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                VirtualFreeEx(proc_h, remote_cfg, 0, MEM_RELEASE);
                return Err(format!(
                    "CreateRemoteThread(daemon): Win32 error {}",
                    GetLastError()
                ));
            }
            let wr = WaitForSingleObject(thread, 30_000);
            let mut code: u32 = u32::MAX;
            GetExitCodeThread(thread, &mut code);
            CloseHandle(thread);
            if wr != 0 {
                winutil::retain_remote_pub(
                    pid,
                    remote_cfg as usize,
                    bytes.len(),
                    "daemon cfg",
                    "wait-timeout",
                );
                return Err(format!("daemon start wait failed (retained): code {wr:#x}"));
            }
            VirtualFreeEx(proc_h, remote_cfg, 0, MEM_RELEASE);
            Ok(code)
        })();
        CloseHandle(proc_h);
        result
    }
}

/// QQ 内 worker 计数取证:远程调用 caligo_qq_daemon_status → 打印报告尾部。
pub fn cmd_qq_status(args: &[String]) -> ExitCode {
    let (pid, bridge, report, _env, _ws, _wm, _extra) = match gates(args, false) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let probe_report = report.with_file_name("qq-status-probe.json");
    let remote_base = match unsafe {
        winutil::inject_and_probe(
            pid,
            &bridge,
            &probe_report,
            None,
            None,
            false,
            20_000,
            None,
            None,
            None,
            None,
            None,
            10_000,
            None,
            10_000,
            None,
            0,
            None,
            None,
            None,
        )
    } {
        Ok(o) => o.remote_base,
        Err(e) => {
            eprintln!("[qq-status] bridge 加载失败:{e}");
            return ExitCode::FAILURE;
        }
    };
    let code = unsafe { remote_status(pid, remote_base) };
    let code = match code {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[qq-status] {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("[qq-status] exit={code:#x}");
    print_report(&report);
    ExitCode::SUCCESS
}

/// SAFETY: 前置:bridge 已加载。
unsafe fn remote_status(pid: u32, remote_base: usize) -> Result<u32, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };
    // SAFETY: 文档化远程调用序列;单参占位导出。
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() || proc_h == INVALID_HANDLE_VALUE {
            return Err(format!("OpenProcess: Win32 error {}", GetLastError()));
        }
        let addr = match winutil::read_remote_export(proc_h, remote_base, "caligo_qq_daemon_status")
        {
            Ok(Some(a)) => a,
            Ok(None) => {
                CloseHandle(proc_h);
                return Err("caligo_qq_daemon_status not found".into());
            }
            Err(e) => {
                CloseHandle(proc_h);
                return Err(e);
            }
        };
        let start: winutil::RemoteThreadFn =
            std::mem::transmute::<usize, winutil::RemoteThreadFn>(addr);
        let thread = CreateRemoteThread(
            proc_h,
            std::ptr::null(),
            0,
            Some(start),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
        );
        if thread.is_null() {
            CloseHandle(proc_h);
            return Err(format!(
                "CreateRemoteThread(status): Win32 error {}",
                GetLastError()
            ));
        }
        let wr = WaitForSingleObject(thread, 15_000);
        let mut code: u32 = u32::MAX;
        GetExitCodeThread(thread, &mut code);
        CloseHandle(thread);
        CloseHandle(proc_h);
        if wr != 0 {
            return Err(format!("status wait failed: code {wr:#x}"));
        }
        Ok(code)
    }
}

/// SAFETY: 前置:bridge 已加载、pid 为执行者指定实例。
unsafe fn remote_bootstrap(
    pid: u32,
    remote_base: usize,
    qqnt_base: usize,
    env: usize,
    report: &Path,
    wait_s: u32,
) -> Result<u32, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };
    // SAFETY: 文档化远程调用序列;门控与留存账本同 inject 路径。
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() || proc_h == INVALID_HANDLE_VALUE {
            return Err(format!("OpenProcess: Win32 error {}", GetLastError()));
        }
        let result = (|| -> Result<u32, String> {
            let addr =
                winutil::read_remote_export(proc_h, remote_base, "caligo_qq_entry_bootstrap")?
                    .ok_or("caligo_qq_entry_bootstrap not found(bridge 是否 research 构建?)")?;
            // D7 缺陷修复:相对路径会按 QQ 进程的工作目录解析(首入阶段日志
            // 即因此丢失)。传给 DLL 的报告路径必须绝对化。
            let report_abs = absolute_report_path(report);
            let wide: Vec<u16> = report_abs
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let remote_path = VirtualAllocEx(
                proc_h,
                std::ptr::null(),
                wide.len() * 2,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote_path.is_null() {
                return Err(format!("alloc report path: Win32 error {}", GetLastError()));
            }
            let mut w: usize = 0;
            if WriteProcessMemory(
                proc_h,
                remote_path,
                wide.as_ptr().cast(),
                wide.len() * 2,
                &mut w,
            ) == 0
                || w != wide.len() * 2
            {
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err("write report path incomplete".into());
            }
            // caligo_bridge::QqEntryCtx{qqnt_base, env, report_path, wait_s, pad}。
            let mut ctx: Vec<u8> = Vec::with_capacity(40);
            ctx.extend_from_slice(&qqnt_base.to_le_bytes());
            ctx.extend_from_slice(&env.to_le_bytes());
            ctx.extend_from_slice(&(remote_path as usize).to_le_bytes());
            ctx.extend_from_slice(&wait_s.to_le_bytes());
            ctx.extend_from_slice(&0u32.to_le_bytes());
            let remote_ctx = VirtualAllocEx(
                proc_h,
                std::ptr::null(),
                ctx.len(),
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote_ctx.is_null() {
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err(format!("alloc ctx: Win32 error {}", GetLastError()));
            }
            let mut w2: usize = 0;
            if WriteProcessMemory(proc_h, remote_ctx, ctx.as_ptr().cast(), ctx.len(), &mut w2) == 0
                || w2 != ctx.len()
            {
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err("write ctx incomplete".into());
            }
            let bootstrap_fn: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 =
                std::mem::transmute::<
                    usize,
                    unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
                >(addr);
            let start: winutil::RemoteThreadFn =
                std::mem::transmute::<usize, winutil::RemoteThreadFn>(bootstrap_fn as usize);
            let thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start),
                remote_ctx,
                0,
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err(format!(
                    "CreateRemoteThread(bootstrap): Win32 error {}",
                    GetLastError()
                ));
            }
            // 首入等待 = wait_s + 60s 余量;超时 ≠ 远程线程终止(§5.3):
            // 缓冲转入留存账本,不释放。
            let wait_total = ((wait_s as u64) * 1000 + 60_000).min(u32::MAX as u64) as u32;
            let wr = WaitForSingleObject(thread, wait_total);
            let mut code: u32 = u32::MAX;
            GetExitCodeThread(thread, &mut code);
            CloseHandle(thread);
            if wr != 0 {
                winutil::retain_remote_pub(
                    pid,
                    remote_ctx as usize,
                    ctx.len(),
                    "qq-entry ctx",
                    "wait-timeout",
                );
                winutil::retain_remote_pub(
                    pid,
                    remote_path as usize,
                    wide.len() * 2,
                    "qq-entry report path",
                    "wait-timeout",
                );
                return Err(format!(
                    "bootstrap thread wait failed (buffers retained): code {wr:#x}"
                ));
            }
            VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
            VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
            Ok(code)
        })();
        CloseHandle(proc_h);
        result
    }
}

/// SAFETY: bridge was loaded by the gated `qq-entry` path and PID is the confirmed target.
/// The remote thread only queues a fixed read-only owner-pump operation.
#[cfg(feature = "research")]
unsafe fn remote_uid_map_probe(
    pid: u32,
    remote_base: usize,
    uin: u64,
    mode: u32,
    output: &Path,
    timeout_ms: u32,
) -> Result<u32, (String, bool)> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };
    // SAFETY: each remote buffer stays allocated through the thread's completion;
    // a wait timeout retains both buffers in the existing remote-allocation ledger.
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() || proc_h == INVALID_HANDLE_VALUE {
            return Err((
                format!("OpenProcess: Win32 error {}", GetLastError()),
                false,
            ));
        }
        let result = (|| -> Result<u32, (String, bool)> {
            let addr = winutil::read_remote_export(proc_h, remote_base, "caligo_qq_uid_map_probe")
                .map_err(|e| (e, false))?
                .ok_or_else(|| {
                    (
                        "caligo_qq_uid_map_probe not found (bridge must be a research build)"
                            .into(),
                        false,
                    )
                })?;
            let output_abs = absolute_report_path(output);
            let wide: Vec<u16> = output_abs
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let remote_path = VirtualAllocEx(
                proc_h,
                std::ptr::null(),
                wide.len() * 2,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote_path.is_null() {
                return Err((
                    format!("alloc UID-map result path: Win32 error {}", GetLastError()),
                    false,
                ));
            }
            let mut written = 0usize;
            if WriteProcessMemory(
                proc_h,
                remote_path,
                wide.as_ptr().cast(),
                wide.len() * 2,
                &mut written,
            ) == 0
                || written != wide.len() * 2
            {
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err(("write UID-map result path incomplete".into(), false));
            }
            // repr(C) QqUidMapProbeCtx is { u64, usize, u32, u32 } on x64.
            let mut ctx = Vec::with_capacity(24);
            ctx.extend_from_slice(&uin.to_le_bytes());
            ctx.extend_from_slice(&(remote_path as usize).to_le_bytes());
            ctx.extend_from_slice(&timeout_ms.to_le_bytes());
            ctx.extend_from_slice(&mode.to_le_bytes());
            let remote_ctx = VirtualAllocEx(
                proc_h,
                std::ptr::null(),
                ctx.len(),
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            if remote_ctx.is_null() {
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err((
                    format!("alloc UID-map context: Win32 error {}", GetLastError()),
                    false,
                ));
            }
            let mut written_ctx = 0usize;
            if WriteProcessMemory(
                proc_h,
                remote_ctx,
                ctx.as_ptr().cast(),
                ctx.len(),
                &mut written_ctx,
            ) == 0
                || written_ctx != ctx.len()
            {
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err(("write UID-map context incomplete".into(), false));
            }
            let start: winutil::RemoteThreadFn =
                std::mem::transmute::<usize, winutil::RemoteThreadFn>(addr);
            let thread = CreateRemoteThread(
                proc_h,
                std::ptr::null(),
                0,
                Some(start),
                remote_ctx,
                0,
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
                VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
                return Err((
                    format!(
                        "CreateRemoteThread(uid-map): Win32 error {}",
                        GetLastError()
                    ),
                    false,
                ));
            }
            let wait_total = timeout_ms.saturating_add(30_000);
            let wait = WaitForSingleObject(thread, wait_total);
            let mut code = u32::MAX;
            let exit_read = GetExitCodeThread(thread, &mut code);
            CloseHandle(thread);
            if wait != 0 {
                winutil::retain_remote_pub(
                    pid,
                    remote_ctx as usize,
                    ctx.len(),
                    "qq UID-map probe context",
                    "wait-timeout",
                );
                winutil::retain_remote_pub(
                    pid,
                    remote_path as usize,
                    wide.len() * 2,
                    "qq UID-map result path",
                    "wait-timeout",
                );
                return Err((
                    format!("UID-map remote thread completion unknown: wait={wait:#x}; buffers retained"),
                    true,
                ));
            }
            VirtualFreeEx(proc_h, remote_ctx, 0, MEM_RELEASE);
            VirtualFreeEx(proc_h, remote_path, 0, MEM_RELEASE);
            if exit_read == 0 {
                return Err((
                    format!("GetExitCodeThread(uid-map): Win32 error {}", GetLastError()),
                    false,
                ));
            }
            Ok(code)
        })();
        CloseHandle(proc_h);
        result
    }
}

pub fn cmd_qq_entry_stop(args: &[String]) -> ExitCode {
    let (pid, bridge, report, _env, _wait_s, wait_ms, _extra) = match gates(args, false) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let probe_report = report.with_file_name("qq-entry-stop-probe.json");
    let remote_base = match unsafe {
        winutil::inject_and_probe(
            pid,
            &bridge,
            &probe_report,
            None,
            None,
            false,
            20_000,
            None,
            None,
            None,
            None,
            None,
            10_000,
            None,
            10_000,
            None,
            0,
            None,
            None,
            None,
        )
    } {
        Ok(o) => o.remote_base,
        Err(e) => {
            eprintln!("[qq-entry-stop] bridge 加载失败:{e}");
            return ExitCode::FAILURE;
        }
    };
    // SAFETY: shutdown 导出签名为 extern "system" fn(u32);远程线程首参
    // 经 rcx 到达,u32 读低 32 位即 wait_ms(值语义,不解引用)。
    let code = unsafe { remote_shutdown(pid, remote_base, wait_ms) };
    let code = match code {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[qq-entry-stop] {e}");
            return ExitCode::FAILURE;
        }
    };
    let verdict = map_code(code);
    println!("[qq-entry-stop] shutdown exit={code:#x} ({verdict:?})");
    print_report(&report);
    match verdict {
        QqEntryExit::Ok => ExitCode::SUCCESS,
        QqEntryExit::ShutdownTimeout => {
            eprintln!("[qq-entry-stop] 关闭超时:句柄保留(不冒充成功);按 §6.7 属 Quarantined 候选");
            ExitCode::from(2)
        }
        _ => ExitCode::from(2),
    }
}

/// SAFETY: 前置:bridge 已加载。
unsafe fn remote_shutdown(pid: u32, remote_base: usize, wait_ms: u32) -> Result<u32, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };
    // SAFETY: 文档化远程调用序列。
    unsafe {
        let proc_h = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if proc_h.is_null() || proc_h == INVALID_HANDLE_VALUE {
            return Err(format!("OpenProcess: Win32 error {}", GetLastError()));
        }
        let addr =
            match winutil::read_remote_export(proc_h, remote_base, "caligo_qq_entry_shutdown") {
                Ok(Some(a)) => a,
                Ok(None) => {
                    CloseHandle(proc_h);
                    return Err("caligo_qq_entry_shutdown not found".into());
                }
                Err(e) => {
                    CloseHandle(proc_h);
                    return Err(e);
                }
            };
        let start: winutil::RemoteThreadFn =
            std::mem::transmute::<usize, winutil::RemoteThreadFn>(addr);
        let thread = CreateRemoteThread(
            proc_h,
            std::ptr::null(),
            0,
            Some(start),
            wait_ms as *mut core::ffi::c_void,
            0,
            std::ptr::null_mut(),
        );
        if thread.is_null() {
            CloseHandle(proc_h);
            return Err(format!(
                "CreateRemoteThread(shutdown): Win32 error {}",
                GetLastError()
            ));
        }
        let wr = WaitForSingleObject(thread, wait_ms.saturating_add(30_000));
        let mut code: u32 = u32::MAX;
        GetExitCodeThread(thread, &mut code);
        CloseHandle(thread);
        CloseHandle(proc_h);
        if wr != 0 {
            return Err(format!("shutdown wait failed: code {wr:#x}"));
        }
        Ok(code)
    }
}
