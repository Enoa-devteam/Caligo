//! Caligo 数据模型:账号、会话、方向与会话代次。
//!
//! 首轮约束(计划 §6):不承载 QQ 对象或裸指针,不绑定 OneBot。
//! 原生消息标识的实际字段组合(如 seq/random/内部 UID)必须等 K2/K3 现场调查
//! 确认,本 crate 只提供不透明占位,禁止在调查前编造。

use std::fmt;

/// 保留同一原请求迟到强回执的最长窗口；超窗后只保留 core 的
/// DeliveryUnknown 台账，bridge 释放关联与 resident 结果容量。
pub const LATE_RECEIPT_GRACE_MS: u64 = 60_000;

/// QQ 账号 ID。
///
/// 以字符串保存:不对号码取值范围、前导零或数值上限做未经证实的假设。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(pub String);

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 会话种类。同数字的群号与好友号必须可区分(验收反例 T03)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionKind {
    /// 好友私聊。
    Private,
    /// 普通群聊。
    Group,
}

impl fmt::Display for SessionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionKind::Private => f.write_str("private"),
            SessionKind::Group => f.write_str("group"),
        }
    }
}

/// 会话对端 ID(好友号或群号)。以字符串保存,理由同 [`AccountId`]。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Peer(pub String);

/// 会话键:账号 + 种类 + 对端。
///
/// 计划 §6.3:同数字群号和好友号不能混淆;账号 ID 独立于连接代次。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub account: AccountId,
    pub kind: SessionKind,
    pub peer: Peer,
}

impl SessionKey {
    pub fn new(account: impl Into<String>, kind: SessionKind, peer: impl Into<String>) -> Self {
        Self {
            account: AccountId(account.into()),
            kind,
            peer: Peer(peer.into()),
        }
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 种类前缀显式进入键的表示,防同数字 peer 在日志与存储层混淆。
        write!(f, "{}:{}:{}", self.kind, self.account, self.peer.0)
    }
}

/// 会话代次:QQ 退出、重登或会话失效后递增,用于使旧回调与旧请求失效(计划 §6.3)。
///
/// K4 迁移说明:本类型不再同时代表宿主代次与连接代次 —— 三者已分离为
/// [`HostIdentity`] / `SessionIdentity::session_generation` / [`ConnectionIdentity`]。
/// 旧 `RunId(g)` 数值原值迁移为 `session_generation`(见
/// [`SessionIdentity::from_run_id`])。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub u64);

/// 消息方向。
///
/// 计划 §6.3:本人发送不伪装成 incoming 用户消息;方向由原生来源字段判定,
/// 不能按正文是否与自己发送过的文本相同来推断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// 他人发来的消息。
    Incoming,
    /// 本账号(本人)发出的消息,含手动发送与程序发送回执。
    SelfSent,
}

/// 原生消息引用:不透明占位。
///
/// K1 阶段唯一确认有效的字段是会话键。原生标识组合待 K2/K3 调查;在此之前
/// 不提供任何可用于去重、关联或排序的字段,防止上游误用。
/// `#[non_exhaustive]`:未来补入原生标识字段时,外部构造与匹配不破坏编译。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeMessageRef {
    pub session: SessionKey,
}

impl NativeMessageRef {
    /// 占位构造。K2/K3 调查确认实际原生标识后,此构造将被具体字段替换。
    pub fn placeholder(session: SessionKey) -> Self {
        Self { session }
    }
}

/// 自有文本消息(完整 UTF-8)。媒体与其他段类型按计划属于后续条目。
#[derive(Debug, Clone)]
pub struct TextMessage {
    pub session: SessionKey,
    pub direction: Direction,
    pub body: String,
    pub native_ref: NativeMessageRef,
}

// ---------------------------------------------------------------------------
// K4-D3:三代次身份、事件与动作状态(计划 §6.2/§6.5/§6.6)。
// 纯数据:不含地址、JS handle 或线程原语。
// ---------------------------------------------------------------------------

