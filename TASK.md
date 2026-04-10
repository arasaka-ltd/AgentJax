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
- [ ] 实现 `ToolRegistry`
- [ ] 定义 tool plugin 的最小调用接口
- [ ] 实现 `read_file`
- [ ] 实现 `list_files`
- [ ] 实现 `shell`
- [ ] 实现最小 tool calling loop：
  - 模型请求
  - tool call 识别
  - tool dispatch
  - tool result 回写
  - 二次模型调用
- [ ] 将 tool 调用过程写入 `RuntimeEvent`
- [ ] 为 tool timeout / basic error / idempotency 留出最小边界

## Batch 3: Session / Event Persistence
- [ ] 落地 `sqlite_sessions` 最小实现
- [ ] 落地 `sqlite_context` 最小实现
- [ ] 将当前内存 `DaemonStore` 迁到可持久化后端或其抽象层之上
- [ ] 重启 daemon 后仍可恢复 session/messages/events
- [ ] 将 `session.list` / `session.get` / `session.send` 统一走存储抽象

## Batch 4: Memory / RAG v0
- [ ] 实现 `MEMORY.md` + `memory/topics/` 的最小读取召回
- [ ] 实现 `knowledge/` 的最小 keyword/full-text 检索
- [ ] 定义最小 collection abstraction
- [ ] 实现 `retrieval_bridge` 最小闭环
- [ ] 将 memory / knowledge 检索结果接入 context assembly
- [ ] 明确区分 memory recall 和 knowledge retrieval

## Batch 5: Daemon API Completion
- [ ] 补齐 `runtime.*` 其余 handler
- [ ] 补齐 `config.*` handler
- [ ] 补齐 `plugin.*` handler
- [ ] 补齐 `task.*` handler
- [ ] 补齐 `node.*` handler
- [ ] 补齐 `schedule.*` handler
- [ ] 补齐 `doctor.*` / `smoke.*` / `logs.*` / `metrics.*`
- [ ] 实现 `subscription.cancel`
- [ ] 实现 `stream.cancel`

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
