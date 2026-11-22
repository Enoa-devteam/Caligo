# 登录/网络持续通知面（固定 9.9.36-53644，2026-11-21 静态补充）

本文是 [登录会话撤权与重连验收合同](../acceptance/2026-11-14-session-revocation.md) R2/R3 阻塞项的第一步：
在固定构建上核对可用的**持续身份/网络通知源**。只读磁盘模块字符串，不加载、不调用、不接入 QQ。
模块 hash 与 pilot manifest 一致：QQNT.dll `75B19E58…`、major.node `4564E402…`、wrapper.node `0D8B9444…`。

## 1. 已定位的通知面

### 1.1 登录/身份（R3）

- JS 侧存在 `NodeIKernelLoginService`，方法 `addKernelLoginListener` / `removeKernelLoginListener`
  均为 **1 参数**（wrapper.node 的断言字符串明确 `needs 1 arguments`）。
- `nodeIKernelLoginListener` 的回调集合（major.node 字符串）：
  `onQRCodeGetPicture`、`onQRCodeSessionFailed`、`onQRCodeSessionQuickLoginFailed`、
  `onQRCodeSessionUserScaned`、`onQRCodeLoginSucceed`、`onLoginState`、`onLoginFailed`、
  `onPasswordLoginFailed`、`onLoginConnected`、`onLoginDisConnected`。
- `onLoginState`、`onLoginConnected/onLoginDisConnected`、`onQRCodeLoginSucceed` 是身份/登录态变化的
  连续来源候选；`getLoginList` 可作当前登录列表的只读核对。

### 1.2 网络/在线状态（R2）

- wrapper 侧存在 `NodeIQQNTWrapperNetwork`，方法 `getNetworkStatus`、`startGetNetworkStatus`、
  `stopGetNetworkStatus`（均 0 参数）；监听器类 `nodeIQQNTWrapperNetworkListener`，
  回调 `onNetworkStatusChanged`（字符串邻接 `oldStatus`，完整参数形状未证）。
- JS kernel 侧另有 `addNetworkListener` 事件名 `networkConnect`、`onLine`、`offLine`、
  `addMsfStatusChangeListener`，以及 `nodeIKernelSessionListener/onNTSessionCreate`。
- `getNetworkStatus` 是同步返回还是 Promise、状态枚举值、回调参数与线程均**未证**。

### 1.3 会话访问器（待现场探针）

wrapper.node 明确存在 `NodeIQQNTWrapperSession::getProfileService`、`getMsgService` 等 0 参数 getter，
但**未找到** `getLoginService` / `getNetworkService` 的方法名字符串；登录服务可能挂在
`NodeIKernelLoginService` 的另一层或由 kernel 对象提供。因此“从会话对象怎样拿到登录/网络服务”
仍需在固定构建上做一次**只读表面探针**（见 §3），不能靠静态猜测接线。

## 2. 对 R2/R3 的含义

| 需求 | 候选来源 | 仍缺 |
|---|---|---|
| R2 网络状态变化全路径撤权 | `NodeIQQNTWrapperNetwork.onNetworkStatusChanged` 或 `addNetworkListener` 的 `onLine/offLine` | 枚举值、参数、线程、移除返回值、与 owner/broker 的接线 |
| R3 换号身份失效 | `nodeIKernelLoginListener.onLoginState` / `onLoginConnected/DisConnected` + `getLoginList`/self 查询 | 状态码语义、旧事件归属、旧请求终结、新代次显式绑定 |
| 共同 | 事件必须进入 owner 泵并可审计 | 监听器注册/移除 API 的宿主对象与生命周期；未知状态不得静默恢复 |

保守语义（拟）：只对**可识别**的 disconnected/离线/身份不符状态触发撤权；未知码只记审计并保持
unknown，不自动重连、不重发；恢复后必须重新核实身份并显式重绑，旧请求保持其账号/代次/unknown。

## 3. 下一步：只读表面探针（零发送）

在新鲜实例上 attach 后，由 owner 执行固定脚本（不发送、不改业务状态），枚举：

1. `getNTWrapperSession('nt_1')` 的自有属性/原型属性名（有界，最多 64 项）；
2. `getLoginService`、`getNetworkService`、`getNetwork`、`getProfileService`、`getMsgService`、
   `getNTWrapperNetwork` 的 `typeof`、返回对象的属性名/原型名（有界）；
