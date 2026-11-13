//! Normal private action -> durable daemon dispatch -> strong receipt and wrapper store.
#![cfg(windows)]
use caligo_core::daemon::{BridgeClient, Daemon, DaemonConfig};
use caligo_core::onebot::{RET_BAD_PARAMS, RET_NOT_CONFIRMED, RET_UNSUPPORTED};
use caligo_core::onebot_server::action_handler;
use caligo_model::capability::{HostQuery, QueryCapability, QueryData, QueryOutcome, QueryScope, VoiceDownload};
use caligo_model::ipc_v3::{BridgeMsg, CoreToBridgeMsg, EventPayload, OutcomePayload};
use caligo_model::onebot_traits::StoreReader;
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
fn advertise(bridge: &mut BridgeClient, scope: QueryScope, private_voice: bool) {
    bridge
        .send(&BridgeMsg::SendCapabilities {
            scope,
            private_text: true,
            group_text: true,
            private_voice,
            group_voice: false,
            private_image: false,
            group_image: false,
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
            image: None,
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
    advertise(&mut bridge, wrong, false);
    assert_eq!(
        handler("send_private_msg", &params, Value::Null).retcode,
        RET_UNSUPPORTED
    );
    advertise(&mut bridge, scope, false);
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
    advertise(&mut bridge, scope, false);
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

#[test]
fn private_record_waits_for_exact_readback_to_be_persisted() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let lab = Lab::new();
    let (mut bridge, scope) = lab.connect();
    let handler = action_handler(lab.daemon.clone());
    let silk = b"\x02#!SILK_V3\x03\x00\x44\x55\x66";
    let message = json!([{"type":"record","data":{"file":format!("base64://{}",STANDARD.encode(silk))}}]);
    let params = json!({"user_id":20002, "message":message});
    assert_eq!(handler("send_private_msg", &params, Value::Null).retcode, RET_UNSUPPORTED);
    advertise(&mut bridge, scope.clone(), true);
    for bad in ["https://example.invalid/audio.silk", "C:/audio.silk", "base64://not-encoded"] {
        let params = json!({"user_id":20002,"message":[{"type":"record","data":{"file":bad}}]});
        assert_eq!(handler("send_private_msg", &params, Value::Null).retcode, RET_BAD_PARAMS);
    }
    assert_eq!(handler("send_group_msg", &json!({"group_id":30003,"message":message}), Value::Null).retcode,
        RET_UNSUPPORTED);
    assert_eq!(handler("send_private_msg", &json!({"user_id":20002,"message":[
        {"type":"text","data":{"text":"mixed"}}, message[0],
    ]}), Value::Null).retcode, RET_BAD_PARAMS);
    let call = handler.clone();
    let task = std::thread::spawn(move || call("send_msg", &params, json!("voice")));
    let CoreToBridgeMsg::DispatchPrivateVoice {
        request_id, scope: dispatch_scope, wire_seq, peer_uin, silk: audio, ..
    } = bridge.recv().unwrap() else { panic!("expected private voice dispatch") };
    assert_eq!(dispatch_scope, scope);
    assert_eq!(peer_uin, "20002");
    assert_eq!(audio, silk);
    assert!(wire_seq > 0);
    bridge.send(&BridgeMsg::NativeStarted { request_id: request_id.clone() }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::NativeStartedAck { .. }));
    let native_id = "90071992547409931234";
    bridge.send(&BridgeMsg::SendResult {
        request_id, outcome: OutcomePayload::Success { native_id: native_id.into() },
    }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::SendResultAck { terminal: true, .. }));
    assert!(!task.is_finished(), "the receipt alone must not complete the action");

    let mut readback = event(native_id);
    readback.text.clear();
    readback.wrapper_message.as_mut().unwrap().record.elements = vec![WrapperElement {
            image: None,
        element_type: 4,
        text_element: None,
        voice: Some(WrapperVoice {
            element_id: "90071992547409931235".into(), size: silk.len() as u32,
            md5: "0123456789abcdef0123456789abcdef".into(),
        }),
    }];
    let mut group = readback.clone();
    group.event_seq = 2;
    group.session = json!({"account":"10001","kind":"group","peer":"30003"});
    group.direction = "incoming".into();
    group.sender = "20002".into();
    let wrapper = group.wrapper_message.as_mut().unwrap();
    wrapper.source = WrapperSource::Recv;
    wrapper.record.chat_type = 2;
    wrapper.record.peer_uid = "30003".into();
    wrapper.record.peer_uin = "30003".into();
    wrapper.record.sender_uid = "u_friend".into();
    wrapper.record.sender_uin = "20002".into();
    wrapper.record.send_status = None;
    bridge.send(&BridgeMsg::Event { event: readback }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::EventAck { suppressed: false, .. }));
    let result = task.join().unwrap();
    assert_eq!(result.retcode, 0);
    assert_eq!(result.echo, "voice");
    let id = &result.data["message_id"];
    let read = handler("get_msg", &json!({"message_id":id}), Value::Null);
    let token = format!("caligo-voice:10001:{}", id.as_i64().unwrap());
    assert_eq!(read.data["message"], json!([{"type":"record","data":{"file":token}}]));
    assert_eq!(handler("get_record", &json!({"file":token}), Value::Null).retcode, RET_UNSUPPORTED);
    let reopened = caligo_core::message_store::MessageStore::open(
        lab.dir.join("journal.messages.jsonl"), 10001, None,
    ).unwrap();
    assert_eq!(
        reopened.get_msg(id.as_i64().unwrap() as i32).unwrap().message,
        read.data["message"],
    );
    bridge.send(&BridgeMsg::QueryCapabilities {
        revision: caligo_model::ipc_v3::CAPABILITY_QUERY_REVISION,
        scope: scope.clone(), capabilities: vec![QueryCapability::ExactVoiceDownload],
    }).unwrap();
    let (_, events) = lab.daemon.subscribe_onebot_events("voice-lab");
    bridge.send(&BridgeMsg::Event { event: group }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::EventAck { suppressed: false, .. }));
    let group_event: Value = serde_json::from_str(&events.recv_timeout(Duration::from_secs(1)).unwrap()).unwrap();
    assert_eq!(group_event["message_type"], "group");
    assert_eq!(group_event["group_id"], 30003);
    assert_eq!(group_event["user_id"], 20002);
    let group_token = group_event["message"][0]["data"]["file"].as_str().unwrap();
    assert_ne!(group_token, token);
    for (file, chat_type, peer_uid, sender_uin) in [
        (token.as_str(), 1, "u_friend", "10001"),
        (group_token, 2, "30003", "20002"),
    ] {
        let info = lab.daemon.voice_info(file).unwrap();
        let call = handler.clone();
        let params = json!({"file":file});
        let task = std::thread::spawn(move || call("get_record", &params, Value::Null));
        let CoreToBridgeMsg::Query { request_id, scope: dispatch_scope, query, .. } =
            bridge.recv().unwrap() else { panic!("expected exact voice download") };
        assert_eq!(dispatch_scope, scope);
        assert_eq!(query, HostQuery::ExactVoiceDownload {
            chat_type, peer_uid: peer_uid.into(), sender_uin: sender_uin.into(),
            message_id: info.message_id.clone(), element_id: info.element_id.clone(),
        });
        let silk_hex = silk.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        bridge.send(&BridgeMsg::QueryResult { request_id, scope: scope.clone(),
            outcome: QueryOutcome::Success(QueryData::VoiceDownload(VoiceDownload { info, silk_hex })),
        }).unwrap();
        let response = task.join().unwrap();
        assert_eq!(response.retcode, 0);
        assert_eq!(response.data["base64"], STANDARD.encode(silk));
    }
}

