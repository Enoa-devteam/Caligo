//! K4-D4 常驻 resident:一次初始化、有界 owned 队列、生命周期 token、
//! owner 线程执行与确定性关闭(计划 §6.2/§6.3/§6.4/§7-D4)。
//!
//! 不变量(对照 K3 审计结论,docs/research/k4-incident-register.md §5):
//! - **每宿主代次 bootstrap 一次**;重复 bootstrap / 关闭后再 bootstrap 拒绝;
//! - **submit 任意线程可调,执行只在 owner 线程**:任何宿主 API 前
//!   (包括第一个)完成 owner/env/current 三重检查,零 current 不放行;
//! - **owned 数据**:队列只保存自有 Rust 结构,不保存宿主借用/栈指针;
//! - **生命周期 token**:回调/请求携带 token;token 过期 → 拒绝且零 native;
//! - **确定性关闭**:未派发项可取消;在途项显式 unknown;listener 对称移除
//!   并确认;关闭后计数回基线;double close 幂等;
//! - 环境失效(env invalid)→ Quarantined:停动作、保留证据,不当正常关闭。
//! - **锁序合同(P4)**:全局锁序 adapter → core;native 调用(drain 的
//!   SendText/Probe、close 的 ListenerRemove)**不持有 core 锁** —— 内联
//!   回调可安全重入 `deliver_callback`/`token`(计划 §7-P4:claim/快照 →
//!   已验证调用 → 独立完成通道)。bootstrap 例外:ListenerAdd 完成前无
//!   监听器存在,回调不可能到达,双锁持有序仍遵守 adapter→core。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::host_adapter::{HostAdapter, HostError, HostOp, HostOpResult, NoHostAdapter};
use caligo_model::capability::{HostQuery, QueryOutcome, QueryScope};

/// 资源限额(计划 §6.4 建议初值;LAB 测试报告固定实际值)。
#[derive(Debug, Clone, Copy)]
pub struct ResidentLimits {
    /// 入站动作队列上限。
    pub ingress_max: usize,
    /// 出站事件队列上限。
    pub events_max: usize,
    /// 已接受发送可占用的生命周期与迟到回执槽上限(每个请求预留
    /// started 与 terminal 两槽;unknown 的 terminal 槽在纠正窗口后归还)。
    pub results_max: usize,
    /// 单次宿主 drain 上限;有剩余则由调用方继续调度。
    pub drain_batch: usize,
}

impl Default for ResidentLimits {
    fn default() -> Self {
        Self {
            ingress_max: 32,
            events_max: 256,
            results_max: 256,
            drain_batch: 32,
        }
    }
}

/// owned 动作(跨线程提交;不持有宿主数据)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedRequest {
    /// 非发送调度(资源/生命周期验收)。
    Probe { id: u64 },
    /// 参数化发送。native 语义与状态由 core(计划 §6.6)持有;bridge 只
    /// 登记"已见请求"防重复派发。
    SendText {
        request_id: String,
        wire_seq: u32,
        timeout_ms: u64,
        chat_type: u32,
        peer_uid: String,
        text: String,
    },
    SendVoice {
        request_id: String,
        wire_seq: u32,
        deadline: std::time::Instant,
        scope: QueryScope,
        peer_uin: String,
        chat_type: u32,
        silk: Vec<u8>,
    },
}

impl OwnedRequest {
    fn send_id(&self) -> Option<&String> {
        match self {
            Self::SendText { request_id, .. } | Self::SendVoice { request_id, .. } => {
                Some(request_id)
            }
            Self::Probe { .. } => None,
        }
    }
}

/// 事件来源回调(D8 去重语义依赖:onRecvMsg=接收,onMsgInfoListUpdate=更新)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSourceKind {
    Recv,
    Update,
}

/// owned 事件(宿主 → core 方向;D8 完整字段,JS 侧已 owned 拷贝)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedEvent {
    pub source: EventSourceKind,
    /// 1=私聊 2=群聊(原生 chatType,opaque 透传)。
    pub chat_type: u32,
    pub peer_uid: String,
    pub peer_uin: String,
    pub sender_uin: String,
    /// 原生消息 ID(opaque 字符串)。
    pub native_id: String,
    /// 完整正文(JS 侧上限 8000 字节,不裁剪语义由 JS 环承担)。
    pub text: String,
    /// 平台时间(原生 msgTime;取不到 None)。
    pub msg_time: Option<u64>,
    pub wrapper_message: Option<caligo_model::wrapper_message::WrapperMessageEnvelope>,
}

impl OwnedEvent {
    pub fn from_wrapper(
        envelope: caligo_model::wrapper_message::WrapperMessageEnvelope,
    ) -> Result<Self, &'static str> {
        use caligo_model::wrapper_message::WrapperSource;
        envelope.validate()?;
        Ok(Self {
            source: match envelope.source {
                WrapperSource::Recv | WrapperSource::AddSend => EventSourceKind::Recv,
                WrapperSource::Update => EventSourceKind::Update,
            },
            chat_type: envelope.record.chat_type,
            peer_uid: envelope.record.peer_uid.clone(),
            peer_uin: envelope.record.peer_uin.clone(),
            sender_uin: envelope.record.sender_uin.clone(),
            native_id: envelope.record.msg_id.clone(),
            text: if envelope.voice().is_some() {
                String::new()
            } else {
                envelope.text()?
            },
            msg_time: envelope.platform_time_seconds(),
            wrapper_message: Some(envelope),
        })
    }
}

