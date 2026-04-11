# AgentJax 工作区、配置与运行时规范草案
## 1. 文档目标
本文档用于定义 AgentJax 下一阶段的三类核心规范：
- Agent Workspace 规范
- Runtime Config / State / Artifact 目录规范
- 模块级热重载与 Skill 规范
本规范的核心结论是：
**工作区是 agent 的自我与长期知识域，不是部署配置目录。**
也就是说，需要明确拆分：
- `workspace/`：agent identity + memory + skills + knowledge
- `config/`：runtime wiring + provider/resource/channel/plugin 配置
- `state/`：sessions/tasks/checkpoints/plugin state
- `artifacts/`：输出文件、媒体、产物
- `logs/`：日志与审计
- `cache/` / `tmp/`：缓存与临时数据
这套拆分是为了让 AgentJax 具备：
- 长期演化能力
- 模块级热重载能力
- 清晰的运行时边界
- 更强的可维护性与可移植性
---
## 2. 核心原则
### 2.1 工作区不是配置目录
工作区只负责承载 agent 的：
- identity
- personality
- mission
- stable memory
- skills
- knowledge
- prompt materials
工作区不负责承载：
- provider API 配置
- plugin enable/disable 配置
- Telegram token
- resource routing
- scheduler wiring
- node registry
- runtime secrets
### 2.2 配置与身份解耦
配置是 runtime wiring，工作区是 agent identity。
因此：
- 同一个 workspace 可以被多个 runtime profile 以不同资源配置启动
- 同一个 runtime 也可以挂载多个 workspace
- workspace 不因为 provider 或 channel 变化而失去稳定 identity
### 2.3 状态与知识解耦
长期知识不应与会话运行态混放。
因此：
- `MEMORY.md` 与 `memory/**` 属于工作区
- `sessions/`、`tasks/`、`checkpoints/` 属于状态目录
### 2.4 热重载应是模块级，而不是整机断电式
修改某个 provider、channel、tool policy、router policy 时，不应导致整个 agent runtime 中断。
### 2.5 Skill 需要通用交换格式
Skill 不应只是 Markdown 文件，也不应完全锁死在私有格式。
建议采用：
- `SKILL.md` 作为通用自然语言降级层
- `skill.toml` 作为结构化强能力层
---
## 3. 工作区是什么
### 3.1 定义
**工作区 = Agent 的自我与长期知识域。**
工作区表达的是 agent 的“是谁、为什么存在、如何行动、长期记住什么、拥有哪些技能和知识”。
它不是：
- 部署配置目录
- 密钥目录
- 运行时状态目录
- 日志目录
- provider 接线目录
### 3.2 工作区顶层规范
建议顶层稳定保留以下文件：
- `AGENT.md`
- `SOUL.md`
- `USER.md`
- `MEMORY.md`
- `MISSION.md`
- `RULES.md`
- `ROUTER.md`
### 3.3 顶层文件职责
#### `AGENT.md`
描述 agent 的运行方式、工作习惯、启动流程、行为节奏。
示例职责：
- 如何拆解任务
- 默认如何与用户协作
- 如何在多步任务中保持状态
- 如何在使用 tools 前后组织动作
#### `SOUL.md`
描述人格、语气、风格、价值倾向、表达内核。
示例职责：
- 说话风格
- 倾向理性还是共情
- 如何处理冲突与不确定性
- 个性化表达习惯
#### `USER.md`
描述服务对象画像。
示例职责：
- 用户偏好
- 用户常见目标
- 用户工作方式
- 沟通习惯与禁忌
#### `MEMORY.md`
描述 curated long-term memory index / distilled memory。
它应该是经过沉淀与整理的长期稳定记忆索引，而不是原始碎片垃圾堆。
#### `MISSION.md`
描述长期目标、存在意义、自治范围。
示例职责：
- agent 的长期使命
- 主动性边界
- 可自主执行的任务类别
- 何时需要请求确认
#### `RULES.md`
描述硬规则、不可踩线、风险控制策略。
示例职责：
- 安全边界
- 合规约束
- tool 使用禁忌
- 数据处理规则
- escalation policy
#### `ROUTER.md`
描述路由与决策策略。
示例职责：
- 何时使用 memory
- 何时触发 LCM
- 何时调用 tools
- 何时启用 skills
- 何时切换 provider
- 何时将任务下发到 node
---
## 4. 工作区目录结构规范
推荐结构：
```text
workspace/
  AGENT.md
  SOUL.md
  USER.md
  MEMORY.md
  MISSION.md
  RULES.md
  ROUTER.md
  skills/
  memory/
    daily/
    topics/
    profiles/
    scratch/
  knowledge/
  prompts/
```
### 4.1 `skills/`
存放 skill 包或 skill 定义。
### 4.2 `memory/`
用于承载长期语义记忆域，而不是任意检索库。
推荐承载：
- `daily/`：候选性日常沉淀，等待提纯
- `topics/`：主题性长期记忆
- `profiles/`：人物、对象、组织画像
- `scratch/`：临时草稿、待筛选内容
### 4.3 `knowledge/`
存放通用知识域或领域知识库，例如项目资料、产品文档、API 文档、笔记库、代码知识块。
推荐按 Agent 可理解的 `library` 分层组织，例如：
```text
knowledge/
  rust/
    book/
    reference/
  blender/
    manual/
    api/
  project-docs/
    architecture/
    runbooks/
```

