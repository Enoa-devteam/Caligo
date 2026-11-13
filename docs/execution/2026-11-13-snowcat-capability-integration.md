# SnowCat 能力整合执行记录

依据：[阶段计划](../plans/2026-11-13-gitnexus-plan-snowcat-capability-integration.md)。日期为 2026-11-13（Asia/Shanghai）。本记录区分源码、接线、离线测试和现场验收。

## A0：固定基线（进行中）

- 新分支：`codex/snowcat-capability-integration`，起点 `598b22b1218a67d45eeb8d5546a5a03e356bb534`。
- 固定参考：私有 `Napcat-Snowluma-research` 的 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64`。参考仅用于静态行为合同和清单，不引入其生产后端、脚本、反编译/重建 C/C++ 或二进制依赖。
- 起始工作树已有 ProfileService research 增量，以及 GitNexus 指令/技能、日志、崩溃文件和缓存。初始 tracked diff 保存于 ignored `local-evidence/snowcat-integration-startup-20261113.patch`；只将与本阶段有关且经审阅的文件纳入提交。
- GitHub API 确认实际仓库为 `Enoa-devteam/Caligo`，旧 origin URL 会重定向。刷新时 `origin/main` 为 `2436f93`，已合入 PR #2/#3；它和起点相对 merge-base `5aaab95` 分别有 9/21 个提交。完成基线保存后整合主线，以保留双方证据和实现。
- GitNexus 1.6.12 绑定本 checkout `E:/stella/Caligo`，索引 `598b22b`。`impact handle_action` 和 `impact is_supported_action` 均报告 CRITICAL、78 impacted / 46 direct / 23 processes。输出包含同名符号及宽泛测试流的混合，需结合文件与实际源码核对；不把该风险降为零。注册层变更需覆盖共享协议装配。

### 起始验证

- `./init.sh -Mode docs`：PASS，10 live docs / 210 local links / 34 historical entries / 10 features。
- `RUST_TEST_THREADS=1 ./init.sh -Mode all`：exit 0，default 与 research workspace tests 均通过。完整输出仅在 ignored `local-evidence/snowcat-integration-baseline-20261113.log`。这是起始工作树离线基线，不是候选冻结或 QQ 验收。
- 编译器提示已有重复 `no_mangle` 属性、未使用变量/代码等告警；不以通过测试声称无告警。
- A0 基线审阅修复：ProfileService acquisition 异常统一清除全局状态；跨 owner tick 保留 session；非 Map 返回继续拒绝，并吸收跨上下文 Promise/thenable 拒绝。`cargo test -p caligo-bridge --lib --features research uid_map_script_tests --locked --offline --target-dir target-harness -- --test-threads=1`：4/4 PASS，覆盖双向 exact-key Map、异常清理、session 持有、foreign Promise 与 delayed thenable。`qq_v8.rs` 单文件 rustfmt 检查仍报告既有格式差异，未批量格式化。

## A0：固定参考清单

[机器清单](../contracts/snowcat-capability-inventory.json) 由[纯静态抽取器](../../scripts/extract-snowcat-capabilities.mjs)读取固定 Git blob 生成；引用 645 份源码的 blob/SHA-256/行号，不执行参考代码，不读取 C/C++。207 次显式实例化对应 206 个基础动作、412 个生成别名、618 个最终注册键；重复 check_url_safely 导致 3 个键被覆盖。显式测试动作 test_download_stream 单列，排除后基础动作分母为 205；两个 decorator 测试没有静态 import 可达注册。router 210 项中独有 4 项，不能用于替代真实注册分母。

清单另列 38 个事件类（26 concrete）、23 消息段、7 网络 adapter 与一个第二协议 scaffold、26 包目录、122 入口导出声明、9 原生组件和 3,297 归档导出行、1,354 wrapper 方法声明、129 WebUI route 与 50 工具入口。它们分别是声明/注册分母，不是 Caligo 支持计数。参考 MCP 的首次初始化接线缺口、第二协议空 Map 与权限/精确版本未知均保留。

`SNOWCAT_REFERENCE=E:/stella/Snowcat/Napcat-Snowluma-research node --test scripts/extract-snowcat-capabilities.test.mjs`：6/6 PASS，含全部引用哈希/行号和逐字节再生。未设置该环境变量时为 5 PASS / 1 SKIP，不能把跳过的固定参考检查计为通过。独立完整输出在 ignored `local-evidence/snowcat-inventory-pinned-tests-20261113.log`。

## A0：Caligo 实现映射

[逐项映射](../contracts/caligo-snowcat-capability-map.json) 与固定参考 inventory 分离；共 5,711 条 reference ID、16 类。每条通过共用 assessment 解析实现位置、正常入口、权限、backend 依赖、版本和验收层级，支持人工复核，不以名称相近映射实现。

[覆盖检查器](../../scripts/check-snowcat-coverage.mjs) 实际运行 [Rust registry 导出](../../crates/caligo-core/examples/dump_onebot_registry.rs)，核对名称、alias、schema、语义哈希和全分母。基础 206 项中 199 missing、3 partial（群/inferred 文本与未绑定的登录资料）、1 私聊 backend blocked、3 wired offline（本地 get_msg、actor status、version）。412 个参考 alias 中 409 missing、2 partial、1 blocked。wired offline 仍不表示 native 查询或真实 QQ 通过，尤其 get_msg 的 native 投影未闭合。

`node scripts/check-snowcat-coverage.mjs` 与 `COVERAGE_RUN_RUST=1 node --test scripts/check-snowcat-coverage.test.mjs`：当前源码 Rust 导出及 12/12 测试 PASS，无跳过；含漏项、重复、分母/registry 漂移与虚假能力提升拒绝。registry 为 7 基础名/3 async alias，语义 SHA-256 `514bfbbb74f41ce2c5638bd583c1183854cc177f2d0b3d8f65db4c53f04cac02`。参考 inventory SHA-256 维持 `89de4711e2577f5531ba38b282bc47dea38c5a4ed0da4d416867c320fc45bc25`。全清单 native/field 已验收均为 0。

## 共享入口与主线整合

- `ef238e1`：保存计划、ProfileService research 基线及异常/持有修复。
- `c1b5b1a`：10 个真实 OneBot 动作名（含 3 个 async alias）的 typed registry；支持判断和执行均经 `resolve_action`。参数 schema 和读写权限是发现元数据，不能替代运行时参数校验、鉴权或原生能力判定。定向 `cargo test -p caligo-core --lib onebot --locked --offline --target-dir target-harness` 27/27 PASS，文件 rustfmt 与 diff check PASS。
- 主线 `2436f93` 合并保留 learned group peer 索引、既有 fail-closed 修复与两侧历史证据。公开目录动作的旧占位/空列表成功语义按本计划更正，见 [查询指南](../guides/onebot-queries.md)。
- 主线整合回归：`RUST_TEST_THREADS=1 ./init.sh -Mode all` exit 0，default/research workspace 均 PASS；完整输出在 ignored `local-evidence/snowcat-main-integration-20261113.log`。目录拒绝语义定向 OneBot 29/29、learned peer 索引 1/1 PASS。
- SSE 与 MCP 作为可独立验证的共享协议入口先行；这不表示 A1/A2 已完成。SSE 使用 HTTP POST 之后的有界 fanout；MCP 通过同一 registry 和 action handler。所有原生能力仍按实际 backend 拒绝或执行。

## A1：typed wrapper 接收至 journal

生产 `D8_START_JS` 监听 recv/update/add-send，完整消息有界入队，不裁剪 ID、正文或 JSON。Rust 保存启动捕获的 wrapper session；批次须匹配该 sid 及账号、host nonce、会话代次。固定 `msgId/msgSeq/msgRandom/clientSeq/msgTime` 字符串保持原精度；时间单位依据固定参考 `business-message/service-entry.ts:39–40` 与 `RESULTS.md` 秒级对照。

`wrapper_message` owned 元数据经 owner decoder、Resident、IPC、Daemon 和 AccountActor 写入 journal/recovery。旧 journal/IPC 可缺省该可选字段。普通文本才进入此路径；缺身份、媒体/@/未知元素及超限记录分别拒绝或计溢出，坏记录不阻塞后续有效记录。AddSend 保留原来源并以 self-sent 首次记录入账，InfoListUpdate 为更新；不从 Promise 或字符串序号制造 OneBot 身份。

队列预算计入保留的 raw metadata；bridge 未确认窗口按完整序列化 payload 计数，pop/ACK 扣减实际被移除的记录，修正旧的误减下一条记录问题。`MsgEvent::queue_bytes` 用于 core 入队、恢复重入和出队。OneBot projection 始终 `None`，本块终点为 journal，不声明 native get_msg 或强回执闭合。

定向测试：`wrapper_message_chain` 1/1、`daemon_client_lab` 6/6、recovery 10/10、runtime 11/11、resident 20/20、typed model 1/1、bridge daemon-client unit 4/4、UID-map script 4/4 均 PASS。完整链实际使用生产 D8 JS → decoder → Resident → `event_payload` → 真实 pipe daemon → journal recovery，含 10k 字符文本、大整数、拒绝/溢出、错 sid/account/host/generation、旧代次审计、AddSend/Update 和 metadata 容量拒绝。定向输出在 ignored `target-harness/d8-final-test.log` 与 `target-harness/d8-window-test.log`。

## A3 先行：SSE/MCP 正常入口

`caligod --onebot-sse-bind <loopback> --onebot-mcp-bind <loopback>` 装配两个可选 listener，共享 daemon token、动作 registry 和停止标志，见[应用入口指南](../guides/onebot-app-entries.md)。正常 `caligod_smoke` 子进程通过鉴权 SSE、MCP tools/list 和 control Stop，1/1 PASS。

SSE default/research 各 unit 3/3、真实 TCP integration 8/8 PASS；HTTP POST quick-operation 后 fanout、双客户端事件字节、鉴权、头/连接/事件上限、心跳、注销与停止有覆盖。确定性部分写入/持续 WouldBlock 验证总期限与停止；同步填满真实 EventBus 验证 3 drops 后写前拒绝/零输出/统计/注销。最初真实慢 socket fixture 未在期限内制造溢出，32MiB 与 4KiB socket buffer 尝试仍被 Windows loopback 吸收；相关失败保留，已删除平台缓冲假设，不将该样本计为 TCP 背压通过。确定性 unit 两构建各额外 3 轮全部通过。

全仓首次复验在 SSE 测试辅助删除的中间态编译失败；第二次在 MCP 两个尚未更新/隔离的 fixture 断言失败（Accept 406 vs 200、async fixture 混入错误测试），原始输出保留在 ignored `local-evidence/snowcat-integration-final-all-20261113{,-r2}.log`。第三次全仓仍发现重复 Accept 头的 fixture 返回 400；修正后 `cargo test -p caligo-core --test onebot_mcp_lab --locked --offline --target-dir target-harness -- --test-threads=1` 6/6 PASS（`local-evidence/snowcat-mcp-final-20261113.log`）。MCP 支持 2025-03-26 / 2025-06-18，默认无版本头按 2025-03-26；Origin loopback、Accept 双类型、版本头、GET 405、总写期限和空 token 拒绝已核对。工具调用以文本返回完整 OneBot JSON，async 受理不误标业务失败。最终全仓结果以修正后的后续记录为准。

## 本批最终验证

- **最终全仓双构建 PASS**：`RUST_TEST_THREADS=1 ./init.sh -Mode all` exit 0，default/research workspace 全部通过。代码基线为 `4f90fb0` 加本批已审阅 SSE/MCP 装配工作树；最终提交仅保存该通过版本及证据。输出：ignored `local-evidence/snowcat-integration-final-all-20261113-r4.log`。
- 并行 `cargo test --workspace --features caligo-core/research,caligo-cli/research --locked --offline --target-dir target-harness` exit 0。本批未复现原先共享 native fixture 失败；不据一次并行通过撤销历史失败。输出：ignored `local-evidence/snowcat-integration-parallel-research-20261113.log`。
- 最终 MCP 6/6，SSE 每构建 unit 3/3 + TCP 8/8，D8 chain research 1/1，正常 caligod smoke 每构建 1/1 均包含在上述全仓门。
- 文档、diff、能力映射与固定分母检查的最终结果随收尾提交记录；既有编译告警仍存在，未实施全仓格式化。
- 本批完成 A0、A1 的 D8 子块及 A3 的 SSE/MCP 子块，未满足完整 A1–A4。D9 持久请求关联/强回执与查询、媒体和其余目录/扩展动作、多账号/WebUI/第二协议/部署仍在逐项清单中保留缺口。A4 未冻结，B1–B3 未执行；没有 QQ 现场操作，产品 NOT READY。

## 后续阶段和停止条件

A0 的固定注册分母与 Caligo 映射审阅完成后，按 A1 宿主/IPC、A2 业务、A3 应用、A4 候选冻结推进，各阶段独立提交。没有真实 backend 的注册、空列表与占位结果不计已接线。未交付 ABI 保持明确失败并登记缺口，其他已有合同的工作继续。

A0–A4 不启动 QQ。B1 对冻结候选做离线矩阵；B2 需要届时指定的新鲜双账号实例和精确模块 profile。历史 unknown 不重发，未证寿命不卸载，旧研究证据不自动提升 Caligo 产品能力。
