# OneBot 群与好友查询动作

适用：修改 `get_group_list`、`get_group_info`、`get_friend_list` 或
`StoreReader` 的读取路径。
依据：[onebot.rs](../../crates/caligo-core/src/onebot.rs)、
[onebot_traits.rs](../../crates/caligo-model/src/onebot_traits.rs)、
[message_store.rs](../../crates/caligo-core/src/message_store.rs)。

## 2026-11-13 查询语义更正

[整合计划](../plans/2026-11-13-gitnexus-plan-snowcat-capability-integration.md) §6–§7
要求实际 handler/backend/正常入口，空列表与占位不计入已接线。
主线 PR #3 曾将 learned peers 包装为目录、为任意群号拼接名称和零计数，
并将无好友来源表示为成功空数组。本次整合替换这些公开返回语义：

- `get_group_list` 和 `get_friend_list` 返回 `failed` / `retcode:1404` /
  `data:null`，保留 echo。
- `get_group_info` 缺少或无效 `group_id` 返回参数错误；合法输入仍明确未支持。
- 三项不进入当前已实现的 `ACTION_REGISTRY`，不会被工具发现声明为可用能力。
- 未接 Caligo 目录 backend 不能推断官方 QQ 不存在目录接口。

这更正了本页此前的占位响应描述；历史 PR 与研究记录保留其当时观察。
后续接入真实目录服务时，应一起更新正常入口、schema、权限和验收证据。

## 已保留的内部索引

`StoreReader::group_peers()` 从**已入库群消息**学习 peer，按 peer 去重并排序。
它表达曾观察到的群会话，不表达完整群目录、好友关系、群名或成员数量。

内部链：`MessageStore`（仅 `ChatType::Group`，`BTreeSet` 去重）
→ `AccountActor` → `Daemon`。没有 MessageStore 的 reader 保留默认空索引。
内部空索引不用于公开目录动作的成功响应。

## 验证与维护

- OneBot 动作测试核对未支持响应、坏参数、echo 和无假数据。
- `message_store::tests::group_peers_are_learned_from_group_messages_only`
  继续覆盖“只学群消息”和按 peer 去重。
- 新目录 backend 需明确区分未就绪、无权限、未知记录和真实空结果；
  参数校验与投影层不补造平台字段。