3. 登录服务与网络对象的 listener 注册方法名与 `length`（不注册）；
4. 若存在 `getNetworkStatus`，只记录 `typeof` 与返回值类型（不强制等待 Promise）。

探针输出写入入口 report 的 `surface_probe` 阶段行，便于离线审计；随后一次 1000 ms 观察停止，零发送。
探针不注册任何 listener，避免在 R2/R3 合同未定前引入新的回调面。

## 4. 关闭条件

探针结果 + 本静态面一起确定：宿主对象获取路径、回调参数/枚举、线程与移除合同。之后才可设计
“通知 → 撤权 → 恢复重绑”的接线与 LAB；R2/R3 现场仍需用户执行断网/换号干预，不能由本页替代。

## 5. 只读探针已实现（离线，2026-11-21）

- bridge 研究导出 `caligo_qq_surface_probe`（导出序号 27，RVA `0x60470`）：置一次性标志并 `wake_owner_pump`，
  由 owner 泵在 `OWNER_INIT` 完成后执行 `D8_SURFACE_PROBE_JS`，把有界 JSON 写入入口 report 的
  `surface_probe` 阶段行；默认构建返回 2，不执行。
- CLI `qq-surface`：沿用 `qq-status` 的准入/已映射 bridge 校验，远程调用该导出并打印 report 中的
  `surface_probe` 行；只读、零发送、不注册 listener。
- 脚本只输出会话/服务的属性名与 `typeof`（有界：会话 64 项、服务 48 项、类 48 项），错误只保留
  `Error` 名称；不输出消息、身份、函数源码或错误文本。Node VM 夹具
  `crates/caligo-bridge/tests/surface_probe.rs` 验证有界性与无泄漏。
- 构建（源码 `d2b981e`）：EXE `83342968…`、bridge `73476CA8…`、caligod `D02B07AA…`；
  现场包见 ignored `local-evidence/feat005-surface-probe-20261121-unbound/`（未绑定、零发送）。
- 在线执行顺序：preflight → `python -B run-field.py`（attach + self 查询 + `qq-surface` + 观察停止）。
  探针结果只用于确定接线；R2/R3 仍未准入、未执行。
- 首次在线尝试（PID `54416`，旧构建 `a422aa9`）返回 `surface_probe unavailable`，当时未细分原因；据此改为
  **仅在同一个 poll 成功的 tick 上执行**，并由 CLI 有界重试 30 次，失败写入具体码
  （`no_symbols`/`no_context`/`run_empty`/`compile_failed`/`bad_json` 等）。该实例已停止、bridge 保留，未重跑；
  新构建 `d2b981e`（EXE `83342968…`、bridge `73476CA8…`、caligod `D02B07AA…`）待下一次现场。
- 第二次在线尝试（PID 56396，构建 0e58fbf）证明 tick gating 生效、探针实际执行，但返回 bad_json：3 KiB 预截断破坏了 JSON；已改为先校验（64 KiB 上限）再序列化。该实例已停止、bridge 保留，未重跑；构建 d2b981e 待下一次现场。

## 6. 第三次在线探针结果（2026-11-21，成功）

实例 `6544`（创建 `2026-11-21T14:30:27.0613801Z`），构建 `d2b981e`（EXE `83342968…`、bridge `73476CA8…`、caligod `D02B07AA…`），
准入 PASS、一次 attach、self 查询通过；`surface_probe` 返回 64 项会话原型与两个服务原型：

- 会话对象**没有** `getLoginService` / `getNetworkService` / `getNetwork` / `getNTWrapperNetwork`（均为 `undefined`）。
- 通知面在 **session 原型**上：`setOnNetworkChanged`、`setOnMsfStatusChanged`、`setOnWeakNetChanged`、
  `onLine`、`offLine`、`offLineSync`、`onNetReply`、`onMsfPush`。
- `getMsgService()` 原型含 `getOnLineDev`、`kickOffLine`、`setStatus`、`fetchStatusMgrInfo`、
  `fetchStatusUnitedConfigInfo`、`sendMsg`、`addSendMsg`、`cancelSendMsg`、`resendMsg` 等。
- `getProfileService()` 原型含 `startStatusPolling`、`getSelfStatus`、`getStatus`、`getStatusInfo`、
  `getCoreInfo`、`getCoreAndBaseInfo`、`getUinByUid`/`getUidByUin` 等。

