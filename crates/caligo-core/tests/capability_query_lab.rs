//! Real Windows named pipes, synthetic host data only; no QQ calls.
#![cfg(windows)]

use caligo_core::daemon::{BridgeClient, ControlClient, Daemon, DaemonConfig};
use caligo_core::runtime::RuntimeConfig;
use caligo_model::capability::{
    FriendInfo, HostQuery, QueryCapability, QueryData, QueryOutcome, QueryScope, VoiceDownload,
    VoiceInfo,
};
use caligo_model::ipc_v3::{
    BridgeMsg, ControlMsg, CoreToBridgeMsg, CoreToControlMsg, CAPABILITY_QUERY_REVISION,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

struct Lab {
    daemon: Arc<Daemon>,
    runner: Option<std::thread::JoinHandle<()>>,
    dir: std::path::PathBuf,
}
impl Lab {
    fn new() -> Self {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(format!("caligo-query-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let daemon = Daemon::start(DaemonConfig {
            pipe_prefix: format!("caligo-query-{unique}"),
            journal_path: dir.join("journal.log"),
            module_baseline: "lab-baseline".into(),
            account: "10001".into(),
            core_build: caligo_core::CORE_BUILD.into(),
            expect_client_pid: 0,
            runtime: RuntimeConfig::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        })
        .unwrap();
        let d = daemon.clone();
        let runner = std::thread::spawn(move || d.run().unwrap());
        Self {
            daemon,
            runner: Some(runner),
            dir,
        }
    }
    fn bridge(&self) -> (BridgeClient, QueryScope) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let result = BridgeClient::connect(
                &self.daemon.pipe_names().0,
                &self.daemon.auth_token(),
                "query-lab",
                1,
                "10001",
                "lab-baseline",
                11,
                "2026-11-13T00:00:00Z",
            );
            if let Ok(client) = result {
                let scope = QueryScope {
                    account: "10001".into(),
                    host_nonce: 11,
                    session_generation: 1,
                    connection_epoch: self.daemon.connection_epoch(),
                };
                return (client, scope);
            }
            assert!(Instant::now() < deadline, "bridge connect deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn query(&self, timeout: Duration) -> std::thread::JoinHandle<QueryOutcome> {
        let d = self.daemon.clone();
        std::thread::spawn(move || d.query_host(HostQuery::FriendList, timeout))
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        self.daemon.stop_flag().store(true, Ordering::Relaxed);
        if let Some(runner) = self.runner.take() {
            runner.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn advertise(client: &mut BridgeClient, scope: &QueryScope, capabilities: Vec<QueryCapability>) {
    client
        .send(&BridgeMsg::QueryCapabilities {
            revision: CAPABILITY_QUERY_REVISION,
            scope: scope.clone(),
            capabilities,
        })
        .unwrap();
    client
        .send(&BridgeMsg::Health {
            ingress_pending: 0,
            native_ops_total: 0,
        })
        .unwrap();
    assert!(matches!(
        client.recv().unwrap(),
        CoreToBridgeMsg::HealthAck {}
    ));
}
fn request(client: &mut BridgeClient) -> (String, QueryScope) {
    match client.recv().unwrap() {
        CoreToBridgeMsg::Query {
            request_id,
            scope,
            timeout_ms,
            query,
        } => {
            assert_eq!(query, HostQuery::FriendList);
            assert!(timeout_ms > 0 && timeout_ms <= 30_000);
            (request_id, scope)
        }
        other => panic!("expected query, got {other:?}"),
    }
}
fn reply(client: &BridgeClient, request_id: String, scope: QueryScope, outcome: QueryOutcome) {
    client
        .send(&BridgeMsg::QueryResult {
            request_id,
            scope,
            outcome,
        })
        .unwrap();
}
fn empty() -> QueryOutcome {
    QueryOutcome::Success(QueryData::FriendList(vec![]))
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(Instant::now() < deadline, "condition deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn send_capabilities_are_scoped_and_cleared_on_reconnect() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    assert!(!lab.daemon.supports_private_text());

    // A mismatched advertisement must not grant support and is intentionally ignored.
    let mut wrong_scope = scope.clone();
    wrong_scope.host_nonce += 1;
    bridge
        .send(&BridgeMsg::SendCapabilities {
            scope: wrong_scope,
            private_text: true,
            group_text: true,
            private_voice: false,
        })
        .unwrap();
    bridge
        .send(&BridgeMsg::Health {
            ingress_pending: 0,
            native_ops_total: 0,
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::HealthAck {}
    ));
    assert!(!lab.daemon.supports_private_text());

    bridge
        .send(&BridgeMsg::SendCapabilities {
            scope: scope.clone(),
            private_text: true,
            group_text: false,
            private_voice: false,
        })
        .unwrap();
    bridge
        .send(&BridgeMsg::Health {
            ingress_pending: 0,
            native_ops_total: 0,
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::HealthAck {}
    ));
    assert!(lab.daemon.supports_private_text());

    drop(bridge);
    let (_reconnected, new_scope) = lab.bridge();
    assert_ne!(scope.connection_epoch, new_scope.connection_epoch);
    assert!(!lab.daemon.supports_private_text());
}

#[test]
fn private_voice_dispatch_requires_current_advertisement_and_is_never_replayed() {
    use caligo_model::ipc_v3::OutcomePayload;
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    let silk = b"\x02#!SILK_V3\x03\x00\x44\x55\x66".to_vec();
    let submit = || lab.daemon.submit_private_voice("20002", silk.clone(), 10_000);
    assert!(submit().is_err());
    let advertise_voice = |bridge: &mut BridgeClient, scope: QueryScope| {
        bridge.send(&BridgeMsg::SendCapabilities {
            scope, private_text: false, group_text: false, private_voice: true,
        }).unwrap();
        bridge.send(&BridgeMsg::Health { ingress_pending: 0, native_ops_total: 0 }).unwrap();
        assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::HealthAck {}));
    };
    let mut stale = scope.clone();
    stale.connection_epoch -= 1;
    advertise_voice(&mut bridge, stale);
    assert!(submit().is_err());
    advertise_voice(&mut bridge, scope.clone());
    assert!(!lab.daemon.supports_private_text());
    assert!(lab.daemon.submit_private_voice("20002", vec![0], 10_000).is_err());
    let request_id = submit().unwrap();
    let CoreToBridgeMsg::DispatchPrivateVoice {
        request_id: actual, scope: actual_scope, wire_seq, timeout_ms, peer_uin, silk: audio,
    } = bridge.recv().unwrap() else { panic!("expected typed voice dispatch") };
    assert_eq!(actual, request_id);
    assert_eq!(actual_scope, scope);
    assert_eq!(peer_uin, "20002");
    assert_eq!(audio, silk);
    assert!(wire_seq > 0 && (1..=10_000).contains(&timeout_ms));
    bridge.send(&BridgeMsg::NativeStarted { request_id: request_id.clone() }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::NativeStartedAck { .. }));
    bridge.send(&BridgeMsg::SendResult {
        request_id: request_id.clone(),
        outcome: OutcomePayload::Unknown { reason: Some("voice_submit_unknown".into()) },
    }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::SendResultAck { .. }));
    let target = caligo_model::SessionKey::new("10001", caligo_model::SessionKind::Private, "20002");
    assert_eq!(
        lab.daemon.onebot_text_status(&request_id, &target),
        Some((caligo_model::ActionState::DeliveryUnknown, None))
    );
    let journal = std::fs::read(lab.dir.join("journal.log")).unwrap();
    let journal = String::from_utf8_lossy(&journal);
    assert!(journal.contains(&request_id));
    assert!(!journal.contains("SILK_V3") && !journal.contains("private-voice:"));
    drop(bridge);
    let (mut bridge, new_scope) = lab.bridge();
    assert_ne!(new_scope.connection_epoch, scope.connection_epoch);
    assert!(submit().is_err());
    advertise_voice(&mut bridge, scope);
    assert!(submit().is_err());
    advertise_voice(&mut bridge, new_scope);
    assert_eq!(
        lab.daemon.onebot_text_status(&request_id, &target),
        Some((caligo_model::ActionState::DeliveryUnknown, None))
    );
}

#[test]
fn no_advertisement_unsupported_then_immediate_reply_success() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    assert_eq!(
        lab.daemon
            .query_host(HostQuery::FriendList, Duration::from_secs(1)),
        QueryOutcome::Unsupported
    );
    advertise(&mut bridge, &scope, vec![QueryCapability::FriendList]);
    assert_eq!(
        lab.daemon
            .query_host(HostQuery::GroupList, Duration::from_secs(1)),
        QueryOutcome::Unsupported
    );
    let waiter = lab.query(Duration::from_secs(60));
    let (id, returned_scope) = request(&mut bridge);
    assert_eq!(returned_scope, scope);
    assert!(id.starts_with("query-"));
    reply(&bridge, id, scope, empty());
    assert_eq!(waiter.join().unwrap(), empty());
    assert_eq!(lab.daemon.pending_query_count(), 0);
    // Pure queries do not create send request records or message projections.
    let journal = std::fs::read_to_string(lab.dir.join("journal.log")).unwrap_or_default();
    assert!(!journal.contains("query-"));
}