这里的 `library` 是 Agent-facing 的知识库命名层。
它不必与底层 RAG collection 一一对应，但应保持稳定、可理解、可在 tool 参数中直接引用。
### 4.4 `prompts/`
存放可复用 prompt blocks、模板、辅助说明材料。
---
## 5. Memory 分层规范
### 5.1 顶层 `MEMORY.md`
`MEMORY.md` 应是：
- curated
- distilled
- stable
- compact
它是长期记忆索引和高价值摘要，不应成为无限膨胀的碎片堆。
### 5.2 `memory/**` 的职责
`memory/**` 是 `Memory System` 在工作区内的主要落点。
它承载的是：
- 值得长期保留并影响未来行为的内容
- 经过筛选、提纯、去重、冲突检测后的稳定语义记忆

它不等于通用检索库，也不应承担所有知识问题。

细节性、可演化、待归档内容进入 `memory/` 子目录。
建议职责：
- `memory/daily/`：按日期或时间周期记录
- `memory/topics/`：按主题组织的知识与记忆
- `memory/profiles/`：用户、团队、角色、组织、系统画像
- `memory/scratch/`：短期草稿、等待 LCM 压缩的临时内容
### 5.3 与 LCM / Recall 的关系
这套结构天然适合后续实现：
- memory recall
- memory distillation
- promotion / conflict / freshness policy
- topic / profile-based retrieval

但要明确：
- `LCM` 负责单会话 / 单任务连续性
- `RAG` 负责通用 retrieval substrate
- `Memory` 负责基于 `RAG` 的长期语义层
### 5.4 推荐原则
- `MEMORY.md` 只保留高价值稳定内容
- 原始堆积不要直接塞顶层
- LCM 可以把 `scratch/` 与 `daily/` 的内容提炼进 `MEMORY.md` 或 `topics/`
- `knowledge/` 可以大量 ingest，`memory/` 必须更贵地写入
### 5.5 与 `knowledge/` 的区别
- `knowledge/` 面向领域知识库，可大量导入、增量更新、通用检索
- `memory/` 面向 durable agent knowledge，只收会影响未来行为的稳定内容
- `knowledge/` 的召回目标是“找到相关证据”
- `memory/` 的召回目标是“拿回长期有效的认知约束”

### 5.6 与 Retrieval Tool Surface 的对应关系
为了让 Agent 明确区分“查长期记忆”和“查领域知识”，建议 retrieval tool surface 直接映射这两个域：
- `memory.search`
- `memory.get`
- `knowledge.search`
- `knowledge.get`

