# Caligo 文档索引

先读 [当前进度](../progress.md)，再按本次任务进入专题；本索引不承载实验指令。
`guides/` 是维护中的规范，`plans/` 是有日期的目标合同，`execution/` 是执行证据，
`acceptance/` 是特定阶段验收，`research/` 是固定环境的研究记录。
历史正文保留原路径；旧状态与行号不能直接代替当前 HEAD 的事实。

## 常用入口

| 任务 | 文档 |
|---|---|
| Agent 开始/收尾 | [AGENTS](../AGENTS.md)、[handoff](../session-handoff.md) |
| 模块边界与调用链 | [架构](guides/architecture.md) |
| 构建与测试 | [开发验证](guides/development.md) |
| OneBot 群/好友查询动作 | [查询动作](guides/onebot-queries.md) |
| FFI、适配、现场准入 | [原生研究](guides/native-research.md) |
| 新规则、状态、文档维护 | [文档规范](guides/documentation.md) |
| 工作块依赖与完成门 | [feature_list](../feature_list.json) |

## 计划与执行（按阶段选择）

- N4 当前解阻入口：[2026-10-11 响应恢复计划](plans/2026-10-11-gitnexus-plan-caligo-n4-response-recovery.md)；先离线修请求关联与结果语义，现场仍硬停。
- N4 执行现状：[响应恢复执行记录](execution/2026-10-11-n4-response-recovery.md)；序号 authority 决策见 [ADR](decisions/2026-10-11-wire-sequence-authority.md)。R1 离线合同通过，R5/R6 OneBot transports 有离线接线与协议合同；R2 native 寿命、R3 强身份、R4 现场和 R6 真实应用完成门仍开放。

- 当前 N0–N8 目标：[2026-10-09 能力对齐计划](plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md)，
  [N 阶段执行台账](execution/2026-10-09-snowluma-parity/ledger.md)；台账已经续写至 10 月 11 日。
- P0–P9 原生合同：[2026-10-08 合同恢复计划](plans/2026-10-08-gitnexus-plan-caligo-native-contract-recovery.md)，
  [P 阶段台账](execution/2026-10-08-native-contract-recovery/ledger.md)。
- K4 常驻恢复：[2026-10-07 恢复计划](plans/2026-10-07-gitnexus-plan-caligo-k4-runtime-recovery.md)。
- K0–K6 初始目标：[2026-10-06 内核计划](plans/2026-10-06-gitnexus-plan-caligo-native-qq-kernel.md)。

## 环境、来源与恢复

- 固定环境：[environment.json](research/environment.json)；本机运行健康需当次核实。
- 固定适配：[manifest](contracts/version-adapter-manifest.json)、[capability profile](contracts/qq-9.9.33-52230.capability-profile.json)。
  profile 的旧 gate 与最新台账有不同时间边界；不通过文档整理自动提升门。
- 采用/参照来源：[source-register](research/source-register.md)。
- 测试身份与唯一正文：[test-scope](research/test-scope.md)。
- 事故收尾与恢复：[recovery-notes](research/recovery-notes.md)、[K4 incident-register](research/k4-incident-register.md)。
- 身份失效：[identity-invalidation-observations](research/identity-invalidation-observations.md)。

## 入口、线程与寿命研究

- ABI 起点：[native-entry-contract](research/native-entry-contract.md)。
- 加载路线：[loading-route-decision](research/loading-route-decision.md)、[probe-build-record](research/probe-build-record.md)。
- K4 冻结与恢复：[execution-scope](research/k4-execution-scope.md)、[native-route-decision](research/k4-native-route-decision.md)。
- 宿主启动：[bootstrap-contract](research/k4-bootstrap-contract.md)、[js-context-acquisition](research/js-context-acquisition.md)。
- 线程与异步持有：[thread-contract](research/qq-native-thread-contract.md)、[lifetime-contract](research/qq-native-lifetime-contract.md)。
- 固定 build 重建：[QQ 合同/版本适配研究](research/2026-10-08-qq-native-contract-and-version-adaptation.md)，
  [SnowLuma 工作流重建](research/2026-10-08-snowluma-native-workflow-reconstruction.md)。
- 阶段历史报告：[k4-stage-report-01](research/k4-stage-report-01.md)。

## 收发与响应研究

- 9.9.33 本地链路研究：[2026-11-11 收发验证](research/2026-11-11-9.9.33-local-verification.md)。研究报告含明确标注的伪身份字段，不构成生产实现或现场验收。
- 接收：[receive-design](research/qq-native-receive-design.md)。
- 发送实验合同：[send-probe-design](research/qq-native-send-probe-design.md)。
- 响应消费者历史起点：[response-consumer-scope](research/response-consumer-scope.md)。
  “实现未开始”等旧结论以 N 台账后续接线记录补充；scope 原文保留。
- 详细样本与负结果：从 P/N 执行台账进入对应 `evidence/`，按样本定位，
  不把整个证据目录作为常驻上下文。真实本机数据另保存在被忽略的 `local-evidence/`。

## 历史验收（逐项读取范围）

| 阶段/问题 | 记录 |
|---|---|
| K3 接收 | [k3-d-receive](acceptance/k3-d-receive.md) |
| K3 发送首样本/参数合同 | [k3-send-001](acceptance/k3-send-001.md)、[k3-f-param-send](acceptance/k3-f-param-send.md) |
| K4 现场准入 | [k4-field-entry](acceptance/k4-field-entry.md) |
| K4 首次 attach | [k4-d7-first-attach](acceptance/k4-d7-first-attach.md) |
| K4 daemon 连通 | [k4-d7b-daemon-link](acceptance/k4-d7b-daemon-link.md) |
| K4 接收/发送接线 | [k4-d8-receive](acceptance/k4-d8-receive.md)、[k4-d9-send-wiring](acceptance/k4-d9-send-wiring.md) |

记录中的 PASS 只针对其日期、构建、路径与样本；生产完成门仍由当前工作块的完整证据决定。
