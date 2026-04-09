# AgentJax Architecture Entrypoint
## 1. 目的
本文档是 AgentJax 当前阶段的**唯一正式开发入口**。
从现在开始，项目不再以 MVP 原型为主线，而以：
- 核心对象契约
- workspace/config/state 边界
- 插件化 runtime
- event/task/LCM runtime
- LCM context engine
- RAG/knowledge/memory 分层
- daemon/client/channels/surfaces/transports 分层
- usage/billing/scheduler/node 扩展边界
作为正式实现起点。
这份文档的作用是：
1. 汇总当前所有正式规范
2. 给出推荐代码结构
3. 定义下一轮 Code 模式的起步顺序
4. 提供一条新的、明确的实现入口
---
## 2. 当前正式规范文档
AgentJax 当前的正式架构规范由以下文档组成。
### Spec 1：Core Object Model
- `docs/CORE_OBJECT_MODEL.md`
- 作用：定义系统中的一等公民对象与核心字段
- 包括：
  - Agent
  - Session
  - Turn
  - Task
  - Event
  - ContextBlock
  - ToolCall
  - Artifact
  - Node
  - Resource
  - Plugin
  - Skill
  - Schedule
  - SummaryNode
  - RuntimeError
  - identity / secret scope / schema version
### Spec 2：Workspace / Config / State Layout
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`
- 作用：定义 workspace、config、state、artifacts、logs、cache、tmp 的边界
- 包括：
  - 工作区顶层身份文件
  - memory 分层
  - config root
  - state root
  - artifact/log/cache/tmp 目录边界
  - 模块级热重载
  - skill 包规范
### Spec 3：Plugin SDK + Resource Model
- `docs/PLUGIN_SDK.md`
- 作用：定义插件能力模型、资源抽象、PluginManifest、PluginContext、PluginRegistry
- 包括：
  - capability model
  - provider/memory/context/tool/channel/node/skill/command/hook/ui/workflow
  - Resource Layer
  - Plugin lifecycle
  - Hook / Event 模型
### Spec 4：Event / Task / LCM Runtime
- `docs/EVENT_TASK_LCM_RUNTIME.md`
- 作用：定义执行流、事件流、任务流、压缩流的基础契约
- 包括：
  - RuntimeEvent
  - turn/task phase
  - retry / idempotency / circuit breaker
  - observability baseline
  - LCM invalidation / recompute
  - context budget
  - autonomy policy
  - routing / planning / skill trigger policy
### Spec 5：LCM Context Engine
- `docs/LCM_CONTEXT_ENGINE.md`
- 作用：定义原生上下文引擎
- 包括：
  - event-stream-first
  - immutable event log
  - active context projection
  - summary DAG
  - recall / expansion plane
  - leaf / condensed / checkpoint compaction
  - purpose-aware assembly
  - resume-first design
### Spec 6：RAG / Knowledge / Memory
- `docs/RAG_KNOWLEDGE_MEMORY_SPEC.md`
- 作用：定义 RAG、Knowledge、Memory 的正式分层与职责边界
- 包括：
  - Context Engine / RAG / Knowledge / Memory 的四层关系
  - RAG engine 作为通用 retrieval substrate 的能力面
  - collection / index pipeline / query pipeline / driver 抽象
  - knowledge system 的领域化组织方式
  - memory system 作为基于 RAG 的特化长期语义层
  - promotion / conflict / freshness / behavioral relevance policy
### Spec 7：Channels / Daemon / Client
- `docs/CHANNELS_DAEMON_CLIENT_SPEC.md`
- 作用：定义 daemon/client、surface/channel/transport、IPC/WebSocket API 的正式分层
- 包括：
  - daemon-only runtime 原则
  - core surfaces 与 plugin channels 的边界
  - unix socket / websocket 双 transport 模型
  - unified API schema across transports
  - daemon lifecycle 与 module-level hot reload
  - cli / tui / webui 的角色划分
### Spec 8：Daemon API / IPC Schema
- `docs/DAEMON_API_IPC_SCHEMA.md`
- 作用：定义 daemon 对外 request/response/event/stream/error 的协议契约
- 包括：
  - hello / hello_ack 握手
  - request / response / event / stream / error envelope
  - method namespace
  - subscription / stream lifecycle
  - error model
  - runtime / config / plugin / session / task 等最低 payload schema
### Spec 9：Usage / Billing / Scheduler / Node
- `docs/USAGE_BILLING_SCHEDULER_NODE_SPEC.md`
- 作用：定义 usage facts、billing facts、scheduled task、capability node 的扩展边界
- 包括：
  - telemetry / usage ledger / billing ledger
  - local estimate / provider reported / reconciliation
  - scheduled task model
  - trigger / execution mode / target / bindings
  - capability node registry / selector
---
## 3. 当前项目的总体方向
一句话概括：
**AgentJax 不是一个“带几个工具的 Telegram Bot”，而是一个围绕 Workspace、Plugin Runtime、Context Engine、Task Runtime 构建的长期演化 Agent Framework。**
### 核心思想
- Workspace 是 agent 的自我与长期知识域
- Config 是 runtime wiring
- State 是运行态
- Artifacts 是产物
- Event log 是事实层
- LCM 是连续性基础设施
- RAG 是通用检索基础设施
- Memory 是基于 RAG 的特化长期语义层
- Daemon 是唯一运行时宿主
- Surface / Channel / Transport 必须显式分层
- Plugin Runtime 是扩展面
- Resource Layer 是能力接入面
- Scheduler / Node / Billing 是扩展执行与运营层
---
## 4. 推荐代码结构
建议下一轮 Code 模式开始后，围绕以下代码结构重组项目。
```text
src/
  main.rs
  app.rs
  bootstrap.rs
  config/
    mod.rs
    loader.rs
    paths.rs
    runtime.rs
    workspace.rs
  core/
    mod.rs
    runtime.rs
    workspace_runtime.rs
    plugin.rs
    registry.rs
    resource_registry.rs
    hook_bus.rs
    event_bus.rs
    errors.rs
    reload.rs
  domain/
    mod.rs
    object_meta.rs
    agent.rs
    session.rs
    turn.rs
    task.rs
    event.rs
    context.rs
    tool.rs
    artifact.rs
    node.rs
    resource.rs
    plugin.rs
    skill.rs
    schedule.rs
    summary.rs
    usage.rs
    billing.rs
    policies.rs
  context_engine/
    mod.rs
    engine.rs
    event_store.rs
    projection_store.rs
    assembler.rs
    compactor.rs
    expander.rs
    resume.rs
    schema.rs
  plugins/
    mod.rs
    providers/
      mod.rs
      openai.rs
      embedding.rs
      tts.rs
      st.rs
    storage/
      mod.rs
      sqlite_sessions.rs
      sqlite_context.rs
    context/
      mod.rs
      workspace_identity.rs
      task_state.rs
      summary_loader.rs
      retrieval_bridge.rs
    tools/
      mod.rs
      read_file.rs
      list_files.rs
      shell.rs
    channels/
      mod.rs
      telegram.rs
    scheduler/
      mod.rs
      local_scheduler.rs
    billing/
      mod.rs
      estimator.rs
    nodes/
      mod.rs
      static_registry.rs
  infra/
    mod.rs
    fs.rs
    security.rs
    process.rs
    sqlite.rs
    tracing.rs
    time.rs
