# AgentJax Daemon API / IPC Schema
## 1. 目标
本文档定义 AgentJax daemon 对外 API 的协议契约层。

这份文档不再讨论：
- daemon/client 为什么分离
- surface/channel/transport 为什么要拆
- unix socket 和 websocket 为什么共用 schema

这些上层原则由：
- `docs/CHANNELS_DAEMON_CLIENT_SPEC.md`

本文档只负责把协议本身钉死：
- envelope
- request / response
- event
- stream
- subscription
- error model
- method namespace
- 最低 payload schema

一句话：
**Channels / Daemon / Client Spec 讲架构边界；Daemon API / IPC Schema 讲线上的包到底长什么样。**

---
## 2. 设计原则
### 2.1 单一协议，多 transport 复用
以下 transport 必须共享同一套 schema：
- `Unix socket`
- `WebSocket`

后续若扩展：
- `HTTP`
- `TCP`

也应优先复用相同方法名、错误模型、事件模型。

### 2.2 Envelope First
所有消息都必须进入统一 envelope，不允许“某些方法直接发裸 JSON”。

### 2.3 Typed Namespaces
方法名必须稳定、可枚举、可版本化，例如：
- `runtime.ping`
- `session.send`
- `task.subscribe`

### 2.4 Streaming Native
流式输出不是“某个方法偷偷多发几段文本”，而是显式的 `stream` 消息族。

### 2.5 Subscription Native
事件订阅必须是协议一等公民，不应退化成“客户端不断轮询”。

---
## 3. 协议版本与握手
### 3.1 协议版本字段
每个连接在握手阶段应确认：
- `api_version`
- `schema_version`
- `daemon_version`

建议：
- `api_version`：方法与 envelope 兼容层版本
- `schema_version`：结构体字段 schema 版本
- `daemon_version`：具体程序版本

### 3.2 握手流程
连接建立后建议先交换：
1. `hello`
2. `hello_ack`

客户端发送：
```json
{
  "type": "hello",
  "api_version": "v1",
  "client": {
    "name": "agentjax-cli",
    "version": "0.1.0"
  },
  "capabilities": ["request_response", "events", "streams"]
}
```

daemon 返回：
```json
{
  "type": "hello_ack",
  "ok": true,
  "api_version": "v1",
  "schema_version": "2026-04-10",
  "daemon_version": "0.1.0",
  "connection_id": "conn_123"
}
```

如果版本不兼容，应返回结构化错误并关闭连接。

---
## 4. 顶层 Envelope
### 4.1 Envelope 分类
协议顶层 envelope 建议固定为：
- `hello`
- `hello_ack`
- `request`
- `response`
- `event`
- `stream`
- `error`

### 4.2 Request Envelope
```json
{
  "id": "req_123",
  "type": "request",
  "method": "session.send",
  "params": {},
  "meta": {
    "trace_id": "tr_1",
    "session_id": "sess_1"
  }
}
```

字段：
- `id`: 请求唯一 id，由客户端生成
- `type`: 固定为 `request`
- `method`: 方法名
- `params`: 方法参数
- `meta`: 可选元数据

### 4.3 Response Envelope
```json
{
  "id": "req_123",
  "type": "response",
  "ok": true,
  "result": {},
  "meta": {
    "served_by": "daemon.main"
  }
}
```

字段：
- `id`: 对应请求 id
- `type`: 固定为 `response`
- `ok`: 是否成功
- `result`: 成功结果
- `meta`: 可选元数据

失败时：
```json
{
  "id": "req_123",
  "type": "response",
  "ok": false,
  "error": {
    "code": "session_not_found",
    "message": "session not found",
    "retryable": false
  }
}
```

### 4.4 Event Envelope
```json
{
  "type": "event",
  "event": "task.updated",
  "subscription_id": "sub_1",
  "seq": 42,
  "data": {}
}
```

字段：
- `type`: 固定为 `event`
- `event`: 事件名
- `subscription_id`: 来源订阅 id
- `seq`: 连接内递增序号
- `data`: 事件负载

### 4.5 Stream Envelope
```json
{
  "type": "stream",
  "stream_id": "str_1",
  "phase": "chunk",
  "event": "token",
  "seq": 3,
  "data": {}
}
```

字段：
- `type`: 固定为 `stream`
- `stream_id`: 流 id
- `phase`: `start | chunk | end | error`
- `event`: 流事件名
- `seq`: 流内序号
- `data`: 流负载

### 4.6 Error Envelope
`error` envelope 只用于：
- 握手失败
- 无法关联到某个 request id 的协议级错误
- transport 级异常通知

```json
{
  "type": "error",
  "error": {
    "code": "protocol_violation",
    "message": "missing method field",
    "retryable": false
  }
}
```

