# OneBot 群与好友查询动作

适用：修改 `get_group_list`、`get_group_info`、群成员、`get_friend_list`、登录资料或宿主查询读取。
依据：[动作层](../../crates/caligo-core/src/onebot.rs)、
[正常装配](../../crates/caligo-core/src/onebot_server.rs)、
[查询类型](../../crates/caligo-model/src/capability.rs)、
[宿主执行器](../../crates/caligo-bridge/src/qq_query.rs)。

## 当前正常入口

目录与群成员动作已纳入共享 `ACTION_REGISTRY`，HTTP、WS、MCP 使用相同
`QueryBackend → Daemon::query_host → typed IPC → owner query`。
`get_group_info` 要求正整数 `group_id`；可选 `no_cache` 必须为布尔值。
当前每次实际查询宿主，不以 learned peers 充当目录缓存。
在精确 9.9.36-53644 profile 上，`get_group_list` 保留 QQ 分别报告的当前人数和容量，即使人数超过非零容量。群详情仍校验人数与容量的关系。
群详情联合 `getGroupDetailInfo` 的受理结果与匹配 `onGroupDetailInfoChange` 通知，
每请求注册/移除 listener；取消、超时或清理未知时拒绝后续详情调用，待新会话重新准入。
精确 9.9.36-53644 profile 的注册返回正 `u64` 十进制字符串，移除时原样传回。
研究构建失败诊断仅在本机 entry 报告保存固定原因、字段类型和受限计数，
不写原始身份、字段值或异常文本；公开 OneBot 错误语义不变。

`get_group_member_list` 和 `get_group_member_info` 使用精确 pilot profile 的完整
`getAllMemberList` 快照。参数要求正整数 group_id，指定成员另需正整数 user_id；
no_cache 为布尔值，列表默认 false，指定成员默认 true。未完成快照、重复身份、
超出 2,048 项或 512 KiB 均拒绝；指定成员不存在时失败。当前不额外抓取 rich profile，
UID、头像路径和私有字段不跨 IPC；详情按快照实际字段返回。
精确 pilot 的非负安全整数时间戳以十进制字符串进入 typed 快照。

查询结果必须匹配账号、宿主 nonce、会话代次及连接 epoch，且由当前 bridge
显式声明支持。Map 按原键取值，UID 和公开 UIN 分开；消息回读按精确 peer/ID。
查询有期限、容量及响应大小上限，不自动重试。旧 bridge 没有能力声明时返回未支持。

- 真实空列表返回成功空数组。
- 未实现、未就绪、无权限、业务拒绝、超时/断链和响应不合合同分别失败。
- 无宿主 backend 时不返回占位群名、零成员数或空好友列表假成功。
- `get_login_info` 由精确 pilot 的 owner adapter 提供。每次查询重新读取
  `nt_1` 的当前身份通知和同一 UID 的昵称，并检查账号、宿主 nonce、会话代次、
  连接 epoch 与 wrapper session ID。配置账号仅用于比对。离线、身份变化、
  无匹配昵称及断线均无法产生成功结果。

`get_login_info` 可选扩展参数 `no_cache` 必须为布尔值；缺省/false 走 `LoginInfo`，
true 仅走独立 `SelfProfile` 能力。刷新目标固定为已验证的当前账号，无任意用户参数。
当前实现在同一 owner/session 下单次请求 KSERVER，联合核对每请求回复与匹配通知；
公开输出仍只有 user_id/nickname，使用刷新回复的昵称，不回退到缓存登录资料。
未声明 SelfProfile、超时、撤权或清理未知均失败；旧请求迟到不能满足新请求。
通知没有请求 ID，source 标记也不独立证明服务器新鲜度。正常入口现场验收另记，
不能从研究探针直接推定通过，也不开放任意 `get_stranger_info`。

固定参考的目录服务合同与 Caligo 离线验证见
[后续执行记录](../execution/2026-11-13-snowcat-capability-integration-continued.md)。
这说明源码接线与离线合同，不能替代特定 QQ build 的现场验收。

## 撤权与重连边界

管道断开/重连清权与 QQ 自身掉线/换号分开验收。当前查询的 Profile listener 是临时的，
不能证明空闲期及目录/收发的全局身份监控。daemon 重启后 epoch 数值可能重复，
查询 ID 另以 core nonce 隔离；resident 在撤权发布时不可逆标记旧 pending，
同 scope 重绑也必须在合法 owner 取消旧查询，Disconnected 只报告一次。
源码、现有证据和现场顺序见[撤权验收合同](../acceptance/2026-11-14-session-revocation.md)。

## 已保留的内部索引

`StoreReader::group_peers()` 从已入库群消息学习 peer，按 peer 去重并排序。
它表达曾观察到的群会话，不表达完整目录、好友关系、群名或成员数量。
内部链为 MessageStore → AccountActor → Daemon，独立于宿主目录查询。

## 历史语义更正

主线 PR #3 曾将 learned peers 包装成目录、为任意群号拼接名称与零计数，
并将无好友来源表示为成功空数组。首批整合先改为明确拒绝；本批增加真实
查询 backend 后才注册目录 handler。两个阶段的历史记录保留。
[整合计划](../plans/2026-11-13-gitnexus-plan-snowcat-capability-integration.md)
要求实际 handler、backend 和正常入口，注册或占位本身不计能力完成。

新增目录或资料能力时，同步强类型参数/结果、权限、精确版本与能力声明，
并覆盖正常 pipe → action handler、错误 scope、真实空与失败、超时和恢复。
