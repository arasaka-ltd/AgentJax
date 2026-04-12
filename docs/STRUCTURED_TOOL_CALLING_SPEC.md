# AgentJax Structured Tool Calling Spec
## 1. 目标
本文档定义 AgentJax 的下一阶段 tool calling 正式协议。

要解决的不是“把 `TOOL_CALL {...}` 这行字符串换个写法”。
而是：
- 把当前文本式 tool-calling 协议升级成结构化协议
- 定义真正可扩展的 `tool calling loop (0..n)`
- 在暂时做不到 token-by-token 真流式时，定义合理的语义流式输出
- 为后续 `sleep`、shell session、多步 task runtime、headless autonomy 打基础

一句话：
**Tool calling 不应再依赖模型输出一段可解析文本，而应升级为 runtime 原生结构化执行协议。**

---
## 2. 现状问题
当前文本式协议的问题很明确：
- tool call 是通过输出文本里的特殊前缀表达，例如 `TOOL_CALL {...}`
- runtime 要靠字符串解析才能知道模型是不是想调工具
- 工具调用和普通 assistant 文本混在一起，边界脆弱
- 很难稳定支持一次响应里多个 tool call
- 很难扩展 `sleep`、`wait`、structured partial output
- 更难和后续 provider-native structured output 对齐

同时，当前流式回复也有一个现实约束：
- 还做不到真正 token-by-token 的 provider 原生流式转发
- 所以现在的“流式”更接近最终文本切块后再发送

这意味着我们要同时解决两个问题：
1. tool calling 协议升级
2. 流式语义重新定义

---
## 3. 一句话边界
- `Prompt Assembly` 负责告诉模型有哪些工具、工具语义和使用规则
- `Provider Adapter` 负责把 provider-specific 响应归一化成结构化输出项
- `Tool Loop Runtime` 负责识别工具调用、执行工具、回写结果、继续循环
- `Stream Layer` 负责把这些结构化步骤以语义事件流发给客户端

也就是说：
- 模型不再“输出一行特殊字符串”
- runtime 接收的是结构化 item
- 客户端收到的是结构化 stream event

---
## 4. 设计原则
### 4.1 不再把 tool call 当作文本协议
不建议继续依赖：
- `TOOL_CALL {"tool":"read",...}`
- 特殊 XML/Markdown fenced payload 再靠 regex 或前缀解析

正确方向应是：
- provider-native structured tool calling
- 或 runtime 内部统一的结构化 item 协议

### 4.2 Tool Loop 应是 runtime 原生状态机
tool loop 不应只是：
- 模型调用一次
- 如果像 tool call 就调一次工具
- 再问一次模型

而应是正式状态机：
- 模型输出结构化 item
- runtime 判断接下来是输出、调工具、等待、还是结束
- 如有工具结果则回到模型
- 可重复多轮直到完成或触发停止条件

### 4.3 输出项必须结构化
assistant 的一次模型响应不应只有一大段字符串。

应建模成：
- 文本输出项
- tool call 项
- tool result 项
- runtime control 项，例如 `sleep`
- 结束项 / 完成原因

### 4.4 流式应面向语义块，不伪装成 token 流
在暂时做不到 provider 原生 token stream 的阶段，不要假装自己在推 token。

更合理的做法是：
- 把流定义为结构化语义事件流
- 可以有文本增量
- 也可以有 tool_call / tool_result / waiting / resumed / completed

这样客户端看到的是：
- “模型正在回答”
- “模型决定调用工具”
- “工具完成了”
- “模型继续回答”

而不是误以为一定是 provider token 级转发。

### 4.5 Provider 差异应收敛在 adapter 层
不同 provider 的响应风格可能不同：
- 原生 tool call
- function call
- response item list
- 纯文本

这些差异不应泄漏到 daemon 主逻辑。
daemon 应只消费统一的内部结构。

---
## 5. 内部统一输出模型
建议引入统一内部模型：

```rust
pub struct ModelTurnOutput {
    pub output_id: String,
    pub items: Vec<ModelOutputItem>,
    pub finish_reason: FinishReason,
    pub usage: Option<ModelUsage>,
}
```

