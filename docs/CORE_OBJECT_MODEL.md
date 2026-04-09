# AgentJax Core Object Model Spec
## 1. 目标
本文档定义 AgentJax 的核心对象模型，用于钉死未来最容易腐烂的基础契约。
目标是避免系统后续退化成：
- 到处临时 struct
- 大量匿名 JSON blob
- 同一概念在不同模块里字段不一致
- event、task、plugin、resource、artifact 等对象边界混乱
本文档是 AgentJax 的 **Spec 1：Core Object Model**。
配套文档：
- `docs/WORKSPACE_AND_CONFIG_SPEC.md` = Spec 2
- `docs/PLUGIN_SDK.md` = Spec 3
- `docs/EVENT_TASK_LCM_RUNTIME.md` = Spec 4
---
## 2. 建模原则
### 2.1 一等公民优先
以下对象必须被定义为一等公民，而不是散落在实现细节里：
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
### 2.2 标识稳定
所有核心对象都应具备稳定 ID，并允许引用链追踪。
### 2.3 元数据可扩展，但不是主结构
允许 `metadata` / `labels` / `extensions`，但核心字段必须先结构化定义。
### 2.4 关系显式
对象之间的依赖关系需要明确，如：
- `Turn` 属于 `Session`
- `Task` 可关联 `Session`
- `Artifact` 应回溯到 `source_event_id`
- `SummaryNode` 必须可回溯 source handles
### 2.5 状态与身份分离
身份对象与运行状态对象分离，不要混装。
例如：
- `Agent` 是身份与行为边界
- `Session` / `Task` 是运行态
---
## 3. 通用字段约定
建议所有核心对象共享以下约定：
```rust
pub struct ObjectMeta {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub schema_version: String,
}
```
### 3.1 字段说明
- `id`: 全局或作用域内稳定唯一 ID
- `created_at` / `updated_at`: 生命周期时间戳
- `labels`: 轻量过滤与路由标签
- `metadata`: 扩展字段容器
- `schema_version`: 对象自身 schema 版本
### 3.2 ID 原则
建议使用：
- ULID / UUIDv7
- 或带前缀的可读 ID，如 `agent_xxx`、`task_xxx`
---
## 4. Agent
### 4.1 定义
`Agent` 是系统中的自治主体定义，表达：
- 身份
- 使命
- 自治边界
- workspace 关联
- 默认资源路由
### 4.2 建议模型
```rust
pub struct Agent {
    pub meta: ObjectMeta,
    pub agent_id: String,
    pub display_name: String,
    pub workspace_id: String,
    pub profile_ref: Option<String>,
    pub mission_ref: Option<String>,
    pub rules_ref: Option<String>,
    pub router_ref: Option<String>,
    pub default_resource_bindings: Vec<ResourceBindingRef>,
    pub autonomy_policy: AutonomyPolicy,
    pub status: AgentStatus,
}
```
### 4.3 关键字段
- `workspace_id`: 指向 agent 的自我与长期知识域
- `autonomy_policy`: 约束 agent 能否自主发消息、花钱、建任务、改 memory 等
- `default_resource_bindings`: 默认资源映射
### 4.4 状态建议
```rust
pub enum AgentStatus {
    Active,
    Suspended,
    Draining,
    Disabled,
}
```
---
## 5. Session
### 5.1 定义
`Session` 是对话或交互线程的持续上下文容器。
### 5.2 建议模型
```rust
pub struct Session {
    pub meta: ObjectMeta,
    pub session_id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub channel_id: Option<String>,
    pub user_id: Option<String>,
    pub title: Option<String>,
    pub mode: SessionMode,
    pub status: SessionStatus,
    pub last_turn_id: Option<String>,
}
```
```rust
pub enum SessionMode {
    Interactive,
    BackgroundBound,
    Imported,
}
```
```rust
pub enum SessionStatus {
    Active,
    Idle,
    Closed,
    Archived,
}
```
---
## 6. Turn
### 6.1 定义
`Turn` 是一次完整处理循环，覆盖从输入接收、上下文构造、模型调用、工具调用到输出生成的闭环。
### 6.2 建议模型
```rust
pub struct Turn {
    pub meta: ObjectMeta,
    pub turn_id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: String,
    pub input_event_id: String,
    pub status: TurnStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub summary: Option<TurnSummary>,
}
```
```rust
pub enum TurnStatus {
    Started,
    ContextBuilt,
    Running,
    WaitingTool,
    Succeeded,
    Failed,
    Cancelled,
}
```
---
## 7. Task
### 7.1 定义
`Task` 是可执行工作单元，可由用户触发、调度触发、事件触发或 agent 自主创建。
### 7.2 建议模型
```rust
pub struct Task {
    pub meta: ObjectMeta,
    pub task_id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub definition_ref: Option<String>,
    pub execution_mode: ExecutionMode,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub goal: String,
    pub checkpoint_ref: Option<String>,
}
```
```rust
pub enum TaskStatus {
    Pending,
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}
```
```rust
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}
```
---
## 8. Event
### 8.1 定义
`Event` 是整个系统的统一事实记录单元。
所有运行时关键动作都应落到 event log。
### 8.2 最低事件类型
至少应定义：
- `message_received`
- `turn_started`
- `context_built`
- `model_called`
- `tool_called`
- `tool_completed`
- `artifact_created`
- `task_started`
- `task_succeeded`
- `task_failed`
- `memory_committed`
- `summary_compacted`
- `plugin_loaded`
- `schedule_triggered`
- `node_status_changed`
### 8.3 建议模型
```rust
pub struct Event {
    pub meta: ObjectMeta,
    pub event_id: String,
    pub event_type: EventType,
    pub workspace_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub task_id: Option<String>,
    pub source_plugin_id: Option<String>,
    pub source_node_id: Option<String>,
    pub payload: serde_json::Value,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
}
```
---
## 9. ContextBlock
### 9.1 定义
`ContextBlock` 是上下文引擎生成的结构化上下文片段，而不是简单字符串拼接结果。
### 9.2 建议模型
```rust
pub struct ContextBlock {
    pub block_id: String,
    pub kind: ContextBlockKind,
    pub source: ContextSource,
    pub priority: u32,
    pub token_estimate: Option<u32>,
    pub freshness: Option<Freshness>,
    pub confidence: Option<Confidence>,
    pub content: String,
}
```
```rust
pub enum ContextBlockKind {
    StableIdentity,
    Mission,
    Rule,
    UserProfile,
    Memory,
    RetrievedKnowledge,
    RecentEvent,
    ToolTrace,
    TaskPlan,
    Summary,
    SkillInstruction,
}
```
---
## 10. ToolCall
### 10.1 定义
`ToolCall` 是工具执行请求的结构化表示。
### 10.2 建议模型
```rust
pub struct ToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub requested_by: ToolCaller,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub turn_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub timeout_secs: Option<u64>,
}
```
```rust
pub enum ToolCaller {
    Agent { agent_id: String },
    Plugin { plugin_id: String },
    Operator { operator_id: String },
    Scheduler,
}
```
---
## 11. Artifact
### 11.1 定义
`Artifact` 是运行期生成或接入的统一产物对象，不只是一个文件路径。
### 11.2 建议模型
```rust
pub struct Artifact {
    pub meta: ObjectMeta,
    pub artifact_id: String,
    pub producer: ArtifactProducer,
    pub mime: String,
    pub uri: String,
    pub size_bytes: Option<u64>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub source_event_id: Option<String>,
    pub retention_policy: Option<RetentionPolicy>,
    pub tags: Vec<String>,
}
```
### 11.3 作用
统一管理：
- TTS 输出
- 下载文件
- 导出报告
- 图像 diff
- ST 中间产物
- 工具结果文件
---
## 12. Node
### 12.1 定义
`Node` 是可被调度和路由的能力端点。
### 12.2 建议模型
```rust
pub struct Node {
    pub meta: ObjectMeta,
    pub node_id: String,
    pub kind: NodeKind,
    pub platform: String,
    pub status: NodeStatus,
    pub capabilities: Vec<String>,
    pub resources: Vec<String>,
    pub trust_level: TrustLevel,
    pub labels: std::collections::BTreeMap<String, String>,
}
```
---
## 13. Resource
### 13.1 定义
`Resource` 是 runtime 可绑定的能力入口，统一命名，不暴露底层 provider 细节给业务层。
### 13.2 命名建议
例如：
- `llm.text.default`
- `llm.embedding.default`
- `audio.tts.default`
- `audio.st.default`
- `exec.shell.local`
- `channel.telegram.main`
### 13.3 建议模型
```rust
pub struct Resource {
    pub resource_id: String,
    pub resource_kind: ResourceKind,
    pub binding_target: String,
    pub capabilities: Vec<String>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub status: ResourceStatus,
}
```
---
## 14. Plugin
### 14.1 定义
`Plugin` 是 runtime 扩展单元，可声明多个 capability。
### 14.2 建议模型
```rust
pub struct PluginDescriptor {
    pub plugin_id: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub api_version: String,
    pub status: PluginStatus,
}
```
```rust
pub enum PluginStatus {
    Loaded,
    Initialized,
    Running,
    Draining,
    Stopped,
    Failed,
}
```
---
## 15. Skill
### 15.1 定义
`Skill` 是可被 agent 使用的结构化能力包，包含自然语言说明与结构化 manifest。
### 15.2 建议模型
```rust
pub struct Skill {
    pub meta: ObjectMeta,
    pub skill_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub manifest_ref: Option<String>,
    pub markdown_ref: Option<String>,
    pub compatibility_version: String,
    pub triggers: Vec<SkillTrigger>,
}
```
---
## 16. Schedule
### 16.1 定义
`Schedule` 是调度定义对象，驱动定时或事件驱动任务。
### 16.2 建议模型
```rust
pub struct Schedule {
    pub meta: ObjectMeta,
    pub schedule_id: String,
    pub name: String,
    pub trigger: TaskTrigger,
    pub target: TaskTarget,
    pub enabled: bool,
}
```
---
## 17. SummaryNode
### 17.1 定义
`SummaryNode` 是 LCM/summary graph 的基础节点，用于承载压缩后的长期上下文表示。
### 17.2 建议模型
```rust
pub struct SummaryNode {
    pub meta: ObjectMeta,
    pub summary_node_id: String,
    pub workspace_id: String,
    pub summary_type: SummaryType,
    pub content: String,
    pub source_event_ids: Vec<String>,
    pub source_artifact_ids: Vec<String>,
    pub confidence: Confidence,
    pub freshness: Freshness,
    pub invalidation_status: InvalidationStatus,
}
```
### 17.3 为什么重要
如果没有结构化 `SummaryNode`，后续 LCM 会退化成不可追踪的 prompt 文本缓存。
---
## 18. 身份与作用域对象
建议显式建模以下 identity：
- `AgentIdentity`
- `OperatorIdentity`
- `ProviderAccountIdentity`
- `NodeIdentity`
- `PluginIdentity`
### 密钥作用域至少区分
- `global`
- `per-agent`
- `per-plugin`
- `per-node`
- `per-session-temporary`
这部分是避免：
- 串号
- 串账
- 串权限
- 串 provider account
---
## 19. 错误对象建议
统一错误对象结构：
```rust
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub category: ErrorCategory,
    pub source: Option<String>,
    pub details: serde_json::Value,
    pub cause_chain: Vec<String>,
}
```
```rust
pub enum ErrorCategory {
    ConfigError,
    ProviderError,
    NetworkError,
    AuthError,
    Timeout,
    RateLimit,
    ToolFailure,
    PluginFailure,
    StateConflict,
    BudgetExceeded,
}
```
---
## 20. 版本化要求
以下 schema version 必须存在：
- workspace schema version
- config schema version
- plugin API version
- state schema version
- event schema version
- skill spec version
如果不做版本化，长期演进必然崩。
---
## 21. 最小落地建议
下一轮 Code 模式先不要把所有对象都完全实现，只需优先把类型边界钉死。
### P0
- `Agent`
- `Session`
- `Turn`
- `Task`
- `Event`
- `ContextBlock`
- `ToolCall`
- `Artifact`
### P1
- `Node`
- `Resource`
- `PluginDescriptor`
- `Skill`
- `Schedule`
- `SummaryNode`
### P2
- 统一 `RuntimeError`
- 统一 `ObjectMeta`
- 统一 schema version 字段
---
## 22. 总结
Core Object Model 的目标不是一次性做完全部实现，而是把所有关键对象先钉成稳定契约。
一句话总结：
**先定义对象，再写代码；先钉边界，再长功能。**
