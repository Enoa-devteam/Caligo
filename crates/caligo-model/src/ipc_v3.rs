//! IPC v3 语义消息(计划 §6.2/C9:真实宿主身份握手)。
//!
//! v3 = v2 + bridge 侧宿主身份进 Hello + 有界发送响应期限/诊断:
//! - `host_nonce`:QQ bridge 宿主实例随机 nonce(宿主进程内一次生成,
//!   保持到 QQ 退出;PID 数字重用也无法与旧宿主混淆);
//! - `host_process_created_utc`:宿主进程创建时间(RFC3339 UTC;取不到
//!   必须如实为空)。
//!
//! v2 定义原样保留(`ipc_v2`);daemon 只受理 v3 握手 —— 旧消息因缺少
//! 必填字段反序列化失败而显式拒绝,不靠 serde 默认值放行(C9 反例 T05)。
//! frame/EventPayload/Role 与 v2 共用；发送结果与 Dispatch DTO 在 v3 独立。

use crate::capability::{HostQuery, QueryCapability, QueryOutcome, QueryScope};
use serde::{Deserialize, Serialize};

/// Read-only query contract, advertised separately after the v3 Hello binding.
pub const CAPABILITY_QUERY_REVISION: u32 = 1;

/// IPC 语义协议 v3。v2 保持原定义供历史对照;不接受 v2/v1 握手降级。
pub const PROTOCOL_VERSION_V3: u32 = 3;
/// Bridge/core business receipt contract revision, independent of frame/IPC v3.
pub const BUSINESS_PROTOCOL_REVISION: u32 = 3;

pub use super::ipc_v2::{EventPayload, Role};

/// v3 receipt payload carries diagnostics for ambiguous native response codes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum OutcomePayload {
    Success { native_id: String },
    AcceptedUnconfirmed,
    Failure { reason: String },
    Unknown { reason: Option<String> },
}

/// v3 core → bridge messages. v2 remains unchanged for historical readers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum CoreToBridgeMsg {
    HelloAck {
        protocol_version: u32,
        core_build: String,
        connection_epoch: u64,
        accepted: bool,
        reject_reason: Option<String>,
    },
    Dispatch {
        request_id: String,
        /// Unique sequence authority is core; persisted before dispatch.
        wire_seq: u32,
        /// Remaining response budget measured from durable request acceptance.
        timeout_ms: u64,
        target: serde_json::Value,
        text: String,
    },
    DispatchPrivateVoice {
        scope: QueryScope,
        request_id: String,
        wire_seq: u32,
        timeout_ms: u64,
        peer_uin: String,
        silk: Vec<u8>,
    },
    DispatchGroupVoice {
        scope: QueryScope,
        request_id: String,
        wire_seq: u32,
        timeout_ms: u64,
        group_id: String,
        silk: Vec<u8>,
    },
    DispatchImage {
        scope: QueryScope,
        request_id: String,
        wire_seq: u32,
        timeout_ms: u64,
        chat_type: u32,
        peer_uin: String,
        png: Vec<u8>,
    },
    Query {
        request_id: String,
        scope: QueryScope,
        timeout_ms: u64,
        query: HostQuery,
    },
    HealthAck {},
    EventAck {
        event_seq: u64,
        suppressed: bool,
    },
    NativeStartedAck {
        request_id: String,
    },
    SendResultAck {
        request_id: String,
        terminal: bool,
    },
    Stopped {},
    Reject {
        reason: String,
    },
}

