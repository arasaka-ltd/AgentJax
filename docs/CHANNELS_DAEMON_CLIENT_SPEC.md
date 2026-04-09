# AgentJax Channels + Daemon / Client Spec
## 1. 目标
本文档定义 AgentJax 的 `daemon / client / surface / channel / transport` 分层。

这套规范要解决的是：
- runtime 到底跑在哪里
- CLI / TUI / WebUI 和 runtime 是什么关系
- `channel`、`surface`、`transport` 这些概念怎么拆
- 本地 IPC 与 WebSocket 如何共用统一 API schema

必须先拍死一个总原则：
- `agentjax-daemon` 是唯一运行时宿主
- `CLI / TUI / WebUI` 都只是客户端
- `Channel` 是外部消息平台接入抽象
- `TUI` 和 `WebUI` 是内建 core surfaces，不是插件化 channels
- `Unix socket` 和 `WebSocket` 是 transports，不是 channels

一句话：
**daemon-only runtime, all clients out-of-process, unified API schema across transports.**

---
## 2. 核心概念
### 2.1 Daemon
`Daemon` 是 headless runtime server。

它负责：
- 加载配置
- 初始化 runtime
- 加载插件
- 挂载资源
- 管理 agents / sessions / tasks / schedules / nodes
- 运行 LCM / RAG / memory / scheduler / plugin runtime
- 暴露 IPC / API
- 持久化状态
- 输出日志 / 指标

它不负责：
- 终端 UI
- 浏览器 UI 渲染
- 交互式命令体验

### 2.2 Client
`Client` 是连接 daemon 的操作端或展示端。

例如：
- `agentjax-cli`
- `agentjax-tui`
- `WebUI`
- 本地脚本或调试工具

客户端不持有 agent runtime，只通过对外 API 与 daemon 交互。

### 2.3 Surface
`Surface` 是人类或前端与 runtime 交互的界面层。

包括：
- `CLI command mode`
- `TUI`
- `WebUI`

`Surface` 不是消息平台接入，不应与 `Channel` 混用。

### 2.4 Channel
`Channel` 是外部消息平台接入。

包括：
- `Telegram`
- `Discord`
- `QQ`
- `Email`
- `Slack`
- `Webhook`

`Channel` 应走插件化扩展，不应把 `TUI / WebUI / WebSocket` 混进来。

### 2.5 Transport
`Transport` 是协议承载层。

包括：
- `Unix socket`
- `WebSocket`
- `HTTP`
- `stdio`

`Transport` 只负责连接、帧、会话与流控，不负责业务语义定义。

---
## 3. 二进制划分
### 3.1 `agentjax-daemon`
职责：
- 加载配置
- 初始化 runtime
- 加载插件
- 挂载资源
- 管理 agents / sessions / tasks / schedules / nodes
- 暴露 `Unix socket API`
- 暴露 `WebSocket API`
- 执行后台工作
- 持久化状态
- 输出日志 / 指标

不负责：
- 终端 UI
- 浏览器 UI 渲染
- 交互式命令解析体验

一句话：
它是 `headless runtime server`。

### 3.2 `agentjax-cli`
职责：
- 连接 daemon
- 发管理命令
- 配置 / 诊断 / doctor
- 模块 / 插件冒烟测试
- 启动 TUI
- 人机交互调试
- 脚本化调用

默认：
- 通过 `Unix socket` 连接本机 daemon
- 后续可扩展到 `TCP` 或 remote endpoint

一句话：
它是 `operator client`。

---
## 4. Core Surfaces 与 Plugin Channels
### 4.1 Core Surfaces
AgentJax 内建两类核心 surface：
- `TUI`
- `WebUI`

它们不应走插件化 channel 模型。

原因：
- 它们是 runtime 的官方操作与观察界面
- 它们需要直接覆盖 control plane 与 interaction plane
- 它们更像 core client surfaces，而不是消息渠道适配器

### 4.2 Plugin Channels
下列才属于插件化 channels：
- `Telegram`
- `Discord`
- `QQ`
- `Email`
- `Slack`
- `Webhook`

这些 channels 的职责是：
- 收外部消息
- 投递到 session / task / agent runtime
- 把 runtime 输出再映射回外部平台消息模型

---
## 5. 通信面拆分
### 5.1 Control Plane
用于管理和配置：
- daemon status
- config inspect
- config reload
- plugin list
- plugin test
- doctor
- logs
- metrics
- schedule manage
- node manage

默认走：
- `Unix socket RPC`

