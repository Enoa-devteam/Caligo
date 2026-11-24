//! K4-D7 候选 B 首入机器 LAB 测试。
//!
//! 用**假 QQNT 符号**(Rust 函数,ABI 与真实导出一致)驱动同一台首入状态机:
//! 机器逻辑(阶段顺序、零 current 拒绝、owner 线程断言、关闭协议)在此验证;
//! 真实 QQNT 地址/ABI 属 D7 现场证据(B0-FIELD),不由本文件替代。
//!
//! 机器是进程单实例(§6.1),用例间经 `test_reset_state` + 全局锁隔离;
//! 运行:`cargo test -p caligo-bridge --test qq_entry_lab`。

use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::qq_entry::{EntryConfig, EntryError, QqOwnerAdapter, bootstrap};
use caligo_bridge::resident::{Resident, ResidentLimits};

#[path = "support/owner_fixture.rs"]
mod owner_fixture;
use owner_fixture::*;

// Each fault runs in a child: before containment the extern C callback aborts.
// Never let a regression terminate the test runner or contaminate other cases.
#[test]
fn owner_hook_faults_are_contained_in_child_processes() {
    let mut failures = Vec::new();
    for case in ["normal", "shutdown", "poison"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "owner_hook_fault_child", "--nocapture"])
            .env("CALIGO_OWNER_HOOK_FAULT_CHILD", case)
            .output().unwrap();
        if !output.status.success() {
            failures.push(format!("{case} child failed: {:?}\n{}\n{}",
                output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn owner_hook_fault_child() {
    let Ok(case) = std::env::var("CALIGO_OWNER_HOOK_FAULT_CHILD") else { return; };
    lab_reset();
    let (_qq, qqnt_base, env, _) = FakeQq::new();
    let handle = bootstrap(&EntryConfig { qqnt_base, env, report_path: report_path(&case) }, &fake_symbols()).unwrap();
    let root = std::sync::Arc::new(());
    let weak = std::sync::Arc::downgrade(&root);
    handle.set_drain_hook(Box::new(move || {
        let _keep = &root;
        DRAIN_CALLS.fetch_add(1, Ordering::Relaxed);
        panic!("synthetic owner hook fault");
    }));
    // Deliberately no SHUTDOWN_READY: the hook itself must remain a root.
    if case == "poison" {
        caligo_bridge::qq_entry::test_poison_drain_hook();
        assert_eq!(caligo_bridge::caligo_qq_entry_shutdown(0), 0x23);
        pump_once();
    } else if case == "shutdown" {
        handle.daemon_stop_flag().store(true, Ordering::Release);
        let (wake_tx, wake_rx) = std::sync::mpsc::channel();
        *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
        let closer = std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(1000));
        wake_rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
        *WAKE_NOTICE.lock().unwrap() = None;
        pump_once();
        assert_eq!(closer.join().unwrap(), 0x23);
    } else {
        pump_once();
    }
    assert!(handle.quarantine());
    let expected_calls = if case == "poison" { 0 } else { 1 };
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), expected_calls);
    let sends = FAKE_SEND_CALLS.load(Ordering::Relaxed);
    for _ in 0..3 {
        pump_once();
        assert!(!handle.wake());
        assert!(!caligo_bridge::qq_entry::wake_owner_pump());
        handle.clear_drain_hook();
        caligo_bridge::qq_entry::clear_drain_hook();
        handle.set_drain_hook(Box::new(|| panic!("replacement must never run")));
        assert!(!caligo_bridge::qq_entry::try_install_drain_hook(Box::new(|| 0)));
        assert_eq!(caligo_bridge::caligo_qq_entry_shutdown(0), 0x23);
        assert!(weak.upgrade().is_some(), "fault must retain captured root");
    }
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), expected_calls);
    assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), sends);
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 0);
}

// Exercise the actual formal hook helper through the registered extern C pump.
// No hand-built copy of the production close branch is used here.
#[test]
fn formal_close_error_is_sticky_and_retains_roots() {
    formal_close_case(true);
}

#[test]
fn formal_close_success_releases_roots() {
    formal_close_case(false);
}

fn formal_close_case(fail_remove: bool) {
    use std::sync::{Arc, atomic::AtomicBool};
    let _guard = TEST_LOCK.lock().unwrap();
    lab_reset();
    struct CloseHost { fail: bool, removes: Arc<AtomicUsize> }
    impl HostAdapter for CloseHost {
        fn owner_thread_id(&self) -> u64 { 1 }
        fn current_thread_id(&self) -> u64 { 1 }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
            match op {
                HostOp::ListenerAdd => Ok(HostOpResult::ListenerAdded { token: 17 }),
                HostOp::ListenerRemove { token: 17 } => {
                    self.removes.fetch_add(1, Ordering::Relaxed);
                    if self.fail { Err(HostError::Native { code: 77 }) }
                    else { Ok(HostOpResult::ListenerRemoved) }
                }
                _ => panic!("unexpected host operation"),
            }
        }
    }
    let (_qq, qqnt_base, env, _) = FakeQq::new();
    let report = report_path(if fail_remove { "close-error" } else { "close-success" });
    let handle = bootstrap(&EntryConfig { qqnt_base, env, report_path: report.clone() }, &fake_symbols()).unwrap();
    let removes = Arc::new(AtomicUsize::new(0));
    let resident = Arc::new(Resident::new(CloseHost { fail: fail_remove, removes: removes.clone() }, ResidentLimits::default()));
    resident.bootstrap().unwrap();
    let weak = Arc::downgrade(&resident);
    caligo_bridge::qq_entry::retain_daemon_until_settled(resident.clone(), Arc::new(AtomicBool::new(true)));
    handle.set_drain_hook(Box::new(move || caligo_bridge::qq_entry::close_daemon_owner(&resident)));
    handle.daemon_stop_flag().store(true, Ordering::Release);
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
    let closer = std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(1000));
    wake_rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
    *WAKE_NOTICE.lock().unwrap() = None;
    pump_once();
    // On the old path uv_close succeeds despite the failed listener removal.
    if FAKE_CLOSE_CALLS.load(Ordering::Relaxed) != 0 { close_once(); }
    let code = closer.join().unwrap();
    assert_eq!(removes.load(Ordering::Relaxed), 1);
    if fail_remove {
        assert_eq!(code, 0x24, "formal close error must never report clean");
        assert!(handle.quarantine());
        assert!(weak.upgrade().unwrap().quarantine_reason());
        let sends = FAKE_SEND_CALLS.load(Ordering::Relaxed);
        for _ in 0..3 {
            pump_once();
            handle.clear_drain_hook();
            caligo_bridge::qq_entry::clear_drain_hook();
            handle.set_drain_hook(Box::new(|| panic!("cleanup-fault replacement must not run")));
            assert!(!caligo_bridge::qq_entry::try_install_drain_hook(Box::new(|| 0)));
            assert!(!handle.wake());
            assert!(!caligo_bridge::qq_entry::wake_owner_pump());
            assert_eq!(caligo_bridge::caligo_qq_entry_shutdown(0), 0x24);
            assert!(weak.upgrade().is_some(), "cleanup failure retains resident to host exit");
        }
        assert_eq!(removes.load(Ordering::Relaxed), 1, "native cleanup is never retried");
        assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), sends);
        assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 0);
        let log = std::fs::read_to_string(&report).unwrap();
        assert!(log.contains("owner_cleanup_failed"));
        assert!(!log.contains("closed via owner pump"));
    } else {
        assert_eq!(code, 0);
        assert!(!handle.quarantine());
        assert!(weak.upgrade().is_none(), "normal close releases both resident roots");
        assert_eq!(caligo_bridge::caligo_qq_entry_shutdown(0), 0);
        assert_eq!(removes.load(Ordering::Relaxed), 1);
        assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
    }
}

