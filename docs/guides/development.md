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

## GitHub Actions test / release

[Test](../../.github/workflows/test.yml) 在面向 `main` 的 PR、`main` 推送、手动触发
和 reusable 调用时运行。Windows 2025 x64 MSVC runner 使用仓库 toolchain 与 Node.js 24，
先 `cargo fetch --locked` 准备干净 runner，再按 default/research 矩阵执行 init 离线测试；
default 额外执行两份 Node capability tooling 测试。没有参考源码或指定 registry 二进制时，
Node 测试中的两项可选核验会 skip；这不构成参考源码完整性或现场验收证据。
旧 bridge FFI fixture 使用共享静态对象，因此 CI 设置 `RUST_TEST_THREADS=1`，测试内部显式
创建的并发线程照常执行。[runner 入口](../../.github/scripts/run-tests.ps1) 在当前测试进程
将 Windows token 默认文件 owner 临时设为当前用户 SID，文件 owner 预检通过后调用 init，
finally 恢复原默认 owner；不修改 DACL、不跳过账号隔离断言。首次托管运行的并发访问冲突与
管理员 owner 失败保留在 progress 的 CI 记录，不用后续成功覆盖。
固定 SHA 的 capability inventory 通过 `.gitattributes` 使用 LF，防止 Windows checkout
改变证据字节；CI 不重新生成清单或替换其预期 SHA。

[Release](../../.github/workflows/release.yml) 在 `v*` 标签推送或手动触发时先复用 Test，
通过后构建 default/research 两套 release。标签必须为 workspace 版本的 `v<version>`
或 `v<version>-<prerelease>`；改版本时同步 Cargo manifests/lock 后再打标签。
手动运行仅上传保留 14 天的 Actions artifacts，不创建 GitHub Release；标签运行将两套包
发布为 GitHub prerelease（不设为 latest）。本任务只建立流程，不创建版本标签或发布。

[打包脚本](../../.github/scripts/package-release.ps1) 仅收集五个 EXE、bridge DLL、README、
LICENSE 和 build-info.json；产物为 ZIP 与独立 SHA-256 文件，build-info 记录源码 commit、
Rust 版本、target、research 开关及包内文件 SHA-256。原始证据、QQ 文件与凭据不进入包。
从仓库根目录、完成相应构建后可单独运行 `.github/scripts/package-release.ps1 -Mode default`
或 `-Mode research`；必须与刚构建的 feature 集匹配。CI 构建不提升产品 feature 状态，
research 包的现场使用仍遵守当前原生准入合同。

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
