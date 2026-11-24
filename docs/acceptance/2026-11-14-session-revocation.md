# 登录会话撤权与重连验收合同

日期：2026-11-14；源码基点 `5665b2b` 加当前工作树，摘要见
[执行记录](../execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-登录会话撤权与重连合同审计)。
本页是离线核对与待验矩阵，不表示已执行 QQ 换号、断网或 core 重启现场。
既有单次 self 查询现场不能替代本矩阵；产品状态仍以 [progress](../../progress.md) 为准。

## 身份与代次

| 标识 | 当前来源与寿命 | 验收时必须区分 |
| --- | --- | --- |
| account | worker 启动配置；LoginInfo/SelfProfile 另读 QQ 当前身份并比对 | 配置值不是持续登录证明 |
| host_nonce、PID、创建时间 | 宿主实例绑定 | 新进程不能作为旧宿主同代次重连 |
| session_generation | worker 启动输入，生命周期内固定 | 当前没有从 QQ 换号通知自动推进代次的已证链 |
| connection_epoch | 同一 daemon 内每次 bridge 连接分配 | daemon 重启后数值可能重复，不能单独跨 core 判代次 |
| core_instance_nonce | daemon 启动时随机；runtime ConnectionIdentity 使用 | QueryScope 不含此字段；request ID 隔离结果，resident 撤权标记防止旧查询恢复 |
| wrapper session ID | LoginInfo/SelfProfile 单次请求内读取/比较 | 与上述配置会话代次、管道 epoch 不是同一概念 |

类型和来源：[QueryScope](../../crates/caligo-model/src/capability.rs)、
[worker](../../crates/caligo-bridge/src/daemon_client.rs)、
[daemon](../../crates/caligo-core/src/daemon.rs)、
[runtime](../../crates/caligo-core/src/runtime.rs)。

## 已有行为与证据边界

| 场景 | 当前实现/离线证据 | 仍缺什么 |
| --- | --- | --- |
| 同一 core 的管道断开 | broker 清能力与 pending；worker ClearScope 撤 scope 并唤醒 owner | QQ 中实际断开与恢复样本 |
| owner 尚有在途查询 | drain 在 owner 取消旧 scope；暂时无合法上下文时保留 cancel_only 等候 | 停止/重连与真实异步回调相交的现场时序 |
| 同一 core 重连 | 新 epoch、重新声明能力，旧 ID/scope 结果只计 stale | 不能概括为 QQ 网络恢复或登录会话更新 |
| core 重启 | 新 core nonce 隔离查询 ID；resident 撤权时持久标记旧 pending，避免相同 scope 恢复 | 已有合成宿主交错修复，真实 core 重启仍待现场 |
| 查询期间 QQ 身份/会话变化 | 临时 Profile listener 与 session 检查使该查询失败 | 不是空闲期、目录、收发或媒体的全局身份撤权 |
| QQ 掉线但 IPC 仍通 | 不能由管道心跳推出 QQ 在线 | 该 build 的完整状态语义、持续通知和全路径清权接线 |
| 同进程换号 | 单次 self 查询会比对当前账号；配置 scope 本身不变 | 连续身份观察、事件归属失效、旧请求终结与新身份重绑 |
| QQ 进程重启 | 旧宿主同代次重连被拒绝 | 新宿主/新会话的显式绑定与现场恢复验收 |

主要源码：[broker reset/disconnect](../../crates/caligo-core/src/capability_broker.rs)、
[owner 查询队列](../../crates/caligo-bridge/src/resident.rs)、
[临时身份 listener](../../crates/caligo-bridge/src/qq_v8.rs)、
[刷新回调](../../crates/caligo-bridge/src/profile_refresh.js)。
worker 事件投影核对的是已有配置绑定；D8 启动扫描 live MsgService，
Profile 查询则从精确 wrapper 的 nt_1 取 ProfileService。
尚无持续身份观察把这两条来源在换号前后闭合到同一账号，不能由配置标签补出保证。

## 先补的离线证据

1. **撤权后相同 scope 再绑定**：旧查询已发起，worker 发布 None，owner 尚未 drain，
   新连接绑定数值相同的 QueryScope。检验旧请求取消是否必达，旧回调不得作为新请求结果。
   该交错已在合成宿主复现并修复：发布撤权/不同 scope 时将旧 pending 标为 cancel_only，
   相同 scope 重绑不清除此标记；Disconnected 只报告一次，native 取消留在合法 owner。
   新回归见 [query_revocation](../../crates/caligo-bridge/tests/query_revocation.rs)，验证结果见
   [修复记录](../execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-相同-scope-重绑撤权修复)。
   这是 resident 合同的 LAB 修复，真实 core 重启及 QQ 异步寿命仍待验。
