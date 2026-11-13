//! Offline production D8 script -> owner decoder -> resident -> IPC -> daemon journal.
#![cfg(feature = "research")]

use caligo_bridge::daemon_client::{event_payload, DaemonClientConfig};
use caligo_bridge::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult};
use caligo_bridge::resident::{Resident, ResidentLimits};
use caligo_core::daemon::{BridgeClient, ControlClient, Daemon, DaemonConfig};
use caligo_core::ipc::{BridgeMsg, ControlMsg, CoreToBridgeMsg, CoreToControlMsg};
use caligo_core::runtime::{AccountActor, RuntimeConfig};
use caligo_model::wrapper_message::WrapperBinding;
use std::time::Duration;

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
            _ => panic!("receive-only test must not submit a message"),
        }
    }
}

fn script_batch() -> String {
    let start = serde_json::to_string(caligo_bridge::qq_v8::D8_START_JS).unwrap();
    let poll = serde_json::to_string(caligo_bridge::qq_v8::D8_POLL_JS).unwrap();
    let fixture = r#"
const assert = require('node:assert/strict');
let listener;
const service = {sendMsg() { throw Error('must not send'); },
  addKernelMsgListener(value) { listener = value; return 1; }};
const session = {getSessionId: () => '1', getMsgService: () => service};
process._linkedBinding = name => ({NodeIQQNTWrapperSession:{getNTWrapperSession: () => session}});
assert.ok(eval(start).startsWith('ARMED'));
function record(id) { return {msgId:id, msgSeq:'9007199254740993', msgRandom:'18446744073709551615',
  msgTime:'1794567890', clientSeq:'9007199254740995', guildId:'', chatType:1, msgType:2, subMsgType:0,
  peerUid:'u_friend', peerUin:'20002', senderUid:'u_friend', senderUin:'20002',
  elements:[{elementType:1,textElement:{content:'a'.repeat(10000)+'完整🙂',atType:0}}]}; }
const bad = record('3'); delete bad.senderUin;
const media = record('4'); media.elements = [{elementType:2,picElement:{}}];
const oversize = record('5'); oversize.elements[0].textElement.content = 'x'.repeat(50000);
listener.onRecvMsg([bad, media, oversize, record('90071992547409931234')]);
const self = record('90071992547409931235'); self.senderUin='10001'; self.senderUid='u_self';
self.sendStatus=1; listener.onAddSendMsg(self);
self.sendStatus=2; listener.onMsgInfoListUpdate([self]);
// Present-but-invalid identity is counted by the Rust decoder, without blocking the next row.
const empty = record('6'); empty.senderUin=''; listener.onRecvMsg([empty,record('90071992547409931236')]);
const first = JSON.parse(eval(poll));
assert.equal(first.rejected,2); assert.equal(first.overflow,1);
assert.equal(JSON.parse(eval(poll)).items.length,0);
// Item limit is explicit, not truncation; retain all admitted records byte-for-byte.
for(let i=0;i<130;i++) { const r=record(String(100+i)); r.elements[0].textElement.content='small'; listener.onRecvMsg([r]); }
const full=JSON.parse(eval(poll)); assert.equal(full.items.length,128); assert.equal(full.overflow,2);
process.stdout.write(JSON.stringify(first));
"#;
    let script = format!("const start={start}, poll={poll};\n{fixture}");
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
fn d8_owned_records_reach_journal_without_projection_and_old_generations_are_audited() {
    let json = script_batch();
    let resident = Resident::new(OfflineHost, ResidentLimits::default());
    resident.bootstrap().unwrap();
    let binding = WrapperBinding {
        account: "10001".into(),
        session_generation: 1,
        host_nonce: 9,
        wrapper_session: "nt_0".into(),
    };
    let batch = resident.ingest_wrapper_batch(&json, &binding).unwrap();
    assert_eq!(batch.stats.rejected, 3);
    assert_eq!(batch.stats.overflow, 1);
    assert_eq!(resident.counters().wrapper_records_rejected, 3);
    assert_eq!(resident.counters().wrapper_records_overflow, 1);
    let events = resident.take_events();
    assert_eq!(events.len(), 4);
    assert_ne!(
        events[1].source, events[2].source,
        "add and update occupy independent dedup classes"
    );
    let mut wrong_sid = binding.clone();
    wrong_sid.wrapper_session = "nt_other".into();
    assert!(resident.ingest_wrapper_batch(&json, &wrong_sid).is_err());
    assert_eq!(events[0].native_id, "90071992547409931234");
    assert_eq!(events[0].text, "a".repeat(10000) + "完整🙂");
    assert_eq!(events[0].msg_time, Some(1794567890));
    assert!(resident.ingest_wrapper_batch("broken", &binding).is_err());
    assert_eq!(resident.counters().wrapper_batches_rejected, 2);

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("caligo-wrapper-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("journal.log");
    let prefix = format!("\\\\.\\pipe\\caligo-wrapper-{}-{nonce}", std::process::id());
    let daemon = Daemon::start(DaemonConfig {
        pipe_prefix: prefix.clone(),
        journal_path: journal.clone(),
        module_baseline: "lab-wrapper".into(),
        account: "10001".into(),
        core_build: "lab".into(),
        expect_client_pid: std::process::id(),
        runtime: RuntimeConfig::default(),
        test_fault_break_bridge_after_hello: false,
        test_fault_break_bridge_after_result: false,
    })
    .unwrap();
    let run = daemon.clone();
    let runner = std::thread::spawn(move || run.run());
    let (bridge_name, control_name) = daemon.pipe_names();
    let auth = daemon.auth_token();
    let mut bridge = None;
    for _ in 0..100 {
        if let Ok(c) = BridgeClient::connect(
            &bridge_name,
            &auth,
            "lab",
            2,
            "10001",
            "lab-wrapper",
            9,
            "fixture",
        ) {
            bridge = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut bridge = bridge.expect("bridge ready");
    let cfg = DaemonClientConfig {
        account: "10001".into(),
        session_generation: 1,
        host_nonce: 9,
        ..Default::default()
    };
    let mut current = cfg.clone();
    current.session_generation = 2;
    for (i, ev) in events.iter().enumerate() {
        // Reject cross-account/host/generation rebinding before wire emission.
        assert!(event_payload(&current, ev, 1).is_err());
        let mut wrong = cfg.clone();
        wrong.account = "30003".into();
        assert!(event_payload(&wrong, ev, 1).is_err());
        wrong = cfg.clone();
        wrong.host_nonce = 10;
        assert!(event_payload(&wrong, ev, 1).is_err());
        let mut payload = event_payload(&cfg, ev, (i + 1) as u64).unwrap();
        assert!(payload.onebot_projection.is_none());
        assert_eq!(
            payload.wrapper_message.as_ref().unwrap().record.msg_time,
            "1794567890"
        );
        if i > 0 {
            // Current records are separately bound to the current generation.
            let mut fresh = ev.clone();
            fresh
                .wrapper_message
                .as_mut()
                .unwrap()
                .binding
                .session_generation = 2;
            payload = event_payload(&current, &fresh, (i + 1) as u64).unwrap();
        }
        bridge.send(&BridgeMsg::Event { event: payload }).unwrap();
        assert!(matches!(
            bridge.recv().unwrap(),
            CoreToBridgeMsg::EventAck { .. }
        ));
    }
    let mut control = ControlClient::connect(&control_name, &auth, "lab").unwrap();
    match control
        .request(ControlMsg::DrainEvents { max: 10 })
        .unwrap()
    {
        CoreToControlMsg::EventsBatch { events } => {
            assert_eq!(events.len(), 3, "old generation is audit-only");
            assert!(events.iter().all(|e| e.onebot_projection.is_none()));
            assert_eq!(events[0].direction, "self_sent");
        }
        other => panic!("{other:?}"),
    }
    control.request(ControlMsg::Stop {}).unwrap();
    runner.join().unwrap().unwrap();
    // A budget that fits normalized text alone must reject the retained raw record.
    let budget_path = dir.join("budget.log");
    let mut config = RuntimeConfig::default();
    config.event_queue_max_bytes = events[0].text.len() + 1024;
    let (mut budget_actor, _) = AccountActor::open(&budget_path, config).unwrap();
    budget_actor
        .attach_session(caligo_model::SessionIdentity {
            host: caligo_model::HostIdentity {
                pid: 1,
                process_created_utc: "fixture".into(),
                module_baseline: "lab-wrapper".into(),
                bridge_build: "lab".into(),
                host_nonce: 9,
            },
            account: caligo_model::AccountId("10001".into()),
            session_generation: 1,
        })
        .unwrap();
    let event = caligo_model::MsgEvent {
        event_seq: 0,
        session_generation: 1,
        session: caligo_model::SessionKey::new(
            "10001",
            caligo_model::SessionKind::Private,
            "20002",
        ),
        direction: caligo_model::Direction::Incoming,
        sender: "20002".into(),
        native_id: caligo_model::NativeMessageId(events[0].native_id.clone()),
        text: events[0].text.clone(),
        platform_time: events[0].msg_time,
        wrapper_message: events[0].wrapper_message.clone(),
        onebot_projection: None,
        observed_at_unix_ms: 1,
        source: caligo_model::EventSource::Recv,
    };
    assert!(event.queue_bytes() > events[0].text.len() + 1024);
    assert!(matches!(
        budget_actor.on_event(event),
        Err(caligo_model::RejectReason::CapacityExhausted { .. })
    ));
    let (_, report) = AccountActor::open(&journal, RuntimeConfig::default()).unwrap();
    assert_eq!(report.recovered_events, 4);
    let (_, recovered) = caligo_core::journal::Journal::open(
        &journal,
        16 * 1024 * 1024,
        caligo_core::journal::SyncMode::Always,
    )
    .unwrap();
    let records: Vec<_> = recovered
        .records
        .iter()
        .filter_map(|row| match row {
            caligo_core::journal::JournalRecord::Event(event) => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|e| e.onebot_projection.is_none()));
    let raw = records[0].wrapper_message.as_ref().unwrap();
    assert_eq!(raw.record.msg_id, "90071992547409931234");
    assert_eq!(raw.record.client_seq.as_deref(), Some("9007199254740995"));
    assert_eq!(raw.record.msg_random, "18446744073709551615");
    assert_eq!(raw.record.msg_time, "1794567890");
    assert_eq!(raw.text().unwrap(), events[0].text);
    assert_eq!(records[0].session_generation, 1);
}
