# AgentJax 插件 SDK 架构草案
## 1. 文档目标
本文档用于定义 AgentJax 下一阶段要采用的 **OpenClaw 风格核心 + 插件化 Runtime + Workspace 中心** 架构中的插件 SDK 设计原则。
目标不是“支持几个工具扩展”，而是把插件提升为整个 runtime 的统一扩展面，使以下能力都能通过统一协议接入：
- 模型 Provider
- RAG / Retrieval
- Knowledge
- Memory
- Context Engine
- Tool / Executor / MCP
- 渠道适配器
- Node / Worker
- Skill
- Command / Admin
- Hook / Event Subscription
- UI
- Workflow / Scheduler
同时，插件作者应拿到的是 **平台能力抽象**，而不是底层 provider 细节、配置读取、密钥管理、HTTP 细节与 fallback 逻辑。

这里必须明确一个边界：
- `Channel` = 外部消息平台接入
- `Surface` = CLI / TUI / WebUI 等人机界面
- `Transport` = unix socket / websocket / http / stdio 等承载协议

不要把三者混成一个词。
---
## 2. 设计目标
### 2.1 核心目标
1. **核心稳定，插件演进**
   - Core 只负责协议、生命周期、注册、调度、资源访问。
   - 插件负责具体能力实现。
2. **能力统一声明**
   - 一个插件可以提供多个 capability。
   - 所有能力必须显式注册，不允许隐式挂载。
3. **资源统一访问**
   - 插件通过 `PluginContext` 访问模型、rag、knowledge、memory、tools、channels、artifacts 等平台资源。
   - 插件不自己管理 provider 细节。
4. **Workspace 一等公民**
   - 插件在 workspace 中启用。
   - workspace 决定能力边界、权限、资源映射和 profile。
5. **结构化 Hook 与事件系统**
   - runtime 钩子必须以结构化对象驱动，而不是字符串 prompt 魔法。
6. **先做静态注册，再做动态装载**
   - 第一阶段先支持编译期/应用启动期插件注册。
   - 后续再扩展动态发现、远程插件、沙箱插件。
