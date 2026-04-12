# AgentJax Runtime Control Tool Spec
## 1. 目标
本文档定义 AgentJax 面向 Agent 的 runtime control tool surface。

这类工具不是业务工具，也不是文件工具、检索工具、shell 工具。
它们的职责是：
- 控制 runtime 自己的执行节奏
- 让 Agent 能显式等待、恢复、轮询、让出执行权
- 为后续真正的 `tool calling loop (0..n)` 与 headless task runtime 打基础

第一阶段先定义一个最小但非常关键的工具：
- `sleep`

---
## 2. 为什么需要 `sleep`
如果没有 `sleep`，Agent 在遇到长任务时通常只有几种很差的选择：
- 忙等，不断重复调用同一个工具
- 立即结束 turn，把“稍后再看”变成人类外部记忆
- 强行阻塞当前执行线程，浪费 runtime 资源

而一个真正可演化的 agent runtime 应允许：
- 启动长任务
- 显式等待一段时间
- 在未来恢复同一个 task / turn continuation
- 再去检查 shell session、node、schedule、外部 job 的状态

简单说：
**`sleep` 不是为了偷懒，而是为了让 Agent 学会“等待”这件事。**

---
## 3. 一句话边界
- `sleep`：请求 runtime 在一段时间后再恢复当前执行链

它不是：
- OS 级线程睡眠 API
- shell 的 `sleep 10`
- provider 侧 blocking call

它本质上是：
**一个 runtime suspension primitive。**

---
## 4. 适用场景
### 4.1 长命令轮询
例如：
1. Agent 在 `shell.session.exec` 里启动 `npm test --watch`
2. 当前没有最终结果
3. Agent 调用 `sleep`
4. 一段时间后恢复，再调用 `shell.session.read`

### 4.2 外部异步任务
例如：
- 等 CI
- 等远程节点完成任务
- 等 scheduler 触发后的副作用稳定
- 等文件生成

### 4.3 分步自治
例如：
- 先启动任务
- 稍后回来检查
- 根据状态继续下一步

---
## 5. 设计原则
### 5.1 `sleep` 必须是调度语义，不是阻塞语义
调用 `sleep` 后，不应阻塞 daemon worker 线程空转等待。

正确语义应该是：
- 当前 task / turn 进入 waiting 状态
- runtime 记录唤醒时间
- 到时后重新调度

### 5.2 `sleep` 必须绑定可恢复上下文
`sleep` 不应只是“10 秒后发个空回调”。

至少应与以下上下文绑定：
- `task_id`
- `turn_id` 或 continuation id
- `session_id`（如果存在）
- 当前 resume pack / checkpoint

### 5.3 `sleep` 必须可被审计
每次 `sleep` 都应进入 event log。

因为它实际上表达了：
- agent 决定暂时不继续
- agent 计划未来某个时间再恢复

### 5.4 `sleep` 是工具循环控制能力，不是用户可见内容
它更像 runtime-internal tool，而不是普通用户业务工具。

所以：
- prompt 中可以暴露给模型
- UI 不一定要像普通工具那样强调展示
- 但 runtime 必须能看见并追踪它

---
## 6. Tool Catalog
第一阶段：
- `sleep`

后续可扩展：
- `wait_until`
- `yield`
- `await_event`
- `await_schedule`

但第一阶段不需要一口气全做。

---
## 7. `sleep`
### 7.1 作用
请求 runtime 暂停当前执行链，并在未来某个时间点恢复。

### 7.2 输入
```json
{
  "duration_ms": 10000,
  "reason": "wait for shell output",
  "resume_hint": "check shell session shsess_123"
}
```

### 7.3 输入字段定义
- `duration_ms?: integer`
  - 可选
  - 等待毫秒数
- `duration_secs?: integer`
  - 可选
  - 等待秒数
- `until?: string`
  - 可选
  - 绝对时间，RFC3339，例如 `2026-04-12T10:00:00Z`
- `reason?: string`
  - 可选
  - 说明为什么要等待
- `resume_hint?: string`
  - 可选
  - 给恢复后的 Agent 一个下一步提示

