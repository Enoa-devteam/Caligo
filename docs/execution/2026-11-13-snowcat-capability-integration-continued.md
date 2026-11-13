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
