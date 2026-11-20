//! K4-D7b daemon_client LAB 集成测试。
//!
//! 真实 caligod daemon(core)+ 真实 bridge worker(管道客户端)对接:
//! - 连接失败 → 退避重试 → 服务端就绪后接入(重连计数,**零 bootstrap 调用**);
//! - Dispatch 全链:NativeStarted → resident 提交 → owner drain → SendResult
//!   → actor 终态关联原生 ID → 控制面可查;
//! - Stopped → worker 干净退出。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use caligo_bridge::daemon_client::{run_worker, DaemonClientConfig, WorkerExit};
use caligo_bridge::host_adapter::{HostAdapter, HostOp, HostOpResult};
use caligo_bridge::resident::{Resident, ResidentLimits};

use caligo_core::daemon::{BridgeClient, ControlClient, Daemon, DaemonConfig};
use caligo_core::ipc::CoreToControlMsg;
use caligo_core::runtime::RuntimeConfig;

// ---- LAB 假宿主(owner = 测试线程;Sent 返回递增原生 ID) ----

static LAB_SENDS: AtomicU64 = AtomicU64::new(0);

/// 两用例共享进程级全局计数器与计时敏感的管道时序:串行执行消除
/// 并行互扰(单测内语义已各自覆盖;串行不减弱断言)。
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct LabHost;

impl HostAdapter for LabHost {
    fn owner_thread_id(&self) -> u64 {
        lab_tid()
    }
    fn current_thread_id(&self) -> u64 {
        lab_tid()
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(
        &mut self,
        op: HostOp,
    ) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
        Ok(match op {
            HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
            HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
            HostOp::SendVoice { .. } | HostOp::SendImage { .. } => panic!("fixture does not send voice"),
            HostOp::Probe => HostOpResult::ProbeDone,
            HostOp::SendText { .. } => {
                let n = LAB_SENDS.fetch_add(1, Ordering::Relaxed) + 1;
                HostOpResult::Sent {
                    native_id: Some(format!("NM-{n}")),
                }
            }
        })
    }
}

fn lab_tid() -> u64 {
    // SAFETY: 无副作用。
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
}

#[test]
fn login_query_runs_on_owner_and_revokes_a_stale_epoch() {
    run_self_query_case(false);
}

#[test]
fn self_profile_query_runs_on_owner_and_revokes_a_stale_epoch() {
    run_self_query_case(true);
}