```rust
pub enum ModelOutputItem {
    AssistantText(AssistantTextItem),
    ToolCall(ToolCallItem),
    ToolResult(ToolResultItem),
    RuntimeControl(RuntimeControlItem),
}
```

```rust
pub struct AssistantTextItem {
    pub item_id: String,
    pub text: String,
    pub is_partial: bool,
}
```

```rust
pub struct ToolCallItem {
    pub item_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub timeout_secs: Option<u64>,
}
```

```rust
pub struct ToolResultItem {
    pub item_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub is_error: bool,
}
```

```rust
pub enum RuntimeControlItem {
    Sleep(SleepRequest),
}
```

### 5.1 为什么需要 item list
因为一次模型输出未来可能同时包含：
- 一段可直接展示的说明
- 一个或多个 tool call
- 一个等待指令

而不是永远只有“纯文本”或“纯工具”二选一。

---
## 6. Tool Calling Loop 正式定义
### 6.1 标准循环
建议统一为：

1. build context
2. call model
3. normalize provider response to `ModelTurnOutput`
4. inspect output items
5. if only assistant text and finished -> finalize
6. if there are tool calls -> dispatch tool execution
7. append structured tool results
8. continue loop
9. if there is runtime control such as `sleep` -> suspend
10. on resume, continue loop

### 6.2 停止条件
tool loop 至少应支持以下停止条件：
- 模型给出最终 assistant 输出，且无待执行 item
- 触发 `sleep`
- 达到 `max_tool_iterations`
- 达到 `max_tool_calls_per_turn`
- 遇到不可恢复错误
- 用户取消 / task 取消

### 6.3 第一阶段建议
第一阶段建议支持：
- 多轮 loop
- 每轮允许多个 structured items
- 多个 tool call 默认顺序执行

不必第一阶段就做：
- 复杂依赖图
- 部分 tool call 并行 + 部分串行混合调度

---
## 7. 多 tool call 语义
### 7.1 一次响应里允许多个 tool call
结构化协议应允许一次模型响应产生多个 tool call：

```json
{
  "items": [
    {
      "type": "tool_call",
      "tool_call_id": "call_1",
      "tool_name": "read",
      "args": { "path": "Cargo.toml" }
    },
    {
      "type": "tool_call",
      "tool_call_id": "call_2",
      "tool_name": "memory.search",
      "args": { "query": "workspace defaults" }
    }
  ]
}
```

### 7.2 第一阶段默认顺序执行
虽然协议允许多个 tool call，但 runtime 第一阶段建议默认：
- 按 item 顺序执行
- 每个结果都记录到 event / transcript
- 再统一回写给下一轮模型

### 7.3 后续可扩展并行
未来如要并行，应由 runtime 显式决定，而不是让模型隐含决定。

---
## 8. Tool Result 回写协议
工具执行后，不应再只是把结果拼成一段普通文本。

建议统一回写为结构化 item：

```json
{
  "type": "tool_result",
  "tool_call_id": "call_1",
  "tool_name": "read",
  "content": "{\"path\":\"Cargo.toml\",\"kind\":\"text\",...}",
  "metadata": {
    "ok": true
  }
}
```

如果失败：

```json
{
  "type": "tool_result",
  "tool_call_id": "call_1",
  "tool_name": "read",
  "content": "file not found",
  "metadata": {
    "ok": false,
    "error_code": "not_found"
  },
  "is_error": true
}
```

这样做的价值：
- 回放稳定
- provider 输入清晰
- LCM / transcript / event log 一致

---
## 9. 与 `sleep` 的关系
结构化 tool calling 之后，`sleep` 不应被建模成普通 assistant 文本。

而应是：

```json
{
  "type": "runtime_control",
  "control": "sleep",
  "duration_ms": 10000,
  "reason": "wait for shell output"
}
```

这样 runtime 才能：
- checkpoint
- 切 task 为 waiting
- 在未来恢复

---
## 10. Provider Adapter 归一化职责
不同 provider 可能返回：
- tool call object
- function call object
- response item list
- 纯文本 completion

adapter 层应负责把这些统一转换为 `ModelTurnOutput`。