2. **持续身份失效来源**：先核对固定 hash 的通知字段、状态枚举、线程和移除合同，
   再决定如何把离线/身份变化传播给 owner、worker、broker 和事件队列。
   不能把一个临时 listener 或已验证的 status=20 分支推成完整网络状态合同。
3. **恢复重绑**：旧请求保留其账号、代次与 unknown；旧事件与新身份不能混用，
   已发起发送不重发。只有重新验证宿主身份并显式重绑后才能恢复新动作。

现有回归：[daemon_client_lab](../../crates/caligo-core/tests/daemon_client_lab.rs) 的
`reconnect_requires_same_host_and_advances_epoch`、LoginInfo/SelfProfile owner 撤权；
[capability_query_lab](../../crates/caligo-core/tests/capability_query_lab.rs) 的清权、错 scope、
迟到与停止；[onebot_query_lab](../../crates/caligo-core/tests/onebot_query_lab.rs) 的独立能力与重连清权；
[脚本合同](../../crates/caligo-bridge/tests/fixtures/scoped-self-profile-contract.cjs) 的旧回调隔离。
这些是合成宿主/真实命名管道证据，不是 QQ 网络或换号样本。

## 待验矩阵与可执行顺序

| 编号 | 干预与对照 | 成功条件 | 当前准入 |
| --- | --- | --- | --- |
| R1a | 同一 core 中断 bridge 管道，QQ 身份不变 | 旧查询终结、能力清除、新 epoch、重新声明后新 self 查询成功 | 缺正常入口的受控断开步骤与相交在途证据；先离线准备 |
| R1b | 仅重启隔离 caligod，QQ 不退出 | 新 core nonce；相同 epoch 数值也不保留旧查询；无旧查询 poll/重发 | **LAB 已通过**：当前工作树 `core_restart_query_lab` research 2/2，真实 caligod/IPC/worker + 合成 owner；**双账号 LoginInfo FIELD 已通过**：新代次 A/B 各 attach 一次，self 查询、鉴权隔离和停止 A 后 B 存活通过；**core 重启交错 FIELD 待验**：尚未在 QQ owner 上做重启期间在途取消/迟到回调样本，已完成根禁止复用/重接 |
| R2 | 指定 QQ 的网络状态变化，IPC 保持可用 | 可定位状态通知触发全路径撤权，恢复后重新核实同一身份 | 静态通知面已定位；**在线探针已确认访问器**：session 原型提供 `setOnNetworkChanged`/`setOnMsfStatusChanged`/`setOnWeakNetChanged`/`onLine`/`offLine`，`getMsgService` 提供 `getOnLineDev`/`fetchStatusMgrInfo`；研究型观察者已实现（network_state 行 + Node VM 夹具），R2 观察现场包已备（未绑定）；首次在线尝试（PID 8036）因窗口内未执行网络断开而无事件，已确认是干预缺失、不是观察者结论；已补注册错误/arity 诊断并更新现场包；第二次完整断网（QQ 显示离线/重连）仍无回调，已加调用计数与绑定层字段，下一步转向 NodeIQQNTWrapperNetwork 或拉取式状态源；探测 v2 现场已读：绑定层暴露 `NodeIQQNTWrapperNetwork.openNetworkService`（0 参）与 `NodeIKernelLoginService.get`；`getSelfStatus` 为 Promise。观察者已接入 `addNetworkServiceListener`；另已实现会话层 `offLine()`/`onLine()` 合成切换（`qq-net-offline`/`qq-net-online` + runner `--synthetic-toggle`），用于不中断操作员网络的折中验证；但 `offLine/onLine` 需要 1 个参数，0 参调用返回 ERR，合成切换暂不可用。已改为先读取 `getSelfStatus` 的异步结果作为 R3 身份源；surface 同时确认 `network_service.getNetworkStatus()` 同步返回 number，观察者已加 `network_status` 轮询，现场读得 current=1；`getSelfStatus` 解析为 `{result,errMsg}` 对象，已补 result 数值采集（现场 result=0）；身份观察者已实现（session id + self_status 每 20 tick，写 identity_state），R3 换号现场仍待用户操作另一测试身份；真实断网样本仍待补（跟踪 issue #13）。撤权原语与契约测试已实现（network/identity 两种，撤权不重放、恢复需重新核实/新代次）；观察者→core 控制面已接：`daemon-control revoke-network|revoke-identity` 调用 `RevokeSession`；runner 自动触发与 R2 真实断网现场仍待补 |
| R3 | 指定测试 QQ 退出账号并登录另一测试身份 | 旧身份失效、旧事件不入新账本、旧请求不重放、显式新代次绑定 | 静态通知面已定位；**在线探针已确认访问器**：session 原型提供 `setOnNetworkChanged`/`setOnMsfStatusChanged` 与 `getSessionId`，`getProfileService` 提供 `startStatusPolling`/`getSelfStatus`/`getStatusInfo`/`getCoreInfo`；身份观察者已实现（session id + getSelfStatus，写 identity_state，首 tick 采样与随时排空修正完成，identity_state 基线行已现场确认（self_status: unset→0））；**桌面 QQ 不支持同实例换号**（操作员确认，2026-11-22），原“同实例退出→登录”现场不可执行；R3 改为跨进程新代次验证：新实例登录另一测试身份、显式新代次绑定，验证旧代次拒绝/旧事件审计/旧请求不重放；离线契约已有 `session_revocation_contract`（含新代次/新 journal 账本隔离：旧请求/旧事件不进入新账本、旧 journal 不变）。现场已做跨进程新代次：`21844` 以 generation 2 绑定，新 journal 仅 gen2、旧 gen1 journal 未变且 native ID 无交集；另一账号实例 `20304` 为多登录实例、preflight 拒绝，无法形成可 attach 的第二主根；“旧身份失效”仍只由旧实例停止 + 新代次绑定间接体现，R3 暂不宣称通过（跟踪 issue #14） |
| R4 | QQ 正常退出后新进程登录 | 旧宿主拒绝、新宿主独立准入与显式绑定 | 独立工作块，不能当 R1–R3 的替代样本 |

