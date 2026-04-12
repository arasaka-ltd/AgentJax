# AgentJax Shell Tool Spec
## 1. 目标
本文档定义 AgentJax 面向 Agent 的 shell 执行工具协议。

要解决的问题不是“再加一个能跑命令的万能工具”。
而是明确区分两类完全不同的执行语义：
- 无状态的一次性命令执行
- 有状态的交互式 shell 会话

同时还要钉死一个关键能力：
**Agent 必须可以同时打开多个 shell 会话，并选择在哪个会话里继续执行、查看输出、关闭会话。**

---
## 2. 一句话边界
- `shell.exec`：一次性执行单条命令，不保留上下文
- `shell.session.open`：创建一个有状态 shell 会话
- `shell.session.exec`：在指定会话中继续执行命令
- `shell.session.read`：读取某个会话的新输出、状态与最近结果
- `shell.session.list`：列出当前可用 shell 会话
- `shell.session.close`：关闭某个会话

可选增强：
- `shell.session.interrupt`
- `shell.session.resize`

---
## 3. 设计原则
### 3.1 shell 不是默认文件工具
Agent 不应为了普通读写文件而优先使用 shell。

优先级应是：
- 看文件，用 `read`
- 改文本，用 `edit`
- 新建或覆盖文件，用 `write`
- 只有确实需要命令执行、进程控制、环境探测、构建、测试时才用 `shell.*`

### 3.2 必须显式区分无状态与有状态
这两个能力不能混成一个模糊的 `shell`。

因为：
- 一次性命令更安全、更容易审计
- 有状态 shell 会话才适合 `export`、`cd`、激活 venv、持续运行 REPL、后台任务观察
- 如果协议不分开，Agent 很容易误判上下文是否会保留

### 3.3 有状态会话应尽量接近用户使用终端的直觉
`shell.session.*` 应尽量模拟一个真实终端会话：
- 当前工作目录会保留
- `export` 的环境变量会保留
- shell state 会保留
- 前一条命令的副作用会影响后一条

简单说：
**同一个 `session_id` 内，后续命令应在同一个 shell 进程上下文中继续运行。**

### 3.4 多会话是原生能力，不是附加技巧
Agent 可能需要同时做这些事：
- 一个会话跑 `npm test --watch`
- 一个会话跑 `cargo check`
- 一个会话临时查看日志

因此 runtime 必须原生支持：
- 同时存在多个 shell 会话
- 每个会话有独立状态
- Agent 能选择继续在哪个会话执行
- Agent 能查看各会话输出和运行状态

### 3.5 输出读取必须与执行解耦
不要要求每次执行时都返回完整最终输出。

因为：
- 长命令可能仍在运行
- 交互式命令可能持续产出 stdout/stderr
- Agent 需要“执行”和“观察”分开

因此：
- `exec` 负责发起命令
- `read` 负责读取新增输出与状态

### 3.6 shell 会话本质上是 runtime resource
状态化 shell 会话不是一次普通 tool call 的临时字符串。
它更接近：
- 一个可寻址 runtime object
- 一个有生命周期的执行资源
- 一个可被 event log / task runtime 观察的对象

---
## 4. Tool Catalog
建议第一阶段定义以下工具。

### 4.1 Stateless
- `shell.exec`

### 4.2 Stateful
- `shell.session.open`
- `shell.session.exec`
- `shell.session.read`
- `shell.session.list`
- `shell.session.close`

### 4.3 Optional
- `shell.session.interrupt`
- `shell.session.resize`

---
## 5. `shell.exec`
### 5.1 作用
执行一次性命令。

适合：
- `pwd`
- `git status`
- `cargo check`
- `ls`
- `python script.py`

不适合：
- 依赖上一条 `export` 的命令
- 依赖 `cd` 后继续工作的多步过程
- 持续运行并需要后续观察的命令

### 5.2 语义
- 每次调用都是独立执行
- 默认不继承之前任意 `shell.exec` 的上下文
- 调用结束后不保留 shell 状态

### 5.3 输入
```json
{
  "command": "cargo check",
  "cwd": "/workspace/project",
  "env": {
    "RUST_LOG": "debug"
  },
  "shell": "/bin/zsh",
  "timeout_secs": 120,
  "capture_limit": 12000
}
```

### 5.4 输入字段定义
- `command: string`
  - 必填
  - 要执行的命令文本
- `cwd?: string`
  - 可选
  - 工作目录
