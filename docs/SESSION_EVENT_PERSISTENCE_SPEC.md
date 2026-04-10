# AgentJax Session / Event Persistence Spec

## 1. 目标
本文档定义 AgentJax 在 `Batch 3` 阶段的 session / event 持久化边界、数据库对象、写入路径与演进策略。

这份文档要解决的问题不是“先随便建几张 SQLite 表”。
而是：
- 如何让 daemon 重启后不丢 session 和事件
- 如何让 `session.list / session.get / session.send` 脱离内存 store
- 如何在不过早锁死未来架构的前提下，定义一套可演进的持久化基线
- 如何为后续 turn / tool / checkpoint / task / context engine 留下弹性扩展空间

一句话：
**Batch 3 先做稳定、最小、可演进的 persistence substrate，而不是一次性设计终极数据库。**

---
## 2. 范围边界
### 2.1 本阶段负责什么
`Batch 3` 持久化层至少负责：
- sessions
- session messages
- runtime events
- schema migrations

可选预留但不要求完整上线：
- turns
- tool call records
- checkpoints

### 2.2 本阶段不负责什么
本阶段不负责：
- 完整 memory / RAG 存储
- 完整 summary DAG 存储
- 完整 task runtime store
- event replay engine
- full-text search
- 分布式一致性

### 2.3 与现有 store 的关系
当前内存 `DaemonStore` 只应视为过渡实现。
`Batch 3` 的目标不是给它继续堆逻辑，而是把它抽象到可替换的 store interface 之上。

---
## 3. 设计原则
### 3.1 SQLite First
本阶段先只做 SQLite。

理由：
- 本地开发和单机 daemon 是当前主场景
- schema、迁移、事务边界更容易先钉死
- 后面换后端时，也应该保留相同 store 抽象

### 3.2 Append-Friendly
事件层应天然偏 append-only。

即使 session 或 message 允许少量状态更新，`runtime_events` 仍应尽量保持事实追加，而不是反复覆盖。

### 3.3 Schema 要留弹性，不要过拟合当前代码
不要假设：
- session 永远只有一种 message 结构
- turn 永远是单轮请求
- task 永远不与 session 交叉
- tool 结果永远只是一段字符串

### 3.4 结构化字段优先，扩展字段走 JSON
原则：
- 高频过滤、联结、排序字段单独建列
- 易变、未来可能扩展的细节放 JSON payload

### 3.5 迁移必须是一等公民
必须从第一版开始就有：
- `schema_migrations` 表
- schema version
- 升级路径

不要等数据库已经用起来了再补迁移。

---
## 4. 持久化层总模型
建议将本阶段 persistence 拆成三层：

### 4.1 Store Traits
面向 runtime 的稳定抽象，例如：
- `SessionStore`
- `EventStore`
- `PersistenceStore` 聚合接口

### 4.2 SQLite Backends
面向 SQLite 的具体实现，例如：
- `SqliteSessionStore`
- `SqliteEventStore`

### 4.3 Migration Layer
负责：
- 建表
- schema version 检查
- 迁移执行

---
## 5. 最小数据库对象
### 5.1 `schema_migrations`
职责：
- 记录已执行 migration
- 记录当前 schema version

建议字段：
- `version` TEXT PRIMARY KEY
- `applied_at` TEXT NOT NULL
- `description` TEXT NULL

### 5.2 `sessions`
职责：
- 持久化 session 主记录

建议字段：
- `session_id` TEXT PRIMARY KEY
- `workspace_id` TEXT NOT NULL
- `agent_id` TEXT NOT NULL
- `channel_id` TEXT NULL
- `surface_id` TEXT NULL
- `user_id` TEXT NULL
- `title` TEXT NULL
- `mode` TEXT NOT NULL
- `status` TEXT NOT NULL
- `last_turn_id` TEXT NULL
- `created_at` TEXT NOT NULL
- `updated_at` TEXT NOT NULL
- `schema_version` TEXT NOT NULL
- `meta_json` TEXT NOT NULL

