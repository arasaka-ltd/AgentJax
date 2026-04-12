# AgentJax / Rig 对齐原则
## 1. 目标
本文档用于明确 AgentJax 与 Rig 的职责分层，避免在后续深度开发中重复造轮子。

这里的核心问题不是“要不要用 Rig”。
而是：
- Rig 已经解决了哪些基础抽象
- AgentJax 应该站在哪一层继续建设
- 哪些层如果再自己重造，会让系统变胖、边界变乱、维护成本失控

一句话结论：
**Rig 负责通用 LLM / embedding / tool / vector store 基础抽象，AgentJax 负责 daemon + workspace + runtime orchestration + policy。**

---
## 2. 一句话边界
- Rig：AI foundation library
- AgentJax：long-lived agent runtime framework

也就是说：
- Rig 更偏“怎么和模型、工具、向量索引交互”
- AgentJax 更偏“怎么让一个长期 agent 在 workspace、daemon、task、plugin、memory、policy 中活起来”

---
## 3. Rig 已经提供的核心能力
根据 `/Users/jaxlocke/rig-docs` 当前文档，Rig 已经明确覆盖以下基础层。

### 3.1 Provider / model 抽象
Rig 已有统一的 provider client 与模型抽象：
- completion model
- embedding model
- provider client
- agent builder

这意味着 AgentJax 不应再平行发明一套“自己的 provider 基础协议”，除非确有 runtime-level 特殊需求。

### 3.2 Tool / function calling 抽象
Rig 已支持：
- tool definition
- tool call
- agent builder 上挂接 tool

这意味着 AgentJax 在设计 Agent-facing tools 时，应尽量复用 Rig 的工具心智模型，而不是再做一套完全独立的 tool 协议哲学。

### 3.3 Embedding 抽象
Rig 已有：
- embedding model
- embeddings builder

这意味着 AgentJax 不应自己重造一套 embedding pipeline 的底层模型接口。

### 3.4 Vector store / index 抽象
Rig 已有：
- `VectorStore`
- `VectorStoreIndex`
- in-memory vector store
- 以及多种 companion crate 的向量库集成方向

这意味着 AgentJax 不应凭空再发明一套和 Rig 平行的“底层向量索引核心协议”。

---
## 4. AgentJax 应该负责的层
AgentJax 的价值不在于重新实现 Rig 已解决的基础层，而在于把这些能力组织成一个长期可演化 agent runtime。

### 4.1 Workspace 层
AgentJax 负责：
- workspace identity
- `AGENT.md` / `SOUL.md` / `USER.md` / `MISSION.md` / `RULES.md` / `ROUTER.md`
- `MEMORY.md` / `memory/**`
- `knowledge/**`
- prompt materials

### 4.2 Daemon / control plane
AgentJax 负责：
- daemon-only runtime
- API / transport / stream / subscription
- session lifecycle
- task lifecycle
- plugin lifecycle
- runtime inspection / diagnostics / control plane

### 4.3 Context / continuity / memory policy
AgentJax 负责：
- context engine
- LCM continuity
- memory vs knowledge 边界
- retrieval policy
- prompt assembly
- autonomy / safety / routing / task policy

### 4.4 Plugin runtime / resource governance
AgentJax 负责：
- plugin manager
- workspace-scoped enable / disable
- runtime config integration
- plugin lifecycle and health
- resource routing / policy

---
## 5. 哪些层必须优先复用 Rig
### 5.1 Provider client 层
应优先复用或对齐 Rig 的：
- provider client
- completion request model
- embedding request model
- provider capability mental model

不要为了 AgentJax 重新造一套几乎同构的 provider substrate。

### 5.2 Tool substrate 层
Agent-facing tool protocol 可以由 AgentJax 自己定义。
但 tool 的底层心智模型应尽量对齐 Rig：
- 有 schema
- 有 name / description
- 有 typed args
- 有 structured output

AgentJax 要做的是：
- workspace-aware file tools
- memory / knowledge retrieval tools
- daemon-level dispatch / policy / audit

而不是再重造一个“连 Rig tool 基础抽象都完全不兼容”的底层工具系统。

### 5.3 Embedding / vector index 基础层
在做 retrieval 深化时：
- embedding model 抽象优先参考 Rig
- vector store / index 抽象优先参考 Rig
- 如需 advanced RAG orchestration，可在上层扩展

但不要自己重新发明：
- embedding model base trait
- vector index base trait
- provider-agnostic semantic search substrate

如果这些与 Rig 高度重合，应尽量借 Rig 的心智模型与接口方向。

---
## 6. 哪些层不应被 Rig 反向吞掉
避免重复造轮子，不等于把 AgentJax 降格成 Rig demo app。

