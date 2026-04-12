# AgentJax Event / Task / LCM Runtime Spec
## 1. 目标
本文档定义 AgentJax 的 **Spec 4：Event / Task / LCM Runtime**，用于钉死执行流、事件流、任务流与压缩流的基础契约。
本规范重点解决以下问题：
- 整个系统到底记录什么事件
- 一个 turn / task 是如何运行的
- 调度任务如何进入 runtime
- LCM（Long Context Management）如何压缩、失效、修正与回溯
- 哪些边界必须先定义，避免后期变成不可调试的屎山
配套文档：
- `docs/CORE_OBJECT_MODEL.md` = Spec 1
- `docs/WORKSPACE_AND_CONFIG_SPEC.md` = Spec 2
- `docs/PLUGIN_SDK.md` = Spec 3
---
## 2. 核心原则
### 2.1 一切关键动作都应事件化
系统的关键动作必须落入统一 event log，而不是只写散乱日志。
事件化的价值在于：
- observability
- replay
- audit
- debugging
- timeline reconstruction
- LCM source tracing
### 2.2 Turn、Task、Event 三者分离
- `Event`：事实记录
- `Turn`：一次处理闭环
- `Task`：可调度、可恢复、可委派的工作单元
### 2.3 LCM 不是 prompt 拼接优化，而是运行时原生能力
LCM 必须原生支持：
- summary compaction
- invalidation
- contradiction marker
- stale marker
- recompute trigger
- source expansion handle
- confidence / freshness rules
### 2.4 Runtime 先定义协议，再定义策略
先钉死：
- 事件类型
- 任务生命周期
- turn 生命周期
- LCM 节点与修正协议
- 重试 / 幂等 / 熔断边界
然后再实现具体策略。
---
## 3. 统一事件模型
### 3.1 最低事件类型
AgentJax 至少需要以下标准事件：
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
### 3.2 推荐扩展事件
建议同时预留：
- `turn_succeeded`
- `turn_failed`
- `model_response_received`
- `tool_failed`
- `sleep_requested`
- `task_waiting`
- `task_resumed`
- `shell_session_opened`
- `shell_session_closed`
- `shell_execution_started`
- `shell_output_appended`
- `shell_execution_completed`
- `shell_execution_interrupted`
- `task_checkpointed`
- `task_cancelled`
- `summary_invalidated`
- `summary_recomputed`
- `plugin_reloaded`
- `plugin_drained`
- `resource_bound`
- `billing_recorded`
- `usage_recorded`
### 3.3 事件字段建议
```rust
pub struct RuntimeEvent {
    pub event_id: String,
    pub event_type: EventType,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub task_id: Option<String>,
    pub plugin_id: Option<String>,
    pub node_id: Option<String>,
    pub source: EventSource,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub payload: serde_json::Value,
    pub schema_version: String,
}
```
### 3.4 事件源建议
```rust
pub enum EventSource {
    User,
    Agent,
    Plugin { plugin_id: String },
    Scheduler,
    Node { node_id: String },
    Operator,
    System,
}
```
### 3.5 事件日志原则
所有 runtime 关键动作都应先有 event，再考虑是否导出日志、指标、timeline。
也就是：
- event log 是事实层
- structured logs 是表现层
- metrics/traces 是聚合层
---
## 4. Turn Runtime 协议
### 4.1 Turn 是什么
`Turn` 是 runtime 对一次输入或一次推进动作的完整处理闭环。
它可以由以下来源触发：
- 用户消息
- 调度器触发
- 插件事件
- headless task step
- node 回传
### 4.2 标准 turn 阶段
建议统一为：
1. input accepted
2. turn started
3. context build
4. model request
5. structured tool loop（0..n）
6. output finalize
7. memory commit
8. turn complete
### 4.3 Turn 生命周期建议
```rust
pub enum TurnPhase {
    Accepted,
    Started,
    BuildingContext,
    CallingModel,
    ExecutingTool,
    FinalizingOutput,
    CommittingMemory,
    Completed,
    Failed,
    Cancelled,
}
```
### 4.4 标准 turn 事件序列
一个典型交互 turn 的事件顺序建议为：
1. `message_received`
2. `turn_started`
3. `context_built`
4. `model_called`
5. `tool_call_requested` / `tool_called` / `tool_completed`（可重复）
6. `artifact_created`（可选）
7. `memory_committed`（可选）
8. `turn_succeeded` 或 `turn_failed`
### 4.5 Headless turn
对于 `headless_task`，没有 `message_received` 也可以进入 turn：
- 由 `task_started` / `schedule_triggered` / `dependency_complete` 驱动
- turn 依然作为处理单元存在