---
## 5. 通用元数据模型
### 5.1 Request Meta
`request.meta` 建议支持：
- `trace_id`
- `requester`
- `session_id`
- `task_id`
- `agent_id`
- `surface_id`
- `channel_id`
- `timeout_ms`

### 5.2 Actor Identity
建议保留一个通用 actor 模型：
```json
{
  "kind": "cli",
  "id": "operator.local",
  "label": "agentjax-cli"
}
```

`kind` 可取：
- `cli`
- `tui`
- `webui`
- `channel`
- `system`

### 5.3 Correlation Fields
所有 response / event / stream 建议可选带上：
- `trace_id`
- `correlation_id`
- `causation_id`

---
## 6. 错误模型
### 6.1 Error Shape
建议统一为：
```json
{
  "code": "session_not_found",
  "message": "session not found",
  "details": {},
  "retryable": false
}
```

字段：
- `code`: 稳定机器码
- `message`: 人类可读摘要
- `details`: 可选结构化细节
- `retryable`: 是否建议重试

### 6.2 错误类别
建议至少区分：
- `invalid_request`
- `unauthorized`
- `forbidden`
- `not_found`
- `conflict`
- `rate_limited`
- `timeout`
- `busy`
- `unsupported_method`
- `unsupported_version`
- `protocol_violation`
- `internal_error`

### 6.3 常见稳定错误码
建议预留：
- `runtime_not_ready`
- `daemon_draining`
- `session_not_found`
- `task_not_found`
- `agent_not_found`
- `plugin_not_found`
- `node_not_found`
- `schedule_not_found`
- `subscription_not_found`
- `stream_not_found`
- `config_invalid`
- `reload_failed`
- `plugin_test_failed`

---
## 7. 订阅模型
### 7.1 订阅返回值
所有 `*.subscribe` 方法成功后，返回：
```json
{
  "subscription_id": "sub_1",
  "accepted_events": ["session.updated", "turn.created"]
}
```

### 7.2 订阅生命周期
建议支持：
- create
- active
- draining
- closed

### 7.3 取消订阅
建议统一方法：
- `subscription.cancel`

参数：
```json
{
  "subscription_id": "sub_1"
}
```

### 7.4 断线恢复
第一版可以不保证断线续订。
但建议预留：
- `last_seq`
- `resume_token`

后续可扩成断线重连补事件。

---
## 8. 流模型
### 8.1 Stream Phase
`stream.phase` 固定建议为：
- `start`
- `chunk`
- `end`
- `error`

### 8.2 流启动
某些请求成功后可在 `response.result` 中返回：
```json
{
  "stream_id": "str_1"
}
```

随后 daemon 通过 `stream` envelope 推送内容。

### 8.3 常见流事件
建议预留：
- `token`
- `message.delta`
- `message.completed`
- `tool.started`
- `tool.completed`
- `task.progress`
- `task.checkpoint`
- `log.line`

### 8.4 取消流
建议统一方法：
- `stream.cancel`

参数：
```json
{
  "stream_id": "str_1"
}
```

---
## 9. 方法命名空间
方法名建议固定为：
- `runtime.*`
- `config.*`
- `plugin.*`
- `agent.*`
- `session.*`
- `task.*`
- `node.*`
- `schedule.*`
- `logs.*`
- `metrics.*`
- `doctor.*`
- `smoke.*`
- `subscription.*`
- `stream.*`

---
## 10. Runtime 方法
### 10.1 `runtime.ping`
请求：
```json
{}
```

响应：
```json
{
  "pong": true,
  "daemon_time": "2026-04-10T12:00:00Z"
}
```

### 10.2 `runtime.status`
响应至少应包含：
```json
{
  "status": "ready",
  "daemon_version": "0.1.0",
  "api_version": "v1",
  "uptime_secs": 12,
  "ready": true,
  "draining": false
}
```

### 10.3 `runtime.shutdown`
请求：
```json
{
  "graceful": true
}
```

---
## 11. Config 方法
### 11.1 `config.inspect`
请求：
```json
{
  "section": "providers"
}
```

响应：
```json
{
  "section": "providers",
  "config": {}
}
```

### 11.2 `config.validate`
响应至少应包含：
```json
{
  "ok": true,
  "errors": [],
  "warnings": []
}
```

### 11.3 `config.reload`
响应至少应包含：
```json
{
  "ok": true,
  "reloaded_modules": ["providers.openai", "channels.telegram"],
  "drained_modules": []
}
```

---
## 12. Plugin 方法
### 12.1 `plugin.list`
响应：
```json
{
  "items": [
    {
      "id": "telegram",
      "enabled": true,
      "healthy": true,
      "capabilities": ["channel.telegram"]
    }
  ]
}
```

### 12.2 `plugin.inspect`
请求：
```json
{
  "plugin_id": "telegram"
}
```

