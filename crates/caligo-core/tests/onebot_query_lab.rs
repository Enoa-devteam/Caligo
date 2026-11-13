//! Shared read-only action contracts and normal daemon/pipe wiring; synthetic data only.
use caligo_core::onebot::{
    handle_action, handle_action_with_queries, resolve_action, ActionPermission, QueryBackend,
    StoreReader, StoredMessageView, ACTION_REGISTRY, QUERY_TIMEOUT, RET_BAD_PARAMS,
    RET_BAD_REQUEST, RET_NOT_CONFIRMED, RET_NOT_READY, RET_PERMISSION_DENIED, RET_UNSUPPORTED,
};
use caligo_model::capability::{
    FriendInfo, GroupInfo, HostQuery, LoginInfo, QueryData, QueryOutcome,
};
use caligo_model::onebot_traits::{SendDisposition, SendPayload, SendSink};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::Duration;

struct NoSend;
impl SendSink for NoSend {
    fn send(&mut self, _: &SendPayload) -> Result<SendDisposition, String> {
        panic!("read-only query must not send")
    }
}
struct OldStore;
impl StoreReader for OldStore {
    fn get_msg(&self, _: i32) -> Option<StoredMessageView> {
        None
    }
    fn login_info(&self) -> Option<(u64, String)> {
        Some((7, "stale store".into()))
    }
    fn online_status(&self) -> Option<bool> {
        Some(true)
    }
    fn group_peers(&self) -> Vec<String> {
        vec!["99".into()]
    }
}
struct FakeQueries {
    outcome: QueryOutcome,
    calls: RefCell<Vec<(HostQuery, Duration)>>,
}
impl FakeQueries {
    fn new(outcome: QueryOutcome) -> Self {
        Self {
            outcome,
            calls: RefCell::new(vec![]),
        }
    }
    fn run(&self, action: &str, params: Value, echo: Value) -> caligo_core::onebot::ActionResponse {
        handle_action_with_queries(action, &params, echo, &mut NoSend, &OldStore, self)
    }
}
impl QueryBackend for FakeQueries {
    fn query_host(&self, query: HostQuery, timeout: Duration) -> QueryOutcome {
        self.calls.borrow_mut().push((query, timeout));
        self.outcome.clone()
    }
}
fn group(group_id: &str) -> GroupInfo {
    GroupInfo {
        group_id: group_id.into(),
        group_name: "Synthetic directory".into(),
        member_count: 17,
        max_member_count: 500,
    }
}
fn actions() -> Vec<(&'static str, Value, HostQuery)> {
    vec![
        ("get_login_info", json!({}), HostQuery::LoginInfo),
        ("get_friend_list", json!({}), HostQuery::FriendList),
        ("get_group_list", json!({}), HostQuery::GroupList),
        (
            "get_group_info",
            json!({"group_id":u64::MAX}),
            HostQuery::GroupInfo {
                group_id: u64::MAX.to_string(),
            },
        ),
    ]
}

#[test]
fn successful_queries_preserve_fields_and_full_width_ids_with_exactly_one_call() {
    let expected_group = json!({"group_id":u64::MAX,"group_name":"Synthetic directory","member_count":17,"max_member_count":500});
    let cases = [
        (
            "get_login_info",
            json!({}),
            HostQuery::LoginInfo,
            QueryData::LoginInfo(LoginInfo {
                user_id: u64::MAX.to_string(),
                nickname: "Host name".into(),
                online: true,
            }),
            json!({"user_id":u64::MAX,"nickname":"Host name"}),
        ),
        (
            "get_friend_list",
            json!({}),
            HostQuery::FriendList,
            QueryData::FriendList(vec![FriendInfo {
                user_id: u64::MAX.to_string(),
                nickname: "Friend".into(),
                remark: "Remark".into(),
            }]),
            json!([{"user_id":u64::MAX,"nickname":"Friend","remark":"Remark"}]),
        ),
        (
            "get_group_list",
            json!({}),
            HostQuery::GroupList,
            QueryData::GroupList(vec![group(&u64::MAX.to_string())]),
            json!([expected_group.clone()]),
        ),
        (
            "get_group_info",
            json!({"group_id":u64::MAX,"no_cache":true}),
            HostQuery::GroupInfo {
                group_id: u64::MAX.to_string(),
            },
            QueryData::GroupInfo(group(&u64::MAX.to_string())),
            expected_group,
        ),
    ];
    for (action, params, query, data, expected) in cases {
        for echo in [
            Value::Null,
            json!(17),
            json!(false),
            json!("echo"),
            json!([1, "x"]),
            json!({"seq":9}),
        ] {
            let backend = FakeQueries::new(QueryOutcome::Success(data.clone()));
            let response = backend.run(action, params.clone(), echo.clone());
            assert_eq!(response.status, "ok");
            assert_eq!(response.retcode, 0);
            assert_eq!(response.data, expected);
            assert_eq!(response.echo, echo);
            assert_eq!(*backend.calls.borrow(), [(query.clone(), QUERY_TIMEOUT)]);
        }
    }
}

