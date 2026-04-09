# AgentJax Usage、Billing、Scheduler 与 Node 扩展规范草案
## 1. 文档目标
本文档用于定义 AgentJax 下一阶段的三类扩展能力规范：
- Usage / Billing / Reconciliation
- Scheduler / Scheduled Task
- Capability Node / Execution Endpoint
本文档的核心原则是：
1. **核心只记录事实，不猜价格**
2. **计费由插件完成，不由 runtime 硬编码**
3. **调度统一抽象为 Scheduled Task，cron 只是 trigger 的一种**
4. **Node 不是“另一台电脑”，而是可被调度和路由的能力端点**
本文档与已有文档的关系：
- `docs/MVP_PLAN.md`：项目阶段目标与 MVP 路线
- `docs/PLUGIN_SDK.md`：插件 SDK、资源层、能力模型
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`：workspace、config、state、热重载、skills
- `docs/USAGE_BILLING_SCHEDULER_NODE_SPEC.md`：usage/billing/scheduler/node 四块扩展规范
---
## 2. 总体设计原则
### 2.1 核心只记录 usage facts
核心 runtime 负责记录客观运行事实，例如：
- request / response / message 计数
- token 使用情况
- audio/image/video 使用情况
- tool 调用情况
- provider / model / resource 维度
- 时间、延迟、重试、endpoint、project 等维度
核心**不直接推导价格**，也不内置任何特定 provider 的价格规则。
### 2.2 Billing 由插件完成
Billing 插件负责将 usage facts 转换为 money facts。
它可以支持：
- 本地估算
- 按不同 provider / endpoint / region 计价
- 云端账单拉取
- 本地估算与云端实际对账
### 2.3 调度不是 heartbeat vs cron 二选一
调度统一抽象为 **Scheduled Task**。
`cron`、`interval`、`startup`、`event` 等都只是 trigger 的不同类型。
### 2.4 Node 是能力端点，不是机器名录
Node 应被定义为：
**可被调度和路由的能力端点（capability endpoint）**。
一个 node 可以是：
- Linux VM
- 树莓派
- ESP32
- 浏览器代理
- 手机
- 本地 ST/TTS 服务
- MCP server
- 远程 shell executor
Node 是 topology 层与 execution domain，不只是“另一台电脑”。
---
## 3. Usage / Billing 总体模型
建议拆成三层：
1. **Telemetry**
2. **Usage Ledger**
3. **Billing Ledger**
### 3.1 Telemetry
用于运行观测：
- 调用次数
- 耗时
- 成功率
- 错误率
- token 统计
- context window 使用情况
- provider/model 延迟与稳定性
Telemetry 面向：
- observability
- diagnostics
- debugging
- performance analysis
### 3.2 Usage Ledger
用于记录**可计费原始事实**。
它回答的是：
- 这次模型调用到底用了什么资源？
- 它属于哪个 agent / workspace / session / task / plugin / account？
### 3.3 Billing Ledger
用于记录**结算结果**。
它回答的是：
- 本地估算价格是多少？
- provider 实际账单是多少？
- 两者差异是多少？
- 使用了哪套计费规则？
这种三层拆分的价值是：
- telemetry 不污染账单逻辑
- usage facts 可重复结算
- billing 可本地估算也可远程对账
---
## 4. Usage Facts 规范
### 4.1 核心原则
核心 runtime 应产出 usage ledger，但不直接产出“猜测账单”。
### 4.2 建议记录字段
每次资源调用建议记录以下 usage facts：
- `request_count`
- `response_count`
- `message_count`
- `input_tokens`
- `output_tokens`
- `cached_tokens`
- `reasoning_tokens`
- `audio_seconds`
- `image_count`
- `video_count`
- `tool_call_count`
- `context_window_used`
- `max_context_tier_crossed`
- `provider_id`
- `model_id`
- `resource_id`
- `endpoint_id`
- `region`
- `account_id`
- `project_id`
- `workspace_id`
- `agent_id`
- `session_id`
- `task_id`
- `plugin_id`
- `started_at`
- `ended_at`
- `latency_ms`
- `retry_count`
### 4.3 数据模型建议
```rust
pub struct UsageRecord {
    pub usage_id: String,
    pub category: UsageCategory,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub resource_id: String,
    pub endpoint_id: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub plugin_id: Option<String>,
    pub request_count: u32,
    pub response_count: u32,
    pub message_count: u32,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub audio_seconds: Option<f64>,
    pub image_count: Option<u32>,
    pub video_count: Option<u32>,
    pub tool_call_count: Option<u32>,
    pub context_window_used: Option<u64>,
    pub max_context_tier_crossed: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub latency_ms: u64,
    pub retry_count: u32,
}
```
```rust
pub enum UsageCategory {
    ModelText,
    ModelReasoning,
    Embedding,
    Reranker,
    AudioTts,
    AudioSt,
    ToolExecution,
    ChannelDelivery,
    ArtifactStorage,
    NodeExecution,
}
```
---
## 5. Billing 模型
### 5.1 Billing 插件职责
Billing 插件只做一件事：
**把 usage facts 转成 money facts。**
它不负责生成 usage facts。
### 5.2 建议角色拆分
建议设计为：
- `UsageMeter`：产出 usage facts
- `BillingPlugin`：根据 usage facts 计算本地价格
- `RemoteBillingPlugin`：调用 provider API 拉真实账单
- `BillingReconciler`：本地估算与云端账单对比
### 5.3 Billing 支持的计费方式
这套模型应支持：
- 按消息数计费
- 按 token 计费
- 分段计费
- context tier 计费
- 首条免费、后续收费
- 音频按秒
- 图像按张
- endpoint / region 差异定价
- 月套餐 + 超额计费
- provider 实时账单接口
### 5.4 Billing 结果不应只有裸数字
建议账单结果至少包括：
- `amount`
- `currency`
- `mode = estimated | provider_reported | reconciled`
- `rule_id`
- `confidence`
- `breakdown`
### 5.5 数据模型建议
```rust
pub struct BillingRecord {
    pub billing_id: String,
    pub usage_id: String,
    pub amount: rust_decimal::Decimal,
    pub currency: String,
    pub mode: BillingMode,
    pub rule_id: Option<String>,
    pub confidence: BillingConfidence,
    pub breakdown: Vec<BillingBreakdownItem>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
}
```
```rust
pub enum BillingMode {
    Estimated,
    ProviderReported,
    Reconciled,
}
```
```rust
pub enum BillingConfidence {
    Low,
    Medium,
    High,
    Exact,
}
```
```rust
pub struct BillingBreakdownItem {
    pub item_type: String,
    pub quantity: String,
    pub unit_price: Option<String>,
    pub subtotal: String,
    pub note: Option<String>,
}
```
---
## 6. Reconciliation 模型
### 6.1 目标
对账用于比较：
- 本地估算费用
- 云端 provider 实际返回费用
### 6.2 价值
对账能力可用于：
- 计费准确性控制
- provider bug 排查
- 成本优化
- 多 provider 成本对比
### 6.3 推荐字段
```rust
pub struct BillingReconciliation {
    pub reconciliation_id: String,
    pub usage_id: String,
    pub local_estimate_amount: rust_decimal::Decimal,
    pub provider_reported_amount: Option<rust_decimal::Decimal>,
    pub reconciled_amount: Option<rust_decimal::Decimal>,
    pub delta_amount: Option<rust_decimal::Decimal>,
    pub currency: String,
    pub last_reconciled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub provider_reference: Option<String>,
    pub note: Option<String>,
}
```
### 6.4 例子
同一次调用可能有：
- 本地估算：`0.034 USD`
- provider 实际：`0.031 USD`
- delta：`-0.003 USD`
### 6.5 插件职责边界
- 核心负责存 usage ledger
- billing plugin 负责算 estimate
- remote billing plugin 负责取 provider_reported
- reconciler 负责合并与偏差分析
---
## 7. Scheduler 总体模型
### 7.1 核心结论
不要把 heartbeat、cron、interval 分裂成不同系统。
统一抽象成：
**Scheduled Task**
### 7.2 Scheduled Task 的组成
一个 scheduled task 至少应包含：
- `trigger`
- `target`
- `execution_mode`
- `task_template`
- `context_policy`
- `retry_policy`
- `timeout`
- `lease_policy`
- `concurrency_policy`
- `artifact_policy`
- `notify_policy`
- `enabled`
### 7.3 数据模型建议
```rust
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub trigger: TaskTrigger,
    pub target: TaskTarget,
    pub execution_mode: ExecutionMode,
    pub task_template: TaskTemplateRef,
    pub context_policy: Option<ContextPolicyRef>,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
    pub lease_policy: LeasePolicy,
    pub concurrency_policy: ConcurrencyPolicy,
    pub artifact_policy: Option<ArtifactPolicy>,
    pub notify_policy: Option<NotifyPolicy>,
    pub enabled: bool,
}
```
---
## 8. Trigger 模型
### 8.1 触发方式应统一抽象
建议支持以下 trigger 类型：
- `cron`
- `interval`
- `once_at_datetime`
- `startup`
- `webhook_or_event`
- `file_change`
- `external_signal`
- `dependency_complete`
### 8.2 数据模型建议
```rust
pub enum TaskTrigger {
    Cron { expr: String, timezone: Option<String> },
    Interval { seconds: u64 },
    OnceAt { at: chrono::DateTime<chrono::Utc> },
    Startup,
    Event { event_type: String, filter: Option<serde_json::Value> },
    FileChange { path: String, recursive: bool },
    ExternalSignal { signal_name: String },
    DependencyComplete { dependency_task_id: String },
}
```
### 8.3 设计收益
这样以后 AgentJax 拥有的就不是“定时任务”而是完整调度系统。
---
## 9. Execution Mode 模型
### 9.1 为什么要显式 execution mode
调度任务不一定是聊天会话。
必须显式区分任务到底在哪种执行上下文中运行。
### 9.2 建议支持三种 execution mode
#### `ephemeral_session`
- 每次触发新建临时会话
- 执行完成即结束
适合：
- 巡检
- 日报
- 抓取
- 备份
#### `bound_session`
- 绑定到某个长期 session / task thread
- 每次触发在同一上下文延续
适合：
- 长期项目
- 连续代理
- 守护型任务
#### `headless_task`
- 不基于 chat session
- 直接作为任务运行
最适合：
- autonomous bot
- 后台任务
- pipeline 型执行
### 9.3 默认建议
对于 AgentJax 这种自主 runtime，**默认建议使用 `headless_task`**，而不是默认创建 chat session。
### 9.4 数据模型建议
```rust
pub enum ExecutionMode {
    EphemeralSession,
    BoundSession { session_id: String },
    HeadlessTask,
}
```
---
## 10. Task Target 与 Bindings 模型
### 10.1 Target 必须显式
一个任务应明确“归谁执行”。
建议支持：
- 单个 agent
- agent group
- capability-selected agent
- node-selected execution
### 10.2 数据模型建议
```rust
pub enum TaskTarget {
    Agent { agent_id: String },
    AgentGroup { group_id: String },
    CapabilitySelectedAgent { capability: String },
    NodeSelectedExecution { selector: NodeSelector },
}
```
### 10.3 一个任务模板绑定多个执行对象
不要复制粘贴多份任务配置。
建议拆成：
- `TaskDefinition`
- `TaskBinding`
也就是：
- 任务模板定义一次
- 再绑定到多个 agent / node / scope
### 10.4 数据模型建议
```rust
pub struct TaskDefinition {
    pub id: String,
    pub name: String,
    pub task_template: TaskTemplateRef,
    pub default_context_policy: Option<ContextPolicyRef>,
}
```
```rust
pub struct TaskBinding {
    pub binding_id: String,
    pub task_definition_id: String,
    pub target: TaskTarget,
    pub trigger: TaskTrigger,
    pub execution_mode: ExecutionMode,
    pub enabled: bool,
}
```
---
## 11. Node 总体模型
### 11.1 核心定义
**Node = 可被调度和路由的能力端点。**
这意味着 node 不必是完整 agent host，它只需要暴露可用能力。
### 11.2 Node 可以是什么
Node 可以是：
- Linux VM
- 树莓派
- ESP32
- 浏览器代理
- 手机
- 本地 LLM 主机
- 本地 TTS/ST 服务
- MCP server
- 远程 shell executor
### 11.3 为什么 Node 不是“机器注册表”
Node 是 topology 层的执行域与能力端点，不是简单的机器清单。
---
## 12. Node 能力模型
### 12.1 推荐字段
一个 node 建议声明：
- `id`
- `kind`
- `platform`
- `status`（online/offline/degraded）
- `capabilities`
- `resources`
- `labels`
- `cost_profile`
- `trust_level`
- `network_reachability`
### 12.2 数据模型建议
```rust
pub struct NodeDescriptor {
    pub id: String,
    pub kind: NodeKind,
    pub platform: String,
    pub status: NodeStatus,
    pub capabilities: Vec<String>,
    pub resources: Vec<String>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub cost_profile: Option<CostProfileRef>,
    pub trust_level: TrustLevel,
    pub network_reachability: NetworkReachability,
}
```
```rust
pub enum NodeKind {
    VirtualMachine,
    RaspberryPi,
    Esp32,
    BrowserAgent,
    MobileDevice,
    LocalService,
    McpServer,
    Custom(String),
}
```
```rust
pub enum NodeStatus {
    Online,
    Offline,
    Degraded,
}
```
```rust
pub enum TrustLevel {
    Untrusted,
    Limited,
    Trusted,
    Privileged,
}
```
### 12.3 capability 示例
常见 capability：
- `shell`
- `browser`
- `camera`
- `mic`
- `speaker`
- `gpio`
- `serial`
- `ble`
- `zigbee`
- `local-llm`
- `st`
- `tts`
- `vision`
- `storage`
ESP32 也可以是 node，但只提供窄能力：
- `gpio`
- `sensor-read`
- `actuator-control`
- `tiny-http-endpoint`
它不需要被伪装成完整 agent host。
---
## 13. Node Routing 与选择
### 13.1 任务不应直接硬编码机器地址
建议通过 node selector 按能力、成本、信任、位置进行路由。
### 13.2 选择维度建议
- capability
- cost
- trust
- location / region
- online status
- labels
- network reachability
### 13.3 数据模型建议
```rust
pub struct NodeSelector {
    pub required_capabilities: Vec<String>,
    pub preferred_labels: std::collections::BTreeMap<String, String>,
    pub min_trust_level: Option<TrustLevel>,
    pub max_cost_profile: Option<String>,
    pub region: Option<String>,
}
```
### 13.4 设计结果
这样任务就可以表达：
- 由 `ops-bot` 执行
- 或由所有带 `maintenance` capability 的 agent 执行
- 或由满足 `shell + browser + trusted` 的在线 node 执行
---
## 14. 配置目录中的映射建议
与 `docs/WORKSPACE_AND_CONFIG_SPEC.md` 对齐后，建议未来在 config root 中补充：
- `billing.toml`
- `usage.toml`
- `scheduler.toml`
- `nodes.toml`
### 14.1 `usage.toml`
控制：
- usage 采集粒度
- ledger 存储策略
- retention policy
### 14.2 `billing.toml`
控制：
- 启用哪些 billing plugins
- 估算规则版本
- provider bill pull 策略
- reconciliation 周期
### 14.3 `scheduler.toml`
控制：
- scheduled task registry
- trigger 解析
- 默认 execution mode
- lease / retry / concurrency 策略
### 14.4 `nodes.toml`
控制：
- node registry
- static node descriptors
- discovery policy
- trust policy
---
## 15. 与插件 SDK 的关系
这些扩展能力应继续走插件化路线：
- UsageMeterPlugin
- BillingPlugin
- RemoteBillingPlugin
- ReconcilerPlugin
- SchedulerPlugin
- NodeRegistryPlugin
- NodeExecutionPlugin
它们应接入 `PluginManifest`、`PluginContext` 与资源层，而不是做成硬编码核心逻辑。
### 15.1 示例能力补充建议
可考虑在 `PluginCapability` 中补充：
- `Usage`
- `Billing`
- `Scheduler`
- `NodeRegistry`
- `NodeExecution`
---
## 16. MVP 与分阶段实现建议
虽然规范可以先定大，但实现上需要收敛。
### Phase 1：只定义模型，不接真实计费
优先做：
- `UsageRecord`
- `BillingRecord`
- `BillingReconciliation`
- `ScheduledTask`
- `TaskTrigger`
- `ExecutionMode`
- `NodeDescriptor`
- `NodeSelector`
### Phase 2：最小可运行骨架
- 在 runtime 中记录 usage ledger
- 本地 billing estimator 插件雏形
- scheduler registry 雏形
- static node registry 雏形
### Phase 3：高级能力
- provider remote billing 拉取
- reconciliation job
- headless task runner
- capability-based node routing
- distributed lease / checkpointing
### 当前不建议立即做
- 完整账单对账 UI
- 多区域动态 node discovery
- 分布式调度集群
- 复杂月套餐计费引擎
---
## 17. 下一轮 Code 模式建议优先级
如果下一轮进入 Code 模式，应优先保证核心架构先落地，再逐步预留这些扩展点。
### P0
1. 在 domain/core 中为 usage、billing、scheduler、node 定义基础类型
2. 把这些类型和 `PluginCapability` / `WorkspaceRuntime` 对齐
3. 不实现复杂逻辑，只先建立协议边界
### P1
4. 预留 usage ledger 存储接口
5. 预留 scheduler registry 接口
6. 预留 node registry / selector 接口
### P2
7. 做最小 billing estimator plugin 雏形
8. 做最小 scheduled task registry 雏形
9. 做最小 static node registry 雏形
### 当前不建议立即实现
- 云端对账
- 真正分布式 node 路由
- 全量计费策略 DSL
- 复杂调度恢复语义
---
## 18. 总结
当前收敛后的正确方向是：
- 核心记录 usage ledger，不内置价格学
- billing 插件把 usage facts 转成 money facts
- 支持本地估算 + 云端账单 + reconciliation
- 调度统一成 scheduled task，cron 只是 trigger 的一种
- 默认 execution mode 应偏向 `headless_task`
- node 是 capability endpoint registry，而不是机器列表
一句话总结：
**Usage 归核心事实层，Billing 归插件结算层，Scheduler 归统一任务系统，Node 归能力路由与执行域。**
