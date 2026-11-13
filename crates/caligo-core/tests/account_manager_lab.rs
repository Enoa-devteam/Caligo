//! Two real core daemons and Windows pipes; no QQ or fabricated login.
#![cfg(windows)]

use caligo_core::account_manager::{
    management_token, AccountManager, AccountSpec, ManagerConfig, PortConfig,
};
use caligo_core::daemon::{BridgeClient, ControlClient, Daemon};
use caligo_core::ipc::{BridgeMsg, ControlMsg, CoreToBridgeMsg, CoreToControlMsg, EventPayload};
use caligo_core::transport::PipeServer;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Sandbox(PathBuf);
impl Sandbox {
    fn new(tag: &str) -> Self {
        Self(std::env::temp_dir().join(
            format!("caligo-accounts-{tag}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()),
        ))
    }
    fn root(&self) -> &Path {
        &self.0
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn config(tag: &str) -> ManagerConfig {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    ManagerConfig {
        version: 1,
        accounts: [10001, 10002]
            .into_iter()
            .map(|account| AccountSpec {
                account,
                data_dir: format!("account-{account}"),
                pipe_prefix: format!("caligo-manager-{tag}-{unique}-{account}"),
                module_baseline: "offline-lab-exact-baseline".into(),
                expect_client_pid: std::process::id(),
                ports: PortConfig::default(),
            })
            .collect(),
    }
}
fn control(daemon: &Arc<Daemon>) -> ControlClient {
    ControlClient::connect(&daemon.pipe_names().1, &daemon.auth_token(), "account-lab").unwrap()
}
fn bridge(daemon: &Arc<Daemon>, account: u64) -> BridgeClient {
    BridgeClient::connect(
        &daemon.pipe_names().0,
        &daemon.auth_token(),
        "account-lab",
        1,
        &account.to_string(),
        "offline-lab-exact-baseline",
        account,
        "2026-11-13T00:00:00Z",
    )
    .unwrap()
}
fn event(account: u64, text: &str) -> EventPayload {
    EventPayload {
        event_seq: 1,
        session_generation: 1,
        session: json!({"account":account.to_string(),"kind":"private","peer":"20001"}),
        direction: "incoming".into(),
        sender: "lab-sender".into(),
        native_id: "same-native-id-separate-accounts".into(),
        text: text.into(),
        platform_time: Some(1700000000),
        wrapper_message: None,
        onebot_projection: Some(caligo_model::OneBotMessageProjection {
            peer_id: 20001,
            sender_user_id: 20001,
            real_id: 77,
            time: 1700000000,
            sender_nickname: Some("lab".into()),
            sub_type: Some("friend".into()),
            font: Some(0),
        }),
        observed_at_unix_ms: 1700000000000,
        source: "recv".into(),
    }
}
fn persist(bridge: &mut BridgeClient, event: EventPayload) {
    bridge.send(&BridgeMsg::Event { event }).unwrap();
    assert!(matches!(
        bridge.recv().unwrap(),
        CoreToBridgeMsg::EventAck {
            event_seq: 1,
            suppressed: false
        }
    ));
}

#[test]
fn two_real_daemons_isolate_pipes_tokens_journals_and_account_stop() {
    let sandbox = Sandbox::new("two");
    let cfg = config("two");
    assert_eq!(cfg.save_atomic(sandbox.root()).unwrap(), cfg);
    let mut manager =
        AccountManager::new(sandbox.root(), ManagerConfig::load(sandbox.root()).unwrap()).unwrap();
    manager.start_all().unwrap();
    assert_eq!(manager.running_accounts(), vec![10001, 10002]);
    let a = manager.daemon(10001).unwrap();
    let b = manager.daemon(10002).unwrap();
    assert_ne!(a.auth_token(), b.auth_token());
    let management = management_token(sandbox.root()).unwrap();
    assert!(
        management != a.auth_token()
            && management != b.auth_token()
            && management_token(sandbox.root()).unwrap() == management
    );
    assert_ne!(a.pipe_names(), b.pipe_names());
    assert_ne!(a.config.journal_path, b.config.journal_path);
    let handler_a = manager.action_handler(10001).unwrap();
    let handler_b = manager.action_handler(10002).unwrap();
    assert_eq!(
        handler_a("get_status", &json!({}), json!(1)).data["online"],
        false
    );
    assert_ne!(handler_a("get_login_info", &json!({}), json!(2)).retcode, 0);
    let mut bridge_a = bridge(&a, 10001);
    let mut bridge_b = bridge(&b, 10002);
    let mut control_a = control(&a);
    let mut control_b = control(&b);
    persist(&mut bridge_a, event(10001, "account-a-only"));
    persist(&mut bridge_b, event(10002, "account-b-only"));
    let read_a = handler_a("get_msg", &json!({"message_id":1}), json!("a"));
    let read_b = handler_b("get_msg", &json!({"message_id":1}), json!("b"));
    assert_eq!(read_a.data["message"], "account-a-only");
    assert_eq!(read_b.data["message"], "account-b-only");
    for (client, text) in [
        (&mut control_a, "account-a-only"),
        (&mut control_b, "account-b-only"),
    ] {
        match client.request(ControlMsg::DrainEvents { max: 10 }).unwrap() {
            CoreToControlMsg::EventsBatch { events } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].text, text);
            }
            other => panic!("expected isolated events, got {other:?}"),
        }
    }
    manager.stop(10001).unwrap();
    assert_eq!(manager.running_accounts(), vec![10002]);
    assert!(manager.action_handler(10001).is_err());
    assert_eq!(
        handler_a("get_msg", &json!({"message_id":1}), json!(0)).retcode,
        1400
    );
    assert!(matches!(
        control_b.request(ControlMsg::Health {}).unwrap(),
        CoreToControlMsg::HealthAck {}
    ));
    assert_eq!(
        handler_b("get_msg", &json!({"message_id":1}), json!(0)).data["message"],
        "account-b-only"
    );
    // A stale Arc/handler cannot dispatch after the account has stopped.
    assert!(a
        .submit_onebot_text(
            caligo_model::SessionKey::new("10001", caligo_model::SessionKind::Group, "20001"),
            "late".into(),
            100
        )
        .is_err());
    drop(control_a);
    drop(bridge_a);
    manager.shutdown().unwrap();
    drop(control_b);
    drop(bridge_b);
    let token_a = a.auth_token();
    let token_b = b.auth_token();
    drop(a);
    drop(b);
    drop(handler_a);
    drop(handler_b);
    manager.start_all().unwrap();
    assert_eq!(manager.daemon(10001).unwrap().auth_token(), token_a);
    assert_eq!(manager.daemon(10002).unwrap().auth_token(), token_b);
    assert_eq!(
        manager.action_handler(10001).unwrap()("get_msg", &json!({"message_id":1}), json!(0)).data
            ["message"],
        "account-a-only"
    );
    assert_eq!(
        manager.action_handler(10002).unwrap()("get_msg", &json!({"message_id":1}), json!(0)).data
            ["message"],
        "account-b-only"
    );
    let serialized = serde_json::to_string(manager.config()).unwrap();
    assert!(!serialized.contains(&token_a));
    assert!(!serialized.contains(&token_b));
    manager.shutdown().unwrap();
}