约束：
- `duration_ms`、`duration_secs`、`until` 至少提供一个
- 如果同时提供多个，建议优先按绝对 `until` 解析

### 7.4 输出
```json
{
  "accepted": true,
  "status": "scheduled",
  "wake_at": "2026-04-12T10:00:10Z",
  "task_id": "task_123",
  "turn_id": "turn_456"
}
```

### 7.5 使用策略
- 只有在明确存在“未来再看更合理”的情况时使用
- 不应用于掩盖错误或无限拖延
- 不应用于超短无意义等待

---
## 8. Runtime 语义
调用 `sleep` 后，建议 runtime 做以下事情：

1. 记录 `sleep_requested`
2. 生成 checkpoint / resume pack
3. 将 task 状态切到 `Waiting`
4. 写入 wake plan
5. 到达 `wake_at` 后重新进入 runtime
6. 继续后续 tool loop 或恢复 headless task

### 8.1 推荐状态变化
- 当前 `TaskStatus`：`Running -> Waiting`
- 恢复时：`Waiting -> Running`

### 8.2 推荐恢复输入
恢复时 runtime 可注入：
- 上次 `sleep` 的 `reason`
- `wake_at`
- `resume_hint`
- 当前关联 shell session / job / node 状态摘要

---
## 9. Event Model
建议至少记录：
- `sleep_requested`
- `task_waiting`
- `task_resumed`

如需更统一，也可以命名成：
- `runtime_wait_requested`
- `runtime_wake_scheduled`
- `runtime_resumed`

关键不是名字，而是必须可回放。

---
## 10. 与 Shell Tool 的关系
`sleep` 和 `shell.session.*` 是天然配套的。

推荐模式：
1. `shell.session.open`
2. `shell.session.exec`
3. `sleep`
4. `shell.session.read`
5. 视结果决定继续 `sleep`、继续 `shell.session.exec`，或结束任务

也就是说：
- shell 负责执行
- `sleep` 负责等待

不要让 shell 自己承担“恢复调度”语义。

---
## 11. 与 Turn / Task Runtime 的关系
### 11.1 普通聊天 turn
在普通用户交互 turn 里，`sleep` 应谨慎使用。

因为用户通常期待立即回复。

更合理的策略是：
- 对用户可见 turn，默认少用或禁用
- 对 headless task / autonomous workflow，允许使用

### 11.2 Headless task
`sleep` 最适合：
- `headless_task`
- scheduler 驱动的后台任务
- 多步自治 workflow

### 11.3 Tool Loop
真正的 `tool loop (0..n)` 里，`sleep` 可以成为一种合法分支：
- 不是每次都“立刻继续”
- 有时是“先等待，未来恢复”

这正是它的价值。

---
## 12. 安全与治理边界
### 12.1 必须有限制
至少建议限制：
- 最短等待时间
- 最长等待时间
- 单 task 最大 sleep 次数
- 最大同时 waiting task 数

### 12.2 防止死循环等待
需要治理：
- 连续短间隔 sleep
- 永远不检查结果的 sleep 循环
- 用 sleep 规避错误处理

### 12.3 可取消
进入 waiting 的 task 应可以：
- 取消
- 重试
- 提前唤醒

---
## 13. Agent 使用策略
建议模型遵守：

- 如果长任务还没出结果，优先考虑 `sleep`，不要忙等
- 如果只需要一次同步结果，不要滥用 `sleep`
- 使用 `sleep` 前最好明确下一次恢复要检查什么
- `sleep` 后恢复时，优先先看状态，再决定下一步

---
## 14. 第一阶段最小验收标准
- 存在 `sleep` tool
- `sleep` 不阻塞 worker 空转
- `sleep` 能让 task 进入 waiting
- 到达时间后 runtime 能恢复 task
- `sleep` 动作进入 event log
- `sleep` 能与 shell session 状态检查配合使用

---
## 15. 非目标
第一阶段不要求：
- 任意复杂条件等待
- 事件总线级 `await_event`
- 高精度定时器
- 秒级以下的强实时保证

第一阶段只需要把一件事做对：
**让 Agent 可以合法地“等一下再回来”。**
