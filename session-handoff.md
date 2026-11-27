# Session handoff

2026-11-27 feat-007 N5 最终 24h（issue #16）：修正 attempt 2 的登录查询与无消费者缺口。新构建 `17f5ec6`（`caligo.exe` `4C9B057D…02BB`、bridge `659CE4D1…CE14`）增加持久登录缓存（`login.private.json`；默认 `get_login_info` 走缓存，`no_cache` 走 native）与 forward-WS 事件消费者（`scripts/n5/ws_drain.ps1`，只记录计数）。离线启动包/未知版本门 PASS；`init.ps1 -Mode all` default 509 / research 569、0 failed。最终 24h 于 `2026-11-27T01:38:44Z` 在新主根 `33284`（创建 `2026-11-27T01:37:42.0474720Z`，账号 B）启动，8h 处发现 WS 消费者 5s 取消重连（5,157 次、309 条）与 control `DrainEvents` 诊断队列满时拒绝 wrapper/projection 事件（2,848 Gap）；control Stop 后 daemon 未退出，force-kill，bridge 保留。已修正队列语义与 WS 30s 取消/45s 停止等待，新增 runtime/wrapper 回归，`init.ps1 -Mode all` default 510 / research 570；新构建 `e9db64a`（EXE `58D9CAE9…2C99`）启动包/未知版本门 PASS，最终 24h 需新新鲜 QQ 重跑，原定时任务已取消。QQ 重启后新代次重绑与真实 TCP 背压仍未闭合。原始材料只在 ignored `local-evidence/feat007-n5-final-20261127-unbound/`。 [验收记录](docs/acceptance/2026-11-25-feat-007-n5-release.md)

2026-11-25 feat-007 N5 离线启动包/未知版本门 + 2h 现场观察通过（issue #16）：`scripts/n5/verify-release.ps1` 的 default release 打包、sidecar/build-info/包内哈希、两次 packaged daemon 启动与诚实无 bridge 登录失败、篡改 manifest 的 gate 1 拒绝均 pass；固定 `caligo.exe` `820781DB…57CE5`、bridge `1EBF6E35…44FC`。现场新 QQ `26620`（创建 `2026-11-25T12:03:11.4979693Z`，账号 B）一次 attach，零发送；117 samples、351/351 查询成功、3/3 次 core 崩溃重启约 1.5s 恢复、495 条入站事件无重复/缺口、控制停止后 bridge 保留。2h 样本不证明 24h/QQ 重启新代次/真实 TCP 背压；`CoreStopped` 后同代次不能重新 attach。已在新主根 `17504`（创建 `2026-11-25T14:54:38.7754423Z`，账号 B）完成 preflight 并启动 24h soak；attempt 1 在约 8h 处 incomplete（`2026-11-25T23:00:15Z`）：version/status 958/958 成功、journal 持续增长，但 `get_login_info` 从 `15:19:49Z` 起长期返回 1502（456 次），第 2 次 core 重启后登录查询 188s 超时触发 fatal。已确认 1502 是 QQ 锁屏/会话不可用时的诚实返回；`n5_soak.py` 已改为 version/status 作为探测与恢复信号、登录查询单独计数（离线 rehearsal 通过）。24h attempt 1 未通过，原定时核验任务已取消。修正判据后 attempt 2 于 `2026-11-25T23:33:25Z` 在新主根 `43444`（创建 `2026-11-25T23:31:19.6213631Z`，账号 B）重跑，完整跑满 86,420s：version/status 2876/2876 成功、3/3 次 core 重启恢复（约 0.2s）、bridge 保留、QQ 响应、内存/句柄稳定；但 journal 含 13,234 条 Gap（本轮无 OneBot 事件消费者，队列满后的有界拒绝），`get_login_info` 1438 次中 215 次成功、265 次 1502、958 次 10s 超时。常驻链 24h 通过，应用级 24h 与登录动作可靠性仍未闭合；需 WS/SSE 消费者参与并修正 default 登录查询（应优先返回持久身份或有界失败）后另行现场复验。原定时任务已取消，issue #16 不关闭。QQ 重启后新代次重绑与真实 TCP 背压仍未闭合。原始材料只在 ignored `local-evidence/feat007-n5-20261125-unbound/`、`feat007-n5-24h-20261125-unbound/` 与 `feat007-n5-24h-20261126-unbound/`。 [验收记录](docs/acceptance/2026-11-25-feat-007-n5-release.md)

2026-11-24 feat-006 现场 attempt 2 通过（issue #15）：新 QQ `31584` 一次 attach，NoneBot2 经反向 WS 收到私聊 36 / 群 41，各回复一次（37/42）；应用与 Caligo `get_msg` 均成功，账本与发送方一致；同一消息有签名 HTTP POST 与正向 WS 证据。慢阶段 90 在慢 POST/超时下仍到达应用（积压后约 38 秒），daemon 记录 23 failed / 66 delivered / 0 dropped；该阶段自动回复请求 `onebot-85939363ae79fe15-4` 为 `DeliveryUnknown`（账本另存自发消息 106），未重发。控制停止 daemon；默认 bridge 无 research owner-stop（0xe0），bridge 保留、QQ 响应。runner 的 `app_get_msg` 误报已由 `verify-evidence.py` 离线更正，9/9 criteria true。操作者已确认 A 收到 37、42、106；issue #15 按证据关闭；feat-006 保持 in-progress（依赖 feat-005 未 done），慢 POST 积压延迟 WS 投递列为后续风险。证据见 [acceptance §6.2](docs/acceptance/2026-11-24-feat-006-text-mvp.md#62-现场-attempt-2通过单构建)。

2026-11-24 PR #20（`codex/feat-006-text-mvp` → `main`）：issue #15 阻塞点判定、传输协议/慢连接回归与可观测日志、现场包与 attempt 记录已提交推送；分支含 `3d2ef1b`/`cfc9168`/`109140e`/`6983f77`/`a7b62d6` 及现场记录提交。GitNexus 刷新索引后 detect 8 files / 34 symbols / 14 flows / high、`impact serve_ws_socket` CRITICAL 已核对（改动为日志/测试路径）。全仓 default 506 / research 566，0 failed。

2026-11-24 feat-005 缺口转 issue：生产链路已打通，R2（网络撤权现场）与 R3（身份切换现场）按项目决定暂不补齐，转 issue #13/#14 跟踪；feat-006–feat-010 拆为 issue #15–#19，避免 PR #12 过大。本 PR 保持已交付范围。

2026-11-24 feat-005 R3 跨进程新代次现场（部分）：`21844`（创建 `2026-11-24T11:33:24.5363966Z`）preflight PASS，以显式 `session_generation=2` 完成一次 attach（观察者、self 查询、1000 ms 观察停止通过、零发送）；新 journal 只含 gen2 事件，旧 gen1 journal（SHA-256 `87B4E8A9…D6D6A`）未变、native ID 无交集。另一账号实例 `20304` 因多登录标记被 preflight 拒绝，无法形成第二个可 attach 主根；“旧身份失效”仍只由旧实例停止 + 新代次绑定间接体现，R3 不宣称通过。现场包 `feat005-r3-rebind-20261124-unbound/`（清单 `74190B92…CE31`）。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#620-r3-跨进程现场显式新代次与账本隔离2026-11-24)

2026-11-22 feat-005 R3 账本隔离契约（离线）：`session_revocation_contract` 新增第 3 条——新 actor 绑定代次 2 使用独立 journal 时，旧请求不可见、旧事件不在队列、旧 journal 字节不变；测试 3/3，串行全仓 default **502** / research **562**、0 failed。daemon 单账号设计拒绝改账号 Hello，R3 现场因此改为“新 daemon + 新 journal + 新代次绑定”，旧实例保留旧账本。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#619-r3-跨进程新代次与账本隔离契约离线2026-11-22)

2026-11-22 feat-005 R3 现场中止（桌面 QQ 限制）：实例 `31800`（创建 `2026-11-22T14:25:57.9185900Z`）preflight PASS、attach、观察者与 `READY_WATCH identity` 通过；操作员说明桌面 QQ 不支持同实例退出并登录另一账号，无法产生换号事件，按操作员要求中止，零发送。中止未执行观察停止，QQ 仍加载 `caligo_bridge.dll`（保留、不热卸载、不重接），实例消耗。R3 改为跨进程新代次验证（新实例登录另一测试身份、显式新代次绑定；旧代次拒绝/旧事件审计/旧请求不重放），离线契约见 `session_revocation_contract`。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#618-r3-现场中止与桌面-qq-限制2026-11-22)

2026-11-22 feat-005 自动撤权触发与 R2n 现场包（离线）：runner 新增 `--watch-revoke identity|network`（检测 `identity_state`/`network_status` 变化后调用 `daemon-control revoke-*` 并记录 `RevokeAck`）与 `--classify-test`；包重建为 R2n：源码 `8439a45`、EXE `A40A81E5…`、bridge `4A25B810…`、caligod `CDCF8A79…`，清单 `8AEDCBE1…A05D`。剩余：撤权后的重新核实/重绑未自动化；R3 换号与 R2 真实断网现场待执行。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#617-自动撤权触发与-r2n-现场包离线2026-11-22)

2026-11-22 feat-005 控制面撤权接线（离线）：新增 `ControlMsg::RevokeSession`/`CoreToControlMsg::RevokeAck`，daemon 将 `network_changed`/`identity_changed` 映射到 `revoke_session` 并返回取消/未知计数；CLI `daemon-control ... revoke-network|revoke-identity` 在拒绝时 exit 2。serde 往返 1 条，串行全仓 default **501** / research **561**、0 failed。剩余：runner 尚未把 `network_status`/`identity_state` 行自动映射为撤权调用；现场可先手动执行 CLI 动作。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#616-控制面撤权接线离线2026-11-22)

2026-11-22 feat-005 会话撤权原语与契约测试（离线）：AccountActor::revoke_session(RevokeReason) 支持 NetworkChanged（要求同宿主/账号/代次重绑）与 IdentityChanged（要求更新的 session_generation）；撤权把 Queued/Accepted → CancelledUnsent、DispatchIntent/NativeStarted → DeliveryUnknown，写 SessionRevoked 审计；恢复不重放，旧代次事件只审计。新增 session_revocation_contract 2/2，串行全仓 default **500** / research **560**、0 failed。**接线缺口**：观察者行目前只写研究报告 JSONL，core 控制面还没有撤权消息/CLI 动作；下一步新增 ControlMsg/CLI 动作，由 runner 在
etwork_status/identity_state 变化时触发撤权，再显式重绑。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#615-会话撤权原语与契约测试离线2026-11-22)

2026-11-22 feat-005 R2 网络观察现场包（离线准备）：构建 `8439a45`（EXE `2E643C6F…`、bridge `EF52E979…`、caligod `0D0218AF…`）通过 `qq-surface` 的 `network_observer` 字段确认；ignored 包 `feat005-r2-network-20261122-unbound/`（6 文件，清单 `8AEDCBE1…A05D`）已备、未绑定。流程：准入 → attach → 确认 listener 与 observer → self 查询 → `READY_NETWORK` → 操作员对测试 QQ 做一次网络状态变化（回环 IPC 保持、QQ 不退出）→ 采集有界 `network_state`（≤600 秒）→ 1000 ms 观察停止；零发送。失败语义：observer 未 armed、窗口无事件、停止非零均失败且不重试。首次在线尝试（PID 8036）因窗口内未执行网络断开而无事件，已确认是干预缺失、不是观察者结论；已补 setter 注册错误/arity 诊断。下一步：用户重启并登录后跑 preflight + run-field，看到 `READY_NETWORK` 后断网 20–60 秒再恢复；结果用于确定网络回调参数形状，再接撤权/重绑。`6544` 已停止、bridge 保留、不可重接；R3 换号仍待身份源接线。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#62-r2-网络观察现场包离线准备2026-11-22)

