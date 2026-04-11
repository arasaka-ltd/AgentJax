# AgentJax `src/plugins` 重构治理计划
## 1. 目标
本文档定义对 `/Users/jaxlocke/AgentJax/src/plugins` 的正式重构治理方向。

当前问题不是“目录名字不够好看”。
而是系统把很多本体内建能力、基础设施模块、真实插件能力混在了同一个 `plugins/` 目录下，导致：
- 插件边界失真
- enable/disable 失真
- 生命周期失真
- 目录结构误导
- 后续动态装载、热重载、配置治理都很难成立

这次要拍死三个方向：
- 内建能力回归本体代码，而不是伪装成插件
- 真正插件按“一个插件一个目录”组织
- 引入正式 `PluginManager` 负责启停、配置、装载与状态

---
## 2. 当前偏差
### 2.1 `src/plugins` 现在更像“内置模块大杂烩”
当前实际结构是按能力类别堆放模块：
- `src/plugins/tools/*`
- `src/plugins/storage/*`
- `src/plugins/providers/*`
- `src/plugins/context/*`
- `src/plugins/channels/*`

这不是“插件目录”，而是一个名为 `plugins` 的内置模块汇总目录。

### 2.2 `Application::new()` 仍是硬编码注册
当前插件、工具、storage、context 的装载基本都直接写死在启动流程里。[src/app.rs](/Users/jaxlocke/AgentJax/src/app.rs#L39)

这意味着：
- 没有真实 discovery
- 没有真实 enable/disable
- 没有真实实例生命周期切换
- 没有真正的 plugin 管理层

### 2.3 “插件状态”目前只是伪状态
当前 `plugin.list` 基本只是把 manifest 列出来，再统一标成 `Running`。[src/daemon/service.rs](/Users/jaxlocke/AgentJax/src/daemon/service.rs#L1462)

这说明系统现在只有：
- manifest registry

而没有：
- plugin runtime state
- enabled/disabled plan
- prepare/start/stop/drain/fail 生命周期

---
## 3. 重构结论
### 3.1 哪些不再归类为插件
以下能力应回归本体代码或 builtin 子系统，而不是继续放在 `src/plugins` 下伪装成插件：

#### Builtin tools
例如：
- `read`
- `edit`
- `write`
- `list`

这些属于 AgentJax 的基础操作面，应视为 core/builtin capability，而不是外部插件。

#### Builtin storage
例如：
- SQLite session store
- SQLite event/context store
- 本地 artifact store

这些属于 runtime foundation。
它们可以保留插件式 trait 边界，但实现代码不应继续放在 `src/plugins/storage/*` 里冒充“独立插件生态”。

#### Builtin context/runtime internals
例如：
- workspace identity loader
- task state context
- summary loader
- retrieval bridge 这类明显偏内建的 context assembly 组件

这些更像 runtime internals 或 builtin context providers，而不是可独立治理的外部插件。

### 3.2 哪些应保留为真正插件
更适合作为真正插件的通常是：
- 渠道接入，如 `telegram`
- 外部 provider，如 `openai`
- 外部 node / browser / remote worker
- 外部 scheduler backend
- 未来的 MCP bridge / remote executor / third-party adapter

这类模块具有更明显的：
- 外部依赖
- 独立配置
- 独立启停价值
- 真实 enable/disable 意义

---
## 4. 目标结构
### 4.1 总原则
不要再采用：
```text
src/plugins/providers/openai.rs
src/plugins/channels/telegram.rs
src/plugins/tools/read_file.rs
src/plugins/storage/sqlite_sessions.rs
```

应改为：
- builtin 与 internal 模块离开 `src/plugins`
- 真正插件一插件一目录

### 4.2 推荐目标目录
建议最终演化为：

```text
src/
  builtin/
    tools/
      read.rs
      edit.rs
      write.rs
      list.rs
    storage/
      sqlite/
        mod.rs
        backend.rs
        sessions.rs
        context.rs
    context/
      workspace_identity.rs
      task_state.rs
      summary_loader.rs
      retrieval_bridge.rs

  plugins/
    openai/
      mod.rs
      plugin.rs
      config.rs
    telegram/
      mod.rs
      plugin.rs
      config.rs
    local_scheduler/
      mod.rs
      plugin.rs
    static_nodes/
      mod.rs
      plugin.rs
```

### 4.3 目录语义
- `src/builtin/**`：本体内建能力
- `src/plugins/<plugin_id>/**`：真正插件
- `src/core/**`：协议、管理器、生命周期、注册中心

这三层必须分开，不然“插件系统”永远只是命名错觉。

---
## 5. Plugin Manager 的正式职责
现在最缺的不是再多几个 plugin trait，而是一个真正的 `PluginManager`。

### 5.1 `PluginRegistry` 不等于 `PluginManager`
`PluginRegistry` 负责：
- 保存 manifest / instance 索引
- capability 查询
- 依赖校验

但它不应同时承担：
- 配置解析
- enable/disable 决策
- 生命周期驱动
- reload / drain / swap
- 健康状态管理

这些应归 `PluginManager`。

### 5.2 `PluginManager` 应负责什么
正式建议 `PluginManager` 负责：
- 读取 `plugins.toml`
- 解析 enabled / disabled / config refs / policy flags
- 组装可启用插件计划
- 解析依赖与启动顺序
- 实例化插件
- 调用 `on_load` / `on_startup` / `on_shutdown`
- 管理 plugin runtime state
- 提供 enable / disable / reload 操作
- 向 daemon 暴露真实 plugin status

### 5.3 建议状态模型
至少支持：
- `Discovered`
- `Disabled`
- `Loading`
- `Loaded`
- `Starting`
- `Running`
- `Stopping`
- `Stopped`
- `Failed`

当前统一返回 `Running` 的做法必须结束。

### 5.4 建议放置位置
建议新增：
- `src/core/plugin_manager.rs`

必要时再拆：
- `src/core/plugin_manager/loader.rs`
- `src/core/plugin_manager/state.rs`
- `src/core/plugin_manager/plan.rs`
- `src/core/plugin_manager/config.rs`

---
## 6. Builtin 与 Plugin 的判定标准
为了避免以后再次把内建模块塞回 `src/plugins/`，需要有明确判定标准。

### 6.1 更像 builtin 的特征
满足越多，越应放入 `src/builtin/**`：
- 是本体必须存在的基础能力
- 没有独立 enable/disable 价值
- 没有独立部署价值
- 没有明显外部依赖
- 与 runtime 内核强耦合
- 更像 framework implementation，而不是 extension

### 6.2 更像 plugin 的特征
满足越多，越应放入 `src/plugins/<plugin_id>/**`：
- 依赖外部服务或外部平台
- 有独立配置
- 有独立凭据或连接参数
- 允许被启用 / 禁用
- 允许被替换
- 允许失败降级
- 有独立生命周期和健康状态

---
## 7. 配置治理方向
现有文档已经定义 `plugins.toml` 承担：
- enabled plugins
- disabled plugins
- plugin-specific config refs
- plugin policy flags

这次重构要把这层从“文档存在”变成“真正 runtime 生效”。[docs/WORKSPACE_AND_CONFIG_SPEC.md](/Users/jaxlocke/AgentJax/docs/WORKSPACE_AND_CONFIG_SPEC.md#L272)

### 7.1 `plugins.toml` 应负责
- 哪些插件启用
- 哪些插件禁用
- 每个插件的 config fragment 引用
- policy flags
- reload 策略提示

### 7.2 builtin 不应通过 `plugins.toml` 关闭核心生存能力
要明确：
- builtin 不等于 plugin
- 不是所有模块都应该出现在 `plugins.toml`

例如：
- 核心 builtin file tools
- 核心 builtin storage substrate
- 核心 context assembly primitives

这些可以有 feature flag 或 policy，但不应被当成普通插件随手关掉。

---
## 8. 迁移策略
### 8.1 第一阶段：先分清 builtin 与 plugin
目标：
- 停止继续把 builtin 模块加到 `src/plugins/*`
- 建立 `src/builtin/**`

建议迁移：
- `src/plugins/tools/*` -> `src/builtin/tools/*`
- `src/plugins/storage/*` -> `src/builtin/storage/*`
- `src/plugins/context/*` -> `src/builtin/context/*`

注意：
- 这里不是说 trait 边界消失
- 而是实现归位，语义纠偏

### 8.2 第二阶段：重组真正插件目录
把现有类别目录改成一插件一目录：
- `src/plugins/providers/openai.rs` -> `src/plugins/openai/*`
- `src/plugins/channels/telegram.rs` -> `src/plugins/telegram/*`
- `src/plugins/scheduler/local_scheduler.rs` -> `src/plugins/local_scheduler/*`
- `src/plugins/nodes/static_registry.rs` -> `src/plugins/static_nodes/*`

### 8.3 第三阶段：引入 `PluginManager`
目标：
- Application 不再手写注册所有插件
- 启动逻辑变成“builtin boot + plugin manager load”

推荐流程：
1. 初始化 builtin runtime
2. 读取 `plugins.toml`
3. 生成 plugin load plan
4. 装载 enabled plugins
5. 注册 plugin states
6. 对 daemon 暴露真实 plugin status

### 8.4 第四阶段：把 daemon 的 plugin API 接到真实状态机
当前 `plugin.list` / `plugin.inspect` / `plugin.reload` 只有壳。
重构后应接到 `PluginManager`：
- `plugin.list` 返回真实状态
- `plugin.inspect` 返回 config / dependencies / health / lifecycle state / provided resources / last error
- `plugin.reload` 真正触发 drain / reload / restart plan
- `plugin.test` 返回 manager 视角的 readiness checks，而不是只验证 manifest 是否存在
- `smoke.run` 至少应能对 plugin manager 做端到端冒烟校验，避免“接口可达但插件控制面不可用”的假阳性

---
## 9. 对 `Application::new()` 的影响
当前 `Application::new()` 过度承担了 runtime wiring、builtin 注册、plugin 注册、storage 初始化等职责。[src/app.rs](/Users/jaxlocke/AgentJax/src/app.rs#L39)

目标应改成：

```text
Application::new()
  -> build builtin runtime
  -> build resource layer
  -> build plugin manager
  -> plugin manager loads enabled plugins
  -> compose runtime host
```

也就是说：
- builtin 初始化和 plugin 装载应分开
- `Application::new()` 不应继续是一个手写“把所有伪插件塞进 registry”的巨型装配点

---
## 10. 建议代码边界
### 10.1 推荐新增
- `src/builtin/mod.rs`
- `src/builtin/tools/mod.rs`
- `src/builtin/storage/mod.rs`
- `src/builtin/context/mod.rs`
- `src/core/plugin_manager.rs`

### 10.2 推荐收缩
- `src/plugins/mod.rs` 应只 re-export 真正插件
- 不再按 `providers/`、`tools/`、`storage/`、`context/` 这种“能力分类目录”继续扩张

### 10.3 推荐保留
trait / capability 边界仍然保留：
- `Plugin`
- `ProviderPlugin`
- `ToolPlugin`
- `StoragePlugin`
- `ContextPlugin`

因为问题不在 trait 抽象本身，而在实现组织和 runtime 治理层缺失。

---
## 11. 风险与约束
### 11.1 不要一步追求动态插件发现
第一阶段重点是治理边界，不是立刻做：
- 动态库装载
- 远程插件 marketplace
- 沙箱执行

先把“什么是 builtin，什么是真插件，谁负责启停”拍死。

### 11.2 不要把 builtin 硬做成可拔插来制造复杂度
一些核心能力即使保留 trait 边界，也不意味着必须具备普通插件级别的 enable/disable。

### 11.3 不要只改目录不改 runtime 语义
如果只是：
- 改文件位置
- 改 module path

但没有：
- `PluginManager`
- 真实 plugin state
- 真实 enable/disable

那这次重构仍然只是表面整理。

---
## 12. 最终结论
这次重构的核心不是“整理 `src/plugins`”。
而是把 AgentJax 从“伪插件目录”升级为“builtin runtime + real plugin system”。

正式结论如下：
- builtin tools 不再放在 `src/plugins/tools/*`
- builtin storage 不再放在 `src/plugins/storage/*`
- builtin context/runtime internals 不再放在 `src/plugins/context/*`
- 真正插件必须是一插件一目录，如 `src/plugins/openai/*`、`src/plugins/telegram/*`
- 必须新增 `PluginManager` 负责 enable/disable、生命周期、状态与 reload
- daemon 的 plugin API 必须改接真实 plugin state，而不是 manifest 伪状态