---
## 3. 总体分层
```text
Application Host
  -> Plugin Registry
  -> Workspace Runtime
      -> Resource Layer
      -> Context Engine
      -> Tool Router
      -> Hook Bus / Event Bus
      -> Session / RAG / Memory / Artifact Stores
      -> Agent Backend (rig 等)
  -> Channel Adapters
  -> Core Surfaces
  -> Built-in Transports
```
### 分层说明
#### Core
负责：
- `Plugin` 协议
- `PluginManifest`
- `PluginRegistry`
- `Workspace`
- `WorkspaceRuntime`
- `PluginContext`
- `ResourceRegistry`
- `HookBus`
- `EventBus`
#### Plugin SDK
负责：
- 插件能力模型
- 生命周期
- resource client API
- hook 注册方式
- workspace 启用协议
#### Plugins
负责：
- Telegram adapter
- rig / OpenAI backend
- SQLite session store
- file memory/context
- shell/read_file/list_files tools
- ST/TTS provider
---
## 4. 插件能力模型（Capability Model）
插件不是单类型，而是 **一个插件可声明多个能力**。
### 4.1 顶层能力分类
```rust
pub enum PluginCapability {
    Provider(ProviderCapability),
    Rag(RagCapability),
    Knowledge(KnowledgeCapability),
    Memory(MemoryCapability),
    Context(ContextCapability),
    Tool(ToolCapability),
    Channel(ChannelCapability),
    Node(NodeCapability),
    Skill(SkillCapability),
    Command(CommandCapability),
    Hook(HookCapability),
    Ui(UiCapability),
    Workflow(WorkflowCapability),
}
```
### 4.2 ProviderPlugin
用于模型与媒体提供能力：
- LLM
- Embedding
- Reranker
- TTS
- ST
```rust
pub enum ProviderCapability {
    LlmText,
    LlmReasoning,
    Embedding,
    Reranker,
    Tts,
    St,
}
```
### 4.3 RagPlugin
用于通用检索基础设施：
- collection abstraction
- indexing
- retrieval
- rerank / expansion hooks
- backend drivers
```rust
pub enum RagCapability {
    Query,
    Indexing,
    BackendDriver,
    EvidencePack,
}
```
### 4.4 KnowledgePlugin
用于特定知识域系统：
- project knowledge base
- product docs
- api docs
- notes / artifacts / code corpora
```rust
pub enum KnowledgeCapability {
    Corpus,
    IngestPolicy,
    RetrievalPolicy,
}
```
### 4.5 MemoryPlugin
用于长期语义记忆能力：
- promotion
- recall
- conflict resolution
- freshness / invalidation
- archival storage
```rust
pub enum MemoryCapability {
    Recall,
    Promotion,
    ConflictResolution,
    FreshnessPolicy,
    Archive,
}
```
### 4.6 ContextPlugin
用于上下文构造与选择：
- context block generation
- context selection
- prompt rendering policy
```rust
pub enum ContextCapability {
    BlockGenerator,
    Selector,
    PromptRenderer,
}
```
### 4.7 ToolPlugin
用于工具与执行器：
- local tools
- remote executors
- MCP bridge
```rust
pub enum ToolCapability {
    Tool,
    Executor,
    McpBridge,
}
```
### 4.8 ChannelPlugin
用于外部消息平台渠道：
- Telegram
- Discord
- QQ
- Email
- Slack / Webhook 等
```rust
pub enum ChannelCapability {
    Telegram,
    Discord,
    Qq,
    Email,
    Slack,
    Webhook,
}
```
### 4.9 NodePlugin
用于远程 worker / machine / browser 节点：
```rust
pub enum NodeCapability {
    RemoteWorker,
    MachineNode,
    DeviceNode,
    BrowserNode,
}
```
### 4.10 SkillPlugin
用于 skills：
- skill manifest
- skill loading
- trigger routing
```rust
pub enum SkillCapability {
    SkillManifest,
    SkillLoader,
    TriggerRouter,
}
```
### 4.11 CommandPlugin
用于 CLI / admin / diagnostics：
```rust
pub enum CommandCapability {
    CliCommand,
    AdminOperation,
    Diagnostic,
}
```
### 4.12 HookPlugin
用于生命周期与事件订阅：
```rust
pub enum HookCapability {
    Lifecycle,
    EventSubscription,
}
```
### 4.13 UIPlugin
用于 dashboard / debug pane / inspector 等可嵌入 UI 扩展，不用于 TUI / WebUI runtime surface 本体：
```rust
pub enum UiCapability {
    DashboardPane,
    Inspector,
    DebugView,
}
```
### 4.14 WorkflowPlugin
用于调度与自动化：
```rust
pub enum WorkflowCapability {
    Scheduler,
    Automation,
    RecurringJob,
}
```
---
## 5. 运行时模型（Runtime Model）
插件统一接入方式建议围绕以下对象：
- `Plugin`
- `PluginManifest`
- `PluginRuntime`
- `PluginContext`
- `PluginRegistry`
- `PluginManager`
### 5.1 Plugin
```rust
#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;
    async fn on_load(&self, _ctx: PluginContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn on_startup(&self, _ctx: PluginContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn on_shutdown(&self, _ctx: PluginContext) -> anyhow::Result<()> {
        Ok(())
    }
}
```
说明：
- 所有插件都有统一 manifest。
- 生命周期方法提供默认空实现。
- 高阶能力通过附加 trait 暴露，而不是把所有方法塞进一个 trait。
- `Unix socket` / `WebSocket` server 不属于普通插件能力，属于 daemon 内建 transport。
### 5.2 PluginManifest
```rust
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub capabilities: Vec<PluginCapability>,
    pub config_schema: Option<serde_json::Value>,
    pub required_permissions: Vec<Permission>,
    pub dependencies: Vec<String>,
    pub optional_dependencies: Vec<String>,
    pub provided_resources: Vec<ResourceDescriptor>,
    pub hooks: Vec<HookPoint>,
}
```
字段说明：
- `id`: 插件唯一标识
- `version`: 版本
- `capabilities`: 显式声明能力
- `config_schema`: 插件配置 schema
- `required_permissions`: 权限申请
- `dependencies`: 强依赖插件
- `optional_dependencies`: 可选依赖
- `provided_resources`: 对外提供的资源
- `hooks`: 插件声明要监听的 hook 点
### 5.3 PluginRegistry
注册中心负责：
- 注册插件实例
- 按 capability 查询插件
- 校验依赖
- 构建 workspace 可用插件集