#[test]
fn wrong_scope_and_old_ids_are_audit_only() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    advertise(&mut bridge, &scope, vec![QueryCapability::FriendList]);
    let waiter = lab.query(Duration::from_secs(2));
    let (id, _) = request(&mut bridge);
    for field in 0..4 {
        let mut wrong = scope.clone();
        match field {
            0 => wrong.account = "20002".into(),
            1 => wrong.host_nonce += 1,
            2 => wrong.session_generation += 1,
            _ => wrong.connection_epoch += 1,
        }
        reply(&bridge, id.clone(), wrong, empty());
    }
    reply(
        &bridge,
        "unknown-old-request".into(),
        scope.clone(),
        empty(),
    );
    wait_until(|| lab.daemon.stale_query_result_count() == 5);
    assert_eq!(lab.daemon.pending_query_count(), 1);
    reply(&bridge, id.clone(), scope.clone(), empty());
    assert_eq!(waiter.join().unwrap(), empty());
    reply(&bridge, id, scope, empty());
    wait_until(|| lab.daemon.stale_query_result_count() == 6);
}

#[test]
fn revision_duplicates_and_misscoped_advertisements_fail_closed() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    let mut wrong = scope.clone();
    wrong.connection_epoch += 1;
    for (revision, advertised_scope, capabilities) in [
        (1, wrong, vec![QueryCapability::FriendList]),
        (99, scope.clone(), vec![QueryCapability::FriendList]),
        (1, scope.clone(), vec![QueryCapability::FriendList; 8]),
        (1, scope.clone(), vec![QueryCapability::FriendList; 2]),
    ] {
        bridge
            .send(&BridgeMsg::QueryCapabilities {
                revision,
                scope: advertised_scope,
                capabilities,
            })
            .unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::Reject { .. }
        ));
        assert_eq!(
            lab.daemon
                .query_host(HostQuery::FriendList, Duration::from_secs(1)),
            QueryOutcome::Unsupported
        );
    }
}