含意：R2 的连续来源是 session 的 `setOnNetworkChanged`/`setOnMsfStatusChanged`（注册回调），不需要不存在的
network service getter；R3 可用 `ProfileService.startStatusPolling` + `getSelfStatus`/`getStatusInfo` 作为
持续身份/状态源，并用会话 `getSessionId` 与会话身份查询核对。

仍未证：setter 回调的参数形状/状态枚举、触发时机与线程、移除方式（传空回调还是单独 remove）、
以及“状态变化 → 撤权 → 恢复重绑”的接线。下一步应先做研究型 observer（只注册 session setter 与
ProfileService 轮询，未知码只审计、不自动恢复），用 Node VM 夹具锁定保守语义，再执行 R2/R3 现场。

### 6.1 研究型网络观察者（离线实现，2026-11-21）

- 启用：research 构建、带 account 的 attach 时，`D8_START_JS` 收到 `caligoNetworkObserverReady=true`，在 session 上注册
  `setOnNetworkChanged`/`setOnMsfStatusChanged`/`setOnWeakNetChanged`。
- 记录：回调只写有界形状——最多 4 个参数；数字/布尔保留值；字符串只写长度；对象只写最多 16 个键名；最多 32 行并带溢出标记。
  字符串内容、身份与错误文本都不入库。
- 取数：`D8_NETWORK_EVENTS_JS` 在 poll 成功后的 owner tick 由 `network_state` 阶段行取走并清空；`R.stopped` 后回调 no-op。
- 夹具：`crates/caligo-bridge/tests/network_observer.rs` 验证注册、sanitize、无泄漏、停止后不再追加。
- 限制与风险：setter 可能覆盖 QQ 自身网络处理器，仅研究构建启用；本轮除已有点击监听器外不注册 message listener、不发送消息；
  尚未接入撤权/重绑动作，也未调用 ProfileService 轮询。
- 构建：源码 `02c15b0`，EXE `4303D964…`、bridge `DAD17EEE…`、caligod `5837BAE7…`。R2 现场需安装该构建后断网/恢复。

### 6.2 R2 网络观察现场包（离线准备，2026-11-22）

- 构建：源码 `52d05a1`，EXE `AE301239…`、bridge `6822935E…`、caligod `E017BCF0…`；观察者由 `qq-surface` 的 `network_observer` 字段确认（不再在 attach 时新增执行，避免破坏 lab 的 last-script 合同）。
- 现场包：ignored `local-evidence/feat005-r2-network-20261122-unbound/`（6 文件，清单 SHA-256 `BAEF5400…4634`），未绑定、零发送。
- 流程：一次准入/attach → 等待 `listener_armed` → `qq-surface` 确认 `network_observer` 含三个 setter → self 查询 → 打印 `READY_NETWORK` →
  操作员对测试 QQ 做一次网络状态变化（回环 IPC 保持可用，QQ 不退出）→ 采集有界 `network_state` 行（最多 600 秒）→
  1000 ms 观察停止。
- 失败语义：observer 未 armed 直接失败；600 秒无事件超时失败；停止非零保根；全程不重发、不重试干预。
- 未执行：R2 现场需操作员干预；`network_state` 的实际参数形状与触发条件仍待首次样本。

### 6.3 R2 首次在线尝试（无干预）与注册诊断（2026-11-22）

- 实例 `8036`（创建 `2026-11-22T01:30:07.8931547Z`），构建 `c87c7f2`：准入 PASS、一次 attach、self 查询通过；
  `qq-surface` 显示观察者只注册了 `setOnNetworkChanged`/`setOnWeakNetChanged`（`setOnMsfStatusChanged` 缺失）。
- 操作员确认 600 秒窗口内**没有执行网络断开**，因此 `network_state` 行为 0、runner 超时失败；观察停止仍全阶段通过、
  exit 0、无 reason；零发送。该样本**不构成**对观察者的负结论。
- 诊断增强（构建 `00dbc7f`）：观察者记录每个 setter 的注册错误类名（`missing` 或异常类名）与声明 arity，并通过
  `qq-surface` 的 `network_observer_errors`/`network_observer_arity` 字段输出；R2 runner 把它们记入 summary。
