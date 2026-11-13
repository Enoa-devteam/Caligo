# 来源登记(source-register)

> K0 交付物。状态冻结于 2026-10-06(Asia/Shanghai)。
> 状态取值:`reference`(仅参考,不产生实现约束)/ `adopted`(已采用为依赖,须固定版本)/ `rejected`(明确不采用)/ `pending`(待审,未决定)。
> 纪律:采用代码必须固定 commit 或 Cargo.lock 版本;公共网页动态内容记录抓取时间;不研究、不复用 SnowLuma 专有二进制;任何来源的"描述"不当作已验证的 ABI 事实。

## 登记

| ID | 来源 | 类型 | 版本/Commit 固定 | 抓取/登记时间 | 状态 | 用途与边界 |
|---|---|---|---|---|---|---|
| S1 | [OneBot 11 标准](https://github.com/botuniverse/onebot-11)(api/public.md、ws.md、ws-reverse.md、authorization.md、event/meta.md) | 公开协议标准 | 待 K5 固定具体 commit(计划 §K5 要求,不把 mutable master 当永久 pin) | 2026-10-06 登记 URL | reference | K5 OneBot 行为契约的唯一行为依据;K0/K1 不产生实现 |
| S2 | [Node-API 官方文档](https://nodejs.org/api/n-api.html) | 官方文档 | 抓取时间 2026-10-06 | 2026-10-06 | reference | Node-API ABI 保证只针对自身;不等于 QQ 私有接口保证。K1 评估加载分支时引用 |
| S3 | [Microsoft PE Format 文档](https://learn.microsoft.com/windows/win32/debug/pe-format) | 官方文档 | 抓取时间 2026-10-06 | 2026-10-06 | reference | caligo-cli 的 PE 导出表解析实现依据 |
| S4 | Rust 标准库 / cargo / rustup 文档(随工具链 1.97.1 发行) | 工具链文档 | rustc 1.97.1 (8bab26f4f) | 2026-10-06 | reference | 工程基础 |
| S5 | windows-sys crate([microsoft/windows-rs](https://github.com/microsoft/windows-rs)) | 公开 Rust 依赖 | Cargo.lock 锁定(见仓库根 Cargo.lock) | 2026-10-06 采用 | adopted | caligo-cli 进程/模块枚举与(受门控的)加载器 Win32 调用 |
| S15 | iced-x86 crate([icedland/iced](https://github.com/icedland/iced),Apache-2.0) | 公开 Rust 依赖 | 1.21.0,Cargo.lock 锁定 | 2026-10-06 采用 | adopted | caligo-cli `disasm` 子命令,F-1 路线的 x64 离线反汇编 |
| S6 | sha2 crate([RustCrypto/hashes](https://github.com/RustCrypto/hashes)) | 公开 Rust 依赖 | Cargo.lock 锁定 | 2026-10-06 采用 | adopted | caligo-cli 模块指纹(SHA-256) |
| S7 | serde / serde_json crate([serde-rs](https://github.com/serde-rs)) | 公开 Rust 依赖 | Cargo.lock 锁定 | 2026-10-06 采用 | adopted | version-adapter-manifest.json 读取与 probe 报告输出 |
| S8 | Stella 归档研究报告 `2026-10-06-snowluma-rust-native-reimplementation-research.md`(归档仓库 HEAD eb5a135b) | 内部研究报告 | 归档仓库 HEAD eb5a135b890fd7876ca1f15b9eab03fe881f6cf0 | 2026-10-06 | reference | 只作背景与范围台账;不是实现输入,不提供 Caligo 代码 pin |
| S9 | `capability-inventory.json`(研究报告附件,203 个静态 wire 名称) | 内部调查附件 | untracked 摘要见计划 §11 | 2026-10-06 | reference | 仅用于后续覆盖跟踪;不作为代码生成输入或 clean-room 证明 |
| S10 | PMHQ | 第三方项目 | 未固定 | - | rejected(本轮) | 公开配置声明需要 manager 凭据;仅列调查候选,不作为执行依赖 |
| S11 | Lagrange NativeAPI | 第三方项目 | 未固定 | - | rejected(本轮) | C ABI 不改变其 C# 实现事实;仅调查候选 |
| S12 | Mania | 第三方项目 | 未固定 | - | rejected(本轮) | 已归档且声明发送能力不完整;仅调查候选 |
| S13 | QQNT 公开架构资料:①[go-cqhttp issue #2471](https://github.com/Mrs4s/go-cqhttp/issues/2471)(wrapper/major 加载关系);②[解析NTQQ数据库(lengyue.me)](https://lengyue.me)(旧版本 wrapper.node 数据库密钥函数);③[NapCat 文档](https://napneko.github.io/guide/napcat)(调用层级描述) | 社区公开资料 | 抓取时间均为 2026-10-06 | 2026-10-06 | pending(线索已核对一轮) | 仅作 K1 调查线索;核对结论记于 native-entry-contract §5(其中 ②的 `nt_sqlite3_key_v2` 在本版本导出表中**不存在**,不得按旧版本外推);不复制其代码,不引用其运行时行为为已验证事实 |
| S14 | 本机 QQ 安装(`D:\Program Files\Tencent\QQNT`,版本 9.9.33-52230-aff854e8) | 本机二进制 | 模块 SHA-256 见 `environment.json` 与 `docs/contracts/version-adapter-manifest.json` | 2026-10-06 | adopted(实验对象) | K1 起的静态观察与受控实验对象;其二进制不入库、不随项目分发 |

## 纪律说明

1. S13 类社区资料只回答"去哪里看",不回答"是什么"。任何进入入口契约的结论必须来自:本机静态观察(导出表、文件元数据)、受控运行观察、或 S1/S2/S3 类可核对标准。
2. 采用任何新第三方代码前,先在本文件加行并固定版本;无来源或来源不可核对的代码不得进入构建。
3. 拒绝清单不是永久判决:若 A 路线失败需评估 B 路线(计划 §12.3),再按当时证据重新登记。

## R3 增补（2026-10-08）

| ID | 来源 | 类型 | 版本/Commit 固定 | 抓取/登记时间 | 状态 | 用途与边界 |
|---|---|---|---|---|---|---|
| S16 | 本机静态证据树 `E:/stella/_reference/qq-native-r3-p1-20261008/`（phaseA–J 反编译导出、字节级扫描脚本、外部 manifest） | 内部调查产物 | 目录内 artifact 清单；wrapper 锚点仅对 SHA `63112ab9…` 有效 | 2026-10-08 | adopted（P1/P2 证据基础） | 线程/寿命合同与 capability profile 的证据来源；Ghidra 伪代码不是符号事实，ABI 以汇编为凭 |
| S17 | `caligo-cli observe-msf`（本仓库 `crates/caligo-cli/src/observe_msf.rs`） | 自有工具 | 随仓库提交 6f85350+ | 2026-10-08 | adopted | 外部只读现场观测（RPM，零注入）；输出即 P1/P6 证据格式 |
| S18 | 实例 47524 现场观测（创建 2026-10-08T11:58:14Z UTC） | 受控现场证据 | `docs/execution/2026-10-08-native-contract-recovery/evidence/p1-observe-field-47524-t{1,2,3}.json` | 2026-10-08 | adopted（一次性证据） | 运行时 switch 分支、transport/dispatcher 拓扑的现场确认；不构成发送/接收准入（profile `gates` 为准） |

## SnowCat 研究增补（2026-11-13）

| ID | 来源 | 类型 | 版本/Commit 固定 | 登记时间 | 状态 | 用途与边界 |
|---|---|---|---|---|---|---|
| S19 | [Napcat-Snowluma-research](https://github.com/Eternal-Wanderer-Vegetable/Napcat-Snowluma-research/tree/7cf025fe1e7f9c609e02a8643c6e6398eeec0d64) | 私有研究归档 | `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64` | 2026-11-13 | reference | Wrapper 服务调用参数、结果及场景对照。Caligo 自有 Rust bridge 复用既有 owner pump，未引入归档中的反编译 C、重建 C/C++、脚本或二进制依赖。 |