/// bridge → core 的 v3 消息(Hello 增加宿主身份;其余与 v2 一致)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum BridgeMsg {
    Hello {
        protocol_version: u32,
        /// Business receipt/state contract; mismatches are rejected, no downgrade.
        business_protocol_revision: u32,
        bridge_build: String,
        /// 认证凭据(hex;OS 随机,经 bootstrap 私有通道交付)。
        auth_token: String,
        role: Role,
        /// 期望接入的会话代次(bridge 侧已核实的会话身份)。
        session_generation: u64,
        account: String,
        /// 模块基线标识(与 v2 Hello 一致)。
        module_baseline: String,
        /// 宿主实例随机 nonce(进程内一次生成,保持到 QQ 退出)。
        host_nonce: u64,
        /// 宿主进程创建时间(RFC3339 UTC;取不到必须如实为空)。
        host_process_created_utc: String,
    },
    /// 接收事件上报(core 持久化成功 = EventAck 依据)。
    Event {
        event: EventPayload,
    },
    /// bridge 确认已进入 native 调用(执行侧受理后才可发送 —— C5)。
    NativeStarted {
        request_id: String,
    },
    /// 发送结果(receipt 契约三分类,与 runtime::NativeResult 对应)。
    SendResult {
        request_id: String,
        outcome: OutcomePayload,
    },
    QueryCapabilities {
        revision: u32,
        scope: QueryScope,
        capabilities: Vec<QueryCapability>,
    },
    SendCapabilities {
        scope: QueryScope,
        private_text: bool,
        group_text: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        private_voice: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        group_voice: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        private_image: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        group_image: bool,
    },
    QueryResult {
        request_id: String,
        scope: QueryScope,
        outcome: QueryOutcome,
    },
    Health {
        ingress_pending: usize,
        native_ops_total: u64,
    },
    Stop {},
}

pub use super::ipc_v2::{ControlMsg, CoreToControlMsg};

#[cfg(test)]
mod tests {
    use super::*;

