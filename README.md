# Caligo

自有 Rust QQ 接入项目：官方 QQ 会话 → 自有 bridge → 自有 core → OneBot V11。
官方 QQ 承担登录、签名、会话与网络通信；产品运行链路采用自有实现。

**项目仍处研究与整合阶段，生产 MVP 尚未验收。** 最新状态与证据入口见
[进度记录](progress.md)，产品工作块见 [feature_list.json](feature_list.json)。
历史研究结果和生产准入按 [原生研究边界](docs/guides/native-research.md)区分。

## 从这里开始

- 首次阅读：[文档索引](docs/README.md)，按任务查专题、计划或验收记录。
- 参与开发：[开发与验证指南](docs/guides/development.md)。
- Agent 工作：[AGENTS.md](AGENTS.md)，随后读进度与交接。
- 模块与调用链：[架构指南](docs/guides/architecture.md)。

## Windows Release 启动

[GitHub Releases](https://github.com/Enoa-devteam/Caligo/releases) 提供 Windows x64 ZIP 和
SHA-256。当前最新发布为 [v0.2.0](https://github.com/Enoa-devteam/Caligo/releases/tag/v0.2.0)；
该版本早于下文的 `manage` 入口和正式桥统一，使用时请查阅
[v0.2.0 的 README](https://github.com/Enoa-devteam/Caligo/blob/v0.2.0/README.md)。

以下说明适用于当前 main 的源码构建，或包含这些改动的后续发布包。Windows x64 的
锁定依赖构建与打包步骤见[开发与验证指南](docs/guides/development.md#github-actions-test--release)。
构建后的 `caligo.exe` 和 `caligo_bridge.dll` 须来自同一构建。

在 PowerShell 中运行 `.\caligo.exe manage`，打开终端显示的本机地址，用终端提示的
`management.token` 文件内容连接页面。添加或选择 QQ 号并点击「启动托管」；
`.\caligo.exe manage --terminal` 提供终端入口。管理入口启动所选账号的 Caligo 服务，
不会替代官方 QQ 登录或自动完成桥接。

真实接入前须由操作者完成进程和模块指纹等准入检查，并按
[管理 UI 的接入步骤](docs/guides/management-ui.md#接入-qq-与核验)配置和启动 bridge。
当前准入对象为精确固定的 QQ 9.9.36-53644；未知构建会被拒绝。桥接就绪后，
点击「核验 QQ 登录」确认所选账号。管理凭据和账号凭据应保存在本机私有目录。

| 用途 | 命令 |
| --- | --- |
| 浏览器管理账号与消息日志 | `.\caligo.exe manage` |
| 终端管理账号与消息日志 | `.\caligo.exe manage --terminal` |
| 单账号服务 | `.\caligo.exe daemon --pipe-prefix <name> --journal <path> --baseline <id> --account <uin>` |
| 多账号管理 | `.\caligo.exe manager --root <私有配置目录的绝对路径>` |
| 媒体工具 | `.\caligo.exe media --help` |
| 诊断与控制 | `.\caligo.exe cli --help` |

具体配置和应用接入见[账号管理](docs/guides/account-management.md)、
[管理 WebUI](docs/guides/management-ui.md)和 [OneBot/SSE/MCP 接入](docs/guides/onebot-app-entries.md)。
现场样本覆盖范围与尚未验收的能力见[当前进度](progress.md)，不能由命令存在推断生产就绪。

## 本地验证

需要 Windows x64、Rust MSVC 工具链（版本由 [rust-toolchain.toml](rust-toolchain.toml) 固定），
文档检查需要 Node.js。依赖来自 [Cargo.lock](Cargo.lock)，默认离线验证。

```powershell
.\init.ps1 -Mode docs  # 纯文档检查
.\init.ps1 -Mode all   # 文档 + 普通构建及 research 离线测试
```

验证产物写入 `target-harness/`；脚本行为和单项命令见开发指南。

## 源码构建的只读 CLI

以下为普通构建后的只读入口示例，路径按本机构建目录填写：

```powershell
caligo-cli process
caligo-cli exports <module-path> --all
caligo-cli verify-manifest docs/contracts/version-adapter-manifest.json
```

版本基线见 [适配清单](docs/contracts/version-adapter-manifest.json)。
现场使用前阅读原生研究边界与当前交接，不从历史 PID 或旧 DLL 恢复实验。

## 仓库内的 skill

按指定命令安装：

```powershell
npx skills add walkinglabs/learn-harness-engineering --skill harness-creator
```

上游入口：[harness-creator/SKILL.md](https://github.com/walkinglabs/learn-harness-engineering/blob/main/skills/harness-creator/SKILL.md)，
来源锁定：[skills-lock.json](skills-lock.json)。项目规则在 AGENTS 与专题指南中维护，
上游 skill 为可选本机工具，干净检出与 CI 只校验仓库内的锁文件。