### 5.3 `session_messages`
职责：
- 持久化 session 下的消息序列

建议字段：
- `message_id` TEXT PRIMARY KEY
- `session_id` TEXT NOT NULL
- `turn_id` TEXT NULL
- `role` TEXT NOT NULL
- `content_text` TEXT NOT NULL
- `message_kind` TEXT NOT NULL
- `source_channel` TEXT NULL
- `source_surface` TEXT NULL
- `actor_id` TEXT NULL
- `sequence_no` INTEGER NOT NULL
- `created_at` TEXT NOT NULL
- `meta_json` TEXT NOT NULL

### 5.4 `runtime_events`
职责：
- 持久化 runtime 事实事件

建议字段：
- `event_id` TEXT PRIMARY KEY
- `event_type` TEXT NOT NULL
- `workspace_id` TEXT NULL
- `agent_id` TEXT NULL
- `session_id` TEXT NULL
- `turn_id` TEXT NULL
- `task_id` TEXT NULL
- `plugin_id` TEXT NULL
- `node_id` TEXT NULL
- `source_kind` TEXT NOT NULL
- `occurred_at` TEXT NOT NULL
- `correlation_id` TEXT NULL
- `causation_id` TEXT NULL
- `idempotency_key` TEXT NULL
- `payload_json` TEXT NOT NULL
- `schema_version` TEXT NOT NULL

### 5.5 可选预留：`turns`
如果本轮想降低后续迁移成本，可以预留：
- `turn_id` TEXT PRIMARY KEY
- `session_id` TEXT NOT NULL
- `status` TEXT NOT NULL
- `created_at` TEXT NOT NULL
- `updated_at` TEXT NOT NULL
- `meta_json` TEXT NOT NULL

但 `Batch 3` 可以先不要求 runtime 真正依赖它。

---
## 6. 索引建议
第一版至少建议：

### 6.1 `sessions`
- `idx_sessions_updated_at` on `updated_at`
- `idx_sessions_agent_id` on `agent_id`

### 6.2 `session_messages`
- `idx_session_messages_session_seq` on `(session_id, sequence_no)`
- `idx_session_messages_session_created` on `(session_id, created_at)`
- `idx_session_messages_turn_id` on `turn_id`

### 6.3 `runtime_events`
- `idx_runtime_events_session_time` on `(session_id, occurred_at)`
- `idx_runtime_events_turn_id` on `turn_id`
- `idx_runtime_events_task_id` on `task_id`
- `idx_runtime_events_type_time` on `(event_type, occurred_at)`

---
## 7. 结构弹性策略
### 7.1 列与 JSON 的分工
以下信息应单独成列：
- 主键
- 外键
- 状态
- 时间
- 高频查询字段

以下信息可进 JSON：
- labels
- metadata
- message annotations
- provider-specific details
- tool result structured payload

### 7.2 不要过早内联未来对象
例如：
- 不要把 checkpoint 直接塞进 sessions 表
- 不要把 task timeline 直接塞进 messages 表
- 不要把 full context projection 直接落成一个大 blob

### 7.3 允许未来加表，不要求第一版全包
第一版数据库设计应允许未来自然扩出：
- `tool_calls`
- `checkpoints`
- `task_records`
- `summary_nodes`
- `artifacts`

---
## 8. 写入路径
### 8.1 `session.send` 最小写入流程
建议顺序：
1. 写 user message
2. 写 `message_received` / `turn_started` / `model_called` event
3. 调用模型
4. 写 assistant message
5. 写 `model_response_received` / `turn_succeeded` event
6. 更新 session 主记录中的 `last_turn_id` / `updated_at`

### 8.2 事务边界
建议：
- “单次消息落库 + 对应 session update” 放在一个事务里
- “事件追加” 可与主写入同事务，也可采用明确的 append transaction

第一版优先选择简单可理解的事务策略，而不是追求极限吞吐。