/// 生命周期 token:回调/迟交结果必须携带;关闭后失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleToken(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResidentState {
    /// 未初始化。
    Idle,
    /// 已 bootstrap,接受提交。
    Ready,
    /// 关闭中:拒绝新提交,处理收尾。
    Stopping,
    Closed,
    /// 环境失效等不可证明安全的场景:停动作、保留证据。
    Quarantined,
}

/// 提交结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitVerdict {
    Accepted,
    /// 未初始化。
    NotReady,
    /// 关闭/隔离中,拒绝新提交。
    Closed,
    /// 队列满:明确拒绝(不静默丢弃;调用方按契约处理)。
    QueueFull,
}

/// 派发结果(worker 经 take_results 取走并回传 core;§6.6 receipt 三分类)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// native 成功且具有稳定 native identity。
    Success {
        request_id: String,
        native_id: String,
    },
    /// 服务已受理但响应没有稳定 native identity。
    AcceptedUnconfirmed { request_id: String },
    /// 宿主侧明确失败(如 native 服务报错)。
    Failure { request_id: String, reason: String },
    /// 结果不确定(env 失效/未知错误):执行与否不可证明。
    Unknown {
        request_id: String,
        reason: Option<String>,
        late_receipt_possible: bool,
    },
}

/// 与 daemon 有序传递的发送生命周期事件。NativeStarted 必须排在同一
/// 请求的任何终态结果之前,包括 native 调用内联触发的 callback。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResidentNotice {
    NativeStarted { request_id: String },
    Outcome(SendOutcome),
}

/// 关闭报告(计数回基线的验收数据)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CloseReport {
    pub cancelled_unsent: usize,
    pub marked_unknown: usize,
    pub listener_removed: Option<bool>,
    pub late_callbacks_rejected: usize,
    pub already_closed: bool,
}

/// 行为/资源计数(资源平衡验收数据源)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResidentCounters {
    pub bootstraps: u64,
    pub listener_adds: u64,
    pub listener_removes: u64,
    pub native_ops_total: u64,
    pub native_ops_rejected_precheck: u64,
    pub submitted_total: u64,
    pub dispatched_total: u64,
    pub callbacks_posted: u64,
    pub callbacks_executed: u64,
    pub callbacks_rejected_stale: u64,
    pub stale_submits_rejected: u64,
    pub cancelled_unsent: u64,
    pub marked_unknown: u64,
    pub quarantine_events: u64,
    pub events_gap_dropped: u64,
    pub wrapper_records_rejected: u64,
    pub wrapper_records_overflow: u64,
    pub wrapper_batches_rejected: u64,
    /// adapter 返回未知 request_id / 不再处于 SendPending 的响应被隔离。
    pub orphan_results_ignored: u64,
}

struct ReqRecord {
    state: ReqState,
    /// V8 Promise 路线的本地调用键；native CPB 文本候选不写入此字段。
    pending_mid: Option<String>,
    late_receipt_deadline: Option<std::time::Instant>,
    /// bit 0 = NativeStarted 未确认, bit 1 = terminal result 未确认。
    unacked_notice_mask: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReqState {
    Queued,
    NativeStarted,
    /// 原生调用已发出、Promise 未回(mid 已生成;§6.6 非终态)。
    SendPending,
    /// 收到业务接受但没有稳定身份；继续等待强身份或 unknown。
    IdentityPending,
    Done,
    CancelledUnsent,
    DeliveryUnknown,
}

struct Core<A: HostAdapter> {
    _marker: core::marker::PhantomData<A>,
    state: ResidentState,
    token: LifecycleToken,
    limits: ResidentLimits,
    ingress: VecDeque<(OwnedRequest, LifecycleToken)>,
    requests: HashMap<String, ReqRecord>,
    events: VecDeque<(OwnedEvent, LifecycleToken)>,
    results_out: VecDeque<ResidentNotice>,
    pending_notice_slots: usize,
    listener_token: Option<u64>,
    bridge_scope: Option<QueryScope>,
    counters: ResidentCounters,
    late_rejections: usize,
}

/// 常驻 resident。`Arc<Resident>` 共享;owner 线程语义由内部 Mutex +
/// adapter 的线程检查共同保证(**Mutex 只串行化数据结构;owner 检查在
/// 每次 native 前独立进行**,不因持锁而豁免)。
struct PendingQuery {
    request_id: String,
    scope: QueryScope,
    timeout_ms: u64,
    query: HostQuery,
    poll: bool,
    deadline: std::time::Instant,
    cancel_only: bool,
}

pub struct Resident<A: HostAdapter> {
    /// 宿主 adapter(独立锁;锁序 adapter→core)。native 调用期间不持
    /// core 锁,内联回调可重入 core 面板(§7-P4)。
    adapter: Mutex<A>,
    core: Mutex<Core<A>>,
    queries: Mutex<VecDeque<PendingQuery>>,
    query_results: Mutex<VecDeque<(String, QueryScope, QueryOutcome)>>,
}

impl<A: HostAdapter> Resident<A> {
    pub fn new(adapter: A, limits: ResidentLimits) -> Self {
        Self {
            adapter: Mutex::new(adapter),
            queries: Mutex::new(VecDeque::new()),
            query_results: Mutex::new(VecDeque::new()),
            core: Mutex::new(Core {
                _marker: core::marker::PhantomData,
                state: ResidentState::Idle,
                token: LifecycleToken(0),
                limits,
                ingress: VecDeque::new(),
                requests: HashMap::new(),
                events: VecDeque::new(),
                results_out: VecDeque::new(),
                pending_notice_slots: 0,
                listener_token: None,
                bridge_scope: None,
                counters: ResidentCounters::default(),
                late_rejections: 0,
            }),
        }
    }

