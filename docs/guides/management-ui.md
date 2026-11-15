# 本机管理 WebUI

适用：启动时选择账号、核验 QQ 登录、查看本地消息以及管理账号配置。
实现入口：[management_http.rs](../../crates/caligo-core/src/management_http.rs)，
页面：[management_ui.html](../../crates/caligo-core/src/management_ui.html)，
离线合同：[management_http_lab.rs](../../crates/caligo-core/tests/management_http_lab.rs)。
管理器及账号配置合同见 [account-management.md](account-management.md)。

## 启动与装配

面向使用者的入口是 [`caligo.exe manage`](../../crates/caligo-core/src/commands/manager.rs)。
默认使用 `%LOCALAPPDATA%\Caligo`，可用 `--root <绝对路径>` 选择已有私有配置目录。
先打开本机管理页，账号初始全部停止。添加 QQ 号，选中账号后点击「启动托管」。
已有配置会直接列出账号，添加操作在所有账号停止时可用。
`caligo.exe manage --terminal` 在终端执行同样的首次添加和序号选择，启动选中账号并自动输出日志。
终端中输入 `v` 并回车核验登录，按 Ctrl+C 停止服务。
`caligo.exe manager --root <路径> --select --management-bind 127.0.0.1:0` 也可从空闲列表启动，
省略 `--select` 保留旧命令的批量启动行为。

HTTP 组件的装配接口为 `ManagementHttpServer::start(ManagementHttpConfig, Arc<Mutex<AccountManager>>)`；
`ManagementHttpConfig` 仅包含 `bind: SocketAddr` 与 `bearer_token: String`。
上层入口显式选择本机监听地址，并从本机私有 token 文件取得独立管理凭据后传入组件。
管理凭据必须是 64 个十六进制字符，不能放入账号 JSON、URL、命令日志或网页源码。
监听地址只允许 IP loopback；端口 0 可用于离线测试，`local_addr()` 返回实际地址。

浏览器使用 `local_addr()` 对应的精确数字 IP 地址访问首页，例如 `http://127.0.0.1:端口/`。
输入私有管理凭据后点击「连接」，页面通过同源 fetch 读取账号列表。高级面板可读取实际配置。
凭据仅保留在页面内存；输入后清空输入框，页面刷新或点击「锁定页面」后须重新输入。
页面无外网资源、CDN 或框架依赖，无 localStorage/sessionStorage。
所有外部数据通过 `textContent`、`value` 和 DOM 节点显示，不拼接为 HTML。

停止单个账号后管理页继续运行，可重新启动；全部账号停止也不关闭 HTTP。
“关闭管理器”调用鉴权 shutdown 端点，停止并 join 全部账号后通知上层退出。
组件 `shutdown()` 或其 stop flag 只停止 HTTP；进程持有者负责最终 manager 生命周期。

## 接入 QQ 与核验

「等待 QQ 桥接」表示选中的 Caligo 服务已启动。真实 QQ 仍由官方客户端登录，
并由操作者按[原生研究边界](native-research.md)指定新鲜进程、核对模块 SHA-256 与接入合同。
正式 Windows 发布包内的 `caligo.exe` 和 `caligo_bridge.dll` 共用此接入链路。
当前准入对象为精确固定的 QQ 9.9.36-53644。CLI 核验加载模块和进程创建时间，
从唯一 Environment 扫描结果启动 bridge。管理页和终端负责启动所选核心服务。

先完成当次 PE/backend 审计和实例核对。在启动账号前，将已准入 PID 写入 `expect_client_pid`。
使用对应 `accounts.json` 条目的账号、管道前缀和私有 `<data_dir>/auth.token`。
现场基线的 IPC 标识沿用 `qq-9.9.33-52230`，QQ 版本识别使用加载模块的精确哈希。

```powershell
.\caligo.exe cli qq-entry --pid $qqTargetPid --created-filetime $qqCreatedFiletime `
  --bridge $bridgePath --manifest $manifestPath --report $reportPath `
  --daemon-pipe $bridgePipe --daemon-auth-file $accountTokenPath --daemon-account $qqAccount `
  --confirm-designated-test-instance
