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

下载并解压 `caligo-<版本>-windows-x64.zip`，在解压目录打开 PowerShell。
运行 `.\caligo.exe --help` 查看入口。启动所需的参数和配置由子命令决定。

| 用途 | 命令 |
| --- | --- |
| 启动单账号服务 | `.\caligo.exe daemon --pipe-prefix <name> --journal <path> --baseline <id> --account <uin>` |
| 启动多账号管理 | `.\caligo.exe manager --root <私有配置目录的绝对路径>` |
| 使用媒体工具 | `.\caligo.exe media --help` |
| 查看只读诊断和控制命令 | `.\caligo.exe cli --help` |

包中的 `caligo_bridge.dll` 是 QQ 进程内桥接组件，由受门控的加载流程使用。
账号配置、管理界面及协议设置见[账号管理](https://github.com/Enoa-devteam/Caligo/blob/main/docs/guides/account-management.md)、
[管理 WebUI](https://github.com/Enoa-devteam/Caligo/blob/main/docs/guides/management-ui.md)和[OneBot 接入](https://github.com/Enoa-devteam/Caligo/blob/main/docs/guides/onebot-app-entries.md)。

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