    /// 一次性初始化(计划 §6.1:指定宿主初始化一次)。
    ///
    /// 检查顺序:**先** owner/env/current,后任何宿主 API(ListenerAdd)。
    pub fn bootstrap(&self) -> Result<LifecycleToken, HostError> {
        // 锁序 adapter→core(全局合同);ListenerAdd 完成前无监听器,回调
        // 不可能到达 —— 双锁持有序在此安全(doc 注)。
        let mut adapter = self.adapter.lock().unwrap();
        let mut c = self.core.lock().unwrap();
        if c.state != ResidentState::Idle {
            return Err(HostError::Native { code: 1 }); // 已初始化:每代次一次
        }
        precheck(&mut *adapter, &mut c.counters)?;
        let token = LifecycleToken(next_token());
        match adapter.native_op(HostOp::ListenerAdd)? {
            HostOpResult::ListenerAdded { token: ltoken } => {
                c.listener_token = Some(ltoken);
            }
            // 延迟模式(D7):无消息监听可注册;关闭时不做对称移除。
            HostOpResult::ListenerDeferred => {
                c.listener_token = None;
            }
            _ => return Err(HostError::Native { code: 2 }),
        }
        c.counters.listener_adds += 1;
        c.counters.native_ops_total += 1;
        c.counters.bootstraps += 1;
        c.token = token;
        c.state = ResidentState::Ready;
        Ok(token)
    }

    /// Read adapter capability metadata. This only reports compiled support.
    pub fn capability_metadata(&self) -> (Vec<caligo_model::capability::QueryCapability>, (bool, bool)) {
        let adapter = self.adapter.lock().unwrap();
        (adapter.query_capabilities().to_vec(), adapter.send_capabilities())
    }

    pub fn supports_private_voice(&self) -> bool {
        self.adapter.lock().unwrap().supports_private_voice()
    }

    pub fn supports_group_voice(&self) -> bool {
        self.adapter.lock().unwrap().supports_group_voice()
    }

    pub fn set_bridge_scope(&self, scope: Option<QueryScope>) {
        let _adapter = self.adapter.lock().unwrap();
        self.core.lock().unwrap().bridge_scope = scope;
    }

    /// Queue one authenticated fixed query; execution stays on the owner drain.
    pub fn submit_query(
        &self,
        request_id: String,
        scope: QueryScope,
        timeout_ms: u64,
        query: HostQuery,
    ) -> Result<(), QueryOutcome> {
        if scope.validate().is_err()
            || query.validate().is_err()
            || request_id.is_empty()
            || request_id.len() > 128
        {
            return Err(QueryOutcome::Rejected {
                code: 400,
                reason: "invalid query".into(),
            });
        }
        let mut queue = self.queries.lock().unwrap();
        if queue.len() >= 32 || queue.iter().any(|pending| pending.request_id == request_id) {
            return Err(QueryOutcome::Rejected {
                code: 409,
                reason: "query capacity or duplicate".into(),
            });
        }
        let timeout_ms = timeout_ms.min(caligo_model::capability::MAX_QUERY_TIMEOUT_MS);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        queue.push_back(PendingQuery {
            request_id,
            scope,
            timeout_ms,
            query,
            poll: false,
            deadline,
            cancel_only: false,
        });
        Ok(())
    }

    pub fn take_query_results(&self) -> Vec<(String, QueryScope, QueryOutcome)> {
        self.query_results.lock().unwrap().drain(..).collect()
    }

    pub fn invalidate_queries_except(&self, scope: &QueryScope) {
        let mut pending = self.queries.lock().unwrap();
        let mut retained = VecDeque::new();
        while let Some(mut item) = pending.pop_front() {
            if &item.scope == scope {
                retained.push_back(item);
            } else {
                self.query_results.lock().unwrap().push_back((
                    item.request_id.clone(),
                    item.scope.clone(),
                    QueryOutcome::Disconnected,
                ));
                item.cancel_only = true;
                retained.push_back(item);
            }
        }
        *pending = retained;
    }

    /// 任意线程提交 owned 动作(TSFN 语义:跨线程只入队,不执行)。
    pub fn submit(&self, req: OwnedRequest) -> SubmitVerdict {
        let mut c = self.core.lock().unwrap();
        c.counters.submitted_total += 1;
        match c.state {
            ResidentState::Ready => {}
            ResidentState::Idle => return SubmitVerdict::NotReady,
            ResidentState::Stopping | ResidentState::Closed | ResidentState::Quarantined => {
                c.counters.stale_submits_rejected += 1;
                return SubmitVerdict::Closed;
            }
        }
        // 先做所有容量检查,避免 QueueFull 留下不可重试的 request 记录。
        if let Some(request_id) = req.send_id() {
            if c.requests.contains_key(request_id) {
                return SubmitVerdict::Accepted; // 查询语义:不重复入队
            }
        }
        if c.ingress.len() >= c.limits.ingress_max {
            return SubmitVerdict::QueueFull;
        }
        let is_send = req.send_id().is_some();
        if is_send && c.pending_notice_slots.saturating_add(2) > c.limits.results_max {
            return SubmitVerdict::QueueFull;
        }
        // 已见请求防重复派发(计划 §6.6:相同 ID 不再次执行)。两个通知槽
        // 从 submit 起一直保留到 core 分别 ACK,使 worker 的断线 outbox 有界。
        if let Some(request_id) = req.send_id() {
            c.requests.insert(
                request_id.clone(),
                ReqRecord {
                    state: ReqState::Queued,
                    pending_mid: None,
                    late_receipt_deadline: None,
                    unacked_notice_mask: 0b11,
                },
            );
            c.pending_notice_slots += 2;
        }
        let tok = c.token;
        c.ingress.push_back((req, tok));
        SubmitVerdict::Accepted
    }

