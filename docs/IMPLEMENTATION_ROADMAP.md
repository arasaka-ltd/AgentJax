# AgentJax Implementation Roadmap

## 1. 目的
本文档定义 AgentJax 接下来几轮开发的顺序、批次边界和阶段目标。

它不替代正式规范。
它的作用是把规范收束成一条可执行线路，避免下一轮对话又回到“同时铺十条线”的状态。

配合关系：
- `docs/ARCHITECTURE_ENTRYPOINT.md`：总入口
- `TASK.md`：可勾选执行清单
- 本文档：为什么按这个顺序做，以及每一批做完算什么状态

---
## 2. 总体原则
- 先把最短可用链路做实，再扩展外围子系统
- 先做 runtime 核心闭环，再做渠道和 UI 花活
- 先做 keyword / file-based / SQLite 级别的最小实现，再谈高级检索与复杂调度
- 每一批都必须产出“真实能力”，不是只多几份 struct

一句话：
**先让 Agent 真能加载自我、组上下文、调用工具、记住会话；再谈更大系统。**

---
## 3. 当前基线
当前代码库已经具备：
- 单 crate Rust 工程
- 最小 daemon
- Unix socket transport
- WebSocket transport skeleton
- CLI / TUI 最小壳
- OpenAI 文本 prompt 基础调用
- `session.send` 的最小聊天闭环
- daemon API envelope 与部分 method schema

当前代码库仍缺：
- workspace 人格文件真实加载
- context assembly 真实现
- tool calling loop
- 持久化 session/event store
- memory / RAG 检索闭环
- task runtime 真执行流

---
## 4. 分批线路
### Batch 1：Workspace Persona + Context v0
目标：
- agent 不再是裸 prompt
- workspace 文件能真实影响输出

做完的判定标准：
- `session.send` 已走 `assemble_context()`
- workspace 稳定文件可真实读取
- agent 输出可观察到 `AGENT.md / SOUL.md / RULES.md / MEMORY.md` 的影响

为什么先做这批：
- 这是最短路径里“从聊天壳变成有自我 Agent”的关键一步
- 不做这步，后面的 tool / memory / task 都没有稳定上下文底座

### Batch 2：Tool Calling v0
目标：
- agent 能调用本地工具，而不是只能说话

做完的判定标准：
- 至少 `read_file` / `list_files` / `shell` 可被统一 dispatch
- 一次用户请求可触发工具调用并回写结果
- tool trace 进入 event/session

为什么第二批做：
- 这是从“可聊天”变成“可执行”的关键分水岭

### Batch 3：Session / Event Persistence
目标：
- daemon 重启后不丢 session 和事件

做完的判定标准：
- session/messages/events 已经不是纯内存
- `session.list` / `session.get` / `session.send` 走统一存储抽象

为什么不是第一批：
- 先做 persona/context/tool，能更快验证 runtime 方向对不对
- 但持久化必须尽快跟上，否则后面所有 runtime 能力都不稳

### Batch 4：Memory / RAG v0
目标：
- agent 能查回长期记忆和知识，而不只是靠当前会话

做完的判定标准：
- `MEMORY.md` / `memory/topics/` 可被 recall
- `knowledge/` 可做最小 keyword/full-text retrieval
- retrieval 结果可接入 context assembly

为什么放在第四批：
- 没有 persona/context/tool/persistence，memory/RAG 接进来也只是漂浮能力

### Batch 5：Daemon API Completion
目标：
- daemon 不只支持聊天，还能支持真正的控制面和诊断面

做完的判定标准：
- `runtime/config/plugin/task/node/schedule/diagnostics` 方法基本可用
- subscription / stream cancel 不再是 schema-only

### Batch 6：Task Runtime v0
目标：
- turn 和 task 不再是只有类型，没有运行意义

做完的判定标准：
- task store 存在
- task inspect/cancel/retry 可用
- 至少有最小 timeline / checkpoint 落点

### Batch 7：Cleanup + Hardening
目标：
- 减少骨架漂移和技术债
- 让项目进入持续可推进状态

做完的判定标准：
- docs 与代码主要边界一致
- 最关键链路有回归测试
- 可以清楚告诉下一个会话“什么已经稳了”

---
## 5. 批次约束
每一批都应遵守：
- 批内任务可并行思考，但提交顺序必须保证主链路先通
- 没完成上一批，不进入下一批
- 新增 schema 必须尽量立刻接线，不要继续累计未实现类型
- 每一批结束必须运行：
  - `cargo fmt`
  - `cargo check`

---
## 6. 下一轮对话执行方式
下一轮开始后，先做三件事：
1. 读取 `docs/ARCHITECTURE_ENTRYPOINT.md`
2. 读取 `docs/IMPLEMENTATION_ROADMAP.md`
3. 读取 `TASK.md`

然后：
- 从 `TASK.md` 最上面的未完成项开始
- 完成一项就勾掉一项
- 不重新发明新的优先级排序，除非发现 blocker
- 如果遇到 blocker，先最小化解决 blocker，再继续原批次

---
## 7. 硬结论
接下来不再以“再补几份 spec”为主，而以：
- `TASK.md` 驱动执行
- `IMPLEMENTATION_ROADMAP.md` 保证顺序
- `ARCHITECTURE_ENTRYPOINT.md` 保证边界

三者配合，逐批完成整个 runtime。