`PluginRegistry` 负责索引与查询，不再作为生命周期执行者；`on_load` / `on_startup` / `on_shutdown`、enable/disable 判定、reload 行为与运行态诊断由 `PluginManager` 统一驱动。

```rust
pub struct PluginRegistry {
    plugins: Vec<std::sync::Arc<dyn Plugin>>,
}
```
第一阶段建议：
- 手动注册
- 启动时校验 manifest
- 支持按 capability 枚举
后续阶段：
- 动态加载
- 远程插件
- 隔离执行
- 版本解析与兼容策略
### 5.4 PluginManager
当前实现中，`PluginManager` 是插件运行时控制面：
- 读取并解释 `plugins.toml` 的 enabled / disabled / config refs / policy flags / reload hints
- 基于依赖关系决定启动顺序
- 驱动 `on_load` / `on_startup` / `on_shutdown`
- 维护 `Discovered` / `Disabled` / `Loading` / `Loaded` / `Starting` / `Running` / `Stopping` / `Stopped` / `Failed` 状态
- 暴露 daemon 控制面所需的 inspect / reload / test 快照与诊断

这意味着插件作者只需要实现 manifest 与生命周期；是否启用、何时重载、失败如何向外暴露，统一由 runtime 侧治理。
---
## 6. PluginContext：插件作者真正拿到的能力
这是 SDK 最关键的对象。
### 6.1 设计原则
插件不应自己：
- 读取全局配置决定默认 provider
- 管理 API Key
- 手写 HTTP 请求
- 处理 provider fallback
- 处理重试、计费、审计、超时
这些都应由 runtime 统一提供。
### 6.2 PluginContext 应包含
```rust
pub struct PluginContext {
    pub app: AppRuntimeHandle,
    pub workspace: WorkspaceHandle,
    pub session: SessionHandle,
    pub turn: TurnHandle,
    pub resources: ResourceRegistryHandle,
    pub models: ModelClient,
    pub audio: AudioClient,
    pub rag: RagClient,
    pub knowledge: KnowledgeClient,
    pub memory: MemoryClient,
    pub tools: ToolClient,
    pub artifacts: ArtifactClient,
    pub channels: ChannelClient,
    pub events: EventBusHandle,
    pub hooks: HookBusHandle,
    pub config: ConfigHandle,
    pub secrets: SecretStoreHandle,
    pub logger: PluginLogger,
    pub cancel: CancellationToken,
    pub scheduler: SchedulerHandle,
}
```
### 6.3 插件使用体验目标
插件内应能直接调用：
```rust
ctx.models.generate(...)
ctx.models.embed(...)
ctx.rag.query(...)
ctx.knowledge.search(...)
ctx.audio.tts(...)
ctx.audio.st(...)
ctx.memory.recall(...)
ctx.memory.append_event(...)
ctx.tools.call(...)
ctx.artifacts.put(...)
ctx.channels.send(...)
```
而不是自己拼 provider。
---
## 7. 资源抽象层（Resource Layer）
这是 SDK 的核心竞争力。
### 7.1 资源类别
建议使用统一资源命名：
- `model:text`
- `model:reasoning`
- `model:embedding`
- `model:reranker`
- `model:expander`
- `audio:tts`
- `audio:st`
- `store:fts`
- `store:knowledge`
- `store:memory`
- `store:vector`
- `store:artifact`
- `exec:shell`
- `exec:browser`
- `net:http`
- `channel:telegram`
- `channel:discord`
- `transport:unix_socket`
- `transport:websocket`
### 7.2 资源声明
插件 manifest 可声明需求：
```toml
requires = [
  "model:text",
  "audio:st",
  "store:artifact"
]
```
runtime 根据全局配置与 workspace 策略进行绑定。
### 7.3 资源抽象的收益
1. 插件不关心底层 provider 是 OpenAI、Gemini 还是本地 provider
2. fallback、routing、timeout、retry 可统一处理
3. 统一审计、配额、计费、日志
4. 插件间资源复用能力更强
5. 可在 workspace 维度限制资源使用
### 7.4 MVP 阶段建议先实现的资源客户端
第一轮不必一步到位，优先抽象：
- `ModelClient`
- `RagClient`
- `KnowledgeClient`
- `ToolClient`
- `MemoryClient`
- `ArtifactClient`
- `ChannelClient`
第二轮再补：
- `AudioClient`
- `SchedulerHandle`
- `NodeClient`
- `HttpClient`
---
## 8. Hook 体系
如果 Hook 不统一，后面一定会退化为屎山。
### 8.1 生命周期 Hook
- `on_load`
- `on_startup`
- `on_shutdown`
- `on_config_changed`
### 8.2 会话 / 回合 Hook
- `before_turn`
- `after_turn`
- `before_model_request`
- `after_model_response`
- `before_tool_call`
- `after_tool_call`
- `before_context_build`
- `after_context_build`
- `before_memory_commit`
- `after_memory_commit`
### 8.3 事件 Hook
- `on_message`
- `on_task_created`
- `on_task_started`
- `on_task_failed`
- `on_task_succeeded`
- `on_schedule_tick`
- `on_artifact_created`
### 8.4 关键原则
Hook 输入必须是结构化对象，例如：
```rust
pub struct BeforeToolCall {
    pub workspace_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
}
```
而不是让插件只收到一段 prompt 字符串。
---
## 9. Skill 不应只是 Markdown
Skill 应升级为结构化协议，而不是单纯 `skills/foo.md`。
### 9.1 Skill 结构建议
- skill manifest
- trigger rules
- loading policy
- input contract
- execution recipe
- output contract
- permissions
- optional tool bindings
### 9.2 SkillPlugin 建议职责
- 注册 skill manifest
- 提供 skill loader
- 处理 trigger routing
- 支持显式调用 / 自动匹配 / command 触发
### 9.3 MVP 阶段建议
第一阶段先支持：
- 文件型 skill manifest（YAML / JSON / TOML）
- skill metadata 解析
- 显式调用路由
后续再支持：
- 复杂触发规则
- skill versioning
- skill marketplace / registry
---
## 10. 与当前 AgentJax 项目的映射关系
当前项目已有这些模块：
- `src/agent/*`
- `src/session/*`
- `src/tools/*`
- `src/adapters/*`
- `src/infra/*`
- `src/domain/*`
它们可以迁移到以下新结构：
### 当前 -> 目标
- `src/agent/builder.rs`
  - -> `src/core/runtime.rs`
  - -> `src/plugins/openai/plugin.rs`