### 8.3 失败策略
至少应明确：
- 用户消息写入成功但模型调用失败时，保留失败事实
- assistant message 未写入时，不伪造成功状态
- 失败必须以 event 形式落库

---
## 9. 读取路径
### 9.1 `session.list`
从 `sessions` 直接读取，不应再从内存 map 推导。

### 9.2 `session.get`
至少读取：
- session 主记录
- 最近或全量 messages
- 最近或全量 events

### 9.3 `session.send`
读取 recent transcript 时，应从 `session_messages` 读取，而不是依赖 daemon 进程内存。

### 9.4 `assemble_context()`
后续读取 recent transcript / runtime events 时，也应优先走存储抽象。

---
## 10. 一致性策略
### 10.1 事实优先
如果出现异常：
- 宁可保留失败事件
- 也不要把 session 状态伪装成“什么都没发生”

### 10.2 不追求第一版的复杂恢复机制
第一版不要求：
- exactly-once event delivery
- crash recovery journal
- distributed lease recovery

但要保证：
- 本地单进程 daemon 正常重启后，session/messages/events 仍在

### 10.3 顺序约束
`session_messages.sequence_no` 应保证单 session 内的稳定顺序。

---
## 11. 迁移策略
### 11.1 第一版 migration
建议第一版版本号：
- `2026_04_10_0001_initial_session_event_persistence`

### 11.2 migration 执行原则
- 启动时检查是否已初始化
- 未初始化则建表并写入 migration 记录
- 已初始化但版本落后则按序迁移
- 迁移失败时拒绝启动 persistence backend

### 11.3 不建议的做法
不建议：
- 运行中隐式修改表结构
- 直接靠 `CREATE TABLE IF NOT EXISTS` 当迁移系统
- 没有版本记录

---
## 12. Store Trait 建议
建议至少定义：

### 12.1 `SessionStore`
负责：
- create / upsert session
- list sessions
- get session
- append message
- update session summary fields

### 12.2 `EventStore`
负责：
- append event
- list events by session
- list events by turn

### 12.3 `PersistenceStore`
作为聚合接口，供 daemon 使用。

这层的目标是：
- daemon 依赖 trait
- SQLite 只是第一实现

---
## 13. 与当前代码的落地关系
### 13.1 当前待替换对象
当前主要待替换的是：
- `src/daemon/store.rs`

### 13.2 当前待接入对象
建议逐步接入：
- `src/plugins/storage/sqlite_sessions.rs`
- `src/plugins/storage/sqlite_context.rs`

但名字不要误导：
- `sqlite_context` 在 `Batch 3` 第一版里主要负责 event/context 相关最小持久化
- 不要求一步做到完整 context engine storage

---
## 14. Batch 3 最小验收标准
做完本阶段，至少要满足：
- daemon 重启后 `session.list` 仍能看到历史 session
- `session.get` 能读到历史消息
- `session.send` 产生的 user / assistant message 会落库
- 对应 runtime events 会落库
- CLI / TUI 不需要换协议就能继续工作

---
## 15. 本阶段明确不做
- 不做完整 checkpoint store
- 不做 summary DAG persistence
- 不做 memory / RAG persistence
- 不做 FTS
- 不做分布式一致性
- 不做跨进程并发写优化

---
## 16. 与其他规范的关系
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`：定义 state / checkpoints / sessions 目录边界
- `docs/EVENT_TASK_LCM_RUNTIME.md`：定义事件与任务契约
- `docs/LCM_CONTEXT_ENGINE.md`：定义 context engine 的长期存储方向
- `docs/DAEMON_API_IPC_SCHEMA.md`：定义对外 API，不定义底层存储

---
## 17. 硬结论
AgentJax 的 `Batch 3` 应采用：
- SQLite-first
- session / message / event 分表
- schema migration 原生化
- daemon 依赖 store traits，而不是依赖内存 map

一句话拍板：
**先做稳定、可演进的 persistence substrate，再在其上扩 turn / tool / checkpoint / task。**