### 4.6 Shell Session 与 Turn 的关系
需要明确一个运行时边界：
- `shell_exec` 更接近普通一次性 tool call
- `shell_session_*` 更接近可跨 turn 持续存在的 runtime resource

这意味着：
- 一个 turn 可以创建 shell session
- 后续 turn 可以继续在同一个 `session_id` 上执行
- shell session 的生命周期不必严格等于单个 turn 生命周期

多会话并发的推荐方式是：
- 多个独立 `session_id`

而不是第一阶段就在单 shell 内支持复杂 job control。
---
## 5. Task Runtime 协议
### 5.1 Task 是什么
`Task` 是可调度、可恢复、可检查点化、可委派的工作单元。
它可以：
- 单步执行
- 多步计划
- 被中断
- 被恢复
- 被重试
- 被转移到 node 执行
### 5.2 Task 生命周期建议
```rust
pub enum TaskPhase {
    Created,
    Ready,
    Scheduled,
    Leased,
    Running,
    Waiting,
    Checkpointed,
    Succeeded,
    Failed,
    Cancelled,
}
```
### 5.3 任务必须支持的能力边界
至少要提前定义：
- 是否单步执行
- 是否多步计划
- 是否可中断
- 是否可恢复
- 是否可委派
- 是否可并行
### 5.4 任务流建议
典型调度任务流：
1. `schedule_triggered`
2. `task_started`
3. 生成 `turn_started` 或直接进入 headless execution
4. 运行中产生 `model_called` / `tool_called` / `artifact_created`
5. 如进入等待，可产生 `sleep_requested` / `task_waiting`
6. 恢复后可产生 `task_resumed`
7. 可能产生 `task_checkpointed`
8. 最终 `task_succeeded` 或 `task_failed`
---
## 6. Scheduled Task 进入 Runtime 的方式
### 6.1 调度对象统一为 Scheduled Task
调度不分 heartbeat / cron / interval 宗教，统一使用 scheduled task。
### 6.2 进入 runtime 的标准流程
1. 调度器触发 `schedule_triggered`
2. 根据 `Schedule` 解析 target
3. 创建或绑定 `Task`
4. 根据 `execution_mode` 决定：
   - `ephemeral_session`
   - `bound_session`
   - `headless_task`