- `env?: object`
  - 可选
  - 本次执行额外环境变量
- `shell?: string`
  - 可选
  - 例如 `/bin/zsh`、`/bin/bash`
- `timeout_secs?: integer`
  - 可选
  - 超时预算
- `capture_limit?: integer`
  - 可选
  - 最大采集输出字节数或字符数

### 5.5 输出
```json
{
  "exit_code": 0,
  "status": "completed",
  "stdout": "Finished dev profile...",
  "stderr": "",
  "combined_output": "Finished dev profile...",
  "cwd": "/workspace/project",
  "timed_out": false,
  "truncated": false
}
```

### 5.6 使用策略
- 单步探测、构建、检查时优先使用
- 如果命令之间需要共享环境或目录，不要连续调用 `shell.exec`，改用 `shell.session.*`

---
## 6. `shell.session.open`
### 6.1 作用
创建一个新的有状态 shell 会话。

### 6.2 语义
- 创建后返回 `session_id`
- 后续 `shell.session.exec` 在同一会话中继续运行
- 会话应保留：
  - 当前工作目录
  - exported env
  - shell options / aliases / functions
  - 仍在运行的前台或后台进程状态

### 6.3 输入
```json
{
  "cwd": "/workspace/project",
  "env": {
    "NODE_ENV": "development"
  },
  "shell": "/bin/zsh",
  "pty": true,
  "title": "frontend-dev"
}
```

### 6.4 输出
```json
{
  "session_id": "shsess_123",
  "status": "idle",
  "cwd": "/workspace/project",
  "shell": "/bin/zsh",
  "pty": true,
  "title": "frontend-dev"
}
```

---
## 7. `shell.session.exec`
### 7.1 作用
在指定 shell 会话中执行一条命令。

### 7.2 语义
- 命令在已有会话上下文中执行
- 可依赖此前的 `cd`, `export`, `source`, `alias`
- 如果会话里已有前台命令正在运行，实现可：
  - 拒绝执行
  - 排队执行
  - 或要求先 interrupt

第一阶段建议：
- **同一会话同一时刻只允许一个前台执行单元**
- 多并行通过“多会话”解决，而不是在单会话里搞复杂 job control

### 7.3 输入
```json
{
  "session_id": "shsess_123",
  "command": "export API_KEY=abc && npm test",
  "timeout_secs": 30,
  "detach": false
}
```

### 7.4 输出
```json
{
  "session_id": "shsess_123",
  "exec_id": "shexec_456",
  "accepted": true,
  "status": "running"
}
```

说明：
- 不要求同步返回完整输出
- 长任务可先返回 `running`
- 后续由 `shell.session.read` 读取结果

---
## 8. `shell.session.read`
### 8.1 作用
读取 shell 会话的新增输出、最近执行状态与当前会话元信息。

### 8.2 输入
```json
{
  "session_id": "shsess_123",
  "since_seq": 42,
  "max_bytes": 8000
}
```

### 8.3 输出
```json
{
  "session_id": "shsess_123",
  "status": "idle",
  "active_exec_id": null,
  "cwd": "/workspace/project",
  "seq": 57,
  "chunks": [
    {
      "seq": 43,
      "stream": "stdout",
      "text": "PASS src/app.test.ts\n"
    },
    {
      "seq": 44,
      "stream": "stderr",
      "text": "warning: ...\n"
    }
  ],
  "last_exit_code": 0,
  "truncated": false
}
```

### 8.4 设计要求
- 输出必须可增量读取
- 必须有稳定递增的 `seq`
- 必须能区分 `stdout` / `stderr`
- 必须能看出当前会话是否仍在运行命令

---
## 9. `shell.session.list`
### 9.1 作用
列出当前所有 shell 会话，供 Agent 选择继续在哪个会话工作。

### 9.2 输出示例
```json
{
  "sessions": [
    {
      "session_id": "shsess_123",
      "title": "frontend-dev",
      "status": "running",
      "cwd": "/workspace/project/web",
      "shell": "/bin/zsh",
      "last_active_at": "2026-04-12T10:00:00Z"
    },
    {
      "session_id": "shsess_456",
      "title": "rust-check",
      "status": "idle",
      "cwd": "/workspace/project",
      "shell": "/bin/zsh",
      "last_active_at": "2026-04-12T10:01:30Z"
    }
  ]
}
```

---
## 10. `shell.session.close`
### 10.1 作用
关闭某个 shell 会话并释放资源。