// ---- 用例 ----

#[test]
fn staged_bootstrap_wake_and_owner_pump() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("full"),
    };
    let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    assert_eq!(
        FAKE_INIT_CALLS.load(Ordering::Relaxed),
        1,
        "uv_async_init 恰一次(常驻)"
    );
    assert_eq!(FAKE_RI_CALLS.load(Ordering::Relaxed), 1);
    assert!(!handle.quarantine());

    // owner 轮转点:drain 钩子被调用。
    handle.set_drain_hook(Box::new(move || {
        DRAIN_CALLS.fetch_add(1, Ordering::Relaxed);
        1
    }));
    assert!(handle.wake());
    assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), 1);
    pump_once();
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), 1);
    assert!(handle.pumps() >= 1);
}

#[test]
fn second_bootstrap_is_rejected_before_any_native_call() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("double"),
    };
    let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    let interrupts = FAKE_RI_CALLS.load(Ordering::Relaxed);
    let inits = FAKE_INIT_CALLS.load(Ordering::Relaxed);
    assert!(matches!(
        bootstrap(&cfg, &fake_symbols()),
        Err(EntryError::AlreadyBootstrapped)
    ));
    assert_eq!(
        FAKE_RI_CALLS.load(Ordering::Relaxed),
        interrupts,
        "second bootstrap must not post another first-entry interrupt"
    );
    assert_eq!(
        FAKE_INIT_CALLS.load(Ordering::Relaxed),
        inits,
        "second bootstrap must not install a second async handle"
    );
    assert!(
        caligo_bridge::qq_entry::uv_loop_alive(),
        "the first session must stay admitted"
    );
    assert!(!handle.quarantine());
}

#[test]
fn close_completes_via_pump_and_close_callback() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("close"),
    };
    // bootstrap 在"owner 线程"上执行(假宿主同步首入 → ENTRY.tid = 该线程);
    // 之后轮转点也必须由同一线程驱动(pump 的 owner 断言是机器契约)。
    let (tx_handle, rx_handle) = std::sync::mpsc::channel();
    let (tx_drive, rx_drive) = std::sync::mpsc::channel::<()>();
    let owner = std::thread::spawn(move || {
        let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
        tx_handle.send(handle).unwrap();
        // 等主线程发起 close 后,由本线程(= owner)驱动轮转点与关闭回调。
        rx_drive.recv().unwrap();
        pump_once(); // 处理 CLOSE_REQ → uv_close
        pump_once(); // stale callback before close confirmation
        close_once(); // 关闭回调 → closed
        pump_once(); // stale callback after close confirmation
    });
    let handle = std::sync::Arc::new(rx_handle.recv().unwrap());
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
    let closer = {
        let h = handle.clone();
        std::thread::spawn(move || h.close(3000))
    };
    // The close thread must actually publish CLOSE_REQ and wake the owner.
    // A fixed sleep races thread scheduling under a loaded full build.
    wake_rx.recv_timeout(std::time::Duration::from_secs(3)).expect("close requested owner wake");
    *WAKE_NOTICE.lock().unwrap() = None;
    tx_drive.send(()).unwrap();
    let r = closer.join().unwrap();
    owner.join().unwrap();
    r.expect("close protocol completes");
    let reboot = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("close-reboot"),
    };
    assert!(
        matches!(
            bootstrap(&reboot, &fake_symbols()),
            Err(EntryError::AlreadyBootstrapped)
        ),
        "a closed host generation must not admit a second bridge"
    );
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
    let sends = FAKE_SEND_CALLS.load(Ordering::Relaxed);
    let handle_wake = handle.wake();
    let trampoline_wake = caligo_bridge::qq_entry::wake_owner_pump();
    assert_eq!(
        (handle_wake, trampoline_wake, FAKE_SEND_CALLS.load(Ordering::Relaxed)),
        (false, false, sends),
        "neither ordinary wake may call libuv after clean shutdown"
    );
}

