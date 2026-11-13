# GitNexus 代码智能

本仓库使用 GitNexus 索引 Caligo 符号、关系和执行流。本文定义索引维护、代码影响分析和图检查的必需流程。

## 索引

- 索引陈旧时，从仓库根目录运行 `node .gitnexus/run.cjs analyze --index-only`，它会选择可用 runner。
- 尚无 `.gitnexus/run.cjs` 时，可用 `npx`、`bunx` 或 `pnpm dlx` 启动 GitNexus；例如 `bunx gitnexus@latest analyze`。注意 npm 11 的 npx crash（#1939）。
- 可用资源：`gitnexus://repo/Caligo/context`（概览/索引新鲜度）、`clusters`（功能区域）、`processes`（执行流）和 `process/{name}`（逐步执行轨迹）。

## 编辑前的图分析

- 编辑函数、类或方法前，必须对目标符号运行 `impact` 上游分析；记录 callers、processes 与 risk。优先 MCP `impact({target: "symbolName", direction: "upstream"})`，CLI fallback 为 `node .gitnexus/run.cjs impact "symbolName" --direction upstream --repo .`。
- 对 HIGH/CRITICAL risk 必须预警，不可用 `riskSharedAxes` 抵消。MCP File 结果不含 axes；使用 Graph-RAG 展开 File。
- 将 `risk: UNKNOWN` 当作未解决。`impact` 会在 `riskNote` 解释索引无法回答；空 caller set 可能源于 plain-object 属性访问、动态 dispatch 或跨语言调用不可解析。仅在 walk 为空/UNKNOWN 或目标是字面量时用文本搜索补证，不能按零 caller 放行。
- 只读调用者、依赖、imports 或 execution flow 查询须先查图：概念/流程用 `query({search_query: "concept"})`，具名符号用 `context({name: "symbolName"})`，影响面用 `impact`。图结果为空、UNKNOWN 或字面量定位时，再用文本搜索确认。
- 安全审查使用 `explain({target: "fileOrSymbol"})` 查看 source→sink findings；此功能需要 `analyze --pdg`。
- 符号重命名用 GitNexus `rename`，不要用 find-and-replace；它会遵循调用图更新引用。
- 架构探索读 `.claude/skills/gitnexus-exploring/SKILL.md`；影响分析读 `gitnexus-impact-analysis/SKILL.md`；故障追踪读 `gitnexus-debugging/SKILL.md`；重命名/拆分读 `gitnexus-refactoring/SKILL.md`；工具/resources/schema 读 `gitnexus-guide/SKILL.md`；索引/status/clean/wiki 命令读 `gitnexus-cli/SKILL.md`。

## 收尾与提交前检查

- 提交前必须分析变更图：MCP `detect_changes({scope: "all"})`，或 CLI `node .gitnexus/run.cjs detect-changes --scope all --repo .`。
- 回归对比用 MCP `detect_changes({scope: "compare", base_ref: "main"})` 或 CLI `node .gitnexus/run.cjs detect-changes --scope compare --base-ref "main" --repo .`。
- `partial: true` 或 `truncated: true` 不是 clean check；零值也可能代表图未覆盖，需继续重跑。未分析全量变更前不得提交。

## MCP 资源

| URI | 用途 |
| --- | --- |
| `gitnexus://repo/Caligo/context` | 概览、索引新鲜度 |
| `gitnexus://repo/Caligo/clusters` | 功能区域 |
| `gitnexus://repo/Caligo/processes` | 执行流目录 |
| `gitnexus://repo/Caligo/process/{name}` | 逐步执行轨迹 |