/// 宿主代次身份:QQ 进程级。QQ 新进程或旧环境销毁即更换;同 PID 重用
/// 仍算新宿主(creation time / nonce 参与判定)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentity {
    pub pid: u32,
    /// 进程创建时间(RFC3339 UTC;空串 = 取不到,取不到时必须如实为空)。
    pub process_created_utc: String,
    /// manifest 基线标识(如 "qq-9.9.33-52230")。
    pub module_baseline: String,
    pub bridge_build: String,
    /// 宿主随机 nonce:PID 数字重用也无法与旧宿主混淆。
    pub host_nonce: u64,
}

/// 账号会话代次身份:登录/登出、原生会话失效或重新绑定时更换;
/// 不能跨账号沿用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub host: HostIdentity,
    pub account: AccountId,
    pub session_generation: u64,
}

impl SessionIdentity {
    /// 迁移映射:K1–K3 的 `RunId(g)` 语义 = `session_generation`(g 原值迁移)。
    pub fn from_run_id(host: HostIdentity, account: AccountId, run: RunId) -> Self {
        Self {
            host,
            account,
            session_generation: run.0,
        }
    }
}

/// core 连接代次身份:core 重启或管道重新认证时更换;**不会**自动让 QQ
/// 重新初始化(重连只换连接代次,不重注册监听器、不重发动作)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionIdentity {
    pub core_instance_nonce: u64,
    pub connection_epoch: u64,
    pub protocol_version: u32,
}

/// 原生消息 ID:不损失精度的 opaque 字符串(原生字段的精确组合待 D8 实测
/// 回填;在确认前禁止把它解析成数值或按正文/时间猜关联)。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeMessageId(pub String);

/// 事件来源:接收 / 更新(同消息的编辑、回应等后续通知)/ 历史回补。
/// 去重语义各不相同,必须区分(计划 §6.5)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventSource {
    Recv,
    Update,
    HistoryBackfill,
}

/// Proven OneBot V11 fields derived from the same native message record.
/// Missing fields stay `None`; callers must not infer them from opaque UIDs,
/// local receive sequence, text, or observation time.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OneBotMessageProjection {
    /// Resolved numeric peer ID (group number or mapped private UIN).
    pub peer_id: u64,
    /// Resolved sender QQ number.
    pub sender_user_id: u64,
    /// Platform's actual OneBot-compatible message ID, not Caligo's ID.
    pub real_id: i32,
    /// Platform event time in Unix seconds, as required by OneBot V11.
    pub time: i32,
    /// Best-effort sender nickname, omitted when unavailable.
    #[serde(default)]
    pub sender_nickname: Option<String>,
    /// OneBot V11 message subtype observed from the native event. Event fanout
    /// stays disabled when this is unknown (for example, anonymous group msg).
    #[serde(default)]
    pub sub_type: Option<String>,
    /// Native OneBot font field. Missing values are not replaced with a guess.
    #[serde(default)]
    pub font: Option<i32>,
}

/// 常驻接收事件(计划 §6.5 字段集)。`event_seq` 由 core 在持久化时分配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsgEvent {
    pub event_seq: u64,
    pub session_generation: u64,
    pub session: SessionKey,
    pub direction: Direction,
    /// 发送者标识(原生 sender 字段; opaque)。
    pub sender: String,
    pub native_id: NativeMessageId,
    /// 完整 UTF-8 正文,无裁剪。
    pub text: String,
    /// 平台时间(若有;来源单独记录,不猜)。
    pub platform_time: Option<u64>,
    /// Owned wrapper evidence retained through journal/recovery; absent on legacy CPB records.
    pub wrapper_message: Option<wrapper_message::WrapperMessageEnvelope>,
    /// OneBot projection only when each numeric field is independently proven.
    pub onebot_projection: Option<OneBotMessageProjection>,
    /// 本地观察时间(unix ms,单调性不作保证,仅展示)。
    pub observed_at_unix_ms: u64,
    pub source: EventSource,
}