2026-11-21 feat-005 R2/R3 研究型网络观察者（离线实现）：research + 带 account 的 attach 时，`D8_START_JS` 注册 session 的 `setOnNetworkChanged`/`setOnMsfStatusChanged`/`setOnWeakNetChanged`；回调只记录有界形状（≤4 参、数字/布尔保留值、字符串只写长度、对象键名 ≤16、≤32 行 + 溢出标记），不记录字符串内容或身份；owner 泵在 poll 成功后执行 `D8_NETWORK_EVENTS_JS` 写 `network_state` 行并清空，`R.stopped` 后 no-op。新增 Node VM 夹具 `network_observer.rs` 1/1；全仓 default 498 / research 557。构建 `02c15b0`：EXE `4303D964…`、bridge `DAD17EEE…`、caligod `5837BAE7…`。限制：setter 可能覆盖 QQ 自身处理器、仅研究构建；尚未接撤权/重绑与 ProfileService 轮询；下一步可准备 R2 现场包（断网/恢复，观察 network_state），R3 换号仍待身份源接线。`6544` 已停止、bridge 保留、不可重接。[研究记录](docs/research/2026-11-21-login-network-notification-surface.md#61-研究型网络观察者离线实现2026-11-21)

2026-11-21 feat-005 R2/R3 在线表面探针（成功）：新实例 `6544`（创建 `2026-11-21T14:30:27.0613801Z`）用 `d2b981e` 构建通过准入，一次 attach、self 查询通过；`surface_probe ok=true` 返回 64 项会话原型。确认会话对象**没有** `getLoginService`/`getNetworkService`/`getNetwork`/`getNTWrapperNetwork`；通知面在 session 原型：`setOnNetworkChanged`、`setOnMsfStatusChanged`、`setOnWeakNetChanged`、`onLine`、`offLine`、`offLineSync`、`onNetReply`、`onMsfPush`；`getMsgService` 含 `getOnLineDev`/`kickOffLine`/`setStatus`/`fetchStatusMgrInfo`；`getProfileService` 含 `startStatusPolling`/`getSelfStatus`/`getStatusInfo`/`getCoreInfo`。1000 ms 观察停止全阶段通过、exit 0、无 reason；零发送、零 listener。证据 `attempt3-pid6544-success/`（29 文件；审计 `F7EDA652…`、索引 `A6F8ED6A…`）。下一步：研究型 observer（session setter + ProfileService 轮询，未知码只审计、不自动恢复），Node VM 夹具锁定后执行 R2/R3 现场。`6544` 已停止、bridge 保留、不可重接；新现场需重启；旧请求不重发。[研究记录](docs/research/2026-11-21-login-network-notification-surface.md#6-第三次在线探针结果2026-11-21成功)

2026-11-21 feat-005 R2/R3 只读表面探针（实现与两次在线修正，待新代次复验）：bridge 研究导出 `caligo_qq_surface_probe`（序号 27）由 owner 泵在 `OWNER_INIT` 后执行 `D8_SURFACE_PROBE_JS`，把有界属性名/`typeof` 写入入口 report 的 `surface_probe` 行；CLI 新增 `qq-surface`（沿用 `qq-status` 的准入与已映射 bridge 校验，零发送、不注册 listener）；Node VM 夹具 `surface_probe.rs` 验证有界与无泄漏。构建 pin：EXE `83342968…`、bridge `73476CA8…`、caligod `D02B07AA…`；现场包 ignored `feat005-surface-probe-20261121-unbound/`（6 文件，清单 SHA-256 `421690AB…1F46`）已备、未绑定。离线验证：导出存在、CLI 用法、gate 解析、脚本/PS 语法、拒绝路径均通过。下一步：用户重启并登录后按 README 跑 preflight + run-field（attach → self 查询 → qq-surface → 观察停止），用 `surface_probe` 行确定登录/网络服务访问器与 listener 形状；首次在线尝试（PID 54416）返回 surface_probe unavailable，已改为仅在 poll 成功的 tick 执行并做有界重试，失败写入具体码；该实例已消耗，第二次在线尝试（PID 56396）返回 bad_json（3 KiB 预截断），已改为先校验再序列化；实例已消耗，待新代次复验。R2/R3 仍未准入、未执行。`38604` 已停止、bridge 保留、不可重接；旧请求不重发。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md#5-只读探针已实现离线2026-11-21)、[合同补充](docs/acceptance/2026-11-14-session-revocation.md#2026-11-21-静态补充r2r3-通知面已定位)

2026-11-21 feat-005 R2/R3 阻塞项：持续通知面静态定位（离线）：固定构建字符串确认登录侧 `NodeIKernelLoginService.addKernelLoginListener`（1 参）与 `nodeIKernelLoginListener` 的 `onLoginState/onLoginConnected/onLoginDisConnected/onQRCodeLoginSucceed` 等；网络侧 `NodeIQQNTWrapperNetwork.getNetworkStatus/start/stop` 与 `nodeIQQNTWrapperNetworkListener.onNetworkStatusChanged`（kernel 侧 `addNetworkListener` 的 `networkConnect/onLine/offLine`）。仍缺：会话到服务的访问器、回调参数/枚举、线程与移除返回、事件归属与撤权接线。下一步：新鲜实例上做**零发送只读表面探针**（不注册 listener、不发消息），再设计撤权/重绑接线；R2/R3 现场仍不执行。`38604` 已停止、bridge 保留、不可重接；后续现场需重启并重新准入；旧请求不重发。 [研究记录](docs/research/2026-11-21-login-network-notification-surface.md)、[合同补充](docs/acceptance/2026-11-14-session-revocation.md#2026-11-21-静态补充r2r3-通知面已定位)

2026-11-21 feat-005 native 三方向复核（离线）：结论见 [研究记录](docs/research/2026-11-21-native-holder-release-drain.md) 与 acceptance §23。生产链路常驻对象 `R` 为双根（全局 `__caligo_d8` + 在途 Promise/回读闭包）；停止只删 `__caligo_res`、移除 listener、关 owner pump，最终释放最早在宿主退出；`--stop-observe-ms` 的 drain 只等 Caligo 结果 ACK + worker 退出，QQ dispatcher 无 stop/cancel/quiescence 证据。新增 Node VM 夹具证明：删 VM 全局后停止快照 `UNAVAILABLE`，resolve 旧 promise 仍追加 `phase=stopped` 行。三项均未闭合但边界明确，不改变 native 合同、不允许热卸载。`38604` 已停止、bridge 保留、不可重接；后续现场需重启并重新准入；旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#23-2026-11-21-native-持有者最终释放全局-drain-复核离线)

2026-11-21 feat-005 在途取消+迟到结果隔离现场（通过）：新实例 `38604`（创建 `2026-11-21T12:34:11.8583479Z`）用 `4911398` 构建执行：一次授权私聊在 `NativeStarted` 后终止 core #1；core #2 同 bridge/control pipe 启动、新 self 查询成功；`ConfirmedSuccess` 在启动期间作为唯一新增状态到达（`waited_s=0`），`DispatchIntent` 仍为 1，无重派/重复终态；账本 669 字节（`5CCD2ED4…`）→ 3045 字节（`C4C7E03D…`）→ 迟到窗口结束不变；1000 ms 观察停止全阶段通过、helper/CLI exit 0、无 reason，core 清理无残留，QQ 响应、bridge 保留，对端确认一条。结论：恢复不重派、迟到结果只完成一次；不是拒绝样本，native 寿命/全局 drain 仍 unknown。`38604` 已停止、bridge 保留、不可重接；后续现场需重启 QQ 并重新准入；旧请求不重发、未证寿命不热卸载。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#22-2026-11-21-在途取消--迟到结果隔离现场通过)

2026-11-21 feat-005 在途取消+迟到结果隔离现场包（未绑定）：本机 `local-evidence/feat005-inflight-late-20261121-unbound/` 已备好（6 文件，清单 SHA-256 `A2230314…7BF9`），构建固定 `4911398`（`FB426133…`/`0269BCE9…`/caligod `F03F5893…`）。流程：一次授权私聊 → 账本 `NativeStarted` 后终止 core #1 → 同 journal/pipe/token 启动 core #2 → 30 秒迟到窗口（前缀不变、至多一个终态、无第二次派发）→ 1000 ms 观察停止。离线验证语法、PS 解析、三个构建 hash、授权/pin 门 exit 1 均通过。**未绑定、未授权、未执行**。下一步：用户重启并登录测试 QQ 后，按 README 跑 preflight、绑定单次授权 `CALIGO-005-INFLIGHT-LATE-5B8E2D64` → peer，然后执行一次；该发送可能实际到达对端，不重发、不补发；结果与审计留本机。`19900` 已停止、bridge 保留、不可重接；旧请求不重发、未证寿命不热卸载。 [现场包记录](docs/acceptance/2026-11-21-feat-005-scope.md#21-2026-11-21-在途取消--迟到结果隔离现场包未绑定)

2026-11-21 feat-005 最终构建四格矩阵+观察停止现场（通过）：新实例 `19900`（创建 `2026-11-21T11:40:21.5077439Z`）用 `4911398` 构建完成一次准入/attach；入站群 ID 6、私聊 ID 7，出站私聊 ID 8、群 ID 10，账本四态、`wire_seq` 1/2、generation 1、payload/native ID 不同，`get_msg` 与对端确认一致；同一实例 1000 ms 观察停止全阶段通过、helper/CLI exit 0、无 reason，停止前后账本快照字节一致（16234 字节、SHA-256 `57B9EDFC…`），管理器退出 0、QQ 响应、bridge 保留、零重发。§18.3 建议的在线轮次完成；剩余为 R2/R3（合同未准入）、在途取消的直接观测、native 寿命/全局 drain。`19900` 已停止、bridge 保留、不可重接；后续现场需重启 QQ 并重新准入；旧请求不重发、未证寿命不热卸载。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#20-2026-11-21-最终构建四格矩阵--观察停止现场通过)

2026-11-21 feat-005 最终构建四格矩阵+观察停止现场包（未绑定）：本机 `local-evidence/feat005-final-matrix-20261121-unbound/` 已备好（6 文件，清单 SHA-256 `FB2D649F…A816AA`），构建固定 `4911398`（`FB426133…`/`0269BCE9…`）；流程为一次准入/attach → 两条对端入站 → 两次本端出站（各自独立单次授权文件）→ SSE/get_msg/账本核对 → 1000 ms 观察停止。

2026-11-21 feat-005 完成门复核与离线检查（离线）：证据矩阵与结论见 §18；全仓 `init.ps1 -Mode all`（代码 `4911398`）default 498 / research 555、Node 69 项、docs 通过；clippy default 通过，research 的 8 个 `not_unsafe_ptr_arg_deref` 错误在 `consumer_probe.rs`/`wirecap.rs`（既有、与本次改动无关，CI 不运行 clippy）。结论：三条完成条件均有现场证据，native 寿命仍 unknown、R2/R3 未准入，不提升状态。下一轮在线建议在最终构建 `4911398` 上重跑四格矩阵 + 观察停止（需两次出站单次授权与两条对端入站），消除“矩阵来自旧构建”和“停止修复只在 idle 验证”两个边界。`30356` 已停止、bridge 保留、不可重接；新现场需重启 QQ 并重新准入；旧请求不重发、未证寿命不热卸载。 [复核记录](docs/acceptance/2026-11-21-feat-005-scope.md#18-2026-11-21-feat-005-完成门复核与离线检查离线)

2026-11-21 feat-005 报告预算修复现场核验（idle，零发送）：新实例 `30356`（创建 `2026-11-21T09:31:43.0962876Z`）使用修复构建（`4911398`，EXE `FB426133…`、bridge `0269BCE9…`）一次接入后 idle 370 秒至轮询 31 条上限；观察停止 `armed → listener_stopped → final_snapshot(+1046 ms) → close_permitted`，helper/CLI exit 0、无 reason 行；停止后报告总数 32（31 轮询 + 最终快照）、无 `report_budget_exhausted` 标记；管理器退出 0，QQ 响应、bridge 保留。旧构建 `6476` 的 `observation_report_budget` 失败已由该现场对照验证修复。零发送；原生寿命仍 unknown。

2026-11-21 feat-005 停止观察预算预留修复（research）：轮询上限 31、最终采样保留第 32 槽；`sample_stopped_observations` 不再因预算返回 `observation_report_budget`（满预算只跳过明细并返回 `Ok`），移除 `report_budget_exhausted` 标记行以免污染分析器；新增 30/31/32 边界单元测试。普通停止路径不变，真实采样失败仍 `0x25` 并保留根。下一块：新 QQ 代次上做 idle >4 分钟的观察停止复验（应通过并出现最终快照）；`6476` 已停止、bridge 保留、不可重接。原生寿命 unknown，旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#16-2026-11-21-停止观察报告预算预留修复research)

2026-11-21 feat-005 空闲重会话观察停止现场（零发送）：新实例 `6476`（创建 `2026-11-21T07:17:41.1156385Z`）一次接入后 idle 轮询，约 230s 累计 32 条 `js_callback_observation`，第 32 条 `report_budget_exhausted`；随后 1000 ms 观察停止以 `stop_observation_reason=observation_report_budget` 失败（0x25、保根），管理器仍持账号并退出 0，QQ 响应、bridge 保留，未重试。线程槽 `capture_incomplete=0`、`sequence=49`，据此更正 §6 的 cap 归属。结论：研究观察停止在 32 条报告预算耗尽后必然失败，与发送无关；§6 原会话仍 unknown。下一块需用户决定是否把诊断报告预算与停止门解耦（例如最终采样不因预算拒绝），或保持为已知工具限制；`6476` 已停止、bridge 保留、不可重接。原生寿命 unknown，旧请求不重发。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#15-2026-11-21-空闲重会话观察停止现场复现-0x25--observation_report_budget)

2026-11-21 feat-005 满仓观察停止现场（单次私聊，未复现）：新实例 `37796`（创建 `2026-11-21T07:04:40.4258757Z`）通过准备包 preflight 与精确授权后执行一次私聊，发送/账本四态/`get_msg` 回读一致，用户确认对端收到一条；1000 ms 观察停止全阶段通过、exit 0、无 reason 行，管理器正常退出，QQ 响应、bridge 保留。单次发送未填满 JS 回调库，`observation_incomplete` 未复现；复现需要更多回调活动（矩阵会话为两次发送 + 两条入站 + 后续查询，另需新授权与新代次）。零重发；原生寿命仍 unknown，未证寿命不热卸载。下一块可准备“多事件”现场包或转向 R2/R3；`37796` 已停止、bridge 保留、不可重接。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#14-2026-11-21-满仓观察停止现场单次私聊未复现满仓)

2026-11-21 feat-005 满仓观察停止现场准备包（未绑定）：本机 `local-evidence/feat005-overfull-stop-20261121-unbound/` 已备 preflight、host-op（attach + 1000 ms 观察停止）、run-field（一次授权私聊 + get_msg + 账本四态 + 原因采集）、preparation、staged_evidence 与 README；构建固定 `63bdd01`（`2AB16751…`/`FE301F2F…`），包清单 SHA-256 `C52E9A3E…30E6`。未绑定、未授权、未执行；`run-field.py` 在缺 state/grant 时 exit 1（已离线验证）。下一块：用户重启 QQ 后按 README 执行 preflight、绑定 grant、跑一次；失败时读 `stop_observation_reason`，不重试不卸载。R2/R3 仍受合同限制；原生寿命 unknown，旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#13-2026-11-21-满仓观察停止现场准备包未绑定)

2026-11-21 feat-005 满仓停止快照离线闭环（research）：Node VM 观察库填满 128 条 + `overflow=true` 后停止 listener，停止快照读取仍返回 128 条、`overflow=true` 且与停止前一致；结合 sanitizer 的 overflow→incomplete 与采样器拒绝 incomplete（§11 原因映射），0x25 的离线因果链完整。矩阵会话现场仍为 unknown，需要一次真实满仓会话把固定原因写进日志。下一块：现场满仓样本（发送需单次授权）或 R2/R3（合同 blocked）；原生寿命仍 unknown，未证寿命不热卸载，旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#11-2026-11-21-停止观察失败原因诊断research)

2026-11-21 feat-005 分析器兼容停止原因行（离线）：`analyze-callback-observation` 与 `analyze-text-stop-evidence` 新增含 `stop_observation_reason` 的对照用例，输出与无原因行时一致；7 个 Node 套件 69 项（67 通过、2 skip）0 failed，无生产改动。下一块：现场复现重会话 `0x25` 并读取原因（需可控会话活动，发送需单次授权），或转向 R2/R3（合同 blocked）；原生寿命仍 unknown，未证寿命不热卸载，旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#11-2026-11-21-停止观察失败原因诊断research)

2026-11-21 feat-005 停止原因白名单分支补全（离线）：`stop_driver` 单元测试锁定 `FAIL_REASONS` 9 项透传与未知→`other`；`qq_entry_lab` 新增 7 个 reason 集成用例，连同 incomplete/other/identity_unavailable 覆盖全部白名单。research `qq_entry_lab` 33/33、default 16/16；全仓 all default 498 / research 554，0 failed；`0x25`/隔离/保留根语义未变。复跑 `cargo test -p caligo-bridge --test qq_entry_lab --features research --locked --offline --target-dir target-harness`。下一块：现场复现重会话 `0x25` 并读取原因（需可控会话活动，发送需单次授权），或转向 R2/R3（合同仍 blocked）；原生寿命仍 unknown，未证寿命不热卸载，旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#11-2026-11-21-停止观察失败原因诊断research)

2026-11-21 feat-005 新构建现场（零发送）：源码 `63bdd01` 的研究构建（EXE `2AB16751…`、bridge `FE301F2F…`，含 §11 原因诊断）在新实例 `59204` 完成固定准入、一次接入、baseline self 查询；同代次第二次 attach 被 CLI 拒绝（exit 2）且首会话不受影响；1000 ms 观察停止全阶段通过、CLI exit 0、QQ 响应且 bridge 保留。成功报告 `stop_observation_reason` 行数为 0（负向对照）。现场材料在 ignored `local-evidence/feat005-newbuild-stop-20261121-59204/`。下一块若继续现场：需可控制造重会话活动才能复现 0x25 并读取原因（发送需单次授权）；R2/R3 换号/断网仍受合同限制；原生寿命未知。当前 `59204` 已停止但 bridge 保留、不可重接，后续现场须重启 QQ 并重新准入；旧请求不重发、未证寿命不热卸载。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#12-2026-11-21-新构建单桥与观察停止复验零发送)

2026-11-21 feat-005 停止观察失败原因诊断（research）：`stop_driver` 在采样 Err 或绑定读取失败时追加固定 `stop_observation_reason`（白名单 9 项 + other），不改变 `0x25`/隔离/保留根语义；新增回归覆盖 `observation_incomplete`、`other`、`identity_unavailable`，research `qq_entry_lab` 26/26、default 16/16。§6 的矩阵会话 `0x25` 仍为 unknown（原构建无原因行），但 128 条 `capture_incomplete` 与采样器的 `observation_incomplete` 吻合；下一次现场失败可直接读固定原因。复跑 `cargo test -p caligo-bridge --test qq_entry_lab --features research --locked --offline --target-dir target-harness`。下一块可继续现场观察（需重启 QQ）或用同类夹具覆盖原因行的其他分支；未证寿命不热卸载，旧请求不重发。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#11-2026-11-21-停止观察失败原因诊断research)

2026-11-21 feat-005 在途请求核心重启离线回归通过：`core_restart_query_lab::inflight_send_across_core_restart_is_not_redispatched` 用合成 owner 在 SendText 内阻塞（派发已到 worker、NativeStarted 未产生），终止 core #1 后同 journal/pipe/token 启动 #2，再释放 owner；断言新 core 不重派（dispatches=1）、native 发送=1、迟到 NativeStarted/结果只交付一次（results_sent=1）、QueryRequest 返回 inflight-native-1、worker CoreStopped。default 与 research 整文件 4/4。边界：合成 owner，不证明真实 QQ native 执行/取消/线程/全局 drain；与现场 §8 互补。复跑 `cargo test -p caligo-core --test core_restart_query_lab inflight_send --features research --locked --offline --target-dir target-harness`。下一块若继续：用同类夹具覆盖“迟到结果在重启窗口被拒/审计”的分支，或回到现场（需重启 QQ）。旧请求不重发、未证寿命不热卸载。 [验收记录](docs/acceptance/2026-11-21-feat-005-scope.md#10-2026-11-21-在途请求核心重启离线回归)

2026-11-21 feat-005 单桥与观察停止复验（零发送）：新实例 `33308` 一次接入、baseline self 查询成功；同一宿主代次第二次 attach 被 CLI 以已映射 bridge 拒绝（exit 2），首会话 report 不变且仍可查询；随后 1000 ms 观察停止全阶段通过（`armed → listener_stopped → final_snapshot(+1047 ms) → close_permitted → shutdown`），CLI exit 0，QQ 响应、bridge 保留，未重试。该样本与 §6 重会话的 `0x25` 形成对照：停止路径本身在最小会话可用，失败相关性未证。现场材料在 ignored `local-evidence/feat005-singlebridge-stop-20261121-33308/`。下一块若继续现场：重会话观察停止的失败条件（需可控地制造会话活动，不能靠猜）或 R2/R3 换号/断网（合同仍 blocked）；原生对象寿命与全局 drain 仍 unknown。当前 `33308` 已停止但 bridge 保留、不可重接，后续现场须重启 QQ 并重新准入；旧请求不重发、未证寿命不热卸载。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#9-2026-11-21-单桥拒绝与观察停止复验零发送)

2026-11-21 feat-005 在途请求核心重启现场通过（R1b 在途档）：新实例 `19860` 一次接入后，单次授权私聊在 `Queued → DispatchIntent`（443B）时终止隔离 `caligod` #1；以相同 pipe/token/journal 启动 #2 后 journal 字节与 SHA-256 不变、无重派，同一 bridge 的新 self 查询成功，随后普通 owner stop 原始退出码 `0`（`closed via owner pump`），QQ 响应、bridge 保留，零重发。消息投递与最终结果保持 unknown，native 是否已开始不可区分。现场材料在 ignored `local-evidence/feat005-inflight-20261121-19860/`。下一块若继续现场：R1b 的迟到结果/取消隔离（需要能在 reboot 时保持可观测的在途路径）、R2/R3 换号/断网；原生对象寿命与全局 drain 仍 unknown。当前 `19860` 已停止但 bridge 保留、不可重接，后续现场须重启 QQ 并重新准入；旧请求不重发、未证寿命不热卸载。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#8-2026-11-21-在途请求核心重启现场单次私聊)

2026-11-21 feat-005 零发送现场：重启后的新实例 `49956` 一次接入后 live self 查询成功；隔离 `caligod` 终止并以相同私有 pipe/token/journal 重启后，同一 bridge 的新查询成功且身份一致；随后普通 managed owner stop 原始退出码 `0`（`closed via owner pump`），QQ 响应、bridge 保留，未重试。本次为 R1b-lite：终止时无在途请求，取消/旧查询重放隔离、换号/断网与原生寿命仍 unknown。现场材料在 ignored `local-evidence/feat005-restart-20261121-49956/`。下一块若继续现场，需要能让查询在重启期间保持在途的受控方式（或使用可阻塞的查询/慢路径），并按 R1b 合同另取新鲜 QQ 与准入；否则转入原生寿命之外的离线项。任何新现场须新鲜指定 QQ、重新准入，旧请求不重发、未证寿命不热卸载。 [现场记录](docs/acceptance/2026-11-21-feat-005-scope.md#7-2026-11-21-隔离核心重启与干净停止现场零发送)

2026-11-21 feat-005 现场：固定 9.9.36-53644 新实例 `55544` 通过准入后一次接入，单构建四格矩阵完成（入站私聊/群 + 出站私聊/群各一条，SSE/get_msg/账本一致，用户确认两条出站各收到一条，零重发）。同轮 research 观察停止返回 `0x25 window_failed`，QQ 仍响应、bridge 保留，未重试停止或卸载；该实例已被这次 attach 消耗。runner 的群 `get_msg` `group_id` 断言为假阴性，已单独记录。现场材料在本机 ignored `local-evidence/feat005-matrix-20261121-55544/`。下一块需用户正常重启 QQ 后做零发送的干净停止与 core 重启恢复样本；原生对象寿命/全局 drain 仍 unknown。任何新现场须新鲜指定 QQ、重新准入；旧请求不重发，未证寿命不热卸载。 [现场与边界](docs/acceptance/2026-11-21-feat-005-scope.md#6-2026-11-21-单构建四格矩阵现场)

2026-11-21 feat-005 起始块完成：正式/历史路线适用性与三条完成门矩阵见 [路线适用性](docs/acceptance/2026-11-21-feat-005-scope.md)；历史 native MSF/G3/consumer 路线判定为研究资产，不作生产阻塞。新增入口级单桥 LAB（第二次 bootstrap 在任何原生调用前拒绝、关闭后仍拒绝、首会话保持 admitted）。复跑 `cargo test -p caligo-bridge --test qq_entry_lab`（default 16/16、research 24/24），全量 `init.ps1 -Mode all` 为 default 497 / research 543，均 0 failed。feat-005 由 blocked 调整为 in-progress；真实 QQ 的 core 重启/断链恢复现场与原生对象持有/最终释放/全局 drain 仍待证。下一块先按文档第 4 节准备单构建四格与 core 重启恢复清单，再做现场；任何新现场须新鲜指定 QQ、重新准入与单次发送授权，旧请求不重发、未证寿命不热卸载。 [进度](progress.md#current-state)

2026-11-21 用户决定按功能门收口 feat-004：四项完成条件均有固定 QQ 9.9.36-53644 的指定样本证据，`feature_list.json` 状态提升为 done；PR #12 原额外保留的 native 持有者、最终释放与全局 drain 记为已知缺口，由 feat-005 的原生链路证据承接，不计作已证。累积工作已归档（`1a30bd5` 代码/测试/工具，`a1a4404` 文档）并更新 PR #12 正文与清单。收口检查：docs PASS、default 496、research 542、7 个 Node 套件 65 项、`git diff --check` 通过。本轮未接入 QQ、零发送、未重接或卸载 bridge。下一块按 feat-005 可离线准备的原生链路证据推进；任何新现场仍须新鲜指定 QQ、重新准入与独立单次授权，旧请求不重发，未证寿命不热卸载。 [进度](progress.md#current-state)

2026-11-21 feat-004 阶段累积工作已归档：代码/测试/工具与文档分两个提交，补齐两个新分析器的 CI 入口（此前只在本地跑过）。提交前重跑 docs PASS、default 496 项、research 542 项、7 个 Node 套件 65 项，均 0 failed；`cargo check --workspace --all-targets` 无错误；GitNexus all-scope 为 critical 且未 partial。本轮未接入 QQ、零发送、未重接或卸载 bridge。feat-004 仍 in-progress：PR #12 保留的 native 持有者、最终释放与全局 drain 尚无现场证据，现有固定构建也没有已验证的非持有式 QQ 采集入口。下一块需先决定是继续该取证路线，还是把该项按已记录 unknown 归档并交给 feat-005 的原生链路证据；任何新现场仍须新鲜指定 QQ 与独立授权。旧请求不重发。 [进度](progress.md#current-state)

2026-11-20 固定镜像已核对异常清理与正常返回共用的解锁路径，以及库存计数的可逆性。复跑 `python -B -E target-harness/feat004-unwind-evidence/verify_evidence.py`。后续先在自建宿主离线验证，去掉专用调用方检查点后，能否用同一线程的展开上下文和外层实际继续执行识别原帧退出。独立移交凭据另行核验，缺少任一项继续保留 unknown。当前无需接入 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 已在自建宿主验证四槽轮换的同类重入漏采、异帧拒绝及异常后同槽复用。复跑 `target-harness/feat004-reentry-unwind/run-lab.cmd`。下一步先离线核对固定镜像所选路径是否有独立于可逆计数的移交凭据，以及可可靠识别异常展开后退出原帧的边界。缺证窗口保留 unknown，当前无需接入 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 已核对固定镜像正常路径的外层返回候选。`target-harness/feat004-outer-completion/run-lab.cmd` 可复跑同槽尾调用及缺证对照。下一步仍离线核对是否有独立于可逆计数的实际移交凭据，并验证同类重入和异常返回时四槽是否会漏掉外层回执。当前无需接入或重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 固定镜像调用续点和尾调用已分类。自建混合四槽能在轮换分配器槽时保留同线程、其他线程的接收者与弱零分支。复跑 `target-harness/feat004-continuations/run-lab.cmd`。后续离线研究真实共享子调用能否补到外层移交与清理、实际栈帧可核范围和独立失采凭据。缺少返回或独立凭据的窗口保持 unknown。当前无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 自建宿主的四槽按实际移交入口轮换通过，观察器对所有线程使用相同初始布局。复跑 `target-harness/feat004-roleless-rotation/run-lab.cmd`。固定 QQ 下一步须离线验证可确认哪些实际调用续点，四槽能否兼顾接收者与弱释放，以及是否具备独立移交次序。当前仅有夹具级、非同类递归的移交观测证据。无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 固定镜像已定位路径内冲刷和补充阶段点，验证共享线程跳板、按需 TLS 缓存和可逆状态计数的边界。复跑目录 `target-harness/feat004-rotation-eligibility/`。恢复后布局回退的窗口继续保留 unknown，即使起末记录完整。若继续研究，先验证覆盖旁路的阶段交接与独立漏采凭据，当前无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 自建宿主的四槽动态轮换通过已知线程角色与阶段的正例，并复现启动交错后布局回退及其他漏采。复跑 `target-harness/feat004-rotating-observer/run-lab.cmd`。后续先离线核对固定 QQ 是否有同线程的可靠阶段入口、新线程准入信号和独立冲刷缺口凭据。立即读回结果只在该调试停止内有效，跨恢复需再次核对实际布局及覆盖。当前无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 固定构建已验证候选缓存路径的固定四点覆盖缺口，受控停顿能改写实际弱计数终止分支。复跑 `target-harness/feat004-hardware-budget/run-lab.cmd`。下步若研究动态轮换，先在自建宿主验证其他线程连续覆盖、槽位重装期间的证据缺口及失败后保留 unknown。尚无 QQ 的线程分工、原生移交或等价覆盖回执，不需要重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 自建子进程硬件执行断点已取得实际接收者、同代零值分支和分配器移交。复跑 `target-harness/feat004-hardware-observer/run-lab.cmd`。后续先离线核算固定 QQ 的所需观测点、每线程断点预算、转移失采判据和捕获暂停的影响。夹具拥有独立分配器序号，真实镜像尚未证实有等价移交记录。当前无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 固定镜像内置分配器通知只覆盖分配后和实际归还前。缓存往返可漏掉中间转移而保持相同的边界字段。复跑目录 `target-harness/feat004-observer-entrances/`。下一步如评估新的采集入口，先在自建宿主验证原生接收者及实际分支、成功移交和受保护的失采回执能在不新增引用时同时取得。当前没有等价现场入口，无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 自建宿主的源码观测点能串起实际虚调用接收者、同代零值快路径和分配器移交，缓存冲刷失采、无票号缺口的失采和溢出均保留 unknown。复跑目录 `target-harness/feat004-host-observer/`。下一步仍离线评估固定 QQ 构建是否有等价取证入口，须保留分配器移交的可信因果和失采判据。夹具采集依赖自有源码且未部署，当前无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 固定构建弱计数快路径和竞争终止路径已用静态与自建夹具复核。复跑目录 `target-harness/feat004-weak-release/`。下一步在自建宿主先验证观测手段能同时取得宿主已持有的接收者、实际分支和分配器移交，覆盖内部缓存冲刷与失采。取得无附加强弱持有且可核因果的证据后再评估现场。当前无需重启 QQ，旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 同址复用和多线程释放的非持有式因果重建通过。复跑目录为 `target-harness/feat004-concurrent-release/`。下一步离线核对固定构建上是否存在合规的观测方式，能同时提供实际接收者、可信的分配器移交与引用操作来源。夹具已验证乱序报告，真实 QQ 的原生采集和弱计数快路径仍没有运行证据，暂不需要重启 QQ。旧请求不重发。 [证据与边界](progress.md#current-state)

2026-11-20 同一分配代次的非持有式采集已在自建 MSVC 夹具验证。`target-harness/feat004-nonowning-release/run-lab.cmd` 与同目录 `check_capture.py` 可复跑。后续先离线核对实际可用观测方式是否能给出分配代次、引用槽复制和匹配的终止调用回执。夹具线程交接已排序，共享日志序号不能代替真实并发中的操作因果。固定镜像尚无可用运行采集方案，不需要重启 QQ。旧请求不重发，旧进程不重接或卸载。 [当前证据](progress.md#current-state)

2026-11-20 固定镜像已确认监听器与同块控制对象的构造、两个引用计数及终止调用目标。复跑 `python -B -E target-harness/feat004-typed-listener-release/check_typed_release.py`。下一步先离线验证可在不改变持有关系的情况下，同代记录实际 service/adapter vptr、登记与快照复制、析构返回和命中的 allocator 效果。N-API 删除可经队列延后，须取独立回执。现有现场样本无法补出这些事件，待采集方法通过受控验证后再评估零发送 QQ 观察。旧请求不重发，旧进程不重接或卸载。 [证据与边界](progress.md#current-state)

2026-11-20 登记项、快照引用及最后释放的离线判据与对照已完成。固定镜像验证见 `target-harness/feat004-listener-identity/check_fixed_pair.py`，C++ 夹具由该目录的 `run-lab.cmd` 重跑，构造证据由 `check_witness.py` 重跑。下一块先离线核对正式 adapter 的实际接收者类型和控制块计数约定，寻找能提供同对象终止释放与回收回执的采集入口。现有 JS 现场材料不含原生对象关联，暂不需要重启指定 QQ 或授权发送。旧请求不重发，旧进程不重接或卸载。 [证据与边界](progress.md#current-state)

2026-11-20 已复验自有关闭根与固定原生候选释放分支。离线重放入口 `python -B -E target-harness/feat004-native-ownership-replay.py`。既有现场的迟到 Promise 与同 pending、同保留 `R` 和 JS owner OS TID 已关联。下一步先离线确认能否辨识同一登记项、真实快照引用、控制块和析构目标。只有收集方法能区分候选释放、实际最后一次释放与 Environment 失效，才提出新 QQ 现场观察；当前没有需要重启或发送的现场步骤。旧请求不重发，旧进程不重接或卸载。 [持有关系和证据索引](progress.md#current-state)

2026-11-20 当前固定研究构建在 QQ `35000` 完成一次新私聊及一次同 ID 的 `get_msg`。CRC 账本 `ConfirmedSuccess`、持久原生身份、本端回读与两份停止快照均核实，B 侧确认仅一条且内容相符。对端回执与技术索引已绑定，现场材料在 `local-evidence/feat004-final-normal-20261120-35000/`。正常成功路径和此前同构建的迟到隔离样本均已有证据。下一工作块按 PR #12 的原生寿命条件收集实际对象持有与最终释放证据。该样本已提交一次，旧请求不重发，旧进程不得重接或卸载。 [现场与证据](progress.md#current-state)

2026-11-20 清单计时解析修复已通过原始日志回放。未绑定模板位于 `local-evidence/feat004-parallel-timing-fix-20261120/`，原始回放命令为 `python -B -E target-harness/feat004-manifest-timing-replay.py`。 [回放和完成门](progress.md#current-state)

2026-11-20 并行清单校验在指定 QQ `47760` 完成一次授权现场。清单校验 124.472 ms，CLI 至 listener 停止 151 ms。依靠同请求账本及状态对照，1 条自然迟到 `promise_fulfilled` 的隔离和 ACK 收尾均已证实，送达保持 unknown。现场归档 `local-evidence/feat004-natural-late-parallel-20261120-47760/`。 [现场证据和边界](progress.md#current-state)

2026-11-20 预启动停止助手在指定 QQ `66188` 上完成单次授权验证。提交报告至 listener 停止 333 ms，其中 CLI 至停止 259 ms。同请求 ACK 收尾通过，1046 ms 窗口内未观察到关联迟到，送达与隔离保持 unknown。管理器在取证后退出，QQ 同代仍响应并保留 bridge。现场归档在 `local-evidence/feat004-natural-late-warm-20261120-66188/`。本样本授权已用尽，旧请求不重发，旧进程不重接或卸载。后续若继续研究，以本轮分段计时为基线，先离线验证可行的缩短方案。 [现场证据](progress.md#current-state)

2026-11-20 单次在途现场已用尽本样本的授权。QQ `58112` 一次提交，分阶段停止的同请求 ACK 收尾已核实。四条关联回调均在停止前，真实迟到隔离仍为 unknown，发送终态为 `DeliveryUnknown`。owner、管理器正常退出，QQ 同代仍响应并保留 bridge。归档 `local-evidence/feat004-composed-single-20261120-58112/`，原准备包在同名前缀的不带 PID 目录。旧样本不重发，当前进程不重接或卸载。后续先离线分析如何在保持业务路径的前提下提高自然迟到的取证机会，任何新现场需新鲜指定 QQ 和另行单次授权。 [现场证据](progress.md#current-state)

2026-11-20 分阶段停止零发送现场通过。指定 QQ `59384` 登录核验后对同连接停发，同一 owner 观察 1043 ms 并正常停止。管理器仍在线，采集第二份空账本后正常退出。QQ 同代仍响应并保留 bridge。后续要核验实际在途 ACK 或真实迟到隔离时，需使用新鲜指定测试 QQ，并先获得新样本的单次授权。该进程不重接或卸载，旧请求不重发。 [现场证据](progress.md#current-state)

2026-11-20 现场前的离线串联通过。真实 Windows 管理器及管道在假宿主完成观察和 worker 收到 ACK 后才退出，单请求落账通过。零发送准备包及现场结果保存在本机忽略目录 `local-evidence/feat004-composed-zero-20261120`。 [进度与证据](progress.md#current-state)

2026-11-20 研究观察的 bridge 收尾已接线。先停监听和采样、关闭 owner pump，再交接 Rust 已拥有的结果并等 core ACK 和 worker 退出。无 ACK 或交接端关闭保留结果。真实命名管道验证在途 `DeliveryUnknown` 落账和 ACK。下一块离线串联绑定账号的 core 停发、维持管道的研究观察、最终 manager 退出以及新顺序的账本审计。既有样本不重发，真实迟到隔离仍待证，feat-004 保持 in-progress。 [进度与验证](progress.md#current-state)

2026-11-19 研究版 core 停发第一步已接通，管理入口绑定当前账号和 `connection_id`。真实 HTTP 和命名管道 LAB 验证拒发、同 actor 重连仍停发、旧连接拒绝、旧请求持久化与 ACK。下一步离线实现并验证 owner 最终交接和 worker 等 ACK 的阶段，包含超时或断连保留；核心停发完成后才能进入该阶段。旧请求不重发，`44116` 不重接或卸载，feat-004 保持 in-progress。 [参考](progress.md#current-state)

2026-11-19 已离线复现 manager 先关管道时的 `0x22` 收尾机制。真实管理器和 worker 对照中，先退出可复现 `DeliveryUnknown` 以及已退出 worker 仍伴随 bridge 未确认状态；先等持久结果 ACK 则正常释放。现场只记录合并门超时，具体挂起项仍待诊断。下一块先验证停止期间保留结果与 ACK 通道的顺序，再安排实际接入。旧请求不重发，`44116` 不重接或卸载，feat-004 保持 in-progress。 [诊断与证据](progress.md#current-state)

2026-11-19 单次在途停止现场已有证据。指定 QQ `44116` 一次接入并核验登录，唯一文本请求提交后启动退出，OneBot `1501`、账本 `DeliveryUnknown`。1 秒观察和最终采样通过，四条关联回调都在停止前。停止返回 `0x22 ShutdownPendingResults`，管理进程和端口已关闭，QQ 同代仍响应并保留 bridge。停止前与失败返回后两个账本副本一致。旧请求不重发，旧进程不重接或卸载。下一块先离线复核提前退出时的结果收尾，真实迟到隔离仍待证，feat-004 保持 in-progress。 [进度与证据](progress.md#current-state)

2026-11-19 迟到隔离只读核对工具已完成，按同一 `wire_seq` 对应唯一 core 请求、提交轨迹和回调，并检查停止前后两份独立账本。33 项测试及两份现场归档回放通过，两份现场均保留迟到 unknown。后续现场需新鲜指定 QQ 和单次发送授权，在 manager 正常退出与 owner 开始停止之间固定第一份账本，owner 正常关闭后固定第二份。只在真实关联回调到达时计入其状态对照。feat-004 保持 in-progress。 [进度与证据](progress.md#current-state)

2026-11-19 文本回调状态对照研究版已在指定 QQ `63120` 完成零发送、登录核验及 1000 ms 停止观察。最终采样先于正常关闭，墙钟差 1010 ms，空账本及端口关闭经独立审计。QQ 同代仍响应并保留 bridge。两份快照只含停止边界，真实迟到仍为 unknown。后续只有同请求的真实迟到及独立账本证据齐备，才推进隔离验收。后续若需发送，应另获单次授权并使用新鲜指定 QQ。feat-004 保持 in-progress。[进度与证据](progress.md#current-state)

2026-11-19 停止后文本回调状态对照已完成离线准备。新构建和未绑定进程的零发送准入包见 [progress](progress.md#current-state)。下一步使用新鲜指定 QQ，按固定模块准入、登录、1000 ms 停止观察和独立空账本审计验证正常路径。随后只有实际关联的迟到回调与独立请求账本对照都取得，才可推进隔离验收。旧 QQ 代次保留 bridge，既有样本不重发。feat-004 保持 in-progress，本轮未接入 QQ。

2026-11-19 resident 结果隔离修复版零发送现场通过。指定 QQ `67792` 通过固定准入、接入和登录核验。1000 ms 窗口的同 owner 最终采样先于关闭，报告墙钟差 1009 ms，空请求账本与关闭端口经独立审计。观察到一次停止前未关联接收回调，真实迟到仍为 unknown。QQ 同代仍响应并保留 bridge，下一次现场须新鲜指定进程。证据见 [progress](progress.md#current-state)。下一块继续真实迟到隔离或 native 寿命证据，feat-004 保持 in-progress。

2026-11-19 resident 停止后回执隔离修复。正式与研究入口只在 Ready 接受结果，适配器读取跨 close 时拒绝入账，关闭前强回执仍可纠正已 ACK unknown。三个红绿对照、两种全仓测试及 release 通过，见 [progress](progress.md#current-state)。修复版 1000 ms 零发送现场已准备，旧 `38316` 保留 bridge，等待新鲜测试 QQ 后核验正常接入与停止。真实迟到和全局 drain 仍待证。feat-004 保持 in-progress。

2026-11-19 好友 B 已确认单次组合样本的数量和正文。只读分析器现能在无请求回调的样本中保留合法停止边界，回调仍为 unknown，矛盾标记拒绝。两份现场日志回放及 20 项回归见 [progress](progress.md#current-state)。后续推进真实迟到与状态隔离的独立证据，原 QQ 代次仍保留 bridge，继续遵守新鲜进程准入与 unknown 不重发。feat-004 保持 in-progress。

2026-11-19 单次私聊与停止观察组合现场。新 QQ `38316` 准入、授权单次发送、稳定消息账本回读和 1000 ms 停止观察技术审计通过，好友 B 确认仅收到一条且内容相符。两个投影行是同一消息的创建和时间修订。四条已关联 JS 回调均在停止前，真实迟到与全局 drain 仍待证。停止后 QQ 仍响应且保留 bridge，后续须新鲜代次。授权机会已消耗，证据见 [progress](progress.md#current-state)。feat-004 保持 in-progress。

2026-11-19 单次私聊与停止观察组合已准备。固定样本 `CALIGO-004-STOP-9AB42E71` 到登记好友 B，账本回读后一次 1000 ms 观察。授权/重复保留负例、回放核对和固定构建见 [progress](progress.md#current-state)。当前等待新鲜指定 QQ 和单次授权，再核验固定准入并绑定 PID/创建时间；送达未知不重发，未观察到真实迟到仍为 unknown。旧 `7776` 保留 bridge。feat-004 保持 in-progress。

2026-11-19 研究版零发送停止观察现场通过。新鲜指定 QQ `7776` 准入、登录、一次 1000 ms 观察和正常停止均通过。owner 停止 listener 至最终采样的日志墙钟差 1017 ms，最终采样先于关闭，账本为空且端口关闭。未观察到请求回调，真实迟到、native 寿命和全局 drain 仍待证。QQ 同代仍响应且保留 bridge，下一次现场须新的指定代次并重新准入。详细证据见 [progress](progress.md#current-state)。共享源码仍未提交，feat-004 保持 in-progress。

2026-11-19 停止观察等待超时修复。已有研究版 CLI/FFI 激活入口，早期交接所述的入口缺失已过时。假 QQNT owner 阻塞采样时，等待线程仍按截止返回 `0x25`，保留根且不触发 `uv_close`。普通与 research 全仓、分析器及两种发布构建通过，源码叠加与产物哈希见 [progress](progress.md#current-state)。当前仍为共享未提交工作树。下一步在新鲜指定 QQ 代次按固定模块准入验证零发送停止观察。旧样本保留，unknown 不重发，未证寿命不卸载。真实迟到、native 对象寿命与 drain 继续按现场证据判定。

2026-11-19 最新研究owner接线：stop_driver复用既有控制器，research Rust显式arm后，实际pump首tick一次清理、窗口只读采样、最终样本后一次关闭，shutdown调用线程50ms只发特殊wake。账号/nonce/sid/generation、owner/env/loop/V8 ladder检查，失败0x25/quarantine保根，原0x22/23/24保持；停止前取消正常close，观察中取消最终采样，慢采样计入deadline。research入口21+模型9+实际JS1定向通过，最终default/research all及分析器17通过，证据清单 `target-harness/feat004-window-wire-evidence.json`，hash见 [progress](progress.md#current-state)。尚无CLI/新C激活入口，无新release/QQ现场动作；下一步应准备research现场激活与准入合同/负例，再有界采真实晚到，不再新增模型。旧16424不重接/卸载，消息不重发；native寿命/drain仍待证，feat-004保持in-progress，暂不需用户重启，未提交推送。


2026-11-19 最新停止窗口块：research纯控制器绑定account/nonce/sid/generation，窗口1..5000ms、50ms间隔、104动作预算，精确动作/回执控制停止→观察→最终样本→关闭，身份/时间/未知失败永久隔离；取消不重试。控制器9项+实际JS重复读取/寿命/发送定向研究28通过，普通发送17通过，最终全仓见 [progress](progress.md#current-state)。控制器与实际reader尚未接线，未改qq_entry shutdown/CLI/FFI，不能启动QQ观察；无新release/现场动作。下一块应直接做研究opt-in owner驱动接线：停止listener后只读采样、合法owner tick/定时唤醒、scope/上下文、取消/最终快照、0x22/23/24与一次uv_close；不要再新增平行模型。旧16424不重接/卸载、消息不重发，真实迟到/native寿命/drain仍待证，feat-004保持in-progress；暂不需用户重启，未提交推送。


2026-11-19 最新分析块：只读callback分析器新增严格phase状态机和late_callbacks直接请求关联汇总，矛盾/重复/无锚点停止序列unknown，state_isolation仍unverified。Node17项及历史TID/PHASE只读回放通过，两现场均未观察迟到，证据见 [progress](progress.md#current-state)。本轮只工具/测试/文档，无生产改动，不重建或重复全仓。下一步先设计新鲜QQ代次下停止listener后、关闭owner pump前的有界只观察窗口（scope/account校验、取消/截止/最后关闭、失败保根），不自动启动；现有STOP仅一次快照不能证明更晚无回调。旧16424不重接/卸载、既有消息不重发，真实迟到/native寿命/drain仍待证，feat-004保持in-progress；无需现在重启QQ，未提交推送。


2026-11-19 同步 TID 样本现场通过。最新 QQ 主实例 16424 完成固定准入，一次私聊发送、即时 `get_msg`、唯一请求账本和用户单条确认均通过。generation 1、seq 1 的四条关联事件直接记录 TID 60992，与同代次 owner 一致。停止顺序为 order 7 → 8，manager 和 owner 均退出 0，端口关闭。事后 QQ 响应正常，线程 60992 和 bridge 仍在。授权机会已消耗，保留现有 bridge 至进程退出。剩余门为真实停止后迟到回调、原生对象寿命、最终 owner 和全局 drain。feat-004 保持 in-progress。证据与指纹见 [progress](progress.md#current-state)。


2026-11-18 最新同步TID块：精确QQNT两个具名回调+整数工厂18项静态断言后，research第六无参getter同步返回TID到同请求/阶段记录；活动/安装/current isolate一致门和独立getter安装成功标志，失败null不影响业务，避免ordinal拼接。原生LAB3、分析器15、最终default/research all和两套release通过，清单 `target-harness/feat004-tid-return-build-evidence.json`，hash见 [progress](progress.md#current-state)。新getter未加载QQ，本轮零接入/发送。下一样本登记好友B单次 `CALIGO-FEAT004-TID-20261118-3C8E50A7`，本机 `local-evidence/feat004-tid-return-send-20261118/` 已准备无grant；须用户对应授权、正常重启登录后最新QQ新根准入再执行一次，核对关联event.callback_tid与owner/live线程，不重发旧样本。真实迟到/生产者线程/原生pointee/final owner/drain仍未知，feat-004保持in-progress；旧18640不复用/卸载，未提交推送。


2026-11-18 最新wake寿命块：复现正常shutdown后ready仍1导致两wake继续native send；已统一独立WAKE_GATE准入，关闭/隔离拒绝新wake，等待已准入send返回再一次停止唤醒/uv_close。default/research入口各15通过，含强制send/close交错、hook内两wake和重复pump一次关闭；最终all/release及hash见 [progress](progress.md#current-state) 和 `target-harness/feat004-wake-build-evidence.json`。强制fault交错未新增，沿用顺序故障拒绝；wait_ms不限制native挂起/门锁等待。新构建未加载QQ，本轮无接入/发送；辅助ABI查找认证失败无结论，不能猜结构。下一步回到精确ABI关联线程/真实迟到和native对象寿命最小取证，不反复正常样本代替drain。旧18640不重新接入/卸载，feat-004保持in-progress，未提交推送。


2026-11-18 最新现场：QQ最新新根18640固定准入、已授权PHASE单次私聊/即时get_msg/唯一请求账本及对端一条确认通过。新观察5快照前缀一致无incomplete，generation1/seq1的matched与closure四事件齐，order9 stopping→10 stopped；无停止后事件，late仍unobserved。汇总TID55432只保留aggregate，不拼接逐请求。manager/owner0、端口关闭，QQ仍响应且bridge保留，不重接/卸载。证据/指纹见 [progress](progress.md#current-state)，本机 `local-evidence/feat004-request-phase-send-20261118/`，授权已消耗不重发。无生产变更，本轮正常路径指定样本通过不关闭真实迟到/逐请求OS线程/native对象寿命/final owner/drain；feat-004仍in-progress。下一块先离线定义剩余最小证据，不再重复正常样本；无需现在重启QQ，未提交推送。


2026-11-18 最新关联观察块：research实际JS接入listener代次/终身order/text临时seq/kind/停止phase/current与none/closure/matched来源，队列128、24KiB白名单快照、sid+generation绑定、报告去重/32预算显式耗尽。实际owner poll与STOP即时读取；不与旧无参TID按顺序拼接，不声称逐请求OS线程。定向default137/research161通过，最终all/release与产物指纹见 [progress](progress.md#current-state) 和 `target-harness/feat004-request-phase-build-evidence.json`。新构建未加载QQ，本轮零接入零发送；生产stop完成后新到事件无读取保证仍unknown。下一现场应直接核对正常请求关联+停止标记与独立账本，不再用零发送代替；具体样本/准入脚本先准备，实际发送机会须新明确范围授权，不重发旧marker。native环境/快照寿命、逐请求OS TID与真实迟到/drain仍未闭合，feat-004保持in-progress；旧19020不复用/卸载，未提交推送。


2026-11-18 最新对象寿命LAB：确认正常STOP后实际 `__caligo_d8 → R → listener/pending` 根保留，正式同代次重接禁止。新增实际脚本+生成send的Node WeakRef/GC回归覆盖发送Promise/精确回读在途及移除失败；合成迟到调用不改业务状态、不增宿主查询。default/research各18定向通过，证据/GC控制失败限制见 [progress](progress.md#current-state) 与观察合同。本轮仅测试/文档，无生产改动，沿用前轮all/release，不接QQ不发消息。下一块收敛可独立关联请求/代次/停止顺序的JS观察合同，真实native快照/opaque pointee/环境寿命仍未知；不再把JS根等同QQ drain。无需现在重启QQ，旧19020不复用/热卸载，feat-004仍in-progress；未提交推送。


2026-11-18 最新离线块：实际正式stop清理helper的listener移除Err曾仍返回0，已红绿复现并修复；独立OWNER_CLEANUP_FAILED/quarantine保留根，shutdown0x24/CLI明确失败，后续不重复清理；正常对照释放。default/research定向各44（入口14、生命周期24、保留6）通过，最终all/release与指纹见 [progress](progress.md#current-state)，新产物未加载QQ，本轮零接入/零发送。owner panic0x23与pending0x22证据保留。下一步离线核对正式wrapper callback对象持有及请求/代次/停止关联，不把本地隔离当QQ drain；普通并发wake竞态仍未闭合。旧19020不重新接入或卸载，既有样本不重发；无需现在重启QQ。feat-004保持in-progress，未提交推送。


2026-11-18 最新离线块：owner hook Rust panic/锁中毒在实际FFI pump的隔离子进程复现旧版abort后修复；sticky quarantine保留hook捕获根，shutdown0x23、CLI明确失败，不重复执行/清空/替换。default/research入口各12、最终all及两套隔离release通过，源码/产物/日志清单为 `target-harness/feat004-owner-panic-build-evidence.json`，指纹见 [progress](progress.md#current-state)。新构建未加载QQ，无新接入或发送；旧19020不复用或热卸载。下一块先离线复现正式停止hook中 `r.close()` Err只日志但继续关闭的错误传播/根释放边界；普通并发wake原子检查竞态仍未闭合，本次仅Rust unwind containment。ABI/请求关联线程/实际迟到/opaque对象寿命与QQ final owner/drain仍待证，见观察合同完成门清单。无需现在重启QQ，feat-004保持in-progress；未提交或推送。


2026-11-18 最新现场：停止/退休回读修复版在最后启动的新QQ19020固定准入、research一次接入、观察器安装、登录及零发送正常停止通过；独立审计核对空请求账本、manager/owner0、实际shutdown0x0与端口关闭。原始证据及hash见 [progress](progress.md#current-state)。本轮无待ACK/native故障注入、无真实回调，不证明0x22现场或QQ drain；19020仍响应并保留bridge，不重接/卸载。下一步先离线收敛剩余owner/停止与异常清理合同；既有单次文本不重发，任何新现场须新代次准入。feat-004保持in-progress，未提交推送，下方为历史检查点。


2026-11-18 最新离线块：修复精确回读在退休/替换后写旧结果，实际JS回归default/research各17通过；正式停止独立持有resident至worker退出及结果确认，未收尾返回0x22并保留，CLI明确失败；误报红灯、ACKed unknown grace负例及入口10+结果6定向通过。完整验证/构建与限制见 [progress](progress.md#current-state)。修复后零发送准备为本机 `local-evidence/feat004-stop-lifetime-zero-20261118/`，使用新隔离release，尚未执行；需用户正常重启并登录测试QQ，继续选择最后启动的新主根重新准入，再仅接入/登录/正常停止，不新发消息。旧62780保留先前bridge不重接/卸载；既有单次文本不重发。QQ native最终owner/真实迟到/drain、任意panic/锁中毒清理与跨宿主持久化仍未闭合，feat-004保持in-progress；未提交或推送。


2026-11-18 最新 feat-004：重启后最后启动 QQ62780 完成固定准入、已授权单次好友 B 文本、即时 get_msg、唯一请求/稳定身份账本和八阶段轨迹独立审计，用户确认对端仅一条且内容完全一致。新观察器记录六条 JS 入口，OS TID 均2440，与 owner 相同；线程记录 request association 仍 unverified，native 生产者/迟到/final owner/drain 未证。manager/owner 停止0、端口关闭、QQ仍响应并保留 bridge，不重接或卸载、不重发样本。证据和指纹见 [progress](progress.md#current-state)，本机 `local-evidence/feat004-js-thread-send-20261118/`；发送机会已消耗，后续先离线制定停止/代次边界最小观察与独立关联合同，不能仅凭本轮线程样本标 feat-004 done。无 Rust 新变更、未提交推送；下方为此前检查点。


2026-11-18 用户已授权现场并指定最后启动的 QQ。PID63032 的固定准入、观察器安装、零发送登录/停止和独立审计通过，真实回调记录为零；证据见 [progress](progress.md#current-state)。该代次仍响应并保留 bridge，不重接或卸载。下一步等用户正常重启并登录该测试账号，再选最后启动的新 QQ 主根、重核 hash/backend/envscan，运行本机 `local-evidence/feat004-js-thread-send-20261118/` 单次好友 B 文本 `CALIGO-FEAT004-THREAD-20261118-6F41B2C9`；grant 已记录用户授权，无需再次询问。发送机会未保留/未执行，unknown 不重发。需独立核对账本/回读/轨迹与实际 JS 线程样本，对端数量另确认；native 生产者/final owner/drain 仍 unknown，feat-004 in-progress。


2026-11-18 feat-004 续验：`E:/stella/Caligo-feat-004` 基于 `9fe721f`，修复同 worker 跨 core 重启/相同 epoch 的未 ACK 结果停滞，两个真实 caligod 进程 LAB 红绿及 default/research 全仓通过。新增精确 hash 绑定、128 次预算的 research JS 入口线程观察器，静态 ABI 21 项和线程/实际 JS 合同通过，尚未加载 QQ；构建与日志指纹见 [progress](progress.md#current-state)。下一步用户正常重启并指定已登录新测试 QQ 根，先使用本机 `local-evidence/feat004-js-thread-zero-20261118/` 零发送验证观察器安装/登录/停止；后续新代次向登记好友 B 单次发送 `CALIGO-FEAT004-THREAD-20261118-6F41B2C9` 需对应授权，准备在 `feat004-js-thread-send-20261118/`，尚无 grant。记录只证明 JS 执行线程，关联/顺序及 native 生产者/最终 owner/drain 未证；feat-004 保持 in-progress。未接 QQ、未发送、未提交或推送；unknown 不重发、未证寿命不热卸载。其他分支未提交改动保留。


2026-11-18 feat-004 管理器停止错误保留版零发送现场：新 QQ `30968` 只读准入、一次接入、登录及正常停止、独立零发送审计通过，固定构建与证据见 [progress](progress.md#current-state)。同代 QQ 仍映射 bridge，不重接、不热卸载；现场未注入 journal 故障，管理器锁存只限同进程。继续跨进程恢复及真实 callback/drain；后续现场须新进程准入，新消息需对应授权，unknown 不重发。


2026-11-18 feat-004 管理器停止错误保留：同进程内 journal 收尾失败、清理失败或线程 panic 不再因 join handle 被移除而变成 stop/shutdown 成功，并拒绝同账号再次 start；已清理的启动冲突仍可重试，其他账号可停止，manager 最终对 shutdown 失败非零退出。受控真实 daemon、启动回滚、失败对照和普通/research 全仓证据见 [progress](progress.md#current-state)。锁存仅限当前管理器进程，非跨重启持久化；未在 QQ 现场制造故障。下一步固定构建并准备新进程零发送正常停止验证；unknown 不重发，未证寿命不卸载。


2026-11-17 feat-004 停止落账修复版零发送现场：新测试 QQ `64928` 只读准入、单次接入、登录核验、正常停止和独立零发送审计通过，证据及固定构建见 [progress](progress.md#current-state)。此 QQ 仍映射 bridge，不重接、不卸载；现场未注入 journal 故障，容量失败与恢复仅有离线证据。继续未闭合的停止恢复及真实 callback/drain；后续现场须新进程并重新准入，新的发送需对应授权，unknown 不重发。


2026-11-17 feat-004 停止落账失败修复：journal 容量耗尽时 stop/close 原先吞错并可能虚报 Closed，现传播错误、维持 Degraded/未决状态，daemon 最终收尾报错；恢复读取、真实本机 daemon 和普通/research 测试见 [progress](progress.md#current-state)。控制面 Stopped 早于最终收尾，不能单凭它证明持久化完成。尚未 QQ 现场验证；下一步固定修复版构建并准备零发送新进程接入、登录和正常停止，不重发 unknown，未证寿命不卸载。


2026-11-17 feat-004 丢失 ACK 后重建组合 LAB：真实命名管道和假宿主中，daemon 持久化结果后断链、首 worker 在重连前正常停止，同 resident 由夹具重建后重放并 ACK，结果及一次发送保持；定向、普通全组和 research 定向验证见 [progress](progress.md#current-state)。不证明 worker 崩溃、自动重启、宿主持久化或 QQ 现场。继续未闭合工作；需现场时重核新进程准入，unknown 不重发，未证寿命不卸载。


2026-11-17 feat-004 满额与 worker 重建组合 LAB：真实命名管道、同 resident、假宿主和人工重建的受控场景下，满额保留项及待交接结果恢复，重复提示未增加结果发送，宿主发送一次；测试与证据见 [progress](progress.md#current-state)。本轮未改生产代码、不接 QQ；自动 worker 重启、崩溃中途、跨宿主恢复和真实现场仍未证。继续选择未闭合的 feat-004 工作块；需现场时重新准入，unknown 不重发，未证寿命不卸载。


2026-11-17 feat-004 修复版单次私聊现场：用户授权后 QQ `17520` 准入、单次发送、持久账本 `get_msg`、独立技术与轨迹复核通过；用户确认仅一条且内容相符。原 runner 轨迹解析 false 保留，混合格式的独立复核及证据见 [progress](progress.md#current-state)。manager/owner 停止、端口关闭，QQ 响应并保留 bridge；不重接、不卸载、不重发。真实 ACK 丢失、worker 重建、背压、panic、迟到及 callback/drain 仍未证；feat-004 in-progress。后续先选取一个未闭合工作块并实施离线验证；需新现场时重新核验新鲜进程，发送须对应授权。


2026-11-17 feat-004 单次私聊正常路径已准备但未授权：样本 `CALIGO-FEAT004-RETAIN-361D4A93BC`，固定源码/脚本、缺授权与重复保留负例、合成审计见 [progress](progress.md#current-state)。当前 QQ `54316` 当次仍映射 bridge；先取得用户对该正文和好友 B 的单次授权，并让用户重启登录，核验新进程后才可执行一次；未知不重发、未证寿命不卸载。

2026-11-17 feat-004 修复版零发送现场：新 QQ `54316` 通过固定模块/后台/唯一 Environment 准入，单次接入、登录、正常停止及独立零发送审计通过；同代 QQ 当次响应且 bridge 保留。首轮加载前 PowerShell 模块路径失败及修复对照均保留，详见 [progress](progress.md#current-state)。真实并发、panic、背压、ACK、callback/drain 与跨宿主恢复未证；下次现场须新鲜 QQ 准入，任何新消息需对应授权，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 零发送现场已准备：源码 `3bbd3ce` 普通/research release 与脚本已固定，证据和限制见 [progress](progress.md#current-state)。当前旧 QQ 根 `38060` 当次仍映射 bridge；先请用户正常重启并登录，再重新核验新进程、固定 hash、backend、唯一 Environment，之后仅做零发送登录与正常停止；不重发、不卸载。

2026-11-17 feat-004 单 worker lease：同 resident 并发 worker 拒绝，正常退出在会话/接收端关闭后释放，unwind panic 则隔离该代次；自建并发和真实管道回归见 [progress](progress.md#current-state)。不证明不同 resident、abort、原生回调/宿主退出；无自动重启或通道替换。当前测试 QQ 根 `38060` 当次仍映射 bridge，现场须重启登录并重新准入；unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 满额背压恢复：正式 owner/research pump 在派发前重试后缀；满额暂停并在 ACK 释放空间后恢复 FIFO，通道拒收/序号耗尽仍硬停。假宿主与受控真实管道对照见 [progress](progress.md#current-state)。长时间暂停下的队列、自动重启/并发排他、跨宿主恢复、stop/drain 与 QQ 现场仍未证；unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 resident 通知保留：正式 owner/research pump 在交接前保留有界副本，序号跨同一 resident 的 worker 重建保持；假宿主和真实命名管道重建/去重回归见 [progress](progress.md#current-state)。满额/拒收会锁存停派且尾项需后续恢复，无自动 worker 重启或并发排他，宿主退出、stop/drain 与 QQ 现场仍未证；unknown 不重发，未证寿命不卸载。上轮到期接口调用遗漏已在 progress 勘误：research native pump 确有调用。

2026-11-17 feat-004 worker 结果序号 ACK：业务协议 revision 4 在结果与 ACK 加必填序号，worker 在自身生存期保留递增身份并按序号匹配，旧 unknown ACK 不释放后续强身份槽；合成和命名管道证据见 [progress](progress.md#current-state)。跨 worker/宿主身份、正文保留、停止恢复与 QQ 现场尚未闭合；两端须匹配 revision，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 unknown 窗口容量：resident 到期接口现在仅在 unknown 已确认时释放槽；未确认时到期保留，确认后释放，假宿主正反测试见 [progress](progress.md#current-state)。源码未发现该到期接口的正式调用；worker 退出丢失、同类终态版本、持久恢复和现场均未闭合。后续继续有界副本与版本化 ACK；unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 worker ACK 防护：结果 ACK 现在匹配请求、终态类型和当前连接已发送项后才移除及释放槽位；定向红绿对照见 [progress](progress.md#current-state)。同类终态版本、worker 退出丢失、持久恢复和现场仍未闭合；继续版本化 ACK 与 resident 保留设计。现场须新鲜进程准入，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 worker 退出缺口：假宿主离线复现通道成功后接收前丢失，以及 worker outbox 持有期间丢失；未修改生产协议。下一步设计 resident 有界持有与版本化 durable ACK，覆盖中间态/迟到结果、重放与退出；详见 [progress](progress.md#current-state)。现场仍须新鲜进程准入，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 owner 通知交接：通道发送明确失败时恢复未交出项，锁存断开并停止后续 dispatch/查询 drain/JS 快照确认；假宿主回归和真实命名管道 ACK 丢失对照见 [progress](progress.md#current-state)。成功进入通道但 worker 退出前未持久化、首次发现断开之前的派发及关闭恢复仍未覆盖；继续处理 durable ACK 与退出边界。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 结果表压力抽样：未匹配观察按既有频率产生 `poll_result_pressure` 本机报告，仅含有界数量和满额/退避状态，异常为 unknown；合成只读、隐私及边界验证见 [progress](progress.md#current-state)。该样本不是持续健康度或未匹配精确计数，报告须留本机；仍需可审计处置、持久化、额外 V8 调用的现场兼容和退出恢复验证。未知不重发，未证寿命不卸载。

2026-11-17 feat-004 未匹配结果退避：同一未匹配对象按单调时钟 250 ms 到 2 秒递增重试，新键/新对象和接收事件不受阻；时钟不可用立即重试，保留结果，合成证据见 [progress](progress.md#current-state)。退避不清理未知或消除 128 条容量阻塞；仍需可审计处置、daemon 持久化及真实 QQ/退出恢复验证。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 快照逐项确认：resident 返回本次确实处理或等价空身份已处理的键，owner 仅确认这些键；未匹配、过期和终结态重复结果保留，伪造/换键重放拒绝，合成证据见 [progress](progress.md#current-state)。未匹配结果可能反复轮询并最终占满 128 条而阻止新发送；需研究有界退避及可审计处置，不能丢弃或重发。daemon 持久化、接收事件、真实 QQ 和退出恢复仍未验证；现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-17 feat-004 发送结果快照确认已接正式 owner：JS 保留终态至指定随机代次/序号快照被确认，owner 确认失败先重试；对象已替换时不清理新值。JS/decoder/resident 离线串联与负例见 [progress](progress.md#current-state)。确认不是 daemon 持久 ACK；未知请求的忽略策略、接收队列 V8 返回失败、宿主退出/恢复、同对象原地变更及实际 QQ 未验证。继续按合同验证持久接收、真实线程及 drain；现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 batch 清空前准入：生产轮询对最终 JSON 大小和 decoder 基础字段校验失败时保留相关状态；故障注入和修复后重取见 [progress](progress.md#current-state)。超时扫描仍先执行，JS 校验不是 Rust/daemon ACK，单条事件、V8 结果丢失、唯一代次与确认重试仍需处理；继续完成版本化确认。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 轮询 session 前置核验：正式 owner 使用一次捕获的 session 生成绑定轮询和 decoder；空绑定跳过，JS sid 不匹配时变更前拒绝，合成复现与匹配对照见 [progress](progress.md#current-state)。sid 名称不是唯一 listener 代次，V8 返回失败、其他解码错误、确认重试和 daemon 持久化仍未解决；继续完成版本化确认协议。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 resident 重复空身份结果：`IdentityPending` 收到重复空 native ID 不再重复通知，后续强身份成功和终态去重通过离线回归，详情见 [progress](progress.md#current-state)。这不提供结果版本确认、失败后重确认或持久化保证；下一步继续生产确认协议，JS 拒收后丢终态缺陷仍未修复。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 结果容量防护：文本/图片/语音在 `sendResults` 达 128 条时于 `sendMsg` 前拒绝并保留旧结果，受控回归与边界见 [progress](progress.md#current-state)。这只为未来确认提供背压，并未修复批次拒收后结果丢失；下一步处理唯一代次、结果修订及 owner 重复接收/确认失败后可安全再确认，再完成生产接线回归。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 轮询确认离线状态模型：单在途批次、代次/修订匹配、重取及有界负例通过，修订检查变异可被测试检出；详见 [progress](progress.md#current-state)。模型不接 QQ/生产路径、不分配唯一代次也不是持久化确认；已知拒收后丢终态缺陷仍在。下一步核定生产代次、预算背压与接收/确认顺序，再对实际 JS 和 Rust owner 做失败、更新和恢复回归。现场须新鲜进程重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 轮询确认边界：生产 JS + decoder 的离线故障注入复现首批因 session 不匹配被拒后终态已从 JS 清除；这是合成对照，不证明真实发生，详情和复验入口见 [progress](progress.md#current-state)。下一步设计并测试按监听器代次和结果修订保护的接收确认，覆盖失败、重复、在途更新、内存上限和 daemon 持久化边界；此前不声称已修复或端到端确认。任何新现场须新鲜进程和重新准入，unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 轮询超限离线修复：合成测试发现未放入 512 KiB 批次的终态曾被清除，现保留至下批，单条无法放入空批次的终态保守 unknown；回归及边界见 [progress](progress.md#current-state)。序列化不是下游确认，修复版现场和真实迟到/线程/drain 未验证。后续继续按现有观测合同推进；任何新现场须新鲜 QQ 进程及重新准入，旧 unknown 不重发，未证寿命不卸载。

2026-11-16 feat-004 媒体 Promise 修复版语音现场：用户授权一次语音，QQ `38060` 准入登录后单次发送、OneBot `get_msg`、账本指纹及对端一条确认通过，详见 [progress](progress.md#current-state)。回读基于持久账本，非 QQ 原生下载；manager/owner 停止、端口关闭、QQ 响应且 bridge 保留，不重接、不卸载、不重发。本轮未制造真实非零 Promise，真实非零语义、未知结果后的迟到匹配及原生线程/drain 仍未证，feat-004 in-progress。

2026-11-16 feat-004 媒体 Promise 修复版图片现场：用户授权一次图片，QQ `48480` 准入登录后单次发送、OneBot `get_msg`、账本指纹及对端仅一张确认通过，详见 [progress](progress.md#current-state)。回读基于持久账本，非 QQ 原生下载；manager/owner 停止、端口关闭、QQ 响应且 bridge 保留，不重接、不卸载、不重发。此轮未制造非零 Promise，语音正常路径、真实 cookie/迟到和线程/drain 尚未证，feat-004 in-progress。

2026-11-16 feat-004 媒体非零 Promise 离线修复：图片/语音原先在非零 Promise 先于通知时会输出 rejected 并进入 Failure；现统一 unknown，安全整数保留诊断码，两种顺序和异常/成功对照见 [progress](progress.md#current-state)。`pending.failed` 仍阻止不确定结果后的迟到匹配，不能宣称迟到纠正已闭合；真实非零语义及新构建现场未验证。后续现场须新进程、固定 hash 准入和单次发送授权，不重发旧样本；feat-004 in-progress。

2026-11-16 feat-004 HTTP 修复版语音现场：用户授权一次语音，QQ `42396` 准入登录后单次发送、紧随其后的 OneBot `get_msg`、账本指纹及对端一条确认通过，详见 [progress](progress.md#current-state)。回读基于持久账本，非 QQ 原生下载；manager/owner 停止、端口关闭、QQ 响应且 bridge 保留，不重接、不卸载、不重发。旧 10053 根因及真实 cookie/线程/drain 仍未证，feat-004 in-progress。

2026-11-16 feat-004 HTTP 修复版图片现场：用户授权一次图片，QQ `4544` 准入登录后单次发送、紧随其后的 OneBot `get_msg`、账本指纹与对端仅一张确认通过，详见 [progress](progress.md#current-state)。回读基于持久账本，非 QQ 原生下载；manager/owner 停止、端口关闭、QQ 响应且 bridge 保留，不重接、不卸载、不重发。旧 10053 根因、语音连续回读及 cookie/线程/drain 仍未证，feat-004 in-progress。

2026-11-16 feat-004 HTTP 修复版单次现场：用户授权一次文本，QQ `55192` 准入登录后单次发送、紧接着的 `get_msg`、账本及对端一条确认通过；独立审计及旧 runner 轨迹误解析保留见 [progress](progress.md#current-state)。manager/owner 停止、端口关闭、QQ 响应且 bridge 保留，不重接、不卸载、不重发。该样本不证明既往 10053 根因、媒体连续发送回读或真实 cookie/线程/drain，feat-004 in-progress。

2026-11-16 feat-004 HTTP 修复版零发送现场：新鲜 QQ `47980` 准入、登录后，延迟请求头与分段请求体的状态请求各一次通过，原图片复制账本 `get_msg` 通过；客户端自报延迟，服务端 accept 时序未独立观测。零发送及正常停止经独立审计，详见 [progress](progress.md#current-state)。QQ 同代次响应且 bridge 保留，不重接、不热卸载；既往 10053 根因、发送后连续回读及 cookie/线程/drain 尚未证明。

2026-11-16 feat-004 HTTP 10053 离线修复：Windows 下监听器非阻塞模式导致已接收连接在请求未到齐时立即失败；受控延迟对照及正式入口回归先失败后通过，`serve_one` 现先将连接改为阻塞并沿用读取期限。现场两次 10053 没有独立内核证据，不能断言同一根因；新构建尚未 QQ 现场验证。结果、HIGH 影响面与产物见 [progress](progress.md#current-state)。后续现场先指定新鲜进程、按合同准入；不重发旧消息，真实 cookie/线程/drain 仍未证，feat-004 in-progress。

2026-11-16 图片数量补充确认：用户明确确认仅收到一张，内容与原预期一致；补充证据和原记录保留见 [progress](progress.md#current-state)。发送后两例 `get_msg` 10053 根因仍待调查，不重发样本。

2026-11-16 feat-004 图片 cookie 防护版现场：用户授权的单次图片发送 retcode 0、单请求账本和图片指纹匹配；用户确认图片可见且内容相符，未明确核实数量。首次 get_msg 因 10053 中断，原因未知、失败保留；新鲜 QQ `6184` 上对原消息复制账本的一次 OneBot 只读回查通过，非 QQ 原生查询。两轮均正常停止，最新 QQ 响应并保留 bridge，不重接、不热卸载或重发；证据见 [progress](progress.md#current-state)。真实 cookie 复用、迟到与线程/drain 未证，feat-004 in-progress。

2026-11-16 feat-004 语音原消息只读回查：新鲜 QQ `42452` 准入、登录后，对原发送消息的复制账本执行一次 OneBot `get_msg`，返回匹配语音标识及原消息身份；零发送、独立审计和正常停止通过。首次 `get_msg` 的 10053 失败保留、根因未知；此回查读取复制账本而非 QQ 原生查询。证据见 [progress](progress.md#current-state)。QQ 同代响应且 bridge 保留，不重接或热卸载；真实 cookie 冲突、迟到回调及原生线程/drain 未证，feat-004 in-progress。

2026-11-16 feat-004 语音 cookie 防护版单次现场：用户授权的一条语音发送 retcode 0、持久账本和用户播放确认通过；现场 `get_msg` 连接 WinError 10053，未获回读，根因未知，不重发。独立审计与失败证据见 [progress](progress.md#current-state)。manager/owner 正常停止、端口关闭；QQ `33768` 响应且保留 bridge，不得重接或热卸载。下一步可准备新鲜进程上只读查询原消息，不能把旧失败覆盖为成功；真实冲突、线程/drain 仍未证，feat-004 in-progress。

2026-11-16 feat-004 语音已退役 cookie 离线防护：合成复现同 cookie、目标及语音指纹的旧通知误确认，现为本 JS 环境内语音 cookie 维护有界历史，耗尽拒绝；回归与范围见 [progress](progress.md#current-state)。真实 QQ 复用和新代码现场未验证；后续现场须指定新鲜进程、重新准入，真实发送另需明确授权，不重发旧样本。其他来源及原生线程/drain 未证，feat-004 仍 in-progress。

2026-11-16 feat-004 图片已退役 cookie 离线防护：合成旧通知在同 cookie、目标与图片指纹下误确认第二条图片，现限制同一 JS 环境内本图片 guard 已认领的 cookie，不驱逐历史且容量耗尽拒绝；边界和测试见 [progress](progress.md#current-state)。真实 QQ 复用及新代码现场未验证；后续现场须指定新鲜进程、重新准入，真实发送另需明确授权，不重发旧样本。语音历史、其他来源及线程/drain 不受本次覆盖，feat-004 保持 in-progress。

2026-11-16 feat-004 已退役 cookie 防护版单次私聊：用户授权一次，新鲜测试 QQ `22232` 准入、登录、发送/持久回读及单请求账本技术审计通过；用户确认对端收到且仅收到一条、内容相符，独立完整审计通过。manager/owner 正常停止、端口关闭，同代 QQ 响应并保留 bridge；不得重接、热卸载或重发。轨迹解析原失败、独立复核及范围见 [progress](progress.md#current-state)；真实 cookie 复用、迟到和原生线程/drain 未证，feat-004 保持 in-progress。

2026-11-16 feat-004 已退役 cookie 防护版零发送现场：固定构建新进程 `26916` 准入、单次接入、登记账号登录核验及正常停止通过，空账本与合成停止探针经独立审计；详细证据见 [progress](progress.md#current-state)。该进程已加载 bridge，不得重接或热卸载；零发送未执行文本 guard，不能证明真实复用与原生 callback 线程/drain。

2026-11-16 feat-004 文本已退役 cookie 防护：生产 listener 的合成重放先复现旧通知误确认后续同 cookie 请求，现对当前 JS 环境内本 guard 已认领的文本 cookie 保留有界历史，超限拒绝；测试、限制和证据见 [progress](progress.md#current-state)。后续可在用户指定的新鲜 QQ 代次按固定 hash 与现场合同准备正常路径验证；单次发送需新的明确范围授权，不重发旧样本。此修复未验证实际 QQ cookie 复用、其他来源 cookie、媒体历史或原生线程/drain，feat-004 仍 in-progress。

2026-11-16 feat-004 cookie 修复版正常路径现场：用户授权单次私聊并确认对端收到一条；指定根 `5384` 当次准入、登录核验、发送/现场回读、单请求账本和独立阶段审计通过。manager/owner 停止码 0、端口关闭，QQ 响应并保留 bridge；旧 runner 的 trace 误解析保留，详见 [progress](progress.md#current-state)。该样本没有制造真实 cookie 冲突，迟到/移除后复用及线程/drain 未证；不得重接此进程、热卸载或重发本样本。

2026-11-16 feat-004 文本 cookie 冲突：合成生产 listener 复现同 cookie、目标和正文的两条在途文本由单个 native ID 双重确认；现于原生 `sendMsg` 前拒绝在途重复 cookie，与媒体路径一致。首条成功、第二条拒绝、媒体占用与不同 cookie 同文正例、普通/research 回归及隔离 research release hash 见 [progress](progress.md#current-state)。当时修复尚未现场加载，后续正常路径样本见上条；该检查不覆盖移出在途表后的迟到与复用，真实 callback OS 线程/最终 drain 仍 unknown。后续现场需指定新鲜 QQ 代次重新准入，发送另有明确范围和授权，不重发旧样本。

2026-11-16 feat-004 私有 ETW 全成员对照：20 次同名同 GUID A→B 替换中，句柄复用，查询结构具名成员与名称共 25 项在 A、B 和过期值查询中一致；仅使用当前句柄停止。脚本、审计和范围见 [progress](progress.md#current-state)。这些字段快照不足以证明私有会话代次，不能外推系统 logger；继续按[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)解决系统级所有权、崩溃恢复和权限，不能凭匹配快照 stop。

2026-11-15 feat-004 私有 ETW query 输出：A 停止后 B 名称/GUID 均不同，旧句柄查询输出两字段为 B（12/12）；B 复用名称/GUID 时输出与 A 两字段相同（12/12），仍不能区分代次；均未用旧句柄停止。脚本、审计负例和范围见 [progress](progress.md#current-state)。不证明其他字段、query→stop 原子性、系统 logger 或跨进程恢复；继续研究独立可核验的系统会话代次与冲突控制，权限和现场准入不变。

2026-11-15 feat-004 私有 ETW 句柄 ABA：A 停止后 B 即便使用不同名称/GUID，12/12 复用 A 句柄数值；A 旧值在 B 创建前 query 12/12 返回 4201、B 创建后 12/12 返回 0。另两组同名对照同样复用；只对本轮当前句柄 stop，未对旧值 stop。细节、hash 和范围见 [progress](progress.md#current-state)。这是本机 in-process private 会话证据，不外推 system logger；后续恢复不能凭过期句柄及 query 成功确认代次，需独立解决系统级所有权与原子性，权限、隐私和现场准入仍未闭合。

2026-11-15 feat-004 私有 ETW 子进程退出对照：控制进程存活时，正常、非零退出和超时终止的自建子进程启动前及结束后，私有 ETW 会话按持有句柄 query/stop 均通过；详情及 hash 见 [progress](progress.md#current-state)。未验证控制器崩溃、具名系统会话或跨进程恢复；下一步仍须建立可核验代次与无法判定时的恢复合同，stop 的 `ERROR_MORE_DATA` 不得盲目重试，系统权限和现场准入保持约束。

2026-11-15 feat-004 空 system logger 尝试：当前未提升 token 的无 provider/无文件/无 PMC/LBR `StartTraceW` 返回访问拒绝 5；未取得句柄、未 query/stop。四项模拟测试验证拒绝及身份边界，脚本 hash 和范围见 [progress](progress.md#current-state)。系统会话成功和清理仍未验证；先完成异常终止恢复与独立所有权方案，再在具备权限和隔离条件时重新准入，不凭同名停止、不开 QQ 采样。

2026-11-15 feat-004 ETW 进程内对照：当前 token 下自建、无 provider 的 private in-process 会话启动/按句柄查询停止通过；同名碰撞 183，两个独立私有会话中停止一个不影响另一个，两个均正常停止。脚本、结果 hash 和范围见 [progress](progress.md#current-state)。该结果不证明 system logger、PMC、LBR 或崩溃清理；下一步先在明确权限和隔离条件下验证具名系统会话及冲突清理，不在系统或 QQ 上采用未经验证的采样方案；不凭同名停止其他会话。

2026-11-15 feat-004 LBR 会话离线模型：新增纯状态模型及 7 项测试、身份检查变异负例和 Windows CI 接线，详见 [progress](progress.md#current-state)。无 ETW 后端或系统调用，不能认作真实会话控制；下一步验证独立具名 system logger 的真实所有权、冲突与崩溃清理方案并审查权限/隐私后，才考虑仅对自建 workload 采样。不要停止默认 kernel logger，不对 QQ 使用未验证方案；本轮未启动 ETW、未接入 QQ、未发送。

2026-11-15 feat-004 LBR 对照准备：自建进程扩展 PID/创建时间、线程和 QPC 真值；独立 Windows API 核验代次及解析器正反测试通过。系统级隐私、独立会话所有权、全局 PMC 冲突、异常清理和时钟/丢失门已写入[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)，细节和本机产物 hash 见 [progress](progress.md#current-state)。尚无可安全启动的采集控制器，未启动 ETW；下一步先实现/离线验证会话与冲突控制，再在权限和隔离条件满足时对自建进程采集；不要使用默认内核 logger 清理，不对 QQ 使用未验证方案。

2026-11-15 feat-004 观测候选离线对照：WPR/xperf 和 PMU 来源可见，但 LBR 采集未启动，本会话未提升；自建双线程同回调 workload 已确认两条独立间接调用和线程真值，首版调用点合并与正反验证均保留，详见 [progress](progress.md#current-state)。下一步先审查系统级采样隐私、会话所有权/清理、丢失和 TID 归属，再在满足权限及隔离条件的自建进程开展 LBR 校验；无法关联请求则保持 unknown，不对 QQ 使用未验证方案。未接入 QQ、未发送；PID `5384` 当次仍响应且 bridge 未映射，后续重新核验代次和准入，不重发旧样本。

2026-11-15 feat-004 只读现场：用户重启后 PID `5384` 通过固定模块、backend、唯一 Environment 准入；只读进程内存核对 `sendMsg` 描述项的指针和名称，错误目标负例拒绝。未加载 bridge、未发送，结束时 QQ 响应；详细 hash、首次工具错误及证据见 [progress](progress.md#current-state)。描述项不证明类注册或实际回调；继续先建立符合[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)的原生入口、对象关联及线程采集和离线负例，后续现场重核 PID/创建时间、模块、backend、唯一 Environment 和账号；旧授权单次样本不重发，未证寿命不热卸载。

2026-11-15 feat-004 固定 hash 静态续核：sendMsg 命名 thunk → NodeIOperateCallback 引用对 → 虚槽 +0x118 → 候选 KernelMsgService → lambda 捕获 → callback 虚槽 +8 → 分派器，已在限定接收者类型及分支条件下逐指令/RTTI/栈布局核对；正反断言、首次算式错误和 hash 见 [progress](progress.md#current-state)。同 hash 的 `sendMsg` 属性表与 `napi_define_class` 初始化路径亦有条件静态核对；不证明实际接收者、类注册成功、现场执行、callback 线程或最终 owner/drain。后续先验证运行时接收者与可安全观测的线程合同；新现场须新鲜代次准入，不重发、不热卸载。

2026-11-15 feat-004 请求级轨迹群聊现场：用户授权后新根 `39500` 准入，登记群单次文本的 HTTP 发送/get_msg、持久账本和用户确认一条均通过；新解析器观察两批同一代次/序号，scope 与完整性仍为 unknown。manager/owner 正常停止、端口关闭，QQ 响应且保留 bridge；不重接、不热卸载或重发。证据、固定构建和解析器 hash 见 [progress](progress.md#current-state)。真实迟到、QQ native callback OS 线程和最终 owner/drain 仍待独立证据；新现场须新进程重新准入。

2026-11-15 feat-004 阶段日志解析：`node scripts/analyze-text-send-trace.mjs <本机日志>` 只读解析固定白名单格式，异常、溢出或缺记录返回 unknown，成功也标范围未核验和完整性未知；新增测试纳入 Windows CI。已对上轮原始日志只读复核，测试与证据见 [progress](progress.md#current-state)。下一步继续独立研究实际 QQ callback OS 线程、最终 owner 和 drain；现场必须新代次准入，不重发旧消息、不热卸载。

2026-11-15 feat-004 请求级轨迹单次现场：用户授权后新根 `50664` 准入并单次向登记 B 发送，HTTP 发送/get_msg、持久账本和用户确认一条均通过；两批轨迹同一代次/序号且完整。原 runner 因日志单引号误解析标 `trace_pass=false`，独立审计解析原日志通过，原结果保留。manager/owner 正常停止、端口关闭，QQ 响应且保留 bridge，不重接、不热卸载或重发。详见 [progress](progress.md#current-state)。下一步先改进离线日志解析与报告，并继续独立研究实际 QQ callback OS 线程及最终 owner/drain；新现场须新鲜代次准入。

2026-11-15 feat-004 新诊断构建零发送现场：新 QQ 根 `47144` 准入、监听、登记账号登录核验和合成停止探针通过；manager/owner 退出码 0、端口关闭、空账本、QQ 响应且保留 bridge。该进程不重接、不热卸载；真实迟到、请求级轨迹现场、callback OS 线程和最终 drain 均未验证。首次 PowerShell 模块路径失败发生在接入前，保留后另目录复验；证据与 hash 见 [progress](progress.md#current-state)。后续测试使用新鲜进程重新准入，需发送时另明确单次目标和授权，不重发 unknown。

2026-11-15 feat-004 请求级诊断离线实现：research 阶段记录现携带 JS 环境内代次和临时请求序号，并标记批次溢出或无法分配；已补并发、跨 poll、旧代次、耗尽、异常与隐私负例。完整证据与日志见 [progress](progress.md#current-state)，边界见[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)。旧 Windows 磁盘索引刷新中断、完整性未知；独立 WSL 增量索引曾触及 16 GiB 上限，后续须使用新建的干净索引并核验。现场未加载，缺日志仍 unknown；实际 QQ callback 线程和最终 owner/drain 仍需独立证据。

2026-11-15 feat-004 并发轨迹核查：实际生成的两个发送脚本在 Node VM 中以相反顺序完成 Promise，得到相同的 listener 级标签数组；11 请求样本另显示 32 项上限会无标记丢失阶段。现有标签不能用于并发请求的逐请求顺序或缺失事件证明。复现入口、hash 及边界见 [progress](progress.md#current-state)，后续请求级诊断约束见[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)。下一步先针对有界、代次隔离的诊断关联设计完成影响分析、隐私和并发负例，再实现与离线验收；实际 callback OS 线程及 QQ drain 仍需独立证据。

2026-11-15 feat-004 合成探针补查：独立 VM 6/6；内存移除 onRecvMsg stopped 守卫的变异按预期失败，脚本/日志 hash 和范围见 [progress](progress.md#current-state)。测试符号 GitNexus 精确 UID 为 UNKNOWN，经源码核对隔离的 Node VM 测试后已纳入三个负例，research 定向 5/5。下一步补证测试符号映射，并在[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)下准备真实事件与线程观测；不能把合成结果升级为现场证据。

2026-11-15 feat-004 离线续验：已通过 WSL 原分析器恢复当前 GitNexus 索引；普通/research 全仓、research 定向 17 项与隔离 research release 通过。原有未提交探针保留，限于合成停止态调用，不是实际 QQ callback 证据。详细图限制、构建 hash 与日志见 [progress](progress.md#current-state)。下一步在现有[观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)下设计可关联的实际事件和线程观测；新现场须新代次准入，unknown 不重发，未证寿命不热卸载。

2026-11-15 feat-004 离线续查：原有未提交停止合成探针未改动，wrapper_send Windows 定向 5/5；新增[线程与停止观测合同](docs/acceptance/2026-11-15-feat-004-observation.md)，将合成、真实迟到、JS 与 native 线程和最终 drain 分开判定。下一步先解决 Windows 与 WSL worktree 的 Git/索引路径不一致（GitNexus 状态分别报 not-git-repository 与 foreign），保留现有索引和未提交代码；再审查原有探针并完成可关联的安全观测实现及正反测试；现场须新代次、重新准入，未获证不得热卸载，unknown 不重发。详见 [progress](progress.md#current-state)。

2026-11-15 feat-004 停止隔离版现场：新根 41684 准入、正式构建单次接入、监听和登记账号登录核验通过；零发送，manager/owner 停止码 0、端口关闭、QQ 响应且保留 bridge。未触发或观察停止后迟到回调，未证明 QQ 内注销/全局 drain；该进程不重接、不热卸载。证据和 hash 见 [progress](progress.md#current-state)。后续先设计可验证的线程及在途回调观测合同，需新现场时重走新鲜代次准入；不追测业务拒绝。

2026-11-15 feat-004 快照与停止隔离：固定 9.9.36 静态路径显示 listener 登记节点移除后，先前拷贝的引用快照仍可能持有对象；消息 listener 入口现拒绝 stopped 后的迟到通知，媒体完成回调保留临时文件清理。定向回归及失败历史、固定 hash 与证据见 [progress](progress.md#current-state)。新实现未现场加载；继续按新鲜代次和准入合同设计正式 callback 线程/最终 owner/drain 证据，不重发、热卸载或追测业务拒绝。

2026-11-15 feat-004 回调与释放：9.9.36 固定映像已静态验证 Promise callback 的同线程/异线程分派与停止分支，以及消息 listener remove 的条件路径；只读清单显示先前两进程已退出，PID 44580 同代次仍持有 bridge。详情及可复验 hash 见 [progress](progress.md#current-state)。静态机制不证明正式链路实际 callback TID、listener 注销、最终 owner 或全局 drain；须先设计可验证的动态观测和安全寿命合同，新现场才使用新鲜代次重新准入。不要热卸载、重发旧样本或重启业务拒绝核验。

2026-11-15 feat-004 范围决策：参考 Caligo_research 后未找到现行构建的明确业务拒绝语义，按用户要求记录已知缺口，不再核验拒绝信息；账号正常连接后可提交群聊/私聊，单次未确认结果仍是 unknown、不自动重发。正式文本路径没有拒绝预查询；Active 是 core/bridge 状态，不能单独证明账号登录，登录身份查询与发送准入的接线仍需核对。本轮无代码或现场动作。其余 callback OS 线程、最终释放和历史 G3 consumer 合同仍开放；详情与证据见 [progress](progress.md#current-state)。

2026-11-15 feat-004 离线续验：Promise 非零码先于 add 不能证明明确失败，现保守 unknown 并保留整数；定向回归先失败后通过。参考研究和固定 hash 的有界静态回调核对及未证边界见 [progress](progress.md#current-state)。本轮无 QQ 接入和发送；既有禁言样本不重发、未证寿命不卸载。后续仍需真实明确拒绝语义、callback 线程、最终释放与 G3 合同证据；新现场须新代次重新准入。

2026-11-15 feat-004 续验：用户指定的禁言测试群经新根 42124、44580 各次独立准入及个人禁言只读核对后，各发送一次不同标记；用户均确认未见，两个请求均为 DeliveryUnknown，不重发。第二次 wrapper Promise 的整数 120 已保留在审计，停止后以实际 AccountActor 打开账本副本核实重启保留且不重派；不能推断其为明确业务拒绝。两个 manager/owner 均正常停止，QQ 响应并保留 bridge；不复用或热卸载。ConfirmedFailure 原因持久化、新旧账本兼容、异常停止隔离与 wrapper 交错均有离线回归；Windows 串行 all 与隔离 research release 通过。详见 [progress](progress.md#current-state)。feat-004 仍 in-progress：真实明确拒绝语义、QQ callback OS 线程、最终 owner/stop/drain 及历史 G3 consumer 合同未闭合。继续前先建立可验证的安全研究合同；任何新现场使用新鲜代次重新准入，原样本不重发。

2026-11-15 feat-004 续验：固定 9.9.36-53644 新根 53716 的单次私聊及 HTTP 回读通过，用户确认对端一条；有界阶段显示登记、submit、匹配回调、受理、精确回读与终结的本次顺序，不能证明 QQ callback OS 线程。manager/owner 已停止，QQ 响应并保留 bridge；不复用或热卸载。wrapper 停止异常与停止态重启拒绝已补回归及修复，新修复尚未现场加载。命令、构建 hash、证据与未闭合门见 [progress](progress.md#current-state)。真实业务拒绝、callback 线程和最终释放未闭合，状态保持 in-progress；下一步先建立可安全复现的拒绝与寿命研究合同，任何新接入须新鲜代次重新准入，不重发既有样本。

2026-11-15 feat-004 进行中：独立工作树 `Caligo-feat-004` 基于 PR #12 `54b6a5a`；正式文本提交异常保守 unknown 的修复、定向及全仓结果和单次现场证据见 [progress](progress.md#current-state)。现场已停止，7204 与 42768 均不复用或热卸载；原发送成功、对端确认一条，新代次现场 HTTP 只读回查通过，原 10053 根因未知。后续补全正式链路回调、拒绝与寿命完成门；任何新接入需用户指定新鲜代次并重走准入，不热卸载。feature 仍 in-progress。

2026-11-15 feat-003 验收完成：固定 9.9.36-53644 的私聊、登记群和自发文本指定样本及离线未知类型合同满足三项完成门，跨代次范围、10053 未知及自发修订核对保留在 [progress](progress.md#current-state)。第七轮 manager/owner 正常停止，QQ 响应且保留 bridge。后续按用户任务选择清单中的下一个工作块；不自动推进 feat-004，不放宽身份准入、不重发旧样本，未证寿命不卸载。

2026-11-15 v0.3.0 发布准备：版本与 README 同步；发布结果见 [Release](https://github.com/Enoa-devteam/Caligo/releases/tag/v0.3.0) 和 [progress](progress.md)。现场及 feature 状态不因发布改变。

2026-11-15 README 更新：基于 main `e99ea37c` 区分 v0.2.0 与当前源码入口，明确固定 QQ 构建和桥接准入；详见 [progress](progress.md)。本次不推进现场或 feature 状态。下次按用户任务及当前进度选择工作块。

## 2026-11-15 正式 bridge 统一

`codex/unified-qq-bridge` 基于 PR #9 的工作分支。正式 `caligo.exe cli qq-entry` 通过当前 PID、创建时间、
模块指纹和唯一 Environment 接入发布包内的 `caligo_bridge.dll`。普通 release 同一组产物完成真实 QQ
Web UI 登录核验和单条私聊入站，现场正常结束，已映射 bridge 保留到 QQ 正常退出。
源码检查点、散列及默认与研究测试见[进度](progress.md#current-state)。
GitHub CI 已统一为单个 Windows x64 正式桥作业，Release 复用同一 Test。
本地诊断仍可用 `init.ps1 -Mode research`，统一后的验证结果见进度记录。
后续桥接修复与验收沿正式入口推进。终端真实 QQ、重连及长期资源寿命仍按各自合同验收。
[正式接入与停止步骤](docs/guides/management-ui.md#接入-qq-与核验)

## 2026-11-15 启动账号选择与消息日志

工作分支 `codex/account-startup-message-log` 基于 v0.2.0。正常入口为
`caligo.exe manage` 或 `caligo.exe manage --terminal`。代码接线在
[manager](crates/caligo-core/src/commands/manager.rs) 和
[管理 HTTP](crates/caligo-core/src/management_http.rs)，本地验证与首轮异常见[进度](progress.md#current-state)。
PR #9 已完成 Web UI 的 QQ 9.9.36-53644 单账号启动、当前登录核验和单条私聊入站日志现场。
管理器与桥接 owner 已正常停止，QQ 仍响应并保留 bridge。原始证据目录和范围见上方进度链接。
终端保留 LAB 证据。feat-010 保持 in-progress。

此阶段确定以正式桥作为唯一运行和发布实现，后续推进与现场证据见上方正式 bridge 统一记录。

## 2026-11-15 Windows 发布 0.2.0

`main` 已合并 [PR #8](https://github.com/Enoa-devteam/Caligo/pull/8) 的单 EXE 打包。版本由 workspace manifest 与锁文件统一，下载入口及托管验证见 [v0.2.0 发布页](https://github.com/Enoa-devteam/Caligo/releases/tag/v0.2.0)。本地验证见 [progress](progress.md)，下次发布见[开发指南](docs/guides/development.md#github-actions-test--release)。现场实验继续按当前准入合同单独执行。

## 2026-11-15 K3-E 归档与 Release 入口

`codex/k3e-single-release` 基于 CI 合入后的 `main`。K3-E 的 [研究归档](docs/research/k3-e-exec-retrospective.md) 汇总 31 份历史材料，原件留在本机。Release 在两种 Test 构建通过后生成一套普通 Windows x64 ZIP，入口为 `caligo.exe`。本机离线验证见 [progress](progress.md)，重跑方式见[开发指南](docs/guides/development.md#github-actions-test--release)。

下一步按 PR 最新 checks 检查 Windows 两套测试与分支手动 Release 的包内容。版本发布由后续版本标签触发。现场实验继续按当前准入合同单独执行。

## 2026-11-14 CI release / test — Current Objective

已创建 [PR #7](https://github.com/Enoa-devteam/Caligo/pull/7)。首次 GitHub runner default 的 bridge 并发访问冲突、research 的管理员默认文件 owner 导致失败，原始 run 与修复边界见 [progress](progress.md)。现以串行 test harness 和 runner 临时 current-user 默认 owner 复验全部用例；不能把本机通过当托管 CI 通过，也不能跳过原 ACL 断言。最终结果以 PR 最新 checks 为准。

用户要求基于现有 `main` 建分支并开启 PR。工作分支 `codex/ci-release-test` 基于 `d1ad46a1`，独立 worktree 为 `Caligo-ci-release-test`；原工作区未提交改动保持原状。变更包括 `.github/workflows/test.yml`、`release.yml`、打包脚本及干净检出不依赖本机 skill 的文档检查/链接修复。Windows MSVC `init.ps1 -Mode all`、Node tooling（16 PASS / 2 SKIP）、actionlint、PowerShell 语法/标签边界、两套 release 构建与 ZIP 文件/哈希/元数据核验通过，证据见 [progress](progress.md)。

PR 合并后的标准入口为 [开发指南](docs/guides/development.md#github-actions-test--release)：PR/main 执行 Test，手动 Release 只生成 artifacts，匹配 workspace 版本的 `v*` 标签先跑 Test 再发布 prerelease。本任务不打标签、不发布版本、不启动下一阶段现场；产品 NOT READY 与既有 feature 验收边界保留。GitHub 上的工作流结果以 PR checks 为准，本地测试不能代替托管 runner 验证。下方为前阶段历史交接。

## 2026-11-13 SnowCat integration — Current Objective

2026-11-14 用户要求先收口第一阶段并提交 PR #6。本次只提交语音缓存修复/回归与验收、状态文档，随分支推送既有 1e8dbb9，并以 PR 评论列明已验收功能和未验边界。范围与证据以[第一阶段清单](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-第一阶段现场验收收口)为准；PR 保持 draft，不合并。本阶段现场已停止；转码与任何下一阶段工作等待后续任务，不自行启动。各 feature 状态与产品 NOT READY 保留。下方“尚未提交”描述为各历史检查点，当前提交状态以 git log 和 PR #6 为准。

2026-11-14 已发群语音 SILK 只读现场通过。新 A 38256 用 B7D38EBB…5764F8F 修复版对原持久消息各执行一次 get_msg/get_record(silk)，均 HTTP 200 / 0；缓存回调 validated，返回 2,575 字节与原三音降调样本逐字节及 SHA-256 一致，账本登记群匹配。daemon/owner 停止 exit 0，最终 HTTP/SSE 无监听、QQ 响应并保留 bridge，不复用或热卸载。本轮零发送，前轮单次群发送及 B 播放确认证据保留，原 1501 失败不覆盖。本机 `local-evidence/snowcat-sent-group-voice-cache-repair-20261114/field-summary.private.json` 保存结果；修复和验收记录尚未提交，HEAD 仍 1e8dbb9。普通私聊/登记群语音两方向及原始 SILK 各有现场样本，转码/其它媒体/长期资源与恢复仍待验，feat-008 in-progress。下一块只准备语音转码工具配置、原消息只读范围和输出校验方案，未准备或启动新现场；准备完成后按新指定 A 准入，不重发。[现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-已发群语音-silk-现场通过)

2026-11-14 登记测试群 B → A 的语音入站、用户播放与原始 SILK 下载通过。新 A 54700 读回原消息并下载 7,294 字节，匹配预登记缓存回调与此前缓存摘要。daemon、owner 和 HTTP/SSE 已关闭，QQ 响应并保留 bridge。原方向更正和接收证据在本机 `local-evidence/snowcat-group-voice-receive-20261114/`，复验在 `local-evidence/snowcat-group-voice-cache-repair-20261114/`。下一工作块为群主动语音发送，需按该动作独立准入。feat-008 保持 in-progress。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-登记群入站语音-silk-现场通过)

2026-11-14 已发私聊语音的 `get_msg`、原始 SILK `get_record` 和逐字节核对通过。新 A 33668 正常停止 daemon 和 owner，HTTP/SSE 端口已关闭，QQ 响应并保留 bridge。本机 `local-evidence/snowcat-sent-private-voice-cache-repair-20261114/` 保存回调与字节校验证据。已发语音缓存修复和本轮记录尚未提交，前阶段提交为 `1e8dbb9`。下一工作块可先准备登记测试群的语音入站，在新指定 A 上核对 SSE、持久消息 ID 和原始 SILK。群发送与转码各自验收，feat-008 保持 in-progress。[现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-已发私聊语音-silk-现场通过)

2026-11-14 主动私聊语音单次发送、持久回读和 B 实际播放通过。A 53416 的 daemon/owner/HTTP/SSE 正常关闭，QQ 响应并保留 bridge。本机 `local-evidence/snowcat-private-voice-send-field-20261114/field-summary.private.json` 保存字段和用户核对结果。已发语音的只读下载由 A 33668 的上述结果接续。[发送现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-主动私聊语音发送现场通过)

上一代诊断版 A 53180 捕获了同消息、同元素的零错误回调，因 `chatType=null` 且目标路径不同返回 1501，现场已正常停止。该样本由上面的修复版结果接续。[诊断记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-私聊语音缓存回调定位与适配)

2026-11-14 普通私聊语音首轮到期无样本：用户已确认 B 未发送；A 32064 的 daemon/owner 停止、HTTP/SSE 已关闭，QQ 响应并保留 bridge，不重接或热卸载。新一轮脚本在本机 `local-evidence/snowcat-private-voice-receive-20261114-attempt2/run-sample.ps1`，同一固定 release/pins、独立采集目录与新测试短语。用户准备正常重启 A；等其确认登录主界面后重新核对 PID/创建时间与准入，listener_armed/LoginInfo/SSE 就绪再让 B 单次录音，并由用户确认 A 播放。零工具发送，不自动重试，feat-008 不提升。[现场收尾](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-私聊语音首轮到期无样本)

2026-11-14 最新：新 A 48692 的已发送群 PNG get_msg/get_image 0，18,615 bytes 与原发送图逐字节/SHA-256 相同，null callback validated，账本登记群匹配。本轮零发送，前轮发送/B 可见性是跨代次证据；daemon/owner/HTTP/SSE 已关闭，QQ 响应并保留 FB12A275…4BC4235 bridge，不重接或热卸载。下一块普通私聊语音入站，仅原始 SILK：本机 `local-evidence/snowcat-private-voice-receive-20261114/run-sample.ps1`、capture.py、sample-plan（含唯一测试短语）、pins/snapshot 已准备，复用当前 release，尚未启动。新 A 登录主界面后准入、listener_armed/LoginInfo/SSE，就绪才请求用户从 B 录制单次 3–5 秒并确认 A 能播放、内容相符；不要在 SSE 前发。验证 SSE/get_msg/账本和单次 get_record(out_format=silk) 的长度/MD5/头，下载限制 128 KiB；无 B 原始文件哈希，不宣称独立解码或转码验收。捕获最多 15 分钟，get_record 失败额外观察 30 秒，不自动重试，finally 正常关闭。仅有语音准备，无 Rust 变更，沿用已通过的同构建全仓结果；其他语音格式/方向、媒体/协议/恢复仍待验，feat-008 in-progress。[证据与下一步](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-已发送群-png-正常下载现场通过)

2026-11-14 最新：新 A 46612 单次 send_group_msg 到登记测试群返回 0/持久 ID，get_msg、账本目标/元数据和 B 群内可见性通过；get_image 1501，匹配的 callback chatType=null，其他消息 callback 被拒绝。daemon/owner/HTTP/SSE 已关闭，QQ 响应并保留 73B11BD1…76EF240 bridge，不重接或热卸载，不重发该图。已复现并将精确 pilot 自发 picType=1000/预登记缓存 null 分支扩展到 group，定向正反回归通过；其他门不变。下一轮仅以原持久 ID 只读 get_msg/get_image：本机 `local-evidence/snowcat-group-png-null-chat-repair-20261114/run-sample.ps1`、停止账本副本、原图/read-plan 已准备；须新 A 登录主界面后重新准入/listener_armed/LoginInfo，失败观察 30 秒后 finally 关闭。default/research 全仓与隔离 release 均通过；新构建 `target-harness/snowcat-group-image-null-chat-repair-20261114/release/`（bridge FB12A275…4BC4235）未加载，feat-008 仍 in-progress。[现场与修复](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-自发群-png-空类型缓存回调适配)

2026-11-14 最新：新 A 52752 的已发送私聊 PNG get_msg/get_image 0，18,569 bytes 与原发送图逐字节/SHA-256 一致，null 回调 validated；本轮零发送，前轮发送及 B 可见性保留为跨代次证据。daemon/owner/HTTP/SSE 已关闭，QQ 响应并保留 73B11BD1…76EF240 bridge，不重接或热卸载。下一块登记群单次主动 PNG：本机 `local-evidence/snowcat-group-png-send-field-20261114/run-sample.ps1`、新图/request/plan/grant/pins 已准备，复用当前修复 release；用户发送授权有效，不再重复询问。须新 A 正常重启并进入主界面后，以 PID/创建时间准入，listener_armed/LoginInfo 后只发一次，核对持久 ID、get_msg、账本登记群/get_image、B 群内可见性。unknown 不重发；群 null callback 仍严格拒绝，不能沿用私聊特例。其他能力不提升，feat-008 in-progress。[证据与下一步](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-已发送私聊-png-正常下载现场通过)

2026-11-14 最新：新 A 43644 的只读样本 get_msg 0/get_image 1501，现场确认为 null chatType，其余 callback 比较匹配，零发送。daemon/owner/HTTP/SSE 已关闭，QQ 响应并保留 D5134337…FA164C bridge，不重接或热卸载。已复现并仅为精确 pilot 的自发 private/picType=1000/预登记缓存路径适配 null，其他分支严格校验；定向回归通过。下一次仍只查询同一已发图片，不重发；本机 `local-evidence/snowcat-sent-png-null-chat-repair-20261114/run-sample.ps1` 准备独立 core/token/pipe、原停止账本副本和 read-plan，预检/单次接入/listener_armed/LoginInfo 后单次 get_msg/get_image，失败观察 30 秒再 finally 关闭。default/research 全仓和隔离 release 均通过；新构建 `target-harness/snowcat-sent-image-null-chat-repair-20261114/release/`（bridge 73B11BD1…76EF240）未加载。必须新 A 正常登录后准入，旧 43644 不复用。发送授权仍保留，其他能力状态不提升。[证据与适配](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-自发私聊-png-空类型缓存回调适配)

2026-11-14 最新：用户已明确授权发送测试消息，后续无需再请求相同范围授权。首轮 A 51188 因 no-live/1502 在发送前停止，提交数 0；补 owner 就绪等待后，用户重启的新 A 12100 单次向 B 发送 PNG，HTTP 200/0、持久 ID/get_msg、账本目标及 B 可见性通过。get_image 1501，callback 仅 chat_matches=false，实际 chatType 未知。daemon/owner/HTTP/SSE 已关闭，QQ 响应并保留 75AE2D73…4A375F bridge；旧代次不重接或热卸载，已发图片不重发。下一块只有 callback 类型分类诊断与该已发消息的只读 get_msg/get_image：本机 `local-evidence/snowcat-sent-png-chat-diagnostic-20261114/` 预备 run-sample.ps1/query-once.py、原账本副本、精确 read-plan 与源码快照；不包含发送动作。default/research 全仓及隔离 release 已通过，新构建 `target-harness/snowcat-sent-image-chat-diagnostic-20261114/release/`（bridge D5134337…FA164C）未加载；须新 A 登录到聊天主界面、重新准入、listener_armed 后才执行一次读取。feat-008 in-progress，群 PNG 发送、其他媒体/协议/恢复仍待验。[现场与下一步](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-主动私聊-png-发送可见与下载类型门)

2026-11-14 最新：登记群 PNG 的 SSE、账本、现场 get_msg/get_image 与原图逐字节/SHA-256 核对通过，群目标匹配登记。A 47904 已正常关 daemon/owner/HTTP/SSE，QQ 响应并保留 75AE2D73…4A375F bridge。下一块 A 到 B 单次主动 PNG：本机 `local-evidence/snowcat-private-png-send-field-20261114/` 的新图/request/plan/pins、run-sample.ps1/send-once.py 已准备，缺单次工具发送授权，尚未接入或发送。先取得明确授权并按 request_sha256 写 authorization.private.json（authorized=true/count=1/request_sha256，记录用户来源），再在用户正常重启后的新 A 准入；正常 LoginInfo 后只发一次，核对持久 ID/get_msg/账本目标/get_image 与原图，B 可见性另由用户确认，unknown 不重发。旧 47904 不复用或卸载，B 可在线。源码/二进制不变，不需重复全仓测试；私聊/群 PNG 入站已通过，其他发送/媒体/协议/恢复未升级。[登记群 PNG 现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-登记群-png-入站与下载现场通过)

2026-11-14 最新：私聊 PNG 的正常 SSE/持久入账/get_msg/get_image 现场通过，下载与新原图逐字节及 SHA-256 相同。A 44108 已关 daemon/owner/HTTP/SSE，QQ 响应并保留修复 bridge（75AE2D73…4A375F），旧失败与 unknown 不改写。下一块登记群 PNG 入站：本机 `local-evidence/snowcat-group-png-receive-20261114/run-sample.ps1` 和新图片已准备，复用 `target-harness/snowcat-image-cache-path-repair-20261114/release/` pins，未启动。需用户正常重启 A 后指定新 PID/创建时间、重新准入，SSE 就绪再由 B 在登记测试群单次原图发送，核对事件/账本/HTTP 回读及 PNG 哈希；旧 44108 不复用或热卸载，B 可在线。feat-008 不提升，主动图片和其他媒体/协议/恢复仍未闭合。[私聊 PNG 现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-私聊-png-正常下载现场通过)

2026-11-14 最新：新 A 20292 已完成下载诊断。SSE/账本/get_msg 通过；get_image 的零错误 callback 消息/元素/chat/slot/deadline 均匹配，仅路径不符被忽略后超时。QQ 已正常关闭 daemon/owner/HTTP/SSE，保留旧 bridge，不重接或热卸载。派发前 host API 登记的缓存路径适配已离线复现/修复，default/research 全仓及隔离 release 通过。新构建 `target-harness/snowcat-image-cache-path-repair-20261114/release/`（bridge `75AE2D73…4A375F`）尚未加载，本机 `local-evidence/snowcat-private-png-cache-path-repair-20261114/run-sample.ps1` 已准备。下一步用户正常重启 A，再准入并验 get_image 字节哈希；不要把官方缓存里的同 hash 文件当正常下载动作已通过。[路径门证据与适配](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-png-下载路径门复现与缓存路径适配)

2026-11-14 最新：新 A 46312 私聊 PNG 的 SSE、账本与现场 get_msg 通过；get_image 单次 1501 超时，无下载字节，临时目录保留。daemon/owner/HTTP/SSE 已正常关闭，QQ 响应且保留 bridge。下一步只核实图片下载的受理、callback 与精确路径/身份匹配；已补有界诊断并通过 default/research 全仓（bridge lib 116/116）。隔离 release `target-harness/snowcat-image-download-diagnostic-20261114/release/`（bridge `F293E157…4AB56B`）未加载；本机 `local-evidence/snowcat-private-png-download-diagnostic-20261114/run-sample.ps1` 已准备，失败后额外观察 30 秒但不重试。下一轮由用户重启指定 A，不复用 46312 或重发本轮旧样本。上一轮入站缺失原因保留 unknown。[现场与下载边界](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-私聊-png-入站回读通过与下载超时)

2026-11-14 最新：B 到 A 的私聊 PNG 单次发送在 A 可见，但窗口内 SSE 与持久消息账本缺失，现场未通过。旧 A 39320 已正常关 daemon/owner/HTTP/SSE，保留 bridge，不重接或热卸载。已补有界 research 图片字段形状诊断，default/research 全仓、隔离 release 和 docs/Windows diff 检查通过。新构建 `target-harness/snowcat-image-diagnostic-20261114/release/`（bridge `72EB54F5…407C5B`），本机 `local-evidence/snowcat-private-png-diagnostic-20261114/run-sample.ps1` 已准备，需新指定 A PID/创建时间；新构建尚未加载。下一轮先用新 A 代次与新构建核实 callback/字段过滤，再逐项 get_msg/get_image 和文件哈希；不要猜 picType 根因或自动重发旧样本。[现场与诊断记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-私聊-png-入站现场缺失与诊断准备)

2026-11-14 最新。Caligo 主动群聊文本通过 OneBot HTTP、发送端回读和 A/B 可见性核验。A 36752 已关闭 daemon、owner、HTTP 和 SSE，QQ 响应并保留 bridge。后续按各项准入推进媒体、其他协议与恢复验收。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-caligo-主动群聊现场通过)

2026-11-14 最新。A 到 B 的 Caligo 主动私聊文本通过 OneBot HTTP、发送端 `get_msg` 和对端可见性核验。A 39368 的 daemon、owner、HTTP 和 SSE 已关闭，QQ 响应并保留 bridge。下一项为登记测试群的单次主动文本发送。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-caligo-主动私聊现场通过)

2026-11-14 最新：用户已授权 A 到 B 的单次主动私聊，A 39368 的预检与离线发送回归通过。执行工具拦截现场命令，未接入或发送。后续复验先核对当前进程代次，并沿已授权的单次测试目标保留回执、回读和对端观察。[预检记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-主动私聊现场预检)。

2026-11-14 最新：B 到 A 的普通私聊和登记测试群文本均已通过 SSE 与现场 HTTP `get_msg`。A 51052 已关闭 daemon、owner、HTTP 和 SSE，QQ 响应并保留 bridge。下一项从固定 wrapper 主动私聊文本的单次样本开始，需新代次和明确的对端发送授权。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-登记测试群现场-http-回读通过)。

2026-11-14 最新：普通文本适配后，B 到 A 的私聊与群聊标记均进入 SSE 和账本，私聊现场 HTTP `get_msg` 通过，两条均经同版生产 handler 从账本副本回读。用户确认实际发送群与 SSE/账本相符，原登记测试群样本仍待验。A 380 已正常关闭 daemon/owner/HTTP，QQ 仍响应且保留 bridge。后续按发送与媒体的各自准入推进。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-普通文本入站现场与空提及字段适配)。

2026-11-14 普通文本入站：用户确认 QQ 显示私聊/群聊标记，A 40844 的 SSE 和账本均缺失。固定 wrapper 的空提及字段已适配，定向、全仓及隔离 release 通过。新构建 `target-harness/snowcat-text-receive-20261114/release/`。待新 A 代次登录并准入后订阅 SSE，再由 B 各发一条新标记，按事件 ID 校验 `get_msg`。原代次 daemon/owner/HTTP 已关闭，QQ 响应，bridge 保留。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-普通文本入站现场与空提及字段适配)。

2026-11-14 最新：A 好友列表 193 项、群列表 75 个唯一群均现场通过，群列表中 2 项保留宿主超容量计数。结合 B 目录、A/B 群详情与成员证据，本轮只读目录缺口闭合。A 50620 仍响应并保留 bridge，daemon/owner/HTTP 已关闭。下轮按整合计划的其余未验收能力和对应准入继续。产品 NOT READY。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-a-好友列表复验与群目录计数修复)。

2026-11-14 最新：A 54900 / B 5608 双账号成员列表和指定成员现场通过，B 原失败目标的 GroupInfo 也通过。A 好友确认重复 UID，已离线去重；群列表计数拒绝已拆细。只需新 A 进程复验这两项。两端已正常关 daemon/owner/HTTP，QQ 仍响应并各保留 bridge，不在原代次重接或热卸载。[现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-双账号群成员通过与-a-目录诊断)。

2026-11-14 最新现场：B 47868 单次 GroupInfo 1440 确认注册返回 `string`，固定 wrapper 的 add/remove 实现验证了原始十进制字符串合同。修复后 A 41076 在登记测试群 GroupInfo 通过，成员/好友/群列表仍 1440。群成员安全整数时间戳已离线适配，好友 UID 与群列表拒绝原因已拆分。两个 QQ 仍响应，daemon/owner/HTTP 均关闭，各保留一份 bridge；不得在同一代次重新接入或卸载。新 QQ 准入后先复验 B 群详情、登记群的成员快照和指定成员，再定位 A 好友及群列表。原始材料和当前构建指针见[执行记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-groupinfo-字符串-handle-修复与目录现场)。

2026-11-14 最新现场：新 A 21800 / B 3932 准入、attach 与 LoginInfo 均通过，但 B 单次 GroupInfo 仍 1440 / `group_detail_listener_cleanup`，按合同停止其余采样，无发送；两端 daemon/owner/HTTP 正常关闭，QQ 响应、旧 bridge 保留。该诊断合并了未知注册 handle 校验与 remove 异常，无法推出派发/移除或 handle 类型。现已离线修正保留注册原因、仅记录类型；未知 handle 不传 remove、不放宽合同，定向及全仓 PASS。新构建 `target-harness/snowcat-directory-registration-diagnostic-20261114/release/`，hashes 与原始现场材料在 `local-evidence/snowcat-directory-repair-field-20261114T0433/`；新构建未加载。下一步新代次先核实注册返回类型与移除合同，不能直接把 string/number 转换当已证修复。21800/3932 不重接/热卸载；好友/群/成员本轮未执行。详见[现场停止记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-groupinfo-修复现场停止与注册诊断收敛)。

2026-11-14 已依据私有研究库固定提交 `7cf025fe1e7f9c609e02a8643c6e6398eeec0d64` 修复 GroupInfo：远程 `GroupApi.fetchGroupDetail` 明确通过 `getGroupDetailInfo(groupCode,KDATACARD)` + `onGroupDetailInfoChange` 取详情，Promise 仅为 GeneralCallResult。Caligo 使用每请求 listener 联合受理/详情，过滤 groupCode/scope/deadline 并终态移除；取消/超时/清理未知关闭详情查询门，已完成后取消/重绑不误隔离。新增 research-only 有界类型诊断到 entry 内部报告，公开失败语义不变。最终 research lib 111/111（脱敏 11 项）、wrapper 7/7、OneBot 10/10、broker 13/13、`init.ps1 -Mode all` 通过；中间失败保留。15 份参考源码 blob/SHA 已核对，manifest 错误已更正，材料在 ignored `local-evidence/research-source-7cf025fe1e7f9c609e02a8643c6e6398eeec0d64/`。新 release 为 `target-harness/snowcat-directory-repair-20261114/release/`，完整 hashes 在 `local-evidence/snowcat-directory-fresh-20261114/repair-builds.json`。下一步新代次准入后复验 GroupInfo、采集剩余目录拒绝的类型诊断；旧 21784/41548 不重接或热卸载。好友列表和 GroupMembers 的 1440 未修复，勿把合成 fixture 当 native raw 现场回放。

当前用户目标是执行 [SnowCat 分阶段计划](docs/plans/2026-11-13-gitnexus-plan-snowcat-capability-integration.md)，分支 `codex/snowcat-capability-integration`，[Draft PR #5](https://github.com/Enoa-devteam/Caligo/pull/5)。已提交 `ef238e1` ProfileService 基线修复、`c1b5b1a` 统一动作 registry、`ca29fd4` 主线整合与目录拒绝语义、`929b914` 固定参考清单。最新证据统一见 [整合执行记录](docs/execution/2026-11-13-snowcat-capability-integration.md)。

本轮 A0 固定注册分母与 Caligo mapping 已提交，`4f90fb0` 接通 A1 typed wrapper 消息至 journal，`65ea2fe` 先行接通 A3 共享 SSE/MCP。最终串行 default/research 全仓、并行 research 全仓、固定参考 6 项和覆盖 12 项检查通过；中间失败和测试修正均保留在执行记录。完整 A1/A2、WebUI/多账号/其余动作、候选冻结和 B1–B3 均未完成。先完成离线接线与验收，不能恢复旧交接中“先现场 ProfileService”的顺序。A0–A4 不启动 QQ；B2 再指定新鲜双账号和精确 profile。历史 unknown 和序号继续保留。后续 `65e7e6c` 已接入 D9 请求关联/精确回读、只读查询、13-key 动作表、显式私聊能力门和 wrapper 同 ID 修订；`8e4583b` 已接入双账号管理/WebUI/媒体工具正常入口。最终 all 与并行 research 通过，浏览器配置/启停/关闭通过，详情见[后续执行记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md)。下一工作块按覆盖清单处理：绑定当前登录结果及昵称、明确 typed wrapper subtype 与事件 fanout 的兼容合同；随后原生媒体上传/下载和 SSO、消息段/管理通知及扩展动作、CLI/SDK/便携部署。不能把仅有工具转换当 QQ 媒体贯通，不能从配置 account 猜登录成功，也不能将本地 correlation 当 native ID。完整候选未冻结，B1–B3 未开始。

恢复执行已完成按连接代次的能力声明、覆盖清单刷新和普通私聊/群聊入站事件接线。下一步先离线核对已登录实例可用的当前身份来源和宿主绑定，再接 `LoginInfo`；若固定参考无可用合同，保持拒绝并推进独立的媒体、SSO 或扩展动作能力。详见[执行记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a1a2-普通私聊与群聊的事件接线)。

语音检查点增加固定 profile 的 `VoiceInfo` 和 `VoiceDownload`。超时后保留在途下载和唯一目标至精确回调，现场仍待候选冻结。[执行记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-私聊语音有界下载)

语音持久化检查点 `e1b4420` 已接当前 profile 的单个私聊入站元素。`318e46d` 的共享事件和 `get_record` 使用同一账号限定文件标识。[执行记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-onebot-语音读取)

`3ddb47d` 接通可选 WAV/OGG。私聊 `record` 发送已有 scoped IPC、owner 提交、精确回读与 OneBot 持久消息 ID。下一工作块核对群语音和其他媒体的宿主文件、关联与资源寿命，再推进 SSO。固定参考 SSO 成功样本依赖实验转换钩子，自有二进制转换仍缺合同与验证。候选冻结前继续离线。[OneBot 私聊语音发送](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-onebot-私聊语音发送)

群入站、已发送私聊语音和群语音发送已接入。普通单 PNG 的 `ImageInfo` / `ImageDownload` 已接固定 profile 的 scoped 查询，合成宿主和真实命名管道验证见[图片查询证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-普通-png-图片精确回读与下载)。普通入站 PNG 的持久身份、D8、共享事件和 `get_image` 已接，见[OneBot 图片读取](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-onebot-png-图片读取)。当前用户明确要求先完成所有整合任务、再统一验收；保持 A0–A4 不启动 QQ。PNG 私聊/群聊发送已接 HostOp/OwnedRequest、独立 scoped image 能力与 IPC、core 持久派发和共享 OneBot；精确自发回读入库后才返回公开 ID，旧声明缺能力默认拒绝。另修复 query broker 9 项上限拒绝当前 11 项声明的真实阻塞。见[图片发送接线](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-onebot-png-图片发送接线)。新回归覆盖真实 worker/管道、scope/撤销/超时/重复、unknown 与重连不重发、持久 ID/恢复；当前只接受 128 KiB 普通 PNG，非完整像素解码验收。下一步继续核对固定参考中的登录结果、SSO 二进制与其他媒体/扩展动作可实现合同；有源合同的继续离线接线，缺原生合同的明确拒绝和列出最小 QQ 取证需求。不能将已登录配置或 Profile 映射当当前身份，不猜 ABI，不因单项缺口提前开始集中 QQ 验收。候选未冻结，历史 unknown 不重发、未证寿命不热卸载。

最新离线块继续接入群成员列表/指定成员，使用强类型 GroupMembers 和精确 pilot 的完整 Map 快照；正常共享动作经真实 pipe 回归，错 scope 和重连清权覆盖。见[群成员接线](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#a2-群成员查询接线)。后续继续固定源合同可实现的其他媒体/资料动作与部署任务；LoginInfo 仍缺已登录会话的当前身份来源，SSO 仍缺自有二进制转换合同。不能用配置账号或本地历史列表合成成功，集中 QQ 验收仍待候选。

原生合同停止点见[下一原生合同门](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#下一原生合同门)：当前登录身份、强制资料刷新回调及自有 SSO bytes 不能从 TypeScript 类型或配置补出成功。需要新指定 QQ/profile 的有界只读合同证据时再进入独立研究；这不是已准入 B2。其他格式/文件视频/扩展动作/CLI SDK 部署仍有离线工作，不能概括成全计划只差 QQ。历史 unknown、未证寿命和 feat-005/006 blocked 保留。

2026-11-14 已在用户指定的两现有登录实例完成首轮有界 ProfileService 取证。两边均 `profile_service_missing`，resolver 零派发，owner pump 已关，bridge 仍映射。先核对固定参考直接交付的 session 和 Caligo 通过 `nt` 重取的 wrapper，再核实 D8 的 `nt_*` 选择及当前账号归属。一次性入口须在新的干净目标代次验证，当前两个 PID 不重复加载或热卸载。[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-双账号-profileservice-现场探针)

已按固定归档晚接入实现修正 research mode 2 的 session 来源，下一轮在干净的已登录根用相同 `qq-entry --uid-map-profile-service` 入口核对加载状态、ProfileService 和精确往返。仅在获得独立当前身份来源后绑定 session，资料刷新 listener 与 SSO 继续按各自合同取证。[修正与验证](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-profileservice-会话来源修正)

新版已在重启后的 A（PID 35864）完成 `roundtrip_match`，owner pump 已关且 bridge 仍映射。后续先验证宿主提供的当前 UIN/UID 与 `nt_1`/host nonce 的绑定，再按独立合同验证资料刷新回调。现有探针输入来自本机资料卡记录，不能将往返值直接当 LoginInfo。[现场复验](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-profileservice-修正后现场复验)

当前身份研究已在 A（PID 50100）无 UIN 输入取得 `nt_1` 的 `onSelfStatusChanged` 身份，精确映射及独立对照匹配，owner pump 正常关闭。下一工作块补已证 UID 的昵称、生产 host nonce/会话代次绑定和失效合同，再决定接入 LoginInfo。资料刷新及 SSO 各自留待受控合同。[现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-当前登录身份通知现场复验)

草稿 PR #6 已发布当前身份修复 `5665b2b`。昵称后续已在 A（PID 31724）通过单次研究验证，已记录同一 session ID 与进程 nonce。下一步验证正常 daemon/IPC 的账号、nonce、会话代次与连接 epoch 绑定及失效，再接入 LoginInfo。当前 A 仍映射探针 bridge，未证寿命不热卸载。[昵称现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-当前登录昵称与宿主-nonce)

正常 daemon/IPC 的 LoginInfo 已在固定构建的 A 单次验证。A（PID 29812）的 daemon 与 owner pump 均已关闭，bridge 留在进程中，B 保持在线。下一工作块先核对资料刷新 listener 的回调关联、超时和移除合同。LoginInfo 的真实换号、断线重连与双账号查询隔离仍列入后续现场矩阵，SSO 独立核对二进制合同。[登录查询现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-正常-daemonipc-登录查询)

2026-11-14 用户再次授权两个现有登录 QQ 继续验收。只读预检确认 A 29812 / B 45824 仍为上轮代次，均响应、各保留旧 bridge，且 caligod/manager 均未运行；模块审计两边都因已有 bridge 拒绝新接入。已请求用户正常退出并重新登录两个测试实例。下一工作块选正常 daemon/IPC 的双账号 LoginInfo 隔离：先重新核对 PID/创建时间、独立身份、精确模块与 PE 审计、唯一 envscan，再使用同一构建各接一次；对照账号/昵称、独立鉴权及停止一端不影响另一端，最后正常关闭。未取得新实例前不重新注入、不热卸载、不发送消息。资料刷新及 SSO 留待各自合同，不将本轮当 B2 完成。[续验预检证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-续验预检与实例代次)

2026-11-14 用户已正常重启两个 QQ，双账号 LoginInfo 隔离工作块通过。A 49952 / B 35780 使用同一已测 release、独立 daemon/目录/随机凭据/管道/HTTP；账号昵称匹配各自 QQ 资料卡，交叉令牌 403、无令牌 401；A 停止后 B 查询成功。两端 daemon/owner 均正常关闭，QQ 均响应、HTTP 已关闭，各保留一份 bridge，不重复接入或热卸载。真实换号、断线重连和长期事件/账本隔离仍未验证。本次只改证据和状态文件，原始数据仅在 ignored `local-evidence/snowcat-field-20261114/dual-login-20261114/`。下一工作块回到资料刷新 listener 的回调关联/超时/移除合同：先离线核对，确需现场时在新进程代次重新核对准入；SSO 独立处理。不得将查询隔离结果概括成消息收发、重连或 B2 全矩阵通过。[双账号现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-双账号登录查询隔离)

2026-11-14 下一工作块已实现资料刷新 research 探针：`qq-entry --refresh-current-profile --uid-map-result <new-local-evidence.json>`，无 UIN 输入，先验证宿主当前身份再单次 KSERVER fetch。精确 wrapper 的 Promise source/detail Map 与通知共用转换器已静态核对，22 个合成场景覆盖联合结果、超时/漂移/迟到与清理未知保留；未注册公开 OneBot 能力。修改 qq_v8/profile_refresh.js、uid_map_probe、FFI mode 4、CLI 参数与门控测试，另把 qq_entry_lab 的 50ms 关闭竞态改为真实唤醒通知同步。初次全仓失败和修正均保留，本轮最终验证/构建见[刷新研究合同](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-当前账号资料刷新研究合同)。隔离二进制在 `target-harness/snowcat-profile-refresh-20261114/release/`。已请求用户正常重启 A（原 PID 49952）；B 可保持运行。新鲜 A 须重新独立核对资料卡、PID/创建时间、PE 审计、实际 loaded hashes 和唯一 envscan，最终离线检查通过后只执行一次探针，不成功时不重试、不热卸载。当前实现/LAB/静态证据不构成资料刷新现场成功，SSO 与 B2 保留各自门。

2026-11-14 用户重启 A 后，PID 25848（创建 `2026-11-14T00:57:15.8739102Z`）完成单次当前账号资料刷新，CLI exit 0 / `profile_refresh_match`，KSERVER 请求回复与目标 UID 通知身份/昵称匹配，listener 移除返回、owner pump 正常关闭。A/B（35780）均仍响应；A 保留本轮 bridge，B 保留前轮 bridge，不重新注入、不热卸载。原始证据在 ignored `local-evidence/snowcat-profile-refresh-20261114/field-*`，见[现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-当前账号资料刷新现场)。当前只有 self 的有界字段/单请求证据，通知唯一请求因果及独立服务器新鲜度未证；公开 OneBot 资料查询、其他目标、换号/重连和长期静默未验收。下一工作块先限定资料输出字段与目标范围，将已证合同接入正常 scoped 查询并离线覆盖撤权/迟到回调/停止；不从本样本自动开放任意 get_stranger_info/no_cache 或 SSO。产品状态不提升。

2026-11-14 self 资料正常接线已完成：`get_login_info(no_cache=true)` → 独立 SelfProfile，严格布尔参数、仅当前账号 user_id/nickname、不回退 LoginInfo。修改 model/capability、core/onebot、bridge/qq_query/qq_entry/qq_v8 及真实 pipe/worker/JS 合同；最终 all 与隔离 release PASS，详见[正常接线记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-当前账号资料刷新正常接线)。新二进制 `target-harness/snowcat-self-profile-wire-20261114/release/`，hash 与脚本/源码快照保存在同名 ignored evidence 目录。下一步只做该入口单次 field：已请求用户正常重启 A（25848），B 35780 可保留；新 A 须独立资料卡、创建时间、精确 loaded hashes、PE 审计和唯一 envscan，再按本机 run-normal-entry.ps1 的 preflight/start/attach/query/cleanup 分段执行。当前未运行该现场；旧代次不重注入或热卸载，失败不重试，身份/hash/审计不符即停。其他字段/目标、SSO、换号重连和 B2 全矩阵不属于本工作块，feat-005/006 blocked 保持。

2026-11-14 用户已重启 A，正常入口资料刷新单次样本通过。新 A 46620（创建 01:25:57.5425320Z）独立资料卡/manifest/PE 审计/envscan 通过，复用前节同源码同 hash release；`get_login_info(no_cache=true)` HTTP 200 / retcode 0，公开 user_id/nickname 与资料卡匹配。daemon/owner 均正常关闭，HTTP 已关闭，A/B 35780 均响应，各保留一份 bridge。详见[现场证据](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-正常入口当前账号资料刷新现场)，本机材料为 `local-evidence/snowcat-self-profile-wire-20261114/normal-*`。本轮只更新证据/状态，无 Rust 变更或提交。下一块先离线收敛真实换号/断线重连撤权合同与验收步骤；任何后续现场均先定 scope 和独立准入，新接入须新代次，不复用 46620/35780、不热卸载。任意目标/其他资料字段、SSO、长期回调静默和 B2 全矩阵仍未验收，feat-005/006 blocked。

2026-11-14 撤权/重连离线合同审计完成，无 Rust 改动或 QQ 操作。见[验收合同](docs/acceptance/2026-11-14-session-revocation.md)与[执行记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-登录会话撤权与重连合同审计)。42 项现有回归通过，但 core 重启可能重复 connection_epoch，QueryScope 无 core nonce；None→owner 未 drain→同 scope 重绑的旧查询取消缺少回归。公开 query ID 含 core nonce，不能直接宣称跨请求污染。下一块先用合成宿主复现该交错（与不同 epoch 正例对照），impact 后评估最小修复并完成适用测试；不得先在 QQ 上试 core 重启。另一个缺口是临时 Profile listener 未接持续 QQ 身份/网络状态到全路径清权，须独立合同。当前不需要用户重启；历史 46620/35780 已映射 bridge 不复用、不热卸载，本轮未重查健康。其他资料字段、SSO 和 B2 全矩阵保持各自边界，feat-005/006 blocked。

2026-11-14 同 scope 撤权交错已复现并完成离线修复。修改 resident 的 set_bridge_scope/invalidate_queries_except，adapter 锁内不可逆标记旧 pending、Disconnected 一次，合法 owner 才取消；同 scope 再绑定不能恢复旧请求。新增 query_revocation.rs 5 项，修复前 1 PASS/3 FAIL 留档，最终 default/research all PASS；详见[修复记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-相同-scope-重绑撤权修复)。未改 drain/native/IPC，未构建新 release 或操作 QQ，旧映射不具备本次修复。

2026-11-14 新代次双账号 LoginInfo 现场通过：用户重启后 A PID 24376（`2026-11-14T02:48:12.5746089Z`）与 B PID 46664（`2026-11-14T02:48:14.6872088Z`）均通过固定 manifest、PE/backend 审计和唯一 envscan；无旧 bridge。隔离 release 各 attach 一次，A 返回 `登记账号 A 及其昵称`，B 返回 `登记账号 B 及其昵称`；交叉 token 403、无 token 401；停止 A daemon/owner 后 B 仍返回 HTTP 200 / retcode 0，随后两端 daemon/owner 正常关闭，QQ 均响应且各保留一份 bridge。全程未发送消息。runner 首次读取 A 的中文响应时发生 PowerShell 默认编码错误，原始 HTTP 200 UTF-8 响应保留且不重发；显式 UTF-8 修正后只执行尚未查询的 B。该样本只闭合新代次 self LoginInfo、鉴权与停止隔离；消息事件/账本、同时在途、换号、QQ 网络重连、全局身份撤权、媒体/SSO 和长期静默仍 open。详见[新代次双账号现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-新代次双账号-logininfo-现场)。

2026-11-14 下一只读工作块为目录与群成员查询。离线/真实 pipe 已通过：`onebot_query_lab` 10/10、`capability_query_lab` 13/13、wrapper 合同 6/6、`daemon_client_lab` 9/9，覆盖好友/群列表、群信息、完整群成员快照/指定成员、严格参数与 scope/重连错误。

2026-11-14 新代次目录现场已执行：A PID 21784（登记账号 A）/B PID 41548（登记账号 B）均通过 manifest、PE/backend/envscan，attach/owner shutdown 通过。A 好友/群列表均 HTTP 200 + retcode 1440；B 好友列表 4 项、群列表 11 项通过。B 实际群 本机登记群 的群详情和成员列表均 HTTP 200 + retcode 1440，未发起成员详情查询；不伪造空列表/成员数据、不发送消息。QQ 仍响应，各保留一份 bridge，daemon/HTTP 已关闭。下一步应先分析当前 1440 宿主响应形状并补固定版本合同/适配，不直接重试或放宽校验；A 目录、群详情/成员、双账号完整矩阵仍未通过。详见[目录现场记录](docs/execution/2026-11-13-snowcat-capability-integration-continued.md#2026-11-14-只读目录与群成员真实-qq-现场)。消息收发、换号/网络重连、媒体/SSO 和长期静默仍 open。

2026-11-14 R1b 离线恢复 LAB 已按当前工作树重跑通过：`cargo test -p caligo-core --test core_restart_query_lab --features research --locked --offline --target-dir target-harness` 为 2/2，覆盖 LoginInfo/SelfProfile；两个真实 caligod 进程之间使用稳定 token/pipe，旧请求没有 poll，owner 取消一次，相同数值 scope 下 fresh 查询成功。该证据仍是合成 HostAdapter，不是 QQ 原生 callback 或消息现场。R1b 的 QQ FIELD 仍需单独覆盖 core 重启交错、在途取消和迟到隔离合同；不得复用已完成样本 PID 24376/46664 进行重接或热卸载。持续 QQ 身份失效通知/全路径清权仍独立 open；feat-005/006 blocked 保持。

## 2026-11-13 PR #2 conflict integration（历史合并记录）

PR integration branch contains `main` snapshot `8191d68` (`55b11c5`) plus fail-closed corrections (`6154b6c`). The current main group-query implementation is retained; fake CPB IDs/sender mappings and the request-derived OneBot success ID were removed. `init.ps1 -Mode all` passes after this correction. GitNexus compare reports medium risk with two affected flows; its index also reports incomplete process recall, so absent graph paths are not proof of no call. No QQ field actions or product acceptance were performed; status remains NOT READY. The 2026-11-11 report is historical for its source commit and includes an integration caveat. For continuing Caligo work, the existing N4 objective below remains the active scope.
## Previous N4 Objective（历史上下文）

2026-11-13 已完成 SnowCat 研究对齐的首个工作块。`qq-entry --uid-map-profile-service --uid-map-uin <decimal> --uid-map-result <new-local-evidence.json>` 在原 owner pump 上运行固定 ProfileService 往返，已通过离线验证。[精确模块与验收记录](docs/execution/2026-10-11-n4-response-recovery.md#2026-11-13-n4-r3-snowcat-profileservice-离线实现)

N4-R2/R3 的离线实现与合成合同检查点已提交。最新 R2 LAB 修复让本地 pump 在 generation quarantine 完成后做一次诊断尾槽 drain，清空副本但不应用回执；定向回归与 `init.ps1 -Mode all` 通过。2026-11-07 固定-hash 续查沿 `D3355C` 第二 wait caller 得到 §7.92 有界 no-hit，并检查 strict-RTTI MSFService/MSFCoreService slot-0 teardown（§7.93）：两条选定 pair-release 路径都没有 guarded-global `0x67510A0` 直接引用，pair 也未绑定到 global `+8`；成功 wait 与 manager final owner 仍未知。

当前-hash G3 profile 已在 PID 32496 通过零发送 P1 swap/restore。随后用户指定新启动 PID 26132 做唯一 G3-P2 样本：manifest、实际模块 hash、单一 envscan 候选、FRIEND-B 摘要和序号门均通过；sequence 4 的 `CALIGO-K3-SEND-004` 已提交一次，CLI exit 0，`g3s_submitted/g3s_done` 成功，PID 仍响应且 bridge 仍映射。发送返回为异步；报告没有 `consumer_result`。源码中直接 G3 路径只登记 capture context，而 `consumer_result` 由 `drain_responses()` 写入，本次没有启动 resident worker，因此 callback 是否发生和 delivery 均未知。`wc_uninstall` 报告恢复两个 callsite；运行后核验二者为原始字节。完整脱敏证据见 execution ledger，原始报告仅在 ignored `local-evidence/n4-r2-callback-26132-20261107/`。R2 callback OS thread/final owner/wait/QQ-native stop-drain/quiescence 与 R3 provider/adapter、response UID→CPB/native ID 仍 open；feat-005/006 blocked，R4 未准入。

2026-11-08 已补上 direct G3 的 research-only callback 观察路径：提交返回后按本次 cookie polling 最多 5 秒，定向 drain 只回收对应 cookie，保留其它代次 ready slots；捕获只证明 callback 被观测，不证明业务成功或对端送达。research lib tests 88/88、default check 和隔离 research release builds 通过。新 release CLI/bridge hash 分别为 `27F3B5E89C5A6D97DCEFBDCF10B089FB378ABD5944DBEE3C03C391A323D4C572` / `E5D1C83A92D6B4A4B554AB45C8790359009EAB465894B1DF99EDA630C6041299`；该路径加入时尚未加载进 QQ，之后已在 PID 9872 完成现场样本，结果见下条。PID 26132 固定着旧 bridge；sequence 4 仍 unknown、不得重放。

用户随后反馈 FRIEND-B 接收端当次检查未显示 `CALIGO-K3-SEND-004`；检查时间未提供。将其记录为负向观察，不据此推断永久未送达；sequence 4 及 delivery 状态仍 unknown，禁止重发。详见 progress 与 N4 execution ledger。

2026-11-08 在本轮 fresh-root PID 9872（创建 `08:21:39 +08:00`）完成一次 G3-P2 sequence 5 样本。当前-hash manifest / loaded module hashes、唯一 envscan 候选、peer alias digest、sequence 4 前置状态及两个原始 wrapper callsites 均通过。唯一正文 `CALIGO-K3-SEND-005` 单次提交，CLI exit 0；cookie-scoped observer 匹配捕获 1 (`err_code=0`, `err_bytes=0`)；按实际 JSONL `kind` 字段找到一条 `consumer_result`，callback TID 3540、drain TID 27176，response UID 状态 unreadable。08:45:22 只读线程快照显示 TID 3540 仍在 PID 9872、StartTime `08:21:41 +08:00`、Wait/EventPairLow，drain TID 27176 已退出；线程语义/稳定 owner 仍未定。delivery 明确 unconfirmed，接收端显示反馈待用户。PID 9872 仍响应且映射新 observer bridge；两 callsites 事后读取仍为原始字节。不重发、不热卸载。脱敏摘要见 ignored `local-evidence/n4-r2-callback-20261108/p2-run-20261108T083056/`。

身份纠正：用户确认 PID 9872 登录 FRIEND-B，因此 sequence 5 实为 FRIEND-B→FRIEND-B 自发送；callback 观察保留，但不可用于对端可见性判断，停止等待 FRIEND-B 的该 marker 回报，不重放。用户随后指定 PID 22704 为 TEST-ACCOUNT-A；sequence 6 / `CALIGO-K3-SEND-006` 在 G3 adopt anchor check 失败，未到 `g3s_submitted`、未提交 native message。sequence 6 已消耗。此次源码修正只让拒绝报告显示 wrapper-relative 实际与期望 vtable RVA；拒绝条件和返回不变。PID 22704 与 9872 都仍映射 bridge，不复用、不热卸载。

2026-11-08 sequence 7 后查：用户确认重启后的 PID/账号；进程清单中唯一新启动且响应正常的 QQ 主窗口 root 为 PID 33236（`11:59:17 +08:00`），本次脚本绑定 TEST-ACCOUNT-A→FRIEND-B。pilot manifest、四个 loaded role hash、单一 envscan 候选与原始 callsites 通过。一次 `CALIGO-K3-SEND-007` 在 `g3s_adopt` anchor gate 失败，没有 `g3s_submitted`/`g3s_done`，无 native message submission；sequence 7 消耗、不复用。精确 wrapper RTTI 确认 primary `MSFService@nt` 表 slots 0–30 共 31 项；slot31 是 offset-8 次级表的 COL。运行时 clone 的 31 个主表槽中 30 个直接匹配，slot17 经临时模块 thunk 转到 profile 原目标。只读 initializer 快照显示 guard state=0，initializer 指针位于 loaded-module 范围之外的 committed private executable 页，128-byte prefix 已本机留存、未调用。随后 PE 导出审计发现 `slrF54B.tmp` 的导出身份是 `snowluma_windows_resident_dispatch.dll` / `nnp_resident_dispatch_win`；slot17 因此确定由第三方代码触及。runner 用文件名过滤而漏掉 `.tmp`，preflight 的 `backend_already_loaded=false` 不能证明 clean backend；该模块 attach 前是否已加载未被保存。下溢日志、RTTI 和 initializer 细节见 progress 与 execution ledger。PID 33236 仍响应，Caligo bridge 与 Snowluma module 仍映射，不再调用、不重注入、不热卸载；PID 26132、9872、22704 已退出。

## Previous Restartable Next Step（由上方整合顺序替代）

以下是 N4 当时交接；当前先完成 A0–A4 离线整合。R3 当时优先核验 ProfileService。现场入口要求用户明确指定新启动的测试账号实例，并重新核对 PID/创建时间、PE backend 身份、pilot manifest、加载模块 hash 和唯一 envscan。随后仅运行一次只读往返，查看 `uid_map_profile_service_ready`、`api_call_attempted` 与 `roundtrip_match`。真实结果写入本机 `local-evidence/`。往返通过后仍需独立验证 response UID → CPB/native ID，才有依据接入常驻私聊和消息投影。

`CALIGO-K1-PROBE-001` 与 `CALIGO-K3-SEND-002` 保持不重放；sequence 3 unknown。sequence 4 / `CALIGO-K3-SEND-004` 已提交一次，FRIEND-B 当次检查未显示，delivery unknown，不重发。sequence 5 / `CALIGO-K3-SEND-005` 是 FRIEND-B 自发送；callback TID 3540、drain TID 27176、response UID unreadable，不能证明 peer visibility，不重放。sequence 6 / `CALIGO-K3-SEND-006` 与 sequence 7 / `CALIGO-K3-SEND-007` 都在 `g3s_adopt` anchor gate 停止，没有 native submit；两序号均消耗。Sequence 8 / `CALIGO-K3-SEND-008` 在新根 PID 38072 上提交一次并观测到 callback (`err_code=0`)，callback TID 29124、drain TID 12244、response UID unreadable；FRIEND-B 用户随后检查称未显示，delivery unknown，不重放。PID 38072 仍响应、Caligo bridge 仍映射，不重用、不热卸载。Backend audit correction: sequence4–7 runner scripts only filtered module filenames, so backend absence is unverified for all four samples; PID 33236 had the Snowluma export identity and is excluded from further field calls. PIDs 26132, 9872, and 22704 have exited; their retained reports do not include complete module inventories.

PE-export-aware helper SHA-256 `2238D736345ECF41E93F626439BE97832892E67B5A9084B88C6DB4621B6855D4` 在 PID 38072 的 attach 前两次均 PASS；attach 后审计只拒绝已加载的 Caligo bridge，没有发现 Snowluma 或未知临时模块。sequence 8 的 preflight/执行/回读材料位于 ignored `local-evidence/n4-r2-callback-20261108/p2-run-20261108T131506/`，包括两次 backend audit、manifest/hash、envscan、callsite 与 consumer report。只读 `NtQueryInformationThread` 已把 callback TID 29124 的 start routine 映射到精确 wrapper `+0xD7DDCF`（SHA-256 `0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C`）；固定-hash `.pdata` 与反汇编表明它是接收线程参数、调用局部 callable pair 的 wrapper trampoline，线程早于 attach 存在。固定-hash wrapper 分析已把 helper +0xD7DD30 绑定到 KERNEL32!CreateThread，并找到两个直接 caller 候选（+0xCBC335→callback +0xCBC9D2；+0xD7BF8C→+0xD7C1F8→+0xD7C41E）。二者共享 +0xD7DDCF trampoline；未读取 TID 29124 的 runtime closure/context，故不能归因到任一 callback，也未证明它属于 response consumer。下一步将两个候选与已映射的 manager/consumer 静态合同交叉验证；owner/wait/quiescence 与 R3 response UID unreadable→CPB/native ID 强关联仍 open。PID 38072 的 bridge 按进程生命周期保留，不复用、不重注入、不热卸载。sequence 8 已消耗且不重放；任何新现场调用都须新鲜明确指定的 TEST-ACCOUNT-A root、重新通过 PE audit/hash/envscan/callsite 门，并使用 sequence 9。R2/R3 仍 open，R4 未准入；运行时证据见 [N4 execution ledger](docs/execution/2026-10-11-n4-response-recovery.md)。

原 sequence-4 报告没有 `consumer_result`，不能回溯证明 callback 是否发生；新增 observer 只改善未来 direct G3 样本。它不证明 QQ callback 的真实 owner/thread contract、manager final owner、successful wait 或 QQ stop/drain/quiescence。R3 resolver provider/request adapter/result 与 response UID→CPB/native ID 仍 open；不猜 `0x9c7a` 语义。R1–R3 与 ABI/线程/寿命门通过前，R4 不准入。

## Preserved Unknowns

- PID 38356 与新指定 PID 31156 的 `getGroupService()` 均返回空；选中的 Group UID provider、`{uinList:[value]}` 到 N-API array 的 runtime adapter 及 API 响应仍未知，resolver 未派发。
- 实际 UID/UIN 值到 CPB/native identity 的强关联。
- SendMsgResult 条件 CPB slot `+0x158` / key `0x9c7a` 的具体接收类型、字段语义和与 response UID 的对象身份关系。
- QQ callback 的现场 OS thread、final owner、成功 wait、stop/drain/quiescence。
- 旧 dump 本身仍没有 V8 sanity 返回值，且未包含 TryCatch 构造函数或 bridge 调用页。独立的新运行已证明修复版 bridge 可以完成 V8 sanity 与 scope 清理，但不改变旧 dump 的证据内容。
- 本轮 G3-P1 仅临时替换 transport slot 7 并成功恢复，捕获计数 1；没有注册 listener、调用 SendSSORequestInternal 或执行 send-response callback。发送阶段未能进入，R2 callback 的现场 OS thread/final owner/wait/quiescence 与 R3 native GroupService resolver 仍未知。
- PID 26132 的 G3-P2 单次调用已提交并返回 async，sequence 4 不重放。`g3-send.jsonl` 只有 G3 阶段行，没有 `consumer_result` / worker summary；调用没有配置 native-resident worker，故无法从报告确定 callback 是否执行。进程仍响应，bridge 已 pin 到进程生命周期；callsite uninstall 报告恢复 2 处，后读两处都等于原始字节。记录见 ignored `local-evidence/n4-r2-callback-26132-20261107/`。
- FRIEND-B 接收端后来反馈当次检查未显示 `CALIGO-K3-SEND-004`，但没有提供检查时间；该负向观察不能证明永久未送达，故 delivery 保持 unknown，sequence 4 不重放。
- Fresh-root PID 9872 的 G3-P2 sequence-5 report 有一条 cookie-matched callback observation (`matches=1`, `err_code=0`, `err_bytes=0`) 与一条 `kind=consumer_result` (`callback_tid=3540`, `drain_tid=27176`)；response UID unreadable。事后线程快照中 callback TID 3540 仍在 PID 9872、StartTime `08:21:41 +08:00`、Wait/EventPairLow；drain TID 已退出。这只给出一个现场 OS TID 样本，不确定稳定线程语义/owner；successful wait / QQ quiescence 仍未知。用户后来确认 PID 9872 的账号也是 FRIEND-B，故 sequence 5 是自发送样本，不能证明对端可见；不等待接收反馈、不重放。PID 9872 仍映射 observer bridge，不可复用。
- 用户指定 PID 22704 为 TEST-ACCOUNT-A 后，G3-P2 sequence 6 / `CALIGO-K3-SEND-006` 在 getter 返回的 service vtable anchor gate 失败 (`g3s_adopt: anchor mismatch`)，没有 `g3s_submitted` 或 `g3s_done`，故 native message 未提交；sequence 6 已消耗。当前报告没有 actual/expected RVA，不能判断是 profile anchor 还是实例对象差异。源码 `g3_send_run_with_profile` 与 `g3_probe_run_with_profile` 的拒绝分支现输出 wrapper-relative `actual_vtbl_rva` / `expected_vtbl_rva`，仍返回 `ERR_SWAP_STATE`。后续诊断只能对用户指定的新鲜 TEST-ACCOUNT-A root 执行；PID 22704 与 9872 均已 pin bridge，不得复用、再次注入或热卸载。原始运行材料位于 ignored `local-evidence/n4-r2-callback-20261108/p2-run-20261108T091434/`。

## Scope and Validation

- 本轮动态核验使用两次隔离 offline release build：`cargo build -p caligo-cli --release --features research --locked --offline --target-dir target-harness/n4-r2-r3-dynamic-20261107` 与 `cargo build -p caligo-bridge --release --features research --locked --offline --target-dir target-harness/n4-r2-r3-dynamic-20261107` 均 PASS；bridge SHA-256 为 `0F652675B02365811DC4350BA5004064A4DB31FC65140890A3055DA387FA903C`。当前 `init.ps1 -Mode docs` 与 `git diff --check` 结果见本轮执行记录。
- 前一检查点 `a33780f` 的 `init.ps1 -Mode all`、default/research workspace tests、定向 UID-map script tests 2/2、`rustfmt --check` 与 research release build 均已通过。本轮另外完成 CLI/bridge 隔离 release build 和一次 runtime pilot；未重跑 Cargo tests。
- 本机 9.9.36-53644 临时 pilot manifest、当前 PID 的 envscan、阶段报告、UID-map 结果和脱敏摘要仅在 ignored `local-evidence/n4-r2-r3-closure-20261107/`；正式 adapter manifest 未改。CALIGO-K1-PROBE-002 未发送消息、不注册 listener、不热卸载、不重试；owner pump 已关闭，bridge 继续映射直至 QQ 退出。
- 2026-11-07 surface-only code checkpoint: `cargo test -p caligo-bridge --lib --features research uid_map_script_tests --locked --offline --target-dir target-harness/n4-r2-r3-surface-only-20261107` passed 3/3; isolated release builds of CLI and bridge passed. PID 31156 matched the local pilot manifest and unique envscan candidate; the 18-stage run returned `group_service_missing`, `api_call_attempted=false`, no resolver stages, and a successful owner shutdown. QQ remained responsive; bridge is still mapped until normal process exit. Console, stage/result JSON and hashes are in the N4 execution record and ignored run directory. Full workspace rustfmt check is not clean because of pre-existing differences outside these edits; no bulk formatting was applied.
- 2026-11-07 current-hash G3 profile/P1 checkpoint: isolated `cargo build -p caligo-cli --release --features research --locked --offline --target-dir target-harness/n4-g3-profile-20261107` passed. CLI SHA-256 `471D9768A830938F0B03885630ABF5480E076EDCFEEB0DF34EB5EF2586C3C2E0`; bridge SHA-256 `CF55282D8E83F52891F4AA46ED136088B48987FB2F8EEA11D77ADDCB3740665C`. PID 32496 exact loaded hashes matched the local `9.9.36-53644` pilot manifest. P1 `g3_adopt/site/swap/restore/done` passed and the target responded at the immediate post-check; the target was gone before the send gate, with no matching Application 1000/1001. No message was dispatched and sequence 4 was not reserved. Raw reports remain in ignored `local-evidence/n4-r2-callback-32496-20261107/`; R2 and R3 stay open.
- After that runtime sample, mode validation was hardened and stop/status now reject the new surface-only flag. The final source passed the same 3/3 bridge script tests, default CLI build, research CLI release build, and research bridge release build in `target-harness/n4-r2-r3-surface-only-final-20261107`; final bridge hash is `A7EA8E9C3FC2206A679337D995DA8902B0695EAF47F4BACC200A73D16AB0BA75`. This build was not loaded into QQ. PID 31156 retains the earlier `B8D683…A621F33` bridge, so do not rebuild over its mapped DLL, reattach, or hot-unload it.
- 前一静态审计的 schema-2 计划快照在 39 个 cited paths 中有 35 条记录相对旧 pin 不同；plan digest 未变，细节见 execution ledger。本轮 GitNexus 1.6.12 `list` 将 Caligo 绑定到 HEAD `cc716b8`；`uid_map_probe::run` 与 `pump_cb` context 经源码和现场阶段报告复核。执行记录新增后的文档没有重建索引。
- 2026-11-07 second-wait owner audit: `wrapper.node` hash `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`; six complete `.pdata` bodies decoded. The `D3355C` wait result is unchecked; the `0x5B060` valid-`.pdata` direct E8 candidate count, strict direct-vtable count, and selected-function RIP refs to `0x67510A0` are all zero. Verifier/report SHA-256 `440AAC1398F88F718F2A47FBA32DD515A8CCDF1DB0329DF8A7129363082086F2` / `91E38400546BC43E81997478BEB756FD78AC68326D53BC0A143A9B8DD7174BDF`; companion RTTI verifier/report `40EEF3E00D964B3F8E729ACD4C8352E90B756236B78D803B7B3D1DEB4658EB49` / `17870B145CAFCB7EF90AF0ED803E9A82F0ABE5AB7F3836BFBF54BABD2278C872`. All artifacts remain under ignored `local-evidence/n4-r2-readonly-20261015/`. Docs validation passed (10 live docs / 202 local links / 33 historical entries / 10 features), JSON and whitespace checks passed. GitNexus CLI 1.6.12 index-only refresh is current at `cb50d79` across 302 files; staged detect covered five files, mapped Section nodes only, reported zero processes/low risk with `partial=false` / `truncated=false`. The full structured result is saved as `local-evidence/n4-r2-readonly-20261015/gitnexus-detect-staged-wait-owner-20261107.json`. This was a no-PDG documentation refresh; source/process graph completeness was not claimed.
- feat-005/006 remain blocked，R4 未准入，产品 NOT READY。本次动态证据与状态文档提交不提升产品能力状态；工作区中的 harness、日志、缓存及其他无关改动保持未纳入提交。
- 2026-11-08 fresh-root G3-P2 sequence 5 used the previously built research CLI/bridge hashes `27F3B5E89C5A6D97DCEFBDCF10B089FB378ABD5944DBEE3C03C391A323D4C572` / `E5D1C83A92D6B4A4B554AB45C8790359009EAB465894B1DF99EDA630C6041299`. `init.ps1 -Mode docs` and post-update diff/GitNexus results are recorded with the resulting docs checkpoint. Raw preflight/envscan/G3/post-callsite files remain only in ignored `local-evidence/n4-r2-callback-20261108/p2-run-20261108T083056/`; no local evidence was staged.
- 2026-11-08 clean-gated G3-P2 sequence 8 on PID 38072: two pre-reservation PE audits passed, manifest/loaded-role hashes and unique envscan passed, and both callsites read back original after the run. One async submit reached `g3s_submitted`; one cookie-matched result had `err_code=0`, callback TID 29124 / drain TID 12244, response UID unreadable. FRIEND-B reported the marker was not displayed; delivery remains unknown and sequence 8 is consumed/no-replay. The root remains responsive with bridge mapped; no hot-unload. `init.ps1 -Mode docs` PASS (10 live docs / 207 local links / 33 historical entries / 10 features), `git diff --check` and `feature_list.json` JSON parse PASS; no Cargo tests run. Full record: [N4 execution ledger](docs/execution/2026-10-11-n4-response-recovery.md), raw reports remain only in ignored `local-evidence/n4-r2-callback-20261108/p2-run-20261108T131506/`.
