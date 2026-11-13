# 本机管理 WebUI

适用：通过本机 HTTP 管理账号 core daemon、编辑账号配置或查看动作声明。
实现入口：[management_http.rs](../../crates/caligo-core/src/management_http.rs)，
离线合同：[management_http_lab.rs](../../crates/caligo-core/tests/management_http_lab.rs)。
管理器及账号配置合同见 [account-management.md](account-management.md)。

## 启动与装配

正常管理入口是 [caligo-manager](../../crates/caligo-core/src/bin/caligo-manager.rs)。
HTTP 组件的装配接口为 `ManagementHttpServer::start(ManagementHttpConfig, Arc<Mutex<AccountManager>>)`；
`ManagementHttpConfig` 仅包含 `bind: SocketAddr` 与 `bearer_token: String`。
上层入口显式选择本机监听地址，并从本机私有 token 文件取得独立管理凭据后传入组件。
管理凭据必须是 64 个十六进制字符，不能放入账号 JSON、URL、命令日志或网页源码。
监听地址只允许 IP loopback；端口 0 可用于离线测试，`local_addr()` 返回实际地址。

浏览器使用 `local_addr()` 对应的精确数字 IP 地址访问首页，例如 `http://127.0.0.1:端口/`。
输入私有管理凭据后点击“连接并读取”，页面通过同源 fetch 读取真实账号与实际配置。
凭据仅保留在页面内存；输入后清空输入框，页面刷新或“清除凭据”后须重新输入。
页面无外网资源、CDN 或框架依赖，无 localStorage/sessionStorage。
所有外部数据通过 `textContent`、`value` 和 DOM 节点显示，不拼接为 HTML。

停止单个账号后管理页继续运行，可重新启动；全部账号停止也不关闭 HTTP。
“关闭管理器”调用鉴权 shutdown 端点，停止并 join 全部账号后通知上层退出。
组件 `shutdown()` 或其 stop flag 只停止 HTTP；进程持有者负责最终 manager 生命周期。

## HTTP 合同

| 方法和路径 | 行为 |
|---|---|
| `GET /` | 无账号数据的静态管理页，可免 Bearer |
| `GET /v1/accounts` | 当前配置中的账号、真实 running、session_active 与登录未知说明 |
| `GET /v1/config` | 实际 manager 配置，无管理或账号 token、无任意 root 字段 |
| `PUT /v1/config` | 严格 JSON 校验、原子保存、磁盘回读，返回回读配置 |
| `POST /v1/accounts/{id}/start` | 按当前 manager 配置启动真实 core daemon |
| `POST /v1/accounts/{id}/stop` | 停止并 join 该账号的真实 core daemon |
| `GET /v1/accounts/{id}/capabilities` | 当前 ACTION_REGISTRY 声明与 backend 状态 |
| `POST /v1/shutdown` | 停止全部账号并设置管理 HTTP stop flag |

所有 `/v1/*` 必须携带 `Authorization: Bearer 管理凭据`。
写请求使用 `Content-Type: application/json`（允许 `; charset=utf-8`）和明确 Content-Length。
start、stop、shutdown 请求体必须是 `{}`，禁止额外字段。
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
只有成功才替换内存中的 manager。页面随后再次 GET 配置并显示服务器回读结果。
失败显示真实错误，不显示“保存成功”；底层回读失败可能发生于已替换磁盘之后，应由本机检查恢复。

`running` 只表示 manager 持有的 daemon 线程仍运行。
`session_active` 来自 daemon 的 StoreReader actor 状态；它不是 QQ 登录凭据。
`online` 保持 null，`online_reason` 为 `QQ login not observed`，不能从启动推导为在线。
启动只创建 Caligo core 与管道，不启动、附加或登录 QQ。

能力的 `declared` 直接序列化真实 `ACTION_REGISTRY`，包括每项 permission/schema。
当前 daemon 没有只读广告快照 getter，因此 `advertised:null`、`backend_state:"unknown"`；
这不能声称声明动作已 enabled，也不会通过执行后端查询来猜测可用能力。

## 验证层级

定向命令：`cargo test -p caligo-core --test management_http_lab --locked --offline --target-dir target-harness`。
测试覆盖真实双账号 daemon 启停、当前配置生效、落盘回读与磁盘拒写、Bearer/Origin/Host 拒绝、
JSON/HTTP 边界、worker 容量、读取期限、shutdown、页面静态控件与 fetch 路由合同。
这些是离线 TCP/Windows 管道证据，不能替代浏览器实际视觉检查、真实 QQ 登录或产品验收。
主入口装配、实际浏览器验证及全仓状态记录由主任务统一收尾。