#[test]
fn strict_json_and_conflicting_specs_are_rejected() {
    let cfg = config("validation");
    let mut value = serde_json::to_value(&cfg).unwrap();
    value["accounts"][0]["auth_token"] = json!("must-not-be-accepted");
    assert!(serde_json::from_value::<ManagerConfig>(value).is_err());
    let mut value = serde_json::to_value(&cfg).unwrap();
    value["accounts"][0]["ports"]["admin"] = json!(1234);
    assert!(serde_json::from_value::<ManagerConfig>(value).is_err());
    let mut bad = cfg.clone();
    bad.accounts[1].account = bad.accounts[0].account;
    assert!(bad.validate().is_err());
    let mut bad = cfg.clone();
    bad.accounts[0].account = 0;
    assert!(bad.validate().is_err());
    let mut bad = cfg.clone();
    bad.accounts[1].data_dir = bad.accounts[0].data_dir.to_uppercase();
    assert!(bad.validate().is_err());
    let mut bad = cfg.clone();
    bad.accounts[1].pipe_prefix = bad.accounts[0].pipe_prefix.to_uppercase();
    assert!(bad.validate().is_err());
    let mut bad = cfg.clone();
    bad.accounts[0].ports.http = Some(18001);
    bad.accounts[1].ports.mcp = Some(18001);
    assert!(bad.validate().is_err());
    let mut bad = cfg.clone();
    bad.accounts[0].ports.http = Some(0);
    assert!(bad.validate().is_err());
    for dir in [
        "../escape",
        "C:\\escape",
        "a/b",
        "CON",
        "a.",
        "secret:stream",
    ] {
        let mut bad = cfg.clone();
        bad.accounts[0].data_dir = dir.into();
        assert!(bad.validate().is_err());
    }
    for prefix in ["", "\\\\.\\pipe\\caligo", "a/b", "bad\0pipe"] {
        let mut bad = cfg.clone();
        bad.accounts[0].pipe_prefix = prefix.into();
        assert!(bad.validate().is_err());
    }
    let sandbox = Sandbox::new("strict");
    cfg.save_atomic(sandbox.root()).unwrap();
    let old = std::fs::read(sandbox.root().join("accounts.json")).unwrap();
    assert!(bad.save_atomic(sandbox.root()).is_err());
    assert_eq!(
        std::fs::read(sandbox.root().join("accounts.json")).unwrap(),
        old
    );
    let mut updated = cfg.clone();
    updated.accounts[0].ports.http = Some(18005);
    assert_eq!(updated.save_atomic(sandbox.root()).unwrap(), updated);
    assert_eq!(ManagerConfig::load(sandbox.root()).unwrap(), updated);
    std::fs::write(
        sandbox.root().join("accounts.json"),
        b"{\"version\":1,\"version\":1,\"accounts\":[]}",
    )
    .unwrap();
    assert!(ManagerConfig::load(sandbox.root()).is_err());
    assert!(AccountManager::new("relative-root", cfg).is_err());
}

