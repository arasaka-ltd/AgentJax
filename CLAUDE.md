# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and test commands

- Build: `cargo build`
- Run CLI help: `cargo run -- --help`
- Start the daemon: `cargo run -- daemon`
- Start daemon without WebSocket transport: `cargo run -- daemon --no-ws`
- Ping a running daemon: `cargo run -- ping`
- Check daemon status: `cargo run -- status`
- List sessions: `cargo run -- session list`
- Get a session: `cargo run -- session get session.default`
- Send a message to a session: `cargo run -- session send session.default "hello"`
- Send a streamed message: `cargo run -- session send session.default "hello" --stream`
- Run all tests: `cargo test`
- Run a single test: `cargo test <test_name>`
- Run tests in one file: `cargo test --test <integration_test_name>`
- Format: `cargo fmt`
- Check formatting: `cargo fmt -- --check`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`

Note: the repo currently appears to have no Rust tests checked in yet, so `cargo test <test_name>` / `--test` are for future tests rather than existing ones.

## High-level architecture

AgentJax is a daemon-first agent framework. The current implementation is an early skeleton that already reflects the intended separation between runtime host, transports, API schema, plugin/provider wiring, and long-context infrastructure.

### Runtime shape

- `src/main.rs` is only the async entrypoint; all behavior goes through `src/cli/mod.rs`.
- `src/cli/mod.rs` is the operator-facing client and daemon launcher. It supports:
  - `daemon` to run the server
  - `ping` / `status` for runtime checks
  - `session list|get|send` for interaction over the daemon API
- `src/bootstrap.rs` builds the application from hard-coded roots (`./config`, `./runtime`, `./workspace`) and assembles `Application`.
- `src/app.rs` is the composition root for the in-process runtime: config, workspace runtime, application runtime, plugin registry, resource registry, event bus, and context engine.

### Daemon, API, and transports

The key architectural rule is: **the daemon is the runtime host; clients talk to it over a shared API schema**.

- `src/daemon/service.rs` contains the request router and the currently implemented API methods:
  - `runtime.ping`
  - `runtime.status`
  - `session.list`
  - `session.get`
  - `session.send`
- `src/daemon/store.rs` is currently an in-memory state store. It owns session state, generated IDs, uptime/readiness flags, and recorded runtime events.
- `src/api/` defines the transport-agnostic protocol types:
  - envelope types (`hello`, `request`, `response`, `stream`, `error`)
  - IDs and error codes
  - method names and request/response payload structs
- `src/transport/unix.rs` and `src/transport/websocket.rs` are two transport adapters over the same envelope model.
  - Both require a `hello` handshake with `API_VERSION` before requests.
  - Unix transport is the default CLI control path.
  - WebSocket transport mirrors the same request/stream behavior for out-of-process clients.

When changing API behavior, keep `src/api/`, `src/daemon/service.rs`, and both transports aligned.

### Application runtime vs daemon runtime

There are two distinct layers that are easy to conflate:

- `src/core/runtime.rs` (`ApplicationRuntime`) is the model/provider execution layer. It resolves an agent definition, selects an LLM provider, and issues prompt requests.
- `src/daemon/service.rs` (`Daemon`) is the external control/interaction server. Right now `session.send` synthesizes a placeholder assistant response instead of calling `ApplicationRuntime`.

This means the provider/plugin system already exists, but the daemon session flow is still a skeleton and not yet fully wired into live model execution.

### Plugins and providers

Plugin support is centered in `src/core/` and `src/plugins/`.

- `src/core/plugin.rs` defines the main traits:
  - `Plugin`
  - `ResourceProviderPlugin`
  - `BillingPlugin`
  - `PluginContext`
- `src/core/registry.rs` stores registered plugins and exposes manifests/capability lookups.
- `src/plugins/providers/openai.rs` is the only implemented provider plugin today.
  - It exposes provider-backed resources into the `ResourceRegistry`.
  - It wraps Rig’s OpenAI client for text prompting.
  - It also provides placeholder local billing estimates for usage records.
- `src/app.rs` registers OpenAI plugins by iterating configured LLM providers and attaching their resources/manifests at startup.

If you add another provider, follow the OpenAI plugin pattern: config type -> provider adapter -> plugin manifest/resources -> registration in `Application::new`.

### Config model

Config types live in `src/config/`.

- `runtime.rs` defines `RuntimeConfig`, `AgentRuntimeConfig`, `AgentDefinition`, and `LlmRuntimeConfig`.
- `provider.rs` defines provider-specific config, currently only `OpenAiProviderConfig`.
- `bootstrap.rs` currently constructs config directly rather than loading real files.

Important current defaults:

- default agent id: `default-agent`
- default provider id: `openai-default`
- default model: `gpt-4o-mini`
- OpenAI API key is resolved from `OPENAI_API_KEY` unless provided inline

### Context engine and domain model

The codebase is structured around a larger intended architecture than the current runtime implements.

- `src/domain/` contains first-class runtime entities: agent, session, turn, task, event, artifact, node, resource, plugin, skill, schedule, summary, billing, usage, and policies.
- `src/context_engine/` defines the long-context interfaces and components (`assembler`, `compactor`, `expander`, `resume`, stores, schema).
- `src/context_engine/engine.rs` currently provides a `ContextEngine` trait plus a `NoopContextEngine` implementation.

So the domain and context-engine modules are mostly architectural scaffolding right now: they define the target system shape even where the daemon is still using simple in-memory behavior.

## Specs and docs

The `docs/` directory is important in this repository: it describes the intended architecture that the current Rust code is incrementally implementing.

Start with these when making structural changes:

- `docs/ARCHITECTURE_ENTRYPOINT.md` — overall project direction and module boundaries
- `docs/CHANNELS_DAEMON_CLIENT_SPEC.md` — daemon/client/surface/channel/transport split
- `docs/WORKSPACE_AND_CONFIG_SPEC.md` — workspace vs config vs state boundaries
- `docs/LCM_CONTEXT_ENGINE.md` — context engine responsibilities and layering

These docs are more detailed than the current implementation; prefer keeping code changes consistent with them unless the user asks to change the architecture.