impl MsgEvent {
    /// Include the retained raw record as well as the normalized text in queue budgets.
    pub fn queue_bytes(&self) -> usize {
        self.text.len().saturating_add(self.sender.len())
            .saturating_add(self.native_id.0.len()).saturating_add(128)
            .saturating_add(self.wrapper_message.as_ref().map_or(0, |raw| {
                serde_json::to_vec(raw).map_or(usize::MAX, |bytes| bytes.len())
            }))
    }
}

/// 发送动作(计划 §6.6)。`payload_hash` 覆盖全部语义参数;
/// 相同 request_id + 不同 payload → 拒绝;相同 request_id 重查 → 只查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendTextRequest {
    pub request_id: String,
    pub session_identity: SessionIdentity,
    pub target: SessionKey,
    pub text: String,
    pub payload_hash: u64,
    /// 用户等待策略(建议 30s);不是资源释放时刻。
    pub deadline_ms: u64,
}

impl SendTextRequest {
    /// FNV-1a(64):覆盖 request_id 之外的全部语义参数。
    pub fn compute_payload_hash(
        session_generation: u64,
        target: &SessionKey,
        text: &str,
        deadline_ms: u64,
    ) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bytes: &[u8]| {
            for b in bytes {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= 0xff; // 域分隔
        };
        mix(&session_generation.to_le_bytes());
        mix(target.to_string().as_bytes());
        mix(text.as_bytes());
        mix(&deadline_ms.to_le_bytes());
        h
    }
}

/// 动作状态机(计划 §6.6)。`DispatchIntent` 是 core 内部持久标记:
/// "已决定派发给 bridge、尚未收到 bridge 确认"的窗口显式化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionState {
    Accepted,
    Queued,
    DispatchIntent,
    NativeStarted,
    /// QQ 服务明确成功且已关联实际原生消息身份。
    ConfirmedSuccess {
        native_id: NativeMessageId,
    },
    /// 服务已受理,但当前响应没有可关联的稳定 native 身份。
    /// 这是可恢复事实:保留请求等待强身份或最终 unknown,也不自动重发。
    AcceptedUnconfirmed,
    ConfirmedFailure {
        reason: String,
    },
    /// 仅 bridge 确认从未开始调用才允许;core 单方面超时不得断言。
    CancelledUnsent,
    /// native 是否执行不确定(崩溃窗口/结果丢失/迟到未回)。
    DeliveryUnknown,
}

impl ActionState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ActionState::ConfirmedSuccess { .. }
                | ActionState::ConfirmedFailure { .. }
                | ActionState::CancelledUnsent
                | ActionState::DeliveryUnknown
        )
    }
}

/// 动作拒绝/未受理原因。与"发送失败"严格区分(计划 §6.6)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// 生命周期未到 Active。
    NotActive { phase: Phase },
    /// 会话身份过期/不匹配(宿主代次、账号或 session_generation)。
    SessionStale,
    /// 连接代次过期(旧连接上的请求)。
    ConnectionStale,
    /// 同 request_id 已存在但 payload 不同 → 冲突拒绝,不执行。
    RequestIdPayloadConflict,
    /// 队列/字节限额到达,明确未受理。
    CapacityExhausted { queue: &'static str },
    /// journal 不可写/损坏 → 拒绝发送(Degraded)。
    JournalFailure,
}

/// 账号生命周期(计划 §6.2)。owner(core actor)串行控制全部迁移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Detached,
    Bootstrapping,
    Bound,
    Active,
    Degraded,
    Stopping,
    Closed,
    Quarantined,
}

