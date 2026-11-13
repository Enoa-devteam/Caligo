//! OneBot V11 action layer. All transports share this action registry and
//! dispatch exactly once through `SendSink`.

use base64::{engine::general_purpose::STANDARD, Engine};
use caligo_model::capability::{positive_decimal, GroupInfo, HostQuery, QueryData, QueryOutcome, VoiceDownload, VoiceInfo, MAX_VOICE_DOWNLOAD_BYTES};
use caligo_model::onebot_traits::{
    extract_text, SendDisposition, SendPayload, SendSink, SendTarget,
};
pub use caligo_model::onebot_traits::{StoreReader, StoredMessageView};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

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
pub const RET_PERMISSION_DENIED: i32 = 1403;
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Fixed read-only operations shared by every public transport.
/// Implementations enforce account, connection and session scope before success.
pub trait QueryBackend {
    fn query_host(&self, query: HostQuery, timeout: Duration) -> QueryOutcome;
}

struct UnsupportedQueries;
impl QueryBackend for UnsupportedQueries {
    fn query_host(&self, _: HostQuery, _: Duration) -> QueryOutcome {
        QueryOutcome::Unsupported
    }
}
pub const RET_BAD_PARAMS: i32 = 1400;
pub const RET_BAD_REQUEST: i32 = 1440;
pub const RET_NOT_CONFIRMED: i32 = 1501;
pub const RET_NOT_READY: i32 = 1502;

/// Capability metadata, not an authorization check. Frontends enforce access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPermission {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SendTargetKind {
    Group,
    Private,
    Inferred,
}

/// Exhaustive dispatch key shared by discovery and execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionHandlerKind {
    Send {
        target: SendTargetKind,
        allow_async: bool,
    },
    GetMessage,
    GetRecord,
    GetLoginInfo,
    GetFriendList,
    GetGroupList,
    GetGroupInfo,
    GetStatus,
    GetVersionInfo,
}

/// Input contracts for normal requests. Serializes as JSON Schema for tool
/// discovery; existing handlers remain responsible for runtime validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionInputSchema {
    Empty,
    GroupMessage,
    PrivateMessage,
    Message,
    MessageId,
    Record,
    GroupInfo,
}

impl ActionInputSchema {
    pub fn to_json_schema(self) -> Value {
        let id = serde_json::json!({"type":"integer", "minimum":0, "maximum":u64::MAX});
        let message = serde_json::json!({
            "description":"Literal text or text segments; other segment types are unsupported",
            "anyOf":[
                {"type":"string"},
                {"type":"array", "minItems":1, "items":{
                    "type":"object", "required":["type", "data"],
                    "properties":{
                        "type":{"const":"text"},
                        "data":{"type":"object", "required":["text"],
                            "properties":{"text":{"type":"string"}}}
                    }
                }}
            ]
        });
        let mut schema = serde_json::json!({"type":"object", "properties":{}});
        match self {
            Self::Empty => {}
            Self::GroupMessage | Self::PrivateMessage => {
                let key = if self == Self::GroupMessage {
                    "group_id"
                } else {
                    "user_id"
                };
                schema["properties"][key] = id;
                schema["properties"]["message"] = message;
                schema["required"] = serde_json::json!([key, "message"]);
            }
            Self::Message => {
                schema["properties"] = serde_json::json!({
                    "group_id":id, "user_id":id, "message":message,
                    "message_type":{"type":"string", "enum":["group", "private"],
                        "description":"Optional; inferred from group_id first, then user_id"}
                });
                schema["required"] = serde_json::json!(["message"]);
                schema["anyOf"] = serde_json::json!([
                    {"required":["group_id"], "properties":{"message_type":{"const":"group"}}},
                    {"required":["user_id"], "properties":{"message_type":{"const":"private"}}}
                ]);
            }
            Self::GroupInfo => {
                schema["properties"]["group_id"] = serde_json::json!({
                    "type":"integer", "minimum":1, "maximum":u64::MAX
                });
                schema["properties"]["no_cache"] = serde_json::json!({
                    "type":"boolean", "default":false,
                    "description":"Queries always read the host; no fabricated cache is used"
                });
                schema["required"] = serde_json::json!(["group_id"]);
            }
            Self::MessageId => {
                schema["properties"]["message_id"] = serde_json::json!({
                    "type":"integer", "minimum":i32::MIN, "maximum":i32::MAX
                });
                schema["required"] = serde_json::json!(["message_id"]);
            }
            Self::Record => {
                schema["properties"] = serde_json::json!({
                    "file":{"type":"string", "maxLength":64, "description":"Caligo record file token"},
                    "file_id":{"type":"string", "maxLength":64, "description":"Alternate Caligo record file token"},
                    "out_format":{"type":"string", "enum":["silk"], "default":"silk"}
                });
                schema["oneOf"] = serde_json::json!([{"required":["file"]},{"required":["file_id"]}]);
            }
        }
        schema
    }
}