#[test]
fn failed_batch_rolls_back_pipe_threads_and_preserves_existing_account() {
    let sandbox = Sandbox::new("rollback");
    let cfg = config("rollback");
    let blocked_name = format!("\\\\.\\pipe\\{}-control", cfg.accounts[1].pipe_prefix);
    let blocker = PipeServer::create(&blocked_name).unwrap();
    let mut manager = AccountManager::new(sandbox.root(), cfg).unwrap();
    assert!(manager.start_all().is_err());
    assert!(manager.running_accounts().is_empty());
    assert!(manager.daemon(10001).is_err());
    // If rollback leaked A's pipe owners, its immediate restart would fail.
    let a = manager.start(10001).unwrap();
    let mut client = control(&a);
    assert!(matches!(
        client.request(ControlMsg::Health {}).unwrap(),
        CoreToControlMsg::HealthAck {}
    ));
    assert!(manager.start_all().is_err());
    assert_eq!(manager.running_accounts(), vec![10001]);
    assert!(matches!(
        client.request(ControlMsg::Health {}).unwrap(),
        CoreToControlMsg::HealthAck {}
    ));
    drop(blocker);
    manager.start(10002).unwrap();
    manager.shutdown().unwrap();
}

#[test]
fn bad_or_unwritable_credentials_fail_and_release_directory_lock() {
    let sandbox = Sandbox::new("token");
    let cfg = config("token");
    let mut manager = AccountManager::new(sandbox.root(), cfg.clone()).unwrap();
    let path = sandbox
        .root()
        .join(&cfg.accounts[0].data_dir)
        .join("auth.token");
    std::fs::write(&path, b"bad-token").unwrap();
    assert!(manager.start(10001).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"bad-token");
    assert!(!sandbox
        .root()
        .join(&cfg.accounts[0].data_dir)
        .join("journal.jsonl")
        .exists());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(manager.start(10001).is_err());
    std::fs::remove_dir(&path).unwrap();
    manager.start(10001).unwrap();
    // A second manager with a distinct pipe still cannot share the live files.
    let mut other_cfg = cfg.clone();
    other_cfg.accounts[0].pipe_prefix.push_str("-other");
    let mut other = AccountManager::new(sandbox.root(), other_cfg).unwrap();
    assert!(other.start(10001).is_err());
    manager.stop(10001).unwrap();
    other.start(10001).unwrap();
    other.shutdown().unwrap();
}

#[test]
fn insecure_existing_root_and_configuration_replace_failure_are_explicit() {
    let sandbox = Sandbox::new("permissions");
    std::fs::create_dir(sandbox.root()).unwrap();
    // Ordinary temp inheritance grants other principals; manager never fixes
    // a caller-owned existing directory silently.
    assert!(AccountManager::new(sandbox.root(), config("permissions")).is_err());
    std::fs::remove_dir(sandbox.root()).unwrap();
    let cfg = config("private");
    cfg.save_atomic(sandbox.root()).unwrap();
    let path = sandbox.root().join("accounts.json");
    let before = std::fs::read(&path).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let mut updated = cfg.clone();
    updated.accounts[0].ports.http = Some(18011);
    assert!(updated.save_atomic(sandbox.root()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
    assert_eq!(cfg.save_atomic(sandbox.root()).unwrap(), cfg);
    assert!(
        !std::fs::read_dir(sandbox.root()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pending-"))
    );
}