其中：
- `memory.search/get` 主要面向 `MEMORY.md` 与 `memory/**`
- `knowledge.search/get` 主要面向 `knowledge/<library>/**`

这层映射应稳定存在，避免让 Agent 直接暴露到底层 `RAG` collection 细节。
---
## 6. Runtime 配置目录规范
### 6.1 配置目录不在工作区内
建议使用独立 config root，例如：
```text
~/.agentjax/
  config/
    core.toml
    plugins.toml
    providers.toml
    models.toml
    resources.toml
    channels.toml
    surfaces.toml
    daemon.toml
    nodes.toml
    scheduler.toml
    skills.toml
  agents/
    <agent-id>/
      workspace/
```
服务化部署下也可采用：
- `/etc/agentjax/`
- `/var/lib/agentjax/`
- `/var/log/agentjax/`
具体路径取决于最终是：
- 单用户本地 agent
- 自托管服务
- 多租户服务化平台
### 6.2 配置文件职责
#### `core.toml`
核心 runtime 配置：
- app env
- API / ABI 版本
- 默认 workspace 解析
- state / log / artifact / cache 根目录
- 热重载策略
#### `plugins.toml`
插件启用策略：
- enabled plugins
- disabled plugins
- plugin-specific config refs
- plugin policy flags
#### `providers.toml`
provider 实例与凭据引用：
- OpenAI / Gemini / Anthropic / local provider endpoints
- secret refs
- timeout / retry / quota policy
#### `models.toml`
模型路由与默认模型：
- text model defaults
- reasoning model defaults
- embedding/reranker defaults
- provider fallback order
#### `resources.toml`
资源层映射：
- `model:text`
- `model:embedding`
- `audio:tts`
- `audio:st`
- `exec:shell`
- `store:memory`
- `store:artifact`
- `channel:telegram`
#### `channels.toml`
渠道配置：
- Telegram bot tokens
- webhook / polling mode
- Discord / Email 等渠道实例
- 这里只配置外部 message channels，不配置 TUI / WebUI / WebSocket surface
#### `surfaces.toml`
core surfaces 配置：
- TUI defaults
- WebUI session policy
- local operator UX policy
#### `daemon.toml`
daemon / transport 配置：
- unix socket path
- websocket bind address
- local auth / token policy
- pid / lock / run directory policy
#### `nodes.toml`
node / worker / browser / remote machine registry。
#### `scheduler.toml`
调度器、cron jobs、automations、recurring jobs。
#### `skills.toml`
skill registry policy：
- 启用哪些 skill 源
- skill compatibility version
- trust policy
- install sources
---
## 7. Runtime State / Artifact / Log 规范
### 7.1 State 目录
建议：
```text
state/
  sessions/
  tasks/
  lcm/
  checkpoints/
  leases/
  plugin-state/
```
#### 含义
- `sessions/`：对话会话状态
- `tasks/`：任务执行状态
- `lcm/`：long context management 中间态
- `checkpoints/`：恢复点
- `leases/`：锁、租约、分布式占用信息
- `plugin-state/`：插件私有持久状态
### 7.2 Artifacts 目录
建议：
```text
artifacts/
```
用于：
- 生成的文件
- 导出内容
- 音频
- 图像
- 中间工作产物
### 7.3 Logs 目录
建议：
```text
logs/
```
用于：
- runtime logs
- plugin logs
- audit trails
- diagnostic dumps
### 7.4 Cache / Tmp 目录
建议：
```text
cache/
tmp/
```
分别用于：
- 可丢弃缓存
- 短期临时文件
---
## 8. 启动注入规范
### 8.1 Stable Bootstrap
默认稳定注入建议为：
- `AGENT.md`
- `SOUL.md`
- `MISSION.md`
- `RULES.md`
- `USER.md`
- `ROUTER.md`
这些文件定义的是 agent 的稳定自我与长期行为边界，应优先进入 bootstrap context。
### 8.2 On-Demand Injection
按需注入建议为：
- `MEMORY.md`
- `memory/**`
- `skills/**`
- `knowledge/**`
- `prompts/**`
### 8.3 原因
这样做可以：
- 避免初始 prompt 过度膨胀
- 把“自我”与“知识”分层
- 让 router/context engine 更灵活地决定加载范围
---
## 9. 模块级热重载规范
### 9.1 目标
热重载必须做到：
- 尽量不打断正在进行的会话
- 不因小配置变更重启整个 runtime
- 支持模块级 diff、替换、drain、切换
### 9.2 可热重载对象
建议支持热重载：
- provider routing
- model defaults
- TTS/ST resources
- channel configs
- plugin enable/disable
- scheduler definitions
- node registry
- tool policies
- router policies
### 9.3 不应轻易热重载的对象
以下内容变更应拒绝热重载，并提示 staged restart：
- core ABI / plugin API major version
- state schema version
- event bus contract
- storage backend migration
### 9.4 原则
能热重载的只重载模块；不能热重载的必须显式阻止，而不是偷偷让系统进入半损坏状态。
---
## 10. 热重载运行时设计
建议采用：
- runtime supervisor
- module registry
- module instance lifecycle
### 10.1 高层结构
```text
Runtime Supervisor
  -> Core Runtime (always-on)
  -> Module Registry
      -> Provider Modules
      -> Plugin Modules
      -> Channel Modules
      -> Node Modules
      -> Scheduler Modules
```
### 10.2 重载流程
配置变化后：
1. diff config
2. 识别受影响模块
3. 调用 `prepare_reload()`
4. 创建新实例
5. 做健康检查
6. 切换引用
7. 老实例 drain / shutdown
这种方式接近：
- RCU
- blue-green module swap
### 10.3 例子
#### 修改 `tts.default_provider`
- 不重启整个 agent
- 只更新 audio broker 的 provider binding
#### 修改某个 Telegram token
- 只重连对应 channel adapter
- 不影响 memory、LLM、task runtime
#### 修改某个 plugin config
- 只 reload 对应 plugin 实例
- 相关 hooks 重新注册
- 已在运行中的 task 根据策略决定：
  - 继续用旧实例直到当前 step 完成
  - 或 checkpoint 后切换到新实例