#[test]
fn group_record_requires_group_capability_and_exact_group_projection() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let lab = Lab::new();
    let (mut bridge, scope) = lab.connect();
    let handler = action_handler(lab.daemon.clone());
    let silk = b"\x02#!SILK_V3\x01\x00\x42";
    let message = json!([{"type":"record","data":{"file":format!("base64://{}",STANDARD.encode(silk))}}]);
    let params = json!({"group_id":30003, "message":message});
    advertise(&mut bridge, scope.clone(), true);
    assert_eq!(handler("send_group_msg", &params, Value::Null).retcode, RET_UNSUPPORTED);
    bridge.send(&BridgeMsg::SendCapabilities {
        scope: scope.clone(), private_text: false, group_text: false, private_voice: false, group_voice: true,

            private_image: false, group_image: false,
        }).unwrap();
    bridge.send(&BridgeMsg::Health { ingress_pending: 0, native_ops_total: 0 }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::HealthAck {}));
    assert_eq!(handler("send_private_msg", &json!({"user_id":30003,"message":message}), Value::Null).retcode, RET_UNSUPPORTED);

    let call = handler.clone();
    let task = std::thread::spawn(move || call("send_group_msg", &params, json!("group")));
    let CoreToBridgeMsg::DispatchGroupVoice { request_id, scope: actual, group_id, silk: audio, .. } =
        bridge.recv().unwrap() else { panic!("expected group voice dispatch") };
    assert_eq!(actual, scope);
    assert_eq!(group_id, "30003");
    assert_eq!(audio, silk);
    let native_id = "90071992547409931234";
    bridge.send(&BridgeMsg::NativeStarted { request_id: request_id.clone() }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::NativeStartedAck { .. }));
    bridge.send(&BridgeMsg::SendResult {
        request_id, outcome: OutcomePayload::Success { native_id: native_id.into() },
    }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::SendResultAck { terminal: true, .. }));

    let mut readback = event(native_id);
    readback.text.clear();
    let wrapper = readback.wrapper_message.as_mut().unwrap();
    wrapper.record.elements = vec![WrapperElement {
            image: None,
        element_type: 4, text_element: None, voice: Some(WrapperVoice {
            element_id: "90071992547409931235".into(), size: silk.len() as u32,
            md5: "0123456789abcdef0123456789abcdef".into(),
        }),
    }];
    bridge.send(&BridgeMsg::Event { event: readback.clone() }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::EventAck { suppressed: false, .. }));
    assert!(!task.is_finished(), "the same native ID in a private conversation cannot satisfy a group send");

    readback.event_seq = 2;
    readback.session = json!({"account":"10001","kind":"group","peer":"30003"});
    let wrapper = readback.wrapper_message.as_mut().unwrap();
    wrapper.record.chat_type = 2;
    wrapper.record.peer_uid = "30003".into();
    wrapper.record.peer_uin = "30003".into();
    bridge.send(&BridgeMsg::Event { event: readback }).unwrap();
    assert!(matches!(bridge.recv().unwrap(), CoreToBridgeMsg::EventAck { suppressed: false, .. }));
    let result = task.join().unwrap();
    assert_eq!(result.retcode, 0);
    assert_eq!(result.echo, "group");
    let read = handler("get_msg", &json!({"message_id":result.data["message_id"]}), Value::Null);
    assert_eq!(read.data["message_type"], "group");
    assert_eq!(read.data["message"][0]["type"], "record");
    let file = read.data["message"][0]["data"]["file"].as_str().unwrap();
    let (_, query) = lab.daemon.voice_record(file).unwrap();
    assert_eq!(query, HostQuery::ExactVoiceDownload {
        chat_type: 2,
        peer_uid: "30003".into(),
        sender_uin: "10001".into(),
        message_id: native_id.into(),
        element_id: "90071992547409931235".into(),
    });
}