```

`$qqCreatedFiletime` 取已准入进程快照中的 `StartTime.ToFileTimeUtc()`，bridge 路径使用绝对路径。
`$bridgePipe` 为 `\\.\pipe\<pipe_prefix>-bridge`，管理凭据和账号桥接凭据各自独立。
通过管理入口停止核心服务后，用同一 PID、创建时间、bridge、manifest、report 及确认标志
执行 `cli qq-entry-stop` 停止 QQ 内 owner。关闭超时须保留已映射 bridge。

连接后点击「核验 QQ 登录」，一次只读 `LoginInfo` 成功并返回所选 QQ 号，才显示「登录已核验」。
核验结果随连接变化失效。核验按钮可在桥接就绪后由操作者再次使用。

「消息日志」显示所选账号最近 100 条 Caligo 持久消息记录，包含私聊或群聊、会话、
发送者、时间和正文预览。图片与语音显示类型。重启账号后可查看原账本中的记录，新条目自动刷新。
已保存的记录也可以在等待桥接时查看。日志数据来自 Caligo 账本，范围由既有入站事件和确认发送链路决定。

## HTTP 合同

| 方法和路径 | 行为 |
|---|---|
| `GET /` | 无账号数据的静态管理页，可免 Bearer |
| `GET /v1/accounts` | 账号列表、running、session_active 和当前连接标记 |
| `POST /v1/accounts` | 以 `{"account":"数字 QQ 号"}` 添加私有账号配置 |
| `GET /v1/config` | 实际 manager 配置，无管理或账号 token、无任意 root 字段 |
| `PUT /v1/config` | 严格 JSON 校验、原子保存、磁盘回读，返回回读配置 |
| `POST /v1/accounts/{id}/start` | 按当前 manager 配置启动真实 core daemon |
| `POST /v1/accounts/{id}/stop` | 停止并 join 该账号的真实 core daemon |
| `POST /v1/accounts/{id}/verify-login` | 查询当前桥接会话的 `LoginInfo`，验证账号并返回昵称与连接标记 |
| `GET /v1/accounts/{id}/messages` | 返回启动中的该账号最近 100 条持久消息预览 |
| `GET /v1/accounts/{id}/capabilities` | 当前 ACTION_REGISTRY 声明与 backend 状态 |
| `POST /v1/shutdown` | 停止全部账号并设置管理 HTTP stop flag |

所有 `/v1/*` 必须携带 `Authorization: Bearer 管理凭据`。
写请求使用 `Content-Type: application/json`（允许 `; charset=utf-8`）和明确 Content-Length。
start、stop、verify-login、shutdown 请求体必须是 `{}`，禁止额外字段。
GET 不能有请求体；查询参数和编码路径全部拒绝，不接受 URL token。
错误返回 JSON `error`；401 缺少或错误凭据、403 Host/Origin 拒绝、409 运行中修改或冲突、
400 非法 JSON/配置、415 内容类型错误、500 启停或落盘失败。
底层含路径的错误细节不直接返回浏览器。

Host 必须精确匹配实际监听地址及端口。Origin 若存在，必须是该地址的 HTTP 同源；
无 Origin 的本机 CLI 可用 Bearer 访问。拒绝 cross-site/same-site fetch 元数据，不提供 CORS 放行。
每连接只处理一条 HTTP/1.1 请求并关闭，禁止 Transfer-Encoding、Expect、重复头与请求流水线。
头部上限 16 KiB、正文上限 256 KiB、读请求绝对期限 5 秒，同时最多 8 个 worker。
达到 worker 上限时非阻塞尽力返回 503；停止标志可取消未读完的请求。
响应禁止缓存并使用 CSP nonce、`frame-ancestors 'none'`、no-referrer 和 nosniff。

## 配置与状态证据

配置始终保存在 manager 固定私有 root 下的 `accounts.json`。
JSON schema 与路径/端口/重复账号检查复用 `ManagerConfig`，未知及重复字段严格拒绝。
任一账号 running 时 PUT 全部拒绝为 409。管理端口与账号声明端口冲突也拒绝。
验证成功后由 `save_atomic()` 在同目录写临时文件、同步、原子替换并磁盘回读比较；
只有成功才替换内存中的配置。高级面板显示服务器返回的 JSON，保留账号数字的原始精度。
失败显示真实错误，不显示“保存成功”；底层回读失败可能发生于已替换磁盘之后，应由本机检查恢复。

`running` 只表示 manager 持有的 daemon 线程仍运行。
`session_active` 来自 daemon 内账号 actor 的当前连接状态。
列表的 `online` 保持 null，单次登录核验通过独立端点返回。

能力的 `declared` 直接序列化真实 `ACTION_REGISTRY`，包括每项 permission/schema。
当前 daemon 没有只读广告快照 getter，因此 `advertised:null`、`backend_state:"unknown"`；
这不能声称声明动作已 enabled，也不会通过执行后端查询来猜测可用能力。

## 验证层级

定向命令：`cargo test -p caligo-core --test management_http_lab --locked --offline --target-dir target-harness`。
测试覆盖真实双账号 daemon 启停、当前配置生效、落盘回读与磁盘拒写、Bearer/Origin/Host 拒绝、
JSON/HTTP 边界、worker 容量、读取期限、shutdown、错账号核验拒绝和账本回读隔离。
当前开发验证与现场验收边界见[进度](../../progress.md)。