### 5.2 Interaction Plane
用于 agent / session / task 交互：
- send message
- stream response
- subscribe events
- inspect session
- inspect task timeline
- interrupt / cancel

可走：
- `Unix socket stream`
- `WebSocket stream`

这个拆分必须保持明确。
control plane 和 interaction plane 混在一起，后面很容易把 API、权限和连接生命周期一起做臭。

---
## 6. Daemon 对外接口
### 6.1 Unix Socket API
默认本机控制接口。

建议路径：
- `~/.agentjax/run/daemon.sock`
- 或 `/var/run/agentjax/daemon.sock`

用途：
- CLI 默认连接
- 本机脚本
- 本地 TUI
- 高权限管理操作

优点：
- 权限清晰
- 延迟低
- 本地安全边界自然

### 6.2 WebSocket API
建议默认只绑定 localhost。

例如：
- `ws://127.0.0.1:4080/ws`

用途：
- WebUI
- 实时事件流
- 浏览器或远程前端
- 流式 agent 输出

后续可扩展：
- auth
- remote access
- reverse proxy
- token session

---
## 7. API 统一原则
`Unix socket` 与 `WebSocket` 应复用同一套 API schema。
不要做成：
- ws 一套
- ipc 一套
- cli 再一套

最好的做法是：
- 同样的 method names
- 同样的 payload structs
- 同样的 error model
- 同样的 event schema
- transport 只负责承载和 framing

一句话：
**统一 API schema，transport 只是不同载体。**

更细的协议包结构、错误模型、subscription / stream schema 由：
- `docs/DAEMON_API_IPC_SCHEMA.md`

负责钉死。

---
## 8. Envelope 协议
### 8.1 请求
```json
{
  "id": "req_123",
  "type": "request",
  "method": "session.send",
  "params": {}
}
```

### 8.2 响应
```json
{
  "id": "req_123",
  "type": "response",
  "ok": true,
  "result": {}
}
```

### 8.3 事件
```json
{
  "type": "event",
  "event": "task.updated",
  "data": {}
}
```

### 8.4 流
```json
{
  "type": "stream",
  "stream_id": "str_1",
  "event": "token",
  "data": {}
}
```

这套 envelope 应同时适用于 `Unix socket` 与 `WebSocket`。

---
## 9. 最低 API 方法集
### 9.1 Runtime
- `runtime.ping`
- `runtime.status`
- `runtime.shutdown`

### 9.2 Config
- `config.inspect`
- `config.validate`
- `config.reload`

### 9.3 Plugins
- `plugin.list`
- `plugin.inspect`
- `plugin.reload`
- `plugin.test`

### 9.4 Sessions
- `session.list`
- `session.get`
- `session.send`
- `session.cancel`
- `session.subscribe`

### 9.5 Tasks
- `task.list`
- `task.get`
- `task.cancel`
- `task.retry`
- `task.subscribe`

### 9.6 Agents
- `agent.list`
- `agent.get`

### 9.7 Nodes
- `node.list`
- `node.get`

### 9.8 Schedules
- `schedule.list`
- `schedule.create`
- `schedule.update`
- `schedule.delete`

### 9.9 Diagnostics
- `doctor.run`
- `smoke.run`
- `logs.tail`
- `metrics.snapshot`

---
## 10. 推荐 crate 与 app 结构
建议按以下结构拆分：

```text
apps/
  agentjax-daemon/
  agentjax-cli/

crates/
  agentjax-runtime/
  agentjax-api/
  agentjax-ipc/
  agentjax-ws/
  agentjax-tui/
```

### 10.1 `agentjax-runtime`
真正的核心 runtime：
- agents
- sessions
- tasks
- schedules
- plugins
- context engine
- rag / memory
- resources
- nodes

### 10.2 `agentjax-api`
定义对外协议模型：
- request / response structs
- event enums
- streaming message schema
- errors
- pagination / filter types

这是协议契约层。

### 10.3 `agentjax-ipc`
实现本地 `Unix socket` transport：
- request / response
- stream framing
- reconnect
- version handshake

### 10.4 `agentjax-ws`
实现 `WebSocket` transport：
- ws server
- auth / session binding
- subscriptions
- event fanout
- flow control

### 10.5 `agentjax-tui`
终端 UI：
- pane system
- session view
- task view
- event / log view
- command palette

它不直接调 runtime，只调 `agentjax-api` client。