    /// 宿主投递回调(携带捕获时的生命周期 token)。
    pub fn deliver_callback(&self, token: LifecycleToken, ev: OwnedEvent) -> Result<(), HostError> {
        let mut c = self.core.lock().unwrap();
        c.counters.callbacks_posted += 1;
        if c.token != token || !matches!(c.state, ResidentState::Ready | ResidentState::Stopping) {
            c.counters.callbacks_rejected_stale += 1;
            c.late_rejections += 1;
            return Err(HostError::EnvInvalid); // 旧代次回调:拒绝,零 native
        }
        if c.events.len() >= c.limits.events_max {
            // 有界队列:满 → 拒绝该回调(宿主侧重放/Gap 由契约处理)。
            return Err(HostError::Native { code: 3 });
        }
        c.events.push_back((ev, token));
        Ok(())
    }

    /// owner 线程 drain:每项处理前独立完成 owner/env/current 检查。
    /// 返回本轮处理数(≤ drain_batch);有剩余时调用方应继续调度,
    /// **不能等下一条业务消息才唤醒**(计划 §6.4)。
    ///
    /// 锁范围(§7-P4):claim(core 锁)→ **invoke(adapter 锁,core 无锁)**
    /// → complete(core 锁)。内联回调在 invoke 期间重入
    /// `deliver_callback`/`token` 不死锁。
    pub fn drain(&self) -> Result<usize, HostError> {
        let mut processed = 0usize;
        loop {
            let batch_limit = self.core.lock().unwrap().limits.drain_batch;
            if processed >= batch_limit {
                break;
            }
            // ---- claim(core 锁):弹出 + 快照;尚未开始 native 调用 ----
            let (req, op) = {
                let mut c = self.core.lock().unwrap();
                if !matches!(c.state, ResidentState::Ready | ResidentState::Stopping) {
                    break;
                }
                let Some((req, tok)) = c.ingress.pop_front() else {
                    break;
                };
                if tok != c.token {
                    // 旧代次请求:拒绝且零 native。
                    c.counters.stale_submits_rejected += 1;
                    continue;
                }
                let op = match &req {
                    OwnedRequest::Probe { .. } => HostOp::Probe,
                    OwnedRequest::SendText {
                        request_id,
                        wire_seq,
                        timeout_ms,
                        chat_type,
                        peer_uid,
                        text,
                    } => HostOp::SendText {
                        request_id: request_id.clone(),
                        wire_seq: *wire_seq,
                        timeout_ms: *timeout_ms,
                        chat_type: *chat_type,
                        peer_uid: peer_uid.clone(),
                        text: text.clone(),
                    },
                    OwnedRequest::SendVoice { request_id, wire_seq, deadline, peer_uin, chat_type, silk, .. } => {
                        HostOp::SendVoice {
                            request_id: request_id.clone(),
                            wire_seq: *wire_seq,
                            timeout_ms: deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as u64,
                            peer_uin: peer_uin.clone(),
                            chat_type: *chat_type,
                            silk: silk.clone(),
                        }
                    }
                };
                (req, op)
            }; // core 锁释放:native 调用期间回调可重入。
            // ---- invoke(adapter 锁;precheck 仍先于第一个宿主 API)----
            let r = {
                let mut adapter = self.adapter.lock().unwrap();
                // 计数器更新需 core 锁:锁序 adapter→core(全局合同)。
                let pre = {
                    let mut c = self.core.lock().unwrap();
                    precheck(&mut *adapter, &mut c.counters)
                };
                if let Err(e) = pre {
                    // 首项失败 → Err;非首项 → 已处理数返回,
                    // 请求退回队首(未执行,不会产生 NativeStarted)。
                    let mut c = self.core.lock().unwrap();
                    let tok = c.token;
                    c.ingress.push_front((req, tok));
                    drop(c);
                    if processed == 0 {
                        return Err(e);
                    }
                    break;
                }
                if let OwnedRequest::SendVoice { deadline, scope, chat_type, .. } = &req {
                    let core = self.core.lock().unwrap();
                    let current = core.state == ResidentState::Ready && core.bridge_scope.as_ref() == Some(scope);
                    drop(core);
                    let supported = match chat_type {
                        1 => adapter.supports_private_voice(),
                        2 => adapter.supports_group_voice(),
                        _ => false,
                    };
                    if !current || *deadline <= std::time::Instant::now() || !supported {
                        Err(HostError::Native { code: 3 })
                    } else {
                        adapter.native_op(op)
                    }
                } else {
                    adapter.native_op(op)
                }
            };
            // ---- complete(core 锁):落账 + 独立完成通道 ----
            {
                let mut c = self.core.lock().unwrap();
                c.counters.native_ops_total += 1;
                let started = matches!(
                    &r,
                    Ok(HostOpResult::Sent { .. } | HostOpResult::SendFired { .. })
                );
                if started {
                    if let Some(request_id) = req.send_id() {
                        if let Some(rec) = c.requests.get_mut(request_id) {
                            rec.state = ReqState::NativeStarted;
                        }
                        c.results_out.push_back(ResidentNotice::NativeStarted {
                            request_id: request_id.clone(),
                        });
                    }
                }
                if !started {
                    if let Some(request_id) = req.send_id() {
                        release_notice_slot(&mut c, request_id, 0b01);
                    }
                }
                match &r {
                    Ok(HostOpResult::Sent { native_id }) => {
                        c.counters.dispatched_total += 1;
                        if let Some(request_id) = req.send_id() {
                            if let Some(rec) = c.requests.get_mut(request_id) {
                                rec.state = ReqState::Done;
                            }
                            let outcome = match native_id.as_deref().filter(|id| !id.is_empty()) {
                                Some(native_id) => SendOutcome::Success {
                                    request_id: request_id.clone(),
                                    native_id: native_id.to_string(),
                                },
                                None => {
                                    if let Some(rec) = c.requests.get_mut(request_id) {
                                        rec.state = ReqState::IdentityPending;
                                    }
                                    SendOutcome::AcceptedUnconfirmed {
                                        request_id: request_id.clone(),
                                    }
                                }
                            };
                            c.results_out.push_back(ResidentNotice::Outcome(outcome));
                        }
                    }
                    // fired 非终态:mid 挂账,Promise 结果经 poll sendResults 回收。
                    Ok(HostOpResult::SendFired { mid: _ }) => {
                        c.counters.dispatched_total += 1;
                        if let Some(request_id) = req.send_id() {
                            if let Some(rec) = c.requests.get_mut(request_id) {
                                rec.state = ReqState::SendPending;
                                rec.pending_mid = Some(request_id.clone());
                            }
                        }
                    }
                    Ok(_) => {
                        c.counters.dispatched_total += 1;
                        if let Some(request_id) = req.send_id() {
                            if let Some(rec) = c.requests.get_mut(request_id) {
                                rec.state = ReqState::DeliveryUnknown;
                            }
                            c.results_out.push_back(ResidentNotice::Outcome(
                                SendOutcome::Unknown {
                                    request_id: request_id.clone(),
                                    reason: None,
                                    late_receipt_possible: false,
                                },
                            ));
                        }
                    }
                    Err(HostError::Native { code }) => {
                        // 宿主明确报错:执行了但失败(Failure),不是 unknown。
                        if let Some(request_id) = req.send_id() {
                            if let Some(rec) = c.requests.get_mut(request_id) {
                                rec.state = ReqState::Done;
                            }
                            c.results_out.push_back(ResidentNotice::Outcome(
                                SendOutcome::Failure {
                                    request_id: request_id.clone(),
                                    reason: format!("native code {code}"),
                                },
                            ));
                        }
                    }
                    Err(_) => {
                        // env 失效/线程/上下文问题:执行与否不确定。
                        if let Some(request_id) = req.send_id() {
                            if let Some(rec) = c.requests.get_mut(request_id) {
                                rec.state = ReqState::DeliveryUnknown;
                            }
                            c.results_out.push_back(ResidentNotice::Outcome(
                                SendOutcome::Unknown {
                                    request_id: request_id.clone(),
                                    reason: None,
                                    late_receipt_possible: false,
                                },
                            ));
                        }
                    }
                }
                processed += 1;
            }
        }
        Ok(processed)
    }