- 现场包更新：pins `C928D9BC…`/`40CBD258…`/`7F8C1BE1…`，清单 `EE7F4266…8BFF`；首次尝试证据归档在包内
  `attempt1-pid8036-nointervention/`。重跑需新 QQ 代次并在 `READY_NETWORK` 后真正断网 20–60 秒再恢复。

### 6.4 R2 第二次在线尝试（完整断网，仍无回调）与调用计数（2026-11-22）

- 实例 `31808`（创建 `2026-11-22T02:43:33.0342589Z`），构建 `ce0d0cd` 前的 `00dbc7f`：准入 PASS、attach、self 查询通过；
  `qq-surface` 显示 `setOnNetworkChanged`/`setOnWeakNetChanged` 注册、`setOnMsfStatusChanged` 抛错；操作员确认
  **完全断开 Wi-Fi+以太网 20–60 秒**且 QQ 显示离线/重连，但 600 秒窗口内 `network_state` 仍为 0。
- 诊断增强（构建 `ce0d0cd`）：观察者新增 `networkObserverCalls` 计数；`qq-surface` 输出
  `network_observer_calls`、绑定类名与 `NodeIQQNTWrapperNetwork`/`NodeIKernelLoginService` 类型；
  R2 runner 在网络窗口结束后再读一次 surface，区分“回调从未触发”与“采集/排空问题”。
- 现场包更新：pins `F432CD20…`/`42A1EFFD…`/`A20CBEB2…`，清单 `B0DBCE31…6EA3`；两次尝试分别归档为
  `attempt1-pid8036-nointervention/`、`attempt2-pid31808-full-disconnect/`。
- 目前判断：session 的 `setOnNetworkChanged`/`setOnWeakNetChanged` 在完整断网下没有可观测回调（或注册被 QQ 覆盖）；
  `setOnMsfStatusChanged` 需要 3 个参数。下一步优先看绑定层是否暴露 `NodeIQQNTWrapperNetwork`
  （`addNetworkListener`），或改用拉取式状态源（`getMsgService().getOnLineDev`、`ProfileService.getSelfStatus`）。

### 6.5 探测 v2：绑定层与拉取式状态源（离线准备，2026-11-22）

- `qq-surface` 新增：`binding_classes`（`NodeIQQNTWrapperNetwork`/`NodeIKernelLoginService`/`NodeIKernelLoginListener` 的
  keys/proto，若绑定层网络类有 `getNetworkStatus` 则记录其返回形状）；`status_probe`（`ProfileService.getSelfStatus`
  的返回类型/数值、`getStatusInfo`/`getCoreInfo`/`getOnLineDev` 的 arity）；保留 `network_observer_calls`。
- R2 runner 新增 `--probe-only`：完成准入、attach、self 查询与 surface 后直接观察停止，不等待网络干预、零发送。
- 构建：源码 `52d05a1`，EXE `AE301239…`、bridge `6822935E…`、caligod `E017BCF0…`；现场包清单 `BAEF5400…4634`。
- 下一步现场（一次）：`python -B run-field.py --probe-only`，读取绑定层是否存在网络类及其方法、`getSelfStatus` 形状；
  据此决定 R2/R3 的观察源（绑定 listener 或周期拉取），再实现撤权/重绑。

### 6.6 探测 v2 现场结果与网络服务监听（2026-11-22）

- 实例 `7036`（创建 `2026-11-22T04:03:37.8628779Z`），构建 `52d05a1`，`--probe-only` 零发送：
  - 绑定层暴露 `NodeIQQNTWrapperNetwork`（静态 `openNetworkService`，0 参）与 `NodeIKernelLoginService`（静态 `get`）；
    `NodeIKernelLoginListener` 为 `undefined`；
  - `status_probe.self_status` 返回 **Promise**（异步），`getStatusInfo`/`getCoreInfo`/`getOnLineDev` 均为 0 参函数；
  - 观察者 `network_observer_calls=0`（本次无网络干预，符合预期）。
- 据此构建 `5527c68`：arm 时调用 `NodeIQQNTWrapperNetwork.openNetworkService()`，在返回对象上注册
  `addNetworkServiceListener`（1 参）并复用同一有界采集；surface 输出 `network_service` 的 keys/proto 与
  `getNetworkStatus` 形状；`--probe-only` 仍为零发送。