#[test]
fn timeout_cancel_and_late_result_cleanup() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    advertise(&mut bridge, &scope, vec![QueryCapability::FriendList]);
    let waiter = lab.query(Duration::from_millis(150));
    let (id, _) = request(&mut bridge);
    assert_eq!(waiter.join().unwrap(), QueryOutcome::Timeout);
    assert_eq!(lab.daemon.pending_query_count(), 0);
    reply(&bridge, id, scope.clone(), empty());
    wait_until(|| lab.daemon.stale_query_result_count() == 1);
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let d = lab.daemon.clone();
    let waiter = std::thread::spawn(move || {
        d.query_host_cancellable(HostQuery::FriendList, Duration::from_secs(2), &c)
    });
    let _ = request(&mut bridge);
    cancel.store(true, Ordering::Relaxed);
    assert!(matches!(
        waiter.join().unwrap(),
        QueryOutcome::Rejected { code: -2, .. }
    ));
    assert_eq!(lab.daemon.pending_query_count(), 0);
}

#[test]
fn disconnect_and_reconnect_clear_pending_and_capabilities() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    advertise(&mut bridge, &scope, vec![QueryCapability::FriendList]);
    let waiter = lab.query(Duration::from_secs(20));
    let (old_id, _) = request(&mut bridge);
    drop(bridge);
    assert_eq!(waiter.join().unwrap(), QueryOutcome::Disconnected);
    let (mut bridge, new_scope) = lab.bridge();
    assert_ne!(scope.connection_epoch, new_scope.connection_epoch);
    assert_eq!(
        lab.daemon
            .query_host(HostQuery::FriendList, Duration::from_secs(1)),
        QueryOutcome::Unsupported
    );
    advertise(&mut bridge, &new_scope, vec![QueryCapability::FriendList]);
    reply(&bridge, old_id, scope, empty());
    wait_until(|| lab.daemon.stale_query_result_count() == 1);
    let waiter = lab.query(Duration::from_secs(1));
    let (id, _) = request(&mut bridge);
    reply(&bridge, id, new_scope, empty());
    assert_eq!(waiter.join().unwrap(), empty());
}

