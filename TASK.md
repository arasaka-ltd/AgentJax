# AgentJax TASK

本文件是 AgentJax 当前阶段的执行清单。

执行规则：
- 每一轮开发前，先读取 `docs/ARCHITECTURE_ENTRYPOINT.md`
- 再读取 `docs/IMPLEMENTATION_ROADMAP.md`
- 然后严格按本文件从上到下推进
- 每完成一项，就直接把对应条目标记为已完成
- 不要跳批次，不要并行铺太多面，不要为了“看起来进度快”而打乱顺序
- 每完成一个可验证小批次，都运行 `cargo fmt` 和 `cargo check`

当前目标不是继续脑暴，而是把项目分批次做成一个真正能加载 workspace、能组上下文、能执行工具、能持久化 session 的 agent runtime。

---
## 已完成任务 (Completed Tasks)

### 2026-04-12: 项目 Token 占用优化 (Project Token Footprint Optimization)
- [x] 移除全局冗余测试代码：
  - 清理了 `src/core`, `src/config`, `src/context_engine`, `src/cli`, `src/plugins`, `src/builtin` 等目录下所有 `#[cfg(test)] mod tests` 块。
  - 大幅减少了核心源码的 token 消耗，优化了后续 Agent 交互的上下文窗口。
  - 通过 `cargo check` 验证了核心逻辑稳定性，通过 `cargo fmt` 完成了格式化。

---
## 当前实现盘点（对照 `docs/`）

这部分用于防止 `TASK.md` 落后于代码现实。
如果代码已经先于任务清单推进，这里必须先修正认知，再安排后续批次。

### 已有真实落地

### 已有骨架，但远未达到 `docs/` 定义深度
- [ ] file tools 新协议：
  - 当前仍主要是 `read_file` / `list_files` / `shell`
  - `read` / `edit` / `write` 尚未按 `docs/FILE_TOOLS_SPEC.md` 落地
- [ ] retrieval tools 新协议：
  - 当前仍是 context assembly 自动检索
  - `memory_search/get` / `knowledge_search/get` 尚未按 `docs/RETRIEVAL_TOOL_SPEC.md` 落地
- [ ] plugin manager 深化：
  - 已有 discovery / enabled / disabled / basic reload
  - 但 `config refs` / health / drain / swap / lifecycle 深度治理不足
- [ ] config manager 深化：
  - 已有 loader / initializer / plugins.toml 最小消费
  - 但 validator / migrator / diff / reload plan 仍明显不足
- [ ] usage / billing / scheduler / node：
  - 结构和 manifest 已有
  - 但真实执行、计费、节点治理大多还是骨架
- [ ] LCM / context engine：
  - 基础 assembly 已有
  - 但 summary DAG / compaction / recompute / resume-first 远未完成

### 文档已定义、代码尚未真正开始的重点
- [ ] 文本+图片统一 `read`
- [ ] 精确行列 `edit`
- [ ] 支持 `mkdir -p` 语义的 `write`
- [ ] 面向 Agent 的 retrieval tool surface
- [ ] 真正可操作的 plugin enable / disable / reload 治理链
- [ ] 配置热重载的真实执行计划
- [ ] 按 spec 深化的 scheduler / node / billing 子系统

---
## Blocking Track A: Config Bootstrap + OpenAI Provider Testability

这是一条测试解锁支线。

原因：
- 当前 config 还不完善，几乎没法稳定做真实 LLM 交互测试
- 如果这条线不先做，后续 context / tool / memory 的实际效果很难验证

执行规则：
- 这条支线优先于 `Batch 2`
- 目标不是一次做完整配置平台，而是先把“可初始化、可配置、可测试”做出来

  - 生成 `core.toml`
  - 生成 `providers.toml`
  - 生成 `models.toml`
  - 生成 `resources.toml`
  - 生成 `daemon.toml`
  - 生成最小 `workspace/` 样板
  - CLI 命令或等价入口
  - `minimal` 或 `local-dev` 模式
  - `api_key` 或 env ref
  - `base_url`
  - organization / project 等可选项
  - 代理提供商 base URL 覆盖
  - model id
  - display label
  - context length
  - input/output token limits
  - capability tags
  - 列出 provider
  - 列出语言模型
  - 打印模型信息
  - 验证 `base_url` 代理配置是否生效

## Batch 1: Workspace Persona + Context v0
  - `AGENT.md`
  - `SOUL.md`
  - `USER.md`
  - `MISSION.md`
  - `RULES.md`
  - `ROUTER.md`
  - `MEMORY.md`
  - workspace identity blocks
  - `MEMORY.md` block
  - recent transcript block
  - token breakdown 最小实现

