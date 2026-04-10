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
## Blocking Track A: Config Bootstrap + OpenAI Provider Testability

这是一条测试解锁支线。

原因：
- 当前 config 还不完善，几乎没法稳定做真实 LLM 交互测试
- 如果这条线不先做，后续 context / tool / memory 的实际效果很难验证

执行规则：
- 这条支线优先于 `Batch 2`
- 目标不是一次做完整配置平台，而是先把“可初始化、可配置、可测试”做出来

- [x] 实现 config root 自动初始化：
  - 生成 `core.toml`
  - 生成 `providers.toml`
  - 生成 `models.toml`
  - 生成 `resources.toml`
  - 生成 `daemon.toml`
  - 生成最小 `workspace/` 样板
- [x] 初始化逻辑支持“缺失则生成、已存在不覆盖”的幂等行为
- [x] 提供本地开发初始化入口：
  - CLI 命令或等价入口
  - `minimal` 或 `local-dev` 模式
- [x] 完善 OpenAI provider 配置模型：
  - `api_key` 或 env ref
  - `base_url`
  - organization / project 等可选项
  - 代理提供商 base URL 覆盖
- [x] 实现 OpenAI provider 模型列表拉取
- [x] 对模型列表做筛选，得到语言模型集合
- [x] 获取并归一化模型信息：
  - model id
  - display label
  - context length
  - input/output token limits
  - capability tags
- [x] 让 `models.toml` 或等价配置快照能消费上述模型信息
- [x] 增加最小诊断/测试能力：
  - 列出 provider
  - 列出语言模型
  - 打印模型信息
  - 验证 `base_url` 代理配置是否生效
- [x] 保证完成后，能通过真实配置发起一次 LLM 测试请求

## Batch 1: Workspace Persona + Context v0
- [x] 实现 workspace 文件真实加载：
  - `AGENT.md`
  - `SOUL.md`
  - `USER.md`
  - `MISSION.md`
  - `RULES.md`
  - `ROUTER.md`
  - `MEMORY.md`
- [x] 定义 `WorkspaceIdentityPack` 或等价结构，承载上述文件内容与来源路径
- [x] 让 `ConfigLoader` / `bootstrap` 不再只返回硬编码默认值，而是能从本地 `config/` 与 `workspace/` 真读取
- [x] 实现 `ContextEngine v0`：
  - workspace identity blocks
  - `MEMORY.md` block
  - recent transcript block
  - token breakdown 最小实现
- [x] 让 `session.send` 改为走 `assemble_context()`，而不是直接拼 transcript
- [x] 为 workspace 加载和 context assembly 补最小测试或可验证样例

## Batch 2: Tool Calling v0
- [x] 实现 `ToolRegistry`
- [x] 定义 tool plugin 的最小调用接口
- [x] 实现 `read_file`
- [x] 实现 `list_files`
- [x] 实现 `shell`
- [x] 实现最小 tool calling loop：
  - 模型请求
  - tool call 识别
  - tool dispatch
  - tool result 回写
  - 二次模型调用
- [x] 将 tool 调用过程写入 `RuntimeEvent`
- [x] 为 tool timeout / basic error / idempotency 留出最小边界

## Blocking Track B: Prompt Injection + XML Assembly

这是一条插在 `Batch 3` 之前的结构化注入支线。

原因：
- 现在虽然有 tools，但 prompt 注入边界不清
- Agent 需要通过明确的协议知道自己有哪些工具
- workspace 核心文件也需要固定 Markdown 约束，不能任意写再直接裸塞

执行规则：
- 这条支线优先于 `Batch 3`
- 目标是先把 prompt assembly 协议、workspace Markdown 约束和 XML 注入落地

- [x] 撰写 `docs/PROMPT_ASSEMBLY_SPEC.md`
- [x] 定义 `PromptDocument` / `PromptSection` / `PromptFragment` 或等价中间模型
- [x] 定义 workspace 核心文件的受约束 Markdown 结构解析规则
- [x] 定义工具注入的 XML 结构
- [x] 定义 memory / knowledge / task / latest_user_message 的 XML 结构
- [x] 让 `assemble_context()` 与 `render_prompt_xml()` 显式分层
- [x] 将当前字符串拼接 prompt 替换为 XML 渲染
- [x] 保证 Agent 能在 prompt 中明确感知工具可用性
- [x] 子改进：定义统一的结构化消息注入 schema：
  - user message
  - assistant message
  - tool result message
  - system / runtime message
- [x] 子改进：为每条消息定义 `meta + content` 结构：
  - message id
  - session id
  - channel / surface
  - user id / actor id
  - timestamp
  - locale / extra metadata
- [x] 子改进：将 `latest_user_message` 扩展为统一 message XML 节点
- [x] 子改进：保证用户原文内容与程序推断字段分离，避免模型把推断当事实

## Batch 3: Session / Event Persistence
- [x] 撰写 `docs/SESSION_EVENT_PERSISTENCE_SPEC.md`
- [x] 落地 `sqlite_sessions` 最小实现
- [x] 落地 `sqlite_context` 最小实现
- [x] 将当前内存 `DaemonStore` 迁到可持久化后端或其抽象层之上
- [x] 重启 daemon 后仍可恢复 session/messages/events
- [x] 将 `session.list` / `session.get` / `session.send` 统一走存储抽象