### 10.2 输入
```json
{
  "session_id": "shsess_123",
  "force": false
}
```

### 10.3 语义
- `force = false` 时，如仍有运行中的前台任务，实现可拒绝关闭
- `force = true` 时，可终止会话及其子进程

### 10.4 输出
```json
{
  "session_id": "shsess_123",
  "closed": true
}
```

---
## 11. Optional: `shell.session.interrupt`
### 11.1 作用
向当前前台执行发送中断信号。

### 11.2 适用场景
- 长命令卡住
- REPL 需要 `Ctrl-C`
- 用户要求停止当前任务

### 11.3 输出示例
```json
{
  "session_id": "shsess_123",
  "signaled": true,
  "signal": "SIGINT"
}
```

---
## 12. Session Model
建议把状态化 shell 会话当作独立 runtime object。

```rust
pub struct ShellSession {
    pub session_id: String,
    pub shell: String,
    pub cwd: String,
    pub status: ShellSessionStatus,
    pub pty: bool,
    pub title: Option<String>,
    pub active_exec_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
```

```rust
pub enum ShellSessionStatus {
    Idle,
    Running,
    Closed,
    Failed,
}
```

执行记录建议独立：

```rust
pub struct ShellExecution {
    pub exec_id: String,
    pub session_id: Option<String>,
    pub mode: ShellExecutionMode,
    pub command: String,
    pub status: ShellExecutionStatus,
    pub exit_code: Option<i32>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

```rust
pub enum ShellExecutionMode {
    Stateless,
    SessionBound,
}
```

---
## 13. Event Model
shell 执行必须进入统一 runtime event log。

建议至少记录：
- `shell_session_opened`
- `shell_session_closed`
- `shell_execution_started`
- `shell_output_appended`
- `shell_execution_completed`
- `shell_execution_failed`
- `shell_execution_interrupted`

说明：
- `tool_called` / `tool_completed` 仍然保留
- 上述 shell-specific events 用于更细粒度回放和观察

---
## 14. 与 Turn / Task Runtime 的关系
### 14.1 `shell.exec`
更像一次普通 tool call：
- 发起
- 等待结束或超时
- 返回结果

### 14.2 `shell.session.*`
更像 runtime resource + long-running execution：
- 一个 turn 可以创建会话
- 后续 turn 可以继续同一会话
- 一个 task 可以绑定某个 shell session 做多步执行

### 14.3 多会话并发的推荐策略
并发来源应优先是：
- 多个 `session_id`

不建议第一阶段优先做：
- 单 shell 内复杂 job control
- 完整 `fg/bg/jobs` 语义

---
## 15. 安全与治理边界
### 15.1 权限应显式分级
至少建议区分：
- 只读命令执行
- workspace 内命令执行
- 网络访问
- 系统级破坏性命令
- 跨 workspace 访问

### 15.2 会话资源必须可治理
必须可配置：
- 最大会话数
- 单会话最大空闲时长
- 单命令超时
- 最大输出缓存
- 是否允许 PTY

### 15.3 输出不能无限增长
实现必须考虑：
- ring buffer
- chunk persistence
- 截断标记
- 增量消费

---
## 16. Agent 使用策略
建议策略如下：

- 单条探测命令优先用 `shell.exec`
- 需要 `cd` / `export` / `source venv/bin/activate` / 长时间观察时，用 `shell.session.open + shell.session.exec`
- 需要并行跑多个命令时，创建多个 shell session
- 在继续执行前，Agent 应先决定“复用已有会话”还是“新开会话”
- 查看长任务结果时，优先用 `shell.session.read`，而不是重复执行同一命令

---
## 17. 第一阶段最小验收标准
- 有 `shell.exec`
- 有 `shell.session.open`
- 有 `shell.session.exec`
- 有 `shell.session.read`
- 有 `shell.session.list`
- 有 `shell.session.close`
- 同时支持至少多个并存 shell 会话
- 同一会话内 `cd` 和 `export` 的效果可被后续命令观察到
- Agent 能明确选择继续某个 `session_id`
- shell 关键动作进入 runtime event log

---
## 18. 非目标
第一阶段不要求：
- 完整终端 UI 仿真
- 任意交互式全屏程序良好渲染
- 完整 job control
- 远程 node shell 调度
- shell 结果自动变成文件工具替代品

第一阶段的重点是：
**把“无状态命令执行”和“多会话有状态 shell”这两个语义钉死。**