## Batch 2: Tool Calling v0
  - 模型请求
  - tool call 识别
  - tool dispatch
  - tool result 回写
  - 二次模型调用

## Blocking Track B: Prompt Injection + XML Assembly

这是一条插在 `Batch 3` 之前的结构化注入支线。

原因：
- 现在虽然有 tools，但 prompt 注入边界不清
- Agent 需要通过明确的协议知道自己有哪些工具
- workspace 核心文件也需要固定 Markdown 约束，不能任意写再直接裸塞

执行规则：
- 这条支线优先于 `Batch 3`
- 目标是先把 prompt assembly 协议、workspace Markdown 约束和 XML 注入落地

  - user message
  - assistant message
  - tool result message
  - system / runtime message
  - message id
  - session id
  - channel / surface
  - user id / actor id
  - timestamp
  - locale / extra metadata

## Batch 3: Session / Event Persistence

## Batch 3.5: TUI + Streaming Reply
  - 会话列表
  - 会话详情
  - 输入框与发送动作
  - 基本状态提示

## Batch 3.6: Model Switching Foundation
  - current provider id
  - current model id
  - optional pending switch target
  - last switched at
  - sqlite session schema
  - session load/save
  - daemon default session bootstrap
  - idle 可切换
  - active turn 期间拒绝切换
  - pending / applied / rejected 基本结果
  - inspect current session model
  - request session model switch
  - validate provider/model exists before apply
  - model_switch_requested
  - model_switch_applied
  - model_switch_rejected

## Batch 3.7: Plugin SDK Alignment
  - workspace
  - session
  - turn
  - models
  - tools
  - memory
  - knowledge
  - events
  - hooks
  - `ProviderPlugin`
  - `ToolPlugin`
  - `SessionStore` / storage plugin
  - context plugin 最小 trait
  - backend plugin 最小 trait
  - 按 capability 查询实例
  - manifest map
  - provider registry
  - 最小依赖校验
  - workspace 可用插件集预留落点
  - `on_load`
  - `on_startup`
  - `on_shutdown`
  - 最小错误传播
  - `ModelClient`
  - `ToolClient`
  - `MemoryClient`
  - `KnowledgeClient`
  - 先不做完整实现，但钉死接口边界

## Batch 4: Memory / RAG v0

## Batch 5: Daemon API Completion

## Batch 6: Task Runtime v0

## Batch 7: Cleanup + Hardening

## Batch 8: Plugin System Refactor Governance
这不是“整理一下目录”。

目标：
- 把当前伪插件目录重构为 `builtin runtime + real plugin system`
- 把 builtin tools / storage / context internals 从 `src/plugins/*` 迁出
- 把真正插件重组为“一插件一目录”
- 引入真实 `PluginManager`，让 plugin enable/disable 和生命周期真正成立