    /// Run at most one bounded batch of typed queries on the QQ owner thread.
    pub fn drain_queries(&self) -> usize {
        let mut processed = 0;
        let batch = self.core.lock().unwrap().limits.drain_batch;
        for _ in 0..batch {
            let mut pending = self.queries.lock().unwrap();
            let Some(mut item) = pending.pop_front() else {
                break;
            };
            drop(pending);
            if item.cancel_only {
                let mut adapter = self.adapter.lock().unwrap();
                let mut core = self.core.lock().unwrap();
                if precheck(&mut *adapter, &mut core.counters).is_ok() {
                    drop(core);
                    adapter.cancel_query(&item.request_id, &item.scope);
                }
                processed += 1;
                continue;
            }
            if std::time::Instant::now() >= item.deadline {
                self.query_results.lock().unwrap().push_back((
                    item.request_id.clone(),
                    item.scope.clone(),
                    QueryOutcome::Timeout,
                ));
                item.cancel_only = true;
                self.queries.lock().unwrap().push_back(item);
                processed += 1;
                continue;
            }
            let outcome = {
                let mut adapter = self.adapter.lock().unwrap();
                let mut core = self.core.lock().unwrap();
                match precheck(&mut *adapter, &mut core.counters) {
                    Ok(()) => {
                        drop(core);
                        let remaining =
                            item.deadline
                                .saturating_duration_since(std::time::Instant::now())
                                .as_millis()
                                .min(item.timeout_ms as u128) as u64;
                        adapter.query(
                            &item.request_id,
                            &item.scope,
                            &item.query,
                            remaining,
                            item.poll,
                        )
                    }
                    Err(
                        HostError::EnvInvalid
                        | HostError::NoCurrentContext
                        | HostError::NotOwnerThread,
                    ) => Some(QueryOutcome::NotReady),
                    Err(_) => Some(QueryOutcome::Unsupported),
                }
            };
            match outcome {
                Some(outcome) => self.query_results.lock().unwrap().push_back((
                    item.request_id,
                    item.scope,
                    outcome,
                )),
                None => self
                    .queries
                    .lock()
                    .unwrap()
                    .push_back(PendingQuery { poll: true, ..item }),
            }
            processed += 1;
        }
        processed
    }

