# AgentJax LCM Context Engine Spec
## 1. 目标
本文档定义 AgentJax 的原生上下文引擎（LCM: Long Context Management / Context Continuity Engine）规范。
本规范要解决的核心问题不是“怎么做一点摘要”，而是：
**如何把长期 agent 连续性从 prompt 技巧变成确定性的系统层能力。**
这里先拍死一个边界：
- 工作区记忆 / RAG 只是检索便利层
- LCM Context Engine 才是无限对话连续性的地基
换句话说：
- Memory / RAG 回答的是：**你知道什么**
- Context Engine 回答的是：**你为什么还没失忆**
本文档与已有文档的关系：
- `docs/CORE_OBJECT_MODEL.md`：定义核心对象
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`：定义 workspace / config / state 布局
- `docs/PLUGIN_SDK.md`：定义插件与资源层
- `docs/EVENT_TASK_LCM_RUNTIME.md`：定义 event / task / LCM runtime 总体契约
- `docs/LCM_CONTEXT_ENGINE.md`：定义原生上下文引擎本体
---
## 2. Context Engine 的职责边界
### 2.1 Context Engine 负责什么
Context Engine 负责：
- 会话 / 任务事件持久化
- 活跃上下文装配
- token budget 管理
- 分层压缩
- summary DAG 维护
- 精确扩展 / 回溯
- 大文件 / 大产物外置引用
- 恢复 / 续跑
- 面向模型目的上下文选择
### 2.2 Context Engine 不负责什么
Context Engine 不负责：
- 跨会话长期知识抽取
- 模糊语义记忆
- 用户画像沉淀
- 普通知识库问答
这些属于：
- memory engine
- RAG engine
- profile store
### 2.3 一句话定义
**Context Engine 管“当前这条长生命线怎么不断”，Memory Engine 管“哪些东西值得跨生命线留下”。**
---
## 3. 核心思维：从 transcript 转向 event stream
### 3.1 不要以 message 为中心
原生输入单元不应只是 `message`，而应是 `event`。
消息只是事件的一种。
### 3.2 原生事件种类建议
Context Engine 至少支持以下 event kind：
- `user_message`
- `assistant_message`
- `tool_call`
- `tool_result`
- `task_state_change`
- `observation`
- `artifact_created`
- `schedule_triggered`
- `note`
- `system_event`
### 3.3 为什么这很重要
autonomous bot 的连续性主要不是靠聊天内容，而是靠：
- 做过什么
- 发现了什么
- 为什么停在这里
- 哪些外部状态已确认
- 哪些失败已经试过
所以被压缩和被恢复的对象，本质上是：
**执行历史**，而不是纯聊天记录。
---
## 4. Context Engine 的四层状态模型
建议将 Context Engine 固化为四层结构。
### 4.1 Immutable Event Log
这是**真相层**，必须 append-only。
存储内容：
- 原始事件
- 原始消息
- tool 输入 / 输出
- artifact 引用
- 时间戳
- causal links
原则：
- 它是唯一事实源
- summary 不能替代它
- projection 不能污染它
### 4.2 Active Context Projection
这是当前要喂给模型的“工作集”。
不是原始全量，而是：
- 最近 raw events
- summary nodes
- active goals / tasks
- runtime directives
- 必要 artifact refs
这是当前 prompt 的直接来源。
### 4.3 Summary DAG
这是压缩层，不是真相层。
建议支持以下节点类型：
- `LeafSummary`
- `CondensedSummary`
- `ArtifactRefSummary`
- `CheckpointSummary`
其中 `CheckpointSummary` 是必须新增的一类，用于表达恢复点，而不是普通叙事摘要。
每个 summary 至少应带：
- `summary_id`
- `conversation_id` / `task_id`
- `depth`
- `kind`
- `content`
- `source_refs`
- `earliest_at`
- `latest_at`
- `descendant_count`
- `token_count`
- `freshness`
- `confidence`
- `invalidated`
### 4.4 Recall / Expansion Plane
这是读取层，不应直接污染主上下文。
提供能力：
- grep / full-text
- summary describe
- DAG expansion
- artifact open
- focused expansion query
默认原则：
- expand 在隔离平面中完成
- 主上下文只接收 distilled result
- 不允许无控制地把全量历史重新塞回 prompt
---
## 5. 四层状态之间的关系
```text
Immutable Event Log
  -> Active Context Projection
  -> Summary DAG
  -> Recall / Expansion Plane
