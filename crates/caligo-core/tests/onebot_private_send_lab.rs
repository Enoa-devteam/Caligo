//! Normal private action -> durable daemon dispatch -> strong receipt and wrapper store.
#![cfg(windows)]
use caligo_core::daemon::{BridgeClient, Daemon, DaemonConfig};
use caligo_core::onebot::{RET_NOT_CONFIRMED, RET_UNSUPPORTED};
use caligo_core::onebot_server::action_handler;
use caligo_model::capability::QueryScope;
use caligo_model::ipc_v3::{BridgeMsg, CoreToBridgeMsg, EventPayload, OutcomePayload};
use caligo_model::wrapper_message::*;
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};

struct Lab {
    daemon: Arc<Daemon>,
    run: Option<std::thread::JoinHandle<()>>,
    dir: std::path::PathBuf,
}
impl Lab {
    fn new() -> Self {
        let tag = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(format!("caligo-private-{tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        let daemon = Daemon::start(DaemonConfig {
            pipe_prefix: format!("caligo-private-{tag}"),
            journal_path: dir.join("journal.log"),
            module_baseline: "lab".into(),
            account: "10001".into(),
            core_build: "lab".into(),
            expect_client_pid: std::process::id(),
            runtime: Default::default(),
            test_fault_break_bridge_after_hello: false,
            test_fault_break_bridge_after_result: false,
        })
        .unwrap();
        let d = daemon.clone();
        let run = std::thread::spawn(move || d.run().unwrap());
        Self {
            daemon,
            run: Some(run),
            dir,
        }
    }
    fn connect(&self) -> (BridgeClient, QueryScope) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(b) = BridgeClient::connect(
                &self.daemon.pipe_names().0,
                &self.daemon.auth_token(),
                "private-lab",
                1,
                "10001",
                "lab",
                99,
                "2026-11-13T00:00:00Z",
            ) {
                return (
                    b,
                    QueryScope {
                        account: "10001".into(),
                        host_nonce: 99,
                        session_generation: 1,
                        connection_epoch: self.daemon.connection_epoch(),
                    },
                );
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        self.daemon.stop_flag().store(true, Ordering::Relaxed);
        if let Some(run) = self.run.take() {
            run.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn advertise(bridge: &mut BridgeClient, scope: QueryScope) {
    bridge
        .send(&BridgeMsg::SendCapabilities {
            scope,
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
}
fn event(native_id: &str) -> EventPayload {
    let record = WrapperMessage {
        msg_id: native_id.into(),
        msg_seq: "9007199254740993".into(),
        msg_random: "2".into(),
        msg_time: "1794567890".into(),
        client_seq: Some("1".into()),
        guild_id: "local-correlation".into(),
        chat_type: 1,
        msg_type: 2,
        sub_msg_type: 0,
        peer_uid: "u_friend".into(),
        peer_uin: "20002".into(),
        sender_uid: "u_self".into(),
        sender_uin: "10001".into(),
        send_status: Some(2),
        elements: vec![WrapperElement {
            voice: None,
            element_type: 1,
            text_element: Some(WrapperText {
                content: "private text".into(),
                at_type: 0,
                at_uid: String::new(),
                at_nt_uid: String::new(),
                at_tiny_id: String::new(),
            }),
        }],
    };
    EventPayload {
        event_seq: 1,
        session_generation: 1,
        session: json!({"account":"10001","kind":"private","peer":"20002"}),
        direction: "self_sent".into(),
        sender: "10001".into(),
        native_id: native_id.into(),
        text: "private text".into(),
        platform_time: Some(1794567890),
        wrapper_message: Some(WrapperMessageEnvelope {
            binding: WrapperBinding {
                account: "10001".into(),
                session_generation: 1,
                host_nonce: 99,
                wrapper_session: "nt_fixture".into(),
            },
            wrapper_session: "nt_fixture".into(),
            source: WrapperSource::AddSend,
            observed_at_unix_ms: 1794567890000,
            record,
        }),
        onebot_projection: None,
        observed_at_unix_ms: 1794567890000,
        source: "recv".into(),
    }
}
#[test]
fn private_requires_current_send_capability_and_strong_persisted_identity() {
    let lab = Lab::new();
    let handler = action_handler(lab.daemon.clone());
    let (mut bridge, scope) = lab.connect();
    let params = json!({"user_id":20002,"message":"private text"});
    assert_eq!(
        handler("send_private_msg", &params, Value::Null).retcode,
        RET_UNSUPPORTED
    );
    let mut wrong = scope.clone();
    wrong.host_nonce += 1;
    advertise(&mut bridge, wrong);
    assert_eq!(
        handler("send_private_msg", &params, Value::Null).retcode,
        RET_UNSUPPORTED
    );
    advertise(&mut bridge, scope);
    let call = handler.clone();
    let task =
        std::thread::spawn(move || call("send_private_msg", &params, json!({"echo":"private"})));
    let request_id = match bridge.recv().unwrap() {
        CoreToBridgeMsg::Dispatch {
            request_id,
            target,
            text,
            wire_seq,
            timeout_ms,
        } => {
            assert_eq!(target["kind"], "private");
            assert_eq!(target["peer"], "20002");
            assert_eq!(text, "private text");
            assert!(wire_seq > 0 && timeout_ms > 0);
            request_id
        }
        other => panic!("{other:?}"),
    };
    bridge
        .send(&BridgeMsg::NativeStarted {
            request_id: request_id.clone(),
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::NativeStartedAck { .. }
    ));
    bridge
        .send(&BridgeMsg::Event {
            event: event("90071992547409931234"),
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::EventAck { .. }
    ));
    bridge
        .send(&BridgeMsg::SendResult {
            request_id,
            outcome: OutcomePayload::Success {
                native_id: "90071992547409931234".into(),
            },
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::SendResultAck { terminal: true, .. }
    ));
    let response = task.join().unwrap();
    assert_eq!(response.retcode, 0);
    assert_eq!(response.echo, json!({"echo":"private"}));
    let mid = response.data["message_id"].as_i64().unwrap();
    let found = handler("get_msg", &json!({"message_id":mid}), Value::Null);
    assert_eq!(found.retcode, 0);
    assert_eq!(found.data["message_type"], "private");
    assert_eq!(found.data["real_id"], mid);
}
#[test]
fn private_unknown_returns_unknown_without_a_second_dispatch() {
    let lab = Lab::new();
    let (mut bridge, scope) = lab.connect();
    advertise(&mut bridge, scope);
    let handler = action_handler(lab.daemon.clone());
    let task = std::thread::spawn(move || {
        handler(
            "send_private_msg",
            &json!({"user_id":20002,"message":"private text"}),
            Value::Null,
        )
    });
    let id = match bridge.recv().unwrap() {
        CoreToBridgeMsg::Dispatch { request_id, .. } => request_id,
        other => panic!("{other:?}"),
    };
    bridge
        .send(&BridgeMsg::NativeStarted {
            request_id: id.clone(),
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::NativeStartedAck { .. }
    ));
    bridge
        .send(&BridgeMsg::SendResult {
            request_id: id,
            outcome: OutcomePayload::Unknown {
                reason: Some("readback timeout".into()),
            },
        })
        .unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::SendResultAck { terminal: true, .. }
    ));
    assert_eq!(task.join().unwrap().retcode, RET_NOT_CONFIRMED);
    bridge
        .send(&BridgeMsg::Health {
            ingress_pending: 0,
            native_ops_total: 1,
        })
        .unwrap();
    assert!(
        matches!(bridge.recv().unwrap(), CoreToBridgeMsg::HealthAck {}),
        "unknown must not enqueue another send"
    );
}