以下层仍应由 AgentJax 明确拥有：

### 6.1 Daemon runtime
- daemon API
- transport
- subscription / stream lifecycle
- runtime state
- control plane

### 6.2 Workspace-native agent identity
- workspace layout
- identity files
- prompt source governance
- memory / knowledge workspace semantics

### 6.3 Task / event / continuity runtime
- event log
- task timeline
- checkpoint / resume
- LCM projection / compaction / invalidation

### 6.4 Plugin system
- plugin manager
- plugin enable / disable / reload
- plugin health / dependency / config governance

Rig 是基础层，不是 AgentJax 整体 runtime 的替代品。

---
## 7. 具体禁止事项
后续开发中，以下方向默认禁止，除非有非常明确且记录在案的理由。

### 7.1 禁止重造 provider foundation
禁止：
- 再定义一套与 Rig 平行的 completion model 基础接口
- 再定义一套与 Rig 平行的 embedding model 基础接口
- 再定义一套仅为“统一 provider”而生的重复 client substrate

### 7.2 禁止重造 vector store foundation
禁止：
- 再发明一套与 Rig 平行的底层 vector store trait
- 再发明一套与 Rig 平行的底层 vector index trait

如果 AgentJax 需要自己的 retrieval policy，那应该建在更高层。

### 7.3 禁止把 AgentJax plugin manager 退化成 Rig provider wrapper
AgentJax 的插件系统必须负责：
- runtime lifecycle
- config integration
- enable / disable
- dependency ordering
- daemon-level governance

它不应只是“对 Rig provider 再套一层薄皮”。

### 7.4 禁止把 RAG 引擎做成重复的 embedding/vector 底座
AgentJax 的 RAG 层应该关注：
- collections
- libraries
- search / get tool surface
- evidence packaging
- memory / knowledge policy

不要在这一层悄悄再重复实现 Rig 已提供的底层 embedding/vector 基础能力。

---
## 8. 推荐分层
建议把 AgentJax / Rig 的关系理解为：

```text
AgentJax
  -> daemon / workspace / plugin runtime / task runtime / context engine
  -> retrieval policy / memory policy / file tools / prompt assembly
  -> Rig integration layer
      -> Rig provider abstractions
      -> Rig tool abstractions
      -> Rig embedding abstractions
      -> Rig vector store abstractions
```

这里的关键点是：
- Rig 是 AgentJax 的基础能力供应层之一
- 但 AgentJax 的高层 runtime、policy、workspace 语义必须保持自主

---
## 9. 对当前开发计划的直接影响
### 9.1 对 File Tools
`read` / `edit` / `write` 是 AgentJax 的 domain tools。
这些由 AgentJax 自己定义没问题。

但：
- tool schema
- typed args
- tool registration 心智模型

应尽量与 Rig 的 tool 模型相容，而不是背道而驰。

### 9.2 对 Retrieval Tools
`memory_search/get` 与 `knowledge_search/get` 是 AgentJax 的 domain surface。
但底层：
- embedding
- vector index
- semantic retrieval substrate

应优先参考 Rig。

### 9.3 对 Plugin Manager
Plugin manager 仍然完全属于 AgentJax。
Rig 不替代：
- `plugins.toml`
- enable / disable
- runtime health
- daemon plugin API

### 9.4 对 OpenAI provider
如果 Rig 已经足够覆盖 AgentJax 所需的 provider/model 抽象，就不应继续在 AgentJax 内部无上限扩张一套独立 provider substrate。

AgentJax 只应补：
- config integration
- runtime policy
- resource wiring
- billing / usage / session-level binding

---
## 10. 开发检查清单
每次准备新增一个“基础抽象”前，先问四个问题：

1. Rig 是否已经有同类抽象？
2. 如果有，AgentJax 这次新增的是 runtime policy，还是重复基础层？
3. 如果只是为了适配 daemon/workspace/plugin/task，能否在 AgentJax 上层包装而不是重造底层？
4. 这个新抽象会不会让 AgentJax 与 Rig 长期分叉？

如果答案是“Rig 已有，而且我们只是在重复基础层”，那默认不做。

---
## 11. 最终结论
正式结论如下：

- Rig 是 AgentJax 的基础 AI capability substrate 参考系
- AgentJax 不应重复发明 provider / embedding / vector store 基础抽象
- AgentJax 应把精力集中在 daemon、workspace、plugin runtime、task runtime、context engine、memory/knowledge policy、Agent-facing tools
- 后续所有 retrieval、provider、tool 深化工作，都必须先检查是否会和 Rig 重复