```
说明：
- Event Log 是事实源
- Projection 是工作集
- Summary DAG 是压缩与回溯骨架
- Expansion Plane 是只读扩展层
### 核心约束
- DAG 节点必须可回溯到 Event Log / Artifact refs
- Projection 不能独立于 Event Log 存在
- Expansion 默认隔离运行
---
## 6. 核心数据流
### 6.1 写入流
每次 turn / task step 的写入流建议为：
1. 收到事件
2. 事件规范化
3. 持久化到 immutable log
4. 必要时解析 artifact / file / tool structure
5. 加入 active context
6. 更新 token accounting
7. 判断是否需要 compaction
8. 如果需要，触发 leaf / condensed / checkpoint compaction
### 6.2 读取流
每次准备模型调用时：
1. 取 active context projection
2. 计算 fresh tail
3. 计算稳定区（rules / mission / router / task state）
4. 选择 summary nodes
5. 若预算不够，优先裁掉旧 raw，不裁 checkpoint
6. 渲染成 model-ready blocks
7. 注入压缩态提示
### 6.3 扩展流
当模型 / 执行器需要细节时：
1. 从 active context 中获取 summary / artifact refs
2. 调用 describe
3. 如仍需要细节，进入 isolated expansion
4. 展开叶子 / 父链 / 证据
5. 返回 focused answer 或 state patch
6. 主上下文只接收 distilled result，不吞全量
### 6.4 必须钉死的原则
**expand 默认隔离。**
否则 LCM 会在自我扩展中把自己打爆。
---
## 7. Compaction 三层策略
不要只做 leaf + condensed，还必须引入 checkpoint。
### 7.1 Leaf Compaction
目标：把 raw events 压成 depth=0 的叶子摘要。
对象：
- 一段连续 event span
- 排除 fresh tail
- 排除 pinned checkpoints
要求：
- 保留 chronology
- 保留关键观察
- 保留 tool/result 因果关系
### 7.2 Condensed Compaction
目标：把同深度 summary 往上折叠。
对象：
- 同 depth 的连续节点
作用：
- 从 session-level 走向 phase-level / arc-level / durable trajectory
### 7.3 Checkpoint Compaction
这是 Context Engine 必须比 lossless-claw 更强的地方。
它专门为恢复执行服务，而不是为概述历史服务。
Checkpoint 内容至少应包含：
- current goal
- active task graph
- latest successful step
- failed attempts worth remembering
- pending blockers
- important live refs / artifacts
- next recommended action
- assumptions / risks
### 7.4 Checkpoint 的角色
Checkpoint 不应被普通 condensed summary 替代。
它更接近：
- save point
- task continuation capsule
- resume anchor
一句话：
- summary 解决“你聊了啥”
- checkpoint 解决“你接下来该干啥”
---
## 8. Active Context 装配规则
最终进入模型的上下文建议由以下 block 组成。
### 8.1 Stable Blocks
- `AGENT`
- `SOUL`
- `MISSION`
- `RULES`
- `ROUTER`
- `USER`（按场景）
### 8.2 Runtime Blocks
- current task state
- current schedule trigger / operator request
- node / resource context
- budgets / mode flags
### 8.3 LCM Blocks
- active checkpoint
- selected summary nodes
- fresh raw tail
- critical artifact refs
### 8.4 Optional Retrieval Blocks
- memory recall
- RAG results
- skill instruction snippets
### 8.5 建议顺序
1. system doctrine
2. runtime / task state
3. checkpoint
4. compacted summaries
5. fresh tail
6. retrieval add-ons
### 8.6 核心原则
fresh tail 重要，但：
**任务恢复点比 fresh tail 更重要。**
---
## 9. Purpose-aware Assembly
同一个 session / task，在不同用途下不应装配同样的上下文。
### 9.1 建议 purpose 类型
- `chat`
- `planning`
- `execution`
- `summarization`
- `resume`
### 9.2 设计原则
- 给 chat 模型的上下文
- 给 planner 的上下文
- 给 executor 的上下文
应该是不同的
### 9.3 好处
这样可以避免：
- 所有上下文一锅炖
- token 浪费
- 恢复时缺乏 operational state
---
## 10. Context Engine 最低原生 API
建议 Context Engine 至少提供以下 API：
- `append_event()`
- `append_events()`
- `register_artifact()`
- `assemble_context()`
- `evaluate_compaction()`
- `compact_leaf()`
- `compact_condensed()`
- `compact_checkpoint()`
- `compact_until_under_budget()`
- `describe_object()`
- `grep_history()`
- `expand_summary()`
- `build_resume_pack()`
- `invalidate_summary()`
- `recompute_summary()`
- `reconcile_session()`
---
## 11. 关键 API 契约
### 11.1 `assemble_context()`
输入：
- `session_id` / `task_id`
- token budget
- model / provider profile
- prompt purpose（chat / planning / execution / summarization / resume）
输出：
- rendered context blocks
- token breakdown
- included refs
- omitted refs
- system prompt additions
### 11.2 `compact_until_under_budget()`
这个过程必须 deterministic，不能依赖“模型今天心情如何”。
建议三级收敛：
1. normal summary
2. aggressive summary
3. deterministic fallback
### 11.3 `build_resume_pack()`
这是 autonomous runtime 的关键 API。
输出不应只是聊天摘要，而应包含：
- current mission
- active tasks
- latest checkpoint
- necessary summaries
- open blockers
- pending artifacts
- last safe action boundary
---
## 12. Token Budget 与压缩策略
### 12.1 Token Accounting
Context Engine 必须原生维护 token accounting，而不是把预算问题留给 prompt 拼接层。
### 12.2 裁剪优先级建议
预算不足时：
1. 先裁掉旧 raw
2. 再裁低优先级 retrieval
3. 再做 aggressive summary
4. 最后 deterministic fallback
### 12.3 不可轻易裁掉的对象
- active checkpoint
- current task state
- mission / rules / router
- critical live artifact refs
---
## 13. Summary / Checkpoint 失效与修正
### 13.1 Summary Invalidation 原生化
一旦外部世界变化、旧结论过期：
- summary 可标记 stale
- checkpoint 可标记 invalid
- assemble 时自动降权或触发 refresh
### 13.2 触发场景示例
- 外部资源状态改变
- 关键任务步骤回滚
- artifact 被替换
- source events 出现矛盾
- mission / rules / router 发生重大变化
### 13.3 修正方式
- 标记 contradiction / stale
- 保留 source refs
- 发起 recompute
- 用新节点替代旧节点，但不抹掉真相层
---
## 14. Artifact-aware Compaction
Context Engine 不只是对大文件外置引用，凡是重产物都应外置并以 artifact ref 进入上下文。
### 14.1 应外置的典型对象
- logs
- reports
- code patches
- transcripts
- binary outputs
- ST / TTS 中间结果
### 14.2 原则
上下文里出现的是：
- artifact summary
- artifact ref
- selective distilled result
而不是原始整文件硬塞 prompt。
---
## 15. Resume-first 设计
### 15.1 不要把 resume 当附加功能
对 autonomous runtime 而言，resume 是主线能力。
### 15.2 Resume Pack 应包含
- current mission
- active task graph
- latest checkpoint
- selected summaries
- unresolved blockers
- important live artifacts
- last safe action boundary
### 15.3 场景
这用于：
- crash recovery
- cold restart continuation
- task handoff
- node failover continuation
---
## 16. 存储模型建议
如果采用 SQLite，前期完全够用，但 schema 必须按 DAG + event stream 设计，而不是按 chat message table 凑合。
### 16.1 canonical
- `conversations`
- `events`
- `event_parts`
- `artifacts`
### 16.2 context
- `context_items`
### 16.3 compaction
- `summaries`
- `summary_events`
- `summary_parents`
- `checkpoints`
### 16.4 bookkeeping
- `compaction_runs`
- `assembly_snapshots`
- `expansion_requests`
- `resume_packs`
---
## 17. 建议的内部模块拆分
Context Engine 建议拆成 6 个内部模块：
### 17.1 `event-store`
负责：
- 真相层持久化
- append-only events
- event normalization
### 17.2 `projection-store`
负责：
- active context materialized view
- raw tail / selected summaries / pinned refs
### 17.3 `compactor`
负责：
- leaf compaction
- condensed compaction
- checkpoint compaction
### 17.4 `assembler`
负责：
- budget-aware assembly
- purpose-aware rendering
- token accounting
### 17.5 `expander`
负责：
- grep
- describe
- expand
- artifact reopen
### 17.6 `resume-engine`
负责：
- build resume pack
- cold-start continuation pack
- task continuation capsule
---
## 18. 与 Memory / RAG 的关系
### 18.1 Context Continuity Memory
- 单会话 / 单任务连续性
- 由 LCM / Context Engine 管理
### 18.2 Durable Semantic Memory
- 跨会话稳定信息
- 由 memory / RAG / profile engine 管理
### 18.3 两者关系
- LCM 提供“不失忆”
- RAG 提供“会联想”
正确顺序是：
**先有前者，再谈后者。**
没有 LCM，再强的 RAG 也只是偶尔想起来点东西，不是连续性。
---
## 19. 比 lossless-claw 再往前走的 6 个点
1. 以 event 为中心，而不是 message 为中心
2. Checkpoint 作为原生节点
3. Purpose-aware assembly
4. Summary invalidation 原生化
5. Artifact-aware compaction
6. Resume-first design
---
## 20. 最小实现优先级
下一轮进入 Code 模式时，不建议一口气做完整上下文引擎，而应先把骨架和协议落地。
### P0
1. `EventKind` / `EventRecord`
2. `SummaryNode` 扩展出 `CheckpointSummary`
3. `ContextAssemblyPurpose`
4. `assemble_context()` 输入输出类型
5. `build_resume_pack()` 输出类型
### P1
6. SQLite schema 初版：`events` / `artifacts` / `summaries` / `checkpoints`
7. token accounting 模型
8. compaction evaluator 接口
### P2
9. leaf / condensed / checkpoint compactor trait
10. expander / grep / describe 接口
11. projection store 抽象
### 当前不建议立即做
- 全自动 LCM worker
- 复杂 DAG 图查询优化
- 全量 summary recompute pipeline
- 分布式 resume handoff
---
## 21. 总结
AgentJax 的真正灵魂不是 tool calling、不是插件 SDK、甚至不是 provider 抽象。
真正的灵魂是：
**把长期 agent 连续性从“prompt 技巧”升级为“确定性的系统层能力”。**
这就是 Context Engine 的使命。
一句话总结：
**LCM 负责让 agent 不失忆；RAG 负责让 agent 会联想。前者是地基，后者是外挂。**