    /// v2 Hello 缺少 v3 必填字段 → 反序列化必须失败(不允许默认值放行)。
    #[test]
    fn v2_hello_without_host_identity_fails_to_decode() {
        let v2 = r#"{"t":"hello","d":{
            "protocol_version":2,
            "bridge_build":"b",
            "auth_token":"tok",
            "role":"bridge",
            "session_generation":1,
            "account":"10001",
            "module_baseline":"m"
        }}"#;
        let r: Result<BridgeMsg, _> = serde_json::from_str(v2);
        assert!(r.is_err(), "v2 Hello 不得被 v3 静默接受");
    }

    /// v3 Hello 完整字段可解码。
    #[test]
    fn v3_hello_with_host_identity_decodes() {
        let v3 = r#"{"t":"hello","d":{
            "protocol_version":3,
            "business_protocol_revision":2,
            "bridge_build":"b",
            "auth_token":"tok",
            "role":"bridge",
            "session_generation":1,
            "account":"10001",
            "module_baseline":"m",
            "host_nonce":123456789,
            "host_process_created_utc":"2026-10-08T00:00:00Z"
        }}"#;
        let msg: BridgeMsg = serde_json::from_str(v3).unwrap();
        match msg {
            BridgeMsg::Hello {
                host_nonce,
                host_process_created_utc,
                ..
            } => {
                assert_eq!(host_nonce, 123456789);
                assert_eq!(host_process_created_utc, "2026-10-08T00:00:00Z");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn old_v3_hello_without_business_revision_is_rejected() {
        let old_v3 = r#"{"t":"hello","d":{
            "protocol_version":3,
            "bridge_build":"b",
            "auth_token":"tok",
            "role":"bridge",
            "session_generation":1,
            "account":"10001",
            "module_baseline":"m",
            "host_nonce":123456789,
            "host_process_created_utc":"2026-10-08T00:00:00Z"
        }}"#;
        assert!(serde_json::from_str::<BridgeMsg>(old_v3).is_err());
    }

    #[test]
    fn capability_query_revision_and_typed_wire_round_trip() {
        let scope = QueryScope {
            account: "10001".into(),
            host_nonce: 7,
            session_generation: 1,
            connection_epoch: 2,
        };
        let advertise = BridgeMsg::QueryCapabilities {
            revision: CAPABILITY_QUERY_REVISION,
            scope: scope.clone(),
            capabilities: vec![QueryCapability::FriendList],
        };
        assert_eq!(
            serde_json::from_slice::<BridgeMsg>(&serde_json::to_vec(&advertise).unwrap()).unwrap(),
            advertise
        );
        let query = CoreToBridgeMsg::Query {
            request_id: "query-lab".into(),
            scope: scope.clone(),
            timeout_ms: 1_000,
            query: HostQuery::FriendList,
        };
        assert_eq!(
            serde_json::from_slice::<CoreToBridgeMsg>(&serde_json::to_vec(&query).unwrap())
                .unwrap(),
            query
        );
        let send_capabilities = BridgeMsg::SendCapabilities {
            scope: scope.clone(),
            private_text: true,
            group_text: false,
            private_voice: false,
            group_voice: false,
            private_image: false,
            group_image: false,
        };
        assert_eq!(
            serde_json::from_slice::<BridgeMsg>(&serde_json::to_vec(&send_capabilities).unwrap())
                .unwrap(),
            send_capabilities
        );
        let legacy = serde_json::to_value(&send_capabilities).unwrap();
        assert!(legacy["d"].get("group_voice").is_none());
        assert!(legacy["d"].get("private_voice").is_none());
        assert!(legacy["d"].get("private_image").is_none());
        assert!(legacy["d"].get("group_image").is_none());
        assert!(matches!(serde_json::from_value::<BridgeMsg>(legacy.clone()).unwrap(),
            BridgeMsg::SendCapabilities {private_image:false,group_image:false,..}));
        let image = CoreToBridgeMsg::DispatchImage {scope:scope.clone(),request_id:"image-lab".into(),
            wire_seq:9,timeout_ms:1000,chat_type:2,peer_uin:"30003".into(),png:vec![1,2,3]};
        assert_eq!(serde_json::from_value::<CoreToBridgeMsg>(serde_json::to_value(&image).unwrap()).unwrap(),image);
        let mut group = legacy;
        group["d"]["group_voice"] = serde_json::json!(true);
        assert!(matches!(
            serde_json::from_value::<BridgeMsg>(group).unwrap(),
            BridgeMsg::SendCapabilities { group_voice: true, private_voice: false, .. }
        ));
        let result = BridgeMsg::QueryResult {
            request_id: "query-lab".into(),
            scope,
            outcome: QueryOutcome::Unsupported,
        };
        assert_eq!(
            serde_json::from_slice::<BridgeMsg>(&serde_json::to_vec(&result).unwrap()).unwrap(),
            result
        );
        assert!(serde_json::from_str::<CoreToBridgeMsg>(r#"{"t":"query","d":{"request_id":"q","scope":{"account":"10001","host_nonce":7,"session_generation":1,"connection_epoch":2},"timeout_ms":10,"query":{"kind":"eval","params":"script"}}}"#).is_err());
        assert!(serde_json::from_str::<BridgeMsg>(r#"{"t":"query_capabilities","d":{"scope":{"account":"10001","host_nonce":7,"session_generation":1,"connection_epoch":2},"capabilities":[]}}"#).is_err());
    }

    #[test]
    fn v3_dispatch_timeout_and_unknown_diagnostic_round_trip() {
        let dispatch = CoreToBridgeMsg::Dispatch {
            request_id: "R-timeout".into(),
            wire_seq: 12,
            timeout_ms: 18_000,
            target: serde_json::json!({"kind":"group","peer":"123"}),
            text: "hello".into(),
        };
        let encoded = serde_json::to_vec(&dispatch).unwrap();
        assert!(String::from_utf8_lossy(&encoded).contains("\"timeout_ms\":18000"));
        let decoded: CoreToBridgeMsg = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, dispatch);

        let result = BridgeMsg::SendResult {
            request_id: "R-timeout".into(),
            outcome: OutcomePayload::Unknown {
                reason: Some("errCode=1006501; provenance unverified".into()),
            },
        };
        let encoded = serde_json::to_vec(&result).unwrap();
        let decoded: BridgeMsg = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, result);
    }
}