#[cfg(feature = "research")]
#[test]
fn research_stop_window_samples_on_owner_before_one_close() { observation_case("success"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_snapshot_failure_preserves_roots() { observation_case("sample_failure"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_sample_incomplete_reason() { observation_case("sample_incomplete"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_binding_changed_reason() { observation_case("sample_binding_changed"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_binding_invalid_reason() { observation_case("sample_binding_invalid"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_symbols_unavailable_reason() { observation_case("sample_symbols"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_v8_failed_reason() { observation_case("sample_v8"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_shape_reason() { observation_case("sample_shape"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_report_poisoned_reason() { observation_case("sample_report_poisoned"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_report_budget_reason() { observation_case("sample_report_budget"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_reports_missing_binding_reason() { observation_case("identity_missing"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_identity_drift_preserves_roots() { observation_case("identity_drift"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_cancel_takes_final_sample() { observation_case("cancel"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_cancel_before_stop_uses_normal_close() { observation_case("cancel_before"); }
#[cfg(feature = "research")]
#[test]
fn research_stop_window_slow_sample_cannot_overrun_deadline() { observation_case("slow_sample"); }

#[cfg(feature = "research")]
#[test]
fn research_stop_window_times_out_while_owner_sample_is_blocked() {
    use caligo_bridge::qq_entry::{arm_stop_observation, install_stop_observation_driver};
    use caligo_bridge::stop_observation::{Sample, StopIdentity};

    let _serial = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, base, env, _) = FakeQq::new();
    let handle = bootstrap(
        &EntryConfig { qqnt_base: base, env, report_path: report_path("blocked_sample") },
        &fake_symbols(),
    ).unwrap();
    let identity = StopIdentity {
        account: "10001".into(), host_nonce: 7,
        wrapper_session: "nt_fixture".into(), listener_generation: 1,
    };
    let keep = Arc::new(());
    let weak = Arc::downgrade(&keep);
    handle.set_drain_hook(Box::new(move || { let _keep = &keep; 0 }));
    handle.daemon_stop_flag().store(true, Ordering::Release);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let cached = identity.clone();
    install_stop_observation_driver(
        Box::new(move || Some(cached.clone())),
        Box::new(move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(LAB_WAIT).map_err(|_| "sample release timeout")?;
            Ok(Sample { callbacks_seen: 0 })
        }),
    ).unwrap();
    arm_stop_observation(identity, 80, 500).unwrap();
    let (wake_tx, wake_rx) = mpsc::channel();
    *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
    let (done_tx, done_rx) = mpsc::channel();
    let closer = std::thread::spawn(move || {
        let _ = done_tx.send(caligo_bridge::caligo_qq_entry_shutdown(500));
    });
    wake_rx.recv_timeout(LAB_WAIT).unwrap();
    let coordinator = std::thread::spawn(move || {
        entered_rx.recv_timeout(LAB_WAIT).unwrap();
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();
        result
    });
    pump_once();
    let result_while_blocked = coordinator.join().unwrap();
    closer.join().unwrap();
    assert_eq!(result_while_blocked, Ok(0x25), "shutdown must not wait for the blocked sampler");
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::SeqCst), 0);
    assert!(weak.upgrade().is_some());
    assert!(handle.quarantine());
}

#[cfg(feature = "research")]
fn observation_case(mode: &'static str) {
    use caligo_bridge::qq_entry::{arm_stop_observation, cancel_stop_observation, install_stop_observation_driver};
    use caligo_bridge::stop_observation::{Sample, StopIdentity};
    let _serial=TEST_LOCK.lock().unwrap();lab_reset();
    let (_qq,base,env,_)=FakeQq::new();
    let report=report_path(mode);
    let handle=bootstrap(&EntryConfig { qqnt_base:base,env,report_path:report.clone() },&fake_symbols()).unwrap();
    let identity=StopIdentity { account:"10001".into(),host_nonce:7,wrapper_session:"nt_fixture".into(),listener_generation:1 };
    let current=Arc::new(Mutex::new(identity.clone()));
    let missing=Arc::new(AtomicBool::new(false));
    let missing_reader=missing.clone();
    let stops=Arc::new(AtomicUsize::new(0));let samples=Arc::new(AtomicUsize::new(0));
    let keep=Arc::new(7usize);let weak=Arc::downgrade(&keep);
    let stopped=stops.clone();
    handle.set_drain_hook(Box::new(move || { let _keep=&keep;stopped.fetch_add(1,Ordering::SeqCst);0 }));
    handle.daemon_stop_flag().store(true,Ordering::Release);
    let binding=current.clone();let seen=samples.clone();let closed=stops.clone();
    let owner=std::thread::current().id();
    let sample_reason = match mode {
        "sample_failure" => Some("fixture"),
        "sample_incomplete" => Some("observation_incomplete"),
        "sample_binding_changed" => Some("observation_binding_changed"),
        "sample_binding_invalid" => Some("observation_binding_invalid"),
        "sample_symbols" => Some("observation_symbols_unavailable"),
        "sample_v8" => Some("observation_v8_failed"),
        "sample_shape" => Some("observation_shape"),
        "sample_report_poisoned" => Some("observation_report_poisoned"),
        "sample_report_budget" => Some("observation_report_budget"),
        _ => None,
    };
    install_stop_observation_driver(Box::new(move || if missing_reader.load(Ordering::SeqCst) { None } else { Some(binding.lock().unwrap().clone()) }),
        Box::new(move |_| { assert_eq!(std::thread::current().id(),owner);assert_eq!(closed.load(Ordering::SeqCst),1);
            let n=seen.fetch_add(1,Ordering::SeqCst);
            if mode=="slow_sample" { std::thread::sleep(Duration::from_millis(60)); }
            if let Some(reason)=sample_reason { return Err(reason); }
            Ok(Sample { callbacks_seen:n as u64 })
        })).unwrap();
    arm_stop_observation(identity,if mode=="slow_sample" { 1 } else if mode=="cancel" { 5000 } else { 80 },
        if mode=="slow_sample" { 40 } else if mode=="cancel" { 6000 } else { 1000 }).unwrap();
    if mode=="cancel_before" { cancel_stop_observation().unwrap(); }
    let (tx,rx)=mpsc::channel();*WAKE_NOTICE.lock().unwrap()=Some(tx);
    let closer=std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(1000));
    rx.recv_timeout(LAB_WAIT).unwrap();
    if mode=="identity_drift" { current.lock().unwrap().host_nonce+=1; }
    if mode=="identity_missing" { missing.store(true,Ordering::SeqCst); }
    pump_once();
    if mode=="cancel" { cancel_stop_observation().unwrap(); }
    let deadline=Instant::now()+LAB_WAIT;
    while !closer.is_finished() {
        assert!(Instant::now()<deadline,"window did not terminate");
        if rx.recv_timeout(Duration::from_millis(100)).is_ok() { pump_once(); }
        if FAKE_CLOSE_CALLS.load(Ordering::SeqCst)>0 { close_once(); }
    }
    let code=closer.join().unwrap();*WAKE_NOTICE.lock().unwrap()=None;
    if mode=="success" || mode=="cancel" || mode=="cancel_before" {
        assert_eq!(code,0);assert_eq!(stops.load(Ordering::SeqCst),1);
        if mode=="cancel_before" { assert_eq!(samples.load(Ordering::SeqCst),0); }
        else { assert!(samples.load(Ordering::SeqCst)>=2); }assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::SeqCst),1);
        assert!(weak.upgrade().is_none());
    } else {
        assert_eq!(code,0x25);assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::SeqCst),0);
        assert!(weak.upgrade().is_some());assert!(handle.quarantine());
        assert!(!handle.wake());pump_once();assert_eq!(caligo_bridge::caligo_qq_entry_shutdown(0),0x25);
        let expected = match mode {
            "sample_incomplete" => Some("observation_incomplete"),
            "sample_binding_changed" => Some("observation_binding_changed"),
            "sample_binding_invalid" => Some("observation_binding_invalid"),
            "sample_symbols" => Some("observation_symbols_unavailable"),
            "sample_v8" => Some("observation_v8_failed"),
            "sample_shape" => Some("observation_shape"),
            "sample_report_poisoned" => Some("observation_report_poisoned"),
            "sample_report_budget" => Some("observation_report_budget"),
            "sample_failure" => Some("other"),
            "identity_missing" => Some("identity_unavailable"),
            _ => None,
        };
        if let Some(expected) = expected {
            let report = std::fs::read_to_string(&report).unwrap();
            let reason = report.lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .find(|row| row.get("stage").and_then(|v| v.as_str()) == Some("stop_observation_reason"))
                .and_then(|row| row.get("detail").and_then(|v| v.as_str()).map(str::to_owned));
            assert_eq!(reason.as_deref(), Some(expected), "bounded stop reason must be reported");
        }
    }
}

#[test]
fn close_waits_for_admitted_wake_and_refuses_new_wakes() {
    use caligo_bridge::qq_entry::{test_close_requested, wake_owner_pump};

    let _guard = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _) = FakeQq::new();
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("wake-close-overlap") };
    let (ready_tx, ready_rx) = mpsc::channel();
    let (pump_tx, pump_rx) = mpsc::channel();
    let (pumped_tx, pumped_rx) = mpsc::channel();
    let owner = std::thread::spawn(move || {
        let handle = Arc::new(bootstrap(&cfg, &fake_symbols()).unwrap());
        // Call both ordinary routes while the owner holds DRAIN_HOOK.
        let reentrant = handle.clone();
        handle.set_drain_hook(Box::new(move || {
            if reentrant.wake() && wake_owner_pump() {
                DRAIN_CALLS.fetch_add(1, Ordering::Relaxed);
            }
            0
        }));
        pump_once();
        ready_tx.send(handle).unwrap();
        pump_rx.recv_timeout(LAB_WAIT).unwrap();
        pump_once();
        close_once();
        pumped_tx.send(()).unwrap();
    });
    let handle = ready_rx.recv_timeout(LAB_WAIT).expect("reentrant wakes returned");
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), 2);

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *BLOCK_NEXT_SEND.lock().unwrap() = Some((entered_tx, release_rx));
    let waking = handle.clone();
    let sender = std::thread::spawn(move || waking.wake());
    entered_rx.recv_timeout(LAB_WAIT).expect("ordinary send entered native code");

    let closing = handle.clone();
    let closer = std::thread::spawn(move || closing.close(5000));
    let deadline = Instant::now() + LAB_WAIT;
    while !test_close_requested() {
        assert!(Instant::now() < deadline, "shutdown did not publish its request");
        std::thread::yield_now();
    }
    // The loop may already have an event queued when shutdown starts. Drive
    // it now, before the first send releases and before the stop signal posts.
    pump_tx.send(()).unwrap();
    while handle.pumps() < 2 {
        assert!(Instant::now() < deadline, "owner did not enter the close pump");
        std::thread::yield_now();
    }
    let (late_tx, late_rx) = mpsc::channel();
    let late_handle = handle.clone();
    let late = std::thread::spawn(move || {
        late_tx.send((late_handle.wake(), wake_owner_pump())).unwrap();
    });
    assert_eq!(late_rx.recv_timeout(LAB_WAIT).expect("late wake admission decided"), (false, false));
    assert!(SEND_HELD.load(Ordering::SeqCst), "first native send is still held");
    assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), 3, "stop signal waits at the gate");
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 0);

    release_tx.send(()).unwrap();
    assert!(sender.join().unwrap());
    late.join().unwrap();
    pumped_rx.recv_timeout(LAB_WAIT).expect("owner closed after release");
    owner.join().unwrap();
    assert_eq!(closer.join().unwrap(), Ok(()));
    assert!(!CLOSE_OVERLAPPED_SEND.load(Ordering::SeqCst));
    assert_eq!(FAKE_SEND_CALLS.load(Ordering::Relaxed), 4, "exactly one stop signal");
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
}

// Produce a strong result after the owner's final handoff, then stop the
// actual worker before it can consume the result channel. Only shutdown's
// retained root may own the resident once the external references are dropped.
#[test]
fn shutdown_retains_result_produced_after_last_handoff() {
    use caligo_bridge::resident::{OwnedRequest, ResidentNotice, ResultHandoffGate, SendOutcome, SubmitVerdict};
    use std::sync::{Arc, atomic::AtomicBool};
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    struct FiredHost;
    impl HostAdapter for FiredHost {
        fn owner_thread_id(&self) -> u64 { 1 }
        fn current_thread_id(&self) -> u64 { 1 }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
            Ok(match op {
                HostOp::ListenerAdd => HostOpResult::ListenerDeferred,
                HostOp::SendText { request_id, .. } => HostOpResult::SendFired { mid: request_id },
                _ => panic!("unexpected operation"),
            })
        }
    }
    let r = Arc::new(Resident::new(FiredHost, ResidentLimits::default()));
    r.bootstrap().unwrap();
    assert_eq!(r.submit(OwnedRequest::SendText {
        request_id: "stop-result".into(), wire_seq: 1, timeout_ms: 5000,
        chat_type: 2, peer_uid: "123".into(), text: "fixture".into(),
    }), SubmitVerdict::Accepted);
    r.drain().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = ResultHandoffGate::default();
    assert_eq!(gate.forward(&r, &tx), Ok(1));
    let started = rx.recv().unwrap();
    assert!(r.ack_retained_notice(&started));
    assert_eq!(r.complete_sends(&[("stop-result".into(), "strong-native".into())], &[], &[]), vec!["stop-result"]);
    assert!(r.retained_notices().is_empty(), "outcome is still after the last handoff");
    let finished = Arc::new(AtomicBool::new(false));
    caligo_bridge::qq_entry::retain_daemon_until_settled(r.clone(), finished.clone());
    {
        let r = r.clone();
        caligo_bridge::qq_entry::install_drain_hook(Box::new(move || { r.close().unwrap(); 0 }));
    }
    let weak = Arc::downgrade(&r);
    let (etx, erx) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(true));
    let worker = {
        let r = r.clone(); let finished = finished.clone();
        std::thread::spawn(move || {
            let (_, exit) = caligo_bridge::daemon_client::run_worker(
                caligo_bridge::daemon_client::DaemonClientConfig::default(), r, &|| false, rx, erx, stop,
            );
            assert_eq!(exit, caligo_bridge::daemon_client::WorkerExit::LocalStop);
            finished.store(true, Ordering::Release);
        })
    };
    worker.join().unwrap();
    let (_qq, qqnt_base, env, _) = FakeQq::new();
    let entry = bootstrap(&EntryConfig { qqnt_base, env, report_path: report_path("stop-result") }, &fake_symbols()).unwrap();
    entry.daemon_stop_flag().store(true, Ordering::Release);
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
    let closer = std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(100));
    wake_rx.recv().unwrap();
    *WAKE_NOTICE.lock().unwrap() = None;
    pump_once(); close_once();
    let code = closer.join().unwrap();
    drop(r); drop(tx); drop(etx);
    assert_eq!(code, caligo_bridge::qq_entry_code::ERR_SHUTDOWN_PENDING_RESULTS,
        "shutdown must not report clean while strong result is unACKed");
    let preserved = weak.upgrade().expect("shutdown must retain the resident after worker teardown");
    let (dead_tx, dead_rx) = std::sync::mpsc::channel();
    drop(dead_rx);
    assert_eq!(gate.forward(&preserved, &dead_tx), Err(caligo_bridge::resident::HandoffError::ChannelClosed(0)));
    let notices = preserved.retained_notices();
    assert_eq!(notices.len(), 1);
    assert!(matches!(&notices[0].notice, ResidentNotice::Outcome(SendOutcome::Success { request_id, native_id })
        if request_id == "stop-result" && native_id == "strong-native"));
    // An explicit acknowledgement settles the result; retry only releases roots,
    // and must not schedule another uv_close on the already closed handle.
    assert!(preserved.ack_retained_notice(&notices[0]));
    drop(preserved);
    assert_eq!(caligo_bridge::caligo_qq_entry_shutdown(100), 0);
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
    assert!(weak.upgrade().is_none());
}

#[test]
fn shutdown_waits_for_worker_teardown_before_releasing_empty_resident() {
    use std::sync::{Arc, atomic::AtomicBool};
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    struct EmptyHost;
    impl HostAdapter for EmptyHost {
        fn owner_thread_id(&self) -> u64 { 1 }
        fn current_thread_id(&self) -> u64 { 1 }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, _: HostOp) -> Result<HostOpResult, HostError> { panic!("no host operations") }
    }
    let resident = Arc::new(Resident::new(EmptyHost, ResidentLimits::default()));
    let weak = Arc::downgrade(&resident);
    let finished = Arc::new(AtomicBool::new(false));
    caligo_bridge::qq_entry::retain_daemon_until_settled(resident, finished.clone());
    let (_qq, qqnt_base, env, _) = FakeQq::new();
    bootstrap(&EntryConfig { qqnt_base, env, report_path: report_path("stop-empty") }, &fake_symbols()).unwrap();
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
    let closer = std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(100));
    wake_rx.recv().unwrap();
    *WAKE_NOTICE.lock().unwrap() = None;
    pump_once(); close_once();
    // Closed pump and empty results do not prove worker teardown.
    assert_eq!(closer.join().unwrap(), caligo_bridge::qq_entry_code::ERR_SHUTDOWN_PENDING_RESULTS);
    assert!(weak.upgrade().is_some());
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || { done_tx.send(caligo_bridge::caligo_qq_entry_shutdown(3000)).unwrap(); });
    assert!(matches!(done_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
    finished.store(true, Ordering::Release);
    assert_eq!(done_rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap(), 0);
    closer.join().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "research")]
#[test]
fn staged_owner_stop_waits_for_exact_result_acks_and_retains_failed_handoff() {
    use caligo_bridge::qq_entry::{
        arm_stop_observation_with_result_drain, cancel_stop_observation, close_daemon_owner,
        install_stop_observation_driver, retain_daemon_for_result_drain,
    };
    use caligo_bridge::resident::{
        OwnedRequest, ResidentNotice, ResultHandoffGate, SendOutcome, SubmitVerdict,
    };
    use caligo_bridge::stop_observation::{Sample, StopIdentity};

    struct FiredHost(Arc<AtomicUsize>);
    impl HostAdapter for FiredHost {
        fn owner_thread_id(&self) -> u64 { 1 }
        fn current_thread_id(&self) -> u64 { 1 }
        fn env_valid(&self) -> bool { true }
        fn current_context_ok(&self) -> bool { true }
        fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
            Ok(match op {
                HostOp::ListenerAdd => HostOpResult::ListenerAdded { token: 7 },
                HostOp::ListenerRemove { token: 7 } => HostOpResult::ListenerRemoved,
                HostOp::SendText { request_id, .. } => {
                    self.0.fetch_add(1, Ordering::Relaxed);
                    HostOpResult::SendFired { mid: request_id }
                }
                _ => panic!("unexpected staged shutdown host operation"),
            })
        }
    }

    let _serial = TEST_LOCK.lock().unwrap();
    for mode in ["ack", "ack_timeout", "closed_channel"] {
        lab_reset();
        let (_qq, qqnt_base, env, _) = FakeQq::new();
        let handle = bootstrap(&EntryConfig { qqnt_base, env, report_path: report_path(mode) }, &fake_symbols()).unwrap();
        let stop = handle.daemon_stop_flag();
        stop.store(false, Ordering::Release);
        let finished = Arc::new(AtomicBool::new(false));
        let sends = Arc::new(AtomicUsize::new(0));
        let r = Arc::new(Resident::new(FiredHost(sends.clone()), ResidentLimits::default()));
        r.bootstrap().unwrap();
        let request = |id: &str, seq| OwnedRequest::SendText {
            request_id: id.into(), wire_seq: seq, timeout_ms: 5000,
            chat_type: 1, peer_uid: "20002".into(), text: "synthetic".into(),
        };
        assert_eq!(r.submit(request("fired", 1)), SubmitVerdict::Accepted);
        r.drain().unwrap();
        assert_eq!(r.submit(request("queued", 2)), SubmitVerdict::Accepted);
        let weak = Arc::downgrade(&r);
        let (tx, rx) = mpsc::channel();
        let mut rx = Some(rx);
        if mode == "closed_channel" {
            drop(rx.take());
        }
        retain_daemon_for_result_drain(
            r.clone(), finished.clone(), stop.clone(), Arc::new(ResultHandoffGate::default()), tx,
        );
        let owner = r.clone();
        handle.set_drain_hook(Box::new(move || close_daemon_owner(&owner)));
        // Leave only the production roots in the harness. A test-owned Arc
        // would hide a premature retirement at any of the shutdown gates.
        drop(r);
        assert_eq!(weak.strong_count(), 2);
        let identity = StopIdentity {
            account: "10001".into(), host_nonce: 7, wrapper_session: "nt_fixture".into(),
            listener_generation: 1,
        };
        let observed = identity.clone();
        install_stop_observation_driver(
            Box::new(move || Some(observed.clone())), Box::new(|_| Ok(Sample { callbacks_seen: 0 })),
        ).unwrap();
        arm_stop_observation_with_result_drain(identity, 60, 1500).unwrap();
        let (wake_tx, wake_rx) = mpsc::channel();
        *WAKE_NOTICE.lock().unwrap() = Some(wake_tx);
        let closer = std::thread::spawn(|| caligo_bridge::caligo_qq_entry_shutdown(1500));
        wake_rx.recv_timeout(LAB_WAIT).unwrap();
        assert!(!stop.load(Ordering::Acquire), "worker must survive until acknowledgement");
        assert!(cancel_stop_observation().is_err(), "a started staged close cannot be disarmed");
        pump_once();
        while FAKE_CLOSE_CALLS.load(Ordering::Acquire) == 0 {
            wake_rx.recv_timeout(LAB_WAIT).unwrap();
            pump_once();
        }
        assert_eq!(weak.strong_count(), 2, "uv_close has not yet acknowledged the handle");
        assert!(!closer.is_finished());
        assert!(!stop.load(Ordering::Acquire));
        if let Some(rx) = rx.as_ref() {
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        }
        close_once();
        {
            let resident = weak.upgrade().expect("result handoff must still hold the resident");
            assert_eq!(resident.request_state("fired"), Some("delivery_unknown"));
            assert_eq!(resident.request_state("queued"), Some("cancelled_unsent"));
            assert!(resident.complete_sends(&[("fired".into(), "late".into())], &[], &[]).is_empty());
            assert_eq!(resident.submit(request("new", 3)), SubmitVerdict::Closed);
        }

        if let Some(rx) = rx.as_ref() {
            let notices: Vec<_> = (0..3).map(|_| rx.recv_timeout(LAB_WAIT).unwrap()).collect();
            assert!(matches!(&notices[0].notice, ResidentNotice::NativeStarted { request_id } if request_id == "fired"));
            assert!(matches!(&notices[1].notice, ResidentNotice::Outcome(SendOutcome::Failure { request_id, .. }) if request_id == "queued"));
            assert!(matches!(&notices[2].notice, ResidentNotice::Outcome(SendOutcome::Unknown {
                request_id, late_receipt_possible: false, .. }) if request_id == "fired"));
            if mode == "ack" {
                let resident = weak.upgrade().expect("partial ACK must still hold the resident");
                assert!(resident.ack_retained_notice(&notices[0]));
                assert!(resident.ack_retained_notice(&notices[1]));
                assert!(!stop.load(Ordering::Acquire) && !closer.is_finished());
                assert!(resident.ack_retained_notice(&notices[2]));
                drop(resident);
                let deadline = Instant::now() + LAB_WAIT;
                while !stop.load(Ordering::Acquire) {
                    assert!(Instant::now() < deadline, "worker shutdown signal missing after ACK");
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(!closer.is_finished(), "release also waits for worker teardown");
                assert_eq!(weak.strong_count(), 2, "worker teardown gate retains both roots");
                finished.store(true, Ordering::Release);
            }
        }

        let code = closer.join().unwrap();
        assert_eq!(FAKE_CLOSE_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(sends.load(Ordering::Relaxed), 1);
        assert!(stop.load(Ordering::Acquire));
        if mode == "ack" {
            assert_eq!(code, 0);
            assert!(weak.upgrade().is_none());
        } else {
            assert_eq!(code, caligo_bridge::qq_entry_code::ERR_SHUTDOWN_PENDING_RESULTS);
            assert_eq!(weak.strong_count(), 2, "failure retains both production roots");
            assert!(weak.upgrade().unwrap().has_unacked_results());
        }
    }
}

#[test]
fn zero_current_context_aborts_before_any_host_api() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    FAKE_CURRENT.store(0, Ordering::Release); // 零 current:不得放行
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("zerocur"),
    };
    let err = bootstrap(&cfg, &fake_symbols()).unwrap_err();
    assert_eq!(err, EntryError::InterruptNoCurrentContext);
    assert_eq!(
        FAKE_INIT_CALLS.load(Ordering::Relaxed),
        0,
        "零 current 下不得 init 任何宿主资源"
    );
    FAKE_CURRENT.store(
        FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );
}

#[test]
fn stale_env_fails_freshness_gate_before_interrupt() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    // 篡改 vfptr:陈旧 env(I-03 语义)。
    // SAFETY: env 是测试持有的真实内存。
    unsafe {
        *(env as *mut u64) ^= 0xFF;
    }
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("stale"),
    };
    let err = bootstrap(&cfg, &fake_symbols()).unwrap_err();
    assert_eq!(err, EntryError::EnvNotFresh);
    assert_eq!(
        FAKE_RI_CALLS.load(Ordering::Relaxed),
        0,
        "新鲜度门先于 RequestInterrupt"
    );
}

#[test]
fn adapter_listener_ops_via_v8_ladder_and_resident_full_mode() {
    let _g = TEST_LOCK.lock().unwrap();
    lab_reset();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    fake_v8::CURRENT.store(
        FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("adapter"),
    };
    let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    let mut adapter = QqOwnerAdapter::new(handle, "10001".into());
    assert_eq!(
        adapter.owner_thread_id(),
        adapter.current_thread_id(),
        "假宿主同步首入:同线程"
    );
    // Probe:通过(新鲜度 + current)。
    assert!(adapter.native_op(HostOp::Probe).is_ok());
    // D8 监听器:经 V8 阶梯执行注册脚本 → ARMED → token 17。
    assert_eq!(
        adapter.native_op(HostOp::ListenerAdd).unwrap(),
        HostOpResult::ListenerAdded { token: 17 }
    );
    // 对称移除:STOPPED 确认。
    assert_eq!(
        adapter
            .native_op(HostOp::ListenerRemove { token: 17 })
            .unwrap(),
        HostOpResult::ListenerRemoved
    );
    // D9 发送:显式拒绝(未接线,不冒充)。
    // D9 发送已接线:装了假 V8 符号 → 阶梯执行 send 脚本 → 假机无应答队列
    // 返回 NOT_ARMED(err 字段)→ 宿主明确失败 Native{3}(NOT_ARMED = 未接线)。
    assert!(matches!(
        adapter.native_op(HostOp::SendText {
            request_id: "LAB-request".into(),
            wire_seq: 1,
            timeout_ms: 30_000,
            chat_type: 1,
            peer_uid: "p".into(),
            text: "x".into()
        }),
        Err(HostError::Native { code: 3 })
    ));

    // resident 全模式 bootstrap/close(注册 → 对称移除,资源回基线)。
    let resident = Resident::new(adapter, ResidentLimits::default());
    resident.bootstrap().unwrap();
    let report = resident.close().unwrap();
    assert_eq!(report.listener_removed, Some(true), "对称移除并确认");
    let c = resident.counters();
    assert_eq!(c.listener_adds, 1);
    assert_eq!(c.listener_removes, 1);
}

// ---- 假 V8 ABI 机器:阶梯全链可测(sret/句柄/Utf8 契约) ----

mod fake_v8 {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    static LOCALS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    pub static CURRENT: AtomicUsize = AtomicUsize::new(0x1DEA_0001);
    pub static CTX: AtomicUsize = AtomicUsize::new(0xCAFE_0002);
    pub static SCOPE_CALLS: AtomicU64 = AtomicU64::new(0);
    /// Run 的应答队列(测试按需填充;弹出顺序消费)。
    pub static RESPONSES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    /// 最近一次编译的源(断言用)。
    pub static LAST_SRC: Mutex<String> = Mutex::new(String::new());

    fn push_local(s: String) -> usize {
        let mut g = LOCALS.lock().unwrap();
        g.push(s);
        g.len()
    }
    fn get_local(h: usize) -> String {
        LOCALS.lock().unwrap()[h - 1].clone()
    }

    pub unsafe extern "C" fn scope_ctor(
        this: *mut core::ffi::c_void,
        _i: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void {
        SCOPE_CALLS.fetch_add(1, Ordering::Relaxed);
        this
    }
    pub unsafe extern "C" fn scope_dtor(_this: *mut core::ffi::c_void) {}
    pub unsafe extern "C" fn get_current() -> *mut core::ffi::c_void {
        CURRENT.load(Ordering::Acquire) as *mut core::ffi::c_void
    }
    pub unsafe extern "C" fn entered(i: *mut core::ffi::c_void, sret: *mut usize) {
        *sret = CTX.load(Ordering::Acquire);
        let _ = i;
    }
    pub unsafe extern "C" fn incumbent(i: *mut core::ffi::c_void, sret: *mut usize) {
        *sret = 0;
        let _ = i;
    }
    pub unsafe extern "C" fn new_from_utf8(
        sret: *mut usize,
        _i: *mut core::ffi::c_void,
        data: *const u8,
        _ty: i32,
        len: i32,
    ) {
        // SAFETY: 调用方保证 data/len 有效。
        let bytes = unsafe { core::slice::from_raw_parts(data, len as usize) };
        let h = push_local(String::from_utf8_lossy(bytes).into_owned());
        *sret = h as usize;
    }
    pub unsafe extern "C" fn compile(
        sret: *mut usize,
        _ctx: usize,
        src: usize,
        _origin: *mut core::ffi::c_void,
    ) {
        let s = get_local(src);
        *LAST_SRC.lock().unwrap() = s.clone();
        let h = push_local(format!("SCRIPT::{}", s));
        *sret = h as usize;
    }
    pub unsafe extern "C" fn run(this: usize, sret: *mut usize, _ctx: usize, _data: usize) {
        let script = get_local(this);
        let src = script
            .strip_prefix("SCRIPT::")
            .unwrap_or(&script)
            .to_string();
        let mut q = RESPONSES.lock().unwrap();
        let result = if !q.is_empty() {
            q.remove(0)
        } else if src.contains("addKernelMsgListener") {
            "ARMED sid=nt_3 lidRet=17".into()
        } else if src.contains("removeKernelMsgListener") {
            "STOPPED sid=nt_3 remove=0".into()
        } else {
            "NOT_ARMED".into()
        };
        drop(q);
        let h = push_local(result);
        *sret = h as usize;
    }
    // Utf8Value:this 前 8 字节 = 结果句柄。
    pub unsafe extern "C" fn utf8_ctor(
        this: *mut core::ffi::c_void,
        _i: *mut core::ffi::c_void,
        value: usize,
    ) -> *mut core::ffi::c_void {
        (this as *mut usize).write(value);
        this
    }
    pub unsafe extern "C" fn utf8_dtor(_this: *mut core::ffi::c_void) {}
    pub unsafe extern "C" fn ctx_enter(_this: *mut core::ffi::c_void) {}
    pub unsafe extern "C" fn ctx_exit(_this: *mut core::ffi::c_void) {}
    pub unsafe extern "C" fn tc_ctor(
        _this: *mut core::ffi::c_void,
        _i: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void {
        _this
    }
    pub unsafe extern "C" fn tc_dtor(_this: *mut core::ffi::c_void) {}
    pub unsafe extern "C" fn tc_caught(_this: *const core::ffi::c_void) -> bool {
        false
    }
    pub unsafe extern "C" fn tc_exc(_this: *const core::ffi::c_void, sret: *mut usize) {
        *sret = 0;
    }
    pub unsafe extern "C" fn utf8_deref(this: *mut core::ffi::c_void) -> *const u8 {
        let h = (this as *const usize).read();
        let s = get_local(h);
        // SAFETY: CString 泄漏为 LAB 常驻缓冲(测试进程生存期)。
        std::ffi::CString::new(s).unwrap().into_raw().cast::<u8>()
    }
}

fn install_fake_v8() {
    type FScopeCtor = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    type FScopeDtor = unsafe extern "C" fn(*mut core::ffi::c_void);
    type FGetCurrent = unsafe extern "C" fn() -> *mut core::ffi::c_void;
    type FCtx = unsafe extern "C" fn(*mut core::ffi::c_void, *mut usize);
    type FNewUtf8 = unsafe extern "C" fn(*mut usize, *mut core::ffi::c_void, *const u8, i32, i32);
    type FCompile = unsafe extern "C" fn(*mut usize, usize, usize, *mut core::ffi::c_void);
    type FRun = unsafe extern "C" fn(usize, *mut usize, usize, usize);
    type FUtf8Ctor = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        usize,
    ) -> *mut core::ffi::c_void;
    type FUtf8Dtor = unsafe extern "C" fn(*mut core::ffi::c_void);
    type FUtf8Deref = unsafe extern "C" fn(*mut core::ffi::c_void) -> *const u8;
    type FCtxEE = unsafe extern "C" fn(*mut core::ffi::c_void);
    type FTcCtor = unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    type FTcDtor = unsafe extern "C" fn(*mut core::ffi::c_void);
    type FTcCaught = unsafe extern "C" fn(*const core::ffi::c_void) -> bool;
    type FTcExc = unsafe extern "C" fn(*const core::ffi::c_void, *mut usize);
    let p1: FScopeCtor = fake_v8::scope_ctor;
    let p2: FScopeDtor = fake_v8::scope_dtor;
    let p3: FGetCurrent = fake_v8::get_current;
    let p4: FCtx = fake_v8::entered;
    let p5: FCtx = fake_v8::incumbent;
    let p6: FNewUtf8 = fake_v8::new_from_utf8;
    let p7: FCompile = fake_v8::compile;
    let p8: FRun = fake_v8::run;
    let p9: FUtf8Ctor = fake_v8::utf8_ctor;
    let p10: FUtf8Dtor = fake_v8::utf8_dtor;
    let p11: FUtf8Deref = fake_v8::utf8_deref;
    let p12: FCtxEE = fake_v8::ctx_enter;
    let p13: FCtxEE = fake_v8::ctx_exit;
    let p14: FTcCtor = fake_v8::tc_ctor;
    let p15: FTcDtor = fake_v8::tc_dtor;
    let p16: FTcCaught = fake_v8::tc_caught;
    let p17: FTcExc = fake_v8::tc_exc;
    caligo_bridge::qq_v8::store_symbols_raw(caligo_bridge::qq_v8::V8Symbols::from_raw([
        p1 as usize,
        p2 as usize,
        p3 as usize,
        p4 as usize,
        p5 as usize,
        p6 as usize,
        p7 as usize,
        p8 as usize,
        p9 as usize,
        p10 as usize,
        p11 as usize,
        p12 as usize,
        p13 as usize,
        p14 as usize,
        p15 as usize,
        p16 as usize,
        p17 as usize,
    ]));
}

#[test]
fn d8_v8_ladder_arm_and_stop_flow() {
    let _g = TEST_LOCK.lock().unwrap();
    caligo_bridge::qq_entry::test_reset_state();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    fake_v8::CURRENT.store(
        FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );

    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("d8arm"),
    };
    let handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    let mut adapter = caligo_bridge::qq_entry::QqOwnerAdapter::new(handle, "10001".into());

    // ListenerAdd → 阶梯执行 START_JS → ARMED → token 17。
    let r = adapter.native_op(HostOp::ListenerAdd).unwrap();
    assert_eq!(
        r,
        caligo_bridge::host_adapter::HostOpResult::ListenerAdded { token: 17 }
    );
    assert!(
        fake_v8::LAST_SRC
            .lock()
            .unwrap()
            .contains("addKernelMsgListener"),
        "阶梯执行的是监听器注册脚本"
    );

    // 轮询:NOT_ARMED 路径(无应答排队,run 走默认分支 → POLL 含 NOT_ARMED 字样)。
    let json = caligo_bridge::qq_v8::poll_listener_json().unwrap();
    assert!(json.contains("NOT_ARMED") || json.contains("items"));

    // 对称移除。
    let r = adapter
        .native_op(HostOp::ListenerRemove { token: 17 })
        .unwrap();
    assert_eq!(
        r,
        caligo_bridge::host_adapter::HostOpResult::ListenerRemoved
    );
}

#[test]
fn d8_zero_current_refuses_v8_ladder_before_scope() {
    let _g = TEST_LOCK.lock().unwrap();
    caligo_bridge::qq_entry::test_reset_state();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    fake_v8::CURRENT.store(
        FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire),
        Ordering::Release,
    );
    let cfg = EntryConfig {
        qqnt_base,
        env,
        report_path: report_path("d8zero"),
    };
    let _handle = bootstrap(&cfg, &fake_symbols()).unwrap();

    // 安静轮转点:current = 0 但 entered ctx 有效 → 阶梯可进入(K2-03 r3 语义)。
    let scope_before = fake_v8::SCOPE_CALLS.load(Ordering::Relaxed);
    fake_v8::CURRENT.store(0, Ordering::Release);
    let r = caligo_bridge::qq_v8::poll_listener_json();
    assert!(r.is_ok(), "current=0 + entered 有效 → 允许进入");
    assert!(
        fake_v8::SCOPE_CALLS.load(Ordering::Relaxed) > scope_before,
        "阶梯即上下文验证"
    );
    // 真正的拒绝条件:current 与 isolate 不一致。
    fake_v8::CURRENT.store(0xBAD_0001, Ordering::Release);
    let err = caligo_bridge::qq_v8::poll_listener_json().unwrap_err();
    assert_eq!(err, caligo_bridge::qq_v8::V8Error::NoCurrentContext);
}

#[test]
fn result_pressure_uses_fake_v8_ladder_and_never_reports_untrusted_fields() {
    let _g = TEST_LOCK.lock().unwrap();
    caligo_bridge::qq_entry::test_reset_state();
    install_fake_v8();
    let (_qq, qqnt_base, env, _loop) = FakeQq::new();
    fake_v8::CURRENT.store(FAKE_EXPECTED_ISOLATE.load(Ordering::Acquire), Ordering::Release);
    let cfg = EntryConfig { qqnt_base, env, report_path: report_path("pressure") };
    let _handle = bootstrap(&cfg, &fake_symbols()).unwrap();
    *fake_v8::RESPONSES.lock().unwrap() = vec![
        r#"{"entries":128,"over_capacity":false,"delayed":4,"secret":"DO-NOT-REPORT"}"#.into(),
        r#"{"entries":129,"over_capacity":false,"delayed":0}"#.into(),
    ];
    let detail = caligo_bridge::qq_v8::read_result_pressure("nt_3").unwrap().unwrap();
    assert!(!detail.contains("DO-NOT-REPORT"));
    let value: serde_json::Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(value["entries"], 128); assert_eq!(value["saturated"], true); assert_eq!(value["delayed"], 4);
    assert!(fake_v8::LAST_SRC.lock().unwrap().contains("pressure_session"));
    assert_eq!(caligo_bridge::qq_v8::read_result_pressure("nt_3").unwrap(), None);
    fake_v8::CURRENT.store(0xBAD_0001, Ordering::Release);
    assert_eq!(caligo_bridge::qq_v8::read_result_pressure("nt_3").unwrap_err(),
        caligo_bridge::qq_v8::V8Error::NoCurrentContext);
}