#[test]
fn real_empty_lists_are_success_and_all_failures_remain_failures_including_login() {
    for (action, data) in [
        ("get_friend_list", QueryData::FriendList(vec![])),
        ("get_group_list", QueryData::GroupList(vec![])),
    ] {
        let backend = FakeQueries::new(QueryOutcome::Success(data));
        assert_eq!(backend.run(action, json!({}), Value::Null).data, json!([]));
        assert_eq!(backend.calls.borrow().len(), 1);
    }
    for (outcome, retcode, wording) in [
        (QueryOutcome::Unsupported, RET_UNSUPPORTED, "未支持"),
        (QueryOutcome::NotReady, RET_NOT_READY, "未就绪"),
        (
            QueryOutcome::PermissionDenied,
            RET_PERMISSION_DENIED,
            "权限",
        ),
        (
            QueryOutcome::Rejected {
                code: i64::MIN,
                reason: "host denied detail".into(),
            },
            RET_BAD_REQUEST,
            "code=-9223372036854775808): host denied detail",
        ),
        (QueryOutcome::Timeout, RET_NOT_CONFIRMED, "超时"),
        (QueryOutcome::Disconnected, RET_NOT_CONFIRMED, "断开"),
        (QueryOutcome::InvalidResponse, RET_BAD_REQUEST, "合同"),
    ] {
        for (action, params, query) in actions() {
            let backend = FakeQueries::new(outcome.clone());
            let echo = json!({"action":action});
            let response = backend.run(action, params, echo.clone());
            assert_eq!(response.status, "failed");
            assert_eq!(response.retcode, retcode);
            assert_eq!(response.data, Value::Null);
            assert_eq!(response.echo, echo);
            let text = response.wording.unwrap();
            assert!(text.contains(wording), "{text}");
            assert!(!text.contains("禁止自动重发"));
            assert_eq!(*backend.calls.borrow(), [(query, QUERY_TIMEOUT)]);
        }
    }
}

#[test]
fn wrong_success_variants_invalid_ids_and_wrong_group_fail_closed() {
    let all_data = [
        QueryData::LoginInfo(LoginInfo {
            user_id: "1".into(),
            nickname: "Host".into(),
            online: true,
        }),
        QueryData::FriendList(vec![]),
        QueryData::GroupList(vec![]),
        QueryData::GroupInfo(group(&u64::MAX.to_string())),
        QueryData::Uid("u_hidden".into()),
        QueryData::Uin("1".into()),
        QueryData::Messages(vec![]),
    ];
    for (action, params, query) in actions() {
        for data in &all_data {
            let expected = matches!(
                (&query, data),
                (HostQuery::LoginInfo, QueryData::LoginInfo(_))
                    | (HostQuery::FriendList, QueryData::FriendList(_))
                    | (HostQuery::GroupList, QueryData::GroupList(_))
                    | (HostQuery::GroupInfo { .. }, QueryData::GroupInfo(_))
            );
            if expected {
                continue;
            }
            let backend = FakeQueries::new(QueryOutcome::Success(data.clone()));
            let response = backend.run(action, params.clone(), json!([action]));
            assert_eq!(response.retcode, RET_BAD_REQUEST);
            assert_eq!(response.data, Value::Null);
            assert_eq!(response.echo, json!([action]));
            assert_eq!(backend.calls.borrow().len(), 1);
        }
    }
    for id in ["0", "-1", "1.5", "1e3", "18446744073709551616", "u_hidden"] {
        for (action, data) in [
            (
                "get_login_info",
                QueryData::LoginInfo(LoginInfo {
                    user_id: id.into(),
                    nickname: "Host".into(),
                    online: true,
                }),
            ),
            (
                "get_friend_list",
                QueryData::FriendList(vec![FriendInfo {
                    user_id: id.into(),
                    nickname: "Friend".into(),
                    remark: String::new(),
                }]),
            ),
            ("get_group_list", QueryData::GroupList(vec![group(id)])),
        ] {
            let response =
                FakeQueries::new(QueryOutcome::Success(data)).run(action, json!({}), Value::Null);
            assert_eq!(response.retcode, RET_BAD_REQUEST);
            assert_eq!(response.data, Value::Null);
        }
    }
    for data in [
        QueryData::GroupInfo(group("2")),
        QueryData::GroupInfo(GroupInfo {
            member_count: 501,
            ..group("1")
        }),
    ] {
        let response = FakeQueries::new(QueryOutcome::Success(data)).run(
            "get_group_info",
            json!({"group_id":1}),
            Value::Null,
        );
        assert_eq!(response.retcode, RET_BAD_REQUEST);
        assert_eq!(response.data, Value::Null);
    }
    // Missing required platform fields cannot become zero/default values.
    assert!(serde_json::from_value::<QueryData>(
        json!({"kind":"group_info","data":{"group_id":"1","group_name":"Name"}})
    )
    .is_err());
}