impl core::fmt::Display for Phase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Phase::Detached => "detached",
            Phase::Bootstrapping => "bootstrapping",
            Phase::Bound => "bound",
            Phase::Active => "active",
            Phase::Degraded => "degraded",
            Phase::Stopping => "stopping",
            Phase::Closed => "closed",
            Phase::Quarantined => "quarantined",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_numeric_peer_private_and_group_are_distinct_sessions() {
        // T03 同数字 private/group peer:两个不同会话。
        let private = SessionKey::new("10001", SessionKind::Private, "123456");
        let group = SessionKey::new("10001", SessionKind::Group, "123456");
        assert_ne!(private, group);
        assert_ne!(private.to_string(), group.to_string());
        assert!(private.to_string().starts_with("private:"));
        assert!(group.to_string().starts_with("group:"));
    }

    #[test]
    fn account_is_part_of_the_session_key() {
        let a = SessionKey::new("10001", SessionKind::Private, "123456");
        let b = SessionKey::new("10002", SessionKind::Private, "123456");
        assert_ne!(a, b);
    }

    #[test]
    fn native_message_ref_is_placeholder_only() {
        // K1:不提供 seq/random/UID 组合;构造只接受会话键。
        let key = SessionKey::new("10001", SessionKind::Private, "123456");
        let r1 = NativeMessageRef::placeholder(key.clone());
        let r2 = NativeMessageRef::placeholder(key);
        assert_eq!(r1, r2);
    }

    // ---- K4-D3 ----

    #[test]
    fn host_nonce_distinguishes_reused_pid() {
        let h1 = HostIdentity {
            pid: 4242,
            process_created_utc: "2026-10-07T01:00:00Z".into(),
            module_baseline: "qq-9.9.33-52230".into(),
            bridge_build: "caligo-bridge 0.1.0".into(),
            host_nonce: 1,
        };
        let mut h2 = h1.clone();
        h2.host_nonce = 2; // PID 重用、创建时间相同(极端)也要可区分
        assert_ne!(h1, h2);
    }

    #[test]
    fn run_id_migrates_to_session_generation_verbatim() {
        use crate::*;
        let host = HostIdentity {
            pid: 1,
            process_created_utc: String::new(),
            module_baseline: "qq-9.9.33-52230".into(),
            bridge_build: "b".into(),
            host_nonce: 7,
        };
        let s = SessionIdentity::from_run_id(host, AccountId("10001".into()), RunId(42));
        assert_eq!(s.session_generation, 42);
    }

    #[test]
    fn payload_hash_binds_all_semantic_params() {
        use crate::*;
        let t1 = SessionKey::new("10001", SessionKind::Private, "123456");
        let t2 = SessionKey::new("10001", SessionKind::Group, "123456"); // 同数字不同种类
        let a = SendTextRequest::compute_payload_hash(1, &t1, "hi", 30_000);
        assert_ne!(
            a,
            SendTextRequest::compute_payload_hash(1, &t2, "hi", 30_000)
        );
        assert_ne!(
            a,
            SendTextRequest::compute_payload_hash(1, &t1, "hi ", 30_000)
        );
        assert_ne!(
            a,
            SendTextRequest::compute_payload_hash(2, &t1, "hi", 30_000)
        );
        assert_eq!(
            a,
            SendTextRequest::compute_payload_hash(1, &t1, "hi", 30_000)
        );
    }

    #[test]
    fn action_state_terminal_classification() {
        use crate::*;
        assert!(!ActionState::Queued.is_terminal());
        assert!(!ActionState::NativeStarted.is_terminal());
        assert!(ActionState::DeliveryUnknown.is_terminal());
        assert!(ActionState::CancelledUnsent.is_terminal());
        assert!(ActionState::ConfirmedSuccess {
            native_id: NativeMessageId("N1".into())
        }
        .is_terminal());
        assert!(!ActionState::AcceptedUnconfirmed.is_terminal());
    }
}

// ---------------------------------------------------------------------------
// K4-D7b:CLG1 帧编解码与 IPC v2 桥方向消息(纯数据,从 caligo-core 下沉;
// bridge 与 core 共用同一协议定义,避免跨 crate 漂移)。
// ---------------------------------------------------------------------------

pub mod capability;
pub mod framing;
pub mod ipc_v2;
pub mod ipc_v3;
pub mod onebot_traits;
pub mod qq_wire;
pub mod wrapper_message;
pub mod voice;
