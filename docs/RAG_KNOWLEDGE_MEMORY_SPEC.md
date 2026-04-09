# AgentJax RAG / Knowledge / Memory Spec
## 1. 目标
本文档定义 AgentJax 中 `RAG`、`Knowledge`、`Memory` 的正式分层与职责边界。
要先拍死一个顺序：
- `Context Engine` 负责单会话 / 单任务连续性
- `RAG Engine` 负责通用检索基础设施
- `Knowledge Systems` 负责特定知识域组织
- `Memory System` 负责长期可行为化记忆

正确方向不是“把 Memory 做成万能检索引擎”，而是：
**RAG is generic retrieval infrastructure. Memory is a constrained, policy-driven domain built on top of RAG.**

翻成大白话：
- RAG 什么都能收
- Memory 只收“值得长期保留并影响未来行为”的东西

---
## 2. 四层关系
### 2.1 Context Engine
解决：
- 单会话 / 单任务连续性
- 长对话不失忆
- 历史可压缩、可展开、可恢复

它不负责：
- 知识库检索
- 通用语义召回
- 长期用户画像沉淀

### 2.2 RAG Engine
解决：
- 面向任意语料的检索、排序、扩展、聚合
- 支持知识库、文档库、会话库、工件库、记忆库
- 作为框架级 retrieval substrate 为上层系统提供统一底座

`RAG Engine` 是核心插件化子系统，但地位接近内建基础设施，而不是普通边角插件。

### 2.3 Knowledge Systems
解决：
- 面向特定领域组织知识域
- 例如项目知识库、产品文档库、API 文档库、个人笔记库
- 在通用 RAG 之上定义更稳定的 collection、schema、治理规则与召回策略

`Knowledge` 比 `Memory` 大，比“任意东西都能塞”的通用 `RAG` 小。

### 2.4 Memory System
解决：
- 哪些内容值得长期记住
- 用户画像、项目事实、偏好、长期决策、稳定规则
- 写入、提纯、召回、更新、失效

`Memory` 不是另一个检索引擎。
`Memory` = 基于 `RAG` 的特化长期语义域。

---
## 3. 一句话边界
一句话总结：
- `LCM / Context Engine` 负责不失忆
- `RAG Engine` 负责会找
- `Memory System` 负责记该记的

进一步说：
- `Context Engine` 回答“你为什么还没失忆”
- `RAG Engine` 回答“你能从哪里把相关证据找回来”
- `Memory System` 回答“哪些东西值得长期留下并影响未来行为”

---
## 4. RAG Engine 的正式定义
`RAG Engine` 不是“向量搜索插件”。
它是正经的 retrieval substrate，至少覆盖三类能力。

### 4.1 检索能力
- keyword / full-text search
- embedding semantic search
- rerank
- hybrid search
- neighborhood expansion
- graph/link expansion
- result fusion
- chunk inspection

### 4.2 索引能力
- document ingest
- chunking
- metadata extraction
- embedding generation
- incremental update
- delete / tombstone / reindex

### 4.3 结果能力
- scored results
- grouped results
- cited source spans
- expanded snippets
- structured evidence pack

### 4.4 治理能力
- collection / library abstraction
- schema / versioning
- freshness metadata
- dedupe
- source-of-truth tracking

---
## 5. RAG Engine 原生负责什么
核心 `RAG` 插件提供的是抽象、管线与最小默认实现，不是把所有后端内嵌死。

### 5.1 核心 RAG 子系统负责
- retrieval pipeline
- indexing pipeline
- collection abstraction
- search orchestration
- provider routing
- result normalization
- evidence packaging

### 5.2 具体依赖由资源层或其他插件提供
- embedding model
- reranker model
- expansion model
- vector store backend

这个边界必须明确，否则后面会把 `RAG` 插件做成难以替换的内嵌杂物堆。

---
## 6. RAG Engine 内部抽象
### 6.1 Corpus / Collection
`Collection` 是检索单位，例如：
- `docs`
- `knowledge`
- `memory`
- `session-history`
- `artifacts`
- `codebase`
- `notes`

每个 `collection` 至少应有：
- schema
- chunking policy
- metadata fields
- retention policy
- retrieval defaults

### 6.2 Index Pipeline
负责写入，建议步骤：
1. source normalize
2. parse
3. chunk
4. enrich metadata
5. embed
6. store
7. update index manifests

