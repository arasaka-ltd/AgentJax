# AgentJax Config Manager Spec

## 1. 目标

本文档定义 AgentJax 中 `Config Manager` 的正式职责、对象边界与运行时契约。

它要解决的问题不是“配置文件放在哪”，而是：

- 如何从硬编码配置演进为结构化配置系统
- 如何自动生成初始化配置
- 如何加载、合并、校验、归一化配置
- 如何处理默认值、密钥引用、版本迁移与热重载
- 如何把配置变化安全地传播到 runtime

本文档是对 `docs/WORKSPACE_AND_CONFIG_SPEC.md` 的补充，不替代其目录边界定义。

---

## 2. 定位

### 2.1 Config Manager 是什么

`Config Manager` 是 runtime 内部的配置控制平面组件，负责：

- 解析 config root
- 管理配置源与优先级
- 生成初始化配置骨架
- 校验配置合法性
- 计算模块级配置 diff
- 向 runtime 提供只读、已归一化的配置快照
- 触发模块级热重载或 staged restart 判定

### 2.2 Config Manager 不是什么

它不是：

- workspace 管理器
- state 存储层
- 密钥明文存储器
- UI 配置页面本身
- 插件实现细节容器

### 2.3 与现有规范的关系

- `workspace/` 仍然表达 agent identity 与长期知识域
- `config/` 仍然表达 runtime wiring
- `state/` 仍然表达运行态
- `Config Manager` 是 `config/` 的加载、校验、生成、发布与变更协调层

---

## 3. 设计原则

### 3.1 配置先结构化，再允许默认值

默认值只能补全明确缺省项，不能替代结构化 schema。

### 3.2 运行时只消费归一化快照

各模块不应自行读取散落文件，更不应在模块内部重复解析 TOML/JSON/YAML。

### 3.3 初始化生成必须幂等

初始化逻辑应能安全重复执行：

- 不覆盖已有人工修改
- 不破坏已有密钥引用
- 能补齐缺失文件

### 3.4 密钥与业务配置分离

配置文件只保存 secret reference 或 secret locator，不鼓励直接存储明文 secret。

### 3.5 变更必须可判定

任意配置变更都应能明确落入以下三类之一：

- 无需动作
- 模块级热重载
- staged restart

### 3.6 错误必须尽早失败

发现配置冲突、非法引用、版本不兼容时，应在 bootstrap 或 reload 准备阶段失败，而不是运行中隐式降级。

---

## 4. 配置管理范围

`Config Manager` 应覆盖以下正式配置域：

- `core.toml`
- `plugins.toml`
- `providers.toml`
- `models.toml`
- `resources.toml`
- `channels.toml`
- `surfaces.toml`
- `daemon.toml`
- `nodes.toml`
- `scheduler.toml`
- `skills.toml`

它也应支持后续扩展：

- `billing.toml`
- `usage.toml`
- `policies.toml`
- 插件专属 config fragment

---

## 5. 顶层职责模型

建议将 `Config Manager` 拆为以下子职责：

### 5.1 `ConfigInitializer`

负责：

- 检测 config root 是否存在
- 生成初始化目录和最小配置骨架
- 写入示例模板、注释、版本字段
- 生成 agent workspace 引用示例

### 5.2 `ConfigLoader`

负责：

- 读取配置源
- 解析文件内容
- 合并多源输入
- 输出 typed config model

### 5.3 `ConfigValidator`

负责：

- schema 校验
- 语义校验
- 引用完整性校验
- secret ref 格式校验
- 路径合法性校验

### 5.4 `ConfigNormalizer`

负责：

- 填充默认值
- 归一化路径
- 生成派生字段
- 形成模块可消费的只读快照

### 5.5 `ConfigMigrator`

负责：

- 识别旧 schema version
- 执行安全迁移
- 输出迁移计划与风险提示
- 在必要时要求 staged restart 或人工确认

### 5.6 `ConfigReloader`

负责：

- 比较旧快照与新快照
- 产出受影响模块集合
- 判定 reload / restart 级别
- 协调 supervisor 触发模块切换

---

## 6. 配置源模型

### 6.1 配置源类型

建议支持以下配置源：

- 文件配置源：`config/*.toml`
- 环境变量配置源：少量高敏感或部署覆盖项
- 启动参数覆盖：进程级临时覆盖
- secret provider：密钥引用解析

### 6.2 推荐优先级

建议优先级从低到高为：

1. 初始化模板默认值
2. 配置文件显式值
3. 环境变量覆盖
4. 启动参数覆盖

密钥解析不属于覆盖层，而属于引用解引用过程。

### 6.3 不建议的做法

不建议：

- 同一字段同时允许过多来源
- 在代码内部写死 fallback endpoint / token / provider 名称
- 让插件私自定义无版本、无 schema 的散乱配置文件