    /// owner 线程消费出站事件。
    pub fn take_events(&self) -> Vec<OwnedEvent> {
        let mut c = self.core.lock().unwrap();
        c.events.drain(..).map(|(ev, _)| ev).collect()
    }

    /// 消费派发结果(worker 回传 core;先落账后 ACK 由 core 负责)。
    pub fn take_results(&self) -> Vec<ResidentNotice> {
        let mut c = self.core.lock().unwrap();
        c.results_out.drain(..).collect()
    }

    /// Promise 状态由原始 core request_id 关联；只有精确 wrapper 回读
    /// 可提供 native_id。CPB peer+text 候选不得调用此方法。
    pub fn complete_sends(
        &self,
        done: &[(String, String)],
        rejected: &[(String, String)],
        unknown: &[(String, String)],
    ) {
        let mut c = self.core.lock().unwrap();
        let mut outcomes = Vec::new();
        let mut updates = Vec::new();
        for (request_id, record) in &c.requests {
            if !matches!(
                record.state,
                ReqState::SendPending | ReqState::IdentityPending | ReqState::DeliveryUnknown
            ) {
                continue;
            }
            if record.state == ReqState::DeliveryUnknown
                && record
                    .late_receipt_deadline
                    .is_none_or(|deadline| std::time::Instant::now() >= deadline)
            {
                continue;
            }
            let Some(pending_request_id) = record.pending_mid.as_deref() else {
                continue;
            };
            if let Some((_, native_id)) = done
                .iter()
                .find(|(candidate, _)| candidate == pending_request_id)
            {
                let (state, outcome) = if native_id.is_empty() {
                    (
                        ReqState::IdentityPending,
                        SendOutcome::AcceptedUnconfirmed {
                            request_id: request_id.clone(),
                        },
                    )
                } else {
                    (
                        ReqState::Done,
                        SendOutcome::Success {
                            request_id: request_id.clone(),
                            native_id: native_id.clone(),
                        },
                    )
                };
                outcomes.push(ResidentNotice::Outcome(outcome));
                updates.push((request_id.clone(), state));
            } else if let Some((_, reason)) = rejected
                .iter()
                .find(|(candidate, _)| candidate == request_id)
            {
                outcomes.push(ResidentNotice::Outcome(SendOutcome::Failure {
                    request_id: request_id.clone(),
                    reason: reason.clone(),
                }));
                updates.push((request_id.clone(), ReqState::Done));
            } else if let Some((_, reason)) = unknown
                .iter()
                .find(|(candidate, _)| candidate == request_id)
            {
                outcomes.push(ResidentNotice::Outcome(SendOutcome::Unknown {
                    request_id: request_id.clone(),
                    reason: Some(reason.clone()),
                    late_receipt_possible: true,
                }));
                updates.push((request_id.clone(), ReqState::DeliveryUnknown));
            }
        }
        for (request_id, state) in updates {
            if let Some(record) = c.requests.get_mut(&request_id) {
                let newly_unknown =
                    state == ReqState::DeliveryUnknown && record.state != ReqState::DeliveryUnknown;
                record.state = state;
                if newly_unknown {
                    record.late_receipt_deadline = std::time::Instant::now().checked_add(
                        std::time::Duration::from_millis(caligo_model::LATE_RECEIPT_GRACE_MS),
                    );
                }
                if state == ReqState::Done {
                    record.pending_mid = None;
                    record.late_receipt_deadline = None;
                }
            }
        }
        c.results_out.extend(outcomes);
    }

    /// core durable ACK 后归还 NativeStarted 通知槽。重复 ACK 幂等。
    pub fn ack_notice(&self, request_id: &str, started: bool) -> bool {
        let mut c = self.core.lock().unwrap();
        release_notice_slot(&mut c, request_id, if started { 0b01 } else { 0b10 })
    }

    /// 中间态 ACK 移除当前 outbox 项，但保留终态结果槽。
    pub fn ack_result_notice(&self, request_id: &str, terminal: bool) -> bool {
        let mut c = self.core.lock().unwrap();
        if terminal {
            if c.requests
                .get(request_id)
                .map(|rec| {
                    rec.state == ReqState::DeliveryUnknown && rec.late_receipt_deadline.is_some()
                })
                .unwrap_or(false)
            {
                // ACK 接受了 Unknown；槽仍为迟到强身份预留。
                return true;
            }
            release_notice_slot(&mut c, request_id, 0b10)
        } else {
            c.requests
                .get(request_id)
                .map(|rec| rec.unacked_notice_mask & 0b10 != 0)
                .unwrap_or(false)
        }
    }

