# Caligo — agent 入口

Caligo 是 Windows x64 Rust 工作区：自有 bridge 接入官方 QQ 会话，
自有 core 管理状态与账本，向应用提供 OneBot V11。项目能力与验收状态见
[progress.md](progress.md)，目录导航见 [docs/README.md](docs/README.md)。

## 全局硬约束

1. 生产运行链路使用自有 Rust bridge/core；第三方 QQ 后端仅作研究参照。
2. 原生结论绑定精确模块 hash；未知构建显式拒绝，不能猜偏移。
3. 区分源码实现、接线、离线测试和现场验收；每次完成声明附可定位证据。
4. 真实身份、聊天原文、凭据、QQ 二进制存本机 `local-evidence/`，不得新增入库。
5. `research` feature 仅启用研究代码；现场动作须满足当前准入合同。
6. 保留历史证据与未确认请求；unknown 不自动重发，未证寿命不卸载。

规则来源、适用条件及替换条件见
[原生研究指南](docs/guides/native-research.md)和
[文档维护指南](docs/guides/documentation.md)。

## 开始任务（Startup Workflow）

1. 确认仓库根目录，运行 `git status --short` 和 `git log -5 --oneline`。
2. 读 [progress.md](progress.md) 的 Current State 与
   [session-handoff.md](session-handoff.md)；按需查 [feature_list.json](feature_list.json)。
3. **One feature at a time**：以用户本次任务为 scope，只选择一个工作块；
   文档任务不能自行变成现场实验或未完成产品功能。
4. 按下表加载相关专题，查到证据后再决定改动。
5. 文档任务运行 `.\init.ps1 -Mode docs`；Rust 改动先按开发指南核对对应构建基线。

## 按任务加载

| 触发条件 | 必读入口 |
|---|---|
| 改模块边界、IPC 或收发调用链 | [架构指南](docs/guides/architecture.md) |
| 改 Rust、测试、构建或启动脚本 | [开发与验证](docs/guides/development.md) |
| 改 FFI、QQ 适配、响应消费者或考虑现场实验 | [原生研究边界](docs/guides/native-research.md) |
| 改代码或需要调用图/影响分析 | [GitNexus 代码智能指南](docs/guides/gitnexus.md) |
| 改 OneBot 查询动作或 `StoreReader` 读取 | [OneBot 群与好友查询](docs/guides/onebot-queries.md) |
| 改文档、规则、状态或交接文件 | [文档维护](docs/guides/documentation.md) |
| 找旧计划、事故、研究合同或验收样本 | [文档索引](docs/README.md)，只读相关条目 |

## 验证入口（Verification Commands）

```powershell
.\init.ps1 -Mode docs     # 链接、入口预算、状态依赖；纯文档任务
.\init.ps1 -Mode default  # docs + cargo test 普通构建
.\init.ps1 -Mode research # docs + cargo test 研究构建；离线测试
.\init.ps1 -Mode all      # docs + 两种构建的测试
```

Git Bash 使用 `./init.sh -Mode docs` 等等，委托同一个 PowerShell 实现。
`cargo build` 的独立命令、测试选择与环境要求见开发指南。

## 完成门（Definition of Done）

- 本次请求的行为/文档已经交付，scope 外的能力状态保持证据边界。
- 适用检查实际运行并记录结果；失败或未跑项如实列出。
- 产品能力只在其 `done_criteria` 全部具备证据后提升 `status`。
- 文档通过结构检查不代表 QQ 收发、长期运行或生产安全通过。

## 收尾（End of Session）

更新 progress 的本次证据、feature 状态和 handoff 下一步；具体写入约定见文档维护指南。
运行 `git diff --check` 并审阅改动，留下 restartable 的标准入口与明确阻塞项。
提交或发布按用户任务范围执行，不由文档整理自动触发。