---
## 11. Daemon 生命周期
### 11.1 启动流程
建议固定成：
1. parse args
2. resolve config root / workspace root / state dirs
3. acquire daemon lock / pid file
4. init logging / tracing
5. load config
6. init storage / state backends
7. init runtime core
8. load core modules
9. load plugins
10. init resources / providers
11. init schedules / nodes
12. open unix socket
13. open websocket server
14. mark ready
15. serve until shutdown

### 11.2 停止流程
1. stop accepting new control requests
2. drain interactive streams
3. checkpoint active tasks if needed
4. stop schedulers
5. stop plugins
6. flush logs / state
7. remove socket / pid marker
8. exit

---
## 12. 模块级热重载
CLI 建议提供：
- `agentjax-cli config reload`
- `agentjax-cli plugin reload <id>`
- `agentjax-cli provider reload <id>`
- `agentjax-cli channel reload <id>`
- `agentjax-cli schedule reload`

daemon 内部行为建议为：
1. diff config
2. 找受影响模块
3. prepare reload
4. instantiate new module
5. health check
6. swap
7. old instance drain
8. emit `module_reloaded` event

目标：
- 不中断 agent runtime
- 不中断 ws / tui 连接
- 只影响对应模块

---
## 13. CLI 能力面
### 13.1 基础命令
- `agentjax-cli status`
- `agentjax-cli doctor`
- `agentjax-cli ping`
- `agentjax-cli version`

### 13.2 Daemon 管理
- `agentjax-cli daemon start`
- `agentjax-cli daemon stop`
- `agentjax-cli daemon restart`
- `agentjax-cli daemon logs`
- `agentjax-cli daemon socket`

### 13.3 配置
- `agentjax-cli config show`
- `agentjax-cli config validate`
- `agentjax-cli config reload`
- `agentjax-cli config diff`

### 13.4 插件
- `agentjax-cli plugin list`
- `agentjax-cli plugin inspect <id>`
- `agentjax-cli plugin reload <id>`
- `agentjax-cli plugin test <id>`

### 13.5 Agent / Session / Task
- `agentjax-cli agent list`
- `agentjax-cli session list`
- `agentjax-cli session send <id>`
- `agentjax-cli task list`
- `agentjax-cli task inspect <id>`
- `agentjax-cli task cancel <id>`

### 13.6 交互
- `agentjax-cli tui`
- `agentjax-cli chat`

### 13.7 冒烟测试
- `agentjax-cli smoke all`
- `agentjax-cli smoke plugins`
- `agentjax-cli smoke channels`
- `agentjax-cli smoke providers`
- `agentjax-cli smoke stt`
- `agentjax-cli smoke tts`

---
## 14. TUI 最小形态
第一版 TUI 建议只做 5 个 pane：
- `Sessions`
- `Tasks`
- `Events / Logs`
- `Chat / Interaction`
- `Inspect / Details`

支持：
- 选 session 发消息
- 看流式回复
- 看 task timeline
- 看 plugin health
- 看 node 状态
- 看成本 / usage

第一版目标不是 IDE，而是能运维、能调试、能交互的控制台。

---
## 15. 权限与认证
### 15.1 Unix Socket
第一版默认依赖文件系统权限：
- 同用户可控
- root / system service 模式后续再扩

### 15.2 WebSocket
第一版建议：
- 默认只监听 localhost
- 可选 token
- 后续再接远程 auth

不要一开始把认证系统做成主战场。

---
## 16. 与 Plugin SDK 的关系
必须明确：
- `Surface` 不是 `Channel`
- `WebSocket` 不是 `Channel`
- `Unix socket` 不是 `Channel`

在插件模型中：
- `ChannelPlugin` 只表示外部消息平台接入
- `TUI` / `WebUI` 属于 core surfaces
- `Unix socket` / `WebSocket` 属于内建 transports

因此：
- `Telegram / Discord / QQ / Email` 走插件化 `ChannelPlugin`
- `TUI / WebUI` 不走插件能力模型
- `agentjax-daemon` 必须内建 `Unix socket` 与 `WebSocket` server

---
## 17. 硬结论
AgentJax 应明确采用以下分层：
- 核心程序：`agentjax-daemon`、`agentjax-cli`
- 内建 surfaces：`TUI`、`WebUI`
- 内建 transports：`Unix socket`、`WebSocket`
- 插件化 channels：`Telegram`、`Discord`、`QQ`、`Email` 等

架构原则：
- daemon-only runtime
- all clients out-of-process
- unified API schema across transports
- module-level hot reload
- surfaces are not runtime plugins
- channels are plugins