fn run_self_query_case(refresh: bool) {
    use caligo_model::capability::{HostQuery, LoginInfo, QueryCapability, QueryData, QueryOutcome, QueryScope};

    let _serial = TEST_SERIAL.lock().unwrap();
    struct LoginHost {
        owner: u64,
        refresh: bool,
        polls: Arc<AtomicU64>,
        cancelled: Arc<AtomicBool>,
    }
    impl HostAdapter for LoginHost {
        fn owner_thread_id(&self) -> u64 { self.owner }
        fn current_thread_id(&self) -> u64 { lab_tid() }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
            LabHost.native_op(op)
        }
        fn query_capabilities(&self) -> &'static [QueryCapability] { &[QueryCapability::LoginInfo, QueryCapability::SelfProfile] }
        fn query(&mut self, _: &str, scope: &QueryScope, query: &HostQuery, _: u64, poll: bool) -> Option<QueryOutcome> {
            assert_eq!(lab_tid(), self.owner);
            assert_eq!(scope.account, "10001");
            assert_eq!(scope.host_nonce, 0xAA00_0001);
            assert_eq!(scope.session_generation, 1);
            assert_eq!(*query, if self.refresh {HostQuery::SelfProfile} else {HostQuery::LoginInfo});
            if !poll { return None; }
            self.polls.fetch_add(1, Ordering::Relaxed);
            let info = LoginInfo {
                user_id: scope.account.clone(),
                nickname: "Synthetic host nickname".into(),
                online: true,
            };
            Some(QueryOutcome::Success(if self.refresh {QueryData::SelfProfile(info)} else {QueryData::LoginInfo(info)}))
        }
        fn cancel_query(&mut self, _: &str, _: &QueryScope) {
            assert_eq!(lab_tid(), self.owner);
            self.cancelled.store(true, Ordering::Release);
        }
    }
    let prefix = unique_prefix("login");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let polls = Arc::new(AtomicU64::new(0));
    let cancelled = Arc::new(AtomicBool::new(false));
    let resident = Arc::new(Resident::new(
        LoginHost { owner: lab_tid(), refresh, polls: polls.clone(), cancelled: cancelled.clone() },
        ResidentLimits::default(),
    ));
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let cfg = worker_config(&prefix, &auth);
    let worker_resident = resident.clone();
    let worker_stop = stop.clone();
    let (wake_tx, wake_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        run_worker(cfg, worker_resident, &|| wake_tx.send(()).is_ok(), _rx_placeholder(),
            _ev_placeholder(), worker_stop)
    });
    let handler = caligo_core::onebot_server::action_handler(daemon.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    let response = loop {
        let handler = handler.clone();
        let action = std::thread::spawn(move || handler("get_login_info", &serde_json::json!({"no_cache":refresh}), serde_json::Value::Null));
        while !action.is_finished() {
            assert!(Instant::now() < deadline, "login query did not finish");
            if wake_rx.recv_timeout(Duration::from_millis(25)).is_ok() {
                resident.drain_queries();
            }
        }
        let response = action.join().unwrap();
        if response.retcode == 0 { break response; }
        assert!(Instant::now() < deadline, "no advertised login query");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(response.data, serde_json::json!({"user_id": 10001, "nickname": "Synthetic host nickname"}));
    assert_eq!(polls.load(Ordering::Relaxed), 1);
    let mut control = ControlClient::connect(&pipe_names_from_prefix(&prefix).1, &auth, "login-lab").unwrap();
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    assert_eq!(worker.join().unwrap().1, WorkerExit::CoreStopped);
    runner.join().unwrap().unwrap();

    // Exercise a request already dispatched before connection revocation.
    let stale = QueryScope { account: "10001".into(), host_nonce: 0xAA00_0001, session_generation: 1, connection_epoch: 100 };
    resident.set_bridge_scope(Some(stale.clone()));
    resident.submit_query("stale".into(), stale.clone(), 1000, if refresh {HostQuery::SelfProfile} else {HostQuery::LoginInfo}).unwrap();
    assert_eq!(resident.drain_queries(), 1);
    resident.set_bridge_scope(None);
    assert_eq!(resident.drain_queries(), 1);
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(polls.load(Ordering::Relaxed), 1);
    assert_eq!(resident.take_query_results(), vec![("stale".into(), stale.clone(), QueryOutcome::Disconnected)]);
    assert_eq!(resident.submit_query("replay".into(), stale, 1000, if refresh {HostQuery::SelfProfile} else {HostQuery::LoginInfo}), Err(QueryOutcome::Disconnected));
}

fn unique_prefix(tag: &str) -> String {
    format!(
        "\\\\.\\pipe\\caligo-k5-dcl-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

/// start 不创建管道(run 才创建):重连用例借此制造"服务端未就绪"窗口。
fn start_daemon(
    prefix: &str,
) -> (
    Arc<Daemon>,
    impl FnOnce() -> std::thread::JoinHandle<Result<(), caligo_core::daemon::SessionError>>,
) {
    start_daemon_mode(prefix, false)
}

/// `fault_break_after_hello = true` → LAB 故障注入(见 DaemonConfig 字段文档)。
fn start_daemon_mode(
    prefix: &str,
    fault_break_after_hello: bool,
) -> (
    Arc<Daemon>,
    impl FnOnce() -> std::thread::JoinHandle<Result<(), caligo_core::daemon::SessionError>>,
) {
    start_daemon_faults(prefix, fault_break_after_hello, false)
}

fn start_daemon_faults(
    prefix: &str,
    fault_break_after_hello: bool,
    fault_break_after_result: bool,
) -> (
    Arc<Daemon>,
    impl FnOnce() -> std::thread::JoinHandle<Result<(), caligo_core::daemon::SessionError>>,
) {
    // Neither a reused Windows PID nor another fixture may recover this
    // test's request IDs from an unrelated journal.
    static JOURNAL_SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "caligo-dcl-{}-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        JOURNAL_SEQ.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let config = DaemonConfig {
        pipe_prefix: prefix.to_string(),
        journal_path: dir.join("journal.log"),
        module_baseline: "qq-9.9.33-52230".into(),
        account: "10001".into(),
        core_build: caligo_core::CORE_BUILD.into(),
        expect_client_pid: 0, // LAB worker 与测试同进程;PID 校验在此禁用
        runtime: RuntimeConfig::default(),
        test_fault_break_bridge_after_hello: fault_break_after_hello,
        test_fault_break_bridge_after_result: fault_break_after_result,
    };
    {
        let probe = dir.join("probe-write.txt");
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&probe)
        {
            Ok(_) => eprintln!("[dbg] probe open OK: {}", probe.display()),
            Err(e) => eprintln!("[dbg] probe open FAILED: {e}; path={}", probe.display()),
        }
    }
    let journal_dbg = config.journal_path.clone();
    let daemon = match Daemon::start(config) {
        Ok(d) => d,
        Err(e) => panic!(
            "daemon start failed: {e}; dir={:?} exists={} journal={:?}",
            dir,
            dir.exists(),
            journal_dbg
        ),
    };
    (daemon.clone(), move || {
        std::thread::spawn(move || daemon.run())
    })
}

fn worker_config(prefix: &str, auth: &str) -> DaemonClientConfig {
    worker_config_host(prefix, auth, 0xAA00_0001)
}

/// `host_nonce` 可注入:宿主身份判定(T05)用不同 nonce 构造不同宿主。
fn worker_config_host(prefix: &str, auth: &str, host_nonce: u64) -> DaemonClientConfig {
    DaemonClientConfig {
        pipe_name: format!("{prefix}-bridge"),
        auth_token: auth.into(),
        bridge_build: "lab-bridge 0.1.0".into(),
        session_generation: 1,
        account: "10001".into(),
        module_baseline: "qq-9.9.33-52230".into(),
        host_nonce,
        host_process_created_utc: "2026-10-08T00:00:00Z".into(),
        self_uid: String::new(),
        heartbeat_ms: 500,
        reconnect_backoff_ms: 50,
    }
}

#[test]
fn reconnect_then_hello_then_clean_stop_without_bootstrap() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("reconnect");
    // daemon start(journal 就绪,管道未创建)→ worker 先行 → 服务端延迟就绪。
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(
                cfg,
                resident,
                &wake_false,
                _rx_placeholder(),
                _ev_placeholder(),
                stop,
            );
            w_tx.send(r).unwrap();
        });
    }
    // 服务端 300ms 后就绪(worker 此间经历连接失败/退避重试)。
    std::thread::sleep(Duration::from_millis(300));
    let runner = make_runner();

    let deadline = Instant::now() + Duration::from_secs(5);
    // worker 计数在退出时才返回;接入成功以 control 可用为证。
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    while Instant::now() < deadline {
        match ControlClient::connect(&control_name, &auth, "lab-client") {
            Ok(c) => {
                control = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut control = control.expect("control connected(worker 已接入)");

    // Health 往返(worker 心跳在跑,daemon 活着)。
    let r = control
        .request(caligo_core::ipc::ControlMsg::Health {})
        .unwrap();
    assert!(matches!(r, CoreToControlMsg::HealthAck {}));

    // 高负载下 worker 首连可能晚于 control:Stop 前等 worker 至少接入一次,
    // 否则 daemon 停止后 worker 永远退避重连、测试超时(预存在 flake)。
    let dl = Instant::now() + Duration::from_secs(10);
    while caligo_bridge::daemon_client::counters_snapshot().connects == 0 {
        assert!(Instant::now() < dl, "worker 未在窗口内完成首连");
        std::thread::sleep(Duration::from_millis(50));
    }

    // Stop:daemon 真实停止协议 → worker 收 Stopped → 干净退出。
    let r = control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    assert!(matches!(r, CoreToControlMsg::Stopped { .. }));
    let (counters, exit) = match w_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(v) => v,
        Err(_) => {
            let snap = caligo_bridge::daemon_client::counters_snapshot();
            panic!("worker exit timeout; snapshot={snap:?}");
        }
    };
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert!(counters.hellos_accepted >= 1, "至少一次接入成功");
    assert!(
        counters.connect_failures + counters.reconnects >= 1,
        "服务端未就绪期应有失败/重连"
    );
    assert_eq!(
        counters.bootstraps_attempted, 0,
        "重连绝不重新 bootstrap —— 核心判据"
    );
}

#[test]
fn dispatch_roundtrip_through_real_worker() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("dispatch");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    // 真实流程中由 caligo_qq_daemon_client_start 调用;LAB 显式执行
    // (owner = 测试线程,与 drain 一致)。
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<caligo_bridge::resident::RetainedNotice>();
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_true, rx, _ev_placeholder(), stop);
            w_tx.send(r).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match ControlClient::connect(&control_name, &auth, "lab-client") {
            Ok(c) => {
                control = Some(c);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut control = control.expect("control connected");

    // 控制面提交发送 → daemon Dispatch → worker 提交 resident。
    // 会话绑定由 worker 的 Hello 完成;等待 SendText 被受理(worker 接入前
    // daemon 尚无 session,重试窗口内受理即成功)。
    let mut accepted = false;
    let acc_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < acc_deadline {
        let r = control
            .request(caligo_core::ipc::ControlMsg::SendText {
                request_id: "R1".into(),
                target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
                text: "CALIGO-K5-DCL-001".into(),
                deadline_ms: 30_000,
            })
            .unwrap();
        if matches!(r, CoreToControlMsg::SendAccepted { .. }) {
            accepted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(accepted, "SendText 未在窗口内被受理");

    // 测试线程 = owner:轮询提交 → drain → 结果回传(模拟 drain 钩子接线)。
    let round_deadline = Instant::now() + Duration::from_secs(5);
    let mut confirmed = false;
    while Instant::now() < round_deadline {
        // resident.drain 只能在 owner 线程(LAB 假宿主以本线程为 owner)。
        let _ = resident.drain();
        caligo_bridge::resident::ResultHandoffGate::default().forward(&resident, &tx).unwrap();
        if let CoreToControlMsg::QueryResult {
            state, native_id, ..
        } = control
            .request(caligo_core::ipc::ControlMsg::QueryRequest {
                request_id: "R1".into(),
            })
            .unwrap()
        {
            if native_id.as_deref() == Some("NM-1")
                || state
                    .as_deref()
                    .map(|s| s.contains("ConfirmedSuccess"))
                    .unwrap_or(false)
            {
                confirmed = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        confirmed,
        "R1 未在窗口内确认成功(NM-1); snapshot={:?}",
        caligo_bridge::daemon_client::counters_snapshot()
    );

    // 收尾:Stop → worker CoreStopped。
    control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    let (counters, exit) = match w_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(v) => v,
        Err(_) => {
            let snap = caligo_bridge::daemon_client::counters_snapshot();
            panic!("worker exit timeout; snapshot={snap:?}");
        }
    };
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert!(counters.dispatches >= 1);
    assert!(counters.results_sent >= 1);
    assert_eq!(counters.bootstraps_attempted, 0);
}

#[test]
fn manager_shutdown_before_inflight_notice_ack_preserves_bridge_reservations() {
    use caligo_bridge::resident::ResultHandoffGate;
    use caligo_core::account_manager::{AccountManager, AccountSpec, ManagerConfig, PortConfig};
    use caligo_core::ipc::ControlMsg;
    use caligo_core::journal::{Journal, JournalRecord, SyncMode};

    struct PendingHost(Arc<AtomicU64>);
    impl HostAdapter for PendingHost {
        fn owner_thread_id(&self) -> u64 { lab_tid() }
        fn current_thread_id(&self) -> u64 { lab_tid() }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(
            &mut self,
            op: HostOp,
        ) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
            Ok(match op {
                HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
                HostOp::SendText { request_id, .. } => {
                    self.0.fetch_add(1, Ordering::Relaxed);
                    HostOpResult::SendFired { mid: request_id }
                }
                _ => panic!("unexpected fake host operation"),
            })
        }
    }

    let _serial = TEST_SERIAL.lock().unwrap();
    let wait_until = |predicate: &mut dyn FnMut() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline, "fixture condition was not met");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let scenarios = [
        (false, false),
        (true, false),
        #[cfg(feature = "research")]
        (false, true),
    ];
    for (settle_before_stop, staged_close) in scenarios {
        let tag = format!("caligo-inflight-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let root = std::env::temp_dir().join(&tag);
        let config = ManagerConfig {
            version: 1,
            accounts: vec![AccountSpec {
                account: 10001, data_dir: "A".into(), pipe_prefix: tag.clone(),
                module_baseline: "qq-9.9.33-52230".into(),
                expect_client_pid: std::process::id(), ports: PortConfig::default(),
            }],
        };
        config.save_atomic(&root).unwrap();
        let mut manager = AccountManager::new(&root, config).unwrap();
        let daemon = manager.start(10001).unwrap();
        let auth = daemon.auth_token();
        let sends = Arc::new(AtomicU64::new(0));
        let resident = Arc::new(Resident::new(PendingHost(sends.clone()), ResidentLimits::default()));
        resident.bootstrap().unwrap();
        let worker_stop = Arc::new(AtomicBool::new(false));
        let (notice_tx, notice_rx) = mpsc::channel();
        let (worker_done_tx, worker_done_rx) = mpsc::channel();
        let worker = {
            let cfg = worker_config(&tag, &auth);
            let resident = resident.clone();
            let stop = worker_stop.clone();
            std::thread::spawn(move || {
                let result = run_worker(cfg, resident, &wake_true, notice_rx, _ev_placeholder(), stop);
                let _ = worker_done_tx.send(result);
            })
        };

        let mut control = None;
        wait_until(&mut || {
            if daemon.connection_epoch() < 2 || caligo_bridge::daemon_client::counters_snapshot().hellos_accepted == 0 {
                return false;
            }
            control = ControlClient::connect(&daemon.pipe_names().1, &auth, "inflight-stop-lab").ok();
            control.is_some()
        });
        let mut control = control.unwrap();
        let request_id = "inflight-stop";
        let accepted = control.request(ControlMsg::SendText {
            request_id: request_id.into(),
            target: serde_json::json!({"account":"10001","kind":"private","peer":"20002"}),
            text: "synthetic stop order".into(),
            deadline_ms: 30_000,
        }).unwrap();
        assert!(matches!(accepted, CoreToControlMsg::SendAccepted { .. }), "fixture dispatch rejected: {accepted:?}");
        wait_until(&mut || resident.pending_ingress() == 1);
        resident.drain().unwrap();
        assert_eq!(resident.request_state(request_id), Some("send_pending"));
        assert_eq!(sends.load(Ordering::Relaxed), 1);

        if settle_before_stop {
            assert_eq!(resident.complete_sends(&[(request_id.into(), "synthetic-native".into())], &[], &[]), vec![request_id]);
            ResultHandoffGate::default().forward(&resident, &notice_tx).unwrap();
            wait_until(&mut || !resident.has_unacked_results());
        }
        #[cfg(feature = "research")]
        if staged_close {
            assert_eq!(resident.close_for_result_drain().unwrap().marked_unknown, 1);
            assert_eq!(ResultHandoffGate::default().forward(&resident, &notice_tx), Ok(2));
            wait_until(&mut || !resident.has_unacked_results());
            assert_eq!(resident.request_state(request_id), Some("delivery_unknown"));
        }
        drop(control);
        manager.shutdown().unwrap();
        worker_stop.store(true, Ordering::Release);
        let (worker_counts, exit) = worker_done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        assert!(matches!(exit, WorkerExit::CoreStopped | WorkerExit::LocalStop));
        caligo_bridge::qq_entry::close_daemon_owner(&resident);
        let unacked = resident.has_unacked_results();
        assert_eq!(unacked, !settle_before_stop && !staged_close);
        assert_eq!(sends.load(Ordering::Relaxed), 1);
        assert_eq!(worker_counts.results_sent, u64::from(settle_before_stop || staged_close));

        drop(daemon);
        let (_, history) = Journal::open(root.join("A/journal.jsonl"), 1024 * 1024, SyncMode::Always).unwrap();
        let states: Vec<_> = history.records.iter().filter_map(|record| match record {
            JournalRecord::Request(request) if request.request_id == request_id => Some(request.state.as_str()),
            _ => None,
        }).collect();
        let expected: &[&str] = if settle_before_stop {
            &["Queued", "DispatchIntent", "NativeStarted", "ConfirmedSuccess"]
        } else if staged_close {
            &["Queued", "DispatchIntent", "NativeStarted", "DeliveryUnknown"]
        } else {
            &["Queued", "DispatchIntent", "DeliveryUnknown"]
        };
        assert_eq!(states, expected);
        println!("normal result ACK={settle_before_stop}, staged close={staged_close}, worker={exit:?}, unacked={unacked}, journal={states:?}, native calls=1");
    }
}

/// Native invocation must not be repeated when the core durably applies the
/// receipt and drops the pipe before its ACK reaches the bridge.
#[test]
fn durable_result_is_replayed_after_ack_loss_without_native_retry() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("resultack");
    let (daemon, make_runner) = start_daemon_faults(&prefix, false, true);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<caligo_bridge::resident::RetainedNotice>();
    let (w_tx, w_rx) = mpsc::channel();
    let send_count_before = LAB_SENDS.load(Ordering::Relaxed);
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_true, rx, _ev_placeholder(), stop);
            w_tx.send(r).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut control = None;
    while Instant::now() < deadline {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "lab-client") {
            control = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut control = control.expect("control connected");

    let mut accepted = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(
            control
                .request(caligo_core::ipc::ControlMsg::SendText {
                    request_id: "R-ACK-LOSS".into(),
                    target: serde_json::json!({"account":"10001","kind":"group","peer":"654321"}),
                    text: "CALIGO-RESULT-ACK-LOSS".into(),
                    deadline_ms: 30_000,
                })
                .unwrap(),
            CoreToControlMsg::SendAccepted { .. }
        ) {
            accepted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        accepted,
        "send request must be accepted before recovery check"
    );

    let expected_native_id = format!("NM-{}", send_count_before + 1);
    let handoff = caligo_bridge::resident::ResultHandoffGate::default();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut durable_result_visible = false;
    while Instant::now() < deadline {
        let _ = resident.drain();
        assert!(handoff.forward(&resident, &tx).is_ok());
        if let CoreToControlMsg::QueryResult { native_id, .. } = control
            .request(caligo_core::ipc::ControlMsg::QueryRequest {
                request_id: "R-ACK-LOSS".into(),
            })
            .unwrap()
        {
            if native_id.as_deref() == Some(expected_native_id.as_str()) {
                durable_result_visible = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        durable_result_visible,
        "journaled native receipt must remain queryable"
    );

    let deadline = Instant::now() + Duration::from_secs(8);
    let mut replay_observed = false;
    while Instant::now() < deadline {
        let counters = caligo_bridge::daemon_client::counters_snapshot();
        if counters.reconnects >= 1 && counters.results_sent >= 2 {
            replay_observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        replay_observed,
        "unacked durable result must be replayed after reconnect"
    );
    assert_eq!(
        LAB_SENDS.load(Ordering::Relaxed) - send_count_before,
        1,
        "recovery must replay only the receipt and never call native twice"
    );

    control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    let (counters, exit) = w_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("worker must stop after recovery");
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert!(counters.reconnects >= 1);
    assert!(counters.results_sent >= 2);
}

/// Rebuild a worker after the core has persisted a result but its ACK was lost.
#[test]
fn worker_rebuild_replays_durable_result_after_lost_ack_without_native_retry() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("rebuild-ack-loss");
    let (daemon, make_runner) = start_daemon_faults(&prefix, false, true);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<caligo_bridge::resident::RetainedNotice>();
    let (w_tx, w_rx) = mpsc::channel();
    let send_count_before = LAB_SENDS.load(Ordering::Relaxed);
    {
        let mut cfg = worker_config(&prefix, &auth);
        cfg.reconnect_backoff_ms = 2_000;
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_true, rx, _ev_placeholder(), stop);
            w_tx.send(r).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut control = None;
    while Instant::now() < deadline {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "lab-client") {
            control = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut control = control.expect("control connected");

    let mut accepted = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(
            control
                .request(caligo_core::ipc::ControlMsg::SendText {
                    request_id: "R-REBUILD-ACK-LOSS".into(),
                    target: serde_json::json!({"account":"10001","kind":"group","peer":"654321"}),
                    text: "CALIGO-REBUILD-ACK-LOSS".into(),
                    deadline_ms: 30_000,
                })
                .unwrap(),
            CoreToControlMsg::SendAccepted { .. }
        ) {
            accepted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        accepted,
        "send request must be accepted before recovery check"
    );

    let expected_native_id = format!("NM-{}", send_count_before + 1);
    let handoff = caligo_bridge::resident::ResultHandoffGate::default();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut durable_result_visible = false;
    while Instant::now() < deadline {
        let _ = resident.drain();
        assert!(handoff.forward(&resident, &tx).is_ok());
        if let CoreToControlMsg::QueryResult { native_id, .. } = control
            .request(caligo_core::ipc::ControlMsg::QueryRequest {
                request_id: "R-REBUILD-ACK-LOSS".into(),
            })
            .unwrap()
        {
            if native_id.as_deref() == Some(expected_native_id.as_str()) {
                durable_result_visible = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        durable_result_visible,
        "journaled native receipt must remain queryable"
    );

    // The daemon fault closes the connection after journal persistence and
    // before the result ACK. Stop during the deliberately long backoff.
    assert!(!resident.retained_notices().is_empty(), "lost ACK must leave resident ownership");
    // Allow the injected break to reach the worker before requesting stop;
    // reconnect backoff is 2 seconds, longer than this observation interval.
    std::thread::sleep(Duration::from_millis(700));
    stop.store(true, Ordering::Relaxed);
    let (first_counters, exit) = w_rx.recv_timeout(Duration::from_secs(5)).expect("first worker must stop");
    assert_eq!(exit, WorkerExit::LocalStop);
    assert!(first_counters.reconnects >= 1, "fault must break the first connection before stop");
    assert_eq!(first_counters.connects, 1, "first worker must not reconnect before replacement");
    let retained = resident.retained_notices();
    assert!(retained.iter().any(|item| matches!(&item.notice,
        caligo_bridge::resident::ResidentNotice::Outcome(caligo_bridge::resident::SendOutcome::Success { .. }))),
        "durable success must still be retained after lost ACK");
    drop(tx);

    let (replacement_tx, replacement_rx) = mpsc::channel();
    for item in &retained { replacement_tx.send(item.clone()).unwrap(); }
    let (done2, wait2) = mpsc::channel();
    let stop2 = Arc::new(AtomicBool::new(false));
    {
        let cfg = worker_config(&prefix, &auth); let resident = resident.clone();
        std::thread::spawn(move || {
            done2.send(run_worker(cfg, resident, &wake_true, replacement_rx, _ev_placeholder(), stop2)).unwrap();
        });
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: "R-REBUILD-ACK-LOSS".into() }).unwrap();
        if resident.retained_notices().is_empty()
            && matches!(result, CoreToControlMsg::QueryResult { native_id: Some(ref id), .. } if id == &expected_native_id) { break; }
        assert!(Instant::now() < deadline, "replacement did not replay and release the durable receipt");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(LAB_SENDS.load(Ordering::Relaxed) - send_count_before, 1,
        "rebuilding the worker must never resend to native");
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    let (counters, exit) = wait2.recv_timeout(Duration::from_secs(5)).expect("replacement must stop");
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert_eq!(counters.results_sent, 1, "duplicate hints must not send a second receipt");
    runner.join().unwrap().unwrap();
}

/// Reconstruct a worker after its old channel accepted notices and vanished.
#[test]
fn worker_rebuild_replays_resident_notices_without_native_retry() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("worker-rebuild");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let send_count_before = LAB_SENDS.load(Ordering::Relaxed);
    let stop1 = Arc::new(AtomicBool::new(false));
    let (_tx1, rx1) = mpsc::channel();
    let (done1, wait1) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone(); let stop = stop1.clone();
        std::thread::spawn(move || {
            done1.send(run_worker(cfg, resident, &wake_true, rx1, _ev_placeholder(), stop)).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut control = loop {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "rebuild-client") { break c; }
        assert!(Instant::now() < deadline, "control connection timeout");
        std::thread::sleep(Duration::from_millis(20));
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(control.request(caligo_core::ipc::ControlMsg::SendText {
            request_id: "R-REBUILD".into(),
            target: serde_json::json!({"account":"10001","kind":"group","peer":"654321"}),
            text: "CALIGO-REBUILD-LAB".into(), deadline_ms: 30_000,
        }).unwrap(), CoreToControlMsg::SendAccepted { .. }) { break; }
        assert!(Instant::now() < deadline, "dispatch acceptance timeout");
        std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while resident.request_state("R-REBUILD") != Some("queued") {
        assert!(Instant::now() < deadline, "resident queue timeout");
        std::thread::sleep(Duration::from_millis(20));
    }
    stop1.store(true, Ordering::Relaxed);
    assert_eq!(wait1.recv_timeout(Duration::from_secs(5)).unwrap().1, WorkerExit::LocalStop);
    assert_eq!(resident.drain().unwrap(), 1);
    let (lost_tx, lost_rx) = mpsc::channel();
    assert_eq!(caligo_bridge::resident::ResultHandoffGate::default().forward(&resident, &lost_tx), Ok(2));
    let retained = resident.retained_notices();
    assert_eq!(retained.len(), 2);
    drop(lost_rx); // simulate successful send then loss before worker receives
    assert_eq!(resident.retained_notices(), retained);
    let stop2 = Arc::new(AtomicBool::new(false));
    let (tx2, rx2) = mpsc::channel();
    // Duplicate hints must not duplicate notices already in the startup snapshot.
    for item in &retained { tx2.send(item.clone()).unwrap(); }
    let (done2, wait2) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone(); let stop = stop2.clone();
        std::thread::spawn(move || {
            done2.send(run_worker(cfg, resident, &wake_true, rx2, _ev_placeholder(), stop)).unwrap();
        });
    }
    let expected_id = format!("NM-{}", send_count_before + 1);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: "R-REBUILD".into() }).unwrap();
        if matches!(result, CoreToControlMsg::QueryResult { native_id: Some(ref id), .. } if id == &expected_id)
            && resident.retained_notices().is_empty() { break; }
        assert!(Instant::now() < deadline, "rebuilt worker did not persist and acknowledge result");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(LAB_SENDS.load(Ordering::Relaxed) - send_count_before, 1);
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    let (counters, exit) = wait2.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert_eq!(counters.results_sent, 1, "duplicate channel hints must not send a second result");
    runner.join().unwrap().unwrap();
}

/// Real daemon ACKs free bounded resident capacity and unblock the pending suffix.
#[test]
fn daemon_ack_releases_backpressure_without_repeating_native_send() {
    use caligo_bridge::resident::{HandoffError, OwnedRequest, ResultHandoffGate, SubmitVerdict};
    struct AsyncLabHost;
    impl HostAdapter for AsyncLabHost {
        fn owner_thread_id(&self) -> u64 { lab_tid() }
        fn current_thread_id(&self) -> u64 { lab_tid() }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
            match op {
                HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
                HostOp::Probe => Ok(HostOpResult::ProbeDone),
                HostOp::SendText { request_id, .. } => {
                    LAB_SENDS.fetch_add(1, Ordering::Relaxed);
                    Ok(HostOpResult::SendFired { mid: request_id })
                }
                _ => panic!("unexpected operation"),
            }
        }
    }
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("backpressure");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string(); let runner = make_runner();
    let resident = Arc::new(Resident::new(AsyncLabHost, ResidentLimits { results_max: 2, ..ResidentLimits::default() }));
    resident.bootstrap().unwrap();
    let before = LAB_SENDS.load(Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let (worker_tx, worker_rx) = mpsc::channel();
    let (done, wait) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth); let resident = resident.clone(); let stop = stop.clone();
        std::thread::spawn(move || { done.send(run_worker(cfg, resident, &wake_true, worker_rx, _ev_placeholder(), stop)).unwrap(); });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut control = loop {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "pressure-client") { break c; }
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(20));
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(control.request(caligo_core::ipc::ControlMsg::SendText {
            request_id: "R-PRESSURE".into(),
            target: serde_json::json!({"account":"10001","kind":"group","peer":"654321"}),
            text: "CALIGO-PRESSURE-LAB".into(), deadline_ms: 30_000,
        }).unwrap(), CoreToControlMsg::SendAccepted { .. }) { break; }
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while resident.request_state("R-PRESSURE") != Some("queued") {
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(resident.drain().unwrap(), 1);
    resident.complete_sends(&[("R-PRESSURE".into(), String::new())], &[], &[]);
    resident.complete_sends(&[], &[], &[("R-PRESSURE".into(), "unknown".into())]);
    resident.complete_sends(&[("R-PRESSURE".into(), "strong-id".into())], &[], &[]);
    let (staged_tx, staged_rx) = mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&resident, &staged_tx), Err(HandoffError::Full(2)));
    assert_eq!(resident.submit(OwnedRequest::Probe { id: 7 }), SubmitVerdict::Accepted);
    assert_eq!(gate.forward(&resident, &staged_tx), Err(HandoffError::Full(0)));
    assert_eq!(resident.pending_ingress(), 1);
    for item in staged_rx.try_iter() { worker_tx.send(item).unwrap(); }
    let deadline = Instant::now() + Duration::from_secs(10); let mut probe_ran = false;
    loop {
        let ready = gate.forward(&resident, &staged_tx).is_ok();
        for item in staged_rx.try_iter() { worker_tx.send(item).unwrap(); }
        if ready && !probe_ran { assert_eq!(resident.drain().unwrap(), 1); probe_ran = true; }
        let result = control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: "R-PRESSURE".into() }).unwrap();
        if probe_ran && resident.retained_notices().is_empty()
            && matches!(result, CoreToControlMsg::QueryResult { native_id: Some(ref id), .. } if id == "strong-id") { break; }
        assert!(Instant::now() < deadline, "ACKs did not drain pressure and pending suffix");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(LAB_SENDS.load(Ordering::Relaxed) - before, 1);
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    assert_eq!(wait.recv_timeout(Duration::from_secs(5)).unwrap().1, WorkerExit::CoreStopped);
    runner.join().unwrap().unwrap();
}