#[test]
fn group_params_validate_before_backend_and_no_cache_is_always_a_host_read() {
    for params in [
        Value::Null,
        json!({}),
        json!({"group_id":0}),
        json!({"group_id":-1}),
        json!({"group_id":"1"}),
        json!({"group_id":1.5}),
        json!({"group_id":true}),
        json!({"group_id":1,"no_cache":null}),
        json!({"group_id":1,"no_cache":0}),
        json!({"group_id":1,"no_cache":"true"}),
    ] {
        let backend = FakeQueries::new(QueryOutcome::Unsupported);
        let response = backend.run("get_group_info", params.clone(), params.clone());
        assert_eq!(response.retcode, RET_BAD_PARAMS);
        assert_eq!(response.data, Value::Null);
        assert_eq!(response.echo, params);
        assert!(backend.calls.borrow().is_empty());
    }
    for params in [
        json!({"group_id":1}),
        json!({"group_id":1,"no_cache":false}),
        json!({"group_id":1,"no_cache":true}),
    ] {
        let backend = FakeQueries::new(QueryOutcome::Success(QueryData::GroupInfo(group("1"))));
        for _ in 0..2 {
            assert_eq!(
                backend
                    .run("get_group_info", params.clone(), Value::Null)
                    .retcode,
                0
            );
        }
        assert_eq!(
            *backend.calls.borrow(),
            vec![
                (
                    HostQuery::GroupInfo {
                        group_id: "1".into()
                    },
                    QUERY_TIMEOUT
                );
                2
            ]
        );
    }
}

#[test]
fn metadata_and_legacy_wrapper_keep_backend_availability_explicit() {
    assert_eq!(ACTION_REGISTRY.len(), 14);
    for name in ["get_friend_list", "get_group_list", "get_group_info"] {
        assert_eq!(
            resolve_action(name).unwrap().permission,
            ActionPermission::Read
        );
        let response = handle_action(
            name,
            &json!({"group_id":1}),
            json!(name),
            &mut NoSend,
            &OldStore,
        );
        assert_eq!(response.retcode, RET_UNSUPPORTED);
        assert_eq!(response.data, Value::Null);
        assert_eq!(response.echo, json!(name));
    }
    let schema = resolve_action("get_group_info")
        .unwrap()
        .input_schema
        .to_json_schema();
    assert_eq!(schema["required"], json!(["group_id"]));
    assert_eq!(schema["properties"]["group_id"]["minimum"], 1);
    assert_eq!(schema["properties"]["group_id"]["maximum"], u64::MAX);
    assert_eq!(schema["properties"]["no_cache"]["type"], "boolean");
    for name in ["uin_to_uid", "uid_to_uin", "get_uid", "get_uin"] {
        assert!(resolve_action(name).is_none());
    }
    assert_eq!(
        handle_action(
            "get_login_info",
            &Value::Null,
            Value::Null,
            &mut NoSend,
            &OldStore
        )
        .data,
        json!({"user_id":7,"nickname":"stale store"})
    );
}

