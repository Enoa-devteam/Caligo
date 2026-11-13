# OneBot 与 MCP 应用入口

适用：修改 `caligod` 的协议装配、动作发现或事件传输。
依据：[启动入口](../../crates/caligo-core/src/bin/caligod.rs)、
[真实动作注册表](../../crates/caligo-core/src/onebot.rs)、
[SSE](../../crates/caligo-core/src/onebot_sse.rs)、
[MCP](../../crates/caligo-core/src/onebot_mcp.rs)。

## 启动与共享语义

`caligod` 保持单账号 daemon。HTTP 默认绑定 `127.0.0.1:5700`；
可选 `--onebot-sse-bind 127.0.0.1:6701` 与
`--onebot-mcp-bind 127.0.0.1:6702` 分别启用两个独立 loopback listener。
端口 `0` 可由系统分配，实际地址以启动首行 JSON 的 `onebot_sse` / `onebot_mcp` 为准。

所有监听复用 daemon token，客户端发送 `Authorization: Bearer <token>`。
启动 JSON 含本机凭据，应由启动方私下读取，不能加入诊断或公开日志。
control Stop 和 Ctrl+C 使用同一停止标志，进程等待协议工作线程退出。

HTTP、WS 和 MCP 调用同一动作层。`ACTION_REGISTRY` 同时驱动支持判断、
handler 分派和工具发现；注册元数据不证明 native backend 已就绪，
读写标签不替代鉴权。只有强回执和持久 OneBot ID 同时具备才返回标准发送成功。
超时、unknown 与未支持保持原有结果，客户端不能自动重发未知请求。

## SSE

`GET /_events` 返回 `text/event-stream`，事件为 `data: <JSON>\n\n`，
复用 HTTP POST quick operation 之后的 general/WS fanout。
每个客户端独立有界队列；队列丢失时断开，并在统计中记录 overflow/dropped。
写入有总期限，空闲时发送注释心跳。请求头、事件大小和客户端数均有上限。

此入口不提供断点重放，拒绝 `Last-Event-ID`，不生成可能被误认为重放游标的 id。
断开后重新订阅仅接收后续事件；持久消息查询仍通过动作层和 MessageStore。

## MCP

`POST /mcp` 是无状态 Streamable HTTP JSON-RPC 入口。
支持 `2025-03-26` 与 `2025-06-18`；无版本头按 `2025-03-26` 处理。
`initialize`、`ping`、`tools/list`、`tools/call` 使用对应消息合同。
客户端声明 `Accept: application/json, text/event-stream`；提供 Origin 时须为
loopback 来源，提供不支持的 `MCP-Protocol-Version` 时明确失败。

工具名称直接使用注册表动作名，包含真实 async alias；参数 schema 来自同一注册表。
工具结果中的文本为完整 OneBot 响应 JSON，业务错误保留 retcode/status，
异步受理仍不表示消息已确认。未注册目录、媒体或原始包工具不会被发现列表声明支持。

该入口不建立 MCP session，不提供 MCP SSE stream 或自动恢复；
OneBot SSE 是独立的业务事件端点。协议层测试与原生 QQ 验收分别记录于
[整合执行记录](../execution/2026-11-13-snowcat-capability-integration.md)。