---

## 7. 配置目录与初始化产物

### 7.1 最小初始化结果

首次执行初始化后，建议生成：

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
    default/
      workspace/
```

### 7.2 初始化策略

初始化应支持以下模式：

- `minimal`：仅生成可启动最小骨架
- `local-dev`：生成本地开发默认骨架
- `service`：生成服务化部署骨架
- `dry-run`：只输出将生成的文件与内容摘要

### 7.3 初始化生成原则

- 已存在文件默认不覆盖
- 缺失文件自动补齐
- 对危险覆盖必须显式拒绝或要求 `force`
- 生成内容必须包含 `schema_version`
- 示例值必须可读，但不能伪装成可直接上线的生产配置

### 7.4 初始化最小内容要求

至少应包含：

- `core.toml` 中的根路径与默认 workspace 解析
- `providers.toml` 中的 provider 实例样板
- `models.toml` 中的默认模型占位
- `daemon.toml` 中的本地 socket / bind 样板
- `plugins.toml` 与 `skills.toml` 中的空 registry 或最小启用策略

---

## 8. 配置对象模型

### 8.1 顶层统一约定

每个配置文件建议都具备：

- `schema_version`
- `generated_by`
- `generated_at`
- `updated_at`
- `profile`

其中：

- `schema_version` 用于迁移与兼容判定
- `generated_by` 用于标记是否来自初始化器
- `profile` 用于表达本地开发、测试、服务部署等配置轮廓

### 8.2 归一化后的统一快照

建议 runtime 内部只消费如下抽象：

```rust
pub struct RuntimeConfigSnapshot {
    pub schema_version: String,
    pub core: CoreConfig,
    pub plugins: PluginsConfig,
    pub providers: ProvidersConfig,
    pub models: ModelsConfig,
    pub resources: ResourcesConfig,
    pub channels: ChannelsConfig,
    pub surfaces: SurfacesConfig,
    pub daemon: DaemonConfig,
    pub nodes: NodesConfig,
    pub scheduler: SchedulerConfig,
    pub skills: SkillsConfig,
}
```

### 8.3 快照要求

- 不暴露原始未校验文本
- 不包含明文 secret
- 可计算 hash / fingerprint
- 可用于 reload diff

---

## 9. 校验层次

### 9.1 语法校验

例如：

- TOML 可解析
- 字段类型正确
- 必填字段存在

### 9.2 结构校验

例如：

- provider 定义结构完整
- model routing 节点格式合法
- scheduler 条目满足最低字段要求

### 9.3 语义校验

例如：

- `models.toml` 引用的 provider 实例必须存在
- `resources.toml` 的 resource binding 必须指向合法 capability
- `channels.toml` 的 channel ID 不得重复
- `core.toml` 的路径不能相互冲突

### 9.4 跨文件引用校验

例如：

- `plugins.toml` 引用的 plugin config fragment 必须存在
- `skills.toml` 中的 skill source 必须满足 trust policy
- `scheduler.toml` 引用的 target agent 或 workspace 必须可解析

### 9.5 启动前健康校验

例如：

- provider endpoint 格式正确
- unix socket 路径可创建
- state/log/artifact 根目录具备权限

启动前健康校验不等于真正连通性检查；连通性检查由模块初始化决定。

---

## 10. 默认值策略

### 10.1 默认值来源

默认值只允许来自：

- schema 内置默认值
- profile 模板默认值
- 经过显式声明的 runtime fallback

### 10.2 默认值边界

不应默认推断：

- 生产 provider endpoint
- 外部平台 token
- 高风险执行权限
- 计费相关阈值

### 10.3 可观测性要求

任何默认值补全都应可追踪，至少能在日志或诊断输出中看到：

- 哪个字段被补全
- 值来自哪个默认层
- 是否影响热重载判定

---

## 11. 密钥引用模型

### 11.1 原则

配置文件中涉及凭据时，建议写入引用，而非明文。

### 11.2 建议形式

例如：

- `env:OPENAI_API_KEY`
- `file:/path/to/secret`
- `keyring:agentjax/openai/main`
- `vault:kv/agentjax/openai/main`

### 11.3 运行时要求

- 加载后仅在需要的模块范围内解引用
- 不将明文 secret 写入归一化快照落盘
- 日志、诊断、事件中必须脱敏

### 11.4 错误处理

secret ref 不存在时：

- bootstrap 阶段应明确失败
- reload 阶段应拒绝切换到新实例

---

## 12. 迁移与版本策略

### 12.1 schema version 必须显式

每份正式配置文件都应带 `schema_version`。

### 12.2 迁移类型

建议区分：

- patch migration：自动修正小格式变动
- minor migration：自动迁移并写回建议
- major migration：拒绝隐式迁移，要求人工确认

### 12.3 迁移输出

迁移器至少应输出：

- 发现的旧版本
- 拟执行的变更摘要
- 是否幂等
- 是否影响热重载
- 是否需要 staged restart

### 12.4 禁止隐式破坏

不允许在未提示用户的情况下：

- 删除未知字段
- 覆盖手工配置
- 把旧配置 silently 改写为半兼容状态

---

## 13. 热重载契约

### 13.1 变更分类

配置变更建议分类为：

- `NoOp`
- `HotReloadSafe`
- `DrainAndSwap`
- `RestartRequired`

### 13.2 推荐判定示例

- 修改 `models.toml` 默认文本模型：`HotReloadSafe`
- 修改单个 channel token：`DrainAndSwap`
- 修改 plugin enable/disable：通常为 `DrainAndSwap`
- 修改核心 ABI 版本：`RestartRequired`
- 修改 state schema：`RestartRequired`

### 13.3 reload 工作流

1. 读取并解析新配置
2. 完成校验与归一化
3. 计算新旧快照 diff
4. 识别受影响模块
5. 生成 reload plan
6. 调用 supervisor 执行 prepare / health check / swap / drain

### 13.4 一致性要求

同一轮 reload 中，所有模块必须看到同一个 config snapshot version，不允许部分模块消费旧快照、部分模块消费新快照而没有版本边界。

---

## 14. 与 Runtime 的交互边界

### 14.1 Runtime 只依赖快照

业务模块应依赖 `RuntimeConfigSnapshot` 或模块子快照，不应直接依赖文件路径。

### 14.2 模块级子快照

例如：

- provider runtime 只消费 `ProvidersConfig` + `ModelsConfig`
- channel runtime 只消费 `ChannelsConfig`
- scheduler runtime 只消费 `SchedulerConfig`

### 14.3 Config Manager 与 Supervisor 分工

- `Config Manager` 负责发现和解释配置变化
- `Supervisor` 负责执行实例生命周期切换

二者不能混成一个“既读文件又直接重建所有模块”的黑箱。

---

## 15. 与 Workspace 的边界

### 15.1 workspace 不承载 runtime wiring

即使初始化器帮助生成默认 workspace，也不意味着 workspace 与 runtime config 混在一起。

### 15.2 允许从配置引用 workspace

配置可以声明：

- 默认 workspace root
- agent 到 workspace 的解析规则
- 多 workspace 挂载策略

但不应把 provider token、daemon bind、plugin enable 策略写回 workspace。

---

## 16. 初始化命令的建议语义

未来实现时，建议至少提供以下命令语义：

- `agentjax init`
- `agentjax config validate`
- `agentjax config print`
- `agentjax config diff`
- `agentjax config reload`
- `agentjax config migrate`

### 16.1 `agentjax init`

负责：

- 初始化 config root
- 生成最小示例文件
- 可选创建默认 agent workspace

### 16.2 `agentjax config validate`

负责：

- 执行完整校验
- 输出错误、警告、建议

### 16.3 `agentjax config print`

负责：

- 输出归一化后的只读配置视图
- 默认脱敏

### 16.4 `agentjax config diff`

负责：

- 对比当前生效快照与候选配置
- 给出受影响模块和 reload 级别

### 16.5 `agentjax config reload`

负责：

- 请求 daemon 重新加载配置
- 输出 reload 结果、失败模块和回滚信息

---

## 17. 推荐实现切分

建议在代码结构中新增或明确以下模块：

```text
src/
  config/
    mod.rs
    paths.rs
    schema.rs
    loader.rs
    validator.rs
    normalizer.rs
    migrator.rs
    initializer.rs
    snapshot.rs
    diff.rs
    secrets.rs
```

---

## 18. 最小落地顺序

若后续进入实现阶段，建议按以下顺序推进：

1. 配置路径建模与 config root 发现
2. `core.toml` + `providers.toml` + `models.toml` 的 typed loader
3. 初始化器与最小模板生成
4. 统一校验器与错误模型
5. 归一化快照与打印能力
6. diff 与 reload plan
7. 迁移器与 secret resolver

---

## 19. 非目标

本阶段不定义：

- 图形化配置编辑器
- 远程配置中心
- 多租户 SaaS 配置控制台
- 完整 secret manager 实现
- 所有插件私有配置字段细节

这些能力可以在本规范稳定后作为上层扩展。

---

## 20. 结论

AgentJax 不能继续依赖硬编码配置推进 runtime 演化。

正式方向应是：

- 用 `config/` 承载 runtime wiring
- 用 `Config Manager` 承载加载、初始化、校验、迁移、发布与热重载协调
- 用归一化配置快照隔离 runtime 模块与原始文件格式

这样才能让后续的 daemon、plugin runtime、scheduler、channels、providers 真正具备可维护的配置生命周期。
