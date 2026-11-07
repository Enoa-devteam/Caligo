//! One-shot, read-only QQNT UID/UIN resolver probe (research build only).
//!
//! The exported entry is called from the loader thread only to install a fixed owner-pump task.
//! All V8 and QQ IPC work runs from the K4 owner loop; the operation never sends chat content.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

pub mod code {
    pub const OK: u32 = 0;
    pub const ERR_NOT_BOOTSTRAPPED: u32 = 1;
    pub const ERR_V8_RESOLVE: u32 = 2;
    pub const ERR_BUSY: u32 = 3;
    pub const ERR_WAKE: u32 = 4;
    pub const ERR_TIMEOUT: u32 = 5;
    pub const ERR_V8: u32 = 6;
    pub const ERR_REPORT: u32 = 7;
    pub const ERR_BAD_TIMEOUT: u32 = 8;
    pub const ERR_BAD_MODE: u32 = 9;
}

const START: usize = 0;
const V8: usize = 1;
const STATE: usize = 2;
const BINDING: usize = 3;
const WRAPPERS: usize = 4;
const GROUP: usize = 5;
const METHODS: usize = 6;
const QUERY: usize = 7;
const POLL: usize = 8;
const DONE: usize = 9;
const MAX_RESULT_BYTES: usize = 128 * 1024;

fn run_owner_step<F>(
    stage: &AtomicUsize,
    last_logged_stage: &AtomicUsize,
    tx: &mpsc::SyncSender<Result<(), u32>>,
    current: usize,
    next: usize,
    entered: &'static str,
    completed: &'static str,
    expected: &'static str,
    run: F,
) -> usize
where
    F: FnOnce() -> Result<String, crate::qq_v8::V8Error>,
{
    if last_logged_stage.swap(current, Ordering::AcqRel) != current {
        crate::qq_entry::append_stage_simple(entered, true, "fixed QQNT wrapper step entered");
    }
    match run() {
        Ok(v) if v == expected => {
            crate::qq_entry::append_stage_simple(
                completed,
                true,
                "fixed QQNT wrapper step returned",
            );
            stage.store(next, Ordering::Release);
            1
        }
        Ok(v) if v == "SURFACE_DONE" => {
            crate::qq_entry::append_stage_simple(
                "uid_map_surface_closed",
                true,
                "surface result recorded; no resolver API call was dispatched",
            );
            stage.store(POLL, Ordering::Release);
            1
        }
        Ok(v) if v == "BUSY" => {
            stage.store(DONE, Ordering::Release);
            let _ = tx.try_send(Err(code::ERR_BUSY));
            1
        }
        Ok(v) if v.starts_with("ERR:") => {
            crate::qq_entry::append_stage_simple(
                "uid_map_wrapper_step_failed",
                false,
                "fixed QQNT wrapper step returned an error",
            );
            stage.store(DONE, Ordering::Release);
            let _ = tx.try_send(Err(code::ERR_V8));
            1
        }
        Ok(_) => {
            crate::qq_entry::append_stage_simple(
                "uid_map_wrapper_step_failed",
                false,
                "fixed QQNT wrapper step returned an unexpected status",
            );
            stage.store(DONE, Ordering::Release);
            let _ = tx.try_send(Err(code::ERR_V8));
            1
        }
        Err(crate::qq_v8::V8Error::NoCurrentContext) => 0,
        Err(_) => {
            crate::qq_entry::append_stage_simple(
                "uid_map_wrapper_step_failed",
                false,
                "fixed QQNT wrapper step could not enter V8",
            );
            stage.store(DONE, Ordering::Release);
            let _ = tx.try_send(Err(code::ERR_V8));
            1
        }
    }
}

/// Run one batch forward lookup and one inverse lookup for a single friend UIN.
///
/// `output_path` is supplied by the CLI and must point into ignored local-evidence.
/// The output intentionally contains the exact IDs so they remain available for local review.
pub fn run(uin: u64, output_path: PathBuf, timeout_ms: u32) -> u32 {
    run_inner(Some(uin), output_path, timeout_ms)
}

/// Record only whether the active wrapper session exposes a GroupService object.
/// This mode never reads or calls UID/UIN resolver methods.
pub fn run_surface_only(output_path: PathBuf, timeout_ms: u32) -> u32 {
    run_inner(None, output_path, timeout_ms)
}