---
## 11. Supervisor / Module Reload 生命周期建议
建议模块实例支持以下生命周期接口：
```rust
#[async_trait::async_trait]
pub trait ReloadableModule {
    async fn prepare_reload(&self) -> anyhow::Result<ReloadPlan>;
    async fn health_check(&self) -> anyhow::Result<()>;
    async fn drain(&self) -> anyhow::Result<()>;
    async fn shutdown(&self) -> anyhow::Result<()>;
}
```
### 11.1 `prepare_reload()`
用于：
- 校验新配置
- 预创建依赖连接
- 准备迁移上下文
### 11.2 `health_check()`
用于确认新实例可接管。
### 11.3 `drain()`
用于：
- 停止接收新流量
- 等待当前消息/任务阶段结束
### 11.4 `shutdown()`
执行最终关闭。
---
## 12. Skill 规范
### 12.1 基本原则
Skill 不能只有 Markdown，也不应完全使用私有不可交换格式。
建议采用双层结构：
- `SKILL.md`：通用降级层
- `skill.toml`：结构化强能力层
### 12.2 Skill 包结构
建议最小 skill 包：
```text
my-skill/
  skill.toml
  SKILL.md
```
### 12.3 `skill.toml` 建议字段
- `id`
- `name`
- `version`
- `description`
- `triggers`
- `resources_required`
- `tools_required`
- `optional_capabilities`
- `instruction_entrypoint`
- `compatibility_version`
### 12.4 `SKILL.md` 职责
用于承载：
- 给 agent 的自然语言说明
- 低保真跨框架可交换语义
这意味着：
- 别的框架最少能读 `SKILL.md`
- AgentJax runtime 能读取更强的 manifest
- 后续可支持导入导出、版本校验、lint、compatibility check
### 12.5 Skill 标准策略
建议采用：
- 定义 `Skill Core Spec`
- 保证 `SKILL.md` 为通用说明入口
- 保证 `skill.toml` 为结构化补充层
不建议：
- 只有 Markdown
- 或只有完全私有 JSON 格式
---
## 13. 推荐的总目录布局
### 13.1 Agent Workspace
```text
workspace/
  AGENT.md
  SOUL.md
  USER.md
  MEMORY.md
  MISSION.md
  RULES.md
  ROUTER.md
  skills/
  memory/
    daily/
    topics/
    profiles/
    scratch/
  knowledge/
  prompts/
```
### 13.2 Runtime Config
```text
config/
  core.toml
  plugins.toml
  providers.toml
  models.toml
  resources.toml
  channels.toml
  nodes.toml
  scheduler.toml
  skills.toml
```
### 13.3 Runtime State
```text
state/
  sessions/
  tasks/
  lcm/
  checkpoints/
  leases/
  plugin-state/
```
### 13.4 Runtime Data
```text
artifacts/
logs/
cache/
tmp/
```
---
## 14. 与前序文档关系
本文档补充并细化以下已有文档：
- `docs/MVP_PLAN.md`
- `docs/PLUGIN_SDK.md`
建议三份文档共同承担的职责如下：
- `docs/MVP_PLAN.md`
  - 项目阶段目标、开发路线、模块拆分
