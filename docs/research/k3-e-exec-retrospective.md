# K3-E 常驻 JS 实验归档

2026-10-07 的历史研究，2026-11-15 整理自本机 `local-evidence/k3-e/` 的
3 份笔记、3 份 Crashpad 帧摘录、2 份 envscan 和 23 份加载器探针报告。
历史基线为 Windows x64 QQ 9.9.33-52230（aff854e8），
`QQNT.dll` 的基线 SHA-256 为
`F6F35B8517DA5C38ACE8D4B6ABAD1ABED42F61C8E706D23192A22D91C7620E59`。
帧摘录未记录模块哈希，旧 bridge 的哈希和匹配 PDB 仍缺失。偏移仅按该历史
基线解读，不能套用到其他构建。[固定版本清单](../contracts/version-adapter-manifest.json)

## 原方案与停止决定

`design.md` 拟通过 `caligo_exec_run(ctx)` 把调用方提供的 JS 投递到 QQ
主环境的循环线程。设计上限为 256 KiB 脚本和 64 KiB 结果。主环境的
`globalThis.__caligo_res` 保存会话引用、监听器、200 条接收环和
`lastSend`，由版本化的 start、poll、send、stop 资产管理。计划沿用
manifest 校验和指定实例确认，验收序列为 ARM、轮询基线、发送一次、
核对回显与对端、STOP、确认监听已移除。

`final-retrospective.md` 判负跨注入的通用 exec 与旧上下文钉扎方案，停止继续
消耗宿主实例。保留禁用的 exec 代码，后续回到固定脚本 ID、每次注入无状态的
`uv_async` 操作，参数由加载器传递。复盘记录历史 async 路径有 60 余次探针
无事故。监听跨注入累积和发送参数化的证据分别见 K3-D 与 K3-F 的历史验收。
K3-E 本身未完成上述验收序列。[K3-D 接收](../acceptance/k3-d-receive.md)、
[K3 首次发送](../acceptance/k3-send-001.md)、[K3-F 参数化发送](../acceptance/k3-f-param-send.md)

## 事故与归因修订

| 台账 | 可复核证据 | 历史结论与边界 |
| --- | --- | --- |
| I-03 | `63a82da9` 的帧 3 为 `QQNT.dll+0x429A0D1`。帧 4、5 为 `caligo_bridge_e_100637.dll+0x1AD19/+0x14B3C`，帧 6 为 KERNEL32。 | `incident-stale-env.md` 初版怀疑数小时前扫描的 env 陈旧。映射检查无法证明 env 仍有效，于是增加主 vtable 新鲜度门。 |
| I-04 | `9a0d383c` 的帧 0–3 与 I-03 一致。帧 4、5 为 `caligo_bridge_k3e_f.dll+0x1AEC4/+0x14C2C`，帧 6 为 KERNEL32。 | 新鲜 env 仍出现同型故障。现场笔记与修复提交将两次事故归因于远程线程执行 V8 上下文阶梯，访问调用线程的 V8 TLS。事故后将阶梯移入 `exec_cb`，保留 env 新鲜度检查。 |
| I-05 | 复盘记录用钉扎的 hint 上下文执行 JS 时发生 V8 内部崩溃。目录中未找到对应帧摘录。 | 跨时间复用上下文是复盘归因。精确时间、调用帧和故障机制仍待原件支持。 |
| I-06 | `7f75e838` 共 29 帧，帧 0–5 落在 QQNT.dll，帧 6 为 `caligo_bridge_diag2.dll+0x1A858`。 | K3-F 之后的 drain/重试注入事故。bridge 在栈上，帧的语义与崩溃因果未定位。 |

Crashpad 摘录只有帧、模块、地址和偏移。PID、时间、线程和异常机制依赖
独立记录。早期笔记和 K4 台账对时区标注有分歧，复盘中的 I-05 时间也与前两起
记录的排序不一致。复盘提到当日共损失四个实例，其三行 K3-E 事故表没有
给出第四起的独立对应关系。I-06 单列为 K3-F 之后的事故。
[完整事故台账](k4-incident-register.md)、
[阶梯移线程的修复提交](https://github.com/Enoa-devteam/Caligo/commit/1554b4b)、
[判负提交](https://github.com/Enoa-devteam/Caligo/commit/94630e1)

## 原始材料索引

以下文件名以本机 `local-evidence/k3-e/` 为根。公开记录保留模块名与相对
偏移，原始帧地址、绝对路径、进程快照与探针载荷由原件提供。

| 文件 | 可复核内容 |
| --- | --- |
| `design.md`、`incident-stale-env.md`、`final-retrospective.md` | 方案、初判及更正、停止决定。曾入库的事故笔记保留在历史提交。 |
| `63a82da9-*.json`、`9a0d383c-*.json`、`7f75e838-*.json` | I-03、I-04、I-06 的 8、8、29 帧。前两份曾入库的摘录保留在历史提交。 |
| `envscan-pid44028.json`、`envscan-pid49688.json` | 各记录五个扫描目标的候选指针。地址只对采样时的进程有效。 |
| `arm-probe.json`、`arm2-probe.json`、`cnt{1,2}.json`、`cyc-arm.json`、`cyc-arm2.json`、`diag{1,2,3,4}.json` | 10 份加载器报告，模块快照均标记完整。 |
| `f-arm-probe.json`、`f-arm{2,3}-probe.json`、`f-poll{1,2,3,4,5}-probe.json`、`f-send-probe.json`、`f-stop-probe.json` | 10 份加载器报告，模块快照均标记完整。 |
| `g-p1.json`、`verify-clean.json`、`verify-clean2.json` | 3 份后续观测与清洁检查的加载器报告，模块快照均标记完整。 |

23 份加载器 JSON 记录的是 bridge 版本、ABI 和宿主模块快照，模块数为
26 或 28。发送、轮询和停止结果需要对应的业务报告，由各阶段验收或事故记录引用。
原目录的三个已跟踪文件可从[整理前的版本](https://github.com/Enoa-devteam/Caligo/tree/bcba478afabeaee4d440bd7ada7a54d4d922d5e0/local-evidence/k3-e)追溯。