- `src/agent/context.rs`
  - -> `src/core/context_engine.rs`
  - -> `src/builtin/context/*`
- `src/agent/memory.rs`
  - -> `src/builtin/context/retrieval_bridge.rs`
- `src/session/sqlite.rs`
  - -> `src/builtin/storage/sqlite/sessions.rs`
- `src/tools/*`
  - -> `src/builtin/tools/*`
- `src/adapters/telegram.rs`
  - -> `src/plugins/telegram/plugin.rs`
- `src/infra/llm.rs`
  - -> `src/plugins/openai/plugin.rs`
---
## 11. 推荐目录结构
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
    provider.rs
    runtime.rs
    workspace.rs
  core/
    mod.rs
    plugin.rs
    plugin_manager.rs
    registry.rs
    runtime.rs
    workspace_runtime.rs
    resource_registry.rs
    hook_bus.rs
    event_bus.rs
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
    resource.rs
    plugin.rs
    usage.rs
    billing.rs
  context_engine/
    mod.rs
    engine.rs
    assembler.rs
    prompt.rs
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
```
---
## 12. MVP 阶段建议收敛范围
尽管 SDK 设计可以很大，但第一轮 Code 重构不能全做。
### 12.1 Phase 1：先建立骨架
必须先做：
- `PluginManifest`
- `Plugin`
- `PluginRegistry`
- `Workspace`
- `WorkspaceRuntime`
- `PluginContext` 最小版
- `ResourceRegistry` 最小版
- `ToolPlugin` / `ContextPlugin` / `ChannelPlugin` / `AgentBackendPlugin` / `SessionStore` trait
### 12.2 Phase 2：适配当前已有实现
把已有代码挂进去：
- `SqliteSessionStore` -> builtin storage substrate
- `Workspace / retrieval context providers` -> builtin context providers
- `OpenAI provider` -> real provider plugin
- `read_file` / `list_files` / `shell_exec` -> tool plugins
- Telegram adapter -> channel plugin
### 12.3 Phase 3：补资源统一层
逐步加入：
- `ModelClient`
- `MemoryClient`
- `ToolClient`
- `ArtifactClient`
- `ChannelClient`
### 12.4 Phase 4：扩展 Hook / Skills / Audio
后续补：
- HookBus
- Skill manifest
- ST / TTS provider plugin
- Workflow / scheduler
---
## 13. 下一轮 Code 模式优先实现顺序
建议下一轮 Code 模式只做“第一刀结构化重构”，不要继续功能扩张。
### 优先级 P0
1. 创建 `src/core/`
2. 定义 `PluginManifest` / `Plugin` / `PluginRegistry`
3. 定义 `Workspace` / `WorkspaceRuntime`
4. 把 `AgentRuntime` 之类的核心 trait 从 `agent/builder.rs` 移出
5. 定义最小版 `PluginContext`
### 优先级 P1
6. 把 `SqliteSessionStore` 改成 storage plugin
7. 把 `FsContextProvider` 改成 context plugin
8. 把 `RigAgentRuntime` 改成 backend plugin
9. 把 `read_file` / `list_files` / `shell_exec` 改成 tool plugin
### 优先级 P2
10. `Application` 改成 plugin host
11. 建立最小可用的 `ResourceRegistry`
12. 保持 `cargo check` 通过
### 本轮不建议做
- 动态插件加载
- UI 插件
- Workflow 插件
- Skill 自动路由
- ST/TTS 真实接入
- 多 provider fallback 系统
---
## 14. 针对当前项目的具体落地原则
### 原则 1
rig 不是系统核心，而是 backend/plugin。
### 原则 2
所有外设能力都走插件：
- Telegram
- tools
- context
- session storage
- ST/TTS
- memory
### 原则 3
插件拿的是 runtime resources，不是 provider 原始配置。
### 原则 4
workspace 决定启用哪些插件与资源映射。
### 原则 5
先做“静态注册 + 单 workspace + 单 provider”，但接口必须允许未来升级。
---
## 15. 建议加入到下一轮实现任务中的验收标准
下一轮 Code 模式完成后，至少应满足：
1. 项目中出现 `src/core/` 基础骨架
2. 存在 `PluginManifest` / `Plugin` / `PluginRegistry`
3. 存在 `WorkspaceRuntime`
4. 至少一个 storage plugin、一个 context plugin、一个 backend plugin、一个 tool plugin 已完成适配
5. `Application` 能注册这些插件
6. 项目仍可 `cargo check`
---
## 16. 总结
这套插件 SDK 的核心思想是：
**插件扩展的是整个 runtime，而不是只扩展工具列表。**
对于 AgentJax，正确方向不是继续围绕 `agent/builder.rs` 堆逻辑，而是尽快建立：
- Core
- Plugin Registry
- Workspace Runtime
- Resource Layer
- Hook / Event 模型
然后把 rig、Telegram、tools、memory、session、ST/TTS 都纳入统一插件协议。
这会让项目从“能跑的 Rust Agent Demo”，升级为“可演化的 Agent Runtime 平台”。