- 构建 pin：EXE `1DDB9123…`、bridge `39111BAB…`、caligod `69C1E2FE…`；现场包清单 `7CC648DF…4D56`。
- 下一步：新代次上重跑 R2（真正断网 20–60 秒），观察 `addNetworkServiceListener` 是否产生 `network_state`；
  若仍无事件，则改用 `getNetworkStatus`/`getSelfStatus` 周期拉取。

### 6.7 合成会话网络切换（离线实现，2026-11-22）

物理断网会中断操作员的流式传输，因此新增**会话层合成切换**作为 R2 的折中干预：

- bridge 研究导出 `caligo_qq_net_offline` / `caligo_qq_net_online`：置一次性模式并唤醒 owner 泵；
  owner 在 poll 成功的 tick 上调用 `session.offLine()` / `session.onLine()`，把结果写入 `network_toggle` 阶段行。
- CLI `qq-net-offline` / `qq-net-online`：沿用状态命令的准入与已映射 bridge 校验，只发一次请求并打印该行。
- R2 runner 新增 `--synthetic-toggle`：attach → 确认观察者 → self 查询 → `qq-net-offline` → 等待
  `network_state`（≤120 秒）→ `qq-net-online` → 再等 30 秒 → 观察停止；零发送。
- 构建：源码 `e3328b1`，EXE `ED1990DE…`、bridge `E629A73E…`、caligod `EC486513…`；现场包清单 `CE3BB394…854C`；
  Node VM 夹具覆盖 offLine/onLine 成功、无会话与抛错路径。
- 语义边界：这是**合成状态注入**，验证“网络状态通知 → 观察者”的链路，不是真实丢包或 OS 断网；
  等流式传输结束后仍应补一次真实断网样本。

### 6.8 合成切换尝试与异步 self-status 探测（2026-11-22）

- 实例 `34040`，构建 `e3328b1`，`--synthetic-toggle`：`session.offLine()` / `onLine()` 以 0 参调用均返回
  `ERR:Error`；固定构建断言这两个方法**需要 1 个参数**，因此合成切换暂不可用，`network_state` 为 0；
  观察停止仍通过、零发送。证据归档 `attempt4-pid34040-synthetic-toggle/`。
- 静态补注：`NodeIQQNTWrapperSession::offLine/onLine/offLineSync` 均 `needs 1 arguments`；参数语义待定，
  不再盲试。
- 转向 R3 的拉取源：`qq-surface` 现在调用 `ProfileService.getSelfStatus()`（Promise）并把解析后的
  有界形状存入 `self_status_result`/`self_status_error`；`--probe-only` 会二次读取。
- 构建：源码 `293267c`，EXE `4891BCB3…`、bridge `80C5724D…`、caligod `98BB27F1…`；现场包清单 `E6D06C78…0839`。
- 下一步（一次零发送）：`python -B run-field.py --probe-only`，读取 `self_status_result` 与 `network_service`
  形状，作为 R3 身份观察的输入；R2 真实断网样本等流式传输结束后补。

### 6.9 网络服务状态拉取与 self-status 探针修正（2026-11-22）

- 实例 `47988` 的首次 `--probe-only` 在 runner 解析阶段失败（报告 detail 为单引号 JSON，`ast.literal_eval`
  无法处理 `null`）；已改为单引号→双引号后走 `json.loads`。该次 attach 已捕获关键 surface：
  - `network_service.proto` 含 `startGetNetworkStatus`、`stopGetNetworkStatus`、`getNetworkStatus`、
    `addNetworkServiceListener`、`removedNetworkServiceListener`；
  - `getNetworkStatus()` 同步返回 **number=1**；`self_status` 仍为 Promise（首次调用未结算）。
- 新构建 `1cb3954`：owner 泵每 tick 拉取一次 `networkService.getNetworkStatus()`，仅当值变化时写
  `network_status` 行（`{previous,current}`，有界）；`self_status_result` 由第二次 surface 读取。
- 构建 pin：EXE `C32A5701…`、bridge `2F2D6724…`、caligod `B42C1715…`；现场包清单 `EC7F602D…D86C`。
- 下一步（零发送）：`--probe-only` 读取 `network_status`（初始值）、`self_status_result`、`network_service`；
  之后实现 R3 身份核对与 R2 状态变化 → 撤权接线。
