//! OneBot V11 action layer. All transports share this action registry and
//! dispatch exactly once through `SendSink`.

use caligo_model::onebot_traits::{
    extract_text, SendDisposition, SendPayload, SendSink, SendTarget,
};
pub use caligo_model::onebot_traits::{StoreReader, StoredMessageView};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActionResponse {
    pub status: &'static str,
    pub retcode: i32,
    /// OneBot responses carry `data:null` when there is no data, including async.
    pub data: Value,
    /// A present echo is returned without changing its JSON type; absent echo is null.
    pub echo: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wording: Option<String>,
}

impl ActionResponse {
    pub fn ok(data: Value, echo: Value) -> Self {
        Self {
            status: "ok",
            retcode: 0,
            data,
            echo,
            wording: None,
        }
    }
    pub fn async_accepted(echo: Value) -> Self {
        // `*_async` names are a Caligo extension; OneBot V11 does not define
        // these action names as part of its public send API.
        Self {
            status: "async",
            retcode: 1,
            data: Value::Null,
            echo,
            wording: None,
        }
    }
    pub fn failed(retcode: i32, wording: &str, echo: Value) -> Self {
        Self {
            status: "failed",
            retcode,
            data: Value::Null,
            echo,
            wording: Some(wording.to_owned()),
        }
    }
}

pub const RET_UNSUPPORTED: i32 = 1404;
pub const RET_BAD_PARAMS: i32 = 1400;
pub const RET_BAD_REQUEST: i32 = 1440;
pub const RET_NOT_CONFIRMED: i32 = 1501;
pub const RET_NOT_READY: i32 = 1502;

fn get_u64(params: &Value, key: &str) -> Result<u64, ActionResponse> {
    params
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ActionResponse::failed(RET_BAD_PARAMS, &format!("{key} 必填"), Value::Null))
}

fn get_message_text(params: &Value) -> Result<String, ActionResponse> {
    match params.get("message").map(extract_text) {
        Some(Ok(t)) => Ok(t),
        Some(Err(e)) => Err(ActionResponse::failed(RET_BAD_PARAMS, &e, Value::Null)),
        None => Err(ActionResponse::failed(
            RET_BAD_PARAMS,
            "message 必填",
            Value::Null,
        )),
    }
}

fn dispatch_send(
    sink: &mut dyn SendSink,
    echo: Value,
    payload: SendPayload,
    allow_async: bool,
) -> ActionResponse {
    match sink.send(&payload) {
        Ok(SendDisposition::Confirmed { message_id }) => {
            ActionResponse::ok(serde_json::json!({"message_id": message_id}), echo)
        }
        Ok(SendDisposition::AcceptedUnconfirmed) if allow_async => {
            ActionResponse::async_accepted(echo)
        }
        Ok(SendDisposition::AcceptedUnconfirmed) => ActionResponse::failed(
            RET_NOT_CONFIRMED,
            "请求已受理但尚无稳定消息身份；结果未知，禁止自动重发",
            echo,
        ),
        Ok(SendDisposition::DeliveryUnknown { reason }) => {
            ActionResponse::failed(RET_NOT_CONFIRMED, &reason, echo)
        }
        Ok(SendDisposition::Unsupported { reason }) => {
            ActionResponse::failed(RET_UNSUPPORTED, &reason, echo)
        }
        Err(e) => ActionResponse::failed(RET_NOT_READY, &e, echo),
    }
}

