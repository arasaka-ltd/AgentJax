# MVP_PLAN（已废弃）

本文件已不再作为 AgentJax 的主开发入口。

## 状态
- 状态：**Deprecated / Archived**
- 原因：项目已从早期 MVP 规划阶段，进入 **规范先行的正式框架设计阶段**
- 后续开发不再以“Telegram Agent MVP”作为主线目标，而以 **核心契约、插件化 runtime、workspace/config/state 边界、LCM context engine** 为主线

## 不再作为主入口的原因
最初的 `MVP_PLAN.md` 适合用于：
- 快速做一个单 provider、单 agent、Telegram 文本链路的可运行原型

但当前 AgentJax 已经完成以下更高优先级的正式规范定义：
- Core Object Model
- Workspace / Config / State Layout
- Plugin SDK + Resource Model
- RAG / Knowledge / Memory
- Channels / Daemon / Client
- Daemon API / IPC Schema
- Event / Task / LCM Runtime
- Usage / Billing / Scheduler / Node
- LCM Context Engine

因此，继续以 MVP 文档驱动开发会造成两个问题：
1. 容易把当前项目拉回“功能堆叠式原型”思路
2. 会弱化已经明确的系统级边界与长期演化方向

## 当前正式开发入口
请改为以下顺序阅读与实现：
1. `docs/ARCHITECTURE_ENTRYPOINT.md`
2. `docs/CORE_OBJECT_MODEL.md`
3. `docs/WORKSPACE_AND_CONFIG_SPEC.md`
4. `docs/PLUGIN_SDK.md`
5. `docs/RAG_KNOWLEDGE_MEMORY_SPEC.md`
6. `docs/CHANNELS_DAEMON_CLIENT_SPEC.md`
7. `docs/DAEMON_API_IPC_SCHEMA.md`
8. `docs/EVENT_TASK_LCM_RUNTIME.md`
9. `docs/LCM_CONTEXT_ENGINE.md`
10. `docs/USAGE_BILLING_SCHEDULER_NODE_SPEC.md`

## 保留本文件的意义
本文件仅作为历史背景材料保留，用于说明项目最初的落地路径与演进起点，不再作为当前实现依据。
