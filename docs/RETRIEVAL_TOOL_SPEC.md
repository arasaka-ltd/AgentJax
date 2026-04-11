# AgentJax Retrieval Tool Spec
## 1. 目标
本文档定义 AgentJax 面向 Agent 的 retrieval tool surface。

核心原则：
- Agent 不直接调用一个泛化的 `rag.search`
- Agent 直接看到的是语义域工具：`memory.*` 与 `knowledge.*`
- `search` 与 `get` 明确分离

这份文档关注的是 agent-facing tool protocol，不是底层 index engine 的实现细节。

---
## 2. 一句话边界
- `RAG Engine`：底层 retrieval substrate
- `Memory System`：长期可行为化记忆域
- `Knowledge System`：领域知识库域
- `Retrieval Tools`：Agent 调用这些域能力的标准入口

换句话说：
- `RAG` 负责会找
- `Memory` 负责记该记的
- `Knowledge` 负责组织可检索的领域资料
- Tools 负责把这些能力稳定、可控地暴露给 Agent

---
## 3. 设计原则
### 3.1 不直接暴露 `RAG` 作为工具名
不建议：
- `rag.search`
- `rag.get`

建议：
- `memory.search`
- `memory.get`
- `knowledge.search`
- `knowledge.get`

原因：
- Agent 心智模型应该围绕“查什么域”，而不是“调用哪个引擎层”
- 避免把底层实现细节泄漏到 tool surface
- 允许未来替换 RAG backend 而不破坏 Agent 协议

### 3.2 `search` 与 `get` 分离
`search` 负责：
- 搜候选
- 返回摘要
- 返回引用
- 返回分数与范围信息

`get` 负责：
- 读取某个已知引用
- 读取完整文档
- 读取精确片段
- 支持按行或按 chunk inspect

### 3.3 稳定引用优先
`search` 结果应尽量返回稳定引用：
- `memory_ref`
- `doc_ref`
- `chunk_ref`

不要依赖模型自己拼路径、猜路径、重写路径。

### 3.4 范围收敛优先
较大的知识域必须支持范围参数，否则 Agent 会频繁误召回和过度读取。

优先使用：
- `scope`
- `library`
- `libraries`
- `path_prefix`
- `metadata_filters`

---
## 4. Tool Catalog
第一阶段最小闭环定义四个工具：
- `memory.search`
- `memory.get`
- `knowledge.search`
- `knowledge.get`

---
## 5. `memory.search`
### 5.1 作用
在长期记忆域中搜索相关候选结果。

典型用途：
- 找用户偏好
- 找长期决策
- 找人物 / 组织画像
- 找稳定事实与约束

### 5.2 输入
```json
{
  "query": "string",
  "top_k": 5,
  "scope": "all",
  "mode": "hybrid",
  "tags": ["optional"],
  "include_excerpt": true
}
```

### 5.3 输入字段定义
- `query: string`
  - 必填
  - 搜索查询
- `top_k?: integer`
  - 可选
  - 返回候选数量
  - 默认建议 `5`
- `scope?: string`
  - 可选
  - 建议枚举：
    - `memory_md`
    - `topics`
    - `profiles`
    - `daily`
    - `all`
- `mode?: string`
  - 可选
  - 建议枚举：
    - `keyword`
    - `semantic`
    - `hybrid`
- `tags?: string[]`
  - 可选
  - 用于未来 tag-based narrowing
- `include_excerpt?: boolean`
  - 可选
  - 是否返回摘要片段

### 5.4 输出
```json
{
  "results": [
    {
      "memory_ref": "mem:topics/project-alpha",
      "title": "project-alpha",
      "path": "memory/topics/project-alpha.md",
      "score": 0.91,
      "excerpt": "Project Alpha prefers Rust for automation...",
      "section_hint": "long-term-decisions",
      "reason": "query terms matched stable project preference"
    }
  ]
}
```

### 5.5 输出字段定义
- `memory_ref`
  - 稳定引用，优先于裸路径
- `title`
  - 供模型快速识别候选
- `path`
  - workspace 相对路径
- `score`
  - 归一化或后端定义分数
- `excerpt`
  - 精短摘要，不应是大段正文
- `section_hint?`
  - 命中 section 提示
- `reason?`
  - 可选的人类可读召回原因

### 5.6 使用策略
- 需要判断“有没有相关长期记忆”时先调用
- 已知明确目标文档时不必先 search，可直接 `memory.get`
- 不要把 `memory.search` 当成“取全文”

---
## 6. `memory.get`
### 6.1 作用
读取某个长期记忆文档的完整内容或精确行段。

### 6.2 输入
```json
{
  "memory_ref": "mem:topics/project-alpha",
  "start_line": 10,
  "end_line": 30,
  "max_lines": 80
}
```

### 6.3 输入字段定义
- `memory_ref?: string`
  - 优先使用
- `path?: string`
  - 当没有稳定引用时可回退使用
- `start_line?: integer`
  - 1-based inclusive
- `end_line?: integer`
  - 1-based inclusive
- `max_lines?: integer`
  - 防止过大读取

约束：
- `memory_ref` 与 `path` 至少提供一个
- 如果同时提供，优先按 `memory_ref` 解析
- 如果提供 `start_line` 而未提供 `end_line`，实现可按 `max_lines` 推导窗口

### 6.4 输出
```json
{
  "memory_ref": "mem:topics/project-alpha",
  "path": "memory/topics/project-alpha.md",
  "title": "project-alpha",
  "content": "10| ...\n11| ...",
  "start_line": 10,
  "end_line": 30,
  "total_lines": 64,
  "truncated": false
}
```