    /// Release result capacity after the bounded late-identity correction window.
    pub fn expire_late_receipts(&self, now: std::time::Instant) -> usize {
        let mut c = self.core.lock().unwrap();
        let expired: Vec<String> = c
            .requests
            .iter()
            .filter(|(_, rec)| {
                rec.state == ReqState::DeliveryUnknown
                    && rec
                        .late_receipt_deadline
                        .map(|deadline| now >= deadline)
                        .unwrap_or(false)
            })
            .map(|(request_id, _)| request_id.clone())
            .collect();
        for request_id in &expired {
            if let Some(rec) = c.requests.get_mut(request_id) {
                rec.late_receipt_deadline = None;
            }
            release_notice_slot(&mut c, request_id, 0b10);
        }
        expired.len()
    }

    /// 排空异步发送响应消费槽(adapter → 账本终局 → core 结果通道)。
    /// 泵线程每拍调用;request 状态 SendPending → Done。
    pub fn drain_send_responses(&self) -> usize {
        // 锁序合同(adapter → core):先取 adapter 结果,再进 core。
        let done = {
            let mut adapter = self.adapter.lock().unwrap();
            adapter.drain_send_responses()
        };
        if done.is_empty() {
            return 0;
        }
        let mut c = self.core.lock().unwrap();
        let mut accepted = 0usize;
        for (request_id, outcome) in done {
            let outcome_id = send_outcome_request_id(&outcome);
            let valid = outcome_id == request_id
                && c.requests
                    .get(&request_id)
                    .map(|rec| {
                        matches!(rec.state, ReqState::SendPending | ReqState::IdentityPending)
                            || (matches!(&outcome, SendOutcome::Success { .. })
                                && rec.state == ReqState::DeliveryUnknown
                                && rec
                                    .late_receipt_deadline
                                    .map(|deadline| std::time::Instant::now() < deadline)
                                    .unwrap_or(false))
                    })
                    .unwrap_or(false);
            if !valid {
                c.counters.orphan_results_ignored =
                    c.counters.orphan_results_ignored.saturating_add(1);
                continue;
            }
            if let Some(rec) = c.requests.get_mut(&request_id) {
                match &outcome {
                    SendOutcome::AcceptedUnconfirmed { .. } => {
                        rec.state = ReqState::IdentityPending;
                    }
                    SendOutcome::Unknown {
                        late_receipt_possible: true,
                        ..
                    } => {
                        rec.state = ReqState::DeliveryUnknown;
                        rec.late_receipt_deadline = std::time::Instant::now().checked_add(
                            std::time::Duration::from_millis(caligo_model::LATE_RECEIPT_GRACE_MS),
                        );
                    }
                    _ => {
                        rec.state = ReqState::Done;
                        rec.late_receipt_deadline = None;
                    }
                }
            }
            c.results_out.push_back(ResidentNotice::Outcome(outcome));
            accepted += 1;
        }
        accepted
    }

    /// Decode the exact D8 wire batch used by the normal owner hook.
    pub fn ingest_wrapper_batch(
        &self,
        json: &str,
        binding: &caligo_model::wrapper_message::WrapperBinding,
    ) -> Result<caligo_model::wrapper_message::WrapperDrain, &'static str> {
        let batch = match caligo_model::wrapper_message::decode_d8_batch(json, binding) {
            Ok(batch) => batch,
            Err(error) => {
                self.core.lock().unwrap().counters.wrapper_batches_rejected += 1;
                return Err(error);
            }
        };
        {
            let mut c = self.core.lock().unwrap();
            c.counters.wrapper_records_rejected = c
                .counters
                .wrapper_records_rejected
                .saturating_add(batch.stats.rejected);
            c.counters.wrapper_records_overflow = c
                .counters
                .wrapper_records_overflow
                .saturating_add(batch.stats.overflow);
        }
        for event in &batch.events {
            // Validation was completed by the shared decoder before queue admission.
            if let Ok(event) = OwnedEvent::from_wrapper(event.clone()) {
                self.ingest_event(event);
            }
        }
        Ok(batch)
    }

    /// 宿主(pump/D8 排空)注入 owned 事件;有界窗口满 → 计数丢弃(不静默)。
    pub fn ingest_event(&self, ev: OwnedEvent) -> bool {
        let mut c = self.core.lock().unwrap();
        if !matches!(c.state, ResidentState::Ready | ResidentState::Stopping) {
            return false;
        }
        if c.events.len() >= c.limits.events_max {
            c.counters.events_gap_dropped += 1;
            return false;
        }
        let tok = c.token;
        c.events.push_back((ev, tok));
        true
    }

    pub fn counters(&self) -> ResidentCounters {
        self.core.lock().unwrap().counters.clone()
    }

    pub fn pending_ingress(&self) -> usize {
        self.core.lock().unwrap().ingress.len()
    }

