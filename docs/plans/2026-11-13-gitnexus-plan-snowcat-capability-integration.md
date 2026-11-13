# Caligo × SnowCat 全功能整合与统一验收计划

日期 2026-11-13（Asia/Shanghai）。本轮交付计划。Caligo 基线 `598b22b1218a67d45eeb8d5546a5a03e356bb534`，含当前工作树的 ProfileService 离线实现。私有参考库 `Napcat-Snowluma-research` 固定 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64`，已核对本地 checkout 和 GitHub HEAD 一致。

## 1. 目标

按固定研究提交建立全量能力清单。先完成自有实现与应用入口的整合并冻结同一版候选，随后逐项核实。已有 SnowCat 真实 QQ 结果作为接口与场景依据，复验集中验证 Caligo 替换后新增的端到端行为、版本差异和未覆盖项。所有可实现项在候选版中必须有实际 handler、backend 和正常入口。禁止复制、编译、链接或逐行转写反编译与重建 C/C++，也禁止把研究库二进制或 SnowCat/NapCat 作为生产后端。生产原生路径归 Caligo 的 Rust bridge/core 所有。

## 2. 当前实现

- [verified] `caligod` 通过同一 OneBot 动作层接入 HTTP、HTTP POST、正向和反向 WS。动作表覆盖文本发送、消息查询、登录、状态和版本。私聊发送仍受身份解析限制，正式 CPB 接收路径的 `onebot_projection` 为 `None`。[启动入口](../../crates/caligo-core/src/bin/caligod.rs) · [动作](../../crates/caligo-core/src/onebot.rs#L200) · [发送适配](../../crates/caligo-core/src/onebot_server.rs) · [接收投影](../../crates/caligo-bridge/src/daemon_client.rs#L920)
- [verified] `NativeSession` 先登记 cookie 后提交，core 对当前代次事件落 journal 并写 MessageStore，已有失败重放和持久 ID 查询结构。ProfileService 当前为未提交的 research 入口，须作为可复用基线纳入第一阶段，完整整合不得卡在该单项的现场结果。[会话](../../crates/caligo-bridge/src/native_session.rs#L293) · [事件入账](../../crates/caligo-core/src/runtime.rs#L824) · [身份探针](../../crates/caligo-bridge/src/uid_map_probe.rs#L135)

## 3. 证据和目标架构

| 参考证据 | 可复用的行为合同 | 使用方式 |
|---|---|---|
| [双账号业务与 OneBot](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/live-testing/business-message/RESULTS.md) | 私聊文本、群文本、私聊语音下载校验，HTTP 发送、WS 接收、两端各自按 ID 查询。QQ 服务器时间用于启动阈值 | 直接采用请求形状、事件顺序和对端可见性判据。保存每端自己的 ID，跨端关联依测试标记与发送者 |
| [真实宿主和原生收发](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/recovered/addons/LIVE-ACCEPTANCE-20261113.md) | 9.9.36 精确 wrapper 上的包观察、单次 SSO 关联、原生入口恢复、PCM/Silk 差分 | 作为自有 Rust 原生/媒体合同及 golden 向量。宿主调用使用本版模块与 ABI 签名 |
| [运行时和六个 DLL](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/live-testing/v2/COMPLETION.md) · [差分验收](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/recovered/ACCEPTANCE.md) | 已登录实例自动初始化、owner thread/env、同 provider、Promise 生命周期、导出/进程/PTY 行为 | 写成独立行为规格。应用正常启动优先使用官方 QQ 已有运行时，独立测试宿主转发面另有合同 |
| [独立 Shell/WebUI](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/live-testing/shell-webui/RESULTS.md) · [9.9.37 启动](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/docs-open/dev/FROM-P1-TO-V8.md) | 9.9.36 隔离目录、扫码、登录后总览与网络配置读操作。9.9.37 有独立启动记录 | 复用 UI 和启动行为。每版独立绑定模块哈希，9.9.36 用作首个联合验收参考 |

目标链路为官方 QQ → Rust 宿主适配与 owner loop → 有界 bridge IPC → 每账号 actor、账本和 MessageStore → 共享动作/事件层 → 协议、WebUI、CLI、SDK、MCP。使用明确的账号、宿主代次和会话代次。证据路径归能力清单管理。

## 4. GitNexus 结果

[graph] Windows 目标 checkout 的 GitNexus 1.6.12 索引绑定 `598b22b`。查询定位到 `Drain_send_responses` 和 `Insert_event`。对 `handle_action` 的影响报告为 CRITICAL，结果把同名符号与无关执行流合并。实际直接调用面通过源码定位到 `onebot_server` 装配、HTTP 单测和 daemon/HTTP 集成测试，以及 `onebot.rs` 自身测试。HTTP、WS 和未来 SSE/MCP 的行为须从共享装配入口回归。工作树上的 UID-map 增量以源码为准。

## 5. 语句与顺序约束

GitNexus 返回 `pdgLayer=no-layer`。本次为跨阶段规划，沿既有调用点核对约束，未刷新 PDG。以下为源码结论。

- [verified] `enqueue_send_with_timeout` 先要求 Ready，再登记 cookie 和 wire sequence，然后提交。回调可能早于 submit 返回。新的 wrapper/service 路线也必须在提交前登记，由 core 持久分配，且只能终结一次。[提交与响应](../../crates/caligo-bridge/src/native_session.rs#L293)
- [verified] `on_event` 对旧代次保留审计并清空业务投影。当前代次先落 journal，再落 MessageStore 和发布，失败进入可恢复路径。变更 native ID 范围时同步审查去重键、重连与历史数据恢复。[事件入账](../../crates/caligo-core/src/runtime.rs#L824) · [MessageStore](../../crates/caligo-core/src/message_store.rs#L219)
- [verified] `Daemon` 对已接受事件按持久 OneBot ID 做投影再扇出。启用 HTTP POST 顺序规则时维持其 quick operation 与事件推送顺序，所有前端共享鉴权、错误与结果语义。[daemon](../../crates/caligo-core/src/daemon.rs#L323) · [事件传输](../../crates/caligo-core/src/onebot_transport.rs)

## 6. 能力清单及实现分组

第一项交付是版本化能力清单，逐项收录真实注册的 action/alias、事件、消息段、传输、组件导出和工具能力。以参考库 `createActionMap` 的显式和装饰器注册为动作分母，`router.ts` 枚举用于找差异，排除仅用于测试的注册条目时记录依据。登记输入输出、权限、宿主/API、依赖、证据位置、支持版本、Caligo handler 和验收项。先标齐所有条目，再实施。覆盖率报告给出分母和未闭合条目，不用 README 宣传数量代替注册清单。[动作注册](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/packages/snowcat-onebot/action/index.ts) · [动作名](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/packages/snowcat-onebot/action/router.ts)

| 组 | 要接入的能力 | Caligo 落点 |
|---|---|---|
| 宿主与启动 | 固定版本识别、进程/环境捕获、owner thread/env、会话和登录、正常 attach/stop、隔离便携启动、运行时诊断。对 BootHook、winmm、napiloader、QQNT 转发、winpty、ffmpeg 代理的导出和可观察行为逐一登记；生产所需行为由 Rust/Windows 接口实现，独立兼容宿主能力走独立组件 | CLI、bridge 入口和版本 profile。官方 QQ 为实际宿主，研究替代宿主的转发合同单列 |
| 身份、消息、回执 | 好友与群身份、ProfileService 双向查询、文本收发、会话/发送者/平台时间、强回执与原生回读、消息更新/历史/撤回、消息 ID 与方向 | NativeSession/IPC、actor/journal、MessageStore。优先实现研究已验证的 QQ wrapper 消息服务合同。消费 `onAddSendMsg`、`onMsgInfoListUpdate` 和 `getMsgsByMsgId` 的同一会话与 ID，保留送达证据层级 |
| 包与媒体 | 包观察、SSO 调用与二进制转换，图片、语音、视频、文件、流式上传下载，@、回复、转发、表情、卡片及注册表其余消息段。FFmpeg 媒体转换、PCM/Silk、缩略图、资源清理 | 原生 I/O 与外部媒体工具隔离。可使用独立受审的常规媒体依赖，固定版本和许可证。上传 ID、消息结果、接收下载分别关联 |
| 动作与事件 | 标准 OneBot、go-cqhttp/NapCat 兼容和 SnowCat 扩展，好友/群/成员/资料、请求通知、权限管理、群文件、QZone、群相册、精华、签到、收藏、点赞、装扮、闪照及清单中其余动作 | core 注册表、强类型参数、按账号能力与权限分派。对暴露凭据或开放原始包的动作保留显式权限、端点访问控制与最小化响应 |
| 协议与产品 | HTTP/POST、双向 WS、HTTP SSE、MCP、SnowCat 第二协议、账号管理、WebUI、CLI/SDK、配置、诊断、实时日志、隔离登录、部署/更新、实际使用的扩展入口与终端能力 | 共享动作层及事件队列。WebUI/MCP 接账号能力模型。部署提供可回滚版本和独立凭据/目录 |

研究文档中仅命名但无可执行合同的保护绕过、隐藏、上报篡改及 Electron 缺失导出同样进入覆盖清单，标出证据缺口和依赖方。完整交付依赖这些条目获得可实现合同或经范围决定采用满足同一产品用途的接口。能力清单中的占位、模拟返回和禁用态不计入已接线。

## 7. 实施顺序与检查点

**阶段 A · 全量整合**

1. **A0 固定基线**。记录参考提交、证据和动作/事件分母，审阅并保留已有未提交的 ProfileService 工作。将当前分支的临时日志和无关改动排除在阶段提交之外。先建立能力清单与现有实现映射，冻结外部接口。
2. **A1 宿主和 IPC**。建立按模块精确哈希、模块内边界及线程/代次绑定的 adapter，接入 session/login、owner 循环、停止与错误状态。把稳定 identity、消息、多媒体、包、查询和事件能力接上类型化 IPC。旧消息/日志能重读，未知帧版本明确失败。
3. **A2 业务层**。完整实现消息、包、媒体及目录/权限/扩展动作的实际处理。按固定 ABI 和参考场景在合成宿主做编译、接口、序列化及失败回归。NativeAccepted、业务拒绝、可查询稳定 ID、对端观察各自记录。遇到未交付的实验 ABI 保持 fail-closed，并继续其他可实现能力，全部缺口纳入冻结报告。
4. **A3 应用与部署**。注册动作与事件，通过统一的协议/管理入口接入 SSE、MCP、WebUI、CLI/SDK、第二协议、多账号、配置、诊断和便携部署。按账号隔离队列、日志、资源、凭据和会话。导出整合后的候选构建摘要、能力清单、版本配置和模拟端到端结果。A1–A3 每个阶段做可恢复的独立提交，提交前只暂存阶段内文件。
5. **A4 冻结候选**。检查每条可接入能力有真实实现且从正常入口可达，同一候选构建执行默认和 research 全仓测试，列出所有未知项和限制。这里做的是完整接线检查。**A0–A4 无需启动 QQ，也无需逐项等待现场批准。**

**阶段 B · 统一逐项核实**

6. **B1 离线验收**。对同一候选运行全量合成宿主和协议矩阵，使用固定参考的脱敏报告、请求形状和 golden 数据做差分。所有注册项给出输入、预期输出/错误、覆盖证据和结果。失败归根因后按受影响范围重跑。
7. **B2 真实 QQ 集中验收**。待完整候选可用再组织现场。先对准入的 9.9.36-53644 两个不同账号各启动一次，沿一次受控会话依次覆盖登录、双向私聊/群聊、消息回读、媒体、SSO、查询与通知、经授权的管理操作及 OneBot 消费者。每项独立记录结果。同一原生链路由代表场景覆盖，具独立 ABI、权限、资源寿命或服务端效果的动作按组补样本。旧序号 unknown 保留，所有会改变 QQ 状态的样本使用新关联号并保留停止条件。9.9.37 使用独立已核对的 profile 与相应证据进入验收。
8. **B3 恢复与发布**。核实重连、core/QQ 重启、慢客户端、并发、双账号隔离、未知版本拒绝和长时运行。对新故障或参考数据不能解释的结果追加有界验证。每项能力填写 Caligo 自身结论，按证据更新进度和发布清单。

## 8. 验收矩阵

| 场景 | 通过条件 |
|---|---|
| 宿主、版本、停止 | 已知 profile 在指定 owner 执行。错误 hash/角色/线程/代次零原生调用。退出与回调交错时如实完成排空或隔离，未证寿命的代码留至 QQ 退出 |
| 收发、ID、回读 | 私聊/群聊各自核对标记、方向、账户、服务端时间、强回执、对端事件和本端 get_msg。拒绝、超时、重复、早到、迟到、重启均有可辨结果 |
| 媒体与 SSO | 代表样本可回读，上传失败、过期、坏输入、慢消费者明确返回。语音下载按固定样本校验字节，SSO 请求、命令、序列与响应可对应 |
| 动作、事件、协议 | 注册项逐条核实 schema、权限、错误和可达性。相同请求经 HTTP/WS/MCP 共享结果语义，SSE/HTTP POST/WS 事件含相同持久 ID。UI 真正读取及修改其配置并可回读 |
| 账号与可靠性 | 两账号独立目录、凭据、事件和账本。断链、重启、日志/磁盘失败时可恢复，无旧回执串代、无 unknown 重发、无虚假 ACK |

每阶段执行 `git diff --check`。候选执行 `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all`，同时记录并行回归和既有共享 fixture 问题。以测试命令的实际退出码、构建 SHA 和脱敏记录为准。[开发与验证](../guides/development.md) · [初始化](../../init.ps1)

## 9. 风险与约束

- Rust 重写会改变对象持有、回调线程和清理方式。参考实现的正例可复用为判据，Caligo 候选必须对自身的生命周期给出证据。对原生入口保留拒绝和隔离分支。
- 用户要求先整合全部功能，整合内仍按依赖推进以便回滚。现场核实集中在候选冻结后。对已确认的源合同，先实现和走离线回归即可，无须重复原研究的偏移摸索或一项一轮现场操作。
- 重复登录会干扰证据，现场使用不同账号和隔离目录。真实凭据、账号、聊天、QQ 二进制与详细日志保存在本机 `local-evidence/`。外部库启动、加载或使用研究原件仅可发生在适用的对照环境。
- 注册与网络层影响多个入口，保留已有调用语义、消息格式、持久数据与停止约束。待回归一致后才提升正式 capability。源码实现在已准入的正常路径运行，未支持项返回明确失败。

## 10. 预计变更面

A1/A2 更新 bridge 宿主、会话及 IPC、core actor、消息存储和能力注册。A3 更新 `caligod`、共享 OneBot 和网络层，新增所需的管理/协议入口及 UI/部署组件。A4/B 更新能力清单、验收记录、`feature_list.json`、`progress.md` 和 `session-handoff.md`。保持现有用户改动与旧证据可追溯。

## 11. 实施上下文

```json
{
  "implementation_context": {
    "task_summary": "固定 SnowCat 参考成果，先完成全部可接入能力的 Caligo 自有实现，再对同一候选集中逐项验证。",
    "acceptance_criteria": [
      "真实 action/alias、事件、宿主组件、协议及工具逐项登记",
      "全部可实现项由 Caligo 正常入口贯通后冻结候选",
      "在同一候选完成逐项验收并记录能力和精确版本证据"
    ],
    "evidence_provenance": {
  "schema_version": 2,
  "head_commit": "598b22b1218a67d45eeb8d5546a5a03e356bb534",
  "generated_plan_path": "docs/plans/2026-11-13-gitnexus-plan-snowcat-capability-integration.md",
  "global_dirty_digest": {
    "algorithm": "sha256",
    "canonicalization": "gitnexus-evidence-provenance-v2 NUL-framed UTF-8 records",
    "value": "0c39e4f5313b411a91480acf96de01cf64bd3a16f0ce8b270f6c89746643175d"
  },
  "cited_path_manifest": [
    {
      "path": "AGENTS.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:4c580d0539e8a594b9a1a07f85a71b79aaa57289262b88ce7f8f88801a22f838",
      "index_digest": "sha256:4c580d0539e8a594b9a1a07f85a71b79aaa57289262b88ce7f8f88801a22f838",
      "worktree_digest": "sha256:40f355287062548ca478458696d25956e352ef01cde5839fff0df6aef232ee6d",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-bridge/src/daemon_client.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:62f0573b0193ab5e1c95ca041d2c880ecb5334d88b46162e96d12cb6b67c6110",
      "index_digest": "sha256:62f0573b0193ab5e1c95ca041d2c880ecb5334d88b46162e96d12cb6b67c6110",
      "worktree_digest": "sha256:62f0573b0193ab5e1c95ca041d2c880ecb5334d88b46162e96d12cb6b67c6110",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-bridge/src/native_session.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:e46e25de462829ed5382f49cb4f9d32722c6196a39c0f4ed9ad409aaa878a974",
      "index_digest": "sha256:e46e25de462829ed5382f49cb4f9d32722c6196a39c0f4ed9ad409aaa878a974",
      "worktree_digest": "sha256:e46e25de462829ed5382f49cb4f9d32722c6196a39c0f4ed9ad409aaa878a974",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-bridge/src/qq_v8.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:9058cfdc2d13732cca49b793a28db488779fec256286d7c734e7412405c7ab80",
      "index_digest": "sha256:9058cfdc2d13732cca49b793a28db488779fec256286d7c734e7412405c7ab80",
      "worktree_digest": "sha256:b195f76816318a99a8538277af7e6b941527ba2019abdd02ecca9f8fba98c20c",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-bridge/src/uid_map_probe.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:1aab0a05c3d6a1324300f7922588df9ec0246569497299c7df2e967fedc9858e",
      "index_digest": "sha256:1aab0a05c3d6a1324300f7922588df9ec0246569497299c7df2e967fedc9858e",
      "worktree_digest": "sha256:e012f9ee8101cfc3d21b9e8db0271dac8457af5d942e19cec02d14db24491e5b",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-bridge/tests/resident_lifecycle.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:4b6b517979da2d974141adca08dfe2bfe3d3bfe6e8a1339eead0af9f11041be9",
      "index_digest": "sha256:4b6b517979da2d974141adca08dfe2bfe3d3bfe6e8a1339eead0af9f11041be9",
      "worktree_digest": "sha256:4b6b517979da2d974141adca08dfe2bfe3d3bfe6e8a1339eead0af9f11041be9",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/bin/caligod.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:79daeac7381ef1afdbcedbc222fa9ddb3a5786dd6c0bd8a1f3278004bf14dada",
      "index_digest": "sha256:79daeac7381ef1afdbcedbc222fa9ddb3a5786dd6c0bd8a1f3278004bf14dada",
      "worktree_digest": "sha256:79daeac7381ef1afdbcedbc222fa9ddb3a5786dd6c0bd8a1f3278004bf14dada",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/daemon.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:314421b0ec6d1c19aadbb6de95dd29e3df407acdec5a2fdfbfe1602cb22ef43e",
      "index_digest": "sha256:314421b0ec6d1c19aadbb6de95dd29e3df407acdec5a2fdfbfe1602cb22ef43e",
      "worktree_digest": "sha256:314421b0ec6d1c19aadbb6de95dd29e3df407acdec5a2fdfbfe1602cb22ef43e",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/message_store.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:e753ab6bfc468ee257be32966cfb24de962aa5c9e12339087f471562f2544e91",
      "index_digest": "sha256:e753ab6bfc468ee257be32966cfb24de962aa5c9e12339087f471562f2544e91",
      "worktree_digest": "sha256:e753ab6bfc468ee257be32966cfb24de962aa5c9e12339087f471562f2544e91",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/onebot.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:ec2ae99d97f6675cd7ee252f3f1d54fcb13cedc642ab02498fec8ea8f68df44d",
      "index_digest": "sha256:ec2ae99d97f6675cd7ee252f3f1d54fcb13cedc642ab02498fec8ea8f68df44d",
      "worktree_digest": "sha256:ec2ae99d97f6675cd7ee252f3f1d54fcb13cedc642ab02498fec8ea8f68df44d",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/onebot_server.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:a2eb27043389b5721d79bda667f05d6815176428ab6fded1b3771cf57899bc6f",
      "index_digest": "sha256:a2eb27043389b5721d79bda667f05d6815176428ab6fded1b3771cf57899bc6f",
      "worktree_digest": "sha256:a2eb27043389b5721d79bda667f05d6815176428ab6fded1b3771cf57899bc6f",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/onebot_transport.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:4c9734e9d8e37bb0cb3cfd8c83210714ca9db2aee40fb0b752546355677302f5",
      "index_digest": "sha256:4c9734e9d8e37bb0cb3cfd8c83210714ca9db2aee40fb0b752546355677302f5",
      "worktree_digest": "sha256:4c9734e9d8e37bb0cb3cfd8c83210714ca9db2aee40fb0b752546355677302f5",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/src/runtime.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:743045fcf3736699d2b0469673b162ed7fa2aa4fd8af64cceb236ffc2f40f23d",
      "index_digest": "sha256:743045fcf3736699d2b0469673b162ed7fa2aa4fd8af64cceb236ffc2f40f23d",
      "worktree_digest": "sha256:743045fcf3736699d2b0469673b162ed7fa2aa4fd8af64cceb236ffc2f40f23d",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/tests/daemon_chain.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:4449bf41ca299389bc56c7182cdd49580bbf47b9c353df0a6cbee8a50f1012a5",
      "index_digest": "sha256:4449bf41ca299389bc56c7182cdd49580bbf47b9c353df0a6cbee8a50f1012a5",
      "worktree_digest": "sha256:4449bf41ca299389bc56c7182cdd49580bbf47b9c353df0a6cbee8a50f1012a5",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/tests/recovery_contract.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:7c2f6c5ddaf90d2cda5ef701041b43d5a931fea325dbefe19aa2c2c5b9ac3514",
      "index_digest": "sha256:7c2f6c5ddaf90d2cda5ef701041b43d5a931fea325dbefe19aa2c2c5b9ac3514",
      "worktree_digest": "sha256:7c2f6c5ddaf90d2cda5ef701041b43d5a931fea325dbefe19aa2c2c5b9ac3514",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-core/tests/runtime_contract.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:d2a4c4c579e4cd88cc16cc533d7d53a366c29f11ae7734a3866e98931a300d54",
      "index_digest": "sha256:d2a4c4c579e4cd88cc16cc533d7d53a366c29f11ae7734a3866e98931a300d54",
      "worktree_digest": "sha256:d2a4c4c579e4cd88cc16cc533d7d53a366c29f11ae7734a3866e98931a300d54",
      "untracked_digest": "absent"
    },
    {
      "path": "crates/caligo-model/src/lib.rs",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:6a99c1043c1151b6db38a4d97072a4cd83732df1258665b7cd820bb2bf11f0be",
      "index_digest": "sha256:6a99c1043c1151b6db38a4d97072a4cd83732df1258665b7cd820bb2bf11f0be",
      "worktree_digest": "sha256:6a99c1043c1151b6db38a4d97072a4cd83732df1258665b7cd820bb2bf11f0be",
      "untracked_digest": "absent"
    },
    {
      "path": "docs/README.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:eb04b978dbb455d8ef25159e17e2c721fda52f2b3c78e50000dee5c7b229c376",
      "index_digest": "sha256:eb04b978dbb455d8ef25159e17e2c721fda52f2b3c78e50000dee5c7b229c376",
      "worktree_digest": "sha256:44ea1bde0aed139a7dc47c50faa49a58bb4805c76322796fbc6ae26a1f5b4181",
      "untracked_digest": "absent"
    },
    {
      "path": "docs/guides/architecture.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:030e574d6bd8094fd4a456e1a4bbb1d2efa4b96393dde07fa66f1b4adab43d70",
      "index_digest": "sha256:030e574d6bd8094fd4a456e1a4bbb1d2efa4b96393dde07fa66f1b4adab43d70",
      "worktree_digest": "sha256:030e574d6bd8094fd4a456e1a4bbb1d2efa4b96393dde07fa66f1b4adab43d70",
      "untracked_digest": "absent"
    },
    {
      "path": "docs/guides/development.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:9ae90404f2da046ff6332eda2f1bb6504d6674592b9713e080efa39631247d96",
      "index_digest": "sha256:9ae90404f2da046ff6332eda2f1bb6504d6674592b9713e080efa39631247d96",
      "worktree_digest": "sha256:9ae90404f2da046ff6332eda2f1bb6504d6674592b9713e080efa39631247d96",
      "untracked_digest": "absent"
    },
    {
      "path": "docs/guides/native-research.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:074e6b7a9230db78de5c61266439c4f08f64da5a43bfdfe266475dc5f75d0822",
      "index_digest": "sha256:074e6b7a9230db78de5c61266439c4f08f64da5a43bfdfe266475dc5f75d0822",
      "worktree_digest": "sha256:074e6b7a9230db78de5c61266439c4f08f64da5a43bfdfe266475dc5f75d0822",
      "untracked_digest": "absent"
    },
    {
      "path": "feature_list.json",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:99bdd414d6e11b9fdf51f427ae5b9917f87eb2a056a88bf7738c67f0ee088127",
      "index_digest": "sha256:99bdd414d6e11b9fdf51f427ae5b9917f87eb2a056a88bf7738c67f0ee088127",
      "worktree_digest": "sha256:b2ad4d4759a4cb9eb2d7562101186059f17be0f3980cf30c3049f088ae856a45",
      "untracked_digest": "absent"
    },
    {
      "path": "init.ps1",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "clean",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:b3e3c765552b438b2cd25b38d39944b444017a988b2b03afcf0f506db63545ee",
      "index_digest": "sha256:b3e3c765552b438b2cd25b38d39944b444017a988b2b03afcf0f506db63545ee",
      "worktree_digest": "sha256:b3e3c765552b438b2cd25b38d39944b444017a988b2b03afcf0f506db63545ee",
      "untracked_digest": "absent"
    },
    {
      "path": "progress.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:5fd60909ceee7ffefd59e466306c734790dee1f4578221588b6e547aee2ac2b1",
      "index_digest": "sha256:5fd60909ceee7ffefd59e466306c734790dee1f4578221588b6e547aee2ac2b1",
      "worktree_digest": "sha256:7c57098c26418b647f84f8207307140c5fef2f9ee9a9d28bea7d1e5f4a95570d",
      "untracked_digest": "absent"
    },
    {
      "path": "session-handoff.md",
      "object_kind": {
        "head": "regular",
        "index": "regular",
        "worktree": "regular",
        "untracked": "absent"
      },
      "state": "unstaged",
      "rename_from": null,
      "rename_to": null,
      "head_digest": "sha256:0b83a0e0cf39a14359135f9fef163870302d00d502fd9a707a6f0ca571ddec79",
      "index_digest": "sha256:0b83a0e0cf39a14359135f9fef163870302d00d502fd9a707a6f0ca571ddec79",
      "worktree_digest": "sha256:a193c7e4cb677a9ce05f618245561c7fbae118b7851d96a982d1edcc592b7014",
      "untracked_digest": "absent"
    }
  ]
},
    "primary_symbols": [
      {
        "symbol": "enqueue_send_with_timeout",
        "file": "crates/caligo-bridge/src/native_session.rs",
        "lines": "293-359",
        "role": "登记先于原生提交"
      },
      {
        "symbol": "on_event",
        "file": "crates/caligo-core/src/runtime.rs",
        "lines": "824-921",
        "role": "代次、journal、投影和幂等"
      },
      {
        "symbol": "handle_action",
        "file": "crates/caligo-core/src/onebot.rs",
        "lines": "217-280",
        "role": "共享动作分派"
      }
    ],
    "related_symbols": [
      {
        "symbol": "publish_onebot_message",
        "relationship": "event projection",
        "relevance": "发布前解析持久 OneBot ID"
      },
      {
        "symbol": "run_profile",
        "relationship": "research baseline",
        "relevance": "保留已有未提交的 ProfileService 工作"
      }
    ],
    "execution_path": [
      "精确构建和账号准入",
      "所有权受控的 bridge 服务与 IPC",
      "持久 actor 及消息投影",
      "共享动作与事件",
      "协议和产品入口"
    ],
    "pdg_constraints": [],
    "architectural_patterns": [
      {
        "pattern": "owner pump + 代次绑定",
        "example_location": "crates/caligo-bridge/src/uid_map_probe.rs",
        "usage_guidance": "借用对象仅在宿主所有者上下文使用，跨线程传输 owned 数据"
      }
    ],
    "files_to_modify": [
      {
        "file": "crates/caligo-bridge/src/native_session.rs",
        "symbols": [
          "enqueue_send_with_timeout",
          "on_response"
        ],
        "intended_change": "共享稳定身份、派发与回执合同"
      },
      {
        "file": "crates/caligo-bridge/src/daemon_client.rs",
        "symbols": [],
        "intended_change": "接入可靠的接收投影和能力 IPC"
      },
      {
        "file": "crates/caligo-core/src/onebot.rs",
        "symbols": [
          "handle_action",
          "is_supported_action"
        ],
        "intended_change": "按固定注册清单实现实际动作"
      },
      {
        "file": "crates/caligo-core/src/bin/caligod.rs",
        "symbols": [],
        "intended_change": "装配协议与账号管理入口"
      }
    ],
    "tests": [
      {
        "file": "crates/caligo-bridge/tests/resident_lifecycle.rs",
        "scenarios": [
          "同步回调、迟到或未确认、关闭与排空"
        ]
      },
      {
        "file": "crates/caligo-core/tests/daemon_chain.rs",
        "scenarios": [
          "端到端正常动作、持久 ID 和消息查询"
        ]
      },
      {
        "file": "crates/caligo-core/tests/recovery_contract.rs",
        "scenarios": [
          "重启、无重发、旧数据恢复"
        ]
      },
      {
        "file": "crates/caligo-core/tests/runtime_contract.rs",
        "scenarios": [
          "代次、幂等和队列容量"
        ]
      }
    ],
    "verification_commands": [
      ".\\init.ps1 -Mode docs",
      ".\\init.ps1 -Mode all",
      "git diff --check"
    ],
    "risks": [
      "原生生命周期与 ID 关联",
      "跨版本证据误用",
      "扩展动作权限与账号隔离"
    ],
    "assumptions": [
      "以固定版本行为等价为覆盖口径",
      "以 9.9.36-53644 作首个联合验收参考"
    ],
    "open_questions": [
      "未闭合原生合同与开发宿主缺失导出",
      "Caligo 自有实现的真实停止和强身份覆盖"
    ],
    "avoid": [
      "直接使用或转写逆向 C/C++",
      "使用归档二进制或第三方生产后端",
      "在候选整合前逐项进行真实 QQ 验证",
      "把占位、unknown 或参考实现的通过计为 Caligo 通过"
    ]
  }
}
```

## 12. 假设与未决项

- [assumed] “全部功能”覆盖固定参考提交中可枚举的运行能力、兼容接口、原生组件和工具行为，按功能等价验收。测试用 handler 及历史失败路线在清单中注明用途。具体原件 ABI 的差分要求由清单逐条说明。
- [verified] 9.9.36 有最完整的双账号、原生及 Shell 现场基线。9.9.37 启动与配置有独立记录。原生 addon 的每一项、Electron 转发缺失部分、所有扩展权限场景和长时运行的参考证据仍有边界，详见 §3。
- [inferred] 建议优先用已验证的 QQ wrapper 消息服务形成稳定身份链，原有 MSF/CPB 账本与捕获层按可证能力保留；具体选路在 A1 以现有合同和完整候选的兼容性决定。
- 本轮未执行代码、构建或 QQ 现场操作。实施从 A0 开始，QQ 测试实例和账号到 B2 再指定。

## 13. 完成定义

固定参考分母上的每项能力均有实现位置、正常入口、权限和适用版本，并附逐项 Caligo 验收结果。完整候选的默认与 research 构建通过，真实 QQ 双账号主链、原生寿命和恢复矩阵满足对应完成门。任何证据不足、无法等价或等待范围决定的项都明确留在覆盖缺口中。缺口关闭前不得声明全量完成或生产就绪。