R1b 若后续获准执行，使用独立目录、稳定私有 `--token-file`、相同 pipe prefix、
journal 与指定 QQ；新 core nonce仍应改变。普通 control Stop 会结束 worker，
不能用作自动重连的断链干预；随意杀主 daemon 或全机断网也不是该对照步骤。
所有现场先核对资料卡、PID/创建时间、精确 loaded hashes、PE 审计和唯一 envscan；
一次接入前准备完操作和停止条件。当前已映射 bridge 的历史代次不重新注入或热卸载。

## 停止与完成条件

任何身份/hash/模块门不符、旧 scope 仍获授权、跨账号投影、重复 native 发起、
无法确认取消或移除时停止该样本，保留原始数据与 unknown，不自动重试。
测试输入、真实身份、凭据、聊天原文、QQ 文件仅留 ignored `local-evidence/`。
每个样本记录干预时刻、完整逻辑身份、查询 ID、清权与 owner 取消时刻、迟到结果、
恢复身份和能力声明、最终 daemon/owner/QQ 健康；保留失败样本。
矩阵各行独立给出 implemented/wired/LAB/field 状态；全部完成前不提升生产验收。

## 2026-11-21 静态补充：R2/R3 通知面已定位

固定构建（QQNT `75B19E58…`、major `4564E402…`、wrapper `0D8B9444…`）字符串复核确认了两条持续通知面：
登录侧的 `NodeIKernelLoginService.addKernelLoginListener`（1 参）与 `nodeIKernelLoginListener`
（`onLoginState`、`onLoginConnected/onLoginDisConnected`、`onQRCodeLoginSucceed` 等），网络侧的
`NodeIQQNTWrapperNetwork.getNetworkStatus/startGetNetworkStatus/stopGetNetworkStatus` 与
`nodeIQQNTWrapperNetworkListener.onNetworkStatusChanged`（另有 kernel 侧 `addNetworkListener` 的
`networkConnect/onLine/offLine`）。仍未闭合：会话对象到登录/网络服务的访问器、回调参数与状态枚举、
线程与移除返回值、事件归属与撤权接线。

详情与下一步（零发送只读表面探针）见
[登录/网络持续通知面研究](../research/2026-11-21-login-network-notification-surface.md)。R2/R3 仍不执行。

2026-11-21 离线实现补充：只读探针已落地——bridge 研究导出 `caligo_qq_surface_probe`（owner 泵执行
`D8_SURFACE_PROBE_JS`，只写属性名/`typeof` 到 `surface_probe` 阶段行）+ CLI `qq-surface`（沿用状态命令
的准入与已映射 bridge 校验，零发送、不注册 listener）+ Node VM 夹具验证有界与无泄漏。现场包
`local-evidence/feat005-surface-probe-20261121-unbound/` 已备、未绑定；待新鲜 QQ 代次在线执行。

2026-11-21 在线探针成功（PID 6544，构建 d2b981e）：确认访问器为 **session 原型**的
`setOnNetworkChanged/setOnMsfStatusChanged/setOnWeakNetChanged/onLine/offLine` 与
`getMsgService().getOnLineDev/fetchStatusMgrInfo`、`getProfileService().startStatusPolling/getSelfStatus/getStatusInfo/getCoreInfo`；
会话对象本身没有 `getLoginService/getNetworkService`。回调参数/枚举/线程与撤权接线仍未证，R2/R3 仍不执行。
详见研究记录 §6。