    /// 正常关闭(owner 线程):取消未执行、显式化在途、对称移除监听器。
    /// double close 幂等。
    pub fn close(&self) -> Result<CloseReport, HostError> {
        let mut c = self.core.lock().unwrap();
        let mut report = CloseReport {
            already_closed: matches!(c.state, ResidentState::Closed),
            ..Default::default()
        };
        if c.state == ResidentState::Closed {
            return Ok(report);
        }
        if c.state == ResidentState::Quarantined {
            // 隔离态不允许"洗白"为正常关闭。
            return Err(HostError::EnvInvalid);
        }
        c.state = ResidentState::Stopping;
        // 1) 取消从未执行的入站项(可证明未派发)。
        while let Some((req, _)) = c.ingress.pop_front() {
            if let Some(request_id) = req.send_id() {
                if let Some(rec) = c.requests.get_mut(request_id) {
                    if rec.state == ReqState::Queued {
                        rec.state = ReqState::CancelledUnsent;
                        report.cancelled_unsent += 1;
                        c.counters.cancelled_unsent += 1;
                    }
                }
            } else {
                report.cancelled_unsent += 1;
            }
        }
        // 2) 在途(native 已开始/fired 未回执)→ 显式 unknown;不宣称未发送。
        // **含 SendPending**(C7:fired 未回执在关闭时同样不可证明已送达)。
        let mut unknown_now: u64 = 0;
        for rec in c.requests.values_mut() {
            if matches!(
                rec.state,
                ReqState::NativeStarted | ReqState::SendPending | ReqState::IdentityPending
            ) {
                rec.state = ReqState::DeliveryUnknown;
                unknown_now += 1;
            }
        }
        report.marked_unknown = unknown_now as usize;
        c.counters.marked_unknown += unknown_now;
        // 3) 对称移除监听器:在途分类(**含 SendPending**,C7)已完成;
        // ListenerRemove 在 adapter 锁内执行(core 无锁,迟到回调可重入)。
        let listener_tok = c.listener_token.take();
        drop(c);
        if let Some(tok) = listener_tok {
            let r = {
                let mut adapter = self.adapter.lock().unwrap();
                let pre = {
                    let mut c = self.core.lock().unwrap();
                    precheck(&mut *adapter, &mut c.counters)
                };
                if let Err(e) = pre {
                    let mut c = self.core.lock().unwrap();
                    c.quarantine();
                    return Err(e);
                }
                adapter.native_op(HostOp::ListenerRemove { token: tok })
            };
            let mut c = self.core.lock().unwrap();
            match r {
                Ok(HostOpResult::ListenerRemoved) => {
                    c.counters.listener_removes += 1;
                    c.counters.native_ops_total += 1;
                    report.listener_removed = Some(true);
                }
                Ok(_) => {
                    report.listener_removed = Some(false);
                }
                Err(e) => {
                    // 移除失败:无法证明清理完成 → Quarantined(T15)。
                    c.quarantine();
                    return Err(e);
                }
            }
            drop(c);
        }
        // 4) 残余出站事件丢弃计数已在 late_rejections 体现;队列清空。
        let mut c = self.core.lock().unwrap();
        c.late_rejections += c.events.len();
        c.events.clear();
        c.state = ResidentState::Closed;
        Ok(report)
    }

    /// 隔离(计划 §6.2):停动作、停自动恢复、保留证据;不可当作正常关闭。
    pub fn quarantine_reason(&self) -> bool {
        self.core.lock().unwrap().state == ResidentState::Quarantined
    }

    /// 当前生命周期 token(宿主捕获回调用)。
    pub fn token(&self) -> LifecycleToken {
        self.core.lock().unwrap().token
    }

    /// 已见请求状态(查询语义;与 core 的 ActionState 正交)。
    pub fn request_state(&self, request_id: &str) -> Option<&'static str> {
        let c = self.core.lock().unwrap();
        c.requests.get(request_id).map(|r| match r.state {
            ReqState::Queued => "queued",
            ReqState::NativeStarted => "native_started",
            ReqState::SendPending => "send_pending",
            ReqState::IdentityPending => "identity_pending",
            ReqState::Done => "done",
            ReqState::CancelledUnsent => "cancelled_unsent",
            ReqState::DeliveryUnknown => "delivery_unknown",
        })
    }
}

fn release_notice_slot<A: HostAdapter>(c: &mut Core<A>, request_id: &str, bit: u8) -> bool {
    let released = if let Some(rec) = c.requests.get_mut(request_id) {
        if rec.unacked_notice_mask & bit != 0 {
            rec.unacked_notice_mask &= !bit;
            true
        } else {
            false
        }
    } else {
        false
    };
    if released {
        c.pending_notice_slots = c.pending_notice_slots.saturating_sub(1);
    }
    released
}

fn send_outcome_request_id(outcome: &SendOutcome) -> &str {
    match outcome {
        SendOutcome::Success { request_id, .. }
        | SendOutcome::AcceptedUnconfirmed { request_id }
        | SendOutcome::Failure { request_id, .. }
        | SendOutcome::Unknown { request_id, .. } => request_id,
    }
}

/// 一切宿主 API 之前的三重检查:owner 线程 → env 有效 → current 上下文。
/// **检查顺序固定;零/空 current 不得放行**(计划 §5.1)。调用方须已持
/// adapter 锁;core 锁仅用于计数(全局锁序 adapter→core)。
fn precheck<A: HostAdapter>(
    adapter: &mut A,
    counters: &mut ResidentCounters,
) -> Result<(), HostError> {
    if adapter.current_thread_id() != adapter.owner_thread_id() {
        counters.native_ops_rejected_precheck += 1;
        return Err(HostError::NotOwnerThread);
    }
    if !adapter.env_valid() {
        counters.native_ops_rejected_precheck += 1;
        return Err(HostError::EnvInvalid);
    }
    if !adapter.current_context_ok() {
        counters.native_ops_rejected_precheck += 1;
        return Err(HostError::NoCurrentContext);
    }
    Ok(())
}

impl<A: HostAdapter> Core<A> {
    fn quarantine(&mut self) {
        self.state = ResidentState::Quarantined;
        self.counters.quarantine_events += 1;
    }
}

/// 全局 token 空间(进程内唯一,跨 Resident 实例不重复)。
fn next_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
