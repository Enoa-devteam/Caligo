# 多账号与私有配置

适用：由正常 core 入口装配多账号 daemon、持久配置和协议 listener。
依据：[account_manager.rs](../../crates/caligo-core/src/account_manager.rs)、
[离线合同测试](../../crates/caligo-core/tests/account_manager_lab.rs)。
本页描述源码合同；真实 QQ 登录、收发和长期运行验收仍由
[progress](../../progress.md) 管理。

## 配置与路径

`ManagerConfig` 的 JSON schema 为 `version: 1` 与非空 `accounts` 数组，
最多 128 项。每个 `AccountSpec` 必填：

| 字段 | 合同 |
|---|---|
| account | 正 `u64`，配置中唯一；序列化为 JSON 数字 |
| data_dir | root 下单个 ASCII 字母、数字、下划线或连字符目录名，最长 64 字节；拒绝 Windows 设备名 |
| pipe_prefix | 短 ASCII 字母、数字、下划线或连字符名称，最长 160 字节 |
| module_baseline | 非空、无控制字符、最多 256 字节；由 bridge Hello 核对，不能代替精确模块准入 |
| expect_client_pid | bridge 客户端 PID 校验；0 关闭此项，control 仍使用认证 |
| ports | http、websocket、sse、mcp，各为 `null` 或非零 `u16` |

所有层级均使用 serde `deny_unknown_fields`，拒绝多余字段、重复字段和
错误类型。账号、大小写折叠后的目录名与 pipe prefix 不得重复；所有
非空端口跨账号及协议唯一。配置不含 token 内容或任意文件路径。

root 为调用方指定的本地磁盘绝对路径，拒绝 UNC、设备路径、父目录跳转、
NUL 与路径链上的 reparse point。新 root 和账号目录在创建时即使用
当前用户 SID 专有、受保护且可继承的 DACL。已有路径必须为当前用户
所有且仅向当前用户授予访问；不自动修复过宽权限。权限读取失败、
不支持的 ACE、目录代替文件以及坏凭据均明确失败。

每个账号目录固定包含 `journal.jsonl`、`journal.messages.jsonl`、
`auth.token` 与 `manager.lock`。manager 持有独占文件句柄直到该账号所有
线程 join，防止另一 manager 即使使用不同 pipe 名也复用同一账本目录。
root/account 路径链拒绝 reparse point；目录名合同同时排除路径逃逸和 ADS。
同用户恶意进程可改动用户自己的文件，不属于此 DACL 的强隔离边界。

真实账号、凭据、聊天和日志应位于被忽略的 `local-evidence/` 或仓库外
私有目录，不得新增入库。临时离线测试使用独立新目录和合成账号。

## 管理 API 与装配

`AccountManager::new(root, config)` 校验规格和私有目录，持有配置，尚不
启动 QQ 或创建虚假登录态。`config()` 返回不含秘密值的规格，`root()`
返回私有路径；调用方不应将私有路径随意暴露为网络管理响应。

- `start(account) -> Result<Arc<Daemon>, ManagerError>` 创建并严格回读 token，
  打开该账号账本，运行真实 `Daemon::run` 线程。必须等 bridge/control
  两条 pipe 建立并完成带认证的 control Hello 后才返回成功。
- `start_all()` 启动尚未启动的账号。某项失败时，逆序停止并 join 本批已
  启动项；之前已经运行的账号继续运行，历史 journal/token 保留供恢复。
- `daemon(account)` 提供内部受信装配句柄。其 `auth_token()` 仅供已有
  bridge/control 私有启动通道，不应放入 JSON 管理输出或日志。
- `action_handler(account)` 复用共享 OneBot 动作层；持有的旧 handler 在
  账号 stop 后返回明确失败，不能继续使用停止实例发送。
- `stop(account)` 设置该账号停止标志，等待 daemon 的两条 pipe owner
  线程退出及 actor 收尾，释放独占目录句柄，不影响其他账号。
- `shutdown()` 对全部运行项执行 stop/join；`Drop` 也执行相同清理。
  未知账号报错；重复 stop 已配置但未运行的账号是幂等成功。

manager 本身不绑定协议端口。正常 CLI/binary 应读取 validated ports，
按账号将 HTTP/WS/SSE/MCP listener 接到该账号 `Arc<Daemon>` 与共享动作层，
使用该 daemon 的 stop flag 回收协议 worker。listener bind 失败同样须
停止/join 本次启动的 daemon；禁止以配置读回成功代替 listener 就绪。

## 配置与凭据持久化

`ManagerConfig::save_atomic(root)` 在同目录创建随机临时文件，检查其私有
权限，写入并 `sync_all`，以 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`
原子替换 `accounts.json`，重新读取、严格解析并与配置比较。
`ManagerConfig::load(root)` 验证私有权限、限制大小并解析同一固定文件。
写入、替换、回读任一步失败均返回失败；未替换的旧文件保持可读。
只读目标等失败不静默改成内存成功，临时文件尽力移除。

`auth.token` 必须恰为 64 个十六进制 ASCII 字节。不存在时生成 OS 随机
32 字节，使用同一原子写入及读回合同；存在但不可读或格式错误时拒绝
启动，不生成替代 token。秘密不进入 config/schema、序列化响应或错误文本。
配置与 token 是两个独立文件操作，不声称跨文件事务；启动回滚保留
已持久化凭据和历史账本，不删除证据或自动重发 unknown 请求。

## 离线验证入口

```powershell
cargo test -p caligo-core --test account_manager_lab --locked --offline --target-dir target-harness
cargo test -p caligo-core --test account_manager_lab --features research --locked --offline --target-dir target-harness
.\init.ps1 -Mode docs
```

测试覆盖两个真实 daemon/pipe、相同 native ID 的账号隔离、journal/message
恢复、token 重启连续性、单账号停止、共享动作路由、冲突拒绝、严格 JSON、
配置原子写后回读、失败替换保留旧配置、坏凭据拒绝、并发目录占用拒绝及
真实 pipe 创建失败后的批量回滚。这些证据不证明 QQ 现场登录或业务送达。
