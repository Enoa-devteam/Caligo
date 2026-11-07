# Session handoff

## Current Objective

N4-R2/R3 的离线实现与合成合同检查点已提交。最新 R2 LAB 修复让本地 pump 在 generation quarantine 完成后做一次诊断尾槽 drain，清空副本但不应用回执；定向回归与 `init.ps1 -Mode all` 通过。2026-11-07 固定-hash 静态续查了 `D3355C` 第二 wait caller 到 `0x5B060` 的 owner 候选，但未找到严格 RTTI/vtable、valid-`.pdata` direct caller 或 guarded-global `0x67510A0` 引用边；wait return 仍被忽略，manager final strong owner 未识别。用户授权后的一次只读现场探针中，修复版 V8 路径与 Caligo owner pump 成功运行并关闭，但 `getGroupService()` 返回空、resolver API 未派发。QQ send-response callback 生命周期、实际 callback thread、最终 owner、successful wait、QQ 原生 stop/drain/quiescence 与 R3 provider/adapter、response UID→CPB/native ID 强关联仍 open；feat-005/006 保持 blocked，R4 不准入。

## Restartable Next Step

`CALIGO-K1-PROBE-001` 保持为旧崩溃样本，不重放。离线栈审计已定位并修复根因：精确 QQNT.dll 哈希的 `TryCatch` 构造函数在 `this+0x28` 写 flag；旧 `[usize;4]` 与 `scope_dtor` 槽重叠。修复使用 `[usize;6]`（48 字节）。新的 `CALIGO-K1-PROBE-002` 在用户指定的当前 QQ PID 38356 上跑过 `uid_map_v8_ready`、state、binding、wrapper、session 和 GroupService 检查；GroupService 返回空，记录 `api_call_attempted=false`，没有派发 resolver。18 个阶段无失败，owner pump shutdown 回执成功；QQ 未崩溃且仍响应，但 bridge 仍映射在该 QQ 进程内。不要在同一进程重注入或热卸载。

下一步固定-hash R2 owner 查找只处理一个候选：复核 [qq-native-lifetime-contract.md](docs/research/qq-native-lifetime-contract.md) §7.78 中 strict-RTTI `MSFCoreService` slot 16 → `0xB81758` 的 guarded-state setup 路径，检查同一 typed owner teardown 是否释放 `0x67510A0` 或其 pair，并核对该 teardown 若有 wait 是否检查返回值。复用 §7.75/§7.78 的既有固定样本报告，不重跑全映像 state-reference 或 caller census；无法绑定 teardown 时记录有界 no-hit。`D3355C` 第二 caller 的 no-hit 已记录于 §7.92。R3 的运行时 GroupService/provider 尚无结果；不重复本次探针或 ASAR 字面量普查，不猜 `0x9c7a` 语义。PID 38356 仍持有 bridge，不重注入或热卸载；在 R1–R3 与 ABI/线程/寿命门通过前，R4 仍不准入。

## Preserved Unknowns

- 本次实例的 `getGroupService()` 返回空；选中的 Group UID provider、`{uinList:[value]}` 到 N-API array 的 runtime adapter 及 API 响应仍未知，resolver 未派发。
- 实际 UID/UIN 值到 CPB/native identity 的强关联。
- SendMsgResult 条件 CPB slot `+0x158` / key `0x9c7a` 的具体接收类型、字段语义和与 response UID 的对象身份关系。
- QQ callback 的现场 OS thread、final owner、成功 wait、stop/drain/quiescence。
- 旧 dump 本身仍没有 V8 sanity 返回值，且未包含 TryCatch 构造函数或 bridge 调用页。独立的新运行已证明修复版 bridge 可以完成 V8 sanity 与 scope 清理，但不改变旧 dump 的证据内容。
- 本次没有注册 listener、调用发送入口或执行 send-response callback；R2 callback 的现场线程/寿命与 R3 native GroupService resolver 仍未知。

## Scope and Validation

- 本轮动态核验使用两次隔离 offline release build：`cargo build -p caligo-cli --release --features research --locked --offline --target-dir target-harness/n4-r2-r3-dynamic-20261107` 与 `cargo build -p caligo-bridge --release --features research --locked --offline --target-dir target-harness/n4-r2-r3-dynamic-20261107` 均 PASS；bridge SHA-256 为 `0F652675B02365811DC4350BA5004064A4DB31FC65140890A3055DA387FA903C`。当前 `init.ps1 -Mode docs` 与 `git diff --check` 结果见本轮执行记录。
- 前一检查点 `a33780f` 的 `init.ps1 -Mode all`、default/research workspace tests、定向 UID-map script tests 2/2、`rustfmt --check` 与 research release build 均已通过。本轮另外完成 CLI/bridge 隔离 release build 和一次 runtime pilot；未重跑 Cargo tests。
- 本机 9.9.36-53644 临时 pilot manifest、当前 PID 的 envscan、阶段报告、UID-map 结果和脱敏摘要仅在 ignored `local-evidence/n4-r2-r3-closure-20261107/`；正式 adapter manifest 未改。CALIGO-K1-PROBE-002 未发送消息、不注册 listener、不热卸载、不重试；owner pump 已关闭，bridge 继续映射直至 QQ 退出。
- 前一静态审计的 schema-2 计划快照在 39 个 cited paths 中有 35 条记录相对旧 pin 不同；plan digest 未变，细节见 execution ledger。本轮 GitNexus 1.6.12 `list` 将 Caligo 绑定到 HEAD `cc716b8`；`uid_map_probe::run` 与 `pump_cb` context 经源码和现场阶段报告复核。执行记录新增后的文档没有重建索引。
- 2026-11-07 second-wait owner audit: `wrapper.node` hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`; six complete `.pdata` bodies decoded. The `D3355C` wait result is unchecked; the `0x5B060` valid-`.pdata` direct E8 candidate count, strict direct-vtable count, and selected-function RIP refs to `0x67510A0` are all zero. Verifier/report SHA-256 `440AAC1398F88F718F2A47FBA32DD515A8CCDF1DB0329DF8A7129363082086F2` / `91E38400546BC43E81997478BEB756FD78AC68326D53BC0A143A9B8DD7174BDF`; companion RTTI verifier/report `40EEF3E00D964B3F8E729ACD4C8352E90B756236B78D803B7B3D1DEB4658EB49` / `17870B145CAFCB7EF90AF0ED803E9A82F0ABE5AB7F3836BFBF54BABD2278C872`. All artifacts remain under ignored `local-evidence/n4-r2-readonly-20261015/`. Docs validation passed (10 live docs / 202 local links / 33 historical entries / 10 features), JSON and whitespace checks passed. GitNexus CLI 1.6.12 index-only refresh is current at `cb50d79` across 302 files; staged detect covered five files, mapped Section nodes only, reported zero processes/low risk with `partial=false` / `truncated=false`. The full structured result is saved as `local-evidence/n4-r2-readonly-20261015/gitnexus-detect-staged-wait-owner-20261107.json`. This was a no-PDG documentation refresh; source/process graph completeness was not claimed.
- feat-005/006 remain blocked，R4 未准入，产品 NOT READY。本次动态证据与状态文档提交不提升产品能力状态；工作区中的 harness、日志、缓存及其他无关改动保持未纳入提交。
