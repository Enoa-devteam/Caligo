# Session handoff

## Current Objective

N4-R2/R3 的离线实现与合成合同检查点已提交。用户授权后新增一次固定 hash 的只读现场探针：修复版 V8 路径与 Caligo owner pump 成功运行并关闭，但当前 `getGroupService()` 返回空，resolver API 未派发。QQ send-response callback 生命周期、R2 最终 owner/thread/wait/quiescence、R3 provider/adapter 与 response UID→CPB/native ID 强关联仍 open；R4 不准入。

## Restartable Next Step

`CALIGO-K1-PROBE-001` 保持为旧崩溃样本，不重放。离线栈审计已定位并修复根因：精确 QQNT.dll 哈希的 `TryCatch` 构造函数在 `this+0x28` 写 flag；旧 `[usize;4]` 与 `scope_dtor` 槽重叠。修复使用 `[usize;6]`（48 字节）。新的 `CALIGO-K1-PROBE-002` 在用户指定的当前 QQ PID 38356 上跑过 `uid_map_v8_ready`、state、binding、wrapper、session 和 GroupService 检查；GroupService 返回空，记录 `api_call_attempted=false`，没有派发 resolver。18 个阶段无失败，owner pump shutdown 回执成功；QQ 未崩溃且仍响应，但 bridge 仍映射在该 QQ 进程内。不要在同一进程重注入或热卸载。

下一步先做 R2 的离线固定-hash 生命周期闭合：核对 callback OS thread、最终强 owner、成功 wait、stop/drain/quiescence 与资源所有权，明确哪些由 LAB/静态证据闭合、哪些仍需专门准入现场观察。R3 的运行时 GroupService/provider 尚无结果；不重复本次探针或 ASAR 字面量普查，不猜 `0x9c7a` 语义。新运行的原始身份材料仅在 ignored `local-evidence/n4-r2-r3-closure-20261107/`。在 R1–R3 与 ABI/线程/寿命门通过前，R4 仍不准入。

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
- feat-005/006 remain blocked，R4 未准入，产品 NOT READY。本次动态证据与状态文档提交不提升产品能力状态；工作区中的 harness、日志、缓存及其他无关改动保持未纳入提交。
