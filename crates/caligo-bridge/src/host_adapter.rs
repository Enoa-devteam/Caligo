//! K4-D2/D4 宿主适配层:owner 线程、合法环境与固定语义操作的抽象缝。
//!
//! 按 D2 契约(§3/§4):**只实现被证实的路线**。在 D7 取得 QQ 现场 owner
//! 入口证据之前,本模块只提供:
//! - [`HostAdapter`] trait —— LAB 自建宿主与未来生产适配器共同的语义面;
//! - [`NoHostAdapter`] —— 显式拒绝的占位实现(零 native 调用),
//!   证明"未证实的路线不接线"。
//!
//! 语义约束(计划 §5.1/§6.1/§6.3):
//! - 一切宿主 API 之前必须完成 owner 线程、env 有效、当前上下文检查;
//!   **current 为空/零不得放行**;
//! - native 操作只允许固定语义枚举,不开放任意脚本/通用 eval;
//! - 引用/token 由 adapter 侧持有,resident 不保存裸指针。

/// 固定语义操作(无任意载荷;参数为 owned 数据)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostOp {
    /// 注册消息监听(每会话代次一次;返回宿主 token)。
    ListenerAdd,
    /// 对称移除本桥注册的监听器(带确切 token;必须确认结果)。
    ListenerRemove { token: u64 },
    /// 非发送调度(生命周期/资源验收用;不代表业务发送)。
    Probe,
    /// 参数化发送(QQ 业务调用;LAB 由假会话服务应答)。
    SendText {
        /// 贯穿 bridge/core/native response ledger 的原始业务身份。
        request_id: String,
        /// core 持久分配的 client_seq；adapter 不得另行分配或替换。
        wire_seq: u32,
        /// 请求尚可等待回执的剩余时长。
        timeout_ms: u64,
        chat_type: u32,
        peer_uid: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostOpResult {
    ListenerAdded {
        token: u64,
    },
    /// 该宿主代次暂无消息监听(D7 面向 Health/身份/停止;监听器属 D8)。
    /// resident 记录为延迟模式:关闭时不执行对称移除。
    ListenerDeferred,
    ListenerRemoved,
    ProbeDone,
    /// 发送完成。`native_id` 为实际原生消息身份 —— **拿不到关联时为 None**,
    /// 上层(§6.6 receipt 契约)不得把 None 冒充成已关联成功。
    Sent {
        native_id: Option<String>,
    },
    /// 原生调用已发出(fired),结果由 Promise 异步完成 —— 非终态;
    /// `mid` 为本地生成的候选消息 ID(receipt 三分类的第一项)。
    SendFired {
        mid: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// 环境失效(GC/销毁/陈旧)—— 不可重试。
    EnvInvalid,
    /// 当前上下文不可用(零 current 不是合法状态)。
    NoCurrentContext,
    /// 非 owner 线程调用。
    NotOwnerThread,
    /// 该路线未取得现场证据(NoHostAdapter 恒定返回)。
    NoProvenRoute,
    /// 宿主侧失败(假会话服务可注入)。
    Native { code: u32 },
}

/// 宿主适配器:owner 线程视角的最小语义面。
///
/// 实现者约定:`current_thread_id` 返回**当前执行线程**在该宿主里的标识;
/// resident 用它与 [`HostAdapter::owner_thread_id`] 比对,先于任何
/// `native_op`。
use crate::resident::SendOutcome;
use caligo_model::capability::{HostQuery, QueryCapability, QueryOutcome, QueryScope};

pub trait HostAdapter {
    /// 宿主初始化时确定的 owner 线程(QQ 的 JS/loop 线程)。
    fn owner_thread_id(&self) -> u64;
    /// 当前执行线程标识(同一标识空间)。
    fn current_thread_id(&self) -> u64;
    /// 合法环境是否仍有效(每次进入宿主 API 前检查;不能缓存)。
    fn env_valid(&self) -> bool;
    /// 当前上下文检查。**返回 false 时必须拒绝;不存在"零 current 放行"。**
    fn current_context_ok(&self) -> bool;
    /// 执行固定语义操作。仅在 owner 线程 + env 有效 + 上下文可用时被调用;
    /// 实现内部仍应自检(防御纵深)。
    fn native_op(&mut self, op: HostOp) -> Result<HostOpResult, HostError>;
    /// 排空异步发送响应(消费通道 → 账本终局)。默认无(纯实现/无消费者)。
    /// 返回 (request_id, SendOutcome);Resident 负责落 core 结果通道。
    fn drain_send_responses(&mut self) -> Vec<(String, SendOutcome)> {
        Vec::new()
    }
    /// Fixed query metadata advertised by this compiled adapter. This does not
    /// imply that a live wrapper service is currently available.
    fn query_capabilities(&self) -> &'static [QueryCapability] {
        &[]
    }
    /// Explicit fixed-operation capability provider. Native routes default closed.
    fn send_capabilities(&self) -> (bool, bool) {
        (false, false)
    }
    /// Cancel a pending wrapper query on the owner thread.
    fn cancel_query(&mut self, _request_id: &str, _scope: &QueryScope) {}
    /// Execute one fixed read-only query on the owner thread.
    fn query(
        &mut self,
        _request_id: &str,
        _scope: &QueryScope,
        _query: &HostQuery,
        _timeout_ms: u64,
        _poll: bool,
    ) -> Option<QueryOutcome> {
        Some(QueryOutcome::Unsupported)
    }
}

/// D2 结论的代码化:在 B0-FIELD 通过之前,生产路线不存在。
/// 所有操作恒定返回 [`HostError::NoProvenRoute`],零副作用。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoHostAdapter {
    owner: u64,
}

impl NoHostAdapter {
    pub fn new(owner: u64) -> Self {
        Self { owner }
    }
}

impl HostAdapter for NoHostAdapter {
    fn owner_thread_id(&self) -> u64 {
        self.owner
    }
    fn current_thread_id(&self) -> u64 {
        self.owner
    }
    fn env_valid(&self) -> bool {
        true
    }
    fn current_context_ok(&self) -> bool {
        true
    }
    fn native_op(&mut self, _op: HostOp) -> Result<HostOpResult, HostError> {
        Err(HostError::NoProvenRoute)
    }
}