/// Rebuild after full retention and a lost channel; drain the suffix without resending.
#[test]
fn worker_rebuild_recovers_full_retention_and_pending_suffix_without_resend() {
    use caligo_bridge::resident::{HandoffError, OwnedRequest, ResultHandoffGate, SubmitVerdict};
    struct AsyncLabHost;
    impl HostAdapter for AsyncLabHost {
        fn owner_thread_id(&self) -> u64 { lab_tid() }
        fn current_thread_id(&self) -> u64 { lab_tid() }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
            match op {
                HostOp::ListenerAdd => Ok(HostOpResult::ListenerDeferred),
                HostOp::Probe => Ok(HostOpResult::ProbeDone),
                HostOp::SendText { request_id, .. } => {
                    LAB_SENDS.fetch_add(1, Ordering::Relaxed);
                    Ok(HostOpResult::SendFired { mid: request_id })
                }
                _ => panic!("unexpected operation"),
            }
        }
    }
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("rebuild-full");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string(); let runner = make_runner();
    let resident = Arc::new(Resident::new(AsyncLabHost, ResidentLimits { results_max: 2, ..ResidentLimits::default() }));
    resident.bootstrap().unwrap();
    let before = LAB_SENDS.load(Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let (worker_tx, worker_rx) = mpsc::channel();
    let (done, wait) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth); let resident = resident.clone(); let stop = stop.clone();
        std::thread::spawn(move || { done.send(run_worker(cfg, resident, &wake_true, worker_rx, _ev_placeholder(), stop)).unwrap(); });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut control = loop {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "pressure-client") { break c; }
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(20));
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(control.request(caligo_core::ipc::ControlMsg::SendText {
            request_id: "R-PRESSURE".into(),
            target: serde_json::json!({"account":"10001","kind":"group","peer":"654321"}),
            text: "CALIGO-PRESSURE-LAB".into(), deadline_ms: 30_000,
        }).unwrap(), CoreToControlMsg::SendAccepted { .. }) { break; }
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while resident.request_state("R-PRESSURE") != Some("queued") {
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(resident.drain().unwrap(), 1);
    resident.complete_sends(&[("R-PRESSURE".into(), String::new())], &[], &[]);
    resident.complete_sends(&[], &[], &[("R-PRESSURE".into(), "unknown".into())]);
    resident.complete_sends(&[("R-PRESSURE".into(), "strong-id".into())], &[], &[]);
    // Stop the active worker before retaining notices, then abandon its channel.
    stop.store(true, Ordering::Relaxed);
    assert_eq!(wait.recv_timeout(Duration::from_secs(5)).unwrap().1, WorkerExit::LocalStop);
    drop(worker_tx);
    let (staged_tx, staged_rx) = mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&resident, &staged_tx), Err(HandoffError::Full(2)));
    assert_eq!(resident.submit(OwnedRequest::Probe { id: 7 }), SubmitVerdict::Accepted);
    assert_eq!(gate.forward(&resident, &staged_tx), Err(HandoffError::Full(0)));
    assert_eq!(resident.pending_ingress(), 1);
    let retained = resident.retained_notices();
    assert_eq!(retained.len(), 2);
    // Discard staged hints after the old worker is gone. The resident retains
    // authoritative notices; reconstruct the replacement from its snapshot.
    drop(staged_rx);
    let (staged_tx, staged_rx) = mpsc::channel();
    let (worker_tx, worker_rx) = mpsc::channel();
    for item in &retained { worker_tx.send(item.clone()).unwrap(); }
    let stop2 = Arc::new(AtomicBool::new(false));
    let (done2, wait2) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth); let resident = resident.clone();
        std::thread::spawn(move || { done2.send(run_worker(cfg, resident, &wake_true, worker_rx, _ev_placeholder(), stop2)).unwrap(); });
    }
    let deadline = Instant::now() + Duration::from_secs(10); let mut probe_ran = false;
    loop {
        let ready = gate.forward(&resident, &staged_tx).is_ok();
        for item in staged_rx.try_iter() { worker_tx.send(item).unwrap(); }
        if ready && !probe_ran { assert_eq!(resident.drain().unwrap(), 1); probe_ran = true; }
        let result = control.request(caligo_core::ipc::ControlMsg::QueryRequest { request_id: "R-PRESSURE".into() }).unwrap();
        if probe_ran && resident.retained_notices().is_empty()
            && matches!(result, CoreToControlMsg::QueryResult { native_id: Some(ref id), .. } if id == "strong-id") { break; }
        assert!(Instant::now() < deadline, "ACKs did not drain pressure and pending suffix");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(LAB_SENDS.load(Ordering::Relaxed) - before, 1);
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    let (counters, exit) = wait2.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert_eq!(counters.results_sent, 3, "duplicate hints must not resend result notices");
    runner.join().unwrap().unwrap();
}