5. 进入 task runtime
### 6.3 默认执行模式建议
默认建议为：
- `headless_task`
而不是每次都建 chat session。
理由：
- autonomous runtime 不应把一切都聊天化
- headless task 更适合后台工作、巡检、同步、自动化
---
## 7. 重试、幂等与熔断协议
### 7.1 为什么必须原生定义
自治系统如果不统一定义重试 / 幂等 / 熔断，后面各插件会各写各的重试器，最终疯狂抽搐。
### 7.2 需统一定义的内容
- 什么 action 可重试
- 重试 key 怎么算
- 幂等 key 怎么传
- 什么错误触发熔断
- cooldown 如何记录
### 7.3 建议模型
```rust
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff: BackoffStrategy,
    pub retryable_categories: Vec<ErrorCategory>,
}
```
```rust
pub enum BackoffStrategy {
    Fixed,
    Exponential,
    ExponentialWithJitter,
}
```
```rust
pub struct CircuitBreakerState {
    pub breaker_id: String,
    pub status: BreakerStatus,
    pub opened_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
    pub failure_count: u32,
}
```
### 7.4 原则建议
- Tool call 默认必须支持幂等 key
- Remote provider call 建议支持 request id / retry key
- 熔断器以 resource / plugin / endpoint 为粒度，而不是整个系统一刀切
---
## 8. 错误模型与恢复策略
### 8.1 错误结构统一
建议与 Core Object Model 对齐，统一使用结构化错误：
- `code`
- `message`
- `retryable`
- `category`
- `source`
- `details`
- `cause_chain`
### 8.2 至少需要的错误分类
- `config_error`
- `provider_error`
- `network_error`
- `auth_error`
- `timeout`
- `rate_limit`
- `tool_failure`
- `plugin_failure`
- `state_conflict`
- `budget_exceeded`
### 8.3 恢复策略建议
- retryable error -> 进入 retry policy
- non-retryable error -> 直接失败并落 event
- repeated endpoint failure -> breaker open
- conflict / lease failure -> 重新排队或延迟重试
---
## 9. 观测性协议
### 9.1 不要后补 observability
至少应原生支持：
- structured logs
- traces
- metrics
- event replay
- per-task timeline
- per-plugin health
- cost / usage timeline
### 9.2 分层建议
- event log：事实记录
- logs：调试文本与结构化输出
- traces：跨模块链路
- metrics：聚合指标
### 9.3 Timeline 重建
只要 event 模型统一，系统就能做：
- 单 task 时间线
- 单 turn 时间线
- 单 plugin 行为轨迹
- 单 node 运行轨迹
- usage / billing 时间线
---
## 10. LCM Runtime 基础模型
### 10.1 LCM 是什么
LCM（Long Context Management）不是“把老消息总结一下”这么简单。
它是：
- 上下文压缩
- 结构化记忆提炼
- 可回溯摘要图
- 失效检测与修正
### 10.2 LCM 至少要支持
- `summary invalidation`
- `contradiction marker`
- `stale marker`
- `recompute trigger`
- `source expansion handle`
- `confidence / freshness rules`
### 10.3 SummaryNode 的运行时语义
`SummaryNode` 不只是一个总结文本，而是：
- 一组 source event / artifact / context 的压缩节点
- 具有 freshness 与 confidence
- 可被 invalidated 或 recomputed
---
## 11. Summary 生命周期
### 11.1 建议状态
```rust
pub enum SummaryStatus {
    Fresh,
    Stale,
    Contradicted,
    Invalidated,
    Recomputing,
    Archived,
}
```
### 11.2 生命周期事件建议
- `summary_compacted`
- `summary_invalidated`
- `summary_recomputed`
- `summary_archived`
### 11.3 summary invalidation 触发场景
- 新事实与旧 summary 矛盾
- 旧 policy / rule / mission 发生重大变化
- 关键 source artifact 被删除或替换
- workspace identity 文件重大更新
- LCM 策略版本升级
---
## 12. 矛盾、过期与修正机制
### 12.1 contradiction marker
用于标记 summary 与事实冲突，而不是马上悄覆盖。
### 12.2 stale marker
用于标记 summary 过期，但未必错误。
### 12.3 recompute trigger
以下情况可触发重算：
- 超过 freshness window
- contradiction 出现
- source expansion 明显不足
- confidence 低于阈值
### 12.4 source expansion handle
summary 必须能回溯到 source handles，例如：
- source event ids
- source artifact ids
- source memory document refs
没有 source expansion，summary 一旦错就只能玄学修复。
---
## 13. Context Budget Policy
### 13.1 为什么要先定义
如果不提前定义上下文预算策略，系统会越来越胖，最后靠随机截断活着。
### 13.2 建议预算分层
每次上下文预算建议分桶：
- stable docs
- mission / rules
- current task
- recent events
- LCM expansion
- retrieved memory
- tool traces
### 13.3 预算模型建议
```rust
pub struct ContextBudgetPolicy {
    pub total_budget_tokens: u32,
    pub stable_docs_budget: u32,
    pub mission_rules_budget: u32,
    pub current_task_budget: u32,
    pub recent_events_budget: u32,
    pub lcm_expansion_budget: u32,
    pub retrieved_memory_budget: u32,
    pub tool_traces_budget: u32,
}
```
### 13.4 原则
预算策略属于 runtime policy，而不是 prompt 小技巧。
---
## 14. 任务规划协议
### 14.1 为什么要先定
如果任务规划协议不定义，后面 agent 会在单步执行、多步计划、并行、委派之间行为飘忽。
### 14.2 需定义的维度
- 单步执行
- 多步计划
- 可中断
- 可恢复
- 可委派
- 可并行
### 14.3 建议模型
```rust
pub struct TaskPlanPolicy {
    pub allow_multistep: bool,
    pub allow_interruption: bool,
    pub allow_resume: bool,
    pub allow_delegation: bool,
    pub allow_parallelism: bool,
}
```
---
## 15. 模型路由策略
### 15.1 必须提前定义的原因
否则 later 很容易退化成 prompt 里写“简单问题用小模型”。
### 15.2 至少需要区分
- 快模型
- 深度模型
- 便宜模型
- 压缩模型
- 工具调用模型
- 总结模型
### 15.3 建议模型
```rust
pub struct ModelRoutingPolicy {
    pub fast_model: String,
    pub deep_model: String,
    pub cheap_model: String,
    pub compression_model: String,
    pub tool_calling_model: String,
    pub summarization_model: String,
}
```
---
## 16. Skill 触发协议
### 16.1 必须显式定义 skill 选择方式
建议支持：
- `rule_based`
- `semantic_match`
- `explicit_user_request`
- `task_policy`
- `plugin_recommendation`
### 16.2 冲突解决也要定义
当多个 skill 同时命中时，应定义优先级策略：
- explicit > policy > semantic > recommendation
### 16.3 建议模型
```rust
pub enum SkillTriggerMode {
    RuleBased,
    SemanticMatch,
    ExplicitUserRequest,
    TaskPolicy,
    PluginRecommendation,
}
```
---
## 17. 自治边界模型
### 17.1 这是必须提前写死的边界
必须定义 agent 在自治模式下是否允许：
- 自主发消息
- 自主花钱
- 自主创建任务
- 自主安装插件
- 自主改配置
- 自主修改 memory / rules / mission
### 17.2 建议模型
```rust
pub struct AutonomyPolicy {
    pub may_send_messages: bool,
    pub may_spend_budget: bool,
    pub may_create_tasks: bool,
    pub may_install_plugins: bool,
    pub may_modify_config: bool,
    pub may_modify_memory: bool,
    pub may_modify_rules: bool,
    pub may_modify_mission: bool,
}
```
### 17.3 原则
没有自治边界模型，autonomous runtime 很容易从智能系统退化成野狗。
---
## 18. 版本化与迁移
### 18.1 必须版本化的对象
至少应定义：
- workspace schema version
- config schema version
- plugin API version
- state schema version
- event schema version
- skill spec version
### 18.2 迁移执行器建议
建议未来支持：
- `dry-run`
- `apply`
- `rollback_hint`
### 18.3 原因
如果没有 schema migration 设计，长期存活基本不可能。
---
## 19. 最小实现优先级
下一轮进入 Code 模式时，建议按如下顺序落地 runtime 契约。
### P0：先钉类型边界
1. `RuntimeEvent` / `EventType`
2. `TurnPhase`
3. `TaskPhase`
4. `RetryPolicy` / `CircuitBreakerState`
5. `ContextBudgetPolicy`
6. `AutonomyPolicy`
### P1：再钉 LCM 契约
7. `SummaryStatus`
8. `summary invalidation` / `recompute` 事件
9. source expansion handle 字段
### P2：再钉任务与路由策略
10. `TaskPlanPolicy`
11. `ModelRoutingPolicy`
12. `SkillTriggerMode`
### 当前不建议立即做
- 完整 event replay engine
- 完整 scheduler engine
- 自动 LCM recompute worker
- 分布式 breaker / lease 协调
---
## 20. 总结
Event / Task / LCM Runtime Spec 的重点不是实现所有运行时功能，而是先把执行与压缩流的基础契约钉死。
一句话总结：
**事件是事实层，任务是执行层，turn 是处理层，LCM 是长期上下文压缩与修正层；这些边界必须先定义，系统才不会后期精神分裂。**