#[cfg(windows)]
mod normal_pipe {
    use super::*;
    use caligo_core::daemon::{BridgeClient, Daemon, DaemonConfig};
    use caligo_core::onebot_server::action_handler;
    use caligo_core::runtime::RuntimeConfig;
    use caligo_model::capability::{QueryCapability, QueryScope};
    use caligo_model::ipc_v3::{BridgeMsg, CoreToBridgeMsg, CAPABILITY_QUERY_REVISION};
    use std::sync::{atomic::Ordering, Arc};
    use std::time::Instant;

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
            let dir = std::env::temp_dir().join(format!("caligo-onebot-query-{unique}"));
            std::fs::create_dir_all(&dir).unwrap();
            let daemon = Daemon::start(DaemonConfig {
                pipe_prefix: format!("caligo-onebot-query-{unique}"),
                journal_path: dir.join("journal.log"),
                module_baseline: "lab-baseline".into(),
                account: "10001".into(),
                core_build: caligo_core::CORE_BUILD.into(),
                expect_client_pid: std::process::id(),
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
        fn connect(&self) -> (BridgeClient, QueryScope) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(client) = BridgeClient::connect(
                    &self.daemon.pipe_names().0,
                    &self.daemon.auth_token(),
                    "onebot-query-lab",
                    1,
                    "10001",
                    "lab-baseline",
                    11,
                    "2026-11-13T00:00:00Z",
                ) {
                    return (
                        client,
                        QueryScope {
                            account: "10001".into(),
                            host_nonce: 11,
                            session_generation: 1,
                            connection_epoch: self.daemon.connection_epoch(),
                        },
                    );
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
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

    #[test]
    fn normal_handler_uses_real_pipe_results_and_never_invents_directory_or_login() {
        let lab = Lab::new();
        let handler = action_handler(lab.daemon.clone());
        assert_eq!(
            handler("get_login_info", &json!({}), Value::Null).retcode,
            RET_NOT_READY
        );
        let (mut bridge, scope) = lab.connect();
        for (action, params, _) in actions() {
            assert_eq!(
                handler(action, &params, Value::Null).retcode,
                RET_UNSUPPORTED
            );
        }
        bridge
            .send(&BridgeMsg::QueryCapabilities {
                revision: CAPABILITY_QUERY_REVISION,
                scope: scope.clone(),
                capabilities: vec![
                    QueryCapability::LoginInfo,
                    QueryCapability::FriendList,
                    QueryCapability::GroupList,
                    QueryCapability::GroupInfo,
                ],
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
        let cases = [
            (
                "get_login_info",
                json!({}),
                HostQuery::LoginInfo,
                QueryOutcome::Success(QueryData::LoginInfo(LoginInfo {
                    user_id: "10001".into(),
                    nickname: "Real synthetic host".into(),
                    online: true,
                })),
                json!({"user_id":10001,"nickname":"Real synthetic host"}),
                0,
            ),
            (
                "get_friend_list",
                json!({}),
                HostQuery::FriendList,
                QueryOutcome::Success(QueryData::FriendList(vec![])),
                json!([]),
                0,
            ),
            (
                "get_group_list",
                json!({}),
                HostQuery::GroupList,
                QueryOutcome::Success(QueryData::GroupList(vec![group("99")])),
                json!([{"group_id":99,"group_name":"Synthetic directory","member_count":17,"max_member_count":500}]),
                0,
            ),
            (
                "get_group_info",
                json!({"group_id":99,"no_cache":true}),
                HostQuery::GroupInfo {
                    group_id: "99".into(),
                },
                QueryOutcome::Success(QueryData::GroupInfo(group("99"))),
                json!({"group_id":99,"group_name":"Synthetic directory","member_count":17,"max_member_count":500}),
                0,
            ),
            (
                "get_login_info",
                json!({}),
                HostQuery::LoginInfo,
                QueryOutcome::PermissionDenied,
                Value::Null,
                RET_PERMISSION_DENIED,
            ),
            (
                "get_friend_list",
                json!({}),
                HostQuery::FriendList,
                QueryOutcome::Unsupported,
                Value::Null,
                RET_UNSUPPORTED,
            ),
        ];
        for (action, params, expected_query, outcome, data, retcode) in cases {
            let h = handler.clone();
            let waiter = std::thread::spawn(move || h(action, &params, json!({"action":action})));
            let CoreToBridgeMsg::Query {
                request_id,
                scope: actual_scope,
                timeout_ms,
                query,
            } = bridge.recv().unwrap()
            else {
                panic!("expected host query")
            };
            assert_eq!(query, expected_query);
            assert_eq!(actual_scope, scope);
            assert!(timeout_ms > 0 && timeout_ms <= QUERY_TIMEOUT.as_millis() as u64);
            bridge
                .send(&BridgeMsg::QueryResult {
                    request_id,
                    scope: actual_scope,
                    outcome,
                })
                .unwrap();
            let response = waiter.join().unwrap();
            assert_eq!(response.retcode, retcode);
            assert_eq!(response.data, data);
            assert_eq!(response.echo, json!({"action":action}));
            assert_eq!(lab.daemon.pending_query_count(), 0);
        }
        let journal = std::fs::read_to_string(lab.dir.join("journal.log")).unwrap_or_default();
        assert!(
            !journal.contains("query-"),
            "read-only queries must not create send ledger entries"
        );
        drop(bridge);
    }
}