### 6.3 Query Pipeline
负责读取，建议步骤：
1. parse query
2. choose retrieval mode
3. keyword search
4. semantic search
5. merge
6. rerank
7. expand
8. return evidence pack

### 6.4 Backend Drivers
后端应可插拔，例如：
- sqlite fts
- local flat index
- LanceDB
- Qdrant
- pgvector

### 6.5 Model Drivers
模型驱动应可插拔，例如：
- embedder
- reranker
- expander / summarizer

---
## 7. 查询模式
不要只暴露一个粗糙的 `search(query)`。
`RAG Engine` 至少应支持：
- `keyword`
- `semantic`
- `hybrid`
- `expand`
- `inspect`
- `neighbors`
- `aggregate`

说明：
- `keyword`：查精确词、标识符、命令、路径、版本号
- `semantic`：查近义与概念相关内容
- `hybrid`：常规默认模式
- `expand`：对候选结果展开上下文
- `inspect`：读取命中的完整块或文档片段
- `neighbors`：拿命中块周边 chunk
- `aggregate`：做结果归并、分组、证据汇总

---
## 8. Knowledge Systems
`Knowledge Systems` 是构建在 `RAG` 之上的领域化知识层。

它们通常用于：
- 项目知识库
- 产品文档库
- API 文档库
- 个人笔记库
- 代码与工件知识域

`Knowledge System` 通常表现为：
- 一组专用 collections
- 一套 schema / metadata 约束
- 一套领域化 ingest 与 retrieval defaults
- 一套来源治理与更新策略

它不是 `Memory`，因为它不要求内容必须“长期影响 agent 行为”。

---
## 9. Memory System 如何站在 RAG 上面
`Memory System` 不应重新发明存储与检索。
它应该只是：
- 一组专用 collections
- 一套写入规则
- 一套提纯规则
- 一套召回策略
- 一套冲突 / 失效策略

例如 `Memory` 可以有这些 collection：
- `profile`
- `preferences`
- `projects`
- `decisions`
- `relationships`
- `open_loops`
- `lessons`
- `operational_notes`

这意味着：
- `Memory` 使用 `RAG` 的索引和查询底座
- `Memory` 额外定义更严格的进入条件与生命周期
- `Memory` 的价值不在“搜得到”，而在“该不该写进去”

---
## 10. Memory 的四个核心策略
### 10.1 Promotion Policy
什么能进长期记忆。

### 10.2 Conflict Policy
新证据和旧记忆冲突时如何处理。

### 10.3 Freshness Policy
哪些记忆会过期、降权、待验证。

### 10.4 Behavioral Relevance Policy
这条记忆会不会影响未来行为；不会就别进。

这四个策略决定 `Memory` 像不像脑子，而不是像垃圾桶。

---
## 11. 写入成本原则
必须明确：
**Memory 的写入应该比 RAG 的写入更贵。**

也就是：
- `RAG` 可以大量 ingest
- `Knowledge` 可以按领域批量 ingest 与周期更新
- `Memory` 必须经过筛选、提纯、去重、冲突检测

否则最后所谓“记忆系统”只是换了名字的 dump 库。

---
## 12. 最小默认实现建议
核心 `RAG` 子系统内建：
- collection abstraction
- FTS / keyword retrieval
- retrieval pipeline
- hybrid merge
- rerank hooks
- expansion hooks
- evidence packaging

默认最小后端：
- SQLite FTS + metadata store
- embedding / rerank / expand 通过资源层接模型

后续可扩展插件：
- LanceDB backend
- Qdrant backend
- pgvector backend
- custom embedder plugin
- custom reranker plugin

---
## 13. 与其他规范的关系
- `docs/LCM_CONTEXT_ENGINE.md`：定义会话连续性，不负责知识库检索
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`：定义 `knowledge/` 与 `memory/` 在工作区中的布局
- `docs/PLUGIN_SDK.md`：定义 `RAG`、`Knowledge`、`Memory` 的插件与资源边界

---
## 14. 硬结论
AgentJax 应明确采用以下分层：
- `LCM / Context Engine`：保证无限对话连续性
- `RAG Engine`：提供通用检索基础设施
- `Knowledge Systems`：组织特定领域知识库
- `Memory System`：作为 `RAG` 之上的特化长期记忆域

一句话拍板：
**LCM 负责不失忆，RAG 负责会找，Memory 负责记该记的。**