### 6.5 使用策略
- search 之后，需要展开某条候选时调用
- 已经知道目标文档时直接调用
- 用户明确要求“读取某个记忆条目”时调用

---
## 7. `knowledge.search`
### 7.1 作用
在领域知识库中搜索证据候选。

典型用途：
- 查 Blender 手册
- 查 Rust 文档
- 查项目资料
- 查 API / 产品说明

### 7.2 输入
```json
{
  "query": "borrow checker lifetime elision",
  "top_k": 5,
  "library": "rust",
  "mode": "hybrid",
  "path_prefix": "book/",
  "metadata_filters": {
    "version": "stable"
  },
  "include_excerpt": true
}
```

### 7.3 输入字段定义
- `query: string`
  - 必填
- `top_k?: integer`
  - 默认建议 `5`
- `library?: string`
  - 单个知识库范围，例如 `rust`
- `libraries?: string[]`
  - 多知识库范围
- `path_prefix?: string`
  - 对某个知识库内路径前缀继续收窄
- `mode?: string`
  - `keyword` / `semantic` / `hybrid`
- `metadata_filters?: object`
  - 面向后续 index metadata
- `include_excerpt?: boolean`
  - 是否返回摘要片段

说明：
- `library` 是 Agent-facing 范围参数
- `collection` 属于底层实现抽象，不建议第一阶段直接暴露给 Agent

### 7.4 输出
```json
{
  "results": [
    {
      "doc_ref": "doc:rust/book/ch10-lifetimes",
      "library": "rust",
      "path": "knowledge/rust/book/ch10-lifetimes.md",
      "title": "Lifetimes",
      "score": 0.88,
      "excerpt": "Lifetime elision rules allow ...",
      "chunk_ref": "chunk:rust/ch10/14",
      "metadata": {
        "section": "10.3"
      }
    }
  ]
}
```

### 7.5 使用策略
- 需要“证据型回答”时优先调用
- 已知用户在问哪个知识域时，应尽量带上 `library`
- 对大型知识库尽量不要无范围搜索后立刻全文读取

---
## 8. `knowledge.get`
### 8.1 作用
读取知识库中的完整文档或精确片段。

### 8.2 输入
```json
{
  "doc_ref": "doc:rust/book/ch10-lifetimes",
  "start_line": 40,
  "end_line": 80,
  "library": "rust"
}
```

### 8.3 输入字段定义
- `doc_ref?: string`
  - 优先使用
- `path?: string`
  - 无稳定引用时回退使用
- `library?: string`
  - 用于定位或校验知识库范围
- `start_line?: integer`
  - 1-based inclusive
- `end_line?: integer`
  - 1-based inclusive
- `chunk_ref?: string`
  - 面向后续 chunk inspect
- `max_lines?: integer`
  - 读取预算保护

### 8.4 输出
```json
{
  "doc_ref": "doc:rust/book/ch10-lifetimes",
  "library": "rust",
  "path": "knowledge/rust/book/ch10-lifetimes.md",
  "content": "40| ...\n41| ...",
  "start_line": 40,
  "end_line": 80,
  "total_lines": 240,
  "truncated": false
}
```

### 8.5 使用策略
- search 返回候选后做正文 inspect
- 用户明确要求“打开某篇文档/某几行”时使用
- 不要把它当大规模扫库工具

---
## 9. Agent 使用策略
### 9.1 什么时候优先用 `memory.*`
- 查询用户偏好
- 查询长期决策
- 查询稳定规则
- 查询人物 / 组织画像
- 查询会影响未来行为的长期约束

### 9.2 什么时候优先用 `knowledge.*`
- 查询外部资料
- 查询技术手册
- 查询 API / 产品文档
- 查询项目资料
- 查询需要证据支撑的领域知识

### 9.3 什么时候直接 `get`
- 已知明确引用
- 已知明确文档
- 用户要求读取具体文件或具体行段

### 9.4 禁止倾向
- 不要把 `memory` 当通用知识库
- 不要把 `knowledge` 当长期行为约束仓库
- 不要把 `search` 当 `get`
- 不要在没有范围限制时反复全文读取大知识库

---
## 10. Line-range 读取规范
所有 `get` 类工具建议遵循：
- 行号为 1-based
- `start_line` 与 `end_line` 都是 inclusive
- 返回内容建议带行号前缀，便于引用与二次检索
- 如果请求范围越界，应裁剪到有效范围，而不是直接失败
- 如果请求范围过大，可返回截断结果并设置 `truncated=true`

---
## 11. Naming 与引用规范
### 11.1 Memory 引用
建议稳定格式：
- `mem:root`
- `mem:topics/<slug>`
- `mem:profiles/<slug>`
- `mem:daily/<date>`

### 11.2 Knowledge 引用
建议稳定格式：
- `doc:<library>/<doc-id>`
- `chunk:<library>/<doc-id>/<chunk-id>`

### 11.3 路径规范
建议：
- `memory/topics/*.md`
- `memory/profiles/*.md`
- `knowledge/<library>/**`

---
## 12. 向后扩展方向
这四个工具是最小闭环，不是终局。
后续可渐进扩展：
- citations / source spans
- neighbors
- aggregate
- evidence packs
- rerank controls
- inspect modes
- structured metadata filters

但扩展原则不变：
- domain-oriented tool naming
- `search` / `get` 分离
- 范围收敛优先
- 稳定引用优先
