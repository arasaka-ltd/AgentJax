use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};

use crate::{
    context_engine::{ContextAssemblyRequest, TokenBreakdown},
    domain::{ContextProjection, SummaryNode},
};

#[derive(Debug, Clone)]
pub struct LcmSqliteStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct PersistRequest<'a> {
    pub workspace_id: &'a str,
    pub request: &'a ContextAssemblyRequest,
    pub projection: &'a ContextProjection,
    pub token_breakdown: &'a TokenBreakdown,
    pub included_refs: &'a [String],
    pub omitted_refs: &'a [String],
    pub summaries: &'a [SummaryNode],
    pub checkpoint: Option<&'a SummaryNode>,
    pub compaction_reason: &'a str,
}

impl LcmSqliteStore {
    pub fn open(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create sqlite state directory for lcm at {}",
                    parent.display()
                )
            })?;
        }
        let connection = Connection::open(&db_path)
            .with_context(|| format!("failed to open lcm sqlite at {}", db_path.display()))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .context("failed to enable WAL for lcm sqlite")?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .context("failed to enable foreign keys for lcm sqlite")?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .context("failed to set sqlite busy_timeout")?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runtime_events (
                    event_id TEXT PRIMARY KEY
                );
                CREATE TABLE IF NOT EXISTS lcm_summary_nodes (
                    summary_node_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    session_id TEXT NULL,
                    task_id TEXT NULL,
                    depth INTEGER NOT NULL,
                    summary_type TEXT NOT NULL,
                    content TEXT NOT NULL,
                    earliest_at TEXT NULL,
                    latest_at TEXT NULL,
                    descendant_count INTEGER NOT NULL DEFAULT 0,
                    token_count INTEGER NOT NULL DEFAULT 0,
                    confidence TEXT NOT NULL,
                    freshness TEXT NOT NULL,
                    invalidation_status TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    schema_version TEXT NOT NULL,
                    meta_json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_summary_scope_depth
                    ON lcm_summary_nodes(workspace_id, session_id, task_id, depth);
                CREATE INDEX IF NOT EXISTS idx_lcm_summary_latest_at
                    ON lcm_summary_nodes(latest_at);
                CREATE INDEX IF NOT EXISTS idx_lcm_summary_freshness
                    ON lcm_summary_nodes(freshness, invalidation_status);
                CREATE TABLE IF NOT EXISTS lcm_summary_event_refs (
                    summary_node_id TEXT NOT NULL,
                    event_id TEXT NOT NULL,
                    position_no INTEGER NOT NULL,
                    PRIMARY KEY (summary_node_id, event_id),
                    FOREIGN KEY(summary_node_id) REFERENCES lcm_summary_nodes(summary_node_id) ON DELETE CASCADE,
                    FOREIGN KEY(event_id) REFERENCES runtime_events(event_id) ON DELETE RESTRICT
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_summary_event_refs_event
                    ON lcm_summary_event_refs(event_id, position_no);
                CREATE TABLE IF NOT EXISTS lcm_summary_artifact_refs (
                    summary_node_id TEXT NOT NULL,
                    artifact_id TEXT NOT NULL,
                    position_no INTEGER NOT NULL,
                    PRIMARY KEY (summary_node_id, artifact_id),
                    FOREIGN KEY(summary_node_id) REFERENCES lcm_summary_nodes(summary_node_id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_summary_artifact_refs_artifact
                    ON lcm_summary_artifact_refs(artifact_id, position_no);
                CREATE TABLE IF NOT EXISTS lcm_compaction_runs (
                    compaction_run_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    session_id TEXT NULL,
                    task_id TEXT NULL,
                    level TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL,
                    output_tokens INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    source_start_event_id TEXT NULL,
                    source_end_event_id TEXT NULL,
                    produced_summary_node_id TEXT NULL,
                    started_at TEXT NOT NULL,
                    completed_at TEXT NULL,
                    error_text TEXT NULL,
                    meta_json TEXT NOT NULL,
                    FOREIGN KEY(produced_summary_node_id) REFERENCES lcm_summary_nodes(summary_node_id) ON DELETE SET NULL
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_compaction_runs_scope_time
                    ON lcm_compaction_runs(workspace_id, session_id, task_id, started_at);
                CREATE INDEX IF NOT EXISTS idx_lcm_compaction_runs_status_time
                    ON lcm_compaction_runs(status, started_at);
                CREATE TABLE IF NOT EXISTS lcm_context_projections (
                    projection_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    session_id TEXT NULL,
                    task_id TEXT NULL,
                    purpose TEXT NOT NULL,
                    block_count INTEGER NOT NULL,
                    token_total INTEGER NOT NULL,
                    token_stable_docs INTEGER NOT NULL,
                    token_runtime INTEGER NOT NULL,
                    token_summaries INTEGER NOT NULL,
                    token_fresh_tail INTEGER NOT NULL,
                    token_retrieval INTEGER NOT NULL,
                    included_refs_json TEXT NOT NULL,
                    omitted_refs_json TEXT NOT NULL,
                    blocks_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    schema_version TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_context_projections_scope_time
                    ON lcm_context_projections(workspace_id, session_id, task_id, created_at);
                CREATE INDEX IF NOT EXISTS idx_lcm_context_projections_purpose_time
                    ON lcm_context_projections(purpose, created_at);
                CREATE TABLE IF NOT EXISTS lcm_checkpoints (
                    checkpoint_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    session_id TEXT NULL,
                    task_id TEXT NULL,
                    source_summary_node_id TEXT NOT NULL,
                    current_goal TEXT NULL,
                    active_task_ids_json TEXT NOT NULL,
                    open_blockers_json TEXT NOT NULL,
                    pending_artifact_ids_json TEXT NOT NULL,
                    next_recommended_action TEXT NULL,
                    assumptions_json TEXT NOT NULL,
                    risks_json TEXT NOT NULL,
                    last_safe_action_boundary TEXT NULL,
                    token_count INTEGER NOT NULL,
                    freshness TEXT NOT NULL,
                    invalidation_status TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_checkpoints_scope_updated
                    ON lcm_checkpoints(workspace_id, session_id, task_id, updated_at);
                CREATE TABLE IF NOT EXISTS lcm_assembly_snapshots (
                    assembly_snapshot_id TEXT PRIMARY KEY,
                    projection_id TEXT NOT NULL,
                    workspace_id TEXT NOT NULL,
                    session_id TEXT NULL,
                    task_id TEXT NULL,
                    purpose TEXT NOT NULL,
                    token_total INTEGER NOT NULL,
                    token_breakdown_json TEXT NOT NULL,
                    included_refs_json TEXT NOT NULL,
                    omitted_refs_json TEXT NOT NULL,
                    compaction_reason TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_lcm_assembly_snapshots_scope_time
                    ON lcm_assembly_snapshots(workspace_id, session_id, task_id, created_at);",
            )
            .context("failed to bootstrap lcm sqlite schema")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn persist(&self, request: PersistRequest<'_>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let connection = self
            .connection
            .lock()
            .expect("lcm sqlite connection lock poisoned");
        let tx = connection
            .unchecked_transaction()
            .context("failed to begin lcm sqlite transaction")?;

        for summary in request.summaries {
            tx.execute(
                "INSERT INTO lcm_summary_nodes (
                    summary_node_id, workspace_id, session_id, task_id, depth, summary_type, content,
                    earliest_at, latest_at, descendant_count, token_count, confidence, freshness,
                    invalidation_status, created_at, updated_at, schema_version, meta_json
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                    ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18
                )
                ON CONFLICT(summary_node_id) DO UPDATE SET
                    depth = excluded.depth,
                    summary_type = excluded.summary_type,
                    content = excluded.content,
                    earliest_at = excluded.earliest_at,
                    latest_at = excluded.latest_at,
                    descendant_count = excluded.descendant_count,
                    token_count = excluded.token_count,
                    confidence = excluded.confidence,
                    freshness = excluded.freshness,
                    invalidation_status = excluded.invalidation_status,
                    updated_at = excluded.updated_at,
                    schema_version = excluded.schema_version,
                    meta_json = excluded.meta_json",
                params![
                    summary.summary_node_id.as_str(),
                    request.workspace_id,
                    summary.session_id.as_deref(),
                    summary.task_id.as_deref(),
                    summary.depth,
                    format!("{:?}", summary.summary_type),
                    summary.content.as_str(),
                    summary.earliest_at.map(|value| value.to_rfc3339()),
                    summary.latest_at.map(|value| value.to_rfc3339()),
                    summary.descendant_count,
                    summary.token_count,
                    format!("{:?}", summary.confidence),
                    format!("{:?}", summary.freshness),
                    format!("{:?}", summary.invalidation_status),
                    summary.meta.created_at.to_rfc3339(),
                    &now,
                    summary.meta.schema_version.as_str(),
                    serde_json::to_string(summary).unwrap_or_else(|_| "{}".into()),
                ],
            )
            .context("failed to upsert lcm_summary_nodes row")?;

            tx.execute(
                "DELETE FROM lcm_summary_event_refs WHERE summary_node_id = ?1",
                params![summary.summary_node_id.as_str()],
            )
            .context("failed to clear summary event refs")?;
            for (position, event_id) in summary.source_event_ids.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO lcm_summary_event_refs (summary_node_id, event_id, position_no)
                     VALUES (?1, ?2, ?3)",
                    params![summary.summary_node_id.as_str(), event_id, position as i64],
                )
                .context("failed to write lcm_summary_event_refs row")?;
            }

            tx.execute(
                "DELETE FROM lcm_summary_artifact_refs WHERE summary_node_id = ?1",
                params![summary.summary_node_id.as_str()],
            )
            .context("failed to clear summary artifact refs")?;
            for (position, artifact_id) in summary.source_artifact_ids.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO lcm_summary_artifact_refs (summary_node_id, artifact_id, position_no)
                     VALUES (?1, ?2, ?3)",
                    params![summary.summary_node_id.as_str(), artifact_id, position as i64],
                )
                .context("failed to write lcm_summary_artifact_refs row")?;
            }
        }

        if let Some(checkpoint) = request.checkpoint {
            let checkpoint_id = checkpoint.summary_node_id.clone();
            let content = checkpoint.content.as_str();
            tx.execute(
                "INSERT INTO lcm_checkpoints (
                    checkpoint_id, workspace_id, session_id, task_id, source_summary_node_id, current_goal,
                    active_task_ids_json, open_blockers_json, pending_artifact_ids_json,
                    next_recommended_action, assumptions_json, risks_json, last_safe_action_boundary,
                    token_count, freshness, invalidation_status, updated_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, ?9,
                    ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17
                )
                ON CONFLICT(checkpoint_id) DO UPDATE SET
                    current_goal = excluded.current_goal,
                    active_task_ids_json = excluded.active_task_ids_json,
                    open_blockers_json = excluded.open_blockers_json,
                    pending_artifact_ids_json = excluded.pending_artifact_ids_json,
                    next_recommended_action = excluded.next_recommended_action,
                    assumptions_json = excluded.assumptions_json,
                    risks_json = excluded.risks_json,
                    last_safe_action_boundary = excluded.last_safe_action_boundary,
                    token_count = excluded.token_count,
                    freshness = excluded.freshness,
                    invalidation_status = excluded.invalidation_status,
                    updated_at = excluded.updated_at",
                params![
                    checkpoint_id.as_str(),
                    request.workspace_id,
                    checkpoint.session_id.as_deref(),
                    checkpoint.task_id.as_deref(),
                    checkpoint.summary_node_id.as_str(),
                    extract_field(content, "current_goal"),
                    json_array_from_line(content, "active_tasks"),
                    json_array_from_line(content, "pending_blockers"),
                    json_array_from_line(content, "important_live_refs"),
                    extract_field(content, "next_recommended_action"),
                    json_array_from_line(content, "assumptions"),
                    json_array_from_line(content, "risks"),
                    extract_field(content, "last_safe_action_boundary"),
                    checkpoint.token_count,
                    format!("{:?}", checkpoint.freshness),
                    format!("{:?}", checkpoint.invalidation_status),
                    &now,
                ],
            )
            .context("failed to upsert lcm_checkpoints row")?;
        }

        tx.execute(
            "INSERT INTO lcm_context_projections (
                projection_id, workspace_id, session_id, task_id, purpose, block_count, token_total,
                token_stable_docs, token_runtime, token_summaries, token_fresh_tail, token_retrieval,
                included_refs_json, omitted_refs_json, blocks_json, created_at, schema_version
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17
            )
            ON CONFLICT(projection_id) DO UPDATE SET
                workspace_id = excluded.workspace_id,
                session_id = excluded.session_id,
                task_id = excluded.task_id,
                purpose = excluded.purpose,
                block_count = excluded.block_count,
                token_total = excluded.token_total,
                token_stable_docs = excluded.token_stable_docs,
                token_runtime = excluded.token_runtime,
                token_summaries = excluded.token_summaries,
                token_fresh_tail = excluded.token_fresh_tail,
                token_retrieval = excluded.token_retrieval,
                included_refs_json = excluded.included_refs_json,
                omitted_refs_json = excluded.omitted_refs_json,
                blocks_json = excluded.blocks_json,
                created_at = excluded.created_at,
                schema_version = excluded.schema_version",
            params![
                request.projection.projection_id.as_str(),
                request.workspace_id,
                request.projection.session_id.as_deref(),
                request.projection.task_id.as_deref(),
                format!("{:?}", request.request.purpose),
                request.projection.blocks.len() as i64,
                request.token_breakdown.total as i64,
                request.token_breakdown.stable_docs as i64,
                request.token_breakdown.runtime as i64,
                request.token_breakdown.summaries as i64,
                request.token_breakdown.fresh_tail as i64,
                request.token_breakdown.retrieval as i64,
                serde_json::to_string(request.included_refs).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(request.omitted_refs).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&request.projection.blocks).unwrap_or_else(|_| "[]".into()),
                &now,
                "2026-04-27",
            ],
        )
        .context("failed to insert lcm_context_projections row")?;

        let snapshot_id = format!(
            "assembly::{}::{}",
            request.projection.projection_id,
            Utc::now().timestamp_millis()
        );
        tx.execute(
            "INSERT INTO lcm_assembly_snapshots (
                assembly_snapshot_id, projection_id, workspace_id, session_id, task_id, purpose,
                token_total, token_breakdown_json, included_refs_json, omitted_refs_json,
                compaction_reason, created_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10,
                ?11, ?12
            )",
            params![
                snapshot_id.as_str(),
                request.projection.projection_id.as_str(),
                request.workspace_id,
                request.projection.session_id.as_deref(),
                request.projection.task_id.as_deref(),
                format!("{:?}", request.request.purpose),
                request.token_breakdown.total as i64,
                serde_json::to_string(request.token_breakdown).unwrap_or_else(|_| "{}".into()),
                serde_json::to_string(request.included_refs).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(request.omitted_refs).unwrap_or_else(|_| "[]".into()),
                request.compaction_reason,
                &now,
            ],
        )
        .context("failed to insert lcm_assembly_snapshots row")?;

        if !request.summaries.is_empty() {
            let run_id = format!(
                "compaction::{}::{}",
                request.projection.projection_id,
                Utc::now().timestamp_millis()
            );
            let produced_id = request
                .summaries
                .last()
                .map(|summary| summary.summary_node_id.clone());
            tx.execute(
                "INSERT INTO lcm_compaction_runs (
                    compaction_run_id, workspace_id, session_id, task_id, level, reason,
                    input_tokens, output_tokens, status, source_start_event_id, source_end_event_id,
                    produced_summary_node_id, started_at, completed_at, error_text, meta_json
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6,
                    ?7, ?8, ?9, ?10, ?11,
                    ?12, ?13, ?14, ?15, ?16
                )",
                params![
                    run_id.as_str(),
                    request.workspace_id,
                    request.projection.session_id.as_deref(),
                    request.projection.task_id.as_deref(),
                    compaction_level_from_reason(request.compaction_reason),
                    request.compaction_reason,
                    request.token_breakdown.total as i64,
                    request.token_breakdown.summaries as i64,
                    "completed",
                    request
                        .summaries
                        .first()
                        .and_then(|summary| summary.source_event_ids.first().cloned()),
                    request
                        .summaries
                        .last()
                        .and_then(|summary| summary.source_event_ids.last().cloned()),
                    produced_id,
                    &now,
                    &now,
                    Option::<String>::None,
                    "{}",
                ],
            )
            .context("failed to insert lcm_compaction_runs row")?;
        }

        tx.commit()
            .context("failed to commit lcm sqlite transaction")?;
        Ok(())
    }
}

fn extract_field(content: &str, key: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}: ")).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn json_array_from_line(content: &str, key: &str) -> String {
    let values = extract_field(content, key)
        .unwrap_or_default()
        .split("||")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    serde_json::to_string(&values).unwrap_or_else(|_| "[]".into())
}

fn compaction_level_from_reason(reason: &str) -> &'static str {
    if reason.contains("level=3") {
        "deterministic_fallback"
    } else if reason.contains("level=2") {
        "aggressive"
    } else {
        "normal"
    }
}
