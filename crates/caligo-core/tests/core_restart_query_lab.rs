//! Two actual caligod processes, one real bridge worker, synthetic owner only.
#![cfg(feature = "research")]

use std::io::BufRead;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use caligo_bridge::daemon_client::{counters_snapshot, run_worker, DaemonClientConfig, WorkerExit};
use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{Resident, ResidentLimits};
use caligo_core::daemon::ControlClient;
use caligo_core::ipc::ControlMsg;
use caligo_core::onebot::{RET_NOT_READY, RET_UNSUPPORTED};
use caligo_model::capability::{
    HostQuery, LoginInfo, QueryCapability, QueryData, QueryOutcome, QueryScope,
};
use serde_json::{json, Value};

static SERIAL: Mutex<()> = Mutex::new(());
struct CoreProcess {
    child: Child,
    ready: Value,
}
impl CoreProcess {
    fn start(dir: &Path, prefix: &str, label: &str) -> Self {
        let stderr = std::fs::File::create(dir.join(format!("{label}.stderr.log"))).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_caligod"))
            .args([
                "--pipe-prefix",
                prefix,
                "--account",
                "10001",
                "--baseline",
                "qq-9.9.33-52230",
                "--expect-pid",
                &std::process::id().to_string(),
                "--journal",
                dir.join("journal.log").to_str().unwrap(),
                "--token-file",
                dir.join("auth.private").to_str().unwrap(),
                "--onebot-http-bind",
                "127.0.0.1:0",
            ])
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .unwrap();
        let mut core = Self {
            child,
            ready: Value::Null,
        };
        let stdout = core.child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let result = reader.read_line(&mut line).map(|_| line);
            let _ = tx.send(result);
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        let line = rx
            .recv_timeout(Duration::from_secs(8))
            .expect("caligod startup deadline")
            .unwrap();
        core.ready = serde_json::from_str(&line).expect("caligod ready JSON");
        assert!(core.ready["onebot_http"]
            .as_str()
            .unwrap()
            .starts_with("127.0.0.1:"));
        core
    }
    fn terminate(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
    fn wait_clean(&mut self) {
        wait_until(|| self.child.try_wait().unwrap().is_some());
        assert!(
            self.child.wait().unwrap().success(),
            "second core clean shutdown"
        );
    }
}
impl Drop for CoreProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct WorkerGuard {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
#[derive(Clone, Debug)]
struct Call {
    id: String,
    scope: QueryScope,
    operation: &'static str,
}
struct Owner {
    thread: std::thread::ThreadId,
    calls: Arc<Mutex<Vec<Call>>>,
    complete: Arc<AtomicBool>,
    first_id: Option<String>,
}
impl HostAdapter for Owner {
    fn owner_thread_id(&self) -> u64 {
        1
    }
    fn current_thread_id(&self) -> u64 {
        if std::thread::current().id() == self.thread {
            1
        } else {
            2
        }
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        assert_eq!(std::thread::current().id(), self.thread);
        match op {
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
            _ => panic!("query-only fixture"),
        }
    }
    fn query_capabilities(&self) -> &'static [QueryCapability] {
        &[QueryCapability::LoginInfo, QueryCapability::SelfProfile]
    }
    fn query(
        &mut self,
        id: &str,
        scope: &QueryScope,
        query: &HostQuery,
        _: u64,
        poll: bool,
    ) -> Option<QueryOutcome> {
        assert_eq!(std::thread::current().id(), self.thread);
        if !poll && self.first_id.is_none() {
            self.first_id = Some(id.into());
        }
        if poll {
            assert_ne!(
                Some(id),
                self.first_id.as_deref(),
                "old request resumed after core restart"
            );
        }
        self.calls.lock().unwrap().push(Call {
            id: id.into(),
            scope: scope.clone(),
            operation: if poll { "poll" } else { "start" },
        });
        if !poll || !self.complete.load(Ordering::Acquire) {
            return None;
        }
        let info = LoginInfo {
            user_id: scope.account.clone(),
            nickname: "Fresh after core restart".into(),
            online: true,
        };
        Some(QueryOutcome::Success(match query {
            HostQuery::LoginInfo => QueryData::LoginInfo(info),
            HostQuery::SelfProfile => QueryData::SelfProfile(info),
            _ => panic!("self query expected"),
        }))
    }
    fn cancel_query(&mut self, id: &str, scope: &QueryScope) {
        assert_eq!(std::thread::current().id(), self.thread);
        self.calls.lock().unwrap().push(Call {
            id: id.into(),
            scope: scope.clone(),
            operation: "cancel",
        });
    }
}
fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !predicate() {
        assert!(Instant::now() < deadline, "condition deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn query(ready: Value, refresh: bool) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .timeout_read(Duration::from_secs(12))
        .build();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let response=agent.post(&format!("http://{}/",ready["onebot_http"].as_str().unwrap()))
            .set("Authorization",&format!("Bearer {}",ready["auth_token"].as_str().unwrap()))
            .set("Content-Type","application/json")
            .send_string(&json!({"action":"get_login_info","params":{"no_cache":refresh},"echo":"core-restart-lab"}).to_string())
            .map_err(|e|e.to_string())?;
        let value: Value =
            serde_json::from_str(&response.into_string().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        // Only retry a known pre-dispatch readiness rejection in this LAB.
        if [RET_NOT_READY, RET_UNSUPPORTED].contains(&(value["retcode"].as_i64().unwrap() as i32))
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        return Ok(value);
    }
}
fn pump_until(
    resident: &Resident<Owner>,
    wake: &mpsc::Receiver<()>,
    mut predicate: impl FnMut() -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < deadline, "owner query deadline");
        if wake.recv_timeout(Duration::from_millis(25)).is_ok() {
            resident.drain_queries();
        }
    }
}
fn run_case(refresh: bool) {
    let _serial = SERIAL.lock().unwrap();
    let tag = format!(
        "{}-{}-{}",
        std::process::id(),
        refresh,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(format!("caligo-core-restart-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    let prefix = format!("caligo-restart-{tag}");
    let mut first = CoreProcess::start(&dir, &prefix, "first");
    let calls = Arc::new(Mutex::new(Vec::<Call>::new()));
    let complete = Arc::new(AtomicBool::new(false));
    let resident = Arc::new(Resident::new(
        Owner {
            thread: std::thread::current().id(),
            calls: calls.clone(),
            complete: complete.clone(),
            first_id: None,
        },
        ResidentLimits::default(),
    ));
    resident.bootstrap().unwrap();
    let cfg = DaemonClientConfig {
        pipe_name: first.ready["bridge_pipe"].as_str().unwrap().into(),
        auth_token: first.ready["auth_token"].as_str().unwrap().into(),
        bridge_build: "core-restart-lab".into(),
        session_generation: 1,
        account: "10001".into(),
        module_baseline: "qq-9.9.33-52230".into(),
        host_nonce: 0xAA00_0001,
        host_process_created_utc: "synthetic-parent-generation".into(),
        self_uid: String::new(),
        heartbeat_ms: 500,
        reconnect_backoff_ms: 100,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (wake_tx, wake_rx) = mpsc::channel();
    let (exit_tx, exit_rx) = mpsc::channel();
    let (_result_tx, result_rx) = mpsc::channel();
    let (_events_tx, events_rx) = mpsc::channel();
    let r = resident.clone();
    let s = stop.clone();
    let handle = std::thread::spawn(move || {
        let outcome = run_worker(
            cfg,
            r,
            &|| wake_tx.send(()).is_ok(),
            result_rx,
            events_rx,
            s,
        );
        let _ = exit_tx.send(outcome);
    });
    let mut worker = WorkerGuard {
        stop,
        handle: Some(handle),
    };
    let ready = first.ready.clone();
    let old_http = std::thread::spawn(move || query(ready, refresh));
    pump_until(&resident, &wake_rx, || {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.operation == "start")
            .count()
            == 1
    });
    let old = calls.lock().unwrap()[0].clone();
    // Keep this exact owner paused through termination, reconnect and re-advertisement.
    first.terminate();
    assert!(
        old_http.join().unwrap().is_err(),
        "first core must not complete the in-flight HTTP request"
    );
    wait_until(|| counters_snapshot().reconnects >= 1);
    let mut second = CoreProcess::start(&dir, &prefix, "second");
    assert_ne!(first.child.id(), second.child.id());
    assert_eq!(first.ready["auth_token"], second.ready["auth_token"]);
    assert_eq!(first.ready["bridge_pipe"], second.ready["bridge_pipe"]);
    wait_until(|| counters_snapshot().hellos_accepted >= 2);
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "worker cannot execute owner cancellation"
    );
    let ready = second.ready.clone();
    let fresh_http = std::thread::spawn(move || query(ready, refresh));
    pump_until(&resident, &wake_rx, || {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.operation == "start")
            .count()
            == 2
    });
    let fresh = calls
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.operation == "start" && c.id != old.id)
        .unwrap()
        .clone();
    assert_eq!(
        old.scope, fresh.scope,
        "test must exercise identical numeric scope across cores"
    );
    assert_ne!(
        old.id.split('-').nth(1),
        fresh.id.split('-').nth(1),
        "query core nonces must differ despite stable token"
    );
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.operation == "cancel" && c.id == old.id)
            .count(),
        1
    );
    let heartbeat = counters_snapshot().heartbeats_sent;
    wait_until(|| counters_snapshot().heartbeats_sent > heartbeat);
    assert!(
        resident.take_query_results().is_empty(),
        "worker must consume the old logical result"
    );
    assert!(
        !fresh_http.is_finished(),
        "old result must not satisfy the fresh request"
    );
    complete.store(true, Ordering::Release);
    pump_until(&resident, &wake_rx, || fresh_http.is_finished());
    let response = fresh_http.join().unwrap().unwrap();
    assert_eq!(response["retcode"], 0);
    assert_eq!(response["echo"], "core-restart-lab");
    assert_eq!(
        response["data"],
        json!({"user_id":10001,"nickname":"Fresh after core restart"})
    );
    let mut control = None;
    wait_until(|| {
        control = ControlClient::connect(
            second.ready["control_pipe"].as_str().unwrap(),
            second.ready["auth_token"].as_str().unwrap(),
            "restart-stop",
        )
        .ok();
        control.is_some()
    });
    control.unwrap().request(ControlMsg::Stop {}).unwrap();
    let (counts, exit) = exit_rx.recv_timeout(Duration::from_secs(8)).unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    worker.handle.take().unwrap().join().unwrap();
    second.wait_clean();
    assert_eq!(counts.bootstraps_attempted, 0);
    assert_eq!(counts.dispatches, 0);
    assert_eq!(counts.hello_rejected, 0);
    assert_eq!(counts.protocol_errors, 0);
    assert_eq!(resident.counters().bootstraps, 1);
    let recorded = calls.lock().unwrap();
    assert_eq!(
        recorded
            .iter()
            .filter(|c| c.id == old.id && c.operation == "start")
            .count(),
        1
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|c| c.id == old.id && c.operation == "poll")
            .count(),
        0
    );
    let summary = json!({
        "refresh": refresh, "first_core_pid": first.child.id(), "second_core_pid": second.child.id(),
        "old_query_id": old.id, "fresh_query_id": fresh.id, "old_scope": old.scope, "fresh_scope": fresh.scope,
        "stable_token": true, "stable_pipe": true, "first_http_interrupted": true,
        "old_query_polls": 0, "owner_cancellations": 1, "fresh_response": response,
        "hellos_accepted": counts.hellos_accepted, "reconnects": counts.reconnects,
        "worker_bootstraps_attempted": counts.bootstraps_attempted, "owner_bootstraps": resident.counters().bootstraps,
        "dispatches": counts.dispatches, "protocol_errors": counts.protocol_errors,
        "worker_exit": "core_stopped", "second_core_clean_exit": true
    });
    let evidence = dir.join("summary.json");
    std::fs::write(&evidence, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
    println!("core-restart-evidence={}", evidence.display());
    println!("refresh={refresh}: two real core PIDs, equal scope epoch={}, distinct query nonces, one owner cancellation, fresh HTTP success; no send/rebootstrap", old.scope.connection_epoch);
}
#[test]
fn login_query_survives_two_core_processes_without_resuming_the_old_request() {
    run_case(false);
}
#[test]
fn refreshed_self_query_survives_two_core_processes_without_resuming_the_old_request() {
    run_case(true);
}
