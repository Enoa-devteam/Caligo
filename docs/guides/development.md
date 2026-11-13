# 开发与验证

适用：修改 Rust、测试、验证脚本或本地构建方式。
依据：[Cargo.toml](../../Cargo.toml)、[toolchain](../../rust-toolchain.toml)、
[执行台账](../execution/2026-10-09-snowluma-parity/ledger.md) 的 N0 及后续测试记录。
工具链、features 或测试布局变化时更新本页；成功次数写入进度，不在本页缓存。

## 环境与初始化

Windows x64 + Rust MSVC 工具链；安装所需 Rust/Windows SDK 是环境准备，
验证脚本不安装依赖、不访问 QQ。Node.js 用于本仓库文档检查和 skill 脚本。
运行 `.\init.ps1 -Mode docs` 可验证文档；Git Bash 的 `./init.sh` 委托同一脚本。
[.gitattributes](../../.gitattributes) 固定 init.sh 使用 LF，避免 Windows checkout 破坏 Bash 入口。

| Mode | 执行内容 |
|---|---|
| docs（默认） | 文档链接、入口长度、feature 状态/依赖和 skill 锁文件检查 |
| default | docs + 普通构建 workspace 测试 |
| research | docs + research workspace 离线测试 |
| all | docs + default + research；Rust 公共合同修改的验证入口 |

脚本从自身路径定位仓库、检查外部命令退出码，任一步失败返回非零。
所有 Cargo 命令使用 `--locked --offline --target-dir target-harness`。
此目录已受 `/target*` 忽略规则覆盖，与历史现场 DLL 目录分离。
缺离线依赖时保留失败结果，再根据任务明确准备依赖。

## 单项命令

```powershell
node scripts/check-docs.mjs
cargo build --workspace --locked --offline --target-dir target-harness
cargo test --workspace --locked --offline --target-dir target-harness
cargo test --workspace --features caligo-core/research,caligo-cli/research --locked --offline --target-dir target-harness
cargo build --release --workspace --locked --offline --target-dir target-harness
```

release 构建在发布/构建问题相关任务中执行，不作为纯文档修改必跑项。
测试已编译对应目标；无需为获取更大检查数量重复运行相同套件。
研究构建启用代码供离线合同测试，不改变现场准入。

## 选择相关测试

| 改动 | 测试入口（路径均已存在） |
|---|---|
| 文本编码 | [qq_wire_golden](../../crates/caligo-model/tests/qq_wire_golden.rs)、[qq_text_codec](../../crates/caligo-core/tests/qq_text_codec.rs) |
| 默认研究门控 | [legacy_gate](../../crates/caligo-cli/tests/legacy_gate.rs)、[onebotd_gate](../../crates/caligo-core/tests/onebotd_gate.rs) |
| 原生 adapter、生命周期 | [native_adapter_contract](../../crates/caligo-bridge/tests/native_adapter_contract.rs)、[resident_lifecycle](../../crates/caligo-bridge/tests/resident_lifecycle.rs) |
| IPC、恢复、持久化 | [ipc_contract](../../crates/caligo-core/tests/ipc_contract.rs)、[runtime_contract](../../crates/caligo-core/tests/runtime_contract.rs)、[recovery_contract](../../crates/caligo-core/tests/recovery_contract.rs) |
| daemon 进程与管道 | [caligod_smoke](../../crates/caligo-core/tests/caligod_smoke.rs)、[daemon_chain](../../crates/caligo-core/tests/daemon_chain.rs)、[daemon_client_lab](../../crates/caligo-core/tests/daemon_client_lab.rs) |
| OneBot 解析/HTTP | [onebot.rs](../../crates/caligo-core/src/onebot.rs)、[onebot_http.rs](../../crates/caligo-core/src/onebot_http.rs) 内的测试 |

定向运行示例：

```powershell
cargo test -p caligo-model --test qq_wire_golden --locked --offline --target-dir target-harness
cargo test -p caligo-bridge --test native_adapter_contract --features research --locked --offline --target-dir target-harness
```

## 结果与失败

在 [progress](../../progress.md) 记录命令、HEAD、日期、结果和验证层级。
代码任务检查相应基线，报告新增失败与已有失败；先定位原因再扩大改动。
台账曾记录一次 `STATUS_ACCESS_VIOLATION` 后未复现；若再次发生保留原始结果，
按同一二进制/并发条件缩小复现范围，不能用重跑成功覆盖失败。

LAB、fake 和合成身份测试证明其合同范围；真实 QQ 接收、发送、停止、
长期存活与 OneBot 应用互通另由现场证据验收。
