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

## 共享入口与主线整合

- `ef238e1`：保存计划、ProfileService research 基线及异常/持有修复。
- `c1b5b1a`：10 个真实 OneBot 动作名（含 3 个 async alias）的 typed registry；支持判断和执行均经 `resolve_action`。参数 schema 和读写权限是发现元数据，不能替代运行时参数校验、鉴权或原生能力判定。定向 `cargo test -p caligo-core --lib onebot --locked --offline --target-dir target-harness` 27/27 PASS，文件 rustfmt 与 diff check PASS。
- 主线 `2436f93` 合并保留 learned group peer 索引、既有 fail-closed 修复与两侧历史证据。公开目录动作的旧占位/空列表成功语义按本计划更正，见 [查询指南](../guides/onebot-queries.md)。
- 主线整合回归：`RUST_TEST_THREADS=1 ./init.sh -Mode all` exit 0，default/research workspace 均 PASS；完整输出在 ignored `local-evidence/snowcat-main-integration-20261113.log`。目录拒绝语义定向 OneBot 29/29、learned peer 索引 1/1 PASS。
- SSE 与 MCP 作为可独立验证的共享协议入口先行；这不表示 A1/A2 已完成。SSE 使用 HTTP POST 之后的有界 fanout；MCP 通过同一 registry 和 action handler。所有原生能力仍按实际 backend 拒绝或执行。

## 后续阶段和停止条件

A0 的固定注册分母与 Caligo 映射审阅完成后，按 A1 宿主/IPC、A2 业务、A3 应用、A4 候选冻结推进，各阶段独立提交。没有真实 backend 的注册、空列表与占位结果不计已接线。未交付 ABI 保持明确失败并登记缺口，其他已有合同的工作继续。

A0–A4 不启动 QQ。B1 对冻结候选做离线矩阵；B2 需要届时指定的新鲜双账号实例和精确模块 profile。历史 unknown 不重发，未证寿命不卸载，旧研究证据不自动提升 Caligo 产品能力。