- `docs/PLUGIN_SDK.md`
  - 插件能力模型、资源层、运行时模型
- `docs/WORKSPACE_AND_CONFIG_SPEC.md`
  - workspace 边界、配置根目录、状态目录、热重载、skill 规范
---
## 15. 下一轮 Code 模式建议实现顺序
下一轮进入 Code 模式时，不建议一口气把整套规范全部落地，而应先实现最小骨架。
### P0：目录与配置模型
1. 引入 `config root` 概念
2. 将当前单体 `AppConfig` 拆出：
   - runtime config root
   - workspace root
   - state root
   - artifacts root
   - logs root
3. 在 Rust 中建模 `WorkspacePaths` 与 `RuntimePaths`
### P1：Workspace 规范落地
4. 建立 workspace loader
5. 支持读取：
   - `AGENT.md`
   - `SOUL.md`
   - `MISSION.md`
   - `RULES.md`
   - `USER.md`
   - `ROUTER.md`
6. 定义 stable bootstrap 与 on-demand injection 规则
### P2：配置与热重载骨架
7. 设计 config loader + config diff
8. 设计 module registry 最小接口
9. 预留 `prepare_reload` / `drain` / `shutdown` 生命周期接口
### P3：Skill 包最小规范
10. 定义 `skill.toml` + `SKILL.md` 解析结构
11. 建立 `skills/` loader 最小版
### 当前不建议立即实现
- 完整 blue-green reload
- 多 workspace 调度
- UI dashboard
- 分布式 node 管理
- 完整 LCM pipeline
---
## 16. 验收标准
下一阶段最小验收标准建议为：
1. 项目中存在 workspace / runtime config / state / artifact 的路径模型
2. 项目可从独立 workspace 目录加载顶层 identity 文件
3. `AppConfig` 不再把工作区和运行时配置混为一谈
4. skill 包最小结构被定义
5. 热重载生命周期接口有初版抽象
6. 项目仍保持可 `cargo check`
---
## 17. 总结
当前收敛后的正确方向是：
- 工作区只放 agent 自我和知识
- 配置单独目录
- 状态与知识分离
- 模块级热重载
- skill 遵守通用交换规范
- memory 做分层
这比把所有东西塞进一个工作目录强得多，也更符合一个长期可演化 autonomous runtime 的形态。
一句话总结：
**Workspace 是 agent 的人格与长期知识体，Config 是 runtime wiring，State 是运行态，Artifacts 是产物，热重载则应以模块为单位而不是整机重启。**
