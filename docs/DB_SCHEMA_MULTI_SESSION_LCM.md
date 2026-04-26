# AgentJax Database Schema: Multi-Session + LCM

## Scope
This schema extends SQLite persistence for:
- multi-session scale (efficient listing/filtering)
- LCM durability (summary DAG / projection snapshots / compaction records)

## Core Runtime Tables (existing)
- `sessions`
- `session_messages`
- `runtime_events`
- `schema_migrations`

## Multi-Session Optimizations (added indexes)
- `idx_sessions_workspace_updated` on `sessions(workspace_id, updated_at DESC, created_at DESC)`
- `idx_sessions_status_updated` on `sessions(status, updated_at DESC, created_at DESC)`
- `idx_session_messages_turn_seq` on `session_messages(turn_id, sequence_no)`
- `idx_session_messages_actor_created` on `session_messages(actor_id, created_at)`
- `idx_runtime_events_workspace_session_time` on `runtime_events(workspace_id, session_id, occurred_at)`
- `idx_runtime_events_correlation_time` on `runtime_events(correlation_id, occurred_at)`
- `idx_runtime_events_idempotency` partial index on non-null `idempotency_key`

## LCM Tables
- `lcm_summary_nodes`
  - durable nodes for leaf/condensed/checkpoint summaries
  - includes confidence/freshness/invalidation and token/time bounds
- `lcm_summary_event_refs`
  - many-to-many mapping from summary node to source runtime events
  - enforces FK integrity with `runtime_events`
- `lcm_summary_artifact_refs`
  - many-to-many mapping from summary node to artifact IDs
- `lcm_compaction_runs`
  - compaction execution history, levels, token delta, status, produced summary node
- `lcm_context_projections`
  - assembled context snapshots with token breakdown + refs + serialized blocks
- `lcm_large_file_refs`
  - path-based references for large files with exploration summary metadata

## API/Store Changes
- Added `session.create` API method.
- Added `SessionStore::list_session_heads()` for lightweight session listing without loading full message/event history.

## Migration
- New migration id: `2026_04_27_0003_lcm_runtime_store`
