# AgentJax Prompt Assembly Spec

## 1. 目标
本文档定义 AgentJax 的提示词拼装、结构化注入与 workspace Markdown 源格式。

要解决的问题不是“把几段字符串拼起来”。
而是：
- 如何让 agent 清楚知道自己是谁
- 如何让 agent 清楚知道有哪些工具可用
- 如何让 workspace 文件保持人类可编辑，同时又能被稳定注入模型
- 如何避免 prompt 最后退化成不可控的大杂烩文本

这里先拍死一个原则：
**workspace 文件以 Markdown 编写，runtime 不直接把原文裸塞给模型，而是先解析成结构化块，再渲染成 XML 注入。**

---
## 2. 一句话定义
- Markdown 是作者格式
- Context Blocks 是运行时中间格式
- XML 是模型注入格式

也就是说：
**Markdown for humans, structured blocks for runtime, XML for model injection.**

---
## 3. 为什么必须做 Prompt Assembly
如果没有这一层：
- 工具信息不会被稳定暴露给模型
- workspace 文件会以任意风格漂移
- prompt 很快变成难以维护的大段拼接文本
- 后续 memory / RAG / task / tools 都会因为注入边界不清而发臭

特别是工具这件事：
**如果工具没有进入明确的提示词协议，Agent 事实上就“不知道自己能用工具”。**

---
## 4. 三层模型
### 4.1 Source Layer
来源层，面向人类维护：
- `AGENT.md`
- `SOUL.md`
- `USER.md`
- `MISSION.md`
- `RULES.md`
- `ROUTER.md`
- `MEMORY.md`
- `prompts/**`

### 4.2 Structured Prompt Layer
运行时中间层：
- `WorkspaceIdentityPack`
- `ContextBlock`
- `ToolDescriptor`
- `MemoryRecall`
- `RetrievedKnowledge`
- `TaskState`
- `RuntimeDirective`

### 4.3 Rendered Prompt Layer
面向模型最终注入层：
- XML prompt document
- provider-specific final wrapper

---
## 5. Prompt Assembly Pipeline
建议固定成以下步骤：
1. load workspace markdown sources
2. parse to normalized prompt documents
3. convert to structured context blocks
4. merge tool / memory / retrieval / task state
5. apply budget and ordering policy
6. render to XML
7. pass rendered XML to provider adapter

必须明确：
- provider 不负责理解 workspace Markdown
- provider 只消费已经组装好的 prompt

---
## 6. Prompt Render 的硬边界
### 6.1 不允许直接裸拼
不允许：
- 直接把所有 Markdown 文件拼成一个大字符串
- 把工具说明临时写成一段无结构 prose
- 把 retrieval 结果随手 append 到 prompt 尾部

### 6.2 必须区分来源
注入时必须能区分：
- identity
- rules
- mission
- router policy
- user profile
- memory
- retrieved knowledge
- tool availability
- task state
- latest user message
- conversation message metadata

### 6.3 必须保留标签语义
模型看到的不是“普通正文”，而是：
- 这是规则
- 这是人格
- 这是长期记忆
- 这是工具清单
- 这是用户刚刚说的话

XML 的意义就在这里。

---
## 7. XML 注入格式
建议最终统一渲染为：

```xml
<agentjax_prompt version="v1">
  <identity>
    <agent>...</agent>
    <soul>...</soul>
    <mission>...</mission>
    <rules>...</rules>
    <router>...</router>
    <user>...</user>
  </identity>

  <memory>
    <item kind="profile">...</item>
    <item kind="preference">...</item>
  </memory>

  <knowledge>
    <item source="knowledge/docs">...</item>
  </knowledge>

  <tools>
    <tool name="read_file">
      <description>Read a file from the workspace.</description>
      <when_to_use>When the user asks about file contents.</when_to_use>
      <arguments_schema>...</arguments_schema>
    </tool>
  </tools>

  <task_state>
    <goal>...</goal>
    <status>...</status>
  </task_state>

  <conversation>
    <latest_user_message>...</latest_user_message>
  </conversation>
</agentjax_prompt>
```

这不是要求一开始就做到最复杂。
但标签层级和语义边界必须先定下来。

---
## 8. Structured Message Injection
除了 identity、rules、tools、memory 这些稳定块之外，进入模型的消息本身也应采用结构化 XML 注入。

原因：
- 让模型清楚知道消息来自哪里、何时到达、属于哪个 session
- 减少模型对时间、渠道、身份的猜测
- 让程序侧更优雅地生成与解析消息上下文

建议统一格式：

```xml
<conversation>
  <message kind="user">
    <meta>
      <message_id>msg_123</message_id>
      <session_id>sess_1</session_id>
      <channel>telegram</channel>
      <surface>webui.local</surface>
      <actor_id>user_42</actor_id>
      <received_at>2026-04-10T12:34:56Z</received_at>
      <locale>zh-CN</locale>
    </meta>
    <content><![CDATA[帮我看看这个报错]]></content>
  </message>
</conversation>
```