### 10.1 Provider-native 优先
如果 provider 原生支持 structured tool calling，应优先使用原生能力。

### 10.2 文本 fallback 仅作为过渡
如果某 provider 暂时只支持文本：
- 可在 adapter 内部做过渡性解析
- 但这不应成为 runtime 主协议

也就是说：
- fallback 可以暂时保留
- runtime 本体不能再围绕文本前缀设计

---
## 11. 流式输出正式定义
### 11.1 当前限制
当前阶段还无法保证：
- provider token-by-token 原生流
- 每个 token 实时穿透到客户端

因此必须明确：
**第一阶段的 streaming 是 semantic streaming，不是 guaranteed token streaming。**

### 11.2 推荐 stream event 类型
建议至少支持：
- `turn.started`
- `assistant.text.delta`
- `tool_call.started`
- `tool_call.completed`
- `tool_call.failed`
- `runtime.waiting`
- `task.resumed`
- `assistant.completed`
- `turn.completed`

### 11.3 文本增量的来源
文本增量可以来自三种情况：
- provider 原生文本 delta
- provider 完整输出后的 runtime 切块
- tool loop 某轮 assistant text 的增量拼装

客户端不应假设它一定是 provider token。

### 11.4 结构化 stream chunk 示例
```json
{
  "type": "stream",
  "stream_id": "str_1",
  "phase": "chunk",
  "event": "tool_call.started",
  "seq": 3,
  "data": {
    "tool_call_id": "call_1",
    "tool_name": "read",
    "args": {
      "path": "Cargo.toml"
    }
  }
}
```

再例如：
```json
{
  "type": "stream",
  "stream_id": "str_1",
  "phase": "chunk",
  "event": "assistant.text.delta",
  "seq": 8,
  "data": {
    "text": "我已经读取了配置文件，"
  }
}
```

---
## 12. Prompt Assembly 对 Tool Calling 的要求
Prompt 中仍需显式注入：
- 工具清单
- 参数 schema
- 何时该用 / 不该用
- tool result message schema

但不应继续依赖这种要求：
- “如果要调工具，请回复一行 `TOOL_CALL ...`”

正确方向应是：
- prompt 告诉模型工具能力与规则
- provider/tool adapter 负责结构化交互

也就是说：
**Prompt 负责认知暴露，不负责文本协议编码。**

---
## 13. Event Model
tool loop 进入结构化后，建议至少记录：
- `model_called`
- `model_output_received`
- `tool_call_requested`
- `tool_called`
- `tool_completed`
- `tool_failed`
- `runtime_wait_requested`
- `task_waiting`
- `task_resumed`
- `assistant_output_finalized`

其中：
- `tool_call_requested` 表示模型提出了结构化工具调用
- `tool_called` 表示 runtime 真开始执行

这两者不要混成一个事件。

---
## 14. 与 IPC / Stream Schema 的关系
Daemon 对外协议应逐步支持更丰富的 stream 事件，而不是只发文本块。

也就是说：
- `stream` envelope 保持不变
- `stream.event` 与 `stream.data` 语义扩展

推荐客户端能力：
- 能渲染文本增量
- 能显示 tool 调用开始/结束
- 能显示 waiting/resumed
- 能在未来显示多轮 tool loop timeline

---
## 15. 第一阶段最小验收标准
- runtime 内部有统一 `ModelOutputItem` 或等价结构
- 不再依赖 daemon 主逻辑直接解析 `TOOL_CALL ...` 文本
- 支持真正的 `tool loop (0..n)`，而不是最多一轮
- 支持一个模型响应里携带多个 tool call item
- tool result 以结构化 item 回写
- stream 层能发出文本以外的语义事件
- 文档明确 semantic streaming 与 token streaming 的区别

---
## 16. 非目标
第一阶段不要求：
- 所有 provider 都支持原生 token stream
- 所有 provider 都支持原生多 tool call 并行
- 完整 reasoning trace 暴露
- 客户端 UI 一次就把所有结构化状态显示得非常漂亮

第一阶段真正要做的是：
**把 tool calling 从“文本技巧”升级为“runtime 协议”。**