fn send_payload(
    action: &str,
    params: &Value,
    sink: &mut dyn SendSink,
    echo: Value,
    allow_async: bool,
) -> ActionResponse {
    let (target, text) = match action {
        "send_group_msg" | "send_group_msg_async" => {
            let group_id = match get_u64(params, "group_id") {
                Ok(v) => v,
                Err(mut r) => {
                    r.echo = echo;
                    return r;
                }
            };
            let text = match get_message_text(params) {
                Ok(t) => t,
                Err(mut r) => {
                    r.echo = echo;
                    return r;
                }
            };
            (SendTarget::Group { group_id }, text)
        }
        "send_private_msg" | "send_private_msg_async" => {
            let user_id = match get_u64(params, "user_id") {
                Ok(v) => v,
                Err(mut r) => {
                    r.echo = echo;
                    return r;
                }
            };
            let text = match get_message_text(params) {
                Ok(t) => t,
                Err(mut r) => {
                    r.echo = echo;
                    return r;
                }
            };
            (SendTarget::Private { user_id }, text)
        }
        "send_msg" | "send_msg_async" => {
            let inferred_type = if params.get("group_id").is_some() {
                "group"
            } else if params.get("user_id").is_some() {
                "private"
            } else {
                ""
            };
            let message_type = params
                .get("message_type")
                .and_then(Value::as_str)
                .unwrap_or(inferred_type);
            let target = match message_type {
                "group" => match get_u64(params, "group_id") {
                    Ok(id) => SendTarget::Group { group_id: id },
                    Err(mut r) => {
                        r.echo = echo;
                        return r;
                    }
                },
                "private" => match get_u64(params, "user_id") {
                    Ok(id) => SendTarget::Private { user_id: id },
                    Err(mut r) => {
                        r.echo = echo;
                        return r;
                    }
                },
                _ => {
                    return ActionResponse::failed(
                        RET_BAD_PARAMS,
                        "message_type 须为 private 或 group",
                        echo,
                    )
                }
            };
            let text = match get_message_text(params) {
                Ok(t) => t,
                Err(mut r) => {
                    r.echo = echo;
                    return r;
                }
            };
            (target, text)
        }
        _ => unreachable!("send_payload called for registered send action"),
    };
    dispatch_send(sink, echo, SendPayload { target, text }, allow_async)
}

/// Whether an action name belongs to the implemented public API surface.
pub fn is_supported_action(action: &str) -> bool {
    matches!(
        action,
        "send_group_msg"
            | "send_private_msg"
            | "send_msg"
            | "send_group_msg_async"
            | "send_private_msg_async"
            | "send_msg_async"
            | "get_msg"
            | "get_group_list"
            | "get_group_info"
            | "get_friend_list"
            | "get_login_info"
            | "get_status"
            | "get_version_info"
    )
}

/// Process one action through the shared action registry.
pub fn handle_action(
    action: &str,
    params: &Value,
    echo: Value,
    sink: &mut dyn SendSink,
    store: &dyn StoreReader,
) -> ActionResponse {
    match action {
        "send_group_msg" | "send_private_msg" | "send_msg" => {
            send_payload(action, params, sink, echo, false)
        }
        "send_group_msg_async" | "send_private_msg_async" | "send_msg_async" => {
            send_payload(action, params, sink, echo, true)
        }
        "get_msg" => {
            let Some(mid) = params
                .get("message_id")
                .and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
            else {
                return ActionResponse::failed(RET_BAD_PARAMS, "message_id 必须是 int32", echo);
            };
            match store.get_msg(mid) {
                Some(msg) => ActionResponse::ok(
                    serde_json::json!({
                        "message_type": msg.message_type,
                        "message_id": msg.message_id,
                        "real_id": msg.real_id,
                        "sender": msg.sender,
                        "message": msg.message,
                        "time": msg.time,
                    }),
                    echo,
                ),
                None => ActionResponse::failed(RET_BAD_REQUEST, "message 不存在", echo),
            }
        }
        // 群列表只能从已入库的群消息里学(平台无列表接口);群名用
        // `群 <号>` 占位,不编造平台未给出的名字。
        "get_group_list" => {
            let list: Vec<serde_json::Value> = store
                .group_peers()
                .into_iter()
                .filter_map(|peer| {
                    let id = peer.parse::<i64>().ok()?;
                    Some(serde_json::json!({
                        "group_id": id,
                        "group_name": format!("群 {peer}"),
                        "member_count": 0,
                        "max_member_count": 0
                    }))
                })
                .collect();
            ActionResponse::ok(serde_json::Value::Array(list), echo)
        }
        "get_group_info" => {
            let id = params
                .get("group_id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            ActionResponse::ok(
                serde_json::json!({
                    "group_id": id,
                    "group_name": format!("群 {id}"),
                    "member_count": 0,
                    "max_member_count": 0
                }),
                echo,
            )
        }
        "get_friend_list" => ActionResponse::ok(serde_json::Value::Array(Vec::new()), echo),
        "get_login_info" => match store.login_info() {
            Some((user_id, nickname)) => ActionResponse::ok(
                serde_json::json!({"user_id": user_id, "nickname": nickname}),
                echo,
            ),
            None => ActionResponse::failed(RET_NOT_READY, "账号身份尚未就绪", echo),
        },
        "get_version_info" => ActionResponse::ok(
            serde_json::json!({
                "app_name": "Caligo",
                "app_version": env!("CARGO_PKG_VERSION"),
                "protocol_version": "v11",
                "impl": "Caligo",
                "os": "Windows"
            }),
            echo,
        ),
        "get_status" => match store.online_status() {
            Some(online) => {
                ActionResponse::ok(serde_json::json!({"online": online, "good": online}), echo)
            }
            None => ActionResponse::failed(RET_NOT_READY, "daemon/session 状态尚未就绪", echo),
        },
        _ => ActionResponse::failed(RET_UNSUPPORTED, "未支持动作", echo),
    }
}

