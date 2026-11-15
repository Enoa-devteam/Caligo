# 原生研究边界

适用：FFI、版本适配、响应消费者、发送元素、宿主生命周期或任何现场动作。
当前状态先读 [progress](../../progress.md) 与 [handoff](../../session-handoff.md)，
再按本页链接读取相关合同。历史 PID、实例健康和 DLL hash 都须当次核实。

## 规则来源与替换条件

| 规则 | 来源与适用条件 | 复审/替换条件 |
|---|---|---|
| 自有产品链路，第三方仅作参照 | [N 计划](../plans/2026-10-09-gitnexus-plan-caligo-snowluma-capability-parity.md) §3；增加运行依赖或迁移发送路径时 | 项目方向由用户明确变更，并记录新依赖决策 |
| 精确 hash 适配，未知版本拒绝 | [manifest](../contracts/version-adapter-manifest.json)、[profile](../contracts/qq-9.9.33-52230.capability-profile.json)；全部原生调用 | 新 build 有独立 ABI/线程/寿命及能力验收后新增 profile；旧基线继续绑定旧证据 |
| 身份、凭据与原始现场数据本机保存 | [test-scope](../research/test-scope.md)、[.gitignore](../../.gitignore)；采样、fixture、日志和发布 | 可公开脱敏 fixture 有明确审核依据；真实原始数据继续留本机 |
| research 开关不等于现场准入 | [K4 scope](../research/k4-execution-scope.md)、[field-entry](../acceptance/k4-field-entry.md)；启用 feature 或尝试加载时 | 准入合同被带证据的新合同替代；feature 仍与现场验收分开 |
| 保留 unknown，未证异步寿命不卸载 | [lifetime](../research/qq-native-lifetime-contract.md)、N 计划 T08/T13；发送、超时、stop/recovery | 有新寿命/结果关联证据并同步合同、实现和恢复测试 |
| 调用点补丁及 slot7 钩子路线否决 | [N 台账](../execution/2026-10-09-snowluma-parity/ledger.md) 2026-10-11（三十八）及 slot7 记录；插桩方案 | 新方案独立审查合同、失败原因及受控验证；现有损失证据始终保留 |

## 执行前与停止边界

正式托管使用同一套发布版 EXE 与 bridge。正式 `qq-entry` 在加载前核验指定 QQ
进程代次、已加载模块的固定哈希和唯一 Environment。bridge 在宿主内再次核对模块哈希与基址。
PE/backend 审计和测试实例指定属于现场准入。历史诊断入口继续受 research 门控。

1. 从本轮请求确认现场范围；文档或离线任务只执行对应工作。
2. 核对固定 build/hash、当次实例身份及准入，记录 PID 与创建时间和选择依据。
3. 按实际工作读取 [thread contract](../research/qq-native-thread-contract.md)、
   [lifetime contract](../research/qq-native-lifetime-contract.md) 和当前 capability gate。
4. 每轮有对照、唯一正文、输入和停止条件；失败保留层级与未知原因。
5. 最新台账的硬停和路线否决优先于早期建议；实例损失后先保存证据与恢复记录。

本轮 2026-10-11 台账的最后现场记录为硬停，不能由文档更新视为恢复。
日期变化也不自动满足准入；下一现场任务仍需当次身份/健康/合同核对。
恢复参考 [recovery-notes](../research/recovery-notes.md)，该文件的历史操作步骤须结合当前门控审查。

## 如何解释证据

| 层级 | 能证明的内容 |
|---|---|
| static | 固定样本的代码/结构/锚点结论 |
| implemented | 有代码，仍需检查实际调用路径 |
| wired | 已接入路径，仍需运行检验 |
| lab | 指定离线合同在合成/自建环境通过 |
| field | 指定 QQ build、实例、方向及样本的现场观察 |
| production acceptance | 完整矩阵、正常入口、真实下游与恢复/持续运行满足发布门 |

数值计数和进度百分比不能替代上述证据。
`NativeStarted` 证明发起；`BusinessAccepted` 表示无稳定消息身份的响应接受。
core 中 `ConfirmedSuccess` 的命名不能单独证明业务关联或对端显示。
响应与 stable native ID 的映射仍要分别检查；当前核对风险列在 progress。

## 按问题进入

- ABI/加载：[native-entry](../research/native-entry-contract.md)、[bootstrap](../research/k4-bootstrap-contract.md)。
- 收到通知但缺记录字段：[receive design](../research/qq-native-receive-design.md) 与 N 台账最近接收记录。
- 发送失败码/cookie：[response-consumer scope](../research/response-consumer-scope.md) 是历史研究起点，
  已实现进展看 N 台账（三十五）至（四十一）和当前 consumer 源码。
- @：N 台账（三十九）的静默拒绝观察保留；具体服务器原因尚未独立证明。
  第三方路径的成功只证明对照研究，不能升级为 Caligo 自有能力。
- 生产支持提升：按 N 计划 T01–T22 和 feature 的 done criteria 逐项验收。