### 8.1 统一 message kinds
建议至少支持：
- `user`
- `assistant`
- `tool_result`
- `system`
- `runtime`

### 8.2 每条消息的统一结构
每条 message 至少应包含：
- `kind`
- `meta`
- `content`

`meta` 至少建议支持：
- `message_id`
- `session_id`
- `channel`
- `surface`
- `actor_id`
- `received_at`
- `locale`

### 8.3 内容与推断分离
必须明确：
- 用户原文进入 `<content>`
- 程序推断、标签、摘要、解释不要混进 `<content>`
- 推断性内容如有需要，应进入独立节点，例如 `<annotations>`

这样可以避免模型把运行时推断误当成用户明确表达的事实。

---
## 9. Workspace Markdown 的推荐源格式
这些文件是作者写的 Markdown，不是 XML。
但为了让 runtime 稳定解析，建议采用受约束的 Markdown 结构。

### 8.1 `AGENT.md`
建议包含：
- `## Role`
- `## Working Style`
- `## Execution Habits`
- `## Collaboration Defaults`

### 8.2 `SOUL.md`
建议包含：
- `## Voice`
- `## Values`
- `## Tone`
- `## Anti-Patterns`

### 8.3 `MISSION.md`
建议包含：
- `## Mission`
- `## Success Criteria`
- `## Autonomy Boundary`

### 8.4 `RULES.md`
建议包含：
- `## Hard Rules`
- `## Safety Rules`
- `## Escalation Rules`

### 8.5 `ROUTER.md`
建议包含：
- `## Routing Policy`
- `## Tool Use Policy`
- `## Memory Use Policy`
- `## Model Selection Policy`

### 8.6 `USER.md`
建议包含：
- `## User Profile`
- `## Preferences`
- `## Communication Style`

### 8.7 `MEMORY.md`
建议包含：
- `## Stable Facts`
- `## Preferences`
- `## Long-Term Decisions`
- `## Open Loops`

---
## 10. 解析规则
### 9.1 Markdown 源规则
- 允许自由 prose
- 但应优先解析二级标题
- 空文档不应导致拼装失败
- 未识别 section 可进入 `misc`

### 9.2 运行时归一化规则
运行时应把 Markdown 文件转成统一结构，例如：
- `PromptDocument`
- `PromptSection`
- `PromptFragment`

每个 fragment 至少应带：
- source file
- section title
- content
- priority
- freshness

---
## 11. 工具注入规范
工具必须以结构化 XML 显式注入。

每个工具至少要提供：
- `name`
- `description`
- `when_to_use`
- `when_not_to_use`
- `arguments_schema`

建议格式：

```xml
<tools>
  <tool name="shell">
    <description>Run a shell command in the workspace.</description>
    <when_to_use>Use for local inspection or implementation tasks.</when_to_use>
    <when_not_to_use>Do not use for destructive commands unless explicitly authorized.</when_not_to_use>
    <arguments_schema>{"cmd":"string"}</arguments_schema>
  </tool>
</tools>
```

如果没有这层，模型只能“隐约猜到也许有工具”，这不够。

---
## 12. Prompt Assembly 与 Context Engine 的关系
- Context Engine 负责决定哪些 block 进入当前工作集
- Prompt Assembly 负责把这些 block 渲染成模型可消费的最终格式

不要混淆：
- `assemble_context()` 负责选什么
- `render_prompt_xml()` 负责怎么喂

这两个阶段必须分开。

---
## 13. 最小实现建议
第一版只需要做到：
- workspace Markdown 读取
- 解析出主要 section
- 形成 `PromptDocument`
- 把 `ContextBlock + ToolDescriptor + latest_user_message` 渲染成 XML
- 让 `session.send` 走 XML prompt，而不是自由拼接字符串

之后的增强项可包括：
- 统一 structured message injection
- assistant / tool_result / system message 的统一 XML schema
- `<annotations>` 与推断字段分离

第一版不必做：
- provider-specific XML dialect
- 多模板切换
- prompt compression DSL
- XML schema validation engine

---
## 14. 与其他规范的关系
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`：定义 workspace 文件边界
- `docs/LCM_CONTEXT_ENGINE.md`：定义 context assembly，不定义最终 prompt 渲染格式
- `docs/PLUGIN_SDK.md`：定义 `PromptRenderer` 这类能力边界
- `docs/DAEMON_API_IPC_SCHEMA.md`：定义 daemon API，不定义模型 prompt 格式

---
## 15. 硬结论
AgentJax 应明确采用：
- Markdown 作为 workspace 作者格式
- 结构化 prompt documents / context blocks 作为运行时中间层
- XML 作为模型注入格式

一句话拍板：
**Workspace 写 Markdown，Runtime 做结构化归一化，最终向 LLM 注入 XML。**
