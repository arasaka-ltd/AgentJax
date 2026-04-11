# AgentJax

AgentJax 是一个仍在快速迭代中的本地优先 agent runtime 原型。当前主链已经打通：

- workspace identity + memory/knowledge retrieval
- daemon API over unix socket / websocket
- session + runtime event SQLite persistence
- TUI streaming reply
- session-scoped model switching
- task runtime v0 with task timeline + checkpoint persistence

项目还不适合生产环境，但已经适合继续做本地开发、回归测试和 runtime 演进。

## Repo Layout

- `src/app.rs`: application host 和内置 plugin 注册
- `src/daemon/`: daemon service、store、task runtime
- `src/context_engine/`: context assembly、prompt rendering、resume pack
- `src/plugins/`: provider/tool/storage/context plugins
- `config/`: runtime 配置根
- `runtime/`: socket、SQLite、task/checkpoint state、日志
- `workspace/`: AGENT/SOUL/USER/MEMORY/MISSION/RULES/ROUTER、knowledge、memory、prompts

## Development

要求：

- Rust stable
- 本地可用的 `cargo`
- 如果要真实调用 OpenAI provider，需要设置 `OPENAI_API_KEY`

常用命令：

```bash
cargo fmt
cargo check
cargo test
```

初始化默认开发目录：

```bash
cargo run -- config init --mode local-dev
```

这会确保以下目录存在：

- `config/`
- `runtime/`
- `workspace/`

## Running

启动 daemon，仅 unix socket：

```bash
cargo run -- daemon --no-ws
```

启动 daemon，同时带 websocket surface：

```bash
cargo run -- daemon
```

默认 unix socket 路径：

```text
runtime/run/daemon.sock
```

常用 CLI 调用：

```bash
cargo run -- ping
cargo run -- status
cargo run -- session list
cargo run -- session get session.default
cargo run -- session send session.default "hello"
cargo run -- session send session.default "stream this" --stream
cargo run -- tui
```

Provider 相关：

```bash
cargo run -- provider list
cargo run -- provider models list --provider-id openai-default
cargo run -- provider models list --provider-id openai-default --refresh
cargo run -- provider test --provider-id openai-default --prompt "Say hello in one sentence."
```

## Runtime State

当前重要状态文件都在 `runtime/` 下：

- `runtime/run/daemon.sock`: unix socket
- `runtime/state/session_event_persistence.sqlite3`: session/message/runtime event persistence
- `runtime/state/tasks/`: task records
- `runtime/state/checkpoints/`: checkpoint records

workspace 中当前已接入的检索面：

- `workspace/MEMORY.md`
- `workspace/memory/topics/`
- `workspace/knowledge/`

## Current Status

已经完成到 `Batch 7: Cleanup + Hardening`。当前较稳的链路：

- CLI/TUI -> unix socket transport -> daemon API
- session.send -> context assembly -> provider call -> tool loop -> persistence
- session/task/runtime event/query after daemon restart

明确还没优先做的内容：

- Telegram / Discord / Email 全链路
- 完整 WebUI
- 高级 RAG
- 分布式 node routing
- 复杂热重载

## Notes

- 测试和本地运行默认使用 repo 根下的 `config/`, `runtime/`, `workspace/`
- 如果 `provider models list --refresh` 被调用，会刷新 provider snapshot，但不会覆盖 `models.toml` 里的显式 defaults
- 如果只想验证 transport/control-plane，不需要真实 provider API key
