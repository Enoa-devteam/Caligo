//! Real Windows pipes and production D8 decoding, using synthetic PNG metadata only.
#![cfg(all(windows, feature = "research"))]

use caligo_bridge::daemon_client::{event_payload, DaemonClientConfig};
use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{Resident, ResidentLimits};
use caligo_core::daemon::{BridgeClient, Daemon, DaemonConfig};
use caligo_core::runtime::RuntimeConfig;
use caligo_model::capability::{
    HostQuery, ImageDownload, QueryCapability, QueryData, QueryOutcome, QueryScope,
};
use caligo_model::ipc_v3::{BridgeMsg, CoreToBridgeMsg, CAPABILITY_QUERY_REVISION};
use caligo_model::onebot_traits::StoreReader;
use caligo_model::wrapper_message::WrapperBinding;
use serde_json::{json, Value};
use std::time::Duration;

const PNG_HEX: &str = "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";

struct OfflineHost;
impl HostAdapter for OfflineHost {
    fn owner_thread_id(&self) -> u64 {
        1
    }
    fn current_thread_id(&self) -> u64 {
        1
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError> {
        match op {
            HostOp::ListenerAdd => Ok(HostOpResult::ListenerAdded { token: 1 }),
            HostOp::ListenerRemove { .. } => Ok(HostOpResult::ListenerRemoved),
            _ => panic!("receive-only fixture must not send"),
        }
    }
}

fn batch() -> String {
    let script = format!(
        "const arm={},poll={};\n",
        serde_json::to_string(caligo_bridge::qq_v8::D8_START_JS).unwrap(),
        serde_json::to_string(caligo_bridge::qq_v8::D8_POLL_JS).unwrap()
    ) + r#"
const assert=require('node:assert/strict');let listener;
const service={sendMsg(){throw Error('no send');},addKernelMsgListener(v){listener=v;return 7;}};
const session={getSessionId:()=>1,getMsgService:()=>service};
process._linkedBinding=()=>({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}});
const caligoVoiceAccount='10001';assert.ok(eval(arm).startsWith('ARMED'));
function record(chatType,peerUid,peerUin){return {msgId:'90071992547409939999',msgSeq:'1',msgRandom:'2',
  msgTime:'1794567890',guildId:'',chatType,msgType:2,subMsgType:0,peerUid,peerUin,
  senderUid:'u_friend',senderUin:'20002',elements:[{elementType:2,elementId:'9007199254740994',
  picElement:{fileSize:'70',picWidth:1,picHeight:1,picType:1001,picSubType:0,
  md5HexStr:'E605EF58C3BB5F33463782AB90D58295',filePath:'private-path',sourcePath:'private-source',fileUuid:'private-token'}}]};}
const bad=record(1,'u_friend','20002');bad.elements[0].picElement.picSubType=1;
const mixed=record(1,'u_friend','20002');mixed.elements.push({elementType:1,textElement:{content:'mixed',atType:0}});
listener.onRecvMsg([bad,mixed,record(1,'u_friend','20002'),record(2,'30003','30003')]);
const result=eval(poll);assert.ok(!result.includes('private-'));process.stdout.write(result);
"#;
    let out = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", "-e", &script])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn png_identity_events_get_image_and_restart_share_the_same_account_token() {
    let resident = Resident::new(OfflineHost, ResidentLimits::default());
    resident.bootstrap().unwrap();
    let binding = WrapperBinding {
        account: "10001".into(),
        session_generation: 1,
        host_nonce: 9,
        wrapper_session: "nt_0".into(),
    };
    let result = resident.ingest_wrapper_batch(&batch(), &binding).unwrap();
    assert_eq!(result.stats.rejected, 2);
    let events = resident.take_events();
    assert_eq!(events.len(), 2);
    let mut wrong = events[0].wrapper_message.clone().unwrap();
    wrong.record.elements[0].image.as_mut().unwrap().sender_uin = "999".into();
    assert!(wrong.validate().is_err());
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(format!("caligo-png-{unique}"));
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("journal.log");
    let daemon = Daemon::start(DaemonConfig {
        pipe_prefix: format!("caligo-png-{unique}"),
        journal_path: journal.clone(),
        module_baseline: "lab".into(),
        account: "10001".into(),
        core_build: "lab".into(),
        expect_client_pid: 0,
        runtime: RuntimeConfig::default(),
        test_fault_break_bridge_after_hello: false,
        test_fault_break_bridge_after_result: false,
    })
    .unwrap();
    let (_, ws) = daemon.subscribe_onebot_events("image-ws");
    let (_, sse) = daemon.subscribe_onebot_events("image-sse");
    let run = daemon.clone();
    let runner = std::thread::spawn(move || run.run().unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut bridge = loop {
        if let Ok(b) = BridgeClient::connect(
            &daemon.pipe_names().0,
            &daemon.auth_token(),
            "image-lab",
            1,
            "10001",
            "lab",
            9,
            "fixture",
        ) {
            break b;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    let cfg = DaemonClientConfig {
        account: "10001".into(),
        session_generation: 1,
        host_nonce: 9,
        ..Default::default()
    };
    let handler = caligo_core::onebot_server::action_handler(daemon.clone());
    let mut saved = vec![];
    for (i, event) in events.iter().enumerate() {
        let payload = event_payload(&cfg, event, i as u64 + 1).unwrap();
        bridge
            .send(&BridgeMsg::Event {
                event: payload.clone(),
            })
            .unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::EventAck {
                suppressed: false,
                ..
            }
        ));
        bridge.send(&BridgeMsg::Event { event: payload }).unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::EventAck {
                suppressed: true,
                ..
            }
        ));
        let raw = ws.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(sse.recv_timeout(Duration::from_secs(2)).unwrap(), raw);
        let public: Value = serde_json::from_str(&raw).unwrap();
        let id = public["message_id"].as_i64().unwrap() as i32;
        assert_eq!(public["message"][0]["type"], "image");
        assert_eq!(public["user_id"], 20002);
        assert_eq!(
            public["message_type"],
            if i == 0 { "private" } else { "group" }
        );
        assert_eq!(daemon.get_msg(id).unwrap().message, public["message"]);
        let token = public["message"][0]["data"]["file"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(public["raw_message"], format!("[CQ:image,file={token}]"));
        let info = daemon.image_info(&token).unwrap();
        assert_eq!(info.message_id, "90071992547409939999");
        assert!(daemon
            .image_info(&token.replace("10001", "30003"))
            .is_none());
        saved.push((id, token, info));
    }
    assert_ne!(
        saved[0].0, saved[1].0,
        "same native ID in two chats has two durable identities"
    );
    assert!(ws.try_recv().is_err());
    for params in [
        json!({}),
        json!({"file":3}),
        json!({"file":"x","file_id":"y"}),
    ] {
        assert_eq!(handler("get_image", &params, Value::Null).retcode, 1400);
    }
    assert_eq!(
        handler(
            "get_image",
            &json!({"file":"caligo-image:30003:1"}),
            Value::Null
        )
        .retcode,
        1440
    );
    let scope = QueryScope {
        account: "10001".into(),
        host_nonce: 9,
        session_generation: 1,
        connection_epoch: daemon.connection_epoch(),
    };
    assert_eq!(
        handler("get_image", &json!({"file":saved[0].1}), Value::Null).retcode,
        1404
    );
    bridge
        .send(&BridgeMsg::QueryCapabilities {
            revision: CAPABILITY_QUERY_REVISION,
            scope: scope.clone(),
            capabilities: vec![QueryCapability::ImageDownload],
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
    for (index, wrong_md5) in [(0, true), (0, false), (1, false)] {
        let (_, token, info) = &saved[index];
        let info = info.clone();
        let token = token.clone();
        let call = handler.clone();
        let waiter = std::thread::spawn(move || {
            call("get_image", &json!({"file_id":token}), json!("image"))
        });
        let CoreToBridgeMsg::Query {
            request_id,
            scope: actual_scope,
            query,
            ..
        } = bridge.recv().unwrap()
        else {
            panic!("image query missing")
        };
        assert_eq!(actual_scope, scope);
        assert_eq!(
            query,
            HostQuery::ImageDownload {
                chat_type: info.chat_type,
                peer_uid: info.peer_uid.clone(),
                sender_uin: info.sender_uin.clone(),
                message_id: info.message_id.clone(),
                element_id: info.element_id.clone()
            }
        );
        let mut returned = info;
        if wrong_md5 {
            returned.md5 = "f".repeat(32);
        }
        bridge
            .send(&BridgeMsg::QueryResult {
                request_id,
                scope: scope.clone(),
                outcome: QueryOutcome::Success(QueryData::ImageDownload(ImageDownload {
                    info: returned,
                    png_hex: PNG_HEX.into(),
                })),
            })
            .unwrap();
        let response = waiter.join().unwrap();
        assert_eq!(response.echo, "image");
        if wrong_md5 {
            assert_eq!(response.retcode, 1440);
        } else {
            assert_eq!(response.retcode, 0);
            assert_eq!(response.data["file_size"], "70");
            assert_eq!(response.data["file_name"], "image.png");
            assert!(response.data["base64"]
                .as_str()
                .unwrap()
                .starts_with("iVBORw0KGgo"));
            assert!(response.data.get("file").is_none());
            assert!(response.data.get("url").is_none());
        }
    }
    daemon
        .stop_flag()
        .store(true, std::sync::atomic::Ordering::Relaxed);
    drop(bridge);
    runner.join().unwrap();
    drop(handler);
    drop(daemon);
    let store = caligo_core::message_store::MessageStore::open(
        journal.with_extension("messages.jsonl"),
        10001,
        None,
    )
    .unwrap();
    let (recovered, report) = caligo_core::runtime::AccountActor::open_with_message_store(
        &journal,
        RuntimeConfig::default(),
        store,
    )
    .unwrap();
    assert_eq!(report.recovered_events, 2);
    for (id, token, info) in saved {
        assert_eq!(recovered.image_info(&token), Some(info));
        assert_eq!(
            recovered.get_msg(id).unwrap().message[0]["data"]["file"],
            token
        );
    }
    assert!(
        !std::fs::read_to_string(journal.with_extension("messages.jsonl"))
            .unwrap()
            .contains("private-")
    );
    drop(recovered);
    std::fs::remove_dir_all(dir).unwrap();
}