## Batch 3.5: TUI + Streaming Reply
- [x] 完善 TUI 基础交互：
  - 会话列表
  - 会话详情
  - 输入框与发送动作
  - 基本状态提示
- [x] 将 daemon `stream` 输出真实接入 TUI
- [x] 实现 assistant 流式回复渲染
- [x] 区分进行中消息与已完成消息
- [x] 为 tool calling / error / stream end 提供最小可视反馈
- [x] 保持 TUI 仍然是 out-of-process core surface，而不是 plugin channel

## Batch 3.6: Model Switching Foundation
- [x] 修正模型目录刷新逻辑，避免 `models list --refresh` 覆盖用户显式设置的默认模型
- [x] 定义 session 级模型绑定基础字段：
  - current provider id
  - current model id
  - optional pending switch target
  - last switched at
- [x] 将 session 模型绑定纳入持久化层：
  - sqlite session schema
  - session load/save
  - daemon default session bootstrap
- [x] 让 runtime 在 `session.send` 时按 session 当前模型解析 provider/model，而不是只读全局 default agent
- [x] 为模型切换预留最小状态机：
  - idle 可切换
  - active turn 期间拒绝切换
  - pending / applied / rejected 基本结果
- [x] 为后续 slash 指令预留 daemon API / application service 落点：
  - inspect current session model
  - request session model switch
  - validate provider/model exists before apply
- [x] 为模型切换记录最小事件：
  - model_switch_requested
  - model_switch_applied
  - model_switch_rejected
- [x] 保持这条能力是“模型热切换底层基础”，不在本批直接实现 slash 命令解析
- [x] 为默认模型、session override、重启恢复补最小回归测试

## Batch 3.7: Plugin SDK Alignment
- [x] 补齐 `PluginContext` 最小版，使其不只是 config/resource 壳，而具备统一 runtime handles 落点：
  - workspace
  - session
  - turn
  - models
  - tools
  - memory
  - knowledge
  - events
  - hooks
- [x] 将当前 provider / tool / storage 接口对齐到统一 plugin trait 体系：
  - `ProviderPlugin`
  - `ToolPlugin`
  - `SessionStore` / storage plugin
  - context plugin 最小 trait
  - backend plugin 最小 trait
- [x] 强化 `PluginRegistry`：
  - 按 capability 查询实例
  - manifest map
  - provider registry
  - 最小依赖校验
  - workspace 可用插件集预留落点
- [x] 强化 `PluginHost` 生命周期接线：
  - `on_load`
  - `on_startup`
  - `on_shutdown`
  - 最小错误传播
- [x] 将 `SqliteSessionStore` 明确提升为 storage plugin 落点，而不只是裸 store 实现
- [x] 将当前 retrieval/context 相关实现整理为 context plugin 落点，而不是只做内部模块
- [x] 为 `ResourceRegistry` 预留资源客户端抽象方向：
  - `ModelClient`
  - `ToolClient`
  - `MemoryClient`
  - `KnowledgeClient`
  - 先不做完整实现，但钉死接口边界
- [x] 为 Hook 系统补结构化输入对象基础，而不是只存 `HookPoint`
- [x] 保持范围收敛在静态注册 + 单 workspace + 单 provider，不进入动态插件加载
- [x] 为 plugin registry / plugin host / storage plugin / context plugin 补最小回归测试

## Batch 4: Memory / RAG v0
- [x] 实现 `MEMORY.md` + `memory/topics/` 的最小读取召回
- [x] 实现 `knowledge/` 的最小 keyword/full-text 检索
- [x] 定义最小 collection abstraction
- [x] 实现 `retrieval_bridge` 最小闭环
- [x] 将 memory / knowledge 检索结果接入 context assembly
- [x] 明确区分 memory recall 和 knowledge retrieval

## Batch 5: Daemon API Completion
- [x] 补齐 `runtime.*` 其余 handler
- [x] 补齐 `config.*` handler
- [x] 补齐 `plugin.*` handler
- [x] 补齐 `task.*` handler
- [x] 补齐 `node.*` handler
- [x] 补齐 `schedule.*` handler
- [x] 补齐 `doctor.*` / `smoke.*` / `logs.*` / `metrics.*`
- [x] 实现 `subscription.cancel`
- [x] 实现 `stream.cancel`

## Batch 6: Task Runtime v0
- [ ] 实现最小 task store
- [ ] 让 `session.send` 可生成 task 或 turn timeline
- [ ] 实现最小 task status transition
- [ ] 实现 task inspect / cancel / retry 的最小闭环
- [ ] 为 checkpoint / resume 留出最小数据模型落点

## Batch 7: Cleanup + Hardening
- [ ] 清理 docs 与代码的漂移项
- [ ] 清理空壳插件与未接线 trait
- [ ] 为 daemon / cli / transport 增加最小集成测试
- [ ] 为 workspace loader / context assembly / tool loop 增加回归测试
- [ ] 补一版真实的开发与运行说明

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
