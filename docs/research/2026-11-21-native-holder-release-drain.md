# 生产 JS 链路的 native 持有者、最终释放与全局 drain（2026-11-21 复核）

本文回答 feat-004 遗留、由 feat-005 承接的三个问题：**谁持有 native/JS 对象**、**它们何时最终释放**、
**停止是否等到全局 drain**。范围限定正式链路 `qq_entry → qq_v8`（D8 脚本 + owner pump + resident）；
历史 native MSF/G3/consumer 路线的静态合同见 [qq-native-lifetime-contract.md](qq-native-lifetime-contract.md)，
两者不可混用。本文所有结论为离线代码/夹具证据，**不是**对 QQ 内部 GC 或 dispatcher 的观测。

## 1. 持有者清单（生产链路）

| 对象 | 持有者 | 代码证据 | 停止时是否释放 |
|---|---|---|---|
| JS 常驻对象 `R`（含 `session`/`ms`/`listener`/`sends`/`sendResults`/`callbackObservations`） | 包装器全局 `__caligo_d8` | 启动时同时写 `__caligo_res` 与 `__caligo_d8`（`qq_v8.rs:797-798`）；停止只 `delete __caligo_res`（`:970`），保留 `__caligo_d8` | **否**（刻意保留给停止快照/迟到观察，`:980-989`） |
| 同上 | 在途发送的 Promise/回读闭包 | `build_send_js` 在发送时取 `G.__caligo_res`，闭包继续引用 `R`（`qq_v8.rs:3096+`） | **否**：停止后 resolve 旧 promise 仍会追加 `phase=stopped` 行（`wrapper_callback_observation.rs` 迟到用例） |
| QQ 监听器对象 | `R.listener` + `R.ms` 的注册 | `addKernelMsgListener`（`:796`）；停止调用 `removeKernelMsgListener`（`:963`） | 注册关系**是**（返回值检查，失败即 0x23 保根）；对象本身仍被 `R` 引用 |
| QQ wrapper session / message service | QQ 模块自身；`R.session`/`R.ms` 只是引用 | `getNTWrapperSession`/`getMsgService`（`:378-394`） | 否；Caligo 无释放 API |
| V8 isolate / environment | QQ 进程；Rust 仅在使用期借用指针 | `isolate_addr()`、`ENTRY.env`（`qq_v8.rs` 顶部/`stop_driver.rs:147-149`） | 否；Caligo 不拥有、不释放 |
| 未 ACK 的结果通知 | bridge resident（256 槽 + handoff gate） | `resident.has_unacked_results`、`close_for_result_drain`、`ResultHandoffGate` | 仅在全部 ACK 后释放；否则保留到宿主退出（见 §3） |
| 请求 journal / 非终态请求 | core（磁盘 journal + 内存状态） | 恢复不重派；迟到结果至多完成一次（§22） | 否；按恢复合同保留 |
| bridge 映像 | QQ 进程 | 注入后不卸载；无 `FreeLibrary` 路径 | 否；仅宿主退出 |

关键点：`__caligo_d8` 与在途 promise 闭包是**两个独立根**。本轮新增夹具（`wrapper_callback_observation.rs`）
先删除 VM 全局 `__caligo_d8`，停止快照读取立即变 `UNAVAILABLE`；再 resolve 旧 promise，仍追加
`phase=stopped` 行——证明闭包本身持有 `R`，清全局不足以释放。

## 2. 最终释放

停止路径释放的东西有限：

1. 监听器注册（`removeKernelMsgListener`，失败即 `0x23` 并保留根）；
2. 全局 `__caligo_res`（`delete`），使轮询/发送脚本拒绝新工作；
3. owner pump（`StopListener → Observe → ClosePump`，`close_permitted` 后关闭 libuv；`stop_driver.rs:139-167`）；
4. 结果通知在 `drain_results` 模式下等 ACK 后交还（§3）。

**没有释放**：`__caligo_d8` 根、在途 promise 闭包、`R` 持有的 session/ms/listener 引用、isolate/environment、
bridge 映像。也就是说，停止后 `R` 仍强可达；即使监听器已移除，旧 promise/闭包仍能让回调向 `R` 追加行。
生产代码没有清除 `__caligo_d8` 的路径——它正是停止观察读取器依赖的根。因此“最终释放”最早只能发生在
**宿主进程退出**（V8 全局与闭包一并消失）或未来引入显式释放 API；在此之前热卸载 bridge 无法证明安全，
与“未证寿命不卸载”一致。

## 3. 全局 drain：三种不同含义

| 层级 | 是否等待 | 证据 |
|---|---|---|
| Caligo 结果 drain | `--stop-observe-ms` 走 `arm_stop_observation_with_result_drain`（`lib.rs` 导出 `caligo_entry_stop_observe`），close 时 `resident.close_for_result_drain()`；`retain_daemon_for_result_drain` 等 `worker_finished && !has_unacked_results`，转发保留通知、按精确 ACK 释放槽（`qq_entry.rs:268-279`、`:302-323`） | 未 ACK 时 `await_shutdown` 失败，CLI 明确提示“**不代表 QQ 原生 drain**，勿重发或卸载 bridge”（`qqentry.rs:1450`） |
| Caligo owner drain | 是：CLOSE_REQ → owner pump `uv_close` → shutdown 等待 worker 退出 | 现场多次 `closed via owner pump`；`stop_driver.rs:164-167`、`:190-211` |
| QQ/native dispatcher drain | **否**：无 stop/cancel/join 合同 | 历史合同 §7.1/§7 明确未闭合；本链路停止后旧回调仍可进入 JS（迟到用例） |

所以“全局 drain”只有在“Caligo 自有结果 + owner pump”这一层是成立的、且**有界**（`wait_ms` 超时即失败并保留）；
QQ 的 dispatcher/worker 是否静止、是否还有排队回调，没有任何证据。§22 的迟到结果在新 core 完成一次，
进一步说明 bridge resident 会跨 core 死亡保留并重放结果——这是 Caligo 的 retention/replay，不是 QQ 全局静默。

## 4. 仍开放与关闭条件

1. **QQ 侧持有者**：`session`/`ms`/isolate/environment 的真实引用计数、GC 时机与线程归属，无法只靠静态/夹具证明；
   需要 QQ 侧调试符号或受支持的释放 API。
2. **`R` 的最终释放**：需要证明 `__caligo_d8`、所有 promise 闭包、QQ listener 引用全部消失；当前设计刻意保留
   `__caligo_d8` 供停止观察，故在宿主退出前不可达。
3. **全局 drain**：需要 QQ dispatcher 的 stop/cancel/join 合同或运行时静默观测；现有证据只能支持“Caligo 结果
   已 ACK + owner pump 已关”，不能支持“QQ 不再回调”。

结论：三项均**未闭合**，但边界已明确并可复核：持有者是 `__caligo_d8` 与在途闭包（双根）、最终释放延后到宿主
退出、drain 仅覆盖 Caligo 自有结果与 owner pump。该结论不改变 feat-005/native 合同状态，也不允许热卸载。