#[test]
fn concurrent_worker_for_same_resident_must_be_rejected() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("worker-exclusive");
    let (daemon, make_runner) = start_daemon(&prefix);
    let baseline_epoch = daemon.connection_epoch();
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (done, wait) = mpsc::channel();
    let (first_tx, first_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone(); let stop = stop.clone();
        std::thread::spawn(move || { done.send(run_worker(cfg, resident, &wake_true, first_rx, _ev_placeholder(), stop)).unwrap(); });
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while daemon.connection_epoch() <= baseline_epoch || caligo_bridge::daemon_client::counters_snapshot().hellos_accepted == 0 {
        assert!(Instant::now() < deadline, "first worker did not attach");
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = Arc::new(AtomicBool::new(true));
    let (second_tx, second_rx) = mpsc::channel();
    let (rejected_counters, exit) = run_worker(worker_config(&prefix, &auth), resident.clone(), &wake_true, second_rx, _ev_placeholder(), stopped);
    assert_eq!(exit, WorkerExit::WorkerAlreadyRunning);
    assert_eq!(rejected_counters.connects, 0);
    drop(second_tx);
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = ControlClient::connect(&control_name, &auth, "exclusive-client").unwrap();
    assert!(matches!(control.request(caligo_core::ipc::ControlMsg::Health {}).unwrap(), CoreToControlMsg::HealthAck {}));
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    assert_eq!(wait.recv_timeout(Duration::from_secs(5)).unwrap().1, WorkerExit::CoreStopped);
    runner.join().unwrap().unwrap();
    drop(first_tx);
    let (next_tx, next_rx) = mpsc::channel();
    let (_, next_exit) = run_worker(worker_config(&prefix, &auth), resident, &wake_true, next_rx, _ev_placeholder(), Arc::new(AtomicBool::new(true)));
    assert_eq!(next_exit, WorkerExit::LocalStop, "normal worker exit must release the lease");
    drop(next_tx);
}

#[test]
fn worker_panic_quarantines_same_resident_without_reconnect() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("worker-panic");
    let (daemon, make_runner) = start_daemon(&prefix);
    let baseline_epoch = daemon.connection_epoch();
    let auth = daemon.auth_token().to_string(); let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (_tx, rx) = mpsc::channel();
    let worker = {
        let cfg = worker_config(&prefix, &auth); let resident = resident.clone(); let stop = stop.clone();
        std::thread::spawn(move || run_worker(cfg, resident, &|| panic!("synthetic worker teardown panic"), rx, _ev_placeholder(), stop))
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while daemon.connection_epoch() <= baseline_epoch || caligo_bridge::daemon_client::counters_snapshot().hellos_accepted == 0 {
        assert!(Instant::now() < deadline, "first worker did not attach");
        std::thread::sleep(Duration::from_millis(20));
    }
    stop.store(true, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "panicking worker did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(worker.join().is_err());
    let (replacement_tx, replacement_rx) = mpsc::channel();
    let (counters, exit) = run_worker(worker_config(&prefix, &auth), resident, &wake_true, replacement_rx, _ev_placeholder(), Arc::new(AtomicBool::new(true)));
    assert_eq!(exit, WorkerExit::WorkerQuarantined);
    assert_eq!(counters.connects, 0);
    drop(replacement_tx);
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = ControlClient::connect(&control_name, &auth, "panic-client").unwrap();
    control.request(caligo_core::ipc::ControlMsg::Stop {}).unwrap();
    runner.join().unwrap().unwrap();
}

fn wake_false() -> bool {
    false
}

fn wake_true() -> bool {
    true
}

fn pipe_names_from_prefix(prefix: &str) -> (String, String) {
    (format!("{prefix}-bridge"), format!("{prefix}-control"))
}

// run_worker 的 rx 参数占位(第一个用例不消费结果)。
fn _rx_placeholder() -> mpsc::Receiver<caligo_bridge::resident::RetainedNotice> {
    let (_t, r) = mpsc::channel();
    r
}

fn _ev_placeholder() -> mpsc::Receiver<caligo_bridge::resident::OwnedEvent> {
    let (_t, r) = mpsc::channel();
    r
}

// ---- T07(C4):断开前未确认事件必须跨连接重放对账 ----

/// daemon 在 Hello 受理后不读在途字节直接断开(LAB 故障注入):worker 已
/// 写入管道的事件随缓冲丢弃。修复后 worker 的跨连接未确认窗口在新连接上
/// 重放该事件 → core 持久化 → control 可拉取;修复前窗口随连接重建而
/// 清空,事件双侧丢失。
#[test]
fn t07_unacked_event_replays_after_break_before_consume() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("t07replay");
    let (daemon, make_runner) = start_daemon_mode(&prefix, true);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (ev_tx, ev_rx) = mpsc::channel::<caligo_bridge::resident::OwnedEvent>();
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_false, _rx_placeholder(), ev_rx, stop);
            w_tx.send(r).unwrap();
        });
    }
    // 在 daemon 故障断开(Hello 受理后 300ms)之前把事件推给 worker:
    // worker 写入管道,daemon 不读取 —— 断开时随缓冲丢弃。
    ev_tx
        .send(caligo_bridge::resident::OwnedEvent {
            source: caligo_bridge::resident::EventSourceKind::Recv,
            chat_type: 1,
            peer_uid: "123456".into(),
            peer_uin: "123456".into(),
            sender_uin: "friend-a".into(),
            native_id: "EVT-T07-1".into(),
            text: "CALIGO-T07-REPLAY".into(),
            msg_time: None,
            wrapper_message: None,
        })
        .unwrap();

    // 故障断开后 worker 重连;窗口重放必须让事件最终落地。
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "lab-client") {
            control = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut control = control.expect("control connected(worker 已重连接入)");

    let mut seen: Vec<(String, String)> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let CoreToControlMsg::EventsBatch { events } = control
            .request(caligo_core::ipc::ControlMsg::DrainEvents { max: 10 })
            .unwrap()
        {
            for e in events {
                seen.push((e.native_id, e.text));
            }
            if seen.iter().any(|(id, _)| id == "EVT-T07-1") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        seen.iter().any(|(id, _)| id == "EVT-T07-1"),
        "断开前未确认事件必须经重连重放对账; seen={seen:?}"
    );

    control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    let (counters, exit) = match w_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(v) => v,
        Err(_) => panic!(
            "worker exit timeout; snapshot={:?}",
            caligo_bridge::daemon_client::counters_snapshot()
        ),
    };
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert!(counters.reconnects >= 1, "故障断开必须经历重连");
}

