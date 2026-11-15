# 架构与调用链

适用：修改模块边界、IPC、原生收发接线、OneBot 动作与状态传播。
维护依据：当前源文件与 Cargo manifests；阶段状态统一查 [progress](../../progress.md)。
代码重命名或职责迁移时更新本页链接，历史设计继续保存在 plans。

## 模块职责

| 模块 | 责任与源码入口 |
|---|---|
| caligo-model | 共享身份/事件模型、IPC v2/v3、帧与文本 wire；[lib.rs](../../crates/caligo-model/src/lib.rs)、[qq_wire/send.rs](../../crates/caligo-model/src/qq_wire/send.rs) |
| caligo-bridge | QQ 进程内 ABI、owner pump、resident 与诊断入口；[lib.rs](../../crates/caligo-bridge/src/lib.rs)、[qq_entry.rs](../../crates/caligo-bridge/src/qq_entry.rs) |
| caligo-core | 进程外 actor、journal、IPC、消息索引与 OneBot；[runtime.rs](../../crates/caligo-core/src/runtime.rs)、[daemon.rs](../../crates/caligo-core/src/daemon.rs) |
| caligo-cli | 只读 PE/进程工具、正式 bridge 接入与受门控诊断；[lib.rs](../../crates/caligo-cli/src/lib.rs)、[qqentry.rs](../../crates/caligo-cli/src/qqentry.rs) |

[Cargo.toml](../../Cargo.toml) 是依赖权威；bridge 同时生成 rlib/cdylib。
`caligo-core/research` 转发诊断 feature，历史探针门控由 [gate.rs](../../crates/caligo-bridge/src/gate.rs) 决定。
正式 bridge 在两种构建中共用精确版本的 owner pump 与 IPC 实现。
各源文件头中的 K1/K4 说明记录其起源；当前行为须读取实现与最新台账。

## 常驻发送路径

1. [caligod](../../crates/caligo-core/src/commands/daemon.rs) 启动 core；
   [daemon](../../crates/caligo-core/src/daemon.rs) 管理 control/bridge 管道。
2. [runtime](../../crates/caligo-core/src/runtime.rs) 维护账号 actor、请求代次及持久状态。
3. [resident](../../crates/caligo-bridge/src/resident.rs) 接收 IPC，驱动 adapter。
4. [native_resident](../../crates/caligo-bridge/src/native_resident.rs) 与
   [native_session](../../crates/caligo-bridge/src/native_session.rs) 管理原生会话。
5. [g3_ops](../../crates/caligo-bridge/src/g3_ops.rs) 提交原生发送，
   [consumer_probe](../../crates/caligo-bridge/src/consumer_probe.rs) 接收 cookie 关联响应，
   [native_response](../../crates/caligo-bridge/src/native_response.rs) 终结响应账本，resident 再上行结果。

这是已存在的代码接线路径；现场验收范围查 progress 和对应台账。
`Queued`、`NativeStarted`、`BusinessAccepted` 与带稳定身份的业务成功是不同证据。
consumer、会话、core 状态和 OneBot 输出都要分别核对，不能凭某个类型名推断端到端语义。

## 接收与 OneBot

- 原生捕获/规范化入口：[native_receive](../../crates/caligo-bridge/src/native_receive.rs)、
  [native_resident](../../crates/caligo-bridge/src/native_resident.rs)；核对每条记录身份、正文、时间来源。
- bridge 事件经 IPC 进入 daemon/actor 和
  [journal](../../crates/caligo-core/src/journal.rs)。AccountActor 可用显式
  `OneBotMessageProjection` 将已核实字段写入 append-only JSONL
  [message_store](../../crates/caligo-core/src/message_store.rs)；
  [Daemon](../../crates/caligo-core/src/daemon.rs) 将该 store 暴露为 OneBot
  `StoreReader`，事件入账和 `get_msg` 查询有离线集成覆盖。
  投影缺失时不会从 opaque UID、接收序号或观察时间合成 OneBot 身份/时间；
  当前 CPB bridge 仍发送 `None`，所以这条 core 接线不代表原生 receive 已产生可查询消息。
- [onebot](../../crates/caligo-core/src/onebot.rs) 管理动作解析与响应，
  [onebot_events](../../crates/caligo-core/src/onebot_events.rs) 转换事件，
  [onebot_http](../../crates/caligo-core/src/onebot_http.rs) 提供 HTTP 组件。
  `get_msg` 动作可以查询 Daemon reader；普通 caligod HTTP listener、所有 transport
  或真实客户端互通仍未完成。
  当前 N4 离线实现与剩余接线边界见
  [响应恢复执行记录](../execution/2026-10-11-n4-response-recovery.md)。

## 改动前核对

- 身份：账号、宿主代次、会话代次、request/cookie/native ID 的作用域。
- 顺序：发送前预登记，回调终结一次，journal 持久化后才推进 ACK。
- 所有权：回调捕获 owned 数据、异步持有与停止排空按固定 ABI 合同执行。
- Wire：普通文本共享编码以 model 为入口，研究 @ 分支有独立支持边界。
- 版本：新增 payload/字段同时检查 IPC reader、journal 恢复与兼容拒绝路径。

具体场景与测试矩阵见
[N 阶段计划](../plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md) §8；
该计划的历史代码行号须重新定位，不能直接视为当前源码锚点。