#[test]
fn png_send_requires_scoped_capability_and_persisted_private_or_group_readback() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use caligo_model::capability::ImageInfo;
    let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQVDL+DwACFAFmBODefwAAAABJRU5ErkJggg==").unwrap();
    for chat_type in [1, 2] {
        let lab = Lab::new();
        let (mut bridge, scope) = lab.connect();
        let handler = action_handler(lab.daemon.clone());
        let message =
            json!([{"type":"image","data":{"file":format!("base64://{}",STANDARD.encode(&png))}}]);
        let params = if chat_type == 1 {
            json!({"user_id":20002,"message":message})
        } else {
            json!({"group_id":30003,"message":message})
        };
        let action = if chat_type == 1 {
            "send_private_msg"
        } else {
            "send_group_msg"
        };
        assert_eq!(
            handler(action, &params, Value::Null).retcode,
            RET_UNSUPPORTED
        );
        // Grant only the opposite route: media and chat capabilities are independent.
        for enabled in [false, true] {
            bridge
                .send(&BridgeMsg::SendCapabilities {
                    scope: scope.clone(),
                    private_text: true,
                    group_text: true,
                    private_voice: true,
                    group_voice: true,
                    private_image: if enabled {
                        chat_type == 1
                    } else {
                        chat_type == 2
                    },
                    group_image: if enabled {
                        chat_type == 2
                    } else {
                        chat_type == 1
                    },
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
            if !enabled {
                assert_eq!(
                    handler(action, &params, Value::Null).retcode,
                    RET_UNSUPPORTED
                );
            }
        }
        for file in [
            "https://example.invalid/image.png",
            "C:/image.png",
            "base64://not-png",
            "base64://",
        ] {
            let mut bad = params.clone();
            bad["message"][0]["data"]["file"] = json!(file);
            assert_eq!(handler(action, &bad, Value::Null).retcode, RET_BAD_PARAMS);
        }
        let mut mixed = params.clone();
        mixed["message"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"text","data":{"text":"mixed"}}));
        assert_eq!(handler(action, &mixed, Value::Null).retcode, RET_BAD_PARAMS);
        let call = handler.clone();
        let task = std::thread::spawn(move || call("send_msg", &params, json!("image")));
        let CoreToBridgeMsg::DispatchImage {
            request_id,
            scope: dispatch_scope,
            wire_seq,
            peer_uin,
            chat_type: dispatched_chat,
            png: bytes,
            timeout_ms,
        } = bridge.recv().unwrap()
        else {
            panic!("expected image dispatch")
        };
        assert_eq!(dispatch_scope, scope);
        assert_eq!(dispatched_chat, chat_type);
        assert_eq!(peer_uin, if chat_type == 1 { "20002" } else { "30003" });
        assert_eq!(bytes, png);
        assert!(wire_seq > 0 && (1..=30_000).contains(&timeout_ms));
        bridge
            .send(&BridgeMsg::NativeStarted {
                request_id: request_id.clone(),
            })
            .unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::NativeStartedAck { .. }
        ));
        let native_id = "90071992547409931234";
        bridge
            .send(&BridgeMsg::SendResult {
                request_id,
                outcome: OutcomePayload::Success {
                    native_id: native_id.into(),
                },
            })
            .unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::SendResultAck { terminal: true, .. }
        ));
        assert!(
            !task.is_finished(),
            "native receipt alone cannot mint a public ID"
        );
        let mut readback = event(native_id);
        readback.text.clear();
        if chat_type == 2 {
            readback.session = json!({"account":"10001","kind":"group","peer":"30003"});
        }
        let wrapper = readback.wrapper_message.as_mut().unwrap();
        wrapper.record.chat_type = chat_type;
        if chat_type == 2 {
            wrapper.record.peer_uid = "30003".into();
            wrapper.record.peer_uin = "30003".into();
        }
        wrapper.record.elements = vec![WrapperElement {
            element_type: 2,
            text_element: None,
            voice: None,
            image: Some(ImageInfo {
                chat_type,
                peer_uid: wrapper.record.peer_uid.clone(),
                sender_uin: "10001".into(),
                message_id: native_id.into(),
                element_id: "90071992547409931235".into(),
                md5: "e605ef58c3bb5f33463782ab90d58295".into(),
                size: png.len() as u32,
                width: 1,
                height: 1,
                pic_type: 1000,
            }),
        }];
        bridge.send(&BridgeMsg::Event { event: readback }).unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::EventAck {
                suppressed: false,
                ..
            }
        ));
        let result = task.join().unwrap();
        assert_eq!(result.retcode, 0, "{result:?}");
        assert_eq!(result.echo, "image");
        let id = result.data["message_id"].as_i64().unwrap() as i32;
        let token = format!("caligo-image:10001:{id}");
        let expected = json!([{"type":"image","data":{"file":token}}]);
        assert_eq!(
            handler("get_msg", &json!({"message_id":id}), Value::Null).data["message"],
            expected
        );
        assert_eq!(
            handler("get_image", &json!({"file":token}), Value::Null).retcode,
            RET_UNSUPPORTED
        );
        let reopened = caligo_core::message_store::MessageStore::open(
            lab.dir.join("journal.messages.jsonl"),
            10001,
            None,
        )
        .unwrap();
        assert_eq!(reopened.get_msg(id).unwrap().message, expected);
        let bytes = std::fs::read(lab.dir.join("journal.log")).unwrap();
        assert!(!bytes.windows(png.len()).any(|chunk| chunk == png));
        let journal = String::from_utf8_lossy(&bytes);
        assert!(
            !journal.contains(&STANDARD.encode(&png)),
            "journal must not retain payload bytes"
        );
    }
}
