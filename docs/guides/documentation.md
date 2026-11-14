# 文档与指令维护

适用：新增规则、更新项目状态、编写专题指南、归档证据或结束会话。
设计来源：[第四讲：把指令拆分到不同文件里](https://walkinglabs.github.io/learn-harness-engineering/zh/lectures/lecture-04-why-one-giant-instruction-file-fails/)，
实施工具：[harness-creator](https://github.com/walkinglabs/learn-harness-engineering/blob/main/skills/harness-creator/SKILL.md)（可选本机安装）。
这里的行数预算是本仓库维护选择，不是模型效果保证或 token 估算。

## 权威位置与按需加载

| 信息 | 唯一维护位置 | 读取条件 |
|---|---|---|
| 全局硬约束、任务入口 | [AGENTS](../../AGENTS.md) | 每次任务 |
| 项目介绍与首次导航 | [README](../../README.md) | 新读者 |
| 模块/开发/现场/文档指导 | 本目录四份专题指南 | 命中 AGENTS 的对应触发条件 |
| 最新总体状态与本次验证 | [progress](../../progress.md) | 任务开始及结束 |
| 工作块 status、依赖、完成条件 | [feature_list](../../feature_list.json) | 选择/推进产品工作块 |
| 下次会话的短交接 | [session-handoff](../../session-handoff.md) | 恢复工作 |
| 历史计划、事故、研究与验收 | [docs 索引](../README.md) 指向原路径 | 当前任务需要具体证据 |
| 类型、参数、版本常量 | Rust 源码、Cargo manifests、contracts | 修改对应实现 |

progress 解释当前验收边界，feature_list 表达各工作块状态；handoff 只保存下一步指针，
README 只路由状态，避免重复维护整套进度。发生冲突时核对当前 HEAD 与证据，
显式记录矛盾和未验证部分，不自动选择更乐观的结论。

## 新规则放在哪里

1. 只对某类任务适用的规则放对应专题，在 AGENTS 保留一行触发链接。
2. 每条规则写清来源、适用条件、复审/失效条件；全局研究规则的来源表在 native-research。
3. 类型/接口细节贴近代码；状态、执行记录和历史教训分别进入状态文件与台账。
4. 新专题确有独立触发条件时才拆分；先检查现有四页是否能够承载。
5. 本仓库根 AGENTS 预算为 100 行；本目录专题每页 150 行以内。
   超预算时按任务分支抽出参考，保留完整事实与原路径。

旧计划的大段 implementation context JSON 与长台账都是按需证据，
不自动注入入口，也不为缩短上下文而删掉历史。
有日期的研究/验收保留当时结论，通过新记录或本页导航说明后续进展。
如需勘误，附日期、来源与被替换结论，保持原始证据可追溯。

## 更新状态与交接（End of Session）

1. 用当前 HEAD、最新台账及源码交叉核对变更；现场健康不能从旧 PID 推断。
2. 对涉及工作块更新 feature 的 status、evidence_paths、done_criteria 与验收层级。
   `done` 要有实际证据；实现/接线/LAB 通过但现场门未闭合可保持 in-progress/blocked。
3. progress 更新日期、范围、实际命令/结果、阻塞项；保留历史台账入口。
4. handoff 写明本次范围、变更文件、下一步与停止条件，并链接 progress 证据。
5. 运行 `.\init.ps1 -Mode docs` 与 `git diff --check`；源码改动另按 development 跑适用测试。
6. 评估是否出现重复规则、坏链接或陈旧状态；同次修正导航，不重写旧验收。

schema 的 feature ID 用 `feat-NNN`；`plan_steps` 保留 N0–N8 的原编号。
依赖描述完整产品闭合顺序，可提前开展离线子任务，但不能凭先行代码消除验收依赖。
用户当前请求优先，不能因为跟踪表有未完成 feature 而擅自扩展任务。

## 机械检查的范围

[check-docs.mjs](../../scripts/check-docs.mjs) 检查维护中入口/指南的本地链接、
预算、历史 Markdown 导航覆盖、feature ID/依赖/证据路径以及 skill 安装锁文件。
它不读取真实 QQ 数据，不检查外部网页可用性，也不证明叙述与现场效果一致。
历史原文链接不在 lint 范围，避免把旧环境路径误当当前可用路径。

上游 harness validator 使用英文结构关键字打分，所以入口与交接保留少量双语标题。
分数反映结构齐备性；真实效果需在代表任务上观察，不能据此宣称 agent 更可靠。
规则审计在文档改动、阶段切换和版本迁移时进行；过期规则删除/替换同时更新来源。