```
---
## 5. 目录职责说明
### `config/`
负责：
- config root 解析
- workspace root 解析
- state/artifacts/logs/cache/tmp 路径建模
- runtime config / workspace config 加载
### `core/`
负责：
- runtime 主骨架
- plugin registry
- resource registry
- workspace runtime
- hook/event bus
- 热重载协议
- 统一错误模型
### `domain/`
负责：
- 所有正式 spec 中的一等公民类型
- schema version
- policy objects
- usage/billing/schedule/node/context summary 类型
### `context_engine/`
负责：
- immutable event log
- active context projection
- summary DAG
- compaction
- assembly
- expansion
- resume pack
### `plugins/`
负责所有具体实现：
- providers
- storage
- context plugins
- tools
- channels
- scheduler
- billing
- nodes
### `infra/`
负责基础设施：
- 文件系统
- 进程执行
- sqlite 连接
- tracing
- 时间与系统工具
---
## 6. 下一轮 Code 模式的实现原则
下一轮开始时，**不要优先接功能，不要优先接 Telegram，不要优先接 tool calling 细节**。
优先级应该是：
### 原则 1
先把 spec 变成 Rust 类型与模块边界。
### 原则 2
先让 `domain/`、`core/`、`config/`、`context_engine/` 成形。
### 原则 3
插件实现先只保留“空骨架 + 最小适配”，不要急着做深功能。
### 原则 4
目标是“架构转向成功且 `cargo check` 通过”，不是一轮做完全部 runtime。
---
## 7. 第一轮 Code 模式的明确目标
建议新对话开启后，第一轮只做 **架构骨架重构**。
### P0：先做目录与类型系统
1. 创建新的目录结构：
   - `src/config/`
   - `src/core/`
   - `src/domain/`
   - `src/context_engine/`
   - `src/plugins/`
2. 将 Spec 1 中最关键对象落成 Rust 类型：
   - `ObjectMeta`
   - `Agent`
   - `Session`
   - `Turn`
   - `Task`
   - `RuntimeEvent`
   - `ContextBlock`
   - `ToolCall`
   - `Artifact`
   - `SummaryNode`
   - `RuntimeError`
3. 将 Spec 2 中最关键路径模型落成 Rust 类型：
   - `WorkspacePaths`
   - `RuntimePaths`
   - `ConfigRoot`
4. 将 Spec 3 中最关键插件骨架落成 Rust 类型：
   - `PluginManifest`
   - `PluginRegistry`
   - `PluginContext`（最小版）
   - `ResourceDescriptor` / `ResourceId`
5. 将 Spec 4 / 5 中最关键 runtime 契约落成 Rust 类型：
   - `TurnPhase`
   - `TaskPhase`
   - `ContextAssemblyPurpose`
   - `ResumePack`
   - `RetryPolicy`
   - `AutonomyPolicy`
### P1：再做最小骨架实现
6. `Application` 重构为 plugin host + workspace runtime host
7. 创建最小 `ContextEngine` trait
8. 创建最小 `WorkspaceRuntime` trait / struct
9. 创建最小 `EventBus` / `PluginRegistry` 空实现
10. 保持项目通过 `cargo check`
### P2：最后做兼容迁移壳
11. 旧代码先不深度实现，只做适配壳：
   - sqlite session
   - rig backend
   - telegram adapter
   - tools
12. 这些实现先放入 `plugins/`，即使是 stub 也行
---
## 8. 当前阶段不建议立即做的事情
为了避免又回到“边写边堆功能”的老路，以下内容本轮不建议优先做：
- Telegram 全链路打通
- 完整 tool calling loop
- ST/TTS 真接入
- 完整 scheduler
- usage/billing 真结算
- 全量 LCM compaction worker
- 自动 skill routing
- 动态插件加载
- 复杂热重载
- 分布式 node routing
这些都应在骨架稳定后再接。
---
## 9. 新对话的启动提示词
你下一轮全新对话，可以直接使用下面这段作为启动提示词。
### 推荐启动提示词
> 请以 `docs/ARCHITECTURE_ENTRYPOINT.md` 作为唯一开发入口，按其中的推荐代码结构对当前 Rust 项目做第一轮架构重构。目标不是继续实现功能，而是把现有规范落成 Rust 类型系统与模块骨架。优先完成 `src/config/`、`src/core/`、`src/domain/`、`src/context_engine/`、`src/plugins/` 的目录重组；落地 Core Object Model、Workspace/Runtime 路径模型、PluginManifest/PluginRegistry 最小骨架、RuntimeEvent/TurnPhase/TaskPhase/ContextAssemblyPurpose/ResumePack 等关键类型；将 `Application` 重构为 plugin host + workspace runtime host；保留旧实现为最小兼容壳，最终目标是保持项目 `cargo check` 通过。
### 更强约束版启动提示词
> 请严格按 `docs/ARCHITECTURE_ENTRYPOINT.md`、`docs/CORE_OBJECT_MODEL.md`、`docs/WORKSPACE_AND_CONFIG_SPEC.md`、`docs/PLUGIN_SDK.md`、`docs/RAG_KNOWLEDGE_MEMORY_SPEC.md`、`docs/CHANNELS_DAEMON_CLIENT_SPEC.md`、`docs/DAEMON_API_IPC_SCHEMA.md`、`docs/EVENT_TASK_LCM_RUNTIME.md`、`docs/LCM_CONTEXT_ENGINE.md` 的契约，重构当前 Rust 项目。第一轮不要扩功能，不要优先 Telegram/tool calling，而是先完成 domain/core/config/context_engine 的类型与骨架，保持 cargo check 通过，并为后续 plugin/runtime/context engine / rag subsystem / daemon api 落地建立稳定边界。
---
## 10. 建议的实现顺序总览
如果把后续工作拆成几轮，我建议：
### Round 1
- 类型系统
- 目录结构
- plugin/runtime/context engine 骨架
### Round 2
- config/workspace loader
- event bus / registry 初版
- sqlite schema 初版
### Round 3
- context engine storage + assemble_context 空实现
- basic projection / summaries / checkpoints 模型
### Round 4
- 旧代码插件化迁移
- sqlite session / rig backend / tools / telegram stub 适配
### Round 5
- 才开始接真正功能流
- tool loop
- context assembly
- event logging
- minimal end-to-end runtime
---
## 11. 最终结论
AgentJax 当前已经不缺想法，也不缺功能方向。
当前最重要的是：
**停止继续追加功能脑暴，开始把所有正式规范收束成一套可编译、可演化的 Rust 架构骨架。**
这份文档就是那个新入口。