// ---- C5:Dispatch 不得在执行侧受理前上报 NativeStarted ----

/// worker 未 bootstrap(resident 未就绪)时,Dispatch 只能挂 pending,
/// 不得上报 NativeStarted;bootstrap 后补交受理才报 started 并完成结果。
/// 修复前"先报 started 再提交"会把未执行项记成已开始(查询可见反例)。
#[test]
fn native_started_only_after_resident_accepts() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("c5boundary");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    // 故意不 bootstrap:Dispatch 到达时 submit 返回 NotReady → pending。
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<caligo_bridge::resident::RetainedNotice>();
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config(&prefix, &auth);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(cfg, resident, &wake_true, rx, _ev_placeholder(), stop);
            w_tx.send(r).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "lab-client") {
            control = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut control = control.expect("control connected");

    // 提交 R1(受理即派发)。
    let mut accepted = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let r = control
            .request(caligo_core::ipc::ControlMsg::SendText {
                request_id: "R-C5".into(),
                target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
                text: "CALIGO-C5-BOUNDARY".into(),
                deadline_ms: 30_000,
            })
            .unwrap();
        if matches!(r, CoreToControlMsg::SendAccepted { .. }) {
            accepted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(accepted, "R-C5 未被受理");

    // resident 未就绪窗口:状态必须停在 DispatchIntent,不得出现 NativeStarted。
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut saw_started = false;
    while Instant::now() < deadline {
        if let CoreToControlMsg::QueryResult { state, .. } = control
            .request(caligo_core::ipc::ControlMsg::QueryRequest {
                request_id: "R-C5".into(),
            })
            .unwrap()
        {
            let s = state.unwrap_or_default();
            if s.contains("NativeStarted") {
                saw_started = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !saw_started,
        "resident 未就绪时不得上报 NativeStarted(C5 真实开始边界)"
    );

    // bootstrap → pending 补交受理 → started → 结果确认。
    resident.bootstrap().unwrap();
    let mut confirmed = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let _ = resident.drain();
        caligo_bridge::resident::ResultHandoffGate::default().forward(&resident, &tx).unwrap();
        if let CoreToControlMsg::QueryResult {
            state, native_id, ..
        } = control
            .request(caligo_core::ipc::ControlMsg::QueryRequest {
                request_id: "R-C5".into(),
            })
            .unwrap()
        {
            if native_id.as_deref() == Some("NM-1")
                || state
                    .as_deref()
                    .map(|s| s.contains("ConfirmedSuccess"))
                    .unwrap_or(false)
            {
                confirmed = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(confirmed, "bootstrap 后 pending 必须补交并完成全链");

    control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    let (_, exit) = match w_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(v) => v,
        Err(_) => panic!(
            "worker exit timeout; snapshot={:?}",
            caligo_bridge::daemon_client::counters_snapshot()
        ),
    };
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
}

// ---- T03+T05(C1/C9):重连推进连接代次;换宿主必须拒绝 ----

/// worker A(宿主 N1)接入 → daemon 故障断开 → 同宿主自动重连:
/// 连接代次必须递增且 actor 回到 Active(T03)。
/// 之后 worker A 退出,同账号同代次但不同宿主 N2 的连接必须被拒绝(T05);
/// 同宿主 N1 重连受理且业务可继续。
#[test]
fn reconnect_requires_same_host_and_advances_epoch() {
    let _g = TEST_SERIAL.lock().unwrap();
    let prefix = unique_prefix("t03t05host");
    let (daemon, make_runner) = start_daemon_mode(&prefix, true);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let resident = Arc::new(Resident::new(LabHost, ResidentLimits::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (w_tx, w_rx) = mpsc::channel();
    {
        let cfg = worker_config_host(&prefix, &auth, 0x1111_1111);
        let resident = resident.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let r = run_worker(
                cfg,
                resident,
                &wake_false,
                _rx_placeholder(),
                _ev_placeholder(),
                stop,
            );
            w_tx.send(r).unwrap();
        });
    }
    let control_name = pipe_names_from_prefix(&prefix).1;
    let mut control = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(c) = ControlClient::connect(&control_name, &auth, "lab-client") {
            control = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut control = control.expect("control connected");

    // T03:故障断开后同宿主重连 —— 代次递增 + actor Active。
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && daemon.connection_epoch() < 2 {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(daemon.connection_epoch() >= 2, "重连必须推进连接代次(T03)");
    let mut active = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let r = control
            .request(caligo_core::ipc::ControlMsg::SendText {
                request_id: "R-T03".into(),
                target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
                text: "CALIGO-T03-RECONNECT".into(),
                deadline_ms: 30_000,
            })
            .unwrap();
        if matches!(r, CoreToControlMsg::SendAccepted { .. }) {
            active = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        active,
        "同宿主重连后 actor 必须 Active(T03);epoch={}",
        daemon.connection_epoch()
    );

    // worker A 干净退出 → daemon 降级,accept 循环继续。
    stop.store(true, Ordering::Relaxed);
    let (_, exit) = w_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(exit, WorkerExit::LocalStop);
    std::thread::sleep(Duration::from_millis(300)); // 断开被 daemon 处理

    let (bridge_name, _) = pipe_names_from_prefix(&prefix);

    // T05(反例):同账号同代次、不同宿主 nonce → 必须拒绝。
    match BridgeClient::connect(
        &bridge_name,
        &auth,
        "t05-bridge",
        1,
        "10001",
        "qq-9.9.33-52230",
        0x2222_2222,
        "2026-10-08T00:00:00Z",
    ) {
        Err(_) => {} // HelloAck accepted=false → connect Err(即拒绝)
        Ok(_) => panic!("不同宿主的同代次连接必须被拒绝(T05)"),
    }

    // T05(正例):同宿主 N1 重连 → 受理;业务继续(Active)。
    // (前次连接的排水窗可能短暂 BUSY:轮询重试。)
    let mut bridge = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match BridgeClient::connect(
            &bridge_name,
            &auth,
            "t05-bridge",
            1,
            "10001",
            "qq-9.9.33-52230",
            0x1111_1111,
            "2026-10-08T00:00:00Z",
        ) {
            Ok(b) => {
                bridge = Some(b);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let mut bridge = bridge.expect("同宿主重连必须受理(T05)");
    let _ = &mut bridge;
    let r = control
        .request(caligo_core::ipc::ControlMsg::SendText {
            request_id: "R-T05".into(),
            target: serde_json::json!({"account":"10001","kind":"private","peer":"123456"}),
            text: "CALIGO-T05-SAMEHOST".into(),
            deadline_ms: 30_000,
        })
        .unwrap();
    assert!(
        matches!(r, CoreToControlMsg::SendAccepted { .. }),
        "同宿主重连后业务必须可继续: {r:?}"
    );

    control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    runner.join().unwrap().unwrap();
}

#[test]
fn private_and_group_png_traverse_real_worker_and_owner_without_retry() {
    let _g = TEST_SERIAL.lock().unwrap();
    struct ImageHost(Arc<std::sync::Mutex<Vec<(u32, String, Vec<u8>)>>>);
    impl HostAdapter for ImageHost {
        fn owner_thread_id(&self) -> u64 {
            lab_tid()
        }
        fn current_thread_id(&self) -> u64 {
            lab_tid()
        }
        fn env_valid(&self) -> bool {
            true
        }
        fn current_context_ok(&self) -> bool {
            true
        }
        fn supports_image(&self, chat: u32) -> bool {
            matches!(chat, 1 | 2)
        }
        fn native_op(
            &mut self,
            op: HostOp,
        ) -> Result<HostOpResult, caligo_bridge::host_adapter::HostError> {
            Ok(match op {
                HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
                HostOp::ListenerRemove { .. } => HostOpResult::ListenerRemoved,
                HostOp::SendImage {
                    chat_type,
                    peer_uin,
                    png,
                    ..
                } => {
                    self.0.lock().unwrap().push((chat_type, peer_uin, png));
                    HostOpResult::Sent {
                        native_id: Some(format!("90071992547409931{chat_type}")),
                    }
                }
                _ => panic!("image-only fixture"),
            })
        }
    }
    let png_hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
    let png: Vec<u8> = png_hex
        .as_bytes()
        .chunks_exact(2)
        .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
        .collect();
    let prefix = unique_prefix("image");
    let (daemon, make_runner) = start_daemon(&prefix);
    let auth = daemon.auth_token().to_string();
    let runner = make_runner();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let resident = Arc::new(Resident::new(
        ImageHost(calls.clone()),
        ResidentLimits::default(),
    ));
    resident.bootstrap().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (w_tx, w_rx) = mpsc::channel();
    let cfg = worker_config(&prefix, &auth);
    let worker_resident = resident.clone();
    let worker_stop = stop.clone();
    std::thread::spawn(move || {
        w_tx.send(run_worker(
            cfg,
            worker_resident,
            &wake_true,
            rx,
            _ev_placeholder(),
            worker_stop,
        ))
        .unwrap();
    });
    let until = Instant::now() + Duration::from_secs(5);
    while !daemon.supports_image(1) || !daemon.supports_image(2) {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    for (chat, kind, peer) in [
        (1, caligo_model::SessionKind::Private, "20002"),
        (2, caligo_model::SessionKind::Group, "30003"),
    ] {
        let id = daemon.submit_image(kind, peer, png.clone(), 5000).unwrap();
        let target = caligo_model::SessionKey::new("10001", kind, peer);
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            resident.drain().unwrap();
            caligo_bridge::resident::ResultHandoffGate::default().forward(&resident, &tx).unwrap();
            if matches!(
                daemon.onebot_text_status(&id, &target),
                Some((caligo_model::ActionState::ConfirmedSuccess { .. }, None))
            ) {
                break;
            }
            assert!(Instant::now() < until, "image {chat} did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(1, "20002".into(), png.clone()), (2, "30003".into(), png)]
    );
    let mut control =
        ControlClient::connect(&pipe_names_from_prefix(&prefix).1, &auth, "image-lab").unwrap();
    control
        .request(caligo_core::ipc::ControlMsg::Stop {})
        .unwrap();
    let (counters, exit) = w_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    runner.join().unwrap().unwrap();
    assert_eq!(exit, WorkerExit::CoreStopped);
    assert_eq!(counters.dispatches, 2);
    assert_eq!(counters.bootstraps_attempted, 0);
}