impl Serialize for ActionInputSchema {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json_schema().serialize(serializer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActionDefinition {
    pub name: &'static str,
    /// Shared capability name. Async aliases retain their distinct acceptance
    /// response policy in `handler.allow_async`; they are not confirmed sends.
    pub canonical: &'static str,
    pub permission: ActionPermission,
    pub handler: ActionHandlerKind,
    pub input_schema: ActionInputSchema,
}

/// Implemented action names only. Availability still depends on the actual
/// sink/store; registration does not claim native or live QQ acceptance.
pub static ACTION_REGISTRY: &[ActionDefinition] = &[
    ActionDefinition {
        name: "send_group_msg",
        canonical: "send_group_msg",
        permission: ActionPermission::Write,
        handler: ActionHandlerKind::Send {
            target: SendTargetKind::Group,
            allow_async: false,
        },
        input_schema: ActionInputSchema::GroupMessage,
    },
    ActionDefinition {
        name: "send_private_msg",
        canonical: "send_private_msg",
        permission: ActionPermission::Write,
        handler: ActionHandlerKind::Send {
            target: SendTargetKind::Private,
            allow_async: false,
        },
        input_schema: ActionInputSchema::PrivateMessage,
    },
    ActionDefinition {
        name: "send_msg",
        canonical: "send_msg",
        permission: ActionPermission::Write,
        handler: ActionHandlerKind::Send {
            target: SendTargetKind::Inferred,
            allow_async: false,
        },
        input_schema: ActionInputSchema::Message,
    },
    ActionDefinition {
        name: "send_group_msg_async",
        canonical: "send_group_msg",
        permission: ActionPermission::Write,
        handler: ActionHandlerKind::Send {
            target: SendTargetKind::Group,
            allow_async: true,
        },
        input_schema: ActionInputSchema::GroupMessage,
    },
    ActionDefinition {
        name: "send_private_msg_async",
        canonical: "send_private_msg",
        permission: ActionPermission::Write,
        handler: ActionHandlerKind::Send {
            target: SendTargetKind::Private,
            allow_async: true,
        },
        input_schema: ActionInputSchema::PrivateMessage,
    },
    ActionDefinition {
        name: "send_msg_async",
        canonical: "send_msg",
        permission: ActionPermission::Write,
        handler: ActionHandlerKind::Send {
            target: SendTargetKind::Inferred,
            allow_async: true,
        },
        input_schema: ActionInputSchema::Message,
    },
    ActionDefinition {
        name: "get_msg",
        canonical: "get_msg",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetMessage,
        input_schema: ActionInputSchema::MessageId,
    },
    ActionDefinition {
        name: "get_login_info",
        canonical: "get_login_info",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetLoginInfo,
        input_schema: ActionInputSchema::Empty,
    },
    ActionDefinition {
        name: "get_record",
        canonical: "get_record",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetRecord,
        input_schema: ActionInputSchema::Record,
    },
    ActionDefinition {
        name: "get_friend_list",
        canonical: "get_friend_list",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetFriendList,
        input_schema: ActionInputSchema::Empty,
    },
    ActionDefinition {
        name: "get_group_list",
        canonical: "get_group_list",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetGroupList,
        input_schema: ActionInputSchema::Empty,
    },
    ActionDefinition {
        name: "get_group_info",
        canonical: "get_group_info",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetGroupInfo,
        input_schema: ActionInputSchema::GroupInfo,
    },
    ActionDefinition {
        name: "get_status",
        canonical: "get_status",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetStatus,
        input_schema: ActionInputSchema::Empty,
    },
    ActionDefinition {
        name: "get_version_info",
        canonical: "get_version_info",
        permission: ActionPermission::Read,
        handler: ActionHandlerKind::GetVersionInfo,
        input_schema: ActionInputSchema::Empty,
    },
];

pub fn resolve_action(name: &str) -> Option<&'static ActionDefinition> {
    ACTION_REGISTRY.iter().find(|entry| entry.name == name)
}

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
    target_kind: SendTargetKind,
    params: &Value,
    sink: &mut dyn SendSink,
    echo: Value,
    allow_async: bool,
) -> ActionResponse {
    let (target, text) = match target_kind {
        SendTargetKind::Group => {
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
        SendTargetKind::Private => {
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
        SendTargetKind::Inferred => {
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
    };
    dispatch_send(sink, echo, SendPayload { target, text }, allow_async)
}

/// Whether an action name belongs to the implemented public API surface.
pub fn is_supported_action(action: &str) -> bool {
    resolve_action(action).is_some()
}

fn public_id(id: &str) -> Option<u64> {
    positive_decimal(id)
        .then(|| id.parse::<u64>().ok())
        .flatten()
}

fn group_data(info: GroupInfo) -> Option<Value> {
    if info.member_count > info.max_member_count {
        return None;
    }
    Some(serde_json::json!({
        "group_id":public_id(&info.group_id)?, "group_name":info.group_name,
        "member_count":info.member_count, "max_member_count":info.max_member_count
    }))
}

fn query_response(query: HostQuery, backend: &dyn QueryBackend, echo: Value) -> ActionResponse {
    query_response_with_voice(query, backend, echo, None)
}

fn query_response_with_voice(
    query: HostQuery,
    backend: &dyn QueryBackend,
    echo: Value,
    expected_voice: Option<&VoiceInfo>,
) -> ActionResponse {
    let invalid =
        || ActionResponse::failed(RET_BAD_REQUEST, "宿主查询响应不符合合同", echo.clone());
    let outcome = backend.query_host(query.clone(), QUERY_TIMEOUT);
    let data = match outcome {
        QueryOutcome::Success(data) => match (query, data) {
            (HostQuery::LoginInfo, QueryData::LoginInfo(info)) => public_id(&info.user_id)
                .map(|user_id| serde_json::json!({"user_id":user_id, "nickname":info.nickname})),
            (HostQuery::FriendList, QueryData::FriendList(list)) => list.into_iter()
                .map(|info| Some(serde_json::json!({
                    "user_id":public_id(&info.user_id)?, "nickname":info.nickname, "remark":info.remark
                })))
                .collect::<Option<Vec<Value>>>().map(Value::Array),
            (HostQuery::GroupList, QueryData::GroupList(list)) => list.into_iter()
                .map(group_data).collect::<Option<Vec<Value>>>().map(Value::Array),
            (HostQuery::GroupInfo { group_id }, QueryData::GroupInfo(info)) if info.group_id == group_id => group_data(info),
            (HostQuery::VoiceDownload { .. }, QueryData::VoiceDownload(download)) => {
                expected_voice.and_then(|expected| voice_data(expected, download))
            }
            _ => None,
        },
        QueryOutcome::Unsupported => return ActionResponse::failed(RET_UNSUPPORTED, "宿主未支持此查询", echo),
        QueryOutcome::NotReady => return ActionResponse::failed(RET_NOT_READY, "宿主查询尚未就绪", echo),
        QueryOutcome::PermissionDenied => return ActionResponse::failed(RET_PERMISSION_DENIED, "宿主拒绝查询权限", echo),
        QueryOutcome::Rejected { code, reason } => return ActionResponse::failed(
            RET_BAD_REQUEST, &format!("宿主查询拒绝 (code={code}): {reason}"), echo),
        QueryOutcome::Timeout => return ActionResponse::failed(RET_NOT_CONFIRMED, "宿主查询超时", echo),
        QueryOutcome::Disconnected => return ActionResponse::failed(RET_NOT_CONFIRMED, "宿主查询连接已断开", echo),
        QueryOutcome::InvalidResponse => return invalid(),
    };
    match data {
        Some(data) => ActionResponse::ok(data, echo),
        None => invalid(),
    }
}

fn voice_data(expected: &VoiceInfo, download: VoiceDownload) -> Option<Value> {
    let size = expected.size;
    if &download.info != expected || !(1..=MAX_VOICE_DOWNLOAD_BYTES).contains(&size)
        || download.silk_hex.len() != size as usize * 2
        || !download.silk_hex.is_ascii()
    {
        return None;
    }
    let bytes = (0..download.silk_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&download.silk_hex[i..i + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    if !bytes.starts_with(b"#!SILK_V3") && !bytes.starts_with(b"\x02#!SILK_V3") {
        return None;
    }
    Some(serde_json::json!({
        "base64":STANDARD.encode(bytes), "file_size":size.to_string(), "file_name":"voice.silk"
    }))
}

fn get_record(params: &Value, store: &dyn StoreReader, queries: &dyn QueryBackend, echo: Value) -> ActionResponse {
    let file = match (params.get("file"), params.get("file_id")) {
        (Some(value), None) | (None, Some(value)) => value.as_str(),
        _ => None,
    };
    let Some(file) = file.filter(|file| !file.is_empty() && file.len() <= 64) else {
        return ActionResponse::failed(RET_BAD_PARAMS, "必须提供一个语音文件标识", echo);
    };
    match params.get("out_format") {
        None => {}
        Some(value) if value.as_str() == Some("silk") => {}
        Some(Value::String(_)) => return ActionResponse::failed(RET_UNSUPPORTED, "当前仅支持原始 Silk", echo),
        Some(_) => return ActionResponse::failed(RET_BAD_PARAMS, "out_format 必须是字符串", echo),
    }
    let Some(info) = store.voice_info(file) else {
        return ActionResponse::failed(RET_BAD_REQUEST, "语音文件标识不存在", echo);
    };
    if info.size > MAX_VOICE_DOWNLOAD_BYTES {
        return ActionResponse::failed(RET_UNSUPPORTED, "语音超过当前下载上限", echo);
    }
    let query = HostQuery::VoiceDownload {
        peer_uid: info.peer_uid.clone(),
        message_id: info.message_id.clone(),
        element_id: info.element_id.clone(),
    };
    query_response_with_voice(query, queries, echo, Some(&info))
}

/// Compatibility entry for callers without a host query backend. Directory
/// actions remain explicitly unsupported; legacy login readers retain their data.
pub fn handle_action(
    action: &str,
    params: &Value,
    echo: Value,
    sink: &mut dyn SendSink,
    store: &dyn StoreReader,
) -> ActionResponse {
    if action == "get_login_info" {
        return match store.login_info() {
            Some((user_id, nickname)) => ActionResponse::ok(
                serde_json::json!({"user_id":user_id, "nickname":nickname}),
                echo,
            ),
            None => ActionResponse::failed(RET_NOT_READY, "账号身份尚未就绪", echo),
        };
    }
    handle_action_with_queries(action, params, echo, sink, store, &UnsupportedQueries)
}

/// Process one action through the shared registry and actual host query backend.
pub fn handle_action_with_queries(
    action: &str,
    params: &Value,
    echo: Value,
    sink: &mut dyn SendSink,
    store: &dyn StoreReader,
    queries: &dyn QueryBackend,
) -> ActionResponse {
    let Some(definition) = resolve_action(action) else {
        return ActionResponse::failed(RET_UNSUPPORTED, "未支持动作", echo);
    };
    match definition.handler {
        ActionHandlerKind::Send {
            target,
            allow_async,
        } => send_payload(target, params, sink, echo, allow_async),
        ActionHandlerKind::GetMessage => {
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
        ActionHandlerKind::GetLoginInfo => query_response(HostQuery::LoginInfo, queries, echo),
        ActionHandlerKind::GetRecord => get_record(params, store, queries, echo),
        ActionHandlerKind::GetFriendList => query_response(HostQuery::FriendList, queries, echo),
        ActionHandlerKind::GetGroupList => query_response(HostQuery::GroupList, queries, echo),
        ActionHandlerKind::GetGroupInfo => {
            let Some(group_id) = params
                .get("group_id")
                .and_then(Value::as_u64)
                .filter(|id| *id > 0)
            else {
                return ActionResponse::failed(RET_BAD_PARAMS, "group_id 必须是正整数", echo);
            };
            if params
                .get("no_cache")
                .is_some_and(|value| !value.is_boolean())
            {
                return ActionResponse::failed(RET_BAD_PARAMS, "no_cache 必须是布尔值", echo);
            }
            query_response(
                HostQuery::GroupInfo {
                    group_id: group_id.to_string(),
                },
                queries,
                echo,
            )
        }
        ActionHandlerKind::GetVersionInfo => ActionResponse::ok(
            serde_json::json!({
                "app_name": "Caligo",
                "app_version": env!("CARGO_PKG_VERSION"),
                "protocol_version": "v11",
                "impl": "Caligo",
                "os": "Windows"
            }),
            echo,
        ),
        ActionHandlerKind::GetStatus => match store.online_status() {
            Some(online) => {
                ActionResponse::ok(serde_json::json!({"online": online, "good": online}), echo)
            }
            None => ActionResponse::failed(RET_NOT_READY, "daemon/session 状态尚未就绪", echo),
        },
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
        fn voice_info(&self, file: &str) -> Option<VoiceInfo> {
            (file == "caligo-voice:10001:42").then(|| VoiceInfo {
                peer_uid: "u_friend".into(),
                message_id: "100".into(),
                element_id: "200".into(),
                size: 10,
                md5: "0123456789abcdef0123456789abcdef".into(),
            })
        }
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
    fn registry_has_unique_names_and_resolvable_canonical_actions() {
        let names: std::collections::HashSet<_> =
            ACTION_REGISTRY.iter().map(|entry| entry.name).collect();
        assert_eq!(names.len(), ACTION_REGISTRY.len());
        assert_eq!(
            names,
            std::collections::HashSet::from([
                "send_group_msg",
                "send_private_msg",
                "send_msg",
                "send_group_msg_async",
                "send_private_msg_async",
                "send_msg_async",
                "get_msg",
                "get_record",
                "get_login_info",
                "get_friend_list",
                "get_group_list",
                "get_group_info",
                "get_status",
                "get_version_info",
            ])
        );
        for entry in ACTION_REGISTRY {
            assert!(is_supported_action(entry.name));
            assert_eq!(resolve_action(entry.name), Some(entry));
            let canonical = resolve_action(entry.canonical).unwrap();
            assert_eq!(canonical.name, canonical.canonical);
            assert_eq!(entry.permission, canonical.permission);
            assert_eq!(entry.input_schema, canonical.input_schema);
        }
        for name in ["", "GET_MSG", "delete_msg"] {
            assert!(!is_supported_action(name));
            let mut sink = FakeSink {
                ready: true,
                ..Default::default()
            };
            let echo = serde_json::json!({"unsupported":name});
            let response = handle_action(name, &Value::Null, echo.clone(), &mut sink, &FakeStore);
            assert_eq!(response.retcode, RET_UNSUPPORTED);
            assert_eq!(response.echo, echo);
            assert!(sink.last.is_empty());
        }
    }

    #[derive(Default)]
    struct ConfirmedSink(Vec<SendPayload>);
    impl SendSink for ConfirmedSink {
        fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String> {
            self.0.push(payload.clone());
            Ok(SendDisposition::Confirmed { message_id: 42 })
        }
    }

    struct FakeQueries;
    impl QueryBackend for FakeQueries {
        fn query_host(&self, query: HostQuery, _: Duration) -> QueryOutcome {
            QueryOutcome::Success(match query {
                HostQuery::LoginInfo => QueryData::LoginInfo(caligo_model::capability::LoginInfo {
                    user_id: "1694717255".into(),
                    nickname: "caligo".into(),
                    online: true,
                }),
                HostQuery::FriendList => QueryData::FriendList(vec![]),
                HostQuery::GroupList => QueryData::GroupList(vec![]),
                HostQuery::GroupInfo { group_id } => QueryData::GroupInfo(GroupInfo {
                    group_id,
                    group_name: "Lab group".into(),
                    member_count: 3,
                    max_member_count: 50,
                }),
                HostQuery::VoiceDownload { peer_uid, message_id, element_id } => {
                    QueryData::VoiceDownload(VoiceDownload {
                        info: VoiceInfo { peer_uid, message_id, element_id, ..FakeStore.voice_info("caligo-voice:10001:42").unwrap() },
                        silk_hex: "02232153494c4b5f5633".into(),
                    })
                }
                _ => panic!("unexpected public query"),
            })
        }
    }

    #[test]
    fn every_registered_action_reaches_its_normal_handler() {
        for entry in ACTION_REGISTRY {
            let mut sink = ConfirmedSink::default();
            let params = match entry.input_schema {
                ActionInputSchema::GroupMessage | ActionInputSchema::Message => {
                    serde_json::json!({"group_id":7,"message":"hello"})
                }
                ActionInputSchema::PrivateMessage => {
                    serde_json::json!({"user_id":8,"message":"hello"})
                }
                ActionInputSchema::MessageId => serde_json::json!({"message_id":42}),
                ActionInputSchema::Record => serde_json::json!({"file":"caligo-voice:10001:42"}),
                ActionInputSchema::GroupInfo => serde_json::json!({"group_id":7,"no_cache":true}),
                ActionInputSchema::Empty => serde_json::json!({}),
            };
            let echo = serde_json::json!([entry.name, {"seq":1}]);
            let response = handle_action_with_queries(
                entry.name,
                &params,
                echo.clone(),
                &mut sink,
                &FakeStore,
                &FakeQueries,
            );
            assert_eq!(response.status, "ok", "{}: {response:?}", entry.name);
            assert_eq!(response.retcode, 0, "{}", entry.name);
            assert_eq!(response.echo, echo);
            match entry.handler {
                ActionHandlerKind::Send { target, .. } => {
                    let target = match target {
                        SendTargetKind::Group | SendTargetKind::Inferred => {
                            SendTarget::Group { group_id: 7 }
                        }
                        SendTargetKind::Private => SendTarget::Private { user_id: 8 },
                    };
                    assert_eq!(
                        sink.0,
                        [SendPayload {
                            target,
                            text: "hello".into()
                        }]
                    );
                    assert_eq!(response.data, serde_json::json!({"message_id":42}));
                }
                ActionHandlerKind::GetMessage => {
                    assert_eq!(response.data["message_id"], 42);
                    assert_eq!(response.data["real_id"], 398263);
                }
                ActionHandlerKind::GetRecord => {
                    assert_eq!(response.data["base64"], "AiMhU0lMS19WMw==");
                }
                ActionHandlerKind::GetLoginInfo => assert_eq!(
                    response.data,
                    serde_json::json!({"user_id":1694717255u64,"nickname":"caligo"})
                ),
                ActionHandlerKind::GetFriendList | ActionHandlerKind::GetGroupList => {
                    assert_eq!(response.data, serde_json::json!([]))
                }
                ActionHandlerKind::GetGroupInfo => assert_eq!(
                    response.data,
                    serde_json::json!({
                        "group_id":7,"group_name":"Lab group","member_count":3,"max_member_count":50
                    })
                ),
                ActionHandlerKind::GetStatus => assert_eq!(
                    response.data,
                    serde_json::json!({"online":false,"good":false})
                ),
                ActionHandlerKind::GetVersionInfo => {
                    assert_eq!(response.data["app_name"], "Caligo");
                    assert_eq!(response.data["protocol_version"], "v11");
                }
            }
            if entry.permission == ActionPermission::Read {
                assert!(sink.0.is_empty());
            }
        }
    }

    #[test]
    fn async_aliases_share_payload_and_terminal_results_with_canonical_actions() {
        struct ResultSink {
            result: SendDisposition,
            calls: Vec<SendPayload>,
        }
        impl SendSink for ResultSink {
            fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String> {
                self.calls.push(payload.clone());
                Ok(self.result.clone())
            }
        }
        for alias in ACTION_REGISTRY
            .iter()
            .filter(|entry| entry.name != entry.canonical)
        {
            let canonical = resolve_action(alias.canonical).unwrap();
            let ActionHandlerKind::Send {
                target,
                allow_async: true,
            } = alias.handler
            else {
                panic!("unexpected alias: {}", alias.name);
            };
            assert_eq!(
                canonical.handler,
                ActionHandlerKind::Send {
                    target,
                    allow_async: false
                }
            );
            for params in [
                serde_json::json!({"group_id":7,"user_id":8,"message":"hello"}),
                serde_json::json!({"message_type":"private","user_id":8,"message":[{"type":"text","data":{"text":"hello"}}]}),
            ] {
                // Direct sends require their own target; supply it for both cases.
                let mut params = params;
                params["group_id"] = serde_json::json!(7);
                for disposition in [
                    SendDisposition::Confirmed { message_id: 42 },
                    SendDisposition::DeliveryUnknown {
                        reason: "unknown; do not retry".into(),
                    },
                    SendDisposition::Unsupported {
                        reason: "mapping unavailable".into(),
                    },
                    SendDisposition::AcceptedUnconfirmed,
                ] {
                    let mut standard_sink = ResultSink {
                        result: disposition.clone(),
                        calls: vec![],
                    };
                    let mut alias_sink = ResultSink {
                        result: disposition.clone(),
                        calls: vec![],
                    };
                    let echo = serde_json::json!({"alias":alias.name});
                    let standard = handle_action(
                        canonical.name,
                        &params,
                        echo.clone(),
                        &mut standard_sink,
                        &FakeStore,
                    );
                    let asynchronous = handle_action(
                        alias.name,
                        &params,
                        echo.clone(),
                        &mut alias_sink,
                        &FakeStore,
                    );
                    assert_eq!(standard_sink.calls.len(), 1);
                    assert_eq!(standard_sink.calls, alias_sink.calls);
                    assert_eq!(asynchronous.echo, echo);
                    if disposition == SendDisposition::AcceptedUnconfirmed {
                        assert_eq!(standard.retcode, RET_NOT_CONFIRMED);
                        assert_eq!(standard.data, Value::Null);
                        assert_eq!(asynchronous, ActionResponse::async_accepted(echo));
                    } else {
                        assert_eq!(standard, asynchronous);
                    }
                }
            }
        }
    }

    #[test]
    fn registry_permissions_and_serialized_input_schemas_describe_implemented_inputs() {
        let serialized = serde_json::to_value(ACTION_REGISTRY).unwrap();
        for (entry, metadata) in ACTION_REGISTRY.iter().zip(serialized.as_array().unwrap()) {
            let schema = &metadata["input_schema"];
            assert_eq!(schema["type"], "object");
            let expected_schema = match entry.handler {
                ActionHandlerKind::Send { target, .. } => {
                    assert_eq!(entry.permission, ActionPermission::Write);
                    assert_eq!(metadata["permission"], "write");
                    assert_eq!(
                        schema["properties"]["message"]["anyOf"][0]["type"],
                        "string"
                    );
                    assert_eq!(
                        schema["properties"]["message"]["anyOf"][1]["items"]["properties"]["type"]
                            ["const"],
                        "text"
                    );
                    let key = if target == SendTargetKind::Private {
                        "user_id"
                    } else {
                        "group_id"
                    };
                    assert_eq!(schema["properties"][key]["minimum"], 0);
                    assert_eq!(schema["properties"][key]["maximum"], u64::MAX);
                    match target {
                        SendTargetKind::Group => {
                            assert_eq!(
                                schema["required"],
                                serde_json::json!(["group_id", "message"])
                            );
                            ActionInputSchema::GroupMessage
                        }
                        SendTargetKind::Private => {
                            assert_eq!(
                                schema["required"],
                                serde_json::json!(["user_id", "message"])
                            );
                            ActionInputSchema::PrivateMessage
                        }
                        SendTargetKind::Inferred => {
                            assert_eq!(schema["required"], serde_json::json!(["message"]));
                            assert_eq!(schema["anyOf"].as_array().unwrap().len(), 2);
                            ActionInputSchema::Message
                        }
                    }
                }
                handler => {
                    assert_eq!(entry.permission, ActionPermission::Read);
                    assert_eq!(metadata["permission"], "read");
                    if handler == ActionHandlerKind::GetMessage {
                        assert_eq!(schema["required"], serde_json::json!(["message_id"]));
                        assert_eq!(schema["properties"]["message_id"]["minimum"], i32::MIN);
                        assert_eq!(schema["properties"]["message_id"]["maximum"], i32::MAX);
                        ActionInputSchema::MessageId
                    } else if handler == ActionHandlerKind::GetRecord {
                        assert_eq!(schema["oneOf"], serde_json::json!([{"required":["file"]},{"required":["file_id"]}]));
                        assert_eq!(schema["properties"]["out_format"]["enum"], serde_json::json!(["silk"]));
                        ActionInputSchema::Record
                    } else if handler == ActionHandlerKind::GetGroupInfo {
                        assert_eq!(schema["required"], serde_json::json!(["group_id"]));
                        assert_eq!(schema["properties"]["group_id"]["minimum"], 1);
                        assert_eq!(schema["properties"]["group_id"]["maximum"], u64::MAX);
                        assert_eq!(schema["properties"]["no_cache"]["type"], "boolean");
                        ActionInputSchema::GroupInfo
                    } else {
                        assert_eq!(schema["properties"], serde_json::json!({}));
                        ActionInputSchema::Empty
                    }
                }
            };
            assert_eq!(entry.input_schema, expected_schema);
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
    fn directory_queries_do_not_report_learned_peers_as_complete_platform_data() {
        // Preserve the internal index without presenting its partial observations
        // as a group directory, placeholder names/counts, or an empty friend list.
        assert_eq!(GroupStore.group_peers(), ["1098345913", "1102382705"]);
        for store in [&GroupStore as &dyn StoreReader, &FakeStore] {
            for (action, params) in [
                ("get_group_list", Value::Null),
                (
                    "get_group_info",
                    serde_json::json!({"group_id":1102382705u64}),
                ),
                ("get_group_info", serde_json::json!({"group_id":999u64})),
                ("get_friend_list", Value::Null),
            ] {
                assert!(is_supported_action(action));
                assert!(resolve_action(action).is_some());
                let echo = serde_json::json!({"action":action, "request":[1, false]});
                let mut sink = ConfirmedSink::default();
                let response = handle_action(action, &params, echo.clone(), &mut sink, store);
                assert_eq!(response.status, "failed");
                assert_eq!(response.retcode, RET_UNSUPPORTED);
                assert_eq!(response.data, Value::Null);
                assert_eq!(response.echo, echo);
                assert!(sink.0.is_empty());
            }
        }
    }

    #[test]
    fn unsupported_group_info_rejects_missing_or_invalid_id_without_fake_data() {
        for params in [
            Value::Null,
            serde_json::json!({}),
            serde_json::json!({"group_id":null}),
            serde_json::json!({"group_id":0}),
            serde_json::json!({"group_id":-1}),
            serde_json::json!({"group_id":"1102382705"}),
            serde_json::json!({"group_id":1.5}),
            serde_json::json!({"group_id":true}),
        ] {
            let echo = serde_json::json!(["invalid", params.clone()]);
            let mut sink = ConfirmedSink::default();
            let response = handle_action(
                "get_group_info",
                &params,
                echo.clone(),
                &mut sink,
                &GroupStore,
            );
            assert_eq!(response.status, "failed");
            assert_eq!(response.retcode, RET_BAD_PARAMS);
            assert_eq!(response.data, Value::Null);
            assert_eq!(response.echo, echo);
            assert!(sink.0.is_empty());
        }
        // Valid request shape still has no actual directory backend.
        let response = handle_action(
            "get_group_info",
            &serde_json::json!({"group_id":u64::MAX}),
            Value::Null,
            &mut ConfirmedSink::default(),
            &GroupStore,
        );
        assert_eq!(response.retcode, RET_UNSUPPORTED);
        assert_eq!(response.data, Value::Null);
    }
}
