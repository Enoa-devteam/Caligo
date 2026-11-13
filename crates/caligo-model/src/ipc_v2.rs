//! IPC v2 语义消息(K4-D5 定义,K4-D7b 下沉至 model):桥方向与控制方向
//! 的固定消息集。core 为 server,bridge/control 为 client;双方共用本定义。
//! 帧内 payload = JSON(serde tag/content 约定不变 —— 与 daemon 已验收格式一致)。

use serde::{Deserialize, Serialize};

/// IPC 语义协议 v2。v1(`crate::ipc::PROTOCOL_VERSION`)仅供 K1 probe 握手回归测试。
pub const PROTOCOL_VERSION_V2: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// bridge 宿主侧(接收 Dispatch、上报 Event/结果)。
    Bridge,
    /// 测试客户端/控制面(提交 SendText、查询、Drain、Stop)。
    Control,
}

/// bridge → core 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum BridgeMsg {
    Hello {
        protocol_version: u32,
        bridge_build: String,
        /// 认证凭据(hex;OS 随机,经 bootstrap 私有通道交付)。
        auth_token: String,
        role: Role,
        /// 期望接入的会话代次(bridge 侧已核实的会话身份)。
        session_generation: u64,
        account: String,
        /// 模块基线标识(T01 的逻辑层,与 v1 Hello 一致)。
        module_baseline: String,
    },
    /// 接收事件上报(core 持久化成功 = EventAck 依据)。
    Event {
        event: EventPayload,
    },
    /// bridge 确认已进入 native 调用。
    NativeStarted {
        request_id: String,
    },
    /// 发送结果(receipt 契约三分类,与 runtime::NativeResult 对应)。
    SendResult {
        request_id: String,
        outcome: OutcomePayload,
    },
    Health {
        ingress_pending: usize,
        native_ops_total: u64,
    },
    Stop {},
}

/// core → bridge 的 v2 消息。
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
    /// 派发参数化发送(带持久 request_id)。
    Dispatch {
        request_id: String,
        /// 唯一序号 authority 在 core；已先落 journal 再进入 bridge。
        wire_seq: u32,
        target: serde_json::Value,
        text: String,
    },
    HealthAck {},
    /// 事件受理回执(core 已持久化;suppressed=true 表示重复已抑制,
    /// bridge 据此前移缓存 —— 计划 §6.5:ACK 只在持久化后前移)。
    EventAck {
        event_seq: u64,
        suppressed: bool,
    },
    /// core 已将 NativeStarted 记入 actor 状态(bridge 可释放该通知槽)。
    NativeStartedAck {
        request_id: String,
    },
    /// core 已持久应用或审计 SendResult(bridge 可释放该通知槽)。
    SendResultAck {
        request_id: String,
        /// false 仅确认 AcceptedUnconfirmed；bridge 必须保留未来终态槽。
        terminal: bool,
    },
    Stopped {},
    Reject {
        reason: String,
    },
}

/// control → core 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum ControlMsg {
    Hello {
        protocol_version: u32,
        client_build: String,
        auth_token: String,
        role: Role,
    },
    SendText {
        request_id: String,
        target: serde_json::Value,
        text: String,
        deadline_ms: u64,
    },
    QueryRequest {
        request_id: String,
    },
    CancelQueued {
        request_id: String,
    },
    /// 拉取已持久化事件(交付游标前移)。
    DrainEvents {
        max: usize,
    },
    Health {},
    Stop {},
}

/// core → control 的 v2 消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum CoreToControlMsg {
    HelloAck {
        protocol_version: u32,
        core_build: String,
        connection_epoch: u64,
        accepted: bool,
        reject_reason: Option<String>,
    },
    /// 受理确认(先持久后受理;拒绝时 reason 属"未受理"而非发送失败)。
    SendAccepted {
        request_id: String,
        state: String,
    },
    QueryResult {
        request_id: String,
        state: Option<String>,
        native_id: Option<String>,
    },
    CancelResult {
        request_id: String,
        /// cancelled_unsent / too_late / not_found。
        verdict: String,
    },
    EventsBatch {
        events: Vec<EventPayload>,
    },
    HealthAck {},
    Stopped {},
    Reject {
        reason: String,
    },
}

/// 事件载荷(与 journal::EventRecord 字段对齐;session 为 SessionKey DTO)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventPayload {
    pub event_seq: u64,
    pub session_generation: u64,
    pub session: serde_json::Value,
    pub direction: String,
    pub sender: String,
    pub native_id: String,
    pub text: String,
    pub platform_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper_message: Option<crate::wrapper_message::WrapperMessageEnvelope>,
    /// Optional, explicitly resolved OneBot fields. Older bridge builds omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onebot_projection: Option<crate::OneBotMessageProjection>,
    pub observed_at_unix_ms: u64,
    pub source: String,
}

/// 发送结果三分类(计划 §6.6)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum OutcomePayload {
    Success {
        native_id: String,
    },
    /// 已受理但没有稳定 native identity,不能当作成功。
    AcceptedUnconfirmed,
    Failure {
        reason: String,
    },
    Unknown,
}
