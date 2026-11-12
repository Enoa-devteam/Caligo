# OneBot 群与好友查询动作

适用：修改 `get_group_list`、`get_group_info`、`get_friend_list` 或
`StoreReader` 的读取路径。
依据：[onebot.rs](../../crates/caligo-core/src/onebot.rs)、
[onebot_traits.rs](../../crates/caligo-model/src/onebot_traits.rs)、
[message_store.rs](../../crates/caligo-core/src/message_store.rs)。
本页只描述当前实现与边界；平台未提供的字段不在此处补默认值。

## 数据来源与边界

- 平台没有给“群列表/好友列表”接口。`group_peers` 从**已入库的群消息**学习群号，
  按 peer 去重并排序；未观察到的群不会出现。
- 群名不编造：统一返回 `群 <群号>` 占位，`member_count` 与 `max_member_count`
  固定为 0，直到平台提供真实来源。
- `get_friend_list` 没有好友数据源，显式返回空数组；不得用群成员冒充好友。
- 三个动作不改变身份投影、消息落库或发送回执；真实群名、成员表与好友列表
  属于后续工作块。

## 动作

### get_group_list

响应 `data` 为数组，每项：

```json
{
  "group_id": 1102382705,
  "group_name": "群 1102382705",
  "member_count": 0,
  "max_member_count": 0
}
```

只输出能解析为数字的 peer；学习到的非数字 peer 被忽略，不生成 `group_id:0`。

### get_group_info

参数 `group_id`（数字）。响应回显该 id，群名仍为 `群 <id>` 占位，计数为 0。
缺少参数时 `group_id` 回落为 0，不做额外猜测。

### get_friend_list

响应 `data` 为 `[]`；当前没有好友数据源。

## 实现链

`StoreReader::group_peers()`（默认空）
→ `MessageStore`（遍历已持久化消息，仅 `ChatType::Group`，`BTreeSet` 去重）
→ `AccountActor` 转发 → `Daemon` 转发 → `onebot::handle_action`。

四层缺一不可：任一层未实现都会回落到 trait 的默认空实现，`get_group_list`
会返回 `[]`。新增读取源时同时更新 trait、存储实现、两处转发和本页。

## 测试与维护

- `onebot::tests::group_queries_use_learned_peers_without_inventing_names`
  覆盖动作白名单与三个响应形状。
- `message_store::tests::group_peers_are_learned_from_group_messages_only`
  覆盖“只学群消息”和按 peer 去重。
- 平台提供真实群名或好友来源后，扩展 `StoreReader` 并同步本页；
  不要在校验/投影层猜名字或补默认计数。
