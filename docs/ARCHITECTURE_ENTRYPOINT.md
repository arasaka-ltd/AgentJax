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
3. 定义执行顺序与任务入口
4. 提供下一轮对话的固定工作方式
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
### Spec 6.1：Retrieval Tools
- `docs/RETRIEVAL_TOOL_SPEC.md`
- 作用：定义 Agent-facing retrieval tool surface
- 包括：
  - 为什么不直接把 `RAG` 暴露成 agent tool
  - `memory.search` / `memory.get`
  - `knowledge.search` / `knowledge.get`
  - `search` 与 `get` 的职责分离
  - `library` / `path` / stable ref 的参数边界
### Spec 6.2：File Tools
- `docs/FILE_TOOLS_SPEC.md`
- 作用：定义 Agent-facing 文件操作工具协议
- 包括：
  - `read` / `edit` / `write`
  - 文本与图片读取
  - 行号 / 列号 / 精确区间编辑
  - `\n` 与 LF/CRLF 处理
  - `mkdir -p` 风格写入
  - 核心文件、memory、knowledge 的编辑边界
### Plan A：`src/plugins` Refactor
- `docs/PLUGIN_REFACTOR_PLAN.md`
- 作用：治理 builtin runtime 与真实插件系统的边界
- 包括：
  - builtin tools / storage / context 回归本体代码
  - 真正插件改为一插件一目录
  - `PluginManager` 的职责、状态机与启停治理
  - `Application::new()` 与 daemon plugin API 的迁移方向
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
### Spec 10：Config Manager
- `docs/CONFIG_MANAGER_SPEC.md`
- 作用：定义配置管理器的职责、初始化、加载、校验、迁移、快照与热重载契约
- 包括：
  - config initializer
  - config loader / validator / normalizer
  - config migrator
  - runtime config snapshot
  - config diff / reload plan
  - secret ref policy
### Spec 11：Prompt Assembly
- `docs/PROMPT_ASSEMBLY_SPEC.md`
- 作用：定义 workspace Markdown 到结构化 prompt，再到 XML 注入的正式协议
- 包括：
  - Markdown source format
  - prompt document / section / fragment 中间层
  - tool / memory / knowledge / task 的 XML 注入格式
  - `assemble_context()` 与 `render_prompt_xml()` 的分层
### Spec 12：Session / Event Persistence
- `docs/SESSION_EVENT_PERSISTENCE_SPEC.md`
- 作用：定义 Batch 3 的 session / message / event 持久化边界、SQLite schema 与迁移策略
- 包括：
  - sessions / session_messages / runtime_events / schema_migrations
  - store traits
  - transaction / consistency boundary
  - migration strategy
  - Batch 3 最小验收标准
---
## 2.5 执行入口文件
从这一轮开始，除了正式 spec 外，还必须配合两份执行文档：
- `/TASK.md`：当前可勾选任务清单
- `docs/IMPLEMENTATION_ROADMAP.md`：分批次实施路线图

执行顺序固定为：
1. 先读本文件
2. 再读相关 spec
3. 再读 `docs/IMPLEMENTATION_ROADMAP.md`
4. 最后按 `/TASK.md` 从上到下推进

工作规则：
- 每完成一项，就直接在 `TASK.md` 里勾掉
- 不跳批次，不重排优先级，除非出现硬 blocker
- 每完成一个小批次，都运行 `cargo fmt` 和 `cargo check`
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
  builtin/
    mod.rs
    tools/
      mod.rs
      read_file.rs
      list_files.rs
      shell.rs
    storage/
      mod.rs
      sqlite/
        mod.rs
        backend.rs
        sessions.rs
        context.rs
    context/
      mod.rs
      workspace_identity.rs
      task_state.rs
      summary_loader.rs
      retrieval_bridge.rs
  config/
    mod.rs
    loader.rs
    paths.rs
    plugins.rs
    runtime.rs
    workspace.rs
  core/
    mod.rs
    runtime.rs
    workspace_runtime.rs
    plugin.rs
    plugin_manager.rs
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
    openai/
      mod.rs
      plugin.rs
    telegram/
      mod.rs
      plugin.rs
    local_scheduler/
      mod.rs
      plugin.rs
    static_nodes/
      mod.rs
      plugin.rs
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
负责真正插件实现：
- external providers
- external channels
- schedulers / nodes 等可独立启停模块
Builtin tools / storage / context internals 不再放在这里。
### `infra/`
负责基础设施：
- 文件系统
- 进程执行
- sqlite 连接
- tracing
- 时间与系统工具
---
## 6. 当前阶段的执行原则
当前阶段不再以“再补一批骨架”为目标，而以“按顺序分批做完真实能力”为目标。

必须遵守：
- 不优先接 Telegram
- 不优先接 WebUI 花活
- 不优先接高级 RAG
- 不把任务重新打散成新的临时优先级

当前主线固定为：
1. workspace persona loading
2. context assembly v0
3. tool calling v0
4. session / event persistence
5. memory / RAG v0
6. daemon API completion
7. task runtime v0

当前存在一条优先级更高的测试解锁支线：
- config bootstrap + OpenAI provider testability
- 这条支线完成前，不进入 `tool calling v0`

当前在 `Batch 2` 之后、`Batch 3` 之前，还存在一条结构化注入支线：
- prompt injection + XML assembly
- 这条支线完成前，不进入 `session / event persistence`

详细批次顺序见：
- `docs/IMPLEMENTATION_ROADMAP.md`

逐项执行清单见：
- `/TASK.md`
---
## 7. 下一轮对话的固定工作方式
新一轮对话开始后，默认工作流程固定为：
1. 读取 `docs/ARCHITECTURE_ENTRYPOINT.md`
2. 读取相关 spec
3. 读取 `docs/IMPLEMENTATION_ROADMAP.md`
4. 读取 `/TASK.md`
5. 从第一个未完成任务开始做
6. 完成后直接勾掉
7. 继续下一个未完成任务

除非出现 blocker，否则不要：
- 自己改批次顺序
- 自己新增平行主线
- 跳过尚未完成的上游任务
---
## 8. 下一轮启动提示词
下一轮全新对话可直接使用：

> 请把 `docs/ARCHITECTURE_ENTRYPOINT.md` 作为总入口，把 `docs/IMPLEMENTATION_ROADMAP.md` 作为开发顺序，把 `/TASK.md` 作为执行清单。先读取相关规范，再从 `TASK.md` 中第一个未完成项开始实现。每完成一项就直接勾掉一项，不要跳批次，不要重新发明优先级；每完成一个可验证小批次，都运行 `cargo fmt` 和 `cargo check`。如果发现 docs 与代码冲突，以 docs 为准，并做最小必要修正。
---
## 9. 最终结论
AgentJax 当前已经不缺规范，不缺方向，也不缺分层定义。
当前最重要的是：
**停止继续发散，按 `/TASK.md` 和 `docs/IMPLEMENTATION_ROADMAP.md` 分批做完。**