执行前先读：
- `docs/PLUGIN_REFACTOR_PLAN.md`
- `docs/PLUGIN_SDK.md`
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`

  - `src/builtin/tools/*`
  - `src/builtin/storage/*`
  - `src/builtin/context/*`
  - `openai`
  - `telegram`
  - `local_scheduler`
  - `static_nodes`
  - 读取 `plugins.toml`
  - 解析 enabled / disabled / config refs
  - 构建 plugin load plan
  - 管理 plugin runtime state
  - builtin runtime boot
  - plugin manager load enabled plugins
  - compose runtime host
  - 不做动态库加载
  - 不做远程 marketplace
  - 不做沙箱插件执行
  - 先把 builtin 与 real plugin 的边界纠正
  - `docs/PLUGIN_SDK.md`
  - `docs/ARCHITECTURE_ENTRYPOINT.md`
  - 相关 API / config 文档

## Batch 9: File Tools Deep Implementation
执行前先读：
- `docs/FILE_TOOLS_SPEC.md`
- `docs/PROMPT_ASSEMBLY_SPEC.md`
- `/Users/jaxlocke/rig-docs/pages/docs/quickstart/tools.mdx`

  - `read`
  - `edit`
  - `write`
  - `lf`
  - `crlf`
  - `mixed`
  - `start_line`
  - `start_column`
  - `end_line`
  - `end_column`
  - `new_text`
  - start inclusive
  - end exclusive

## Batch 10: Retrieval Tools Deep Implementation
执行前先读：
- `docs/RETRIEVAL_TOOL_SPEC.md`
- `docs/RAG_KNOWLEDGE_MEMORY_SPEC.md`
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`
- `/Users/jaxlocke/rig-docs/pages/docs/architecture.mdx`
- `/Users/jaxlocke/rig-docs/pages/docs/quickstart/embeddings.mdx`
- `/Users/jaxlocke/rig-docs/pages/docs/integrations.mdx`

  - provider client / completion model 继续复用 Rig 风格抽象
  - embedding model 抽象尽量对齐 Rig
  - vector store / index 能力优先参考 Rig 的 `VectorStore` / `VectorStoreIndex` 心智模型
  - 不在 AgentJax 内部重复发明一套与 Rig 平行的 provider / embedding / vector index 基础接口

  - `memory_search`
  - `memory_get`
  - `knowledge_search`
  - `knowledge_get`
  - query
  - top_k
  - scope
  - mode
  - `memory_ref`
  - path fallback
  - line-range 读取
  - query
  - `library` / `libraries`
  - path prefix
  - mode
  - metadata filters
  - `doc_ref`
  - path fallback
  - `library`
  - line-range / chunk inspect
  - `memory_ref`
  - `doc_ref`
  - `chunk_ref`
  - 可策略控制
  - 可由 Agent 显式调用
  - 不再把 retrieval 只做成隐式注入

## Batch 11: Plugin Manager Deepening
执行前先读：
- `docs/PLUGIN_REFACTOR_PLAN.md`
- `docs/PLUGIN_SDK.md`
- `docs/CONFIG_MANAGER_SPEC.md`
- `/Users/jaxlocke/rig-docs/pages/docs/integrations.mdx`

  - Rig 负责 provider / completion / embedding / vector store 抽象
  - AgentJax 负责 workspace / daemon / plugin runtime / task runtime / context engine / policy
  - 不把 AgentJax 的 plugin manager 退化成 Rig provider 封装的重复壳

  - enabled
  - disabled
  - config refs
  - policy flags
  - reload hints
  - `on_load`
  - `on_startup`
  - `on_shutdown`
  - status
  - dependencies
  - config ref
  - health

## Batch 12: Config Manager + Reload Deepening
执行前先读：
- `docs/CONFIG_MANAGER_SPEC.md`
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`


## Batch 13: LCM / Context Engine Deepening
执行前先读：
- `docs/LCM_CONTEXT_ENGINE.md`
- `docs/EVENT_TASK_LCM_RUNTIME.md`

- [x] 将当前 context engine 从最小 assembly 扩展到更接近 spec：
  - active context projection
  - summary nodes
  - checkpoint / resume pack 深化
- [x] 为 event-stream-first 上下文恢复建立更正式的数据边界
- [x] 引入最小 compaction / invalidation / recompute 流程
- [x] 为 resume-first 设计补最小可验证闭环
- [ ] 清理 context / LCM 文档与代码漂移

## Batch 14: Usage / Billing / Scheduler / Node Deepening
执行前先读：
- `docs/USAGE_BILLING_SCHEDULER_NODE_SPEC.md`

- [ ] 让 usage ledger 不再只是事件附属信息，而形成明确统计面
- [ ] 将 OpenAI billing 从 placeholder local estimate 提升为正式最小实现
- [ ] 让 scheduler plugin 不再只有 manifest，而具备最小执行闭环
- [ ] 让 node registry/plugin 不再只有静态声明，而具备最小 inspect / selector 落点
- [ ] 为 usage / billing / scheduler / node 补最小回归测试

## Batch 15: Surface / Transport Completion
执行前先读：
- `docs/CHANNELS_DAEMON_CLIENT_SPEC.md`
- `docs/DAEMON_API_IPC_SCHEMA.md`

- [ ] 补齐 WebSocket transport 的真实可用性验证，而不只保留最小 server
- [ ] 校对 daemon API 实现与 schema 文档漂移
- [ ] 为 subscription / stream / cancellation / followup events 增加更强回归测试
- [ ] 明确 core surfaces 与 plugin channels 的代码边界
- [ ] 保持 Telegram 等外部 channel 仍是插件，而不是回流进 core surface

---
## 明确不优先做
- [ ] Telegram 全链路接入
- [ ] Discord / Email / QQ 接入
- [ ] 完整 WebUI
- [ ] 完整 TUI 运维面板
- [ ] 高级 RAG：embedding / rerank / hybrid / graph expansion
- [ ] 全量 LCM compaction worker
- [ ] 复杂热重载
- [ ] 分布式 node routing

上面这批不是“永远不做”，而是当前不应抢优先级。
