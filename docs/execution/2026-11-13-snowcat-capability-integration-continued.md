# SnowCat 后续整合执行记录

承接[首批执行记录](2026-11-13-snowcat-capability-integration.md)，计划为[固定参考整合计划](../plans/2026-11-13-gitnexus-plan-snowcat-capability-integration.md)。用户确认继续全部剩余计划。分支 `codex/snowcat-capability-integration`，本批起点 `67664d38909fc7d4a8b83215c346675e92808e2b`，继续更新 [Draft PR #5](https://github.com/Enoa-devteam/Caligo/pull/5)。

## 范围与证据边界

本批推进 D9 请求关联/精确回读、只读能力 IPC、真实目录动作、消息持久修订、双账号管理与正常进程、管理 WebUI 和受控外部媒体工具。仍先离线整合，A4 冻结前不启动 QQ。固定参考始终为 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64`；只读行为合同与 TypeScript 类型，不执行参考代码、不引入反编译 C/C++ 或研究二进制依赖。

GitNexus 开始绑定此 checkout/index `67664d3`。部分符号结果异常/未命中按 UNKNOWN 处理并经源码补证，随后强制 index-only 刷新。`handle_action` 上游为 CRITICAL（27 symbols / 14 direct / 8 flows），共享协议回归必须覆盖；没有把无 callers 解释为零影响。

## 已运行的定向验证

- 共享动作与查询：default `onebot::` unit 12/12；`onebot_query_lab` 6/6；MCP 6/6 PASS。正常 action handler 经真实 Daemon 管道读取 typed QueryData，测试未把失败转换为空列表。
- Query IPC/model：model unit 16 + wire golden 6 PASS。broker default/research unit 各 3/3，真实 query pipe default/research 各 7/7 PASS。核心 query 范围按 account/host nonce/session generation/connection epoch 绑定；先登记后派发，有界容量和期限，不自动重试。
- 消息修订：default/research Store 各 6/6、runtime 各 11/11、recovery 各 10/10 PASS；research wrapper chain 1/1。default wrapper chain 为 feature gate 的 0 tests，不能记为场景通过。
- 账号管理：default/research 各 5/5 PASS，管理 token 新断言 1/1 PASS。真实双 Daemon 目录、账本、凭据和管道隔离、单账号停止、配置原子读写和失败回滚有覆盖。最初管理 binary smoke default/research 各 1/1 PASS；后续 WebUI 动态装配仍待最终复验。
- 媒体工具：default `media_tool_lab` 6 PASS / 1 ignored helper；真实自有 child 验证 kill/wait 和 stdout/stderr 限制；`caligo-media` 构建 PASS，相对配置路径拒绝 exit 2。尚无真实 FFmpeg 转码或 QQ 上传/语音/Silk 验收，不将 CLI 通过计为媒体全链通过。

## 实现与兼容合同

消息 identity 保持 account/chat kind/peer/native ID，更新以业务快照指纹去重，排除本地事件序号和观察时间；同 ID 修订不重复产生新 message event。MessageStore 初始记录维持 v1，v2 仅追加同 identity/同本地 ID 修订；旧记录继续可读。严格 wrapper 记录可生成持久 get_msg 条目，`real_id=本地持久 OneBot ID` 是兼容语义，字体 14 是兼容默认，均不冒充原生字段。subtype 来源未闭合时不发布业务消息事件。

固定 6 项查询为 UIN→UID、UID→UIN、精确消息回读、好友列表、群列表和群详情；登录资料未声明。只读查询与发送能力分别协商，不从 Profile 查询存在推定私聊发送可用。QQ wrapper 的服务调用在 owner 线程上执行；core 等待不持 actor 锁。D9 保留 core request_id/wire_seq/剩余期限，Promise 只提供受理/拒绝证据，稳定 add/update 身份与精确回读另行核对。没有把本地 correlation cookie 当作 native ID。

## 中间失败保留

联合编译曾遇到 D9 旧变量名、修订指纹 SessionKind 序列化和 resident 查询队列元组/期限改造的中间态错误，修复后才运行相关定向测试。管理 HTTP 首跑 5/7：Windows 只读句柄权限夹具和未读请求的 TCP reset 使两个测试失败，修后结果待最终记录。固定查询执行器审阅发现 UID/UIN 混用、数值 ID 字符串洗白、owned 拷贝静默截断和立即失败 pending 泄漏，修复与回归未结束前不计已验证。

## 收口定向结果与浏览器验收

D9 bridge source 冻结前通过 research check、library 91/91、resident 20/20、wrapper send 1/1、query contract 2/2。固定发送测试限定 research（生产 V8 固定脚本属于该 feature）；普通构建不把零用例计为发送验证。

发送能力协商与查询能力分开保存，当前 scope 的私聊发送声明必须明确为 true；错 scope、断链和重连清零。正常私聊入口保留 UIN 作为持久 target，owner 解析 native UID。`onebot_private_send_lab` 2/2 PASS：未声明/错声明拒绝、真实 pipe 派发 UIN+强回执+wrapper 持久 ID 查询，以及 unknown 后不二次派发。`capability_query_lab` 扩展 8/8，daemon chain 5/5、broker 3/3 与新增 wire 1/1 PASS。

管理 HTTP 修正后 default/research 各 7/7 PASS。管理 binary default/research 各 3/3 PASS：配置 readback、启停与 restart 后实际协议 listener、鉴权 shutdown、协议端口冲突在 10 秒内退出。此前 metadata EOF 来自 smoke 夹具绕过 `--set-config` 私有 ACL 初始化，正常入口初始化后通过；启动失败清理已修为先停止 daemon 再 join listener，避免锁内重入和等待死锁。

浏览器使用本机隔离 manager（两个合成账号，没有 QQ attach），实际完成：连接读取配置、停止 A 后 B 仍运行、全部停止后修改配置并点击保存回读、磁盘 `accounts.json` 精确包含新配置、重新启动 A、鉴权关闭管理器 exit 0。界面始终显示 QQ 状态未知。该证据是本地 core/UI 行为，非 QQ 登录或消息验收。原始截图/页面数据和配置仅保留 ignored `local-evidence/`。此浏览器进程曾占用 `target-harness/debug/caligo-manager.exe` 导致一次构建替换失败，已通过 UI 正常退出后恢复构建。

媒体模块补版本验证时误覆盖全文件已恢复完整实现，并在隔离 target 下重新通过 6 项 LAB（1 个 ignored child helper）及 CLI build。异常记录不由原先通过覆盖。版本声明通过 `-version` 输出核对；子进程使用独立 cwd 与 file/pipe 协议白名单、无 stdin，try_wait 错误清理 kill/wait/join 保留；该罕见 I/O 错误尚无独立注入测试。

一次 rustfmt 递归扩散到 20 个无关 bridge 文件，主执行者将 HEAD 内容重放相同 rustfmt 后逐字节比较，确认完全相同才恢复，未恢复本批真实修改或用户文件。

## 本批完整验证

`RUST_TEST_THREADS=1 ./init.sh -Mode all` exit 0，default/research 全仓 PASS；原始输出为 ignored `local-evidence/snowcat-next-all-3.log`。随后并行 research workspace `cargo test --workspace --features caligo-core/research,caligo-cli/research --locked --offline --target-dir target-harness` exit 0（`local-evidence/snowcat-next-parallel-research.log`）。此前 all-1 为取消标志中间态编译失败，all-2 为管理 smoke ACL 夹具错误，均保留；修后才记录最终通过。

提交前完整 GitNexus detect 为 critical（74 changed symbols / 20 affected flows / 21 tracked changed files），无 partial/truncated 结果；其不覆盖尚未跟踪的新文件的限制由显式 stage 后再次 detect 补足。共享注册/派发仍按 critical 风险处理。`git diff --check`、文档和覆盖映射收尾结果随阶段提交更新。当前不声明完整计划完成、候选已冻结或产品就绪。

## 恢复检查点：bridge 声明与覆盖清单

在 `8e4583b` 和现有工作树上恢复。经 WSL 使用计划 provenance 工具读取并核对计划 SHA-256 `ce9aedc51ed469cd9fd6efa8fce9528608628ffce87e8053af8c0826e7cf8482`。基线和 dirty digest 均已变化，19 条引用路径重新定位。固定参考 checkout 仍为 `7cf025f`。现有工作树的纯换行差异、日志与本机缓存未纳入阶段文件。

bridge 每次接受 HelloAck 后用同一 QueryScope 清理旧查询并发布 adapter 的查询/发送声明。默认 adapter 声明为空，QQ owner adapter 声明六项固定只读查询和两种文本发送能力。具体操作仍须通过 owner 和宿主校验。业务协议 revision 3 使旧桥接在握手阶段明确被拒绝。

- 初次覆盖测试失败 4 项，原因包括 13-key 映射未刷新、证据锚点漂移、评审 fixture 与 refresh 合同不一致。修正后用真实 Rust exporter 重生成 5,711 项映射并验证 12/12。固定参考重生成 6/6。保留显式评审，自动重算派生行，媒体映射限于已实现的 PCM 解码重叠部分。
- 串行 `init.ps1 -Mode all` 在开始工作时通过。调整声明后，research bridge 声明 1/1、default query pipe 8/8、私聊发送 2/2、IPC 14/14 通过。
- GitNexus 1.6.12 带 PDG 刷新。Windows 全文索引构建和修复均在 `Union.union_fts` 失败，随后用 `--skip-fts` 重建符号、调用和 PDG 索引。能力声明影响报告为 CRITICAL（2 个直接调用方，5 条执行流），包含正常 bridge 会话循环。

## A1/A2 普通私聊与群聊的事件接线

固定参考 `7cf025f` 的 `packages/snowcat-core/types/msg.ts:201–215` 和 `packages/snowcat-onebot/api/msg.ts:1097–1165` 将 C2C（1）映射为 `friend`，普通群（2）映射为 `normal`，字体字段用兼容默认 14。临时群聊另有独立类型。Caligo 从当前账号、宿主 nonce 和会话代次绑定的 wrapper 记录取消息字段，journal 和 MessageStore 写入成功后才向共用事件队列发布，`message_id` 与 `get_msg` 一致。仅发布普通私聊和群聊的首次入站文本。自发消息、历史回填、修订和旧代次仅保留各自已有处理路径。

实时和恢复使用相同的账号/会话/peer/native ID 去重键，同 native ID 的跨会话消息分别入库。重复旧修订不会覆盖当前版本。新增旧代次日志记录写入可选 `audit_only` 并在恢复时跳过消息索引和去重登记。历史日志缺少该字段时按原语义读取，无法从旧记录可靠回判其入账时的代次。

新增真实管道回归先复现订阅超时，接线后通过，覆盖同一持久 ID、双订阅者、普通私聊/群聊、同 ID 跨会话、更新和重复抑制、错宿主拒绝、旧代次审计及恢复。影响检查对 journal/recovery 路径报 CRITICAL，完整兼容性验证以正常入口和工作区回归为准。

本阶段 research 定向 pipe 5/5、SSE 8/8、recovery 10/10、runtime 11/11、wrapper chain 1/1。串行 `init.ps1 -Mode all` 退出 0，覆盖默认和 research 全仓。真实 registry 覆盖检查 12/12，`init.ps1 -Mode docs` 与 `git diff --check` 通过。输出保留在 ignored `local-evidence/snowcat-event-all.log` 等本机证据中。

登录仍缺已登录 QQ 实例的当前账号来源。固定参考 Framework/Shell 从扫码成功回调获得 UIN/UID，但回调留下空昵称。双账号研究从 `getLoginList` 按已知昵称选账号，并有额外宿主及样本约束。现有 Caligo wrapper session ID、配置账号、历史登录列表或目录查询均不足以绑定当前已登录身份，`LoginInfo` 继续不声明。

## A2 私聊语音精确回读

固定参考 `7cf025f` 的 `research/native-reconstruction/live-testing/business-message/probe.cjs` 以私聊入站记录的消息 ID、元素 ID 和 peer UID 标识语音，`packages/snowcat-core/types/element.ts` 声明 PTT 的大小和 MD5。Caligo 通过现有 scoped 查询协议增加 `VoiceInfo`。只从精确消息回读接受单一匹配的入站私聊语音元素，保留字符串 ID，仅返回身份、大小和 MD5。媒体路径及凭据不进 IPC。查询上限 16 MiB 表示元素声明的文件大小。

旧 adapter 的六项查询能力仍可独立声明。语音查询仅由 research owner adapter 在所加载 wrapper.node、QQNT.dll 和 major.node 路径的文件 SHA-256 与 9.9.36-53644 pilot 三角色完全匹配时启用。目录查询和既有文本 ABI 保持原作用域。合成脚本覆盖精确大整数 ID、错 peer/消息/元素、自发、重复元素、错误大小和 MD5，以及私有字段不出 IPC。此检查点是下载前的身份与元数据查询，当前没有语音文件下载或 `get_record` 验收。

SSO 参考样本中 42 字节请求与 919 字节响应的二进制转换依赖 `napi2native` 实验钩子。该证据给出 Caligo 后续自有二进制接线的对照，当前未提供 SSO backend 或 `send_packet`。

首轮全仓编译发现 `QqOwnerAdapter::new` 的公开签名被改动，导致两个既有 `qq_entry_lab` 调用失败。修正为在原构造器内部检查宿主模块，保留原签名。定向语音 readback 与精确 profile 拒绝测试均通过。最终串行 `init.ps1 -Mode all` 退出 0，默认和 research 全仓均通过，日志位于本机 `local-evidence/snowcat-voice-info-all-final.log`。

## A2 私聊语音有界下载

基于 `0affb67` 的精确回读，在相同宿主 profile 增加 `Daemon::query_host(HostQuery::VoiceDownload { ... })`。查询先重新核对入站私聊的 peer、消息和语音元素，在原 D8 listener 上登记完成处理，再调用固定 `downloadRichMedia`。回调必须匹配消息 ID、元素 ID、chat type 和本次独立临时目标。完成后打开并核对文件身份、长度、MD5 和 Silk 头，返回最多 128 KiB 的 owned 字节（IPC 内用 hex 编码）。目标路径由 bridge 生成，路径和 QQ 媒体凭据不通过查询返回。

每个 owner 会话只允许一笔 native 语音下载在途。取消或超时后，等待精确完成回调回收临时目录；未收到回调时保留该槽与目标，不再次派发，也不声称 native 已停止。后续需现场核对回调路径是否与请求路径一致，以及 Windows 上的真实停止和临时资源回收。本阶段仅覆盖固定 profile 下的私聊入站 Silk 和正常 Rust 宿主查询入口。公开 `get_record`、其他媒体类型、上传、SSO 和真实 QQ 验收仍未完成。

合成 owner 与命名管道回归分别覆盖同步完成、错元素/路径、损坏数据、取消、迟到、停止后禁止派发、当前能力声明及精确响应。测试夹具早期曾因 Windows 命令行长度上限无法启动 Node，改用标准输入运行同一段脚本。最终串行 `init.ps1 -Mode all` 退出 0，默认和 research 全仓均通过。真实 Rust registry 覆盖检查 12/12，映射只把 `NodeIKernelMsgService.downloadRichMedia` 标为离线部分支持。

## A2 语音元素身份持久化

固定 profile 的 D8 listener 接收单个入站 C2C 语音元素。Rust 二次验证来源、账号和 peer，仅保存消息/元素身份、大小和 MD5。原始媒体路径与文件内容不进入事件。MessageStore 使用与文本相同的持久 ID 分配，`get_msg` 返回 `record` 段及账号限定的本地文件标识。旧文本日志无需迁移。合成 D8 → Resident → IPC → journal → 重启回归通过，另验证错 peer 拒绝及既有私聊发送回归。此阶段公开事件仍按文本条件发布。

## A2 OneBot 语音读取

承接 `e1b4420`。普通私聊的单个语音元素在入账后向共享 OneBot 事件队列发布 `record` 段，沿用 `get_msg` 的同一文件标识。共享动作注册 `get_record`，接受 `file` 或 `file_id` 中的一个。文件标识在当前账号持久消息库内解析，再由 `VoiceDownload` 按当前 bridge scope 精确回读和下载。返回结果核对原入站元素的身份、大小和 MD5，响应给出原始 Silk 的 `base64`、`file_size` 和 `file_name`。本机绝对路径和 QQ 文件凭据不暴露。默认或明确 `silk` 可用；其余输出格式明确返回未支持，固定参考的转码依赖独立媒体后端。

最终串行 `init.ps1 -Mode all` 退出 0，默认和 research 全仓均通过。正常 handler 加真实 pipe 的合成回归核对了事件、`get_msg`、`get_record` 和重启后的同一标识，并覆盖异账号、未声明能力、MD5 不匹配和不支持的格式。初次 core 单测发现注册表 schema 断言缺新增类型，补上后通过。真实 Rust registry 的覆盖检查 12/12，共 14 个动作键。此结果仅为离线验收。

## A2 独立 SILK 转码

媒体工具通过可选的本地 `silk_decoder` 解码到 24 kHz 单声道 s16le PCM，再调用已有 FFmpeg 容器转换。独立子进程使用固定参数、版本、许可证及 SHA-256 校验、超时与有界输出。旧配置仍可运行原媒体操作。首轮真实 CLI 运行发现 Windows debug 的 1 MiB 栈上哈希缓冲溢出，移到堆后通过。

离线实测从 [rust-silk 固定提交](https://github.com/wangnov/rust-silk/tree/7d4fd9eb265ac44b6d76bda4002d327651a022c0) 编译解码器（SDK 1.0.9.6），结合 [FFmpeg 8.1.2 essentials](https://www.gyan.dev/ffmpeg/builds/) 处理 24 kHz、440 Hz 的 200 ms 合成腾讯前缀样本。SILK 样本 SHA-256 为 `97ca76deaff11794d723439df80cf759add9c3c4a927d65dea75a0b67030efc7`。真实 `caligo-media` 两种命令退出 0，ffprobe 回读 WAV 为 24 kHz PCM/单声道/200 ms，Ogg 为 Opus/单声道，含预跳过的容器时长 206.5 ms，任务目录清空。工具与样本留在本机。定向 `media_tool_lab` 7 项通过，子进程夹具 1 项按设计忽略。

## A2 OneBot 语音转码

承接 `099db2a`。共享 `get_record` 根据当前会话的有界下载结果核对持久化语音元数据，然后按配置返回 WAV 或 OGG。省略 `out_format` 继续返回原始 Silk。转码走显式配置的独立工具，两个子进程执行前重新核对哈希，停止信号与账号 daemon 共用。PCM 和输出均最多 4 MiB。单账号从 `--media-config` 加载，多账号从各账号私有目录中的 `voice-media.json` 加载，并固定各自的临时目录。

定向合成 wrapper → IPC → daemon → OneBot 回归覆盖 WAV、OGG、原始 Silk、错指纹、工具被替换、错误码和任务目录清理。真实 `caligod` 通过 OneBot HTTP、合成命名管道宿主和固定解码工具返回原始 682 字节 Silk、9,678 字节 WAV 和 924 字节 Ogg，控制管道正常停止，临时目录清空。真实 `caligo-manager` 双账号启动验证已配置账号使用独立私有工作目录，管理 API 正常停止两账号。串行 `init.ps1 -Mode all` 通过默认和 research 全仓，真实 Rust registry 覆盖检查 12/12。现场尚待候选冻结。下一工作块核对语音上传的关联合同，随后推进其他媒体和 SSO。

## A2 私聊语音发送宿主合同

固定参考 `7cf025f` 的 `live-testing/business-message/probe.cjs` 和 `packages/snowcat-core/apis/file.ts` 表明，`getRichMediaFilePathForGuild` 返回宿主缓存路径。私聊语音通过 wrapper 的 `sendMsg` 提交，稳定消息 ID 由发送事件和 `getMsgsByMsgId` 回读提供。参考样本还在接收端匹配消息指纹并核对下载 SHA-256。见[固定参考的双账号验收](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/live-testing/business-message/RESULTS.md)。

新增 research 内部脚本构造器，输入限于有界的腾讯前缀 SILK v3 短样本。桥在绑定账号上解析目标 UIN，独占写入宿主分配的缓存路径。若缓存已存在，仅在文件身份和 SHA-256 一致时复用。发送前登记请求，Promise 受理、同一消息与元素的发送事件、终态更新和精确回读均匹配时才产出 native ID。提交抛异常或结果冲突保留 unknown。宿主缓存文件交由宿主生命周期管理。该检查点的合成宿主用例覆盖错误账号、损坏帧、缓存冲突、错指纹、错元素、歧义、超时和停止。语音发送仍待接入 scoped IPC、持久请求和 OneBot。

## A2 私聊语音发送 IPC 与账本

`79a53bd` 的宿主合同接入 `Daemon::submit_private_voice` 和正常 bridge owner drain。私聊语音独立声明发送能力，core 与 bridge 分别核对当前账号、连接和会话代次。旧能力声明缺少语音字段时继续按禁用处理。core 先持久化请求身份、指纹和派发序号，再经有界 IPC 传递音频。音频字节只用于内存队列和宿主缓存。过期、断链后的未派发旧代次以及不支持的 adapter 不触发 native 提交，未知结果保留原请求，重连后不重放。此次还修正了精确语音 profile 宣告八个查询能力时 core 上限为七导致整组拒绝的问题。

离线回归用真实命名管道覆盖能力缺失、错代次、类型与内容、unknown 和重连。resident 用例覆盖 owner 前二次检查、过期和重复请求。首轮测试把二进制 journal 当 UTF-8 文本读，已更正。最终串行 `init.ps1 -Mode all` 通过默认和 research 全仓。下一步把确认的语音消息投影持久化，再接入 OneBot `record` 发送。

## A2 OneBot 私聊语音发送

`60d703c` 的 scoped 派发已接到共享 `send_private_msg` 和私聊 `send_msg`。单个 `record` 段的 `data.file` 接受 `base64://` 编码的腾讯 SILK v3，限 128 KiB 和 50 个封装包。配置路径、URL、群语音和混合段不在本轮合同中。宿主受理、带准确元素身份的发送事件和精确消息回读成立后，将回读的脱敏语音记录通过现有事件通道入账。OneBot 同时等到强回执和持久消息 ID 才返回成功，`get_msg` 可读取发出的语音段。已发送语音的 `get_record` 暂未支持。QQ 现场、任意音源编码、缓存寿命及更广泛的媒体能力仍待验证。

串行 `init.ps1 -Mode all` 通过。真实命名管道的 OneBot 回归验证仅收到强回执时请求继续等待，持久化后同一消息 ID 可回读，重开消息库仍可查询。合成宿主验证只有完整匹配才产出脱敏语音投影。首轮该断言发现投影放进了文本结算分支，移到 D8 共享 listener 后通过。真实 Rust registry 覆盖检查 12/12。

## A2 群语音接收与已发送语音读取

固定参考 `7cf025f` 的 `packages/snowcat-core/apis/file.ts` 将会话类型、peer、消息 ID 和元素 ID 传给同一 `downloadRichMedia` 方法。Caligo 在同一精确模块 profile 接入群语音入账与 OneBot 群消息事件，并由 `ExactVoiceDownload` 读取群入站和已发送私聊语音。新请求从持久记录获取发送者 UIN 与会话类型，宿主回读须匹配发送者、会话、消息和元素。已发送语音须处于成功状态。完成回调仍绑定独立文件路径和会话类型，保留大小与 MD5 校验。已有私聊入站语音继续使用原 IPC 类型。

固定参考现场矩阵覆盖的是群文本和私聊语音。群语音和已发送语音的新下载路径使用合成宿主与真实命名管道验证，实际 QQ 回调和资源寿命待 B2 核对。

## A2 OneBot 群语音发送

固定参考 `7cf025f` 的 `action/msg/SendMsg.ts` 将 `group_id` 映射为普通群会话，`api/file.ts` 构造 PTT 元素，`snowcat-core/apis/msg.ts` 用同一 `sendMsg` 提交并关联终态更新。Caligo 沿用私聊语音的内容指纹、宿主缓存校验、持久请求和精确回读。群语音独立声明能力，使用 `DispatchGroupVoice`，目标为十进制群号，跳过私聊 UID 查询。群发送成功需同群的终态记录入库；已发送群语音沿用精确下载。

离线夹具覆盖群与私聊能力独立、跨会话同 native ID、错误会话回调、精确投影及旧文本路径。串行 `init.ps1 -Mode all` 通过，命名管道动作回归 4/4，wrapper 发送和查询回归 7/7，真实 Rust registry 覆盖检查 12/12。实际群语音投递、下载和宿主缓存寿命仍待 QQ 现场验收。


## A2 普通 PNG 图片精确回读与下载

本块从 HEAD `ca07bd9` 继续 A2，沿用固定参考 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64`。只读参考 `packages/snowcat-core/types/element.ts` 的 `PicElement`、`types/msg.ts` 的 PIC/PNG 枚举以及 `apis/file.ts:251–285` 的下载调用合同，没有执行参考代码。Caligo 增加 `ImageInfo` / `ImageDownload`；正常 `Daemon::query_host` → scoped IPC → resident owner queue → `QqOwnerAdapter` 可达，仅在既有三角色精确 hash 的 9.9.36-53644 pilot 声明。

只接受普通私聊或群聊的单个 PNG 元素（`picType=1001`、`picSubType=0`）。精确匹配 chat type、peer、sender UIN、消息与元素 ID；群 peer UIN 与会话一致，私聊入站 sender UID 与 peer 一致，自发消息须 sendStatus=2。大小接受参考的整数或十进制字符串，元数据上限 16 MiB；尺寸各不超过 32768，像素积不超过一亿。返回 owned 元数据，排除 sourcePath/filePath、下载 URL 和媒体凭据。

下载回读后使用自有唯一临时目标，与语音共用现有 `voiceDownload` 在途槽及 D8 回调转发，先登记再派发 `downloadRichMedia`。回调须匹配消息、元素、chat type 与目标路径；打开文件后比对类型、真实路径、文件身份、长度和 MD5，另校验 PNG signature/IHDR 与声明尺寸，最多返回 128 KiB 的 hex owned bytes。此校验不等于完整 PNG 解码或所有格式支持。Promise 受理不构成下载完成；取消、超时、提交结果 unknown 及 stop 后保留槽和目标，直到精确回调才清理。不主动取消未知 native 工作，不自动重试。

GitNexus 1.6.12 绑定 Windows checkout `E:/stella/Caligo` / `ca07bd9`，初始索引内容 current。Linux runner 无法读取 Windows-owned 索引，改用本机 Windows runner；随后以 `analyze --index-only --skip-fts --pdg` 更新该 checkout 的原索引，没有用 Linux runner 重建 Windows-owned 存储。FTS unavailable 的概念查询为空，按 UNKNOWN 处理，改用精确符号和源码补证。`HostQuery::validate` 上游 HIGH（10 symbols / 4 direct / 4 flows）；`build_query_start_js`、`QueryData::validate_for`、`supported_capabilities` 和 coverage `initialProfiles` 为 LOW。同名 kind/capability/validate 初查有歧义，已用文件/UID 消歧。

实际验证（起点 HEAD `ca07bd9`，Windows MSVC，`--locked --offline --target-dir target-harness`）：

- research `wrapper_image_contract` 4/4（含两个已有 query/profile 单测）；原 `wrapper_query_contract` 5/5。图片覆盖普通 PNG 元数据、精确大整数 ID、错会话/发送者/元素、重复/混合元素、大小/尺寸/格式拒绝、路径凭据隔离，以及同步回调、错 callback、超时、迟到、提交 unknown、文件长度/MD5/IHDR 不符和 stop 后目标清理。
- research `capability_query_lab` 最终 11/11，覆盖真实 Windows 命名管道的图片能力声明、查询序列化、错 sender/IHDR/预算拒绝和正确元数据/owned bytes 返回。首轮为 10 PASS / 1 FAIL：新图片场景误用只接受 FriendList 的测试辅助函数；改为检查真实 ImageInfo 请求后通过。原失败日志保留 `local-evidence/snowcat-image-pipe.log`，最终结果为 `snowcat-image-pipe-final.log`；随后更换为 CRC 正确的自造 1×1 PNG fixture，最终全仓再次覆盖。
- `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all` 退出 0，default/research 全仓通过。独立默认并发 research 全仓 `cargo test --workspace --features caligo-core/research,caligo-cli/research --locked --offline --target-dir target-harness` 退出 0。日志分别为本机 ignored `local-evidence/snowcat-image-all.log` 和 `snowcat-image-parallel.log`。
- `node scripts/check-snowcat-coverage.mjs --refresh` 用真实 registry 重生成映射；`$env:COVERAGE_RUN_RUST='1'; node --test scripts/check-snowcat-coverage.test.mjs` 12/12，无 skipped。分母保持 5,711，OneBot 图片段/上传不计已接线。
- `init.ps1 -Mode docs` PASS（15 live docs / 258 local links / 35 historical entries / 10 features）。Windows 原生 `git diff --check` PASS；Linux Git 对仓库既有 CRLF 报尾随空白，未为此批量改动原文件。已审阅本块 source/coverage/state 变更，原有改动和本机证据不纳入本块提交。
- GitNexus 变更检查报告 CRITICAL；初次 all 为 9 files / 40 symbols / 265 flows，混入 Markdown section 对多条流程的关联，未据此降低风险。提交前另外对包含新测试的 staged/all 执行结构化完整检查，原始报告保留本机 `snowcat-image-detect-*.json`。索引另报告部分 PDG 控制依赖跳过和执行流召回限制，图中无路径不代表无调用。

OneBot 图片身份持久化、image 事件/get_image、上传、其他图片格式、视频/文件/SSO 和 B2 QQ 验收仍未完成。feat-008 保持 in-progress；完整候选未冻结，产品 NOT READY。


## A2 OneBot PNG 图片读取

承接 `c6c8792`，继续先完成整合、候选就绪后再集中验收。固定参考 `7cf025f` 的 `action/file/GetImage.ts` 继承 `GetFileBase`，`GetFile.ts` 给出 file/file_id 输入和可选 file/url/file_size/file_name/base64 输出合同。Caligo 的普通入站单 PNG 已由固定 profile 的 D8 进入 typed wrapper、resident、journal 和 MessageStore，使用账号限定的 `caligo-image:<account>:<OneBot ID>` 文件标识。共享私聊/群聊事件与 get_msg 使用同一持久 ID 和 image 段。图片身份包含 sender/chat/peer/native message/element/MD5/尺寸，图片与文本、语音互斥；旧日志缺少 image 字段时仍可恢复。

`get_image` 在共用动作 registry 注册 Read 权限，通过正常 handler → Daemon scope broker → ImageDownload 查询读取。下载结果必须与持久元数据完全一致，PNG 头部与尺寸再次校验；返回不超过当前 128 KiB 上限的 Base64、file_size 和 image.png 文件名。本机路径与媒体凭据不暴露。保留 file_id 别名，双参数、坏类型、其他账号标识明确失败。图片上传、自发图片、其他格式和混合段仍未支持，不将读取贯通计为完整 image 段等价。

GitNexus 初次增量索引出现异常 target UID 与同名合并，按 UNKNOWN 处理后用 Windows `analyze --force --index-only --skip-fts --pdg` 重建并复查。精确 wrapper validate 为 HIGH、共享动作 handler 为 CRITICAL；持久化插入/投影和 resident 规范化为 LOW。固定脚本字面量及 trait 动态调用按源码补证。当前是离线实现与回归，不是 B1/B2 正式验收。

本块实际检查：`onebot::` 定向 unit 12/12，research `wrapper_image_chain` 与原 `wrapper_message_chain` 各 1/1；PNG 场景经真实命名管道覆盖私聊/群聊、两个事件订阅者、跨会话同 native ID、重复抑制、错账号 token、未声明能力、元数据指纹不符、Base64 下载和 actor/store 重启恢复。固定参考覆盖检查含真实 exporter 12/12，registry 为 15 keys（12 base + 3 aliases），分母 5,711 不变。

中间编译发现新增枚举未同步两个穷尽匹配，以及测试 WrapperElement 重复 image 初始化；修正测试后 unit 通过。首轮 all 在 `onebot_query_lab` 的旧 registry 数量 14 断言失败（5 PASS / 1 FAIL），显式更新为实际的 15 后，最终串行 `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all` 和默认并发 research 全仓均退出 0。失败/最终结果分别保存在 ignored `local-evidence/snowcat-image-onebot-compile.log`、`snowcat-image-onebot-all.log`、`snowcat-image-onebot-all-final.log`、`snowcat-image-onebot-parallel.log`，未用重跑覆盖原报告。

文档检查 PASS（15 live docs / 260 local links / 35 historical entries / 10 features），Windows `git diff --check` 通过。本块提交前对 staged/all 分别执行完整结构化变更图检查，保留 CRITICAL 风险与索引召回限制。没有启动 QQ；完整候选与剩余任务继续未完成，feat-008 保持 in-progress，产品 NOT READY。


## A2 PNG 发送构造与 D9 合同

从 `051a035` 继续离线实现。固定参考 `7cf025f` 的 `apis/file.ts:147–185` 提供宿主缓存路径/拷贝行为，`snowcat-onebot/api/file.ts:61–87` 提供发送元素，`helper/msg.ts` 当前对 PNG 走默认 picType=1000。Caligo 自有 `png::dimensions` 验证不超过 128 KiB 的普通 PNG chunk 边界、CRC、IHDR 尺寸/颜色深度、PLTE/IDAT/IEND 顺序，拒绝 APNG、坏 CRC、截断和尾随数据；这不是像素解码。

新增 research `build_image_send_js` 固定构造器和自有 `qq_image_send.js`。脚本用既有媒体账号绑定、UID 查询、服务器时间和 QQ cookie，独占创建宿主返回的 MD5.png 缓存目标，复用旧文件前核对类型、真实路径、长度、文件身份和 SHA-256；文件交给宿主后保留，不按 Promise 或 timeout 删除。发送元素按参考合同采用 picType=1000。D9 关联 cookie、chat/peer/sender、消息/元素、MD5/大小/尺寸，并在 Promise 受理且 add/update/精确回读一致后才产生 owned 自发图片记录。歧义、拒绝、超时和停止继续保留 unknown 层级。

自发回读仅在同账号 scope 允许 picType=1000/1001；原入站合同仍只接 1001，下载仍校验真实 PNG 头与尺寸。初次放宽模型时原入站 pic_type 拒绝回归失败，随后在 QueryData 校验中增加账号条件，未将收到的 JPEG/任意 1000 元素放行。原失败日志为 ignored `local-evidence/snowcat-png-send-initial.log`。

当前只有固定构造器/D9 合同与合成宿主覆盖，尚未接 HostOp、scoped image dispatch、core 持久派发或 OneBot image 发送，也不声明图片发送能力。真实 QQ 与缓存实际寿命仍待统一验收。不能把本检查点计为公开图片上传贯通。


实际检查：PNG framing unit 1/1；research 新 `wrapper_image_send` 1/1、原 `wrapper_send` 2/2、`wrapper_image_contract` 4/4 通过。新发送场景覆盖两种会话、缓存冲突/错路径、重复、Promise 与精确回读先后、元素/消息歧义、超时及 stop，并将成功的两条自发记录送入生产 Rust D8 decoder，核对同一 native/element/MD5/尺寸。新增自发 picType=1000 下载回归也通过，继续返回真实 PNG owned bytes。

串行 `init.ps1 -Mode all` 与默认并发 research 全仓均退出 0，覆盖检查含实际 exporter 12/12。日志在 ignored `local-evidence/snowcat-png-send-contract.log`、`snowcat-png-send-all.log`、`snowcat-png-send-parallel.log`、`snowcat-png-send-download.log` 和 `snowcat-png-send-coverage.log`。docs PASS（15 live docs / 262 local links / 35 historical entries / 10 features）；Windows diff 检查通过。GitNexus 按实际变更重新 force/index-only/PDG 刷新；字符串脚本与方法匹配 UNKNOWN 由源码补证，完整 staged/all 报告保留本机，未将缺失图边当作无影响。没有新增公开 action、image 发送 capability 或现场准入，候选仍未冻结。


## A2 OneBot PNG 图片发送接线

从 `a35463f` 接通普通单 PNG 的正常发送路径：共享 OneBot `image` 段 → Daemon 持久请求 → 绑定账号/host nonce/session generation/connection epoch 的 `DispatchImage` → bridge worker → Resident owner → 固定 profile 的缓存构造与 D9 精确回读。私聊和群聊使用独立 image 能力位；旧声明缺字段时默认 false。只接受不超过 128 KiB 的 Base64 普通 PNG，拒绝路径、URL、坏 framing/CRC、混合段和错账号；请求账本不保存媒体原文，unknown 不重发。native 回执本身不产生公开消息 ID，精确自发图片入库后才返回 ID；get_msg/get_image 与重启恢复沿用同一账号限定标识。

本轮同时修复真实阻塞：broker 的查询能力上限停在 9，而当前固定 profile 声明 11 项，整份声明因此被拒绝。上限改由 `QueryCapability::ALL` 的 12 个强类型变体派生；全部声明可通过，重复声明继续拒绝并清空权限。

GitNexus 绑定仓库/工作目录 `E:/stella/Caligo`，起点索引/HEAD `a35463f`。共享 send_payload 为 CRITICAL（22 symbols），已预警并覆盖共用动作层和真实管道；其余 owner、能力和 registry 影响分析保留 ignored `local-evidence/snowcat-png-wire*-impact.log`。字符串构造与动态 trait 分派由源码补证。

定向检查：research `resident_lifecycle` 22/22，包含 scope 失效、能力撤销、过期、坏 PNG 和重复提交零额外原生调用；真实 worker 的 `daemon_client_lab` 7/7，包含私聊/群聊 PNG 字节、owner 与回执接线；`onebot_private_send_lab` 5/5，包含独立能力、坏输入、等待持久回读、get_msg 和重启；查询 LAB 最终 13/13（包含 image unknown/reconnect 不重发）。原失败日志保留：能力测试第二次故意发送重复声明却误期待 HealthAck；图片测试误用 UTF-8 读取带二进制 framing 的 journal；均已修正测试，未改业务条件。串行 all 首轮因 Windows PowerShell 5 对 native stderr 的重定向行为提前退出；改为由外层 shell 捕获原始 stdout/stderr 后，串行 default/research 全仓通过。覆盖检查含实际 exporter 12/12，无 skipped。首次并行 research 在 manager_smoke 发生 1 PASS / 2 FAIL：各 fixture 的 free_port 立即释放，Windows 可复用同一个端口，导致多端口配置重复或监听冲突；已给合成 fixture 共享端口去重集合，并补配置失败 stderr，定向烟测 3/3。这里只修测试端口分配，不放宽生产唯一性和绑定拒绝。原失败和最终报告分别保留 ignored `snowcat-png-wire-parallel.log`、`snowcat-manager-ports-final.log`，最终全仓/图检查在本节追加。没有启动 QQ，完整候选仍未冻结，产品 NOT READY。


后续顺序复核保留两项新增失败：一次 default/research 在同一 target-dir 重叠执行，默认 gate 读到被 research 覆盖的可执行文件，结果作废并改为顺序运行；之后 research 的 daemon_client_lab 首项发现旧 fixture 目录仅含 PID，Windows PID 复用会读到历史 `R1` 成功，尚未派发便满足旧成功条件，引发 dispatch 计数失败和后续 Mutex poison。已按每次 fixture 的 PID、时钟和进程序号隔离 journal，保持生产恢复语义不变。失败原报告为 ignored `snowcat-png-wire-all-verified.log`、`snowcat-png-wire-all-sequential-final.log`。不将失败隐藏为通过。

最终验证：`snowcat-png-wire-all-isolated-final.log` 的顺序 default/research 全仓退出 0，随后独立默认并发 research 全仓 `snowcat-png-wire-parallel-isolated-final.log` 退出 0。覆盖含真实 registry exporter 12/12，无 skipped；docs 和 Windows diff 检查通过。提交前 force/index-only/PDG 重建并对 staged/all 分别做结构化 detect，完整报告保存在 ignored `snowcat-png-wire-detect-*.json`；前轮两个 scope 均完整返回 98 symbols / 18 flows、CRITICAL，后续 fixture 隔离也纳入最终检查。图仍有 FTS 不可用及 PDG 召回限制，未凭缺边降低风险。


## A2 群成员查询接线

从 `abab4e0` 继续处理无需 QQ 的独立合同。固定参考 `7cf025f` 的 `NodeIKernelGroupService.ts:111–124`、`GroupApi.getGroupMemberAll` 和 `OB11Construct.groupMember` 提供完整成员 Map 快照与 OneBot 字段合同。新增强类型 `GroupMembers` 查询/结果，经精确 pilot hash、当前账号/连接代次的 broker 和 owner 查询执行；新增共享 Read 动作 `get_group_member_list` 与 `get_group_member_info`。后者按完整快照的精确 UIN 选取，不把不存在的成员变成空对象，也不伪称调用了额外的 rich profile fetch。

校验 errCode、finish、ids/Map 键和值 UID 的一致性、唯一 UID/UIN、正整数 UIN 和 64 位边界、角色、长度、计数与时间字段；未完成快照、超过 2,048 项或 512 KiB 均明确失败，不静默截断。原生 UID、头像路径和额外凭据不进入 IPC。`no_cache` 仅接受布尔值，列表默认 false、指定成员默认 true；两者都实际查询宿主。旧 bridge 未声明 GroupMembers 时返回未支持，断链清空能力，不读 learned peers 或本地假成员数据。

编辑前 GitNexus 在 `E:/stella/Caligo` 当前工作目录检查共享 handler（CRITICAL）、query builder 和响应校验（LOW）；按源码补证固定脚本和 trait 分派。定向 wrapper 合同 6/6 通过，覆盖完整/空快照、Map 错键、重复身份、错误 role/时间/尺寸、业务拒绝、精确大整数及私有字段隔离，并将成功输出送入生产 Rust decoder。首轮 unit 为 11 PASS / 1 FAIL：新增两条真实注册项后，旧白名单未同步；已显式补齐白名单。其余实际命令、最终回归和状态在本节追加。没有启动 QQ，未冻结候选。


最终检查：`onebot_query_lab` 8/8，新增场景验证两个正常动作经过真实管道、64 位 ID、no_cache、错 nonce 回包忽略与 reconnect 清权；共享 unit、全仓串行 default/research（`snowcat-members-all.log`）与随后独立默认并发 research（`snowcat-members-parallel.log`）均退出 0。覆盖检查含真实 exporter 12/12，registry 17 keys（14 base + 3 aliases），固定参考分母 5,711 不变；新增成员动作和 wrapper 方法仍按 partial_offline 记录，未假称 rich profile 或现场完成。一次带 `onebot` 名称过滤的命令只跑 unit、将集成测试全过滤，随后专门不带过滤执行 `onebot_query_lab`，结果以上述 8/8 为准。GitNexus staged/all 首轮均完整返回 85 symbols / 21 flows、CRITICAL，报告为 ignored `snowcat-members-detect-*.json`；回归保留图的召回限制。feat-006 继续 blocked，其余完成门与 QQ 准入不变。


收尾另修 smoke 证据根目录：Cargo 从 package 目录运行测试，旧 `current_dir()/local-evidence` 可能让失败夹具落到未忽略的 `crates/caligo-core/local-evidence`。现按 CARGO_MANIFEST_DIR 定位 workspace 的 ignored 根目录；本轮已确认来源的失败输入夹具移入根 `local-evidence` 保留，未删除原证据。仅此测试路径变更后，default/research `manager_smoke` 各 3/3 通过，日志 `snowcat-manager-evidence-tests.log`。测试函数图为 UNKNOWN，由 `#[test]` 入口、三个真实 binary 调用及源码路径补证；没有把零图边当低风险。

### 下一原生合同门

固定参考再次只读核对到 `7cf025f`，当前需要 QQ 补证的具体边界如下；这些不是合成宿主可以替代的结论。

| 原生依赖 | 已有来源与缺口 | 下一项最小证据 |
|---|---|---|
| 当前登录身份 | `snowcat-framework/snowcat.ts:93–124` 等待 `onQRCodeLoginSucceed` 后才构造 selfInfo；`NodeIKernelLoginService.getLoginList` 返回历史列表。ProfileService.getSelfStatus 仅声明 Promise&lt;unknown&gt;，固定参考没有调用样本 | 在新指定、精确 hash 的已登录实例确认可重取的当前 UIN/UID 与昵称来源，并绑定 wrapper session/host nonce；不能从配置或历史列表推断 |
| 强制资料刷新 | `apis/user.ts:81–96` 经 `callNormalEventV2` 等待 `onUserDetailInfoChanged`，虽然 service TypeScript 声明了 Promise 返回形状，但实际流程依赖监听回调 | 确认当前 profile 的返回值、回调字段、scope/迟到关联和 listener 寿命；未据声明直接注册 get_stranger_info 或用旧缓存伪装 no_cache |
| 自有 SSO 二进制 | `packet/client/nativeClient.ts:33–83` 依赖 `napi2nativeLoader.initHook` 再取 rspbuffer；既有 42/919 字节对照仍来自第三方实验转换 | 在已准入的独立研究合同下确认自有转换的输入/输出 owned bytes、命令/序列与回调线程/寿命；不复用第三方后端，也不重放历史 unknown |
| 原生媒体与停止 | PNG/语音接线和精确回读已有离线证据，缓存保留及临时下载槽的真实 owner/final callback 尚无 Caligo 现场证据 | 在后续同一候选、双账号和各自 scope 上核实实际回调路径、对端观察、get_msg/get_image 和停止边界；本轮不启动 QQ 或热卸载 |

这是新增原生合同的取证边界，不是完整 A4/B1/B2 验收完成。其他格式、视频/文件、管理/通知与扩展动作、CLI/SDK/便携部署等仍有离线实现工作，继续以覆盖清单逐项推进；不能把所有 missing 项统称为“只差 QQ”。完整候选未冻结，B2 集中验收仍未开始，产品 NOT READY。

最终收尾包含 smoke 证据目录修正及原生合同门记录：GitNexus staged/all 各 14 files / 89 symbols / 21 flows，完整返回且风险 CRITICAL，结果为 `snowcat-members-detect-*-final.json`。`init.ps1 -Mode docs` PASS（15 live docs / 266 local links / 35 historical entries / 10 features），Windows staged/all diff 检查通过。

## 2026-11-14 双账号 ProfileService 现场探针

用户指定两个现有登录实例，本轮以 A、B 记录。通过两个 QQ 主窗口的当前账号资料卡独立确认 UIN、昵称及账号差异，窗口句柄与主进程 PID 和创建时间关联。A 为 PID 31112（`2026-11-13T07:28:25.1407405Z`），B 为 PID 45824（`2026-11-13T07:28:25.6469536Z`）。真实身份、窗口对应和输入保存在 ignored `local-evidence/snowcat-field-20261114/designation.private.json`。QQ 资料卡只作为当前账号的独立对照。

使用 `e9c291a` 源码构建隔离 research release CLI/bridge，`cargo build -p caligo-cli -p caligo-bridge --release --features caligo-cli/research,caligo-bridge/research --locked --offline --target-dir target-harness/snowcat-field-20261114` 通过。CLI SHA-256 为 `38B9C80F8F4DC0D62D82E7F8431485FE56DE0BFAE6B11E381206B138CE24AC79`，bridge 为 `F87FD9CA1EE7479AFFA8C07B251A2D2CB6051D9B69B0781145D16F9882B01EA5`。本机 pilot 清单五项通过。每个 PID 在注入前通过已知 backend/临时模块 PE 审计，按已记录的试点指纹执行 `envscan --vtable 0xA804990 --context 0`，各有唯一候选、零读失败。CLI 另核对目标实际加载的 QQNT.dll、major.node 和 wrapper.node 哈希。

顺序各跑一次现有 `qq-entry --uid-map-profile-service`，仅将对应资料卡 UIN 作为查询输入。A、B 均有 18 条成功的阶段记录，覆盖 bootstrap、V8、binding、wrapper session 阶段及 shutdown。两边进入 ProfileService 服务获取阶段后均返回 `profile_service_missing`，即 `getProfileService()` 未提供可用对象。两边均 `api_call_attempted=false`、UID 候选 0、CLI exit 2。探针退出后 PID 和创建时间未变，两边仍响应，owner pump 已正常关闭，bridge 留至 QQ 退出。B 的第一次准备因 PowerShell 自动转换 JSON 时间戳在 preflight 提前停止，经保留原时间字符串修正后才开始其唯一一次探针。未派发资料刷新或 SSO，未发送测试消息。

原始证据在同一本机目录的 `{a,b}-pre-attach-*`、`{a,b}-probe.log`、`{a,b}-profile.private.json` 和 `{a,b}-entry.jsonl`。两份阶段报告 SHA-256 分别为 `F2E413D7813C99DAD159A1771ED95B73540CA28E964ADE72D5143FCD94A556C1` 和 `18026CC502B18485FF88220035283B2933962680770331996D7942E52C2CFBB2`。

固定参考 `7cf025f` 的 `business-message/probe.cjs` 从 `initializeNapCat(session, login)` 获得宿主直接交付的 session。Caligo 此探针经 `Startup.create().getSessionIdList()` 取 `nt`，再用 `getNTWrapperSession` 重取 wrapper。两条路径的 session 等价性尚无证据。下一步为固定 hash 下核对两条 session 来源及 Caligo D8 使用的已知 `nt_*` 选择逻辑，先证明候选属于当前登录身份，再做一次有界服务表面对照。资料刷新仍需要 ProfileService 与 listener 寿命证据。SSO 继续依赖独立自有二进制合同及准入。现有两个 PID 已有 bridge 映射，当前一次性入口会拒绝重复使用。B2 候选与产品验收门仍待闭合，`feat-005/006` 保持 blocked，产品 NOT READY。

收尾运行 `init.ps1 -Mode docs`（15 live docs / 268 local links / 35 historical entries / 10 features）与 `git diff --check`，均通过。此次只新增现场证据与交接记录，未重跑 Cargo 全仓测试。

### 2026-11-14 ProfileService 会话来源修正

固定归档 `7cf025f` 的 [晚接入实现](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/blob/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/research/native-reconstruction/recovered/napiloader/napi_runtime.cpp#L96) 记录了已运行 QQ 上的公开加载路径：在宿主 N-API 回调中使用 `require` 或带 receiver 的 `process.mainModule.require`，按进程实际加载路径取得 `wrapper.node`，再用 `getNTWrapperSession('nt_1')` 取得 session。归档保留了原有 hook 捕获路径，真实初始化记录未区分两条路径的 session 来源。因此归档能支持本次选择该公开入口，现场仍需核对 Caligo 的 V8 owner 上下文能否走通。

Caligo 的 research mode 2 现在由自身 Rust bridge 重查 QQNT、major 与 wrapper 三个已加载模块的 SHA-256，携带快照中唯一 wrapper 路径，从 K4 owner pump 使用上述公开加载面和固定 `nt_1` 获取 ProfileService。缺少 require、加载失败、缺少 wrapper accessor 或 ProfileService 都在 resolver 派发前结束。现有 UIN↔UID 精确键匹配仍是查询判据，给定 UIN 的往返结果无法单独证明当前登录账号。mode 0/1 仍使用原 GroupService 阶段链。生产 D8/D9 与 LoginInfo 仍依赖各自合同。

旧版脚本不能通过新会话来源夹具。改后 5 项 UID-map 脚本测试通过，覆盖精确路径、`nt_1`、require receiver、不可用路径零派发、映射结果和异常清理；将 `nt_1` 改回 `nt` 的变异检查也被该夹具拒绝。串行 `init.ps1 -Mode all` 的 default/research 全仓、隔离 research release、最终 docs 和 diff 检查通过。新版 CLI/bridge 在 `target-harness/snowcat-profile-session-20261114/release/`，SHA-256 分别为 `9AF78959D8E6B12F611A0A8EDE51ADD9D396306A9959D95398576C7F1671A5C0` 和 `0EA7AB9E7613087636C36CFDD4D69394EDA73B28B91CDA36DE127F1088FDA5F9`。阶段图在 `e9c291a` 刷新。GitNexus 对 `run_inner` 的上游为 LOW，`start_uid_map_profile` 的函数值调用边为 UNKNOWN，经源码确认调用点在 UID-map owner pump。全量 detect 报 HIGH，7 条关联流，均为 UID-map export 和查询脚本调用链。完整构建和检查输出保存在本机 `local-evidence/snowcat-field-20261114/session-*.log`。

两个已指定实例仍映射上一轮的 bridge。新版尚无 QQ 现场结果。下一轮需干净的已登录 QQ 根，重跑同一有界探针，对照 `profile_require_missing`、`profile_wrapper_require_failed`、`profile_service_missing` 或实际往返结果，再单独核对当前身份与会话的绑定。

### 2026-11-14 ProfileService 修正后现场复验

用户确认重启账号 A。旧 A 的 PID 31112 已退出，唯一新增 QQ 主进程为 PID 35864，创建时间 `2026-11-13T23:23:21.5532160Z`。以用户确认、上一轮资料卡的本地账号记录和主进程创建时间指定测试对象。账号 B 继续使用原 PID 45824。新版 CLI/bridge 与上一节记录的 SHA-256 一致。五项 pilot manifest、PE backend/临时模块审计以及三项实际加载模块校验通过，外部只读 Environment 扫描唯一命中、零读失败。

在 A 上运行一次 `qq-entry --uid-map-profile-service`。实际路径为 `wrapper.node nt_1 NodeIQQNTWrapperSession.getProfileService`，服务获取与查询派发阶段均成功。正向精确键得到一个 UID，逆向精确键得到与查询输入相同的 UIN。结果为 `roundtrip_match`、`api_call_attempted=true`、CLI exit 0。共 14 条成功阶段记录，V8、服务获取和查询均在同一个 owner TID 执行。owner pump 正常关闭，A 同一 PID 仍响应，bridge 留至 QQ 退出。本轮只调用 A，未发送测试消息。

原始记录仅在本机 `local-evidence/snowcat-field-20261114/session-retest-35864/`。结果 `profile.private.json` 的 SHA-256 为 `EA4B2EC9ED13D9FF495FE160DB55EE06A9C332B758457210BA163E415FB40389`，阶段记录 `entry.jsonl` 为 `490A9D95BF293DB48F463BBEF1AD92EA9420A0B66AC3BBC2ED611CB7470E81EC`。

本次通过的是固定构建、一个登录实例、已知输入的 ProfileService 映射。独立的当前登录身份来源及 wrapper session/host nonce 绑定仍待验证。资料刷新回调、SSO、B2 集中验收仍开放，`feat-005/006` 状态保持 blocked。收尾更新进度、交接和 feature 证据，运行 `init.ps1 -Mode docs` 与 `git diff --check`。

### 2026-11-14 当前登录身份通知现场复验

使用精确 `9.9.36-53644` wrapper SHA-256 `0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C`。零参数 `getSelfStatus` JS 描述符/入口为 RVA `0x41C9680/0xF65B32`。用户第一次重启 A 后，PID 18856 的实际 Promise 返回数值 `result`，未提供直接身份字段或 `statusInfo`，探针拒绝并正常关闭。后续固定映像追踪确认 `0x22A0AB0` 的 Promise resolver 写 `result` 与 `errMsg`。身份字段来自 `onSelfStatusChanged` 转换链 `0x23F8E01 → 0xEBEBCE`，转换器写 `uid`、十进制字符串 `uin`、`status`。listener add/remove 描述符分别定位 `0xF60E60/0xF6131C`，add 以十进制字符串返回注册 ID，remove 将原字符串按十进制解析。固定映像脚本、反汇编与首轮负结果在 ignored `local-evidence/snowcat-field-20261114/current-login/`。

自有 research `qq-entry --current-login` 在精确模块校验后复用 K4 owner pump 与已验证的 `wrapper.node nt_1` 获取路径。此入口拒绝用户 UIN 和 daemon 账号输入。使用一笔 `getSelfStatus()` 及同一 ProfileService 的 `onSelfStatusChanged`，由 Promise 确认操作结果，从通知提取有界字段。在 owner tick 核对同一 session ID 与精确双向映射。捕获后以原字符串 ID 移除 listener。代码按探针状态丢弃迟到通知，停止或超时尝试在 owner 上移除 listener，清理无法确认时拒绝结果，bridge 随 QQ 生命周期保留。

用户第二次重启 A 后，选中唯一新增主进程 PID 50100（`2026-11-13T23:41:31.7396013Z`）。先后通过模块 PE 审计、五项 pilot manifest、三项实际加载模块 hash 和唯一 Environment 候选，扫描零读失败。运行一轮无 UIN 输入探针得到 `current_login_match`、操作结果 `0`，同一 wrapper session ID 在查询前后匹配。通知提供的 UIN 和 UID 都匹配 PID 35864 现场映射的独立本机对照。listener 移除调用正常返回，owner pump 正常关闭，CLI exit 0。A 与 B（PID 45824）仍响应，本轮未调用 B、未发送消息、未热卸载。

最终 CLI/bridge SHA-256 为 `E833092A1B2AE9A04FF97B697AEEBD2571BD333DD31698E1446B1D48B98344C5` / `3DA7D9D4D10AC5724754F84094E773D5496332B4673C01B01A46A62F74DC6C53`。现场证据在本机 `current-login/listener-50100/`，结果与阶段报告 SHA-256 为 `9F76C9830621B2D89852E7F9A11BCF1D1933C6E4AEB8AA26BE1667A3D5EC9A72` / `A0A59B8D4ACA5AED1505CB494FE81F3F35E4D319D340D9B6ED1B5BCA67C3CAE7`。

此次证据覆盖固定构建、A 的单次现有登录、宿主通知提供身份、会话 ID 稳定和移除 API 返回。生产 host nonce、昵称、换号/断线撤权、跨账号隔离和原生 listener 全局静默仍待验证。通知无请求 ID，此样本不能证明通知的唯一请求因果关系。B2、资料刷新与 SSO 仍待验收，`feat-005/006` 保持 blocked。

最终 listener 构建通过 6 项 UID-map 定向测试、6 组无效 CLI 参数拒绝、隔离 research release、串行 `init.ps1 -Mode all`（default/research）、最终 docs（15 live docs / 274 links / 35 historical entries / 10 features）和 `git diff --check`。样本与构建日志均保存在上述本机目录。

### 2026-11-14 当前登录昵称与宿主 nonce

`5665b2b` 已推送并纳入 [草稿 PR #6](https://github.com/Enoa-devteam/Caligo/pull/6)。后续在同一 research 探针上接入固定参考和 Caligo 好友查询共用的 `getCoreAndBaseInfo('nodeStore', [uid])`。以宿主通知取得并经双向映射验证的 UID 为唯一输入，按精确 Map 键、`coreInfo.uid` 与 `coreInfo.uin` 校验资料条目，仅保留有界昵称。请求期间持续检查同一 wrapper session ID。收到离线或冲突身份通知时，拒绝当前身份结果并忽略后续 Promise 回包。结果同时记录与生产 daemon 共用生成函数的进程 nonce。单次探针报告的 PID、nonce 与 session ID 尚未进入生产 IPC 连接代次。

用户重启 A 后，唯一新增 QQ root PID 31724（`2026-11-13T23:50:24.7320164Z`）通过模块审计、五项 pilot manifest、实际加载模块校验与唯一 Environment 候选。执行一次 `--current-login`，返回 `current_login_match`、`nickname_status=matched`，UIN/UID 匹配上轮独立对照，昵称与先前 QQ 资料卡一致。报告含非空会话 ID，宿主 nonce 的 PID 部分与目标匹配。14/14 阶段成功，移除 listener 返回，owner pump 正常关闭，CLI exit 0，两台 QQ 仍响应。此次仅调用 A，未发送消息。

构建 CLI/bridge SHA-256 为 `B9C493174024C4926C5899361DB845C114BA73E0C68BCA753C6069CBE1772819` / `E9F944F38AA1A62B406C0E6EA248558EB80C2F38EE78B8FFDF56CF39C1D0182C`。本机证据在 ignored `local-evidence/snowcat-field-20261114/current-login/nickname-31724/`，结果与阶段报告 SHA-256 为 `01FD8157253A2EE19FC58E02B2E018FDCA1C3B041C8F14E50F3B79E8FB4B2E00` / `67B99C3CB13527CCBEB3D74C7F4501BF37E11E1D8FF61DABB47B42971AB3CE43`。

6 项 UID-map 定向测试、default/research 全仓、隔离 research release、文档检查（15 live docs / 276 links / 35 historical entries / 10 features）及 `git diff --check` 通过。测试包含错键、错 UIN、UTF-8 字节上限、资料请求失败、会话漂移、冲突身份与离线后迟到回复。

生产下一步在正常 daemon/IPC 查询作用域内验证账号、host nonce、session generation、连接 epoch 与宿主身份一致，并覆盖撤权后旧请求及迟到回复。当前探针的离线/换号用例为合成回归，真实 QQ 换号/断线仍待验证，`LoginInfo` 尚未声明。资料刷新、SSO 和 B2 保留各自现场门。

### 2026-11-14 正常 daemon/IPC 登录查询

正常 `caligod` 的共享 OneBot 动作层通过已认证命名管道向 QQ owner 查询 `LoginInfo`。请求和结果绑定账号、宿主 nonce、会话代次与连接 epoch。固定 profile 从当前 ProfileService 通知取得身份，核对账号及精确 UIN/UID 往返后读取昵称，移除临时 listener 后才接受结果。撤销连接时，排队中的旧请求失效，已派发的请求回到 owner 清理。旧基线不声明此能力。

首轮 A（PID 38652）在 D8 脚本的 `return` 后遇到换行，监听启动返回 `undefined`，OneBot 请求未完成。daemon 和 owner pump 随后正常关闭。修复 D8 的表达式包装并加回归，同时让只读查询可以独立于 D8 的启动状态被调度。

用户重登 A 后，PID 29812 通过 QQ 9.9.36-53644 的五项 pilot manifest、加载模块校验、注入前 PE 审计和唯一 Environment 候选检查。连接仍使用现有 `qq-9.9.33-52230` IPC 协商标签，原生调用由实际加载模块的 SHA-256 选中 9.9.36 profile。正常 `qq-entry --daemon-pipe` 只接入 A，D8 报告 `listener_armed`。首次 PowerShell HTTP 调用连接中止，用直连 loopback 的 curl POST 复验，收到 HTTP 200、OneBot `retcode=0`。返回账号与指定 A 一致，昵称与前次独立对照一致。无令牌请求返回 HTTP 401。

`daemon-control stop` 和 `qq-entry-stop` 均 exit 0。随后确认 daemon 已退出、HTTP 端口已关闭、owner 阶段报告为 `shutdown ok`，A、B 仍响应。未发送消息。A 保留 bridge 映射。本机原始记录位于 ignored `local-evidence/snowcat-field-20261114/current-login/ipc-29812/`，`entry.jsonl` SHA-256 为 `E1C1FE310CB9D6A96EAA81CCA8B9AA9B7BADB925FD8E03FFCAD734571691E28B`。

隔离 release 的 CLI、bridge、daemon SHA-256 依次为 `428B4664C6F282DEC58531983EF5F6CF2CE7DF76CDCC1C1EE3AA232E104A8D63`、`309EADD93F2759ABED9A59583DA23D8CB7FA3C90FFB9A1FABFC6CAF00EA8A85A`、`6271670F8F01C236BF7B887E6DFFBD20DB9B100D7846334642E4F914B823E3E3`。default/research 全仓和覆盖检查 12/12 通过。真实命名管道离线回归覆盖共享动作的成功路径、撤权时取消在途请求及拒绝重放旧 scope。真实 QQ 换号、断线重连、双账号查询隔离和 listener 长期静默仍待验证。资料刷新与 SSO 各有独立合同，B2 尚未开始，`feat-005/006` 保持 blocked。

### 2026-11-14 续验预检与实例代次

用户再次授权使用两个现有登录实例继续测试，并提示已有部分结果。本轮先对照上方正常登录查询证据，选定下一工作块为正常 daemon/IPC 的双账号 LoginInfo 隔离；不重复已通过的单账号样本，不重放历史 unknown 消息。

北京时间 08:15 左右的只读进程/模块核对确认：A 仍为 PID 29812、创建 `2026-11-14T00:07:23.5849154Z`；B 仍为 PID 45824、创建 `2026-11-13T07:28:25.6469536Z`。两者为响应正常的 QQ root，各已有一个 `caligo_bridge.dll`。A 的 bridge SHA-256 为 `309EADD93F2759ABED9A59583DA23D8CB7FA3C90FFB9A1FABFC6CAF00EA8A85A`，B 为 `F87FD9CA1EE7479AFFA8C07B251A2D2CB6051D9B69B0781145D16F9882B01EA5`，分别匹配既有登录查询与首轮 ProfileService 构建。两边磁盘 wrapper hash 均匹配固定 pilot 的 `0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C`；这次未运行完整 manifest 或 envscan，不声称通过新一轮原生准入。当前 caligod/manager 进程数为 0。

对两个 PID 分别运行已有 PE-export-aware 模块审计（脚本 SHA-256 `2238D736345ECF41E93F626439BE97832892E67B5A9084B88C6DB4621B6855D4`），两次均 exit 2 / REJECT，findings 仅为已加载 Caligo bridge 的名称和 QQ/Windows 根目录外路径；签名有效的 Defender 模块沿用既有白名单。此结果证明当前目标不能按干净实例门再次接入，不是新的 QQ 功能失败，也不证明全局静默或卸载安全。

完整进程/模块快照、审计和本轮脚本仅保存在 ignored `local-evidence/snowcat-field-20261114/resume-preflight/`。`processes.private.json` SHA-256 为 `2ECCF1A1A6482B228756647437B54DE5F5D340505C5556DE7159D7E3924965F6`，两份审计为 `DC877CC192DC33A165CFC03B7BB8DE58D0408CE5A99947D3311F2E935F820A9F` / `94D8ECF67D397003A40F3DCFD0C12C81EB40BD4A41CAE0203A1A1BC81C9B6809`。本轮未注入、未调用宿主 API、未发送消息或终止 QQ。已请求用户正常退出并重新登录两个测试实例；取得新 PID/创建时间并重新独立核对账号及准入后，才开始双账号查询隔离。`feat-005/006` 保持 blocked，完整候选与 B2 验收不因授权或本次预检自动完成。

本轮 HEAD `5665b2b`，保留任务开始时的大量既有未提交改动；本次仅补状态/交接/证据，未修改 Rust。`init.ps1 -Mode docs` PASS（15 live docs / 280 local links / 35 historical entries / 10 features），feature JSON 解析通过。WSL Git 的 diff 检查将既有 CRLF 文件报为尾空白，未据此批量改写；Windows Git `git diff --check` exit 0，完整输出保留本机 `windows-diff-check.log`。未运行 Cargo tests，不产生新的功能验收结论。

### 2026-11-14 双账号登录查询隔离

用户回复已正常重启两个 QQ。本轮只验正常 daemon/IPC 的双账号登录查询和单端停止隔离。A 为 PID 49952（创建 `2026-11-14T00:24:00.9951886Z`），B 为 PID 35780（创建 `2026-11-14T00:24:01.4962797Z`）；两者均为新代次、响应正常且注入前未映射 bridge。通过各自 QQ 资料卡的号码/昵称与窗口、root PID 对照，沿用原 A/B 别名；没有按进程先后次序猜账号。

五项 pilot manifest 校验 PASS；各自实际加载的 QQNT.dll、major.node、wrapper.node 磁盘 SHA-256 与固定清单一致，wrapper 仍为 `0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C`。两份 PE-export-aware 模块审计均 exit 0 / PASS，envscan 各 1 个 Environment 候选、0 read failures。复用上一节已通过离线回归的隔离 release：CLI / bridge / caligod SHA-256 分别为 `428B4664C6F282DEC58531983EF5F6CF2CE7DF76CDCC1C1EE3AA232E104A8D63` / `309EADD93F2759ABED9A59583DA23D8CB7FA3C90FFB9A1FABFC6CAF00EA8A85A` / `6271670F8F01C236BF7B887E6DFFBD20DB9B100D7846334642E4F914B823E3E3`，本轮未重建或修改 Rust。

两端分别启动正常 caligod，使用独立目录、journal、随机凭据、命名管道和 loopback HTTP 端口，凭据确认为不同。各自 `qq-entry --daemon-pipe` 单次接入 exit 0，D8 均 `listener_armed sid=nt_1`。正常连接仍使用既有 `qq-9.9.33-52230` IPC 协商标签，真实原生 profile 由实际模块 hash 选择。两端 daemon 同时存活；下表 HTTP 请求依次执行，没有覆盖同时在途的查询竞争。

| 样本 | A | B |
|---|---|---|
| 各自令牌查询 get_login_info | HTTP 200 / retcode 0 | HTTP 200 / retcode 0 |
| 返回账号与昵称 | 匹配本端 QQ 资料卡 | 匹配本端 QQ 资料卡，与 A 不同 |
| 使用另一端令牌 | HTTP 403 / retcode 1403 | HTTP 403 / retcode 1403 |
| 无令牌 | HTTP 401 | HTTP 401 |
| A 正常停止后再查 B | A daemon/owner 均已关闭 | HTTP 200 / retcode 0，身份结果与停止前相同 |

A 的 `daemon-control stop` 和 `qq-entry-stop` 均 exit 0，随后才查询 B。B 查询通过后也正常停止 daemon/owner，均 exit 0。最终核对两 QQ 仍在同一创建代次且 Responding=true、两个 daemon 均退出、两个 HTTP 端口均无 listener；各保留一份上述固定 hash 的 bridge，未热卸载。阶段日志均以 `shutdown ok` 结束。本轮没有发送消息；两个 journal 都为空，因此不据此声称跨账号事件或账本持久化已验收。

验收脚本中间有两个问题：PowerShell 默认 ANSI 解码使 manifest JSON 读取失败，显式 UTF-8 后完成准备，失败时尚未接入；首次 A 交叉令牌实际返回 403，但脚本错误预期 401 而停止。保存响应证明请求已拒绝，将断言修正为交叉令牌 403 / 无令牌 401 后复用已有 A 响应，完成剩余测试；没有用重跑覆盖原响应。该修正仅涉及本机验收脚本，不是产品鉴权修复。

原始身份、QQ 截图、凭据、模块清单、运行脚本和 HTTP/阶段日志仅保留在 ignored `local-evidence/snowcat-field-20261114/dual-login-20261114/`，汇总为 `summary.json`。A/B 的 `entry.jsonl` SHA-256 分别为 `983B5CF22251DC53DA5258B680DEA360ACEF48280DE64BE952AF50B52E941F75` / `1A94C0C6360FC6964E885CFE13C7E9247F75BBE08FCABB359EAB32AE54A35C01`；具体命令见本机 `run.ps1` 的分阶段入口。GitNexus Windows 注册表绑定 `E:/stella/Caligo`、索引 HEAD `5665b2b`；概念 query 因 FTS 不可用返回空，具名 `cmd_qq_entry` context 找到符号，启动/鉴权/停止参数再从当前源码核对。本轮未提交。

本样本证明固定构建的两个正常登录查询会话、独立 HTTP 鉴权及停止一端后的另一端可用性。真实换号、断线重连、同时在途/迟到回复、事件/账本隔离和长期静默仍待各自现场证据；资料刷新与 SSO 独立留待合同核对。`feat-005/006` 保持 blocked，未将本项通过当作 B2 全矩阵或产品验收完成。

收尾：汇总脚本校验 7 个 HTTP 样本、账号/昵称独立对照、不同凭据/管道和最终健康状态通过；`summary.json` SHA-256 为 `9A8A29E1FAA30731945CEFE82B3DD0DEA448706B1BE1FEB12B3877590F6B73D3`。`init.ps1 -Mode docs` PASS（15 live docs / 282 local links / 35 historical entries / 10 features），feature JSON 解析和 Windows Git `git diff --check` exit 0；已审阅本轮状态/交接差异，保留所有既有改动。本次没有 Rust 变更，未重跑 Cargo tests，沿用所测二进制的前次离线构建证据。

### 2026-11-14 当前账号资料刷新研究合同

本工作块继续核对资料刷新，未更改公开 OneBot registry、HostQuery 或能力声明。固定参考 checkout `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64` 的 `apis/user.ts:81–96` 使用 `fetchUserDetailInfo('BuddyProfileStore', [uid], mode, [0])` 并等待匹配 UID 的 `onUserDetailInfoChanged`；枚举 KSERVER=1、KALL=0。参考 helper 的事件只按 UID 过滤，通知本身没有请求标识，不能推断唯一因果关系。TypeScript listener ID 声明为 number，与本构建已证十进制字符串不同，继续以精确本机实现为准。

静态核验仅针对 `wrapper.node` SHA-256 `0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C`。方法描述符 RVA `0x41C9A40` 指向 `[0xF68DAA,0xF6910C)`：明确检查 4 参数，依次读取 trace string、UID string list、32-bit source、32-bit biz list，经 provider 虚槽 `+0x140` 传递，并返回本次创建的 Promise。该结论来自固定映像的参数提取与调用指令，不是从接口声明猜测 ABI。

该 Promise 的回调构造链 `0xF6922E → 0xF79534` 使用 vtable `0x41CB0C8`；slot 1 的 `[0x23FA818,0x23FA8CD)` 将 result、errMsg、source 与 detail 数据打包，固定执行闭包最终到 `[0x23FABB9,0x23FADDC)`。后者构造 `result`、`errMsg`、`source`、`detail` 并 resolve 原 Promise。detail 转换器 `[0x77399C,0x773B7D)` 构造 JavaScript Map；key/value 转换经 `0x7742A0 → 0x9B96`、转换器表 `0x4413C78` slot 5 到 `[0x23FAEB2,0x23FAF47)`，将字符串 UID 作为 key，并调用 `0x23F6DE6` 构造 value。通知入口 `[0x23F6C73,0x23F6DBA)` 也调用同一 `[0x23F6DE6,0x23F705F)` 转换器，传 1 个 JS 参数给 `onUserDetailInfoChanged`。输出字段为 uid、uin、simpleInfo、commonExt、photoWall、bizExt；其中可选字段有明确 native 分支。通知转换器没有写请求 ID、trace 或 source。本轮静态证据不证明实际调用成功、回调线程动态归属或服务器新鲜度。

自有实现新增 research-only `qq-entry --refresh-current-profile --uid-map-result <new-local-evidence.json>`，复用精确 hash 检查、K4 owner、无 UIN 输入的当前身份通知与往返校验，再对当前账号单次请求 KSERVER。原 listener 先于请求存在，保留 self-status 漂移/离线检测。请求 Promise 必须 result=0 / source=1 / 真实 Map 的精确 UID 行，通知也必须有相同 uid/uin 和有界昵称；只复制 uid、uin、nickname，两个结果一致且 listener 移除返回后才报告 `profile_refresh_match`。通知独立不能成功，报告显式 `notification_request_causality_proven=false`；source=1 表示请求及返回的来源标记，不冒充独立服务器抓包证明。缺结果、缺通知、冲突、会话变化、错误来源、超时和取消均不重试；清理状态未知时保留全局状态阻止新研究请求，并随 QQ 进程保留 bridge。

`profile_refresh.js` 为固定语义脚本，由 Rust owner tick 驱动；新增模式 4 保持原 FFI context 布局、拒绝外部 UIN 和 daemon 共用。普通构建、互斥参数及 stop 入口拒绝均有回归。新增 Node VM 合同覆盖 22 个场景：同步早到/ack-first、无关与重复通知、错误 UID/UIN/source、非 Map、缺结果/通知、冲突、预算、异常、超时、离线/会话变化、取消、迟到拒绝与清理未知后 BUSY。只验证自有状态机；不替代真实 QQ。

GitNexus 绑定 `E:/stella/Caligo`、起点索引 `5665b2b`。`gates` 上游 HIGH（3 direct / 4 flows），已预警并对共用准入及停止拒绝回归；run_inner、poll/cancel 和 CLI 主入口为 LOW。动态导出及同名 parse_args/usage 的图结果 UNKNOWN，经 `qqentry.rs` 与 `lib.rs` 的 export-name/调用点文本补证，不按零 caller 放行。原始固定-hash 静态脚本、disassembly 和检查日志仅在 ignored `local-evidence/snowcat-profile-refresh-20261114/`。本轮沿用现场实例的 bridge 映射，不重新接入或热卸载；该探针尚无真实 QQ 结果，`feat-005/006` 保持 blocked。

验证与中间失败：直接 Node 严格拒绝模式的 22 场景、research 身份脚本 9/9、CLI legacy_gate 5/5 通过。首次 `init.ps1 -Mode all` 被 PowerShell 5 合并重定向将 Cargo stderr 转为终止错误而中断，保留 `all.log`；改由外层 shell 重定向后，default 全仓通过，但 research 的 qq_entry_lab 首个关闭测试报“close 回调未注册”，其余 5 项因共享锁中毒失败（1 PASS / 6 FAIL），记录为 `all-final.log`。单独未修代码的 qq_entry_lab 7/7 通过，不能抹除前次失败。源码核对发现该夹具在 closer 线程启动后固定 sleep 50ms 就驱动 owner，无法保证 close_req 已发布；生产 shutdown 明确先写 close_req 再 uv_async_send。将夹具改为通过 fake_uv_async_send 的 channel 通知同步后，串行及默认并发 qq_entry_lab 各 7/7 通过，生产关闭逻辑未改。最终串行 `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all` exit 0、default/research 全仓通过，日志 `all-fixed.log`；未用后续通过覆盖失败。

隔离 research release 构建 exit 0：`cargo build -p caligo-cli -p caligo-bridge --release --features caligo-cli/research,caligo-bridge/research --locked --offline --target-dir target-harness/snowcat-profile-refresh-20261114`。CLI SHA-256 `8F8F9E634BB86B6295379887985037EBDB51F3FA086AEE6E290B33896783A156`，bridge `A961B2A36CCD9AF13B9C572E45F7DDD4C7C5CAA0302FE2AFD8AAED1E43004F9C`；没有覆盖 QQ 仍映射的旧 DLL。精确静态锚点额外核验了 descriptor、回调表和 7 个完整 `.pdata` 函数体摘要，保存为本机 `native-contract.json`。已准备单次现场 runner（`run-field.ps1`），先要求独立资料卡匹配的新 PID/创建时间，再跑 manifest/PE audit/唯一 envscan，固定 CLI/bridge hash，最后保存结果和双实例健康状态。已请求用户正常重启 A（原 PID 49952），B 可保留；在取得新目标前不执行探针。

收尾：GitNexus 按当前工作树刷新，all 结构化检测为 18 files / 72 symbols / 247 flows、CRITICAL（包含任务开始时既有大量未提交改动）；返回列举数量与汇总一致，无 partial/truncated 标志，完整报告为本机 `detect-all.json`。索引自身仍报告候选执行流召回上限，不能把未出现的路径当作无影响。本轮未提交。修改的源码和测试 SHA-256 保存为 `source-snapshot.json`，与 HEAD `5665b2b` 共同绑定证据。`init.ps1 -Mode docs` PASS（15 live docs / 284 local links / 35 historical entries / 10 features），feature JSON 解析与 Windows Git `git diff --check` exit 0；已审阅本轮源码/状态差异，既有改动保持未覆盖。

### 2026-11-14 当前账号资料刷新现场

用户回复“A 已重启”后，选定新的 A root PID 25848（创建 `2026-11-14T00:57:15.8739102Z`，窗口 2755464）。QQ 当前资料卡的账号和昵称与原 A 独立对照一致；B 仍为 PID 35780、创建代次未变。真实身份和截图只保存在本机 `field-designation.private.json` / `field-profile-card.private.png`。现场前核对源码/测试全部匹配前节 `source-snapshot.json`，复用已通过 default/research 全仓的隔离 release，不重建或覆盖旧映射 DLL。

单次 `run-field.ps1` 先核对 CLI / bridge 的前节固定 hash，再验证五项 pilot manifest（PASS）、新 A 的 PE-export-aware 模块审计（exit 0 / PASS）、Environment 唯一候选（1 hit / 0 read failures），并在接入前再次核对 PID 创建时间与响应。正常 `qq-entry` 的 mode 4 另核实际加载 QQNT.dll、major.node、wrapper.node 的精确 hash。随后只运行一次 `--refresh-current-profile --uid-map-timeout-ms 20000`，没有 UIN 输入、没有 daemon 共用，也没有消息发送。

CLI exit 0，结果 `status=profile_refresh_match`、`profile_refresh_call_attempted=true`、`self_status_result=0`、`nickname_status=matched`。当前身份来自 `nt_1` 宿主通知并通过精确往返；报告记录同一 host nonce。返回账号/昵称与本轮 QQ 资料卡相同。刷新请求与 Promise 回复 source 均为 1（KSERVER），其精确 UID 行和已观察目标通知的 uid/uin/昵称一致，报告 `matching_notification_observed=true`。最终 `listener_registration=remove_returned`；CLI 关闭 owner pump 成功，阶段日志末条 `shutdown ok`。报告继续保留 `notification_request_causality_proven=false`，不把无请求标识的通知当作唯一因果证明，也不把 source 标记当独立网络抓包或服务器新鲜度证明。

关闭后两次只读健康核对均见 A 25848 / B 35780 响应正常、创建代次不变，当前 caligod/manager 进程数为 0。A 保留 bridge SHA-256 `A961B2A36CCD9AF13B9C572E45F7DDD4C7C5CAA0302FE2AFD8AAED1E43004F9C`，B 保留前轮 `309EADD93F2759ABED9A59583DA23D8CB7FA3C90FFB9A1FABFC6CAF00EA8A85A`。后查审计 exit 2 / REJECT 的 findings 仅为已加载 Caligo bridge 的名称及目录，未发现该审计覆盖的第三方 backend/未知临时模块；此拒绝符合“不得把已映射实例再次当干净目标”的门。没有重试或热卸载。

本机 ignored `local-evidence/snowcat-profile-refresh-20261114/field-*` 保存本轮全部输入、预检、模块、结果、日志和健康状态，`field-summary.json` 校验通过。结果文件 SHA-256 `7596BBD6847E0DD437CEF69DA45DB6335646969BAA0E3984D415C743A6199B2E`，阶段日志 `E6D095A36A372C08FD5D8DC1077FD8B81FB7C6BDF0BF68572F228DF8D81FA46F`，汇总 `17E9178CB4BE1020954BB0742603BD122CB36DB79E4D37F10E4AC39502D04934`。本轮只补现场证据/状态，未修改 Rust 或注册公开动作。该样本仅证明固定构建、当前账号 self、单次请求、有界身份/昵称和移除 API/owner 关闭返回；其他资料字段/目标、公开 OneBot 接线、真实换号/重连、listener 长期静默和 B2 全矩阵仍待验收。`feat-005/006` 保持 blocked。

收尾：HEAD 仍为 `5665b2b`，本轮源码/测试与已验证 snapshot 相同，未重复运行 Cargo tests；沿用前节全仓和隔离构建证据。`init.ps1 -Mode docs` PASS（15 live docs / 286 local links / 35 historical entries / 10 features），feature JSON 解析和 Windows Git `git diff --check` exit 0。已审阅状态/交接变更，原始现场文件确认被忽略；保留既有未提交改动，本轮未提交或推送。


### 2026-11-14 当前账号资料刷新正常接线

本工作块将前节已证 self 合同接入正常查询，不扩展任意目标或 rich profile。共享 `get_login_info` 新增可选扩展参数 `no_cache`，缺省/false 继续 `LoginInfo`；严格布尔 true 仅派发独立 `SelfProfile` capability/query/data。公开结果仍为 user_id/nickname；typed 请求不含目标参数，UID 和其他资料不跨 IPC。旧 bridge 未声明此能力时明确未支持，不回退到 LoginInfo 或 StoreReader。source=1 表达 KSERVER 请求/回复标记，不额外宣称独立服务器新鲜度或通知的唯一因果关系。

精确 pilot owner 使用已验的 session/profile provider、当前身份/往返/昵称合同和固定刷新脚本；单次发起、5 秒脚本期限和外层查询期限。LoginInfo/SelfProfile 共享 singleflight，绑定 request ID、完整 QueryScope 和查询种类；撤权在 owner 取消，迟到 Promise/通知检查原 state 与 phase，不得写入新请求。刷新成功必须为 profile_refresh_match、匹配通知、source=1、同一 UID/UIN 与 listener remove_returned，并采用刷新回复昵称而非起始缓存昵称。未证 listener 移除保留状态并禁止继续派发；从未注册 listener 的 surface 失败可清理，不将其误留为未知寿命。桥 DLL 仍按 QQ 生命周期保留。

修改位置：`caligo-model/capability.rs` 强类型和验证，`caligo-bridge/qq_query.rs` 独立精确 profile 声明，`qq_entry.rs` 的 owner 分派/种类绑定及报告解码，`qq_v8.rs` scoped start/poll。公共动作 registry 仍 17 keys，覆盖图仅更新自有查询证据说明。正常 pipe 夹具覆盖独立能力、错响应类型、跨账号、旧 epoch 与重连清权；真实 worker/owner 夹具覆盖执行线程与撤权不重放。实际 scoped JS 新增 5 个场景（错账号零 fetch、无 listener surface 清理、刷新行优先、旧回调跨下一请求隔离、清理未知 BUSY），原研究 22 场景继续通过。

验证绑定 HEAD `5665b2b` 及本机 `local-evidence/snowcat-self-profile-wire-20261114/source-snapshot.json`：research workspace no-run 编译 exit 0；定向 bridge lib 100/100、model 18/18、daemon_client_lab 9/9、onebot_query_lab 10/10 PASS。随后清理边界细化后的最终 `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all` exit 0，default/research 全仓均通过（`all.log`，保留套件原有 ignored 项）；原 probe 与实际 scoped JS 合同在全仓中通过。覆盖清单刷新执行当前 Rust registry PASS，结构测试 11 PASS / 1 个显式环境门跳过；该跳过项的 registry 比对已由刷新命令实际执行。文档与最后 diff 检查结果在本节收尾补记。

隔离 `cargo build --release --workspace --features caligo-core/research,caligo-cli/research --locked --offline --target-dir target-harness/snowcat-self-profile-wire-20261114` exit 0，没有覆盖任何已映射 DLL。SHA-256：CLI `83DE0C6366EC87585D2B27883FBA7EE5A87E4CCCC50644EE09AC70CC4E6DC9C2`；bridge `F1A128B60840F80EDA3F1B9112837AF7A690B951668F8BD23F887794117911C1`；caligod `AEB71E628AEAE676CC4CC33D384811F70CC81726A76E3BE4FB5AC024FDD9B1FB`。本机 `builds.json` 和已通过 PowerShell 语法检查的 `run-normal-entry.ps1` 准备单次真实入口测试：新 A 独立资料卡与创建时间、manifest/PE 审计/唯一 envscan、固定构建、一次刷新、核对后关闭 daemon/owner。脚本准备不代表现场执行通过。

GitNexus 显式绑定 `E:/stella/Caligo`、索引 HEAD `5665b2b`；编辑前 handle_action_with_queries 为 CRITICAL（3 direct / 5 flows）、query_response_with_voice 为 HIGH（3 direct / 3 flows），已预警并覆盖实际管道。query/query_capabilities/capability 的同名或动态路径 UNKNOWN，经 resident 及 typed broker/动作装配调用点补证。当前工作树重新索引 7,813 nodes / 19,460 edges / 565 flows；all 结构化检查 21 files / 80 symbols / 153 affected flows、CRITICAL，包含开始时已有的大量 dirty 改动。列举与 summary 数量相同，未返回 partial/truncated 字段；索引自身明确报告候选流召回及分支预算缺失，缺席路径仍不能解释为无影响。报告与原始日志只在本机 evidence 目录。

本轮没有新 QQ 接入或消息发送。只读复核仍为 A 25848 / B 35780，各一份旧 bridge，创建时间不变、均响应。已请求用户正常重启 A，B 可保留；依据现有接入与未证寿命合同，不重注入同代次、不热卸载。收到重启后才可重新独立核对并运行一次正常入口刷新；任何身份/hash/审计失败立即停，失败不自动重试。当前结论为 implemented/wired/LAB，正常入口 field、换号/重连、长期静默与 B2 全矩阵仍未验收，feat-005/006 继续 blocked，产品状态不提升。

收尾：`.\init.ps1 -Mode docs` PASS（15 live docs / 288 local links / 35 historical entries / 10 features），Windows Git `git diff --check` exit 0；源码快照 10 项与已验证文件一致，feature JSON 解析通过。已审阅此次模型、动作分派、owner/scoped 脚本及回归差异，历史 dirty 改动未覆盖；本轮未提交或推送。现场 runner、构建/hash、图和原始测试日志均位于 ignored evidence 目录。当前等待用户重启 A，尚无本次正常入口 field 结果。


### 2026-11-14 正常入口当前账号资料刷新现场

用户回复“A 已重启”后，新 A root PID 46620，创建时间 `2026-11-14T01:25:57.5425320Z`；B 保持 35780、创建 `2026-11-14T00:24:01.4962797Z`。新 A 未映射 Caligo bridge。通过该 PID 所属 QQ 主窗口的当前资料卡独立核对账号/昵称，与原 A 指定身份相符；截图、账号和昵称仅保存在 ignored `local-evidence/snowcat-self-profile-wire-20261114/normal-a-profile-card.private.png` 与 `field-designation.private.json`。界面弹出资料卡的绘制有延迟，后续不重新激活窗口的截图已取得清晰对照；没有从配置账号推定身份。

复核前节 10 项源码/测试 snapshot 全部相同，CLI/bridge/caligod 匹配前节固定 SHA-256，复用已通过全仓 default/research 的隔离 release，没有重建或覆盖旧映射文件。五项 pilot manifest PASS，新 A 的 PE-export-aware 模块审计 exit 0 / PASS，envscan 1 hit / 0 read failures。正常 qq-entry 接入另校验实际 loaded 的 QQNT.dll、major.node、wrapper.node 精确 hash；未沿用旧版本偏移猜测。daemon 的既有 runtime baseline 标签仍为 qq-9.9.33-52230，原生准入以实际 pilot manifest/loaded module hash 为依据，不能将标签当真实构建证明。

使用预备的 `run-normal-entry.ps1` 分段启动独立 caligod、随机管道/凭据和 loopback 临时 HTTP 端口，attach exit 0，owner TID 47564。随后只发送一次带鉴权的正常动作请求 `get_login_info`、`params={"no_cache":true}`、固定 echo。HTTP 200、curl exit 0、retcode 0 / status ok，返回 user_id 和 nickname 精确匹配该轮独立资料卡，echo 原样返回；data 恰含这两个公开字段。请求保留一次性 reservation，未重发、未发送聊天消息，也未执行研究 mode 4 探针替代正常入口。

该结果沿共享动作 → scoped broker/IPC → owner SelfProfile → typed SelfProfile 返回。固定构建成功分支要求 profile_refresh_match、单次 KSERVER source=1 的每请求回复、匹配资料通知、同一当前 UID/UIN 及临时 listener remove_returned，采用刷新回复昵称，不接受 LoginInfo 充当刷新结果。该轮公开结果只暴露有界身份/昵称，没有另外导出底层刷新报告；不能将本次 HTTP 结果说成独立的通知因果追踪或服务器新鲜度测量。此前研究样本的 `notification_request_causality_proven=false` 边界继续保留。

正常关闭 caligod 及 qq-entry owner，两个命令均 exit 0，阶段日志 `shutdown ok`。最终只读核对 A/B 创建代次不变、均响应，daemon 已退出、HTTP 无监听。A 保留 bridge `F1A128B60840F80EDA3F1B9112837AF7A690B951668F8BD23F887794117911C1`，B 保留旧 `309EADD93F2759ABED9A59583DA23D8CB7FA3C90FFB9A1FABFC6CAF00EA8A85A`；每边各一份，不重复接入、不热卸载。A 后查审计 exit 2 / REJECT，findings 仅已加载 caligo_bridge.dll 的已知名称和目录；它不再是干净目标，符合后续接入拒绝条件。

本机 `normal-summary.json` 断言通过，输入、鉴权、身份、journal 和原始结果全部留在 ignored evidence 目录。结果文件 SHA-256 `70FF7CCAC1FF85699B663F9AD82E128F84DEA85CD64145D27FEDDB9A9BAB9829`；阶段日志 `2595A050250FC3169EFE5716EF3597C4C07D2C684CD380FE61BBA1085060DAFE`；最终健康报告 `84EFE858DD1192782DD2A9F9D65793F6EB4811C2209368455CA7C283E67A5915`。本轮未改 Rust/测试或重跑 Cargo，使用前节匹配源码/二进制的全仓证据。本次仅证明固定 pilot、指定 self 和一次正常入口资料刷新；任意目标/其他字段、真实换号/断线重连、长期回调静默及 B2 全矩阵仍未验收，feat-005/006 blocked 和产品 NOT READY 保留。

下一工作块可先离线收敛真实换号/断线重连的撤权合同与验收步骤；不能凭本样本自动开放任意 get_stranger_info 或 SSO，也不能复用 A/B 的已映射代次做新接入。本轮只补现场结果和状态，未提交或推送；文档与 diff 检查见本节收尾。

收尾：`.\init.ps1 -Mode docs` PASS（15 live docs / 290 local links / 35 historical entries / 10 features），Windows Git `git diff --check` exit 0，feature JSON 解析通过。已审阅本轮新增现场记录、progress、feature evidence 与 handoff；真实身份、凭据和原始响应确认受 `local-evidence/` 忽略规则覆盖，未新增入库。本轮工作块完成，未扩展到其他现场能力或提交发布。


### 2026-11-14 登录会话撤权与重连合同审计

本工作块按前节交接只审计真实换号/断线重连的撤权前提与验收顺序，没有访问 QQ 进程、改网络、切换账号或重新接入。新增[撤权验收合同](../acceptance/2026-11-14-session-revocation.md)，明确配置 account/session_generation、宿主 nonce/创建时间、同 core 的 connection_epoch、core_instance_nonce 与 wrapper session ID 的不同寿命；更新查询指南、索引和状态。没有修改 Rust、测试或产品入口。

现有管道合同：daemon BridgeQuerySession 的 Drop 清 broker scope/能力并终结 pending；worker session_loop 退出由 ClearScope 发布 None 并唤醒 owner。resident 在合法 owner 上取消旧查询，无合法上下文时保留 cancel_only。重连需重新声明能力，旧 scope/ID 结果只作 stale。现有 LoginInfo/SelfProfile 夹具在 set_bridge_scope(None) 后主动 drain 证明取消，但没有覆盖 None 发布后、owner 尚未 drain、新连接恢复数值相同 scope 的交错。

审计发现两项仍待闭合，不能用已通过测试替代：

1. **core 重启的撤权交错**：daemon 的 epoch 从固定初值开始，新进程可能重复 epoch 数值。runtime ConnectionIdentity 带随机 core nonce，公开 query ID 也含该 nonce，所以不能据此声称旧结果能满足新请求；但 QueryScope 不含 core nonce，resident.invalidate_queries_except 按 scope 相等保留 pending。旧 owner 尚未执行撤权就恢复相同 scope 时，旧查询取消是否必达缺少回归。当前结论为可定位实现边界和测试缺口，尚未通过新夹具复现缺陷，不夸大为已证跨请求污染。
2. **QQ 身份变化的持续失效来源**：已验 Profile listener 是单次查询临时 listener，只使该查询在身份/session 变化或离线分支失败；worker 的 account/session_generation 固定来自启动配置。当前受审 D8 MsgService 与 ProfileService 路径没有闭合持续通知→全路径清权→事件归属失效→新身份重绑。管道仍通时，QQ 自身网络变化不能从 IPC 心跳推定；任意目录/收发也不能继承一次 self 查询的身份保证。

合同将同 core 管道恢复、core 重启、QQ 网络状态、同进程换号及 QQ 进程重启分列，定义每项输入、成功条件、证据和停止边界。R1b 的稳定私有 token-file 与 pipe/journal 条件已查 CLI 正常参数；普通 control Stop 会使 worker 正常退出，不能冒充自动重连干预。当前不请求用户重启 QQ：先补同 scope 交错的离线用例和必要修复，再独立核实持续身份通知合同；准备完整后才进入相应新代次现场。

GitNexus 绑定 `E:/stella/Caligo` / `codex/snowcat-capability-integration`，索引 `5665b2b`、349 files / 7,813 symbols / 19,460 edges / 565 flows。多个同名 Caligo 注册已显式消歧。使用 gitnexus-exploring 的 CLI/LocalBackend 回退，读取 overview、run_worker/session_loop/set_bridge_scope、query_host 的精确 UID、broker bind 和 listener 上下文，以及 session_loop 的两条 process 轨迹。FTS 禁用且 embeddings=0，概念 query 为空；带 file_path 的部分符号记录缺 filePath，按精确 UID 补查。图自身还有动态分派/receiver typing 和候选流召回边界，某些轨迹含保守误关联，已以源码复核，不按图缺席证明不存在。日志/结构化图结果在 ignored `local-evidence/snowcat-reconnect-contract-20261114/`。

实际离线验证：串行 Windows MSVC `cargo test -p caligo-core --features research --test daemon_client_lab --test capability_query_lab --test onebot_query_lab --locked --offline --target-dir target-harness` exit 0，分别 9/9、13/13、10/10 PASS；`cargo test -p caligo-bridge --lib --features research uid_map_script_tests --locked --offline --target-dir target-harness` exit 0，10/10 PASS（90 filtered）。共 42 项现有回归，无新增测试或 QQ 操作；这些通过不覆盖上述新交错和持续身份缺口。受审源码/夹具 15 项 SHA-256 为本机 source-snapshot.json，上一 self-profile 的 10 项 snapshot 全部相同，未重复全仓或构建 release。

下一工作块选同 scope 撤权交错的离线复现与修复评估，编辑前须按调用图做 impact，必要变更后运行适用回归/全仓；本轮不扩展到未证身份 observer 或现场切号。feat-005/006 仍 blocked，产品 NOT READY；既有 46620/35780 是上一现场保留 bridge 的历史代次，本轮未核对其当前健康，不据旧样本推断。

收尾：`.\init.ps1 -Mode docs` PASS（15 live docs / 296 local links / 36 historical entries / 10 features），Windows Git `git diff --check` exit 0；feature JSON 与 15 项源码 snapshot 再核对通过。本轮新增 87 行验收合同已由 docs 索引导航，原始图/测试日志确认被忽略；审阅新增文档与状态差异，既有 dirty 源码未改动。本轮未提交或推送，现场状态不提升。


### 2026-11-14 相同 scope 重绑撤权修复

本工作块承接前节同 scope 交错缺口，先用合成宿主重现，再最小修改 resident；没有 QQ 接入、网络干预或账号操作。新测试 `crates/caligo-bridge/tests/query_revocation.rs` 在独立线程模拟 worker 的 scope 发布/重绑，owner 线程控制何时 drain，并记录 query start/poll/cancel；不依赖 sleep 制造竞态、不调用 QQ 或发送操作。

**修复前证据**：default `cargo test -p caligo-bridge --test query_revocation --locked --offline --target-dir target-harness` exit 101，1 PASS / 3 FAIL（`repro-before.log`）。相同 scope 重绑使已发起旧查询执行 poll 而不是 cancel；未发起的旧查询也进入 start；两次 invalidate 对同一旧查询产生两条 Disconnected。未撤权、仅重复发布同 scope 的正例通过。此处已证明旧查询取消丢失和重复终态，不据此扩大为跨 core 新请求结果污染；query ID 的随机 core nonce 隔离仍保留。原 resident 和输入 SHA、失败日志在 ignored `local-evidence/snowcat-scope-revocation-fix-20261114/` 保留。

**修复**：`Resident::set_bridge_scope` 在 adapter 锁内、发布新 scope 前，将不属于新 scope 的 pending 不可逆标为 cancel_only 并一次性报告 Disconnected；None 撤权包含所有 pending。重绑数值相同 scope 不清除该标记。`invalidate_queries_except` 复用同一内部标记逻辑，并与 submit/owner poll 共用 adapter 锁，重复失效不再重复报告。取消仍由既有 owner drain 在合法线程/env/context 上执行，无法执行时保留队列与标记；worker 线程不执行 native cleanup。没有修改 IPC/schema、core nonce、公开动作或 QQ ProfileService 脚本，也没有以逻辑 Disconnected 宣称 native callback 已静默。

首轮修复后同命令 exit 0 / 4 PASS（`repro-after.log`）。随后追加 scope 从原值切到另一值、再切回而不显式经过 None 的用例。最终 5 项合同覆盖 LoginInfo/SelfProfile 的在途旧查询、未发起旧查询、无撤权正例、上下文暂不可用/恢复后恰一次 owner 取消及无重复结果、直接 scope 切回；取消完成后新查询可正常完成。新文件 rustfmt --check PASS；本轮只格式化该新增文件，没有批量格式化历史 dirty 源码。

编辑前 GitNexus 显式绑定 `E:/stella/Caligo`、worktree 同路径、索引/HEAD `5665b2b`。set_bridge_scope 和 invalidate_queries_except 均 HIGH（各 1 direct / 4 flows / 3 modules），已预警；前者有 receiver typing 缺口，drain_queries 上游 UNKNOWN，已从 daemon_client::session_loop/ClearScope、lib.rs owner drain hook 和现有回归调用点补查。查看相应 process/context 后才改动，helper 为新符号。当前树索引刷新为 7,843 nodes / 21,376 edges / 564 flows；all detect 为 22 files / 72 symbols / 34 affected flows、CRITICAL，包含开始时既有改动。列举数量与 summary 相同，未返回 partial/truncated 字段；索引仍有 639 候选入口未排名、分支/trace 预算和动态分派缺口，不能将未列出的流当无影响。完整结构化结果为本机 detect-all.json。

本轮没有生成或加载新 release。既有 QQ 映射仍是旧二进制；源码/LAB 修复不能当现场生效。R1b 的真实 core 重启 runner、取消观测与现场恢复仍待准备；持续 QQ 身份失效到全路径清权、真实换号/网络状态及长期寿命也不由本修复闭合。feat-005/006 继续 blocked，产品 NOT READY。全仓与最终文档检查结果在本节收尾补记。

最终验证：Windows MSVC 串行 `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all` exit 0，default/research workspace 全部通过，query_revocation 两模式各 5/5 PASS，既有真实 pipe/worker 和 profile 脚本回归均通过（all.log，套件原有 ignored 项保留）。这是共享 resident 修改的全仓验证，不代表 QQ 现场恢复通过；未额外生成 release。当前两项源码/测试 SHA-256 为本机 source-snapshot.json，具体本轮 resident 差异另存 resident-fix.patch，避免与既有 dirty 改动混淆。状态/合同已更新，产品层级不提升；最终 docs/diff 检查见下段。

收尾：`.\init.ps1 -Mode docs` PASS（15 live docs / 298 local links / 36 historical entries / 10 features），Windows Git `git diff --check` exit 0，feature JSON 与本轮源码/测试 snapshot 核对通过。已审阅最小 resident patch、新增夹具和文档/状态改动；复现材料、旧源码和图/测试日志仅存 ignored evidence。历史未提交改动未覆盖，本轮未提交、推送或发布。下一步为 R1b 跨两个真实 daemon 实例的离线恢复验证与现场 runner 准备，当前不要求用户重启 QQ。

### 2026-11-14 只读目录与群成员真实 QQ 现场

用户重启两个 QQ 后，新根 A PID `21784`（启动参数账号 `登记账号 A`）和 B PID `41548`（启动参数账号 `登记账号 B`）均响应正常，精确 profile 模块 hash 匹配。两根均无旧 bridge；固定 manifest verify PASS；PE-export-aware backend 审计均 `PASS_KNOWN_BACKEND_AND_TEMP_MODULE_GATE`，仅允许合法签名 Windows Defender 模块；envscan 各 1 hit、0 read failures。当前工作树 release 各 attach 一次成功，entry log 均记录 `env_fresh`、`first_entry`、`listener_armed`；两端 owner shutdown 成功。

只读动作结果：A `get_friend_list` 和 `get_group_list` 均 HTTP 200，但 `retcode=1440`、`data=null`、wording 为“宿主查询响应不符合合同”；没有将它们转为空数组。B `get_friend_list` HTTP 200 / retcode 0 返回 4 项；`get_group_list` HTTP 200 / retcode 0 返回 11 项。按真实返回选取 B 的第一个群 `本机登记群`，`get_group_info` 和 `get_group_member_list` 均 HTTP 200 / `retcode=1440` / `data=null`，因此没有猜成员 ID，也没有发起 `get_group_member_info`。全程没有消息发送。

两端 daemon/owner 与 HTTP listener 已关闭；A/B QQ 仍响应，各保留本轮一份 bridge。该样本证明 B 的好友/群列表路径在固定 profile 上通过；A 目录、群详情、完整成员快照/指定成员及双账号完整目录矩阵仍未通过。`retcode=1440` 作为宿主响应合同拒绝保留，不放宽校验、不自动重试。原始响应、审计、envscan、entry 和脱敏摘要仅在 ignored `local-evidence/snowcat-directory-fresh-20261114/`。

### 2026-11-14 只读目录与群成员查询离线/管道验收

本工作块没有操作 QQ。当前两个根 PID `24376`/`46664` 仍保留上一轮 bridge，因此未重复接入、未读取 QQ 目录、未发送消息。按固定 profile 和共享动作合同运行：`cargo test -p caligo-core --test onebot_query_lab --locked --offline --target-dir target-harness` 通过 10/10；`capability_query_lab` 通过 13/13；`cargo test -p caligo-bridge --test wrapper_query_contract --features research --locked --offline --target-dir target-harness` 通过 6/6；`daemon_client_lab` 通过 9/9。

证据覆盖好友列表、群列表、群详情、完整群成员 Map 快照和指定成员，正整数/布尔参数、64 位 ID、Map 身份、角色和大小预算校验，未声明能力、错 scope、断链/重连、迟到结果以及真实空列表/业务失败的区分。正常 handler 只消费真实 pipe 的 typed 结果，不从 learned peers 或配置拼接目录，不把失败转换为空列表；群成员指定成员不额外假称 rich profile fetch。以上是 implemented/wired/LAB 证据，不是特定 QQ build 的目录现场结果；真实 QQ 目录现场待下一次新鲜无 bridge 代次完成准入后单独执行。

### 2026-11-14 普通文本入站现场与空提及字段适配

用户重启后的 A PID 40844（`2026-11-14T06:16:46.5895354Z`）通过固定模块摘要、PE/backend 与唯一 envscan。使用上一轮固定 release 接入一次，LoginInfo 匹配账号，独立 loopback SSE 已订阅。用户从 B 单次发送私聊和登记群各一个唯一标记，确认 A 的 QQ 显示两条。原构建的 SSE 与消息账本均没有这两条标记。同期账本收录普通群图片，证明该现场存在已进入账本的入站记录。采集脚本先按 `get_msg` 的现有返回字段修正核对逻辑，第二订阅沿用原进程，两个标记仍均缺失。daemon、owner 和 HTTP 正常关闭，QQ 响应且保留 bridge。现场原始材料保存在 ignored `local-evidence/snowcat-text-receive-field-20261114T0616/`。

固定 `wrapper.node` 的 TextElement 输出 `0x102B116` 将两个数值提及字段转换为十进制字符串，缺省数值可表现为 `"0"`，可选 `atNtUid` 未设置时调用 Null helper `0xC3EC7D`。参考的普通文本分支以 `atType=0` 区分。Caligo 只在精确 pilot 对 `atType=0` 接纳这几种空值并归一为原有 owned 空串，保留非零提及和非空身份拒绝。GitNexus 重建后对共享接收脚本的影响为 CRITICAL，定向覆盖旧 profile 拒绝、私聊和群聊转换、真实管道投影、图片与发送脚本。隔离构建为 `target-harness/snowcat-text-receive-20261114/release/`。首轮现场只有缺失证据，具体根因须用修复后真实样本确认。

修复后复验使用 A 新进程 380（`2026-11-14T07:46:21.6864561Z`）。固定模块、PE/backend、唯一 envscan 和 LoginInfo 均通过，bridge `B82C45A2…060A595` 单次接入。B 发来的私聊和群聊标记均进入 SSE 和消息账本，各有一条账本记录和独立的 OneBot ID。私聊在现场通过正常 HTTP `get_msg` 核对。群聊的发送者、接收账号和会话类型匹配，群号与预设测试群不同，采集脚本在调用群聊 `get_msg` 前按原判据退出。用户在 QQ 中独立核对实际发送群号，结果与 SSE 和账本一致。此次群聊样本对应实际发送群，原登记测试群样本仍待验证。

关闭现场 daemon、owner 和 HTTP 后，使用同版 release library 打开账本副本，两条消息经生产 `get_msg` handler 读回，消息 ID、类型、发送者、平台时间和正文均与 SSE 一致。该次回读覆盖持久化恢复与共享动作层。QQ 仍响应，保留 bridge。原始响应、群号核对与回读材料在 ignored `local-evidence/snowcat-text-receive-fixed-field-20261114T0746/`。本轮只覆盖 B 到 A 的普通文本，反向收发、Caligo 主动发送、其他协议和媒体仍待验。

### 2026-11-14 登记测试群现场 HTTP 回读通过

A 新进程 51052（`2026-11-14T07:52:32.9216946Z`）通过固定模块哈希、PE/backend 审计和唯一 envscan，使用上一轮文本修复构建单次接入，LoginInfo 匹配账号。SSE 就绪后，用户从 B 在登记测试群发送一个新标记。群事件的目标群、发送者、接收账号和会话类型匹配。账本中恰有一条对应记录，现场 HTTP `get_msg` 返回 HTTP 200 / `retcode=0`，消息 ID、类型、发送者、时间和正文与 SSE 相符。回读后的三秒观察窗口没有重复标记。

daemon、owner、HTTP 和 SSE 均正常关闭，QQ 仍响应，bridge 保留。原始材料在 ignored `local-evidence/snowcat-designated-group-receive-20261114/`。结合上一轮私聊，B 到 A 的普通文本已有私聊和登记群的正常 SSE 与 HTTP 回读证据。下一项是固定 wrapper 的 Caligo 主动文本发送现场验收，需新进程及明确的单次发送目标和授权。旧 G3/MSF 的 R4 准入条件继续适用其原生路径。

### 2026-11-14 主动私聊现场预检

用户授权 A 到 B 的单条私聊测试并重启 A。旧 51052 退出，新进程 39368（`2026-11-14T07:56:22.8761822Z`）响应正常且无 bridge。复用已核对的文本修复构建，固定模块、PE/backend 审计和唯一 envscan 通过。`onebot_private_send_lab` research 5/5 通过，含持久消息 ID 关联和结果未知时不重复派发。

执行工具以策略拒绝启动包含单次 OneBot `send_private_msg` 的现场命令。检查时没有 caligod，目标仍无 bridge，本轮没有发送请求、回执、读回或 B 端观察。原始预检和样本计划在 ignored `local-evidence/snowcat-private-send-field-20261114T0756/`。主动私聊现场仍待执行。

### 2026-11-14 Caligo 主动私聊现场通过

同一 A 进程 39368（`2026-11-14T07:56:22.8761822Z`）仍符合预检并且尚未接入。确认无既有发送请求后，按固定文本修复构建 `B82C45A2…060A595` 单次接入，LoginInfo 匹配 A。经鉴权的 OneBot HTTP `send_private_msg` 向 B 提交一条授权文本，返回 HTTP 200、`retcode=0` 和持久 OneBot 消息 ID。发送端用该 ID 调用 `get_msg`，核对私聊类型、发送者、正文和有效平台时间。账本恰有一条样本，收件人匹配 B，并含稳定 native ID。用户在 B 的 QQ 确认显示一条对应标记。

daemon、owner、HTTP 和 SSE 正常关闭，QQ 响应并保留 bridge。原始请求、响应、账本和停止记录在 ignored `local-evidence/snowcat-private-send-field-20261114T0756/`。本轮通过固定版本的单次 A 到 B 私聊文本发送及回读。Caligo 主动群聊、媒体、其他协议和恢复矩阵仍待验证。

### 2026-11-14 Caligo 主动群聊现场通过

A 新进程 36752（`2026-11-14T08:02:44.1809079Z`）通过模块哈希、PE/backend 审计和唯一 envscan，使用固定文本修复构建单次接入。LoginInfo 匹配 A。鉴权的 OneBot HTTP `send_group_msg` 向登记测试群提交一条标记，返回 HTTP 200、`retcode=0` 和持久消息 ID。发送端现场 `get_msg` 读回同一 ID，群聊类型、发送者、正文和平台时间校验通过。账本的初始记录与修订具有同一 OneBot ID、原生消息 ID 和目标群。

B 首次查看时未见消息，随后用户确认 A 和 B 均显示同一标记。全程只提交一次，显示延迟未测量。daemon、owner、HTTP 和 SSE 均正常关闭，A 的 QQ 仍响应并保留 bridge。原始材料在 ignored `local-evidence/snowcat-group-send-field-20261114/`。本轮同时运行 `daemon_chain` research 5/5。结合主动私聊与两种入站场景，固定构建已有普通文本收发样本，其他传输、媒体和恢复验收仍待完成。

### 2026-11-14 A 好友列表复验与群目录计数修复

用户重启 A，旧 54900 已退出。新主根 6620（`2026-11-14T06:08:43.6942191Z`）通过精确模块、PE/backend 审计、唯一 envscan；此前已存在的 30156 不参与。单次接入后，LoginInfo 匹配绑定账号。`get_friend_list` HTTP 200 / 0，共 193 项。`get_group_list` HTTP 200 / 1440，`group_member_count_exceeds_capacity` 确认宿主返回人数超过非零容量。daemon、owner 与 HTTP 均关闭，QQ 响应，保留 bridge。原始证据在 ignored `local-evidence/snowcat-directory-buddy-dedup-field-20261114T0608/`。

固定 wrapper 对 `memberCount` / `maxMember` 分别调用 `napi_create_uint32`。精确 profile 的 GroupList 接线现在原样传递这两个字段，保留 ID、类型、范围、重复、预算及零容量有成员时的拒绝。GroupInfo 继续校验人数不超过容量。定向 wrapper 7/7、OneBot 10/10，含正常命名管道上的超容量群列表和详情拒绝。初版定向回归发现持久 listener 会捕获首次请求的 profile，已改为读取当前 pending 的标记后通过。

用户再次重启 A，旧 6620 退出。新主根 50620（`2026-11-14T06:10:22.2461175Z`）通过同样准入，隔离 release `target-harness/snowcat-directory-group-counts-20261114/release/` 的 bridge `298C67B5…C13152` 接入一次。LoginInfo 匹配绑定账号，`get_group_list` HTTP 200 / 0，共 75 个唯一群，其中 2 项原样保留人数超过非零容量的宿主字段。daemon、owner 和 HTTP 均正常关闭，QQ 响应，保留 bridge。未发送消息。原始证据在 ignored `local-evidence/snowcat-directory-group-counts-field-20261114T0610/`。至此 A/B 的只读目录、群详情和成员动作各有真实样本通过，证据跨多个进程代次与构建。

### 2026-11-14 双账号群成员通过与 A 目录诊断

用户重登后，A PID 54900（`2026-11-14T05:55:46.1873354Z`）与 B PID 5608（`2026-11-14T05:55:46.6879620Z`）均通过固定 manifest、三角色模块 SHA-256、PE/backend 审计和唯一 envscan。使用隔离 `snowcat-directory-member-diagnostic-20261114` release（bridge `C193FD35…E01D1EE5`），各接入一次，登录查询均 HTTP 200 / 0 且身份相符。

B 的原失败群详情 HTTP 200 / 0，群号匹配；补齐同一目标的修复后对照。两端在登记测试群各得到 389 项完整成员快照，成员 ID 集合一致。各以对应当前账号的唯一快照项查询成员详情，HTTP 200 / 0，群和成员身份匹配。A 的好友列表仍 1440，诊断确认重复 UID；群列表仍 1440，诊断为 `invalid_group_counts`。两端 daemon/owner/HTTP 正常关闭，QQ 仍响应，各保留 bridge。无发送，原始材料在 ignored `local-evidence/snowcat-directory-member-diagnostic-field-20261114T0555/`。

后续离线修复对好友分类中重复 UID 保留首次出现顺序，按唯一键调用 ProfileService，保留 2,048 项和 typed UIN 校验。群列表诊断再区分超范围、容量为零但有成员和人数超过非零容量；原有拒绝条件继续生效。定向回归 7/7、`init.ps1 -Mode all` 和隔离 release 通过。release 为 `target-harness/snowcat-directory-buddy-dedup-20261114/release/`，完整 hashes 见本机 `buddy-dedup-builds.json`。下一轮用新 A 复验好友并核对群列表分支。

固定 wrapper 的 `onGroupListUpdate` 经 `0x240080C` 将群结构 `+0x50` / `+0x54` 分别用 `napi_create_uint32` 暴露为 `maxMember` / `memberCount`。该静态结论将 1440 缩小到人数高于容量的关系校验，仍需现场区分容量为零与非零两种分支。重启后的初次核对中，旧 A 54900 仍保留 bridge，另有无账号启动参数的干净 QQ 主根 30156；其窗口最小化，未完成账号独立核对，因此本次未接入该根。

### 2026-11-14 GroupInfo 字符串 handle 修复与目录现场

用户重启后选择 B PID 47868（创建 `2026-11-14T05:36:48.0095304Z`）运行上轮诊断构建。固定 manifest、已加载三角色模块 SHA-256、PE/backend 审计与唯一 envscan 均通过。B 接入一次，LoginInfo HTTP 200 / 0 并匹配绑定账号；此前失败的群详情目标单次 HTTP 200 / 1440。内部诊断为 `group_detail_listener_registration`，注册返回类型为 `string`，代码在 `getGroupDetailInfo` 和 `removeKernelGroupListener` 前拒绝。按合同停止本代次后续查询。现场脚本首次预检因子 PowerShell 未加载签名模块失败，换用 `pwsh` 复验通过；启动前的日期反序列化修正使用 `-DateKind String`。这两项修正前没有启动 daemon 或接入 QQ。

对固定 `wrapper.node` SHA-256 `0D8B9444…D534C` 进行离线反汇编。add 的 N-API thunk `0xF79764` 通过 `0x7808A0` 格式化无符号 ID，并在 `0xF79906` 调用 `napi_create_string_utf8`。remove thunk `0xF799E4` 通过 `0xC3EE2F` 的字符串分支及 `0xC3ED7E` 读取 UTF-8，经 `0x780440` 按十进制解析，随后在 `0xF79B35` 调用原生 remove；后端 `0x1C776E` 经 `0x7FB8E` 查找并移除同一注册表项。封闭/空会话路径返回零。适配仅启用在精确 pilot 上，校验规范正 `u64` 十进制字符串，将原值传给 remove，保留原有整数 profile 及未知/取消/超时的失败边界。GitNexus 重建绑定此工作树，入口 impact 为 CRITICAL（59 个上游、8 类流程），共享嵌入脚本引用需以源码补证。

字符串适配定向 7/7、default/research 全仓及隔离 release 均通过。保留未接入的新 A PID 41076（创建 `2026-11-14T05:36:47.5075487Z`）通过相同准入后，用新 release（bridge `4E36A3A2…80416C`）接入一次。LoginInfo HTTP 200 / 0，身份匹配；已登记测试群的 GroupInfo HTTP 200 / 0，群号匹配且群名存在。成功通过受理、匹配通知、typed 校验与 remove 正常返回，未验证全局 callback 静默。A 与 B 样本使用不同群，B 原失败目标的修复后对照仍待复验。

A 的单次群成员列表、好友列表、群列表均 HTTP 200 / 1440。成员快照已进入匹配 Map 项，`joinTime`、`lastSpeakTime` 为 `number`，拒绝码 `invalid_member_timestamp`。好友列表进入 `buddy_uid_shape`，旧代码合并了非字符串与重复 UID。群列表进入 `typed_contract`，原结果只保存外层失败。成员列表失败后未执行成员详情。两端 daemon、owner、HTTP 均关闭，QQ 响应，各保留原 bridge；未发送消息。原始材料分别在 ignored `local-evidence/snowcat-directory-registration-field-20261114T0536-b/` 和 `local-evidence/snowcat-directory-string-handle-field-20261114T0536-a/`。

后续离线变更对精确 pilot 的非负安全整数时间戳按十进制无损编码；原十进制字符串仍支持。好友 UID 类型和重复分别诊断，群列表在原有拒绝条件前记录固定原因；Rust 解码失败使用有限固定码。公开失败行为维持原合同。定向测试覆盖安全整数边界、越界/负数/小数拒绝、群号重复和数量边界。下一轮需新 QQ 进程复验成员，并采集好友/群列表拒绝原因。

### 2026-11-14 GroupInfo 修复现场停止与注册诊断收敛

用户重启后本轮选定 A PID 21800（创建 `2026-11-14T04:33:04.5733619Z`）/B PID 3932（创建 `2026-11-14T04:33:05.0785430Z`），启动参数与各自测试账号相符。固定 manifest、实际三角色模块 hashes、PE/backend、唯一 envscan 全部通过，attach 前无 bridge。使用前节已测 release（bridge `F613C122…3EC9CE8`）各接入一次，正常 `get_login_info` 各 HTTP 200 / retcode 0 并匹配绑定账号。

B 对上一轮实际群单次 `get_group_info` 返回 HTTP 200 / retcode 1440，新增内部诊断为 `group_detail_listener_cleanup`，diagnostics 为空。本轮立即停止剩余好友/群/成员查询，没有重试该请求、没有发送消息。两端 daemon/owner 停止 exit 0，HTTP 监听均关闭，QQ 均响应、各保留原 bridge。runner 初次读取 daemon stdout 曾遇共享句柄错误，修正读取方式后复用已启动 A，没有重复启动或 attach。独立原始材料和摘要在 ignored `local-evidence/snowcat-directory-repair-field-20261114T0433/`。

失败语义复核：旧 `closeDetail` 将非 safe-integer 注册返回值的校验失败和 native remove 异常合并成同一 cleanup 原因，可能覆盖 `group detail listener registration`。因此本样本不能证明返回值类型、remove 已调用、详情已派发或详情通知已取得。通过 gh 对固定参考追加核对：Group service 类型声明 number，但生产 `helper/event.ts` 忽略 add 返回并保留代理；研究 `service-entry.ts` 对非 Msg listener 原样传回 remove。后者不是本 Group handle 类型或全局静默的现场证明，不能据此盲目转换 string/number 或放宽注册门。

本轮仅离线修正诊断：未知 handle 保持拒绝且不进行未证 remove，保留原注册失败原因，新增 `group_detail_registration` 只记录返回值类型；合法整数 handle 的 remove 失败继续报 cleanup。Node 回归覆盖 string/object/负数被注册门拒绝、详情派发计数为 0、未调用 remove、后续查询拒绝；已有联合结果/取消/超时/迟到隔离保持通过。脚本入口 impact 仍 UNKNOWN（索引未解析嵌入 JS），按前节 CRITICAL 主链接口边界处理。

`registration-diagnostic-tests.log` 定向 PASS，`final-all.log` 的 default/research 全仓 exit 0；本次未以重跑覆盖现场失败。新隔离 release 构建 exit 0，路径 `target-harness/snowcat-directory-registration-diagnostic-20261114/release/`，完整 hashes 在本轮 `next-builds.json`：CLI `A9AEF05F…DB52DA3` / bridge `6DF64A1F…FFE8149` / core `3248D7D7…D7A2760`。新构建未加载 QQ；当前 21800/3932 保留旧映射，不重接或热卸载。

下一步先在新代次只核实 Group 注册返回类型及适用移除合同，不将此样本说成 GroupInfo 已通过；其余目录/成员本轮未测，原 1440 缺口继续保留。feat-009 in-progress，feat-005/006 blocked，产品 NOT READY。源码快照、源码引用、HTTP/entry/关闭报告与构建日志均本机保留。

### 2026-11-14 依据固定研究合同修复 GroupInfo 查询

用户授权参考私有仓库 `Eternal-Wanderer-Vegetable/Napcat-Snowluma-research`。通过 `gh api` 按完整固定提交 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64` 读取并保存了 Buddy/Group service、listener、API、类型和 OneBot action 原文；tree 为 `7f84554c09ae28ca69b4836ba0560b39357f9adf`，文件 hash 与取证 manifest 在 ignored `local-evidence/research-source-7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/manifest.json`。

固定源码确认 `NodeIKernelGroupService.getGroupDetailInfo(groupCode, GroupInfoSource.KDATACARD)` 的 Promise 为 `GeneralCallResult { result, errMsg }`；详情由 `NodeIKernelGroupListener.onGroupDetailInfoChange` 交付。`GroupApi.fetchGroupDetail` 联合 `ret.result === 0` 与匹配 groupCode 的通知。旧 Caligo 把 Promise 直接当平铺详情读取，这是已定位的接线错误；历史现场只保存外层 HTTP body，因此本修复尚不能证明已解决该现场所有 1440。

最终实现使用每请求独立 listener，先注册再单次调用 `getGroupDetailInfo(groupCode,2)`，联合受理成功和匹配详情后才完成；其他群通知忽略，不新增仅为 9.9.37 描述的 List 回调兼容。回调绑定原 pending、账号/nonce/代次/epoch/session 与 deadline；完成/拒绝后 owner 移除 listener，取消/超时或移除未知则阻止后续详情调用，避免迟到无请求 ID 通知满足新请求。已完成但未 poll 的请求再取消/重绑不会误置永久隔离。字段、ID、预算及 IPC/OneBot 失败语义不放宽。

新增 research-only `query_diagnostics.rs` 和 `qq_entry` 内部报告接线：失败只记录固定原因码、字段类型和受限数量，最多 8 条/每条 16 字段/2,048 bytes；原始异常文本、身份、凭据、路径不进入日志或公开响应。好友/群列表与成员只补观测点，没有更改成功解码合同。旧外层 HTTP body 不是 native raw fixture，新的 Node 夹具是固定源码合同合成场景，不冒充现场对象回放。

验证：最终 `detail-final-all.log` 中 `./init.sh -Mode all` exit 0（default/research 全仓），research bridge lib 111/111（含诊断脱敏 11 项），wrapper 7/7；OneBot query 10/10、broker 13/13 亦通过。中间错群过滤断言失败、Windows `node -e` 长度上限 206 均保留；后者改 stdin，不改业务行为。独立审阅发现完成后取消误隔离，已增加完成后取消/换 epoch 回归并修复。GitNexus 精确接口 impact 为 CRITICAL：82 上游符号、128 流程，含 receiver typing 缺口；脚本构造器索引 UNKNOWN，源码和全仓补证，不能宣称完整图覆盖。

15 份参考文件逐一核对固定提交 blob 和本机 SHA-256，原错误 service 哈希清单已保留并纠正。隔离 release 构建 exit 0，目录 `target-harness/snowcat-directory-repair-20261114/release/`；最终补充 typed decoder 静态诊断后 worker/pipe 9/9 和 release 再构建通过；CLI/bridge/core hashes 为 `5347632C…A0766EA` / `F613C122…3EC9CE8` / `C3D62C3D…3238967`，完整值在 ignored `repair-builds.json`。原始源码、15 项 provenance、测试与构建日志均本机保存。

本次未执行 QQ 动作，旧 21784/41548 不重接、不热卸载。下一步新代次准入后单次复验 GroupInfo，并按剩余失败的固定阶段采集本机诊断；好友/成员 1440 未关闭，feat-005/006 blocked 保持。文档和 diff 收尾另记，不以构建成功替代现场通过。

### 2026-11-14 新代次双账号 LoginInfo 现场

用户正常重启两个 QQ 后，本轮先只读确认新根：A PID `24376`（创建 `2026-11-14T02:48:12.5746089Z`，主根）与 B PID `46664`（创建 `2026-11-14T02:48:14.6872088Z`，`--from-multiple-login` 子根），均响应正常，旧 PID 已退出。两根的 `QQNT.dll`、`major.node`、`wrapper.node` 分别匹配固定 9.9.36-53644 profile：`75B19E58…5086E5`、`4564E402…4A07`、`0D8B9444…D534C`；两边均无 `caligo_bridge.dll`、Snowluma 或未知 `.tmp` backend。PE-export-aware 审计脚本 hash `2238D736…B6855D4` 对两根均返回 `PASS_KNOWN_BACKEND_AND_TEMP_MODULE_GATE`，仅列出合法签名 Windows Defender `MpOav.dll`。固定 manifest verify PASS；envscan 对 A/B 各得到 1 个候选、0 read failures，候选地址仅留本机。

当前工作树隔离 release 构建成功，材料目录为 ignored `local-evidence/snowcat-dual-login-fresh-20261114/`，构建 hash 记录在 `state.private.json`。使用独立 pipe prefix、journal、token 和 HTTP listener 启动两个 `caligod`；A/B 各 attach 一次均 PASS，entry log 均记录 `env_fresh`、`first_entry`、`listener_armed`。身份绑定使用已有独立资料卡与新根启动参数：A 为 `登记账号 A 及其昵称`，B 为 `登记账号 B 及其昵称`。两次 `get_login_info(no_cache=true)` 均 HTTP 200 / retcode 0 且 user_id/nickname 匹配；A 首次响应因 runner 用系统默认编码读取中文 JSON 而解析失败，但原始 HTTP 200 UTF-8 响应已保存，未重发 A；runner 修正为显式 UTF-8 后只查询未执行的 B，B 通过。

交叉 token 返回 403，缺 token 返回 401；停止 A daemon/owner 后 B 再查询仍 HTTP 200 / retcode 0，返回 `登记账号 B 及其昵称`。随后 B daemon/owner 也正常关闭。收尾时 A/B QQ 仍响应，各保留本轮一份 bridge，A/B HTTP listener 均已关闭，无 `caligod` 进程。全程 `messages_sent=0`。该样本证明新代次正常入口的 self LoginInfo、双账号认证隔离和停止一端不影响另一端；不证明消息事件/账本、同时在途查询、core 重启期间取消/迟到回调、QQ 网络重连、同进程换号、全路径身份撤权、媒体/SSO 或长期静默。原始证据与脱敏摘要仅在 ignored `local-evidence/snowcat-dual-login-fresh-20261114/`。

### 2026-11-14 R1b core 重启恢复 LAB 与 QQ 现场预检

本工作块按当前工作树重跑 `cargo test -p caligo-core --test core_restart_query_lab --features research --locked --offline --target-dir target-harness`，LoginInfo 与 SelfProfile 两项均 PASS（2/2）。夹具真实启动两个 `caligod` 进程并复用稳定 token/pipe；第一 core 终止时 owner 保持暂停，旧请求没有 poll，重连后以相同数值 scope 接受新请求。两次 query ID 使用不同 core nonce，旧请求由合法 owner 取消恰一次，fresh 请求在放行完成标志后返回成功；没有额外 owner bootstrap、native dispatch 或协议错误。合成 HostAdapter 只记录取消，不证明 QQ native listener/callback、网络状态、换号、消息或长期静默。

本轮对用户授权的现有双 QQ 根只读核对：A PID 46620，创建时间 `2026-11-14T01:25:57.5425320Z`；B PID 35780，创建时间 `2026-11-14T00:24:01.4962797Z`。两者均为响应正常的 QQ 根窗口，加载的 `QQNT.dll`、`major.node`、`wrapper.node` SHA-256 分别为 `75B19E58…5086E5`、`4564E402…4A07`、`0D8B9444…D534C`，与 9.9.36-53644 现场 profile 相符；但 A/B 各自已有一份 `caligo_bridge.dll` 映射，hash 分别为 `F1A128B6…911C` 和 `309EADD9…A85`，且 PID/创建时间属于已有现场代次。根据停止合同，本轮没有 attach、发送、热卸载或停止 QQ，未把响应状态当作新现场通过。R1b 状态因此为 LAB 通过、QQ FIELD 待新鲜无 bridge 代次；R2/R3/长期寿命和产品验收边界不变。原始现场材料继续只保留在 ignored `local-evidence/`。


### 2026-11-14 私聊 PNG 入站现场缺失与诊断准备

本次从普通文本验收继续，范围仅为 B 到 A 的单张普通 PNG：SSE 图片事件、持久 ID、现场 HTTP `get_msg` 与 `get_image` 文件校验。接管前已有采集器运行；用户明确同意接管现有一轮，没有重新接入或发起额外发送。A PID 39320（创建 `2026-11-14T08:07:53.9878027Z`）的原始预检通过固定 manifest、实际三角色模块 hash、PE/backend 和唯一 envscan；使用文本修复 release 的 bridge `B82C45A2EC0780B902BCA6C6FD63C2532631782E6F24B387C73EC4E1B060A595` 单次接入，LoginInfo 与 A 匹配。接管时重新核对三项构建 hash 均与 pin 一致。原始材料与测试图片仅在 ignored `local-evidence/snowcat-private-png-receive-20261114/`。

用户确认 B 以图片原图方式发送一次，A 在 16:16（Asia/Shanghai）采集器关闭前已显示该图。原图 96×64、18,074 bytes，SHA-256 `38d6495f5ad85dc279320588677b7f3d4abc58e747260ccd233ea62c0ed37d62`。采集器到期时没有目标私聊图片 SSE，持久消息账本也没有 B 到 A 的私聊图片；未取得该样本的持久 ID，因此没有执行其 `get_msg` / `get_image` 或下载比较。账本中同期的群图片不作为该私聊样本。本轮私聊 PNG 入站现场**未通过**，不能归为用户未发送或窗口外发送，也不能从 QQ 显示推出 Caligo 收录成功。

原 runner 在 `2026-11-14T08:16:20Z` 正常关闭 daemon 与 owner，两项 exit 0，owner report 记录 `closed via owner pump`；QQ 响应且保留一份 bridge。接管后额外核对 HTTP/SSE listeners 均 0、原采集器已退出。没有重放发送，没有重接该代次或热卸载。结论与停止结果见本机 `takeover-summary.private.json` 和 `A/takeover-final-health.json`。

离线排查确认入站门只接受 `picType=1001`，自发 PNG 回读按既有合同允许 1000/1001；本样本实际字段未采集，不能将此差异认定为根因。为下一轮补充 research-only 的图片 callback 形状诊断：D8 在既有接收判断前记录固定枚举和布尔校验结果，涵盖来源、会话类别、元素数、格式分类、原始字符串字段、身份关系和尺寸/MD5 等形状；不收集身份值、消息 ID、路径、MD5 值、正文、文件或凭据。每次 poll 最多 8 个形状，Rust 再白名单过滤，内部阶段报告 `receive_image_shape` 每进程最多 32 次，并保留有界的 JS 接受/拒绝/溢出计数。报告的 ok 表示诊断记录，不是消息成功。诊断不放宽接收/下载/发送门，不改变 D8 owned 记录、IPC 或公开 OneBot 结果。

改动为 `qq_v8.rs`、新增 `receive_diagnostics.rs`、research 模块声明与 Node 夹具。GitNexus 显式绑定 `E:/stella/Caligo` 和 HEAD `5665b2b`；初始旧图把 D8 常量列为 CRITICAL，已预警。index-only 刷新后常量 impact 为 UNKNOWN（常量引用没有边），已文本核对 listener account 包装、owner poll、D9 发送、图片/语音与核心链路测试；`poll_listener_json` 的图为 1 个直接 caller / 1 flow / LOW，实际调用在正常 daemon owner hook。FTS 不可用使概念 query 为空，不当作无影响。图与刷新日志仅留本机。

定向 Windows research 图片诊断夹具通过，bridge lib 首轮 115/115 通过；覆盖现有 1001 接纳 / 1000 拒绝不变、错误头字段/混合元素/错 peer 可区分、诊断预算不影响接收、getter 异常被容纳、敏感值和任意键被过滤。额外真实管道 `wrapper_image_chain` 1/1 通过，属于合成宿主 LAB，不替代失败现场。第一次 `init.ps1 -Mode all` 由 Windows PowerShell 5 内部重定向触发 native stderr/WriteError 退出，保留 `all-diagnostics.log`；使用 PowerShell 7 和外层捕获执行正式全仓检查。最终检查与隔离构建信息在本节收尾追加。


收尾验证：PowerShell 7 执行 `$env:RUST_TEST_THREADS='1'; .\init.ps1 -Mode all` exit 0，default/research 全仓通过（`all-diagnostics-final.log`，原有 ignored 项保留），research bridge lib 115/115。仅有定向通过后给诊断报告补接受/拒绝计数，最终全仓已包含该版本和隐私/预算用例。隔离 release 构建 exit 0，路径 `target-harness/snowcat-image-diagnostic-20261114/release/`，bridge SHA-256 `72EB54F584020ABE3FD899E48A8E0BD32015C9480B6A54A8D94BC1F1CC407C5B`；CLI/core pin 及新 PNG 仅在 ignored `local-evidence/snowcat-private-png-diagnostic-20261114/`。新构建未加载。该目录 `run-sample.ps1` / `run-field.ps1` 以新指定 A PID + 创建时间启动，单账号独立目录/随机 token/pipe，采集窗口 15 分钟，预检、单次接入、LoginInfo、SSE、get_msg/get_image、finally 关闭按顺序执行；PowerShell 两脚本解析、Python 编译及新增 Rust 模块格式检查通过。新图与旧图 hash 不同；只能在 SSE 就绪后发送一次新样本，不自动重发旧图。

已审阅本轮独立 patch 与新增夹具，开始时既有未提交改动保持；本轮 source SHA、独立 patch、图/测试/构建日志留本机。Windows `init.ps1 -Mode docs` 和 Windows Git `git diff --check` exit 0（行尾转换 warnings 保留）；WSL Git 在原有历史文本中另报尾空白，未改写历史证据。进度、feat-008 和 handoff 已记录失败，状态不提升，未提交或推送。下一步需用户正常退出并重新登录 A，以新进程代次重新核对身份/模块/准入后使用诊断版；B 可保留。新现场前不再接入 39320，不热卸载，不把诊断准备当 PNG 修复或现场通过；其他媒体、协议与恢复场景仍待其各自验收。


### 2026-11-14 私聊 PNG 入站回读通过与下载超时

用户正常重启指定 A 后，旧 39320 已退出；新主根 PID 46312（创建 `2026-11-14T08:29:11.5670016Z`）为唯一新根，既有未指定根排除。诊断版 bridge `72EB54F584020ABE3FD899E48A8E0BD32015C9480B6A54A8D94BC1F1CC407C5B`、CLI/core pin 与已测源码 snapshot 一致；当次三角色实际 loaded hashes、固定 manifest、PE/backend 审计及唯一 envscan 通过。按正常 daemon/IPC 单次接入，LoginInfo HTTP 200 / 0 与 A 相符，再订阅 SSE。原始材料仅在 ignored `local-evidence/snowcat-private-png-diagnostic-20261114/`。

用户从 B 单次向 A 私聊发送本轮新 PNG 原图，并确认 A 显示。SSE 图片事件和现场 HTTP `get_msg` 均通过：账号、发送者、private/friend、平台时间、持久消息 ID、账号限定图片标识和消息段一致。消息账本恰有一条该样本，图片 size=18,569、96×64 与测试原图相符，MD5 亦匹配。入站诊断显示 `pic_type=png`（1001）、single/ordinary、字符串字段、会话/发送者关系、元素 ID、大小/尺寸/MD5 校验均通过，无接收溢出。后续同图 update 被既有入站门拒绝，不构成重复消息。本轮证明指定样本的私聊 PNG 入账及 HTTP 消息回读；上一轮 39320 缺失样本的原因没有由此确定，不能称其已被诊断代码修复。

同一持久图片标识的单次 `get_image` 返回 HTTP 200 / `retcode=1501`，wording 为宿主查询超时；未取得 PNG 字节，因此 SHA-256/完整下载验收未通过。下载创建了当轮唯一临时目录，停止后只读检查该目录无文件；目录继续保留，不把超时当 native 工作终结，不重试该请求。字段诊断不能区分 Promise 受理、回调未到、回调匹配失败或较晚完成。固定研究提交 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64` 的 file API、Msg listener/service 三个源码 blob 与本地文件逐项一致，确认参考也使用 downloadRichMedia 与 onRichMediaDownloadComplete；参考收到路径和较长等待并不证明 Caligo 本样本应放宽精确目标路径或超时。

runner 在 `2026-11-14T08:50:41Z` 正常执行 finally，daemon/owner exit 均 0，QQ 响应且保留一份 bridge。后续核对 HTTP/SSE listener 均 0；没有重新接入或热卸载。独立核对结果为本机 `field-summary.private.json`，停止与遗留目录材料在 `A/`。本轮整体为部分通过，feat-008 保持 in-progress，其他媒体/协议/恢复的既有未验边界不变。

下一轮仅补图片下载诊断，不改原生参数、超时、回调匹配或文件验证/清理语义：记录 dispatch、Promise 的固定受理分类、callback 的消息/元素/会话/路径/错误类型/slot/在途比较布尔，以及文件验证阶段，不保留被比较的值、路径或异常原文。每次 D8 poll 最多 8 项，Rust 再按固定标签/布尔白名单过滤，内部 `image_download_stage` 每进程最多 32 次。诊断队列独立于 pending 查询，允许超时后继续观察回调，但不凭观察窗口判定 native 静默。共用下载 helper 对语音不新增此诊断；接收与公开动作合同保持。

GitNexus 延用显式 `E:/stella/Caligo` / `5665b2b` 绑定：query start/poll builder 与 owner poll 的 impact LOW，D8 poll 常量 UNKNOWN 已从 raw JS 常量引用、owner hook 与现有图片/语音调用点补查；概念查询仍因 FTS 缺失为空。新增 Node 验证下载 Promise、错路径回调可观测、诊断 drain 不改变 pending/timeout/迟到清理；Rust 对敏感值、未知标签、错误字段类型和长度预算过滤。定向 research wrapper_image_contract 4/4 和诊断单测 3/3 通过。最终全仓、隔离构建和收尾检查另记于本节末。


收尾：PowerShell 7 串行 `init.ps1 -Mode all` exit 0，default/research 全仓通过，research bridge lib 116/116，原有 ignored 项保留；本轮没有新增失败（`download-diagnostic-all.log`）。隔离 release 构建 exit 0：`target-harness/snowcat-image-download-diagnostic-20261114/release/`，bridge SHA-256 `F293E1577ECA40D11AF68F4B5EF1BED6313AAB0306BD141DE302067E984AB56B`，新构建尚未加载。新的单次现场脚本、不同 hash 的 PNG 与完整构建 pins 在 ignored `local-evidence/snowcat-private-png-download-diagnostic-20261114/`；新 A 的 PID/创建时间为必填，PE/backend/manifest/唯一 envscan、LoginInfo 与 SSE 依次通过后才由用户发送新图。若 get_image 失败，不重试，仅保持 owner 正常 poll 额外 30 秒观察迟到回调，再由 finally 正常停止 daemon/owner。30 秒不是 native quiescence 证据；slot 与未知工作仍按原合同保留。PowerShell 语法、Python 编译和新增 Rust 模块格式检查通过。图、增量 patch、当前源 hash 与本轮失败样本均保存；没有改写之前的现场记录，也没有提交或推送。最终 docs/Windows diff 检查记录于 `download-final-check.log`。下一次需用户正常退出并重新登录指定 A，旧 46312 不重接、不热卸载；B 可保持在线。当前只证明入站事件/持久消息回读，完整图片下载、其他媒体与其余协议/恢复验收仍未完成。


### 2026-11-14 PNG 下载路径门复现与缓存路径适配

用户重启指定 A 后，旧 46312 退出，新唯一 QQ 主根 20292（创建 `2026-11-14T09:00:42.6151576Z`）与既有未指定根区分。已测源码 snapshot 与隔离 release pins 一致，固定 manifest、实际 loaded 三角色 hash、PE/backend、唯一 envscan 通过；bridge `F293E1577ECA40D11AF68F4B5EF1BED6313AAB0306BD141DE302067E984AB56B` 单次接入，LoginInfo 匹配 A，SSE 就绪。用户从 B 私聊单次原图发送新 PNG 并确认 A 显示。本轮 SSE、持久入账、现场 get_msg 再次通过，单条账本的大小/MD5/96×64 尺寸与新图匹配。get_image 仍为 HTTP 200 / 1501，随后观察 30 秒而未重试。原始材料仅在 ignored `local-evidence/snowcat-private-png-download-diagnostic-20261114/`。

诊断记录一次 dispatch、Promise 分类 unknown（不是已证拒绝或已证受理），随后出现错误码字符串 0 的下载 callback。callback 的消息 ID、元素 ID、chat、当前 slot 均匹配且请求仍在 deadline 内；只有 `path_matches=false`。旧 helper 因目标路径不等直接 return，保留 slot，最终查询超时。这是本轮明确的忽略点。只读检查 A 的当前月份图片缓存，按测试 MD5 定位到一个文件，18,569 bytes、SHA-256 与本轮原图相同；文件位置只保存在本机 cache-files 报告。没有采集 callback 原始路径，因此不能将该缓存文件路径与 callback 路径直接认定为相等，仍需新代码现场核对。

runner 于 `2026-11-14T09:04:57Z` 正常关闭 daemon/owner，两项 exit 0，QQ 响应并保留一份 bridge；后续核对 HTTP/SSE listener 均 0。没有重新接入、卸载或重发；独立字段/账本/停止核对见 `field-summary.private.json` 和 `A/final-health.private.json`。本轮整体仍为部分通过，未取得正常 get_image 的 PNG 字节，feat-008 保持 in-progress；上一轮整条图片缺失的原因保留 unknown。

离线新增回归让正常宿主下载返回它登记的缓存路径。旧实现出现 pending 而不是 success，定向 exit 101（0 PASS / 1 FAIL），保留 `cache-path-repro-before.log`。随后适配只使用派发前允许的精确目标：图片经已有固定 profile、已用于 PNG 发送的 `getRichMediaFilePathForGuild` API，以消息回读的 MD5 和确定的 `.png` 文件名取得 host cache path，校验绝对规范路径、驱动器、文件名和真实父目录；不从配置推导 QQ 路径，也不采用 callback 自选的任意路径。API 缺失时保留原独立临时目标合同，API 存在但返回无效时拒绝且不派发下载。回调仍须消息/元素/chat/错误码/slot/请求代次匹配，只能读取原临时目标或派发前登记的缓存文件；图片按 Windows 路径语义比较，读取使用登记路径。保留 regular file、realpath、打开前后 file identity/长度、MD5 和 PNG signature/IHDR 尺寸验证。只清理 Caligo 自建临时目录，不删除 QQ 缓存；超时后的精确回调只收尾，不恢复旧查询。语音保留原临时目标及严格路径比较。

新增 `registered_path_matches` 布尔诊断，不记录路径值。共用 helper 的调用图仍按显式 Caligo / `5665b2b` 绑定：query builder LOW，raw JS 常量与测试无上游边按 UNKNOWN，已从 builder 拼接、owner hook 和图片/语音夹具补查；没有将缺边当无影响。改动为 qq_query.rs、receive_diagnostics.rs 与 wrapper_image_contract.rs；初版适配后 research 图片合同 4/4 通过（`cache-path-repro-after.log`）。最终回归还覆盖合法缓存完成、缓存损坏、无效/未登记路径、缓存保留和超时迟到收尾，不只验证成功分支。最终全仓、隔离构建与文档检查见本节末；这是 implemented/LAB 适配，当前 QQ 尚加载旧诊断版，不能提前宣布下载现场修复。


最终检查：PowerShell 7 串行 `init.ps1 -Mode all` exit 0，default/research 全仓通过，research bridge lib 116/116，图片合同 4/4，原有 ignored 项保留；预期的修复前 1 FAIL 保留，没有新失败。隔离 release 构建 exit 0，`target-harness/snowcat-image-cache-path-repair-20261114/release/`，bridge SHA-256 `75AE2D7393341C5D3AF40C1307E5DF129BD52FE4CCC4568FCC6635B9674A375F`，未加载。新原图、脚本和完整 pins 在 ignored `local-evidence/snowcat-private-png-cache-path-repair-20261114/`；脚本要求新指定 A 的 PID/创建时间，准入后单次接入、LoginInfo、SSE，再发送新图一次；get_image 失败仍只观察迟到、不自动重试。PowerShell 脚本解析、Python 编译和改动模块格式检查通过。本轮独立 patch/source SHA、修复前后和全仓/构建结果在前一 field 目录的 `cache-path-*` 文件，已审阅改动并保留开始时其他 dirty 内容。最终 docs/Windows Git diff 检查记录于 `cache-path-final-check.log`；未提交、推送或发布。新 A 现场 get_image 与原图 SHA-256 仍是下一验收门，旧 20292 不重接或热卸载；需要用户正常重启指定 A，B 可保持在线。


### 2026-11-14 私聊 PNG 正常下载现场通过

用户重启指定 A 后，旧 20292 退出。唯一新主根 44108（创建 `2026-11-14T09:17:01.6707059Z`）与既有未指定根区分。沿用前节全仓通过的源码 snapshot、隔离修复 release，bridge SHA-256 `75AE2D7393341C5D3AF40C1307E5DF129BD52FE4CCC4568FCC6635B9674A375F`；当次固定 manifest、实际 loaded 三角色 hashes、PE/backend、唯一 envscan 均通过。正常 daemon/IPC 单次接入，LoginInfo HTTP 200 / 0 匹配 A，SSE 就绪。原始身份、请求、事件、账本、图片及阶段记录仅在 ignored `local-evidence/snowcat-private-png-cache-path-repair-20261114/`。

用户从 B 向 A 私聊单次发送本轮新 PNG 原图，并确认 A 显示。SSE 图片事件、持久入账和现场 HTTP get_msg 均通过：账号、发送者、private/friend、持久消息 ID、平台时间和消息段一致，图片标识绑定 A 与同一 ID。账本恰有一条本样本，图片大小、MD5、96×64 尺寸与原图一致。随后对该标识单次调用正常 OneBot HTTP get_image，HTTP 200 / retcode 0，返回 18,569 bytes；base64 严格解码后的长度、文件大小、PNG 尺寸与原图相符，SHA-256 为 `a98ac89666b9616ced43833d2c46ff3cc0ab102c8f25113dcd843ca603f6e919`，与本轮原图完全一致。独立核对再次比较原图、响应解码和下载文件，三份字节相同。

下载阶段显示 dispatch 后 Promise 分类 unknown；随后零错误 callback 的消息/元素/chat/slot/deadline 匹配，原独立临时路径仍不匹配，但 `registered_path_matches=true`，继而进入 validated。结合正常 HTTP 返回文件，这证明派发前登记路径的适配在本 QQ 样本贯通；没有将 unknown Promise 单独当成功。用户只发送一次，get_image 未重试。本轮是固定构建 B 到 A 的单张普通私聊 PNG 入站、消息回读与完整文件下载现场通过；不是主动 PNG 发送、群图片、其他格式、长期缓存/恢复或完整媒体发布验收，也不倒推此前整条图片缺失样本的根因。

runner 于 `2026-11-14T09:20:09Z` 正常关闭 daemon 和 owner，两项 exit 0，QQ 响应且保留一份 bridge；后续独立核对 HTTP/SSE listener 均 0。原始采集 summary 与独立结果为本机 `A/summary.json`、`field-summary.private.json` 和 `A/final-health.private.json`。旧失败样本、目录和诊断均保留，不重放、不热卸载。本轮未改 Rust 或二进制，未重复运行前节已通过的相同 Cargo 套件。

下一项已离线准备登记测试群的 B 到 A 单张普通 PNG。沿用同一已通过修复 build/pins；本机 `local-evidence/snowcat-group-png-receive-20261114/run-sample.ps1` 要求新指定 A PID/创建时间，使用独立目录/token/pipe 和新图，沿用模块/backend/envscan/LoginInfo/SSE 准入，严格过滤登记群、发送者与接收账号，依次核对 get_msg/get_image 和文件哈希；只在 SSE 就绪后由 B 在登记群发送一次，不自动重发。登记群取自此前已验收文本发送的本机计划，不猜群号；尚未启动 daemon、接入或发送。本机脚本通过 PowerShell 语法与 Python 编译检查。下一轮需新 A 代次，旧 44108 不重接或热卸载；B 可保持在线。feat-008 保持 in-progress，其他媒体/协议/恢复验收边界不变。最终 docs/Windows diff 检查结果保存在本轮 `final-docs-check.log`，未提交、推送或发布。


### 2026-11-14 登记群 PNG 入站与下载现场通过

用户重启指定 A 后，旧 44108 退出，新唯一 QQ 主根 47904（创建 `2026-11-14T09:24:23.6288744Z`）与既有未指定根区分。沿用已测源码 snapshot 和缓存路径修复版 release，bridge SHA-256 `75AE2D7393341C5D3AF40C1307E5DF129BD52FE4CCC4568FCC6635B9674A375F`，三项构建 pins 一致。当前 manifest、实际 loaded 三角色 hashes、PE/backend、唯一 envscan 均通过，正常 daemon/IPC 单次接入，LoginInfo HTTP 200 / 0 匹配 A，SSE 就绪。原始数据、群号与图片仅保存在 ignored `local-evidence/snowcat-group-png-receive-20261114/`。

用户从 B 在登记测试群单次原图发送本轮新 PNG，并确认 A 在群内显示。SSE 的 group/normal、接收账号、发送者和群号与本机登记计划相符；持久账本恰有一条该样本，其 account、group peer、chat_type=2 与图片 sender、MD5、18,569 bytes、96×64 尺寸一致。现场正常 OneBot HTTP get_msg 返回 200 / 0，持久消息 ID、消息段、群聊类型、发送者与平台时间匹配 SSE。随后同一账号限定图片标识的 get_image 返回 200 / 0，base64 严格解码、文件大小/PNG 尺寸和原图相符，SHA-256 为 `7e8aabc422cf6a961816a51abfe4b6e0bc879f13533b9fc52efb8fdce39cb896`。独立核对再次比较原图、响应解码与下载文件，三份字节完全相同，并确认登记群与账本目标一致。

下载诊断显示匹配当前消息/元素/chat/slot/deadline 的零错误 callback，原临时路径不匹配，预先登记路径匹配且 validated。用户只发送一次，下载未重试。runner 在 `2026-11-14T09:26:21Z` 正常关闭 daemon 和 owner，两项 exit 0，QQ 响应并保留一份 bridge；独立核对 HTTP/SSE listener 均 0。采集与独立结论见本机 `A/summary.json`、`field-summary.private.json` 和 `A/final-health.private.json`。未改 Rust 或构建，复用前节已通过的相同二进制和 Cargo 验证，没有重复运行无变化的全仓套件。

结合上一轮私聊，本固定构建已有普通单 PNG 的 B 到 A 私聊与登记群入站、持久消息回读和正常文件下载现场通过样本。尚未证明 Caligo 主动 PNG 发送、其他媒体/格式、缓存长期寿命、恢复或完整 N6；feat-008 保持 in-progress。历史缺失、超时和 unknown 记录保留，不以本次成功覆盖。

下一块已准备 A 到 B 的单次主动私聊 PNG：本机 `local-evidence/snowcat-private-png-send-field-20261114/` 有新测试图、精确 OneBot send_private_msg 请求、request SHA、build pins、正常准入/启动/发送/回读/关闭脚本。只发单个不超过 128 KiB 的 Base64 PNG 段，返回持久 ID 后才核对 get_msg、账本的 B 目标、get_image 与输入字节；unknown 不重发，异常只留 30 秒观察再 finally 关闭。该块尚待用户明确的一次工具发送授权与新 A 代次。driver 和发送脚本都检查 request-hash 绑定的单次 authorization，持久独占 reservation 在唯一发送 POST 之前落盘并 fsync；没有授权文件则拒绝。PowerShell 解析与 Python 编译通过，实际执行无授权 guard 测试在网络/预检/daemon/消息发起前拒绝，未启动或发送。旧 47904 不重接或热卸载，B 可保持在线。最终 docs/Windows diff 检查见本轮 `final-docs-check.log`；未提交、推送或发布。


### 2026-11-14 主动私聊 PNG 发送前会话未就绪

用户已明确授权测试消息发送。首次指定新 A 51188（创建 `2026-11-14T09:34:17.0998646Z`）通过固定 manifest、loaded hashes、PE/backend 与唯一 envscan，单次接入 exit 0。正常 LoginInfo HTTP 200 / retcode 1502，data 为空；内部 listener 报 `ERR:no live session`。driver 的通用异常文案为 identity mismatch，但本响应只证明查询尚未就绪，没有证明登录了错误账号。发送脚本尚未进入，发送 reservation 与发送请求文件均不存在，实际 send_private_msg 提交数为 0；授权未消耗，不属于 unknown 发送。材料仅在 ignored `local-evidence/snowcat-private-png-send-field-20261114/`。

finally 正常停止 daemon/owner，两项 exit 0，QQ 响应且保留 bridge；另核对 HTTP/SSE listener 均 0。用户随后确认已进入聊天主界面，仍未对该已停止 owner 的代次重新接入。下一 runner 补 30 秒有界 owner 报告等待，先出现 listener_armed 再执行唯一 LoginInfo；明确 listener 错误立即停止，no-live 等待到 deadline，失败均不发送。合成就绪/错误/超时/半行四场景通过，三份脚本解析、Python 编译通过，已授权请求 hash 与上一未提交请求相同。另修正 runner 将非零 LoginInfo 与真实身份不匹配分开报告，没有改 Rust 或原生调用。


### 2026-11-14 主动私聊 PNG 发送可见与下载类型门

用户正常重启并确认 A 已进入聊天主界面、B 在线。旧 51188 已退出，新唯一主根 12100（创建 `2026-11-14T09:42:24.9931246Z`）无 bridge，与既有未指定根区分。已测源码 snapshot、三项 build pins、固定 manifest、实际 loaded 三角色 hashes、PE/backend 与唯一 envscan 均通过；沿用缓存路径修复版 bridge `75AE2D7393341C5D3AF40C1307E5DF129BD52FE4CCC4568FCC6635B9674A375F`，正常 daemon/IPC 单次接入。owner 的 listener_armed 先于唯一 LoginInfo，HTTP 200 / 0 与 A 匹配。原始身份、请求、平台 ID、账本和图片仅在 ignored `local-evidence/snowcat-private-png-send-ready-field-20261114/`。

在用户授权内，发送脚本先将 request-hash 绑定的独占 reservation 落盘并 fsync，再经正常 OneBot HTTP 提交一次 send_private_msg，目标为登记 B，消息仅含一个 18,569 bytes、96×64 的合成 Base64 PNG 段。HTTP 200 / retcode 0 返回持久消息 ID；随后 get_msg HTTP 200 / 0，private、A 发送者、平台时间、同一 ID 与账号限定图片段一致。账本目标 B、账号 A、图片 MD5/大小/尺寸与计划匹配。用户独立确认 B 已显示这张图片。独立脚本复核通过，未重复发送，未测量发送到显示时延。该样本证明本固定构建的单次普通私聊 PNG 发送、持久消息回读和对端可见性；不证明完整发送媒体文件下载闭环或其他场景。

后续同一图片标识的单次 get_image 返回 HTTP 200 / 1501 宿主查询超时，没有取得返回 PNG 字节。下载诊断出现零错误 callback，消息/元素/登记路径/slot/deadline 均匹配，只有 chat_matches=false；因此当前 helper 忽略该回调并保留在途 slot。现有诊断未采集 chatType 的实际值或 JS 类型，不能判断是数字差异、类型差异还是缺字段，不能据此删除校验。等待额外 30 秒没有重试或重发；finally 于 `2026-11-14T09:44:00Z` 正常停止 daemon/owner，两项 exit 0。独立核对 QQ 响应、HTTP/SSE listener 均 0，bridge 保留。详见本机 field-summary.private.json 与 A/final-health.private.json；失败和 unknown 状态保留，feat-008 仍 in-progress。

下一块仅补下载 callback 的 chatType 固定分类诊断，不改请求、比较、文件校验、超时或资源寿命。分类只允许 private/group/zero/string_private/string_group/missing/null/other；Rust 再按固定枚举过滤，每次 poll 最多 8、每进程 32 的原预算不变，不记录任意值或异常原文。固定研究仓库 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64` 的三个 file/Msg 源码 blob 与本机文件核对一致；参考的回调筛选只按消息和元素 ID，不能作为放宽 Caligo 身份门的证据。

GitNexus 显式绑定 Caligo / `E:/stella/Caligo` / `5665b2b` 并刷新：builder、sanitizer 与合同测试 impact 报 MEDIUM，但包含宽泛 File 依赖，不能视为精确 JS 调用链。COMMON_JS 字面量 UNKNOWN，已由拼接 builder、正常 owner hook、图片/语音共用 helper 和 Rust poll sanitizer 文本补查。FTS 缺失、跨语言属性和流程预算截断保留为覆盖限制，不以空 query 证明无影响。材料与本轮增量源码 snapshot 在 ignored `local-evidence/snowcat-sent-png-chat-diagnostic-20261114/`。定向 research 图片合同 4/4、诊断过滤 3/3 通过，包括八类不匹配回调继续 pending、固定分类、任意值不出诊断、原成功和寿命合同。全仓检查、隔离 release 与最终脚本检查结果在本节末追加。

收尾验证：PowerShell 7 串行 `init.ps1 -Mode all` exit 0，default/research 全仓通过，research bridge lib 116/116，原有 ignored 项保留；定向图片 4/4 与诊断 3/3 已包含本轮最终版本，没有新增失败。隔离 release exit 0，路径 `target-harness/snowcat-sent-image-chat-diagnostic-20261114/release/`，bridge SHA-256 `D5134337FEABD4C0D10EEC0F708469188D9BD91CFEABA2615DDA4F1EA6FA164C`，完整 pins/source snapshot 与增量 patch 在上述本机目录。新构建未加载，不把诊断准备当下载修复。

下一轮的 run-sample.ps1/query-once.py 只接受新指定 A PID/创建时间，独立 token/pipe/端点，使用本轮已停止账本的逐字节副本与明确 read-plan；正常准入、listener_armed、唯一 LoginInfo 后只执行一次 get_msg 与一次 get_image。读取已有 PNG 的原持久 ID，返回字节才比较原图；异常保留 30 秒观察并 finally 停止，不调用任何发送动作。重新启动 core 读账本副本属于本诊断设置，不据此升级生产恢复验收。脚本 PowerShell 解析、Python 编译、源码/构建 pins 和账本副本检查通过。最终 `init.ps1 -Mode docs` 与 Windows Git `diff --check` 结果在本机 final-check.log；开始时既有 dirty 内容保留，未提交、推送或发布。下一轮需用户正常重启 A 并进入聊天主界面，旧 12100 不重接或热卸载，B 可在线；已发本图不重发。群 PNG 主动发送、其他媒体/协议与恢复仍待其各自验收。


### 2026-11-14 已发送 PNG 只读诊断确认空会话类型

用户正常重启 A 并确认聊天主界面；旧 12100 已退出，唯一新主根 43644（创建 `2026-11-14T09:52:52.2708416Z`）与未指定根区分，无旧 bridge。沿用前节已测诊断构建，bridge SHA-256 `D5134337FEABD4C0D10EEC0F708469188D9BD91CFEABA2615DDA4F1EA6FA164C`，source snapshot 与三项 pins 一致；固定 manifest、loaded hashes、PE/backend 与唯一 envscan 通过。正常 daemon/IPC 单次接入、listener_armed 与 LoginInfo HTTP 200 / 0 均通过。

本轮只读本地停止账本副本中的原持久消息 ID：get_msg HTTP 200 / 0 与已发私聊 PNG 的账号/发送者/ID/图片段一致；单次 get_image 仍 HTTP 200 / 1501。新诊断明确 callback_chat=null，而消息/元素/预登记路径/错误字符串 0/slot/deadline 均匹配；因此已定位为实际 null 字段，不是字符串数字或缺字段。此回调形状仅在精确 build 的本张自发普通私聊图片得到现场证据，不推及入站、群聊、语音或其他构建。没有取得下载响应字节，没有发送、重发或重试。原始材料在 ignored `local-evidence/snowcat-sent-png-chat-diagnostic-20261114/`，含 field-summary.private.json。

观察 30 秒后正常关闭 daemon/owner，两项 exit 0；独立核对当前 QQ 响应、HTTP/SSE listener 均 0，诊断 bridge 保留。新 core 使用账本副本不作为生产恢复验收。本轮下载仍未通过，feat-008 in-progress；前轮 PNG 已发送且 B 可见的事实保留。下一步离线复现该形状，评估仅对 hash 绑定、自发 private/picType=1000 且回调返回预登记精确缓存文件的 null 类型适配；消息/元素、请求代次和文件完整性不能放宽。任何新现场仍需新 A 代次，不重接或热卸载 43644，不再发送本图。


### 2026-11-14 自发私聊 PNG 空类型缓存回调适配

依据上一节精确 hash 现场的 null 类型回调，在普通图片下载合同中新增同形状回归：原实现保持 pending 而非 success，定向 exit 101（0 PASS / 1 FAIL），保留 `local-evidence/snowcat-sent-png-null-chat-repair-20261114/repro-before.log`。随后只在 hash 验证的 9.9.36-53644 pilot、private chatType=1、自发 sender=当前 query scope 账号、已读回 picType=1000，以及 callback 路径等于派发前登记的 QQ 缓存文件时允许 chatType=null。该分支不接受 undefined、字符串类型、任意数字、群聊、入站、picType=1001、未验证 profile 或原临时路径的 null 回调。

前置 getMsgsByMsgId 的精确消息/peer/sender/元素、已发送状态和元数据校验不变；callback 的消息/元素/错误字段/slot/代次/deadline，打开后的 regular/realpath/fd 身份/大小/MD5/PNG 尺寸校验均保留。缓存路径仍须 host API 预登记，只清理 Caligo 自建临时目录，不删除 QQ 缓存。迟到/取消只按原合同收尾，不复活旧查询。语音和其他图片分支仍严格比较 chatType。源码改动只涉及 qq_query.rs 的完成校验和 wrapper_image_contract.rs；receive_diagnostics.rs 无改动。

GitNexus 在显式 Caligo / `E:/stella/Caligo` / `5665b2b` 下刷新后，builder impact LOW，COMMON_JS 与合同测试 UNKNOWN，经 builder 拼接、owner、图片/语音共用 helper 和夹具文本补查；FTS 与跨语言/流程预算限制保留。共享 sanitizer impact 为 CRITICAL，已明确提示且本次不修改它。图、原始失败、前后源码 hash 和本次增量 patch 仅留上述 ignored evidence 目录。修复后的定向回归 exit 0；新增拒绝场景覆盖错误消息/元素/路径/类型、未知 profile、入站/群聊/1001，以及损坏文件；并验证成功字节、缓存保留、超时/取消不复活与临时目录清理。全仓和隔离构建在收尾记录，本节当前仅为 implemented/LAB 适配，不是 QQ 下载修复现场通过。

收尾：PowerShell 7 串行 `init.ps1 -Mode all` exit 0，default/research 全仓通过，research bridge lib 116/116、图片合同 4/4，原有 ignored 项保留；修复前预期 1 FAIL 留档，没有新增失败。隔离 release exit 0，路径 `target-harness/snowcat-sent-image-null-chat-repair-20261114/release/`，bridge SHA-256 `73B11BD13B489B2C780AA67158302D5F51915EAFFC5C717F91611E33276EF240`，CLI/core pins、source snapshot 和增量 patch 在本机 repair evidence 目录。rustfmt 经夹具的 path 模块同时格式化 query poll/cancel 的两个 Rust 拼接调用，仅换行，无额外语义变更，最终全仓包含该版本。新构建未加载。

本机 `local-evidence/snowcat-sent-png-null-chat-repair-20261114/run-sample.ps1` 已准备，沿用前节的只读合同与已发图片：独立 core/token/pipe，原停止账本的逐字节副本；新指定 A 的 PID/创建时间、精确 hash/backend/envscan、listener_armed 和 LoginInfo 准入后，只调用一次 get_msg/get_image，取得字节才与原图核验。异常仍观察 30 秒再 finally 停止，不发送或重发。PowerShell 解析、Python 编译、source/build pins 和账本副本检查通过；最终 docs 与 Windows Git diff --check exit 0，日志为本机 final-check.log。已审阅本轮独立 patch，保留全部既有 dirty 改动和历史失败，没有提交、推送或发布。下一步需用户正常重启指定 A 并进入聊天主界面，再在新代次验证修复；旧 43644 不重接或热卸载，B 可在线，已发本图不重发。feat-008 仍 in-progress；群 PNG 主动发送、其他媒体/协议与恢复仍待各自现场门。


### 2026-11-14 已发送私聊 PNG 正常下载现场通过

用户正常重启 A 并确认已进入聊天主界面。旧 43644 已退出，唯一新主根 52752（创建 `2026-11-14T10:03:08.3505220Z`）无旧 bridge，与既有未指定根区分。沿用前节全仓通过的源码 snapshot 和隔离修复构建，bridge SHA-256 `73B11BD13B489B2C780AA67158302D5F51915EAFFC5C717F91611E33276EF240`，三项 build pins 一致；当次固定 manifest、loaded 三角色 hashes、PE/backend 与唯一 envscan 均通过。正常 daemon/IPC 单次接入，listener_armed 后唯一 LoginInfo HTTP 200 / 0 匹配 A。原始材料仅在 ignored `local-evidence/snowcat-sent-png-null-chat-repair-20261114/`。

本轮零发送，仅在原停止账本副本中读取前轮已经发送且 B 确认可见的同一持久消息 ID。get_msg HTTP 200 / 0，private、发送者 A、原 ID/时间/图片段及账本 B 目标一致；随后单次 get_image HTTP 200 / 0 返回 18,569 bytes、96×64 PNG。SHA-256 `d560bd9de44c80be94ee57e5318eb688f82edd0d5c2a2182526159094257cbfc`，与原发送输入完全相同。独立核对再次比较原图、HTTP base64 严格解码与下载文件，三份逐字节一致，并核对消息 ID、发送者和账本目标。

callback 仍为 chatType=null，消息/元素/预登记路径/错误字符串 0/slot/deadline 匹配，随后 validated；结合正常 HTTP 文件证明限定缓存回调适配在本固定 QQ 样本通过。没有放宽群聊、入站或其他格式。本样本补齐前轮已发送私聊 PNG 的正常文件下载现场证据，发送与 B 可见性来自前轮，属于跨代次核验，不能称新构建同一进程内重新完成了发送。也不以停止账本副本的使用宣称生产恢复通过。

runner 正常停止 daemon/owner，两项 exit 0；独立健康核对时间 `2026-11-14T10:04:49.6184814Z`，当前 QQ 响应，HTTP/SSE listener 均 0，保留修复 bridge。没有重接、热卸载或重发。采集 summary、独立 field-summary.private.json 和 A/final-health.private.json 留档；历史超时/null/unknown 均保留。Rust 与二进制未变化，复用前节实际通过的全仓检查，不重复运行相同套件。feat-008 保持 in-progress，其他媒体/协议/恢复未提升。

下一块准备登记测试群的 A 单次主动 PNG：本机 `local-evidence/snowcat-group-png-send-field-20261114/` 有独立新图、精确 send_group_msg 请求、request-hash 绑定授权与独占落盘 reservation、原登记群来源、当前 build pins/source snapshot、正常准入/就绪/唯一 LoginInfo/单次发送/持久 get_msg/get_image/finally 关闭脚本。目标沿用已通过群文本和群 PNG 入站的登记群，不猜群号。发送授权沿用户明确的继续验收及发送消息授权，不重新询问；单次、unknown 不重发、异常观察 30 秒。群回调 null 仍无适配，本次不预先放宽；现场可能暴露其独立缺口。脚本和请求检查、最终 docs/Windows diff 结果见本轮 final-field-check.log。尚未启动下一 daemon、接入或发送，须用户正常重启 A 并登录主界面，新代次重新准入；旧 52752 不重接或热卸载，B 可在线。未提交、推送或发布。


### 2026-11-14 主动群聊 PNG 发送可见与下载空类型

用户重启指定 A、确认已进入主界面且 B 在线；旧 52752 已退出，唯一新 QQ 主根 46612（创建 `2026-11-14T11:24:26.0928331Z`）无旧 bridge，与未指定根区分。源码 snapshot 和私聊限定修复 release pins 一致，bridge SHA-256 `73B11BD13B489B2C780AA67158302D5F51915EAFFC5C717F91611E33276EF240`；固定 manifest、loaded 三角色 hashes、PE/backend 和唯一 envscan 通过。正常 daemon/IPC 单次接入，listener_armed 后 LoginInfo HTTP 200 / 0 匹配 A。原始身份、群号、请求、平台 ID 和图片仅在 ignored `local-evidence/snowcat-group-png-send-field-20261114/`。

在用户明确发送授权内，request-hash 绑定的独占 reservation 落盘并 fsync 后，正常 OneBot HTTP send_group_msg 向登记测试群单次发送一张合成 PNG，18,615 bytes、96×64。HTTP 200 / retcode 0 返回持久消息 ID；get_msg HTTP 200 / 0，group、A 发送者、ID/时间和账号限定图片段一致。账本账号 A、group peer 与原登记群一致，图片 picType=1000、MD5/大小/尺寸与输入匹配。用户确认 B 在群内已显示本轮图片。独立复核通过，实际只提交一次，未测量显示时延；发送、持久消息回读和 B 可见性通过，不代表完整文件下载闭环通过。

随后单次 get_image HTTP 200 / 1501，无返回文件字节。匹配本消息的 callback 为 chatType=null，其消息/元素/错误字符串 0/预登记路径/slot/deadline 均匹配；当前自发私聊特例不适用于群聊，因此拒绝后超时。同轮另有其他群图片 callback，因消息/元素/路径不匹配被忽略，没有抢占本查询或提供文件。异常额外观察 30 秒，不重试或重发；finally 正常停止 daemon/owner，两项 exit 0。独立核对时间 `2026-11-14T11:26:51.6509350Z`，QQ 响应、HTTP/SSE listener 均 0，bridge 保留。证据为本机 field-summary.private.json 和 A/final-health.private.json。feat-008 仍 in-progress，下载缺口及未知异步寿命保留。


### 2026-11-14 自发群 PNG 空类型缓存回调适配

按上节精确 hash 群场景增加回归，原实现仍 pending，0 PASS / 1 FAIL 留在 `local-evidence/snowcat-group-png-null-chat-repair-20261114/repro-before.log`。本次只将已验证 pilot 的自发 picType=1000、预登记缓存文件的 null 回调适配，从 private=1 扩展到已现场观察的 group=2；原消息/peer/sender/已发送状态/元素/元数据、callback ID/错误/slot/在途/deadline、文件身份/大小/MD5/PNG 格式与清理寿命不变。前置 native 群消息回读必须准确匹配 group peer，不能凭缓存文件或 null 类型确认其他群。语音、入站、1001、未知 profile、未登记路径、undefined/字符串/其他数字继续严格拒绝。

只改 qq_query.rs 的分支与 wrapper_image_contract.rs，未改共享诊断过滤器。GitNexus 显式绑定 Caligo / `E:/stella/Caligo` / `5665b2b`，刷新后 builder impact LOW；COMMON_JS 与测试 UNKNOWN 由固定拼接、owner、图片/语音 helper 调用和夹具补查，FTS/跨语言/流程预算限制保留，不以缺边证明无影响。群正例及拒绝/隔离回归定向通过：错误群回读不派发下载，其他消息/元素/路径/类型不完成，未知 profile、1001 与入站仍拒绝，损坏文件失败，超时/取消不复活且缓存保留。私聊既有用例同时保留。全仓与隔离 release 见本节收尾；当前仅 implemented/LAB 修复，尚非新构建群下载现场通过。

收尾验证：PowerShell 7 串行 `init.ps1 -Mode all` exit 0，default/research 全仓通过，research bridge lib 116/116、图片合同 4/4，原有 ignored 项保留；修复前预期的 1 FAIL 留档，最终无新增失败。隔离 release exit 0，`target-harness/snowcat-group-image-null-chat-repair-20261114/release/`，bridge SHA-256 `FB12A275EA02C2441051BA9E7CE39CA38464C4348DFDDB480E34A5D6A4BC4235`，CLI/core pins、source snapshot、增量 patch 和检查日志在本机 repair 目录。新构建尚未加载，不把 LAB 当群文件下载 FIELD 通过。

下一轮本机 `local-evidence/snowcat-group-png-null-chat-repair-20261114/run-sample.ps1` 只读取本轮已发送、B 已显示的原群图片：停止账本的逐字节副本、同一持久 ID/登记群/原图和明确 read-plan；新指定 A 的 PID/创建时间、模块/backend/envscan、listener_armed 与 LoginInfo 通过后，只调用一次 get_msg/get_image，取得文件才与原图比较。没有发送动作，异常仍保留 30 秒观察后正常 finally 停止，恢复能力不因账本副本而提升。PowerShell 解析、Python 编译、source/build pins、账本和 read-plan 核对通过；最终 `init.ps1 -Mode docs` 与 Windows Git `diff --check` exit 0（本机 final-check.log）。已审阅本轮增量 patch，既有 dirty 内容及原失败样本保留，未提交、推送或发布。下一步需用户正常重启 A 并进入聊天主界面，新代次重新准入；旧 46612 不重接或热卸载，本图不重发。feat-008 继续 in-progress，其他媒体/协议与恢复仍未验收。


### 2026-11-14 已发送群 PNG 正常下载现场通过

用户正常重启指定 A 并进入聊天主界面；旧 46612 已退出，新唯一 QQ 主根 48692（创建 `2026-11-14T11:36:14.7505867Z`）无旧 bridge，与未指定根区分。前节已测 source snapshot、三项 release pins 均一致，bridge SHA-256 `FB12A275EA02C2441051BA9E7CE39CA38464C4348DFDDB480E34A5D6A4BC4235`；当次 manifest、loaded 三角色 hashes、PE/backend 和唯一 envscan 通过。正常 daemon/IPC 单次接入，listener_armed 后唯一 LoginInfo HTTP 200 / 0 匹配 A。材料仅在 ignored `local-evidence/snowcat-group-png-null-chat-repair-20261114/`。

本轮零发送，只读原停止账本副本中的已发群图片。get_msg HTTP 200 / 0，group、发送者 A、原持久 ID/平台时间/图片段以及账本登记群目标一致。随后对该账号限定标识单次 get_image HTTP 200 / 0，返回 18,615 bytes、96×64 PNG；SHA-256 `f6e13b7d22921e0370038d8d1250d37b1cc2710c6b1967f2902a2c7ce755de9f`，与原发送图相同。独立复核原图、严格 base64 解码和下载文件三份逐字节一致，再核对原群 ID/账号/消息 ID/发送者/时间。

匹配本查询的 callback 仍为 chatType=null，消息/元素/登记路径/错误字符串 0/slot/deadline 匹配后 validated。该限定群缓存适配正常取得文件，未用其他消息回调或外部缓存文件替代公开查询。原图的单次发送和 B 群内可见性由前轮提供，本轮跨代次补齐下载，不称新构建同一生命周期内重新发送通过。停止账本副本的使用也不升级生产恢复能力。

runner 正常停止 daemon/owner，两项 exit 0；独立核对时间 `2026-11-14T11:38:04.5301228Z`，QQ 响应，HTTP/SSE listener 均 0，修复 bridge 保留。没有重发、重接或热卸载。采集 summary、独立 field-summary.private.json 与 A/final-health.private.json 留档，历史失败/unknown 保留。结合已有私聊/登记群普通 PNG 入站和主动发送样本，两个方向各有对应现场证据，自发图片的正常文件下载跨代次补齐；其他格式、媒体资源长期寿命和恢复仍未闭合，feat-008 继续 in-progress。Rust 与二进制未改，复用前节已通过的全仓验证。

下一块只准备普通私聊语音入站：本机 `local-evidence/snowcat-private-voice-receive-20261114/` 的 run-sample.ps1/capture.py、固定 A/B 计划、唯一测试短语、当前 build pins/source snapshot 已准备，不发起下一现场。需新 A 正常登录主界面并重新准入，listener_armed/LoginInfo/SSE 就绪后，由 B 用 QQ 录音单次发送约 3–5 秒测试短语，用户另确认 A 能播放且内容相符。采集器只接收指定 B 到 A 的单个普通 private/friend record，核对持久 get_msg、账本身份/时间/元数据，随后单次 get_record(out_format=silk)，比较返回长度/MD5 与 native 元数据和 SILK 头，保存 SHA-256。没有 B 原始文件独立哈希，也不自动识别或转录语音内容；A 播放由用户核对，不能冒称下载文件独立解码已验收。WAV/OGG、主动语音、群语音、其他媒体/协议与恢复不属于本工作块。

该计划限制原始 SILK 下载 128 KiB，与当前公开合同一致，不配置转码工具、不改 Rust。GitNexus 显式绑定 Caligo / `E:/stella/Caligo` / `5665b2b` 刷新，get_record、voice_bytes、D8_START_JS 的 context 与源码交叉核对；FTS 缺失导致概念 query 降级，没有把空查询当无支持。固定接线为 SSE/StoreReader voice_record、private VoiceDownload 和原始 SILK 响应；目录/资料等能力未提升。PowerShell 解析与 Python 编译、source/build pins、计划/动作范围检查通过，最终 docs/Windows diff 结果在本机语音目录 final-check.log。下一次只需用户重启 A，旧 48692 不重接或卸载；B 在 SSE 就绪前不要发送。没有提交、推送或发布。


### 2026-11-14 私聊语音现场采集启动

用户正常重启指定 A 并确认主界面、B 在线且未发送语音。旧 48692 已退出，新唯一主根 32064（创建 `2026-11-14T11:51:50.4124299Z`）无旧 bridge，与未指定根区分。沿用前节同构建 pins/source snapshot，bridge `FB12A275EA02C2441051BA9E7CE39CA38464C4348DFDDB480E34A5D6A4BC4235`；manifest、loaded 三角色 hashes、PE/backend 和唯一 envscan 通过。正常 daemon/IPC 单次接入，listener_armed 与唯一 LoginInfo HTTP 200 / 0 匹配 A，SSE 已就绪。

仅请求用户从 B 发一次约 3–5 秒普通私聊 QQ 录音并确认 A 能播放、内容相符。用户回复尚未发送；截至 `2026-11-14T11:55:51Z` 没有指定 voice SSE、消息或 get_record 采样文件。因此当前只有准入/监听准备证据，不是语音成功或失败样本。本轮没有工具发送或重试，采集器仍运行，最多等到 `2026-11-14T12:07:31.800430Z`（北京时间 20:07:31），届时无样本则按 finally 正常关闭。材料在 ignored `local-evidence/snowcat-private-voice-receive-20261114/`，active-field-state.private.json、state.private.json、driver.private.log 和 A/sse-ready 可恢复当前状态。

当前不要重新接入或要求用户重复发送；先查 voice.event.private.json、message-summary.private.json、get_record 响应、summary 与停止/健康文件。已启动 driver 的 exec session 为 81991，原始 SILK/128 KiB/单次查询/失败后观察 30 秒合同不变。尚未证明播放或文件下载，也未宣布关闭，feat-008 保持 in-progress。此处记录等待输入时的现场状态，完成后另附结果，不覆盖本启动记录。文档检查和 Windows diff 结果留本机 active-docs-check.log。

### 2026-11-14 私聊语音首轮到期无样本

首轮于北京时间 20:07:31 到期，采集器未观察到指定语音 SSE，账本复核无指定 B 到 A 的私聊记录，未调用 `get_msg` 或 `get_record`。用户随后确认 B 未发送；这是无输入样本的收尾，不判定 Caligo 语音成功或失败。daemon 和 owner 停止均 exit 0，独立复核原代次 A 32064 响应正常、HTTP/SSE listener 均 0、原 bridge 保留。原始证据与收尾摘要只在 `local-evidence/snowcat-private-voice-receive-20261114/`（`A/capture-timeout.private.json`、`A/final-health.private.json`、`field-summary.private.json`）。零工具发送，无自动重试。

用户准备正常重启 A。下一轮使用独立的本机 `local-evidence/snowcat-private-voice-receive-20261114-attempt2/` 和新测试短语，旧采集证据原样保留；PowerShell 解析、Python 编译、source snapshot 与 release pins 核对通过。保持原语音合同，只在新 A 重新准入且 SSE 就绪后安排 B 单次录音，再校验 SSE、持久账本、`get_msg`、单次原始 SILK `get_record` 和 A 客户端播放。Rust 和二进制未变，feat-008 仍 in-progress。

### 2026-11-14 私聊语音入站通过与下载连接中断

用户正常重启指定 A；旧 32064 已退出，新 A 51352（创建 `2026-11-14T12:12:18.1434539Z`）无旧 bridge，与未指定 QQ 主根区分。沿用同一固定源码快照及 release，bridge SHA-256 `FB12A275EA02C2441051BA9E7CE39CA38464C4348DFDDB480E34A5D6A4BC4235`。精确 loaded module hashes、manifest、PE/backend、唯一 envscan（1 hit、0 read failures）及正常 daemon/IPC 单次接入通过。`listener_armed`、LoginInfo HTTP 200 / 0 与 A 身份匹配，SSE 于北京时间约 20:13:16 就绪后通知 B。

用户确认 B 单次用 QQ 私聊发录音、A 收到并能播放，内容与本轮短语一致。Caligo 观察到同一范围内的单条 private/friend `record`，SSE、持久账本及 HTTP `get_msg` 200 / 0 的账号、发送者、消息 ID、时间和 `record` 标识匹配；账本语音声明长度 7,344 bytes，带有效 MD5 格式。本轮完成普通私聊语音入站和原生 QQ 播放样本；未取得下载字节，不能从 QQ 播放推断 `get_record` 或独立解码通过。

随后通过 Python 客户端单次尝试 `get_record(out_format=silk)`，在 HTTP 请求写入阶段报告 WinError 10053（连接中断），未读到 HTTP 状态或 OneBot 响应。采集日志未证实宿主查询派发，不能归因于原生下载或回调拒绝。观察 30 秒，未自动重试或重新发送；finally 正常停止 daemon/owner（两项 exit 0）。独立复核 A 仍响应，HTTP/SSE listener 均 0，bridge 保留。原始证据在 `local-evidence/snowcat-private-voice-receive-20261114-attempt2/`，见 `field-summary.private.json`、`A/message-summary.private.json`、`A/final-health.private.json` 和 `driver.private.log`。

下一轮只读原样本：本机 `local-evidence/snowcat-private-voice-readback-20261114/` 已准备校验过 SHA-256 的停止账本副本、原账号/消息/语音标识及元数据固定计划；PowerShell 解析、Python 编译、源码快照、release pins 和账本摘要核对通过。新指定 A 登录并重新准入后，用 curl 明确记录客户端退出与 HTTP 状态，按次序各查一次原消息 `get_msg` 及原始 SILK `get_record`，保留失败证据，不请求 B 重发。旧 A 不重接或热卸载。无 Rust 或二进制变更。主动发送、群语音、转码、生产恢复和完整媒体验收未覆盖，feat-008 保持 in-progress。

### 2026-11-14 已收私聊语音只读下载超时

用户再次正常重启 A 并确认登录主界面。旧 51352 已退出，新 A 40204（创建 `2026-11-14T12:17:56.8397038Z`）无旧 bridge，通过同一源码和 release pins、固定模块/manifest、PE/backend、唯一 envscan；正常 daemon/IPC 单次接入，通过 `listener_armed` 和 LoginInfo。使用停止账本的摘要校验副本，只查询原样本。`get_msg` curl exit 0 / HTTP 200 / OneBot 0；持久消息 ID、来源、时间与 `record` 标识和上一轮一致。

单次 `get_record(out_format=silk)` curl exit 0 / HTTP 200 / OneBot 1501，服务端返回“宿主查询超时”，无 SILK 字节。这次已取得明确 HTTP 响应，与上一轮客户端写入中断分别留档。停止后按采样时段只读检查，有一个相关时段的 `caligo-media-*` 临时目录候选，未见所需 `voice.silk`；不能仅凭时间关联证明该查询已被宿主受理或确定故障阶段。现有媒体阶段诊断只记录图片；没有匹配的语音回调、文件路径或 chatType 证据，不沿用 PNG 兼容分支。未自动重试、未发消息，也未让 B 重发。观察 30 秒后 daemon/owner 正常停止，两项 exit 0；A 仍响应，HTTP/SSE listener 均 0，bridge 保留，原现场证据不覆盖。

本机证据：`local-evidence/snowcat-private-voice-readback-20261114/` 的 `field-summary.private.json`、`A/get_msg.http.json`、`A/get_record.http.json`、`A/get_record.response.private.json` 和 `A/final-health.private.json`。下一块先离线准备有界脱敏的语音下载阶段取证，再按新代次准入只读原消息；不扩大媒体能力、不自动重试。已证普通私聊入站及 A 播放，未证原始 SILK 下载、独立解码、主动发送、群语音、转码或生产恢复。Rust 与 release 未变，feat-008 保持 in-progress。

### 2026-11-14 私聊语音下载阶段诊断

精确 9.9.36-53644 profile 的研究构建在 `get_record` 宿主查询中记录消息回读发起、回读匹配、下载派发、Promise 受理分类、完成回调比较与文件校验。媒体类型以固定 `image` 或 `voice` 标签写进本机 `media_download_stage`。D8 每批最多保留 8 项，本机报告每进程最多输出 32 批，Rust 白名单只允许固定标签和布尔比较。超时后可观察迟到回调，原下载目标和在途槽寿命继续按精确匹配合同处理。

GitNexus 重新建立 Caligo 当前路径的独立索引，匹配 HEAD `5665b2b`。`build_profile_query_start_js`、`sanitize_media_download` 和 `poll_listener_json` 均为 LOW，分别关联 owner query、D8 排空与 daemon client 启动流程。嵌入 JS 及测试影响面用源码与合成宿主核对。固定 SnowCat `7cf025f` 的样本使用同一下载调用，但观察器按消息和元素关联后读取回调给出的路径。Caligo 仍要求回调匹配预先登记的目标，差异留待真实语音取证。

定向 research bridge lib 116/116、语音与查询合同 7/7、图片合同 4/4，`init.ps1 -Mode all` default/research 全仓和隔离 release 构建均通过。首次定向测试未及时排空诊断批次，修正测试在独立下载间排空后通过。离线日志、GitNexus 输出、源码快照与二进制 pins 在本机 `local-evidence/snowcat-voice-download-diagnostic-20261114/`。bridge SHA-256 为 `8F6A6999975976C62575928F152E247D58DBFD99AFB28FD800614C5DFBDD08DA`。

本机 `run-sample.ps1` 已准备同一已收语音的账本副本和固定读回计划，PowerShell 解析、Python 编译、账本摘要及构建校验通过。待新 A 重新准入后执行单次 `get_msg` 和原始 SILK `get_record`。下载成功须核对文件长度、MD5 和 SILK 头，失败则观察 30 秒并保存固定阶段证据。当前结果为离线诊断通过，原始 SILK 下载待验，feat-008 保持 in-progress。

### 2026-11-14 私聊语音缓存回调定位与适配

用户重启 A 后，53180（创建 `2026-11-14T12:38:37.7201592Z`）通过精确模块哈希、PE/backend 审计、唯一 envscan 和 LoginInfo。诊断版只读原语音，`get_msg` HTTP 200 / OneBot 0，`get_record` HTTP 200 / OneBot 1501。精确回读与下载派发均发生。Promise 返回形状分类为 unknown，随后一条截止前回调匹配消息、语音元素和在途槽，错误码为字符串 `0`。它的 `chatType=null`，路径与独立临时目标不匹配，所以被关联校验拒绝。30 秒观察后正常停止 daemon 和 owner，两项退出码为 0。A 仍响应，HTTP/SSE 监听均为 0，bridge 保留。零发送。现场摘要和停止证据在本机 `local-evidence/snowcat-voice-download-diagnostic-20261114/`。

A 的 QQ 原始语音缓存下找到一份 7,344 字节的文件，文件名为消息 MD5 加 `.amr`，文件内容的 MD5 与原消息相符，文件头为 SILK v3。这个磁盘样本确认了缓存目录和文件形态。已有回调诊断只记录比较结果，尚不能确定回调路径或精确回读的 `pttElement.filePath` 指向该文件。

离线适配仅用于 hash 验证的 9.9.36-53644 普通入站私聊语音。从已精确匹配的 wrapper 语音元素获得路径，登记前校验绝对规范路径、同账号 `nt_qq/nt_data/Ptt/<month>/Ori`、MD5 文件名与真实父目录。`chatType=null` 只在零错误回调精确命中该登记目标时接受。保留消息、元素、请求槽、期限和文件身份、长度、MD5、SILK 文件头校验。只清理 bridge 自建目录，保留 QQ 缓存。注册值不存入账本或 IPC。编排仍只派发一次。固定研究参考 `7cf025f` 定义 `PttElement.filePath`，实际字段和回调路径一致性由下次现场复查确认。

GitNexus 增量索引对共享查询入口和测试报 CRITICAL，含 G2 与脚本文件等宽泛边，嵌入 JS 目标未索引。核对 Rust builder、两处 JS 调用和 D8 回调路由后，运行定向语音/查询 7/7、图片 4/4 与 default/research 全仓回归。隔离 release 的 bridge SHA-256 为 `3695D707ECD68D1B9AC3933787A3F5B1A221754B6E85F488C95701E52000D69F`。只读脚本、账本副本、构建 pins 和验证日志在本机 `local-evidence/snowcat-voice-cache-path-repair-20261114/`。下一步使用新 A 复查原语音，核对 `registered_path_matches` 与实际 SILK 字节。原始 SILK 下载仍待现场验收，feat-008 保持 in-progress。

### 2026-11-14 已收私聊语音 SILK 现场通过

用户正常重启 A，确认已进入聊天主界面。新 A 11908（创建 `2026-11-14T12:46:54.2442740Z`）通过固定模块哈希、PE/backend 审计、唯一 envscan、`listener_armed` 和 LoginInfo。修复版用停止账本副本各执行一次 `get_msg` 和 `get_record(out_format=silk)`，两者均 curl exit 0 / HTTP 200 / OneBot 0，复用原消息标识。

完成回调的 `chatType=null`，错误码为字符串 `0`，匹配消息、元素、在途槽与作用域。返回路径命中从精确 `pttElement.filePath` 预登记的 QQ 缓存文件，阶段记录为 `validated`。OneBot 返回 7,344 字节，长度、MD5 和 SILK v3 文件头通过校验，SHA-256 为 `e5f5ede3b3ab0606a0da412a376b549156727f8e9e0817fd0712c0988a5cc4e5`，与上一轮只读找到的缓存文件哈希相同。

本轮零发送。daemon 和 owner 正常停止，两项 exit 0，HTTP/SSE 端口均关闭，QQ 响应并保留 bridge。本机证据在 `local-evidence/snowcat-voice-cache-path-repair-20261114/` 的 `field-summary.private.json`、`A/summary.json` 和 `A/final-health.private.json`。与先前 SSE、账本及用户播放确认合并，覆盖该版本上一条普通入站私聊语音的接收、回读及原始 SILK 下载。主动发送、群语音与转码待验，feat-008 保持 in-progress。

### 2026-11-14 主动私聊语音发送现场通过

使用既有 SILK 工具离线编码 0.9 秒的三个上行提示音，腾讯前缀 SILK v3，45 个包、2,584 字节。离线解码核对 24 kHz 单声道、21,600 个采样和三个有声段。样本、请求和 SHA-256 绑定的单次授权保存在本机 `local-evidence/snowcat-private-voice-send-field-20261114/`。PowerShell 解析、Python 编译、源码快照和沿用 release 的构建摘要核对通过，无授权脚本在现场预检前拒绝。

用户授权 A 到登记 B 单次发送，并重启 A 登录主界面。新 A 53416（创建 `2026-11-14T13:05:47.3502667Z`）通过固定 9.9.36-53644 模块哈希、PE/backend 审计、唯一 envscan、`listener_armed` 和 LoginInfo。单次发送 reservation 持久落盘后，经正常 OneBot HTTP 提交 `send_private_msg`，返回 retcode 0 和持久消息 ID。独立 `get_msg` 返回 retcode 0，同一消息 ID、私聊类型、发送者和 `record` 标识匹配。持久账本目标匹配登记 B，语音长度和 MD5 匹配输入。用户确认 B 收到一条、可以播放、听到三个依次升高的提示音。

daemon 和 owner 停止均 exit 0，HTTP/SSE 已关闭，QQ 仍响应并保留 bridge。现场凭据、录音、真实身份及回执只留本机，见上述目录 `field-summary.private.json` 和 `A/final-health.private.json`。已发送语音的原始 SILK 下载、群语音和转码仍待验，feat-008 保持 in-progress。

### 2026-11-14 修复提交与已发私聊语音复查准备

本地提交 `1e8dbb9` 保存已验证的登录与目录查询、会话撤权、文本及媒体修复与证据。提交前将真实账号、昵称、群号和扫描地址留在本机，仓库证据以别名描述。默认和研究模式全仓测试、覆盖结构检查和 staged diff 检查通过。GitNexus 对全部 35 个变更文件报告 296 个变化符号、83 条受影响流程，整体风险为 critical，分析未截断或降级。日志在本机 `local-evidence/snowcat-fixes-checkpoint-20261114/`。

已发私聊语音的只读复查已准备。使用已停止发送账本的校验副本和原发送样本，核对登记 A/B、私聊类型、持久消息 ID、语音标识、长度及 MD5。仍使用上轮经验证的固定构建，源码快照、构建哈希、账本哈希和 PowerShell/Python 脚本均通过检查。新 A 准入后按原 ID 各执行一次 `get_msg` 与 `get_record(out_format=silk)`，对返回内容执行逐字节比较。失败保留 30 秒回调观察后正常关闭。原始样本和现场脚本在本机 `local-evidence/snowcat-sent-private-voice-readback-20261114/`。待用户提供新 A 代次，feat-008 保持 in-progress。

### 2026-11-14 已发私聊语音缓存回调修复

用户重启后的 A 10044（创建 `2026-11-14T13:15:13.0596075Z`）通过固定模块哈希、PE/backend 审计、唯一 envscan 和 LoginInfo。原已发消息的 `get_msg` 返回 HTTP 200 / OneBot 0，`get_record` 返回 HTTP 200 / OneBot 1501。诊断捕获同消息、同元素和同请求槽的零错误回调，`chatType=null`，目标既非独立临时路径也未登记。其它回调因消息和元素不同被过滤。观察 30 秒后 daemon 和 owner 正常停止，HTTP/SSE 监听关闭，QQ 响应且保留 bridge。本次只读，原始证据在本机 `local-evidence/snowcat-sent-private-voice-readback-20261114/`。

同账号 `nt_qq/nt_data/Ptt/<month>/Ori` 下发现与 2,584 字节发送样本 SHA-256 一致的 `<md5>.silk`。修复限定在精确 9.9.36-53644 的成功自发私聊记录，从精确回读的 `pttElement.filePath` 预登记同账号、同 MD5 和规范缓存路径。零错误的空会话回调须匹配该路径和原消息及元素，文件仍须通过身份、长度、MD5 和 SILK 头验证。原入站 `.amr` 规则继续适用。GitNexus 共享 builder 风险为 CRITICAL，嵌入 JS 未单独索引，已核对 owner 路由和语音及图片调用者。

合成回归先复现已发回调无法完成，修复后查询与语音合同 7/7、图片合同 4/4，默认及研究模式全仓测试、隔离 release 和文档检查通过。用例覆盖成功发送状态、固定 profile、私聊范围和精确路径，保留原缓存。新 bridge SHA-256 为 `F30094F975D07CFA0B07679AC479FBDC7A5B8BC01EEFA525A02CFC855E2736E4`。构建、验证日志和只读复查材料在本机 `local-evidence/snowcat-sent-private-voice-cache-repair-20261114/`，复用同一发送账本和样本。现场下载待新 A 准入复查。

### 2026-11-14 已发私聊语音 SILK 现场通过

用户正常重启 A。新进程 33668（创建 `2026-11-14T13:21:41.8333472Z`）通过精确 9.9.36-53644 模块哈希、PE/backend 审计、唯一 envscan、`listener_armed` 和 LoginInfo。上节修复版使用已停止的发送账本副本，对原持久消息各调用一次 `get_msg` 和 `get_record(out_format=silk)`，均为 curl exit 0 / HTTP 200 / OneBot 0。

QQ 的零错误完成回调为 `chatType=null`，匹配原消息、语音元素、在途请求和预登记的缓存路径，进入 `validated`。返回的 2,584 字节与原发送样本逐字节相同，独立复核的 SHA-256 为 `557d6b3260d5d9c4bf2700bc902da6a6c840166722be9ebc53057baf59b5b907`。本轮仅查询原消息，发送与 B 播放证据沿用上一轮现场记录。

daemon 和 owner 均正常停止，退出码为 0。HTTP/SSE 监听均关闭，QQ 响应并保留 bridge。现场摘要及验后健康记录留在本机 `local-evidence/snowcat-sent-private-voice-cache-repair-20261114/` 的 `field-summary.private.json`、`A/summary.json` 和 `A/final-health.private.json`。本次执行未修改源码或重建二进制，使用上节通过全仓离线验证的修复版。群语音与转码待验，feat-008 保持 in-progress。

### 2026-11-14 登记群语音入站与缓存回调修复

用户更正首条消息方向后，现场核对原 SSE 连接仍在线，账本和采集文件均未出现符合目标范围的语音。沿用 A 8392 的同一接入和采集窗口，为正确方向使用新的口令。用户确认 B 在登记群只发一条录音，A 收到并能播放，所听内容与新口令一致。SSE、持久账本和 `get_msg` HTTP 200 / OneBot 0 匹配同一消息、发送方、群和 `record` 标识。

单次 `get_record` 返回 HTTP 200 / OneBot 1501。精确回读和派发完成，匹配原消息与元素的回调错误码为字符串 `0`，会话类型为 `null`，路径未登记。其它消息和元素的回调被过滤。观察 30 秒后停止 daemon 和 owner，两项退出码为 0，HTTP/SSE 已关闭，QQ 响应且保留 bridge。A 账号缓存中发现一个长度与 MD5 匹配的 7,294 字节 `.amr` 文件，SILK v3 头通过检查。初次验收没有获得 API 下载字节。采集日志、用户方向更正、播放确认及缓存摘要留在本机 `local-evidence/snowcat-group-voice-receive-20261114/`。

合成回归复现后，将入站语音的 `.amr` 缓存规则扩展到精确 9.9.36-53644 的入站群语音。仅在原消息、群、发送方和元素均通过精确回读后登记该元素的同账号缓存路径，继续校验真实父目录、MD5 文件名、请求槽、文件身份、长度和 SILK 文件头。空会话类型只对零错误且命中预登记路径的回调生效。群自发语音继续使用原路径合同。GitNexus 共享 builder 为 LOW，嵌入 JS 与测试的未知影响面由 owner 路由及两个媒体调用点补查。定向语音及查询合同 7/7、图片 4/4，隔离 release 构建完成。并行全仓首轮 journal 测试因共享临时目录报路径不存在，保留失败日志并串行复验。新构建及同一录音的只读验收材料留在本机 `local-evidence/snowcat-group-voice-cache-repair-20261114/`。
### 2026-11-14 登记群入站语音 SILK 现场通过

串行 `init.ps1 -Mode all` 通过默认和研究模式全仓测试。首次并行失败的 journal 测试在串行复验中通过，其用例以进程 ID 共用临时目录。修复版 bridge SHA-256 为 `E5749D9D73798C1E40B4F702AE999EFAF3E6A38A79FC9EC328A202C919818F6A`。用户重启后的新 A 54700（创建 `2026-11-14T13:35:32.5649300Z`）通过模块哈希、PE/backend 审计、唯一 envscan 和 LoginInfo。

在停止账本的校验副本上，`get_msg` 和 `get_record(out_format=silk)` 对原消息各执行一次，均返回 HTTP 200 / OneBot 0。匹配消息和元素的零错误回调命中预登记路径，进入 `validated`。返回内容 7,294 字节，长度、MD5、SILK 头均通过，SHA-256 与首次采集后独立找到的缓存一致。B 的语音、A 的 SSE 与播放证据均来自上一代现场，本轮仅只读复验。daemon 和 owner 正常停止，HTTP/SSE 已关闭，QQ 继续响应。复验摘要与健康记录在本机 `local-evidence/snowcat-group-voice-cache-repair-20261114/`。群主动语音、群自发下载和转码待验。

### 2026-11-14 主动群语音发送端通过，播放待核对

恢复任务时发现，本机 `local-evidence/snowcat-group-voice-send-field-20261114/` 已有单次发送及正常收尾记录，交接首段仍停留在“方案已准备”。本次只读复核这批证据，没有再次发送、接入或卸载 bridge。原发送授权绑定单次请求摘要，发送预留已消耗，不能重跑该发送脚本。

原现场使用新 A 38620（创建 `2026-11-14T13:40:18.4020644Z`），通过固定 QQ 9.9.36-53644 模块哈希、PE/backend 审计、唯一 envscan、owner 就绪及 LoginInfo，bridge SHA-256 为 `E5749D9D73798C1E40B4F702AE999EFAF3E6A38A79FC9EC328A202C919818F6A`。正常 OneBot HTTP 向登记群单次提交 0.9 秒三音降调 SILK，`send_group_msg` 和 `get_msg` 均 HTTP 200 / OneBot 0。独立复核持久 ID 相同，账本恰有一条匹配记录，账号、群目标、2,575 字节长度和 MD5 均与发送样本一致；样本 SILK 头及 SHA-256 `75547048ca48a4f9d532855b3526c3f3ac6fe56012c2df327426bb803d1f2a6b` 核对通过。

daemon 与 owner 的停止退出码均为 0；`A/final-health.private.json` 在 `2026-11-14T13:41:25.1700406Z` 记录 QQ 响应、daemon 停止、HTTP/SSE 无监听，bridge 保留。此为原采样时的健康证据，不据此推断恢复任务时的进程状态。`A/summary.json` 的对端播放仍为 `pending_user_check`，本次已请求用户核对 B 是否仅收到一条且播放为三个依次降低的提示音；未获确认前不宣称群语音端到端发送通过。该消息尚无 `get_record` 证据，后续只读原消息、另行核实新 A 准入，不重发。

本次未改 Rust，源码 SHA-256 与该现场的 `source-snapshot.json` 全部一致，沿用前节已通过的定向及串行全仓测试，不重复运行。feat-008 保持 in-progress，转码及其他媒体/恢复边界不提升。

本次复核摘要保存为同目录 `recovery-audit.private.json`。HEAD `1e8dbb996a09ae522e783f076281b2748e6896af`；Windows PowerShell 执行 `init.ps1 -Mode docs` 通过（15 份维护文档、360 条本地链接、36 个历史入口、10 个 feature）。WSL Git 的 `git diff --check` 将既有 CRLF 行尾报告为尾随空白，exit 2；随后 Windows Git 的同一检查 exit 0，仅有行尾转换提示。已审阅本次四个状态/证据文件的增量，未提交。

### 2026-11-14 主动群语音播放现场通过

用户随后确认：“收到一条，播放为三个依次降低的提示音”。结合上一节同一消息的正常 OneBot 单次发送、持久 ID、get_msg、登记群和语音元数据，主动群语音发送及 B 端实际播放现场通过。确认另存本机 `local-evidence/snowcat-group-voice-send-field-20261114/recipient-playback-confirmation.private.json` 和 `field-summary.private.json`，保留原 `A/summary.json` 的待确认快照，不覆盖历史。本轮不再发送；已发群语音 SILK 下载、转码和长期资源/恢复仍各自待验，feat-008 保持 in-progress。

### 2026-11-14 已发群语音只读下载与缓存适配

用户确认新 A 已登录主界面后，以唯一新建、无 bridge 的主进程 38428（创建 `2026-11-14T13:51:20.6693180Z`）准入。固定 9.9.36-53644 模块、PE/backend 审计、唯一 envscan、owner 就绪和 LoginInfo 均通过。使用停止发送账本的哈希校验副本，原群语音 `get_msg` HTTP 200 / 0；单次 `get_record(out_format=silk)` HTTP 200 / 1501，未取得下载字节。匹配原消息、元素和请求槽的零错误回调为 `chatType=null`，缓存路径未登记；其它消息/元素回调被过滤。观察 30 秒后正常关闭 daemon 和 owner，均 exit 0；最终健康核对 QQ 响应、HTTP/SSE 关闭，旧 bridge 保留。本轮零发送，不重试，证据在本机 `local-evidence/snowcat-sent-group-voice-readback-20261114/`。

同账号缓存发现与 2,575 字节发送样本长度、MD5、SHA-256 和 SILK 头一致的 `.silk` 文件；缓存文件存在不等于正常下载通过。代码此前只为成功自发私聊登记 `.silk` 缓存，群自发没有登记。新增合成回归先复现匹配群自发缓存回调不能终结，再将精确 pilot 的成功自发 `.silk` 规则扩展到 group。登记仍取自匹配的消息/群/发送者/元素回读，要求 sendStatus=2、同账号规范缓存目录及 MD5 文件名；零错误空会话回调仍须命中精确预登记路径，后续身份/长度/MD5/SILK 验证与缓存保留不变。回归覆盖错误群、非成功发送、错误账号/路径/消息/元素/会话类型/错误码和非 pilot 拒绝。

GitNexus 绑定 `E:\stella\Caligo`，HEAD `1e8dbb9`，刷新索引后查询。FTS 不可用导致概念搜索降级；具名 context/impact 可用，builder 上游为 owner query 和默认 builder，risk LOW，影响 owner 查询流程。测试函数的 UNKNOWN 通过 `#[test]` 入口、唯一文本定义及 Cargo 定向运行补证；嵌入 JS 的影响面用两个媒体调用点补查。完整日志在上述 readback 目录，修复前失败回归和后续验证在本机 `local-evidence/snowcat-sent-group-voice-cache-repair-20261114/`。新构建尚未现场复验，不把本适配记为下载通过。

修复前定向回归因 `exact self-sent group cache callback must finish` 失败；修复后 wrapper 查询合同 7/7。首次 `init.ps1 -Mode all` 在 PowerShell 5 内合并 native stderr 时将 Cargo 编译输出提升为脚本错误，未形成全仓结果，保留 `all-serial.log`。改由外层 shell 捕获原生输出后，同一 `init.ps1 -Mode all` 顺序执行 default/research 两套全仓并通过（含图片合同），日志为 `all-serial-native-streams.log`；该次没有设置 Windows 测试线程上限，不声称进程内单线程测试。Windows Git `diff --check` 通过。GitNexus 全量变更检查覆盖 6 个文件、18 个变化符号，risk LOW；嵌入 JS 的跨语言边界仍以上述补查为准。

隔离 research release 构建通过，目录 `target-harness/snowcat-sent-group-voice-cache-repair-20261114/release/`，bridge SHA-256 `B7D38EBB6890890B4A10B0D1F68C5F61D60EA6FA2363A5E42448AE2385764F8F`。独立只读脚本、原停止账本副本、发送样本、精确 read-plan、源码快照和三项二进制 pins 已准备并核对，尚未接入。现场 runner 使用已验证保留时间戳字符串的 Windows PowerShell 5，脚本语法通过；查询仍只有 get_msg/get_record，各一次、不重试，finally 正常停止并保留 bridge。已请求用户再次正常重启 A，待其就绪后重新准入；旧 A 38428 不重接、不热卸载。feat-008 保持 in-progress。

### 2026-11-14 已发群语音 SILK 现场通过

用户确认再次正常重启 A 并进入聊天主界面。唯一新建、无 bridge 的根进程 38256（创建 `2026-11-14T14:01:04.3746538Z`）通过固定 QQ 9.9.36-53644 模块哈希、PE/backend 审计、唯一 envscan、owner 就绪和 LoginInfo。使用上节修复构建，bridge SHA-256 `B7D38EBB6890890B4A10B0D1F68C5F61D60EA6FA2363A5E42448AE2385764F8F`，源码快照及构建 pins 均匹配；本轮未修改 Rust 或重新构建，沿用上节全仓验证。

在原停止发送账本的哈希校验副本上，对原持久消息各执行一次 `get_msg` 与 `get_record(out_format=silk)`，均 curl exit 0 / HTTP 200 / OneBot 0。零错误空会话类型回调匹配消息、元素、在途请求和预登记缓存路径，进入 `validated`。返回 2,575 字节，与原三音降调发送样本逐字节相同，长度、MD5、SILK 头及 SHA-256 `75547048ca48a4f9d532855b3526c3f3ac6fe56012c2df327426bb803d1f2a6b` 均通过。独立核对消息 ID 和账本登记群一致，匹配账本记录恰有一条。本轮零发送、不重试；群发送及 B 端实际播放证据沿用前轮，原失败样本保留。

daemon 与 owner 正常停止，退出码均为 0。最终健康在 `2026-11-14T14:02:59.5788649Z` 核对同一 QQ 代次仍响应、daemon 已停止、HTTP/SSE 无监听，修复 bridge 保留，不重接或热卸载。现场回执、字节文件、回调和健康证据只留本机 `local-evidence/snowcat-sent-group-voice-cache-repair-20261114/`，摘要为 `field-summary.private.json`、`A/summary.json` 和 `A/final-health.private.json`。

至此，登记群主动语音发送/B 播放与该已发消息的原始 SILK 下载均有现场证据，属于跨代次闭环。结合此前样本，普通私聊与登记群语音的两个方向及原始 SILK 各有对应现场样本；转码、长期资源寿命、恢复及其它媒体仍须独立验收，feat-008 保持 in-progress。下一工作块为语音转码，须先准备工具配置、只读范围与校验方案，再按新 A 准入执行；本轮不自动启动下一现场或提交。

收尾在 HEAD `1e8dbb996a09ae522e783f076281b2748e6896af` 执行 Windows `init.ps1 -Mode docs` 通过（15 份维护文档、362 条本地链接、36 个历史入口、10 个 feature），Windows Git `diff --check` 通过；已审阅本轮状态与验收增量。源码未变，不重复前节已通过的定向/default/research 测试；上述结构检查不提升转码或生产验收状态。

### 2026-11-14 第一阶段现场验收收口

用户要求在此收口第一阶段，将当前修复和验收记录提交至 PR #6，并评论列明验收功能。本次只整理、检查和提交，不启动语音转码或新 QQ 现场。现场范围为 Windows x64、精确 hash 绑定的 QQ 9.9.36-53644、登记 A/B 与测试群；实际模块 hash、构建和进程代次仍由各样本记录约束。以下为跨代次、跨修复构建的指定样本结果，不宣称最终构建已重跑全部现场矩阵。

| 本阶段已验收的功能 | 现场证据与范围 |
|---|---|
| 正常入口当前登录账号与昵称 | [双账号查询](#2026-11-14-双账号登录查询隔离)、[新代次复验](#2026-11-14-新代次双账号-logininfo-现场)；各自身份独立核对 |
| 当前账号资料刷新 | [正常入口 SelfProfile](#2026-11-14-正常入口当前账号资料刷新现场)；不扩展为任意目标或服务器新鲜度保证 |
| 好友列表、群列表、群详情、完整成员列表与指定成员查询 | [A 目录复验](#2026-11-14-a-好友列表复验与群目录计数修复)、[双账号群成员](#2026-11-14-双账号群成员通过与-a-目录诊断)；证据来自登记账号及对应群 |
| 普通私聊与登记群文本入站及主动发送 | [文本入站](#2026-11-14-普通文本入站现场与空提及字段适配)、[登记群回读](#2026-11-14-登记测试群现场-http-回读通过)、[主动私聊](#2026-11-14-caligo-主动私聊现场通过)、[主动群聊](#2026-11-14-caligo-主动群聊现场通过)；SSE/持久账本/get_msg 与对端显示按方向核对 |
| 普通单 PNG 私聊/登记群入站、主动发送与下载 | [私聊入站下载](#2026-11-14-私聊-png-正常下载现场通过)、[群入站下载](#2026-11-14-登记群-png-入站与下载现场通过)、[私聊发送后下载](#2026-11-14-已发送私聊-png-正常下载现场通过)、[群发送后下载](#2026-11-14-已发送群-png-正常下载现场通过)；下载与原图字节/摘要一致，发送的对端可见性另有用户确认 |
| 原始 SILK 私聊/登记群语音入站、主动发送、播放与下载 | [私聊入站下载](#2026-11-14-已收私聊语音-silk-现场通过)、[私聊发送播放](#2026-11-14-主动私聊语音发送现场通过)、[私聊发送后下载](#2026-11-14-已发私聊语音-silk-现场通过)、[群入站下载](#2026-11-14-登记群入站语音-silk-现场通过)、[群发送播放](#2026-11-14-主动群语音播放现场通过)、[群发送后下载](#2026-11-14-已发群语音-silk-现场通过)；对端/接收端实际播放与原始文件验证分列 |
| 双账号 HTTP 鉴权与单端停止隔离、样本收尾 | [双账号隔离](#2026-11-14-双账号登录查询隔离)；交叉 token 403、无 token 401、停 A 后 B 查询可用。媒体样本分别记录正常停止 daemon/owner、HTTP/SSE 关闭及 QQ 响应；不等于长期静默或安全热卸载 |

保留的未验范围：WAV/OGG 转码、@/reply 与其它媒体/组合消息、完整 OneBot transport/真实下游应用矩阵、真实断线重连/换号/core 重启交错、持续身份撤权、长期资源寿命与回调静默。相关部分虽有实现或 LAB 证据，不能算本阶段 FIELD 通过；feat-005/006 的既有 blocked、feat-008/009 的 in-progress 及产品 NOT READY 均不提升。旧失败、unknown 与已消耗请求继续保留。

提交范围为已验证的语音缓存适配、对应正反回归及状态/验收记录；既有本地修复提交 `1e8dbb9` 随同分支推送到 PR #6。最新源码与已通过的 default/research 全仓和隔离 release 快照一致，本次不重复运行 Rust 测试。提交前重新运行文档、暂存差异和完整 GitNexus 变更检查；真实身份、凭据、聊天原文、原始媒体、QQ 二进制及现场脚本仍只留本机 `local-evidence/`。PR 维持现有 draft 状态，不自动合并。

收口检查：Windows `init.ps1 -Mode docs` 通过（15 份维护文档、364 条本地链接、36 个历史入口、10 个 feature）；Windows Git `diff --check` 通过。GitNexus 全量 JSON 覆盖当前 6 文件/18 个变化符号，risk LOW，`partial=false`、`truncated=false`；先前 `1e8dbb9` 的 35 文件/296 符号、83 流程与 CRITICAL 总体风险记录继续保留，不用当前小增量抵消整条 PR 的影响面。已扫描 PR 远端检查点之后新增行中的登记身份及本轮凭据，未命中已知敏感值；该扫描不替代人工审阅。检查日志留在本机 `local-evidence/snowcat-phase1-close-20261114/`。