### 12.3 `plugin.reload`
请求：
```json
{
  "plugin_id": "telegram"
}
```

### 12.4 `plugin.test`
请求：
```json
{
  "plugin_id": "telegram"
}
```

---
## 13. Agent 方法
### 13.1 `agent.list`
响应：
```json
{
  "items": [
    {
      "agent_id": "main",
      "status": "active",
      "workspace_id": "default"
    }
  ]
}
```

### 13.2 `agent.get`
请求：
```json
{
  "agent_id": "main"
}
```

---
## 14. Session 方法
### 14.1 `session.list`
响应中的 item 至少应包含：
- `session_id`
- `agent_id`
- `title`
- `status`
- `channel_id`
- `surface_id`
- `last_activity_at`

### 14.2 `session.get`
请求：
```json
{
  "session_id": "sess_1"
}
```

### 14.3 `session.send`
请求建议至少支持：
```json
{
  "session_id": "sess_1",
  "message": {
    "role": "user",
    "content": "hello"
  },
  "stream": true
}
```

成功响应：
```json
{
  "accepted": true,
  "turn_id": "turn_1",
  "stream_id": "str_1"
}
```

### 14.4 `session.cancel`
请求：
```json
{
  "session_id": "sess_1"
}
```

### 14.5 `session.subscribe`
请求：
```json
{
  "session_id": "sess_1",
  "events": ["session.updated", "turn.created", "message.completed"]
}
```

---
## 15. Task 方法
### 15.1 `task.list`
响应 item 至少应包含：
- `task_id`
- `kind`
- `status`
- `agent_id`
- `session_id`
- `created_at`

### 15.2 `task.get`
请求：
```json
{
  "task_id": "task_1"
}
```

### 15.3 `task.cancel`
请求：
```json
{
  "task_id": "task_1"
}
```

### 15.4 `task.retry`
请求：
```json
{
  "task_id": "task_1"
}
```

### 15.5 `task.subscribe`
请求：
```json
{
  "task_id": "task_1",
  "events": ["task.updated", "task.progress", "task.completed"]
}
```

---
## 16. Node 与 Schedule 方法
### 16.1 `node.list`
响应 item 至少应包含：
- `node_id`
- `status`
- `capabilities`

### 16.2 `node.get`
请求：
```json
{
  "node_id": "node_1"
}
```

### 16.3 `schedule.list`
响应 item 至少应包含：
- `schedule_id`
- `kind`
- `enabled`
- `next_run_at`

### 16.4 `schedule.create`
### 16.5 `schedule.update`
### 16.6 `schedule.delete`
第一版可以先收敛为最小 CRUD。

---
## 17. Diagnostics 方法
### 17.1 `doctor.run`
响应建议至少包含：
```json
{
  "ok": true,
  "checks": [
    {
      "id": "config.paths",
      "status": "pass"
    }
  ]
}
```

### 17.2 `smoke.run`
请求：
```json
{
  "target": "plugins"
}
```

### 17.3 `logs.tail`
请求：
```json
{
  "stream": true,
  "level": "info"
}
```

建议返回 `stream_id`。

### 17.4 `metrics.snapshot`
响应：
```json
{
  "counters": {},
  "gauges": {},
  "histograms": {}
}
```

---
## 18. 事件命名建议
建议统一使用 `<domain>.<action>`：
- `runtime.ready`
- `runtime.draining`
- `config.reloaded`
- `plugin.loaded`
- `plugin.reloaded`
- `session.created`
- `session.updated`
- `turn.created`
- `message.completed`
- `task.updated`
- `task.completed`
- `node.updated`
- `schedule.triggered`
- `module.reloaded`

---
## 19. 第一版可简化的部分
第一版可以先不做：
- 跨连接断线恢复
- 多 tenant auth
- event replay
- exactly-once delivery
- backpressure 自适应复杂策略
- distributed subscription broker

但以下边界不能省：
- 统一 envelope
- 统一错误模型
- request / response / event / stream 明确分型
- method namespace 固定
- subscription / stream 作为协议一等公民

---
## 20. 与其他规范的关系
- `docs/CHANNELS_DAEMON_CLIENT_SPEC.md`：定义 daemon/client、surface/channel/transport 的架构边界
- `docs/CORE_OBJECT_MODEL.md`：定义 session / task / event / plugin 等核心对象
- `docs/PLUGIN_SDK.md`：定义 plugin / channel / ui / workflow 能力边界

---
## 21. 硬结论
AgentJax 应采用：
- 一套统一 daemon API schema
- `Unix socket` 与 `WebSocket` 共用同一 envelope 与 method namespace
- 显式的 request / response / event / stream / error 分型
- 原生 subscription 与 streaming 模型

如果这层不先钉死，后面实现出来的 CLI、TUI、WebUI、WebSocket、IPC 一定会各长一套，最后变成协议屎山。