#[test]
fn control_stop_immediately_disconnects_query_waiters() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    advertise(&mut bridge, &scope, vec![QueryCapability::FriendList]);
    let waiter = lab.query(Duration::from_secs(20));
    let _ = request(&mut bridge);
    let mut control = ControlClient::connect(
        &lab.daemon.pipe_names().1,
        &lab.daemon.auth_token(),
        "query-lab-control",
    )
    .unwrap();
    assert!(matches!(
        control.request(ControlMsg::Stop {}).unwrap(),
        CoreToControlMsg::Stopped {}
    ));
    assert_eq!(waiter.join().unwrap(), QueryOutcome::Disconnected);
    assert_eq!(lab.daemon.pending_query_count(), 0);
}

#[test]
fn malformed_and_over_budget_results_are_invalid_response() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    advertise(&mut bridge, &scope, vec![QueryCapability::FriendList]);
    for outcome in [
        QueryOutcome::Success(QueryData::Uin("10001".into())),
        QueryOutcome::Success(QueryData::FriendList(vec![FriendInfo {
            user_id: "10002".into(),
            nickname: "n".repeat(4097),
            remark: "".into(),
        }])),
        QueryOutcome::Rejected {
            code: 1,
            reason: "r".repeat(4097),
        },
        QueryOutcome::Success(QueryData::FriendList(
            (1..=140)
                .map(|n| FriendInfo {
                    user_id: n.to_string(),
                    nickname: "n".repeat(4096),
                    remark: "".into(),
                })
                .collect(),
        )),
    ] {
        let waiter = lab.query(Duration::from_secs(3));
        let (id, _) = request(&mut bridge);
        reply(&bridge, id, scope.clone(), outcome);
        assert_eq!(waiter.join().unwrap(), QueryOutcome::InvalidResponse);
        assert_eq!(lab.daemon.pending_query_count(), 0);
    }
}

#[test]
fn voice_download_requires_scoped_advertisement_and_exact_owned_response() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.bridge();
    let query = HostQuery::VoiceDownload {
        peer_uid: "u_voice".into(),
        message_id: "90071992547409939999".into(),
        element_id: "9007199254740994".into(),
    };
    let timeout = Duration::from_secs(3);
    advertise(&mut bridge, &scope, vec![QueryCapability::VoiceInfo]);
    assert_eq!(
        lab.daemon.query_host(query.clone(), timeout),
        QueryOutcome::Unsupported
    );
    advertise(
        &mut bridge,
        &scope,
        vec![QueryCapability::VoiceInfo, QueryCapability::VoiceDownload],
    );

    let good = VoiceDownload {
        info: VoiceInfo {
            peer_uid: "u_voice".into(),
            message_id: "90071992547409939999".into(),
            element_id: "9007199254740994".into(),
            size: 12,
            md5: "0123456789abcdef0123456789abcdef".into(),
        },
        silk_hex: "02232153494c4b5f563300ff".into(),
    };
    let mut wrong = good.clone();
    wrong.info.element_id = "99".into();
    for (data, expected) in [
        (wrong, QueryOutcome::InvalidResponse),
        (
            good.clone(),
            QueryOutcome::Success(QueryData::VoiceDownload(good)),
        ),
    ] {
        let daemon = lab.daemon.clone();
        let submitted = query.clone();
        let waiter = std::thread::spawn(move || daemon.query_host(submitted, timeout));
        let CoreToBridgeMsg::Query {
            request_id,
            scope: wire_scope,
            query: wire_query,
            ..
        } = bridge.recv().unwrap()
        else {
            panic!("missing voice download request");
        };
        assert_eq!(wire_query, query);
        assert_eq!(wire_scope, scope);
        reply(
            &bridge,
            request_id,
            scope.clone(),
            QueryOutcome::Success(QueryData::VoiceDownload(data)),
        );
        assert_eq!(waiter.join().unwrap(), expected);
    }
}