fn run_inner(query_uin: Option<u64>, output_path: PathBuf, timeout_ms: u32) -> u32 {
    if timeout_ms < 1_000 || timeout_ms > 180_000 {
        return code::ERR_BAD_TIMEOUT;
    }
    if !crate::qq_entry::is_ready() {
        return code::ERR_NOT_BOOTSTRAPPED;
    }
    let qqnt_base = crate::qq_entry::qqnt_base();
    let Some(syms) = crate::qq_v8::V8Symbols::resolve(qqnt_base, "") else {
        return code::ERR_V8_RESOLVE;
    };
    crate::qq_v8::store_symbols_raw(syms);

    let (tx, rx) = mpsc::sync_channel::<Result<(), u32>>(1);
    let stage = Arc::new(AtomicUsize::new(START));
    let cancelled = Arc::new(AtomicBool::new(false));
    let hook_stage = stage.clone();
    let last_logged_stage = Arc::new(AtomicUsize::new(usize::MAX));
    let hook_last_logged_stage = last_logged_stage.clone();
    let hook_cancelled = cancelled.clone();
    let surface_only = query_uin.is_none();
    let hook_surface_only = surface_only;
    let query_uin = query_uin.map(|uin| uin.to_string());
    let hook_uin = query_uin.clone().unwrap_or_default();
    let hook_output = output_path.clone();
    let hook_tx = tx.clone();

    let installed = crate::qq_entry::try_install_drain_hook(Box::new(move || {
        if hook_cancelled.load(Ordering::Acquire) {
            let _ = crate::qq_v8::cancel_uid_map_probe();
            hook_stage.store(DONE, Ordering::Release);
            let _ = hook_tx.try_send(Err(code::ERR_TIMEOUT));
            return 1;
        }

        match hook_stage.load(Ordering::Acquire) {
            START => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                START,
                V8,
                "uid_map_v8_entered",
                "uid_map_v8_ready",
                "V8_READY",
                crate::qq_v8::start_uid_map_sanity,
            ),
            V8 => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                V8,
                STATE,
                "uid_map_state_entered",
                "uid_map_state_ready",
                "STATE_READY",
                crate::qq_v8::start_uid_map_state,
            ),
            STATE => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                STATE,
                BINDING,
                "uid_map_binding_entered",
                "uid_map_binding_ready",
                "BINDING_READY",
                crate::qq_v8::start_uid_map_probe,
            ),
            BINDING => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                BINDING,
                WRAPPERS,
                "uid_map_wrapper_classes_entered",
                "uid_map_wrapper_classes_ready",
                "WRAPPERS_READY",
                crate::qq_v8::start_uid_map_wrappers,
            ),
            WRAPPERS => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                WRAPPERS,
                GROUP,
                "uid_map_session_entered",
                "uid_map_session_ready",
                "SESSION_READY",
                crate::qq_v8::start_uid_map_session,
            ),
            GROUP => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                GROUP,
                METHODS,
                "uid_map_group_service_entered",
                "uid_map_group_service_ready",
                "GROUP_READY",
                crate::qq_v8::start_uid_map_group,
            ),
            METHODS if hook_surface_only => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                METHODS,
                POLL,
                "uid_map_surface_only_entered",
                "uid_map_surface_only_ready",
                "SURFACE_DONE",
                crate::qq_v8::finish_uid_map_surface_only,
            ),
            METHODS => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                METHODS,
                QUERY,
                "uid_map_methods_entered",
                "uid_map_methods_ready",
                "METHODS_READY",
                || crate::qq_v8::start_uid_map_methods(&hook_uin),
            ),
            QUERY if hook_surface_only => {
                hook_stage.store(DONE, Ordering::Release);
                let _ = hook_tx.try_send(Err(code::ERR_BAD_MODE));
                1
            }
            QUERY => run_owner_step(
                &hook_stage,
                &hook_last_logged_stage,
                &hook_tx,
                QUERY,
                POLL,
                "uid_map_api_entered",
                "uid_map_api_dispatched",
                "DISPATCHED",
                crate::qq_v8::start_uid_map_query,
            ),
            POLL => match crate::qq_v8::poll_uid_map_probe() {
                Ok(v) if v == "PENDING" => 1,
                Ok(v) if v.starts_with("ERR:") => {
                    hook_stage.store(DONE, Ordering::Release);
                    let _ = hook_tx.try_send(Err(code::ERR_V8));
                    1
                }
                Ok(v) => {
                    let parsed: Result<Value, _> = serde_json::from_str(&v);
                    let Ok(mut result) = parsed else {
                        hook_stage.store(DONE, Ordering::Release);
                        let _ = hook_tx.try_send(Err(code::ERR_V8));
                        return 1;
                    };
                    if v.len() > MAX_RESULT_BYTES {
                        result = json!({"status":"result_too_large","bytes":v.len()});
                    }
                    let status = result
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let uids = result.get("uidCandidates").cloned().unwrap_or(Value::Null);
                    let reverse_uins = result
                        .get("reverseUinValues")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let captured_unix_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or_default();
                    let report = if hook_surface_only {
                        json!({
                            "schema": 1,
                            "captured_unix_ms": captured_unix_ms,
                            "pid": std::process::id(),
                            "route": result.get("route").cloned().unwrap_or(Value::Null),
                            "group_service_available": status == "group_service_available",
                            "status": status,
                            "api_call_attempted": false,
                            "raw_identity_values_written": false,
                            "raw_chat_text_written": false
                        })
                    } else {
                        json!({
                            "schema": 1,
                            "captured_unix_ms": captured_unix_ms,
                            "pid": std::process::id(),
                            "requested_uin": query_uin.clone().unwrap_or_default(),
                            "forward_api": "nodeIKernelGroupService/getUidByUins",
                            "forward_payload_shape": "[[uin]]",
                            "reverse_api": "nodeIKernelGroupService/getUinByUids",
                            "reverse_payload_shape": "[uidList]",
                            "route": result.get("route").cloned().unwrap_or(Value::Null),
                            "api_call_attempted": result
                                .get("apiCallAttempted")
                                .cloned()
                                .unwrap_or(Value::Bool(false)),
                            "forward_arity": result.get("forward_arity").cloned().unwrap_or(Value::Null),
                            "reverse_arity": result.get("reverse_arity").cloned().unwrap_or(Value::Null),
                            "status": status,
                            "uid_candidates": uids,
                            "reverse_uin_values": reverse_uins,
                            "roundtrip_match": status == "roundtrip_match",
                            "forward_result": result.get("forward").cloned().unwrap_or(Value::Null),
                            "reverse_result": result.get("reverse").cloned().unwrap_or(Value::Null),
                            "error": result.get("error").cloned().unwrap_or(Value::Null),
                            "raw_identity_values_written": true,
                            "raw_chat_text_written": false
                        })
                    };
                    let write_result = serde_json::to_vec_pretty(&report)
                        .map_err(|_| ())
                        .and_then(|bytes| std::fs::write(&hook_output, bytes).map_err(|_| ()))
                        .map_err(|_| code::ERR_REPORT);
                    hook_stage.store(DONE, Ordering::Release);
                    match write_result {
                        Ok(()) => {
                            crate::qq_entry::append_stage_simple(
                                "uid_map_probe",
                                true,
                                if hook_surface_only {
                                    "GroupService surface result written; resolver API not dispatched"
                                } else {
                                    "one read-only batch round-trip result written to local evidence"
                                },
                            );
                            let _ = hook_tx.try_send(Ok(()));
                        }
                        Err(e) => {
                            crate::qq_entry::append_stage_simple(
                                "uid_map_probe",
                                false,
                                "local evidence result write failed",
                            );
                            let _ = hook_tx.try_send(Err(e));
                        }
                    }
                    1
                }
                Err(crate::qq_v8::V8Error::NoCurrentContext) => 0,
                Err(_) => {
                    hook_stage.store(DONE, Ordering::Release);
                    let _ = hook_tx.try_send(Err(code::ERR_V8));
                    1
                }
            },
            _ => 0,
        }
    }));
    if !installed {
        return code::ERR_BUSY;
    }

    let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        if let Ok(result) = rx.try_recv() {
            crate::qq_entry::clear_drain_hook();
            return result.map_or_else(|e| e, |_| code::OK);
        }
        if Instant::now() >= deadline {
            cancelled.store(true, Ordering::Release);
            let _ = crate::qq_entry::wake_owner_pump();
            let _ = rx.recv_timeout(Duration::from_millis(250));
            crate::qq_entry::clear_drain_hook();
            crate::qq_entry::append_stage_simple(
                "uid_map_probe",
                false,
                "timeout; request was not retried",
            );
            return code::ERR_TIMEOUT;
        }
        if !crate::qq_entry::wake_owner_pump() {
            crate::qq_entry::clear_drain_hook();
            return code::ERR_WAKE;
        }
        if let Ok(result) = rx.recv_timeout(Duration::from_millis(100)) {
            crate::qq_entry::clear_drain_hook();
            return result.map_or_else(|e| e, |_| code::OK);
        }
    }
}
