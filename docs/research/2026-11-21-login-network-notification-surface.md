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
