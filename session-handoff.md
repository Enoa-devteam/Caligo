# Session handoff

## Current Objective

N4-R2/R3 的离线实现与合成合同检查点已提交。新增静态复核确认固定构建的 GroupService backend/schema/N-API 方法表相邻可证，但选中的运行时 provider、`{uinList:[value]}` 到 wrapper array 的适配、以及 response UID→CPB/native ID 强关联仍未证明。QQ 现场 callback 生命周期与 R2 owner/thread/wait/quiescence 也仍 open；R4 不准入。

## Restartable Next Step

不要重放 `CALIGO-K1-PROBE-001`，也不要自动改选别的 QQ PID。离线栈审计已闭合该 UID-map sanity pilot 的根因：精确 QQNT.dll 哈希的 `TryCatch` 构造函数在 `this+0x28` 写 flag；旧 `[usize;4]` 为 32 字节，Rust 生成的 `this=RBP-0x10` 与析构函数指针槽 `RBP+0x18` 相距正好 `0x28`，构造函数将指针低字节从 `0xB0` 写成 `0xB6`。源码已改为 `[usize;6]`，隔离 research release build 通过；新反汇编把 `TryCatch` 存储扩到 48 字节，并将析构指针槽放到 `RBP+0x28`。修复后的 DLL 没有重新加载进 QQ；不要把离线修复当作现场验证。

下一离线步回到 R2：按固定-hash 证据继续核对 callback OS thread、最终强 owner、成功 wait 和完整 stop/drain/quiescence 的缺口；把静态 caller/释放候选与现场执行分开记录。R3 provider/adapter 静态范围已由 Phase4150/4151/4163/4165/4170–4173 覆盖，本次未找到能继续闭合的精确身份边；若无新的 hash-bound 输入，不重复 ASAR 字面量普查，也不猜 `0x9c7a` 语义。R3 强身份仍 unsupported。任何现场动作都须满足当次准入合同；R1–R3 与 ABI/线程/寿命门通过之前不准入 R4。

## Preserved Unknowns

- 选中的 Group UID provider、{uinList:[value]} 到 N-API array 的 runtime adapter。
- 实际 UID/UIN 值及 API 响应，不同请求到 CPB/native identity 的强关联。
- SendMsgResult 条件 CPB slot `+0x158` / key `0x9c7a` 的具体接收类型、字段语义和与 response UID 的对象身份关系。
- QQ callback 的现场 OS thread、final owner、成功 wait、stop/drain/quiescence。
- 本次 dump 没有 V8 sanity 返回值；是否成功运行完整 JS、具体 V8 调用结果仍未知。minidump 未包含 TryCatch 构造函数或 bridge 调用页，因此静态写入与堆栈变化的对应关系尚未由现场指令页独立复核。
- 修复后的代码没有运行在 QQ 中；R2/R3 回调/身份现场门、runtime provider/result 和 native GroupService resolver 仍未知。

## Scope and Validation

- 本轮 `.\init.ps1 -Mode docs` PASS（10 live docs / 199 本地链接 / 33 历史入口 / 10 features）；`git diff --check` PASS，Git 只报告现有 LF→CRLF 转换提示。
- 前一检查点 `a33780f` 的 `.\init.ps1 -Mode all`、default/research workspace tests、定向 UID-map script tests 2/2、`rustfmt --check` 与 research release build 均已通过；本次文档静态复核未重跑 Cargo tests/build。
- 本机 9.9.36-53644 临时 pilot manifest、envscan、阶段报告与脱敏 Crashpad 摘要仅在 ignored local-evidence/n4-r2-r3-closure-20261107/；正式 adapter manifest 未改。此次一次性运行未发送消息、不注册 listener、不热卸载、不重试。
- 本轮开工时 schema-2 计划快照在 39 个 cited paths 中有 35 条记录相对旧 pin 不同；plan digest 未变，快照和当前工作区分类见 execution ledger。本轮 GitNexus 1.6.12 schema-4 状态在 HEAD `a33780f` up-to-date、302 覆盖文件且无 incomplete reasons；`drain_responses` 与 `cpb_event_from_record` context 经源码复核。文档更新后的 staged/all 图门与最终索引状态见本节对应 execution ledger 收尾记录。
- feat-005/006 remain blocked，R4 未准入，产品 NOT READY。本阶段只提交 R3 静态边界与 handoff/progress 更新；工作区中的 harness、日志、缓存及其他无关改动保持未纳入提交。
