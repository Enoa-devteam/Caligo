# Session handoff

## Current Objective

N4-R2/R3 的离线实现与合成合同阶段已记录收口；QQ 现场回调/生命周期验收和 R3 强身份关联仍 open。私有 Caligo_research 的 Phase4113/4165/4173 说明：固定构建的 GroupService 原生 UIN→UID backend 已静态映射，但 {uinList:[value]} 到 wrapper array 的适配与 provider 仍未连接。

## Restartable Next Step

不要重放 CALIGO-K1-PROBE-001，也不要自动改选别的 QQ PID。离线栈审计已闭合该 UID-map sanity pilot 的根因：精确 QQNT.dll 哈希的 `TryCatch` 构造函数在 `this+0x28` 写 flag；旧 `[usize;4]` 为 32 字节，Rust 生成的 `this=RBP-0x10` 与析构函数指针槽 `RBP+0x18` 相距正好 `0x28`，构造函数将指针低字节从 `0xB0` 写成 `0xB6`。源码已改为 `[usize;6]`，隔离 research release build 通过；新反汇编把 `TryCatch` 存储扩到 48 字节，并将析构指针槽放到 `RBP+0x28`。这仍是离线源码/build 证据；没有重新加载 QQ，也没有运行测试。

下一离线步按精确构建证据继续收敛 R2/R3：优先核对 GroupService UID provider、`{uinList:[value]}` 到 N-API array 的适配，以及 response UID 与 CPB/native identity 的强关联。源码审计确认 response UID 目前只用于诊断指纹；CPB 接收路径因缺少已证 `native_id`/`sender_uin` 而 fail closed。只有找到 hash-bound 字段映射或取得获准的成对回执/CPB 样本，才能关闭 R3 身份桥；否则保持 unsupported。R2 的 callback OS thread、final owner、成功 wait、stop/drain/quiescence 仍未闭合。R1–R3 与 ABI/线程/寿命门通过且获得新的现场任务授权之前，不准入 R4；不重放旧探针、不自动选 PID。

## Preserved Unknowns

- 选中的 Group UID provider、{uinList:[value]} 到 N-API array 的 runtime adapter。
- 实际 UID/UIN 值及 API 响应，不同请求到 CPB/native identity 的强关联。
- QQ callback 的现场 OS thread、final owner、成功 wait、stop/drain/quiescence。
- 本次 dump 没有 V8 sanity 返回值；是否成功运行完整 JS、具体 V8 调用结果仍未知。minidump 未包含 TryCatch 构造函数或 bridge 调用页，因此静态写入与堆栈变化的对应关系尚未由现场指令页独立复核。
- 修复后的代码没有运行在 QQ 中；R2/R3 回调/身份现场门、runtime provider/result 和 native GroupService resolver 仍未知。

## Scope and Validation

- 本轮文档检查 `init.ps1 -Mode docs` PASS（10 live docs / 198 本地链接 / 33 历史入口 / 10 features）；`git diff --check` PASS，Git 只报告现有 LF→CRLF 转换提示。
- .\init.ps1 -Mode all PASS；docs 10/195/33/10；default 与 research workspace tests PASS；定向 UID-map script tests 2/2；rustfmt --check 与 research release build PASS。
- 本机 9.9.36-53644 临时 pilot manifest、envscan、阶段报告与脱敏 Crashpad 摘要仅在 ignored local-evidence/n4-r2-r3-closure-20261107/；正式 adapter manifest 未改。此次一次性运行未发送消息、不注册 listener、不热卸载、不重试。
- GitNexus PDG rebuild 触及 16 GiB DB 上限；刷新后的 no-PDG schema-4 索引在 HEAD `5aaab95` up-to-date，覆盖 302 文件、无 `incompleteReasons`。分析器仍有 33>32 callable-value 候选上限、30 处跨语言字段拒绝，以及 393/593 entry candidates、1,065 callees、16 walks 被 flow 预算截断。结构化 staged/all `detect_changes` 均返回 `partial=false`、`truncated=false`，但聚合风险为 critical、受影响流程计数在重跑时波动；热点含共享 `callback_tid`、研究 CLI 的 `uid_map_timeout_ms` 和文档标题 `Caligo 文档索引`，不能当作真实运行时 blast radius。最新全量 JSON 留在本机 `local-evidence/n4-r2-r3-closure-20261107/gitnexus-final-{staged,all}.json`；本轮未重跑 Cargo tests 或 QQ runtime。
- feat-005/006 remain blocked，R4 未准入，产品 NOT READY。本阶段 R2/R3 相关代码与证据作为独立 checkpoint 提交；工作区中的 harness、日志、缓存及其他无关改动保持未纳入提交。