#[derive(Debug, Deserialize)]
struct RawAction {
    action: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    echo: Value,
}

/// Parse one action envelope shared by HTTP and future WS transports.
pub fn parse_action(raw: &str) -> Result<(String, Value, Value), String> {
    let ra: RawAction =
        serde_json::from_str(raw).map_err(|e| format!("动作 JSON 解析失败: {e}"))?;
    Ok((ra.action, ra.params, ra.echo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use caligo_model::onebot_traits::StoredMessageView;

    #[derive(Default)]
    struct FakeSink {
        ready: bool,
        last: Vec<SendPayload>,
    }
    impl SendSink for FakeSink {
        fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String> {
            if !self.ready {
                return Err("session not ready".into());
            }
            self.last.push(payload.clone());
            Ok(SendDisposition::AcceptedUnconfirmed)
        }
    }
    struct FakeStore;
    impl StoreReader for FakeStore {
        fn get_msg(&self, id: i32) -> Option<StoredMessageView> {
            (id == 42).then(|| StoredMessageView {
                message_id: 42,
                message_type: "group".into(),
                real_id: 398263,
                sender: serde_json::json!({"user_id": 123, "nickname": "other", "sex": "unknown", "age": 0}),
                message: serde_json::json!([{"type":"text","data":{"text":"hello"}}]),
                time: 1791621805,
            })
        }
        fn login_info(&self) -> Option<(u64, String)> {
            Some((1694717255, "caligo".into()))
        }
        fn online_status(&self) -> Option<bool> {
            Some(false)
        }
    }

    #[test]
    fn async_variant_has_null_data_and_preserves_arbitrary_echo() {
        let mut sink = FakeSink {
            ready: true,
            ..Default::default()
        };
        let (action, params, echo) = parse_action(
            r#"{"action":"send_group_msg_async","params":{"group_id":263402786,"message":"hi"},"echo":{"seq":3}}"#,
        ).unwrap();
        let response = handle_action(&action, &params, echo, &mut sink, &FakeStore);
        assert_eq!(response.status, "async");
        assert_eq!(
            serde_json::to_value(&response).unwrap()["data"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(&response).unwrap()["echo"],
            serde_json::json!({"seq":3})
        );
    }

    #[test]
    fn standard_send_does_not_report_queue_acceptance_as_success() {
        let mut sink = FakeSink {
            ready: true,
            ..Default::default()
        };
        let r = handle_action(
            "send_group_msg",
            &serde_json::json!({"group_id":1,"message":"x"}),
            Value::Null,
            &mut sink,
            &FakeStore,
        );
        assert_eq!(r.retcode, RET_NOT_CONFIRMED);
        assert_eq!(r.data, Value::Null);
    }

    #[test]
    fn unsupported_and_unknown_sends_keep_distinct_public_results() {
        struct ResultSink(SendDisposition);
        impl SendSink for ResultSink {
            fn send(&mut self, _: &SendPayload) -> Result<SendDisposition, String> {
                Ok(self.0.clone())
            }
        }
        let payload = serde_json::json!({"group_id":1,"message":"x"});
        let unsupported = handle_action(
            "send_private_msg",
            &serde_json::json!({"user_id":2,"message":"x"}),
            Value::Null,
            &mut ResultSink(SendDisposition::Unsupported {
                reason: "private mapping unavailable".into(),
            }),
            &FakeStore,
        );
        assert_eq!(unsupported.retcode, RET_UNSUPPORTED);
        assert_eq!(
            unsupported.wording.as_deref(),
            Some("private mapping unavailable")
        );
        assert_eq!(unsupported.data, Value::Null);

        let unknown = handle_action(
            "send_group_msg",
            &payload,
            Value::Null,
            &mut ResultSink(SendDisposition::DeliveryUnknown {
                reason: "receipt deadline elapsed".into(),
            }),
            &FakeStore,
        );
        assert_eq!(unknown.retcode, RET_NOT_CONFIRMED);
        assert_eq!(unknown.wording.as_deref(), Some("receipt deadline elapsed"));
        assert_eq!(unknown.data, Value::Null);
    }

    #[test]
    fn private_user_ids_are_preserved_as_distinct_uins() {
        let mut sink = FakeSink {
            ready: true,
            ..Default::default()
        };
        for user_id in [123, 456] {
            handle_action(
                "send_private_msg_async",
                &serde_json::json!({"user_id":user_id,"message":"hi"}),
                Value::Null,
                &mut sink,
                &FakeStore,
            );
        }
        assert_eq!(sink.last[0].target, SendTarget::Private { user_id: 123 });
        assert_eq!(sink.last[1].target, SendTarget::Private { user_id: 456 });
    }

    #[test]
    fn get_msg_returns_complete_standard_fields_and_int32_id() {
        let response = handle_action(
            "get_msg",
            &serde_json::json!({"message_id":42}),
            Value::Null,
            &mut FakeSink::default(),
            &FakeStore,
        );
        let data = serde_json::to_value(response).unwrap()["data"].clone();
        assert_eq!(data["message_type"], "group");
        assert_eq!(data["real_id"], 398263);
        assert_eq!(data["sender"]["user_id"], 123);
        assert_eq!(data["message"][0]["data"]["text"], "hello");
        let bad = handle_action(
            "get_msg",
            &serde_json::json!({"message_id":i64::MAX}),
            Value::Null,
            &mut FakeSink::default(),
            &FakeStore,
        );
        assert_eq!(bad.retcode, RET_BAD_PARAMS);
    }

    #[test]
    fn login_status_and_version_do_not_invent_missing_runtime_state() {
        struct Unknown;
        impl StoreReader for Unknown {
            fn get_msg(&self, _: i32) -> Option<StoredMessageView> {
                None
            }
            fn login_info(&self) -> Option<(u64, String)> {
                None
            }
            fn online_status(&self) -> Option<bool> {
                None
            }
        }
        let mut sink = FakeSink::default();
        assert_eq!(
            handle_action(
                "get_login_info",
                &Value::Null,
                Value::Null,
                &mut sink,
                &Unknown
            )
            .retcode,
            RET_NOT_READY
        );
        assert_eq!(
            handle_action("get_status", &Value::Null, Value::Null, &mut sink, &Unknown).retcode,
            RET_NOT_READY
        );
        let version = serde_json::to_value(handle_action(
            "get_version_info",
            &Value::Null,
            Value::Null,
            &mut sink,
            &Unknown,
        ))
        .unwrap();
        assert!(!version["data"]["app_version"].as_str().unwrap().is_empty());
    }

    struct GroupStore;

    impl StoreReader for GroupStore {
        fn get_msg(&self, _: i32) -> Option<StoredMessageView> {
            None
        }
        fn login_info(&self) -> Option<(u64, String)> {
            None
        }
        fn online_status(&self) -> Option<bool> {
            Some(true)
        }
        fn group_peers(&self) -> Vec<String> {
            vec!["1098345913".into(), "1102382705".into()]
        }
    }

    #[test]
    fn group_queries_use_learned_peers_without_inventing_names() {
        assert!(is_supported_action("get_group_list"));
        assert!(is_supported_action("get_group_info"));
        assert!(is_supported_action("get_friend_list"));

        let mut sink = FakeSink::default();
        let list = serde_json::to_value(handle_action(
            "get_group_list",
            &Value::Null,
            Value::Null,
            &mut sink,
            &GroupStore,
        ))
        .unwrap();
        let groups = list["data"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["group_id"], 1098345913i64);
        assert_eq!(groups[0]["group_name"], "群 1098345913");
        assert_eq!(groups[0]["member_count"], 0);
        assert_eq!(groups[0]["max_member_count"], 0);

        let info = serde_json::to_value(handle_action(
            "get_group_info",
            &serde_json::json!({"group_id": 1102382705i64}),
            Value::Null,
            &mut sink,
            &GroupStore,
        ))
        .unwrap();
        assert_eq!(info["data"]["group_id"], 1102382705i64);
        assert_eq!(info["data"]["group_name"], "群 1102382705");

        let friends = serde_json::to_value(handle_action(
            "get_friend_list",
            &Value::Null,
            Value::Null,
            &mut sink,
            &GroupStore,
        ))
        .unwrap();
        assert_eq!(friends["data"], serde_json::json!([]));
    }
}
