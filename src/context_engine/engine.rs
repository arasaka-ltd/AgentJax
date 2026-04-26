use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::builtin::context::{
    retrieval_bridge::RetrievalBridgeContextPlugin,
    retrieval_types::{
        KnowledgeSearchRequest, MemorySearchRequest, MemorySearchScope, RetrievalDocument,
        RetrievalDocumentKind, RetrievalMode, RetrievalScope,
    },
};
use crate::config::{WorkspaceIdentityPack, WorkspacePaths};
use crate::context_engine::{
    assembler::{AssembledContext, ContextAssemblyRequest, TokenBreakdown},
    event_store::{EventStore, MessageBodyRecord},
    expander::ExpansionResult,
    persistence::{LcmSqliteStore, PersistRequest},
    projection_store::ProjectionStore,
    schema::ContextEngineSchema,
};
use crate::domain::{
    Confidence, ContextAssemblyPurpose, ContextBlock, ContextBlockKind, ContextProjection,
    ContextSource, EventType, Freshness, InvalidationStatus, ObjectMeta, ResumePack, RuntimeEvent,
    SummaryNode, SummaryType,
};
use anyhow::Result;
use std::path::PathBuf;

const LEAF_SUMMARY_CHUNK_SIZE: usize = 4;
const SELECTED_SUMMARY_LIMIT: usize = 3;
const SUMMARY_AGGRESSIVE_WORDS: usize = 18;
const SUMMARY_FALLBACK_WORDS: usize = 10;
const FRESH_TAIL_AGGRESSIVE_LINES: usize = 6;
const FRESH_TAIL_FALLBACK_LINES: usize = 3;

pub trait ContextEngine: Send + Sync {
    fn append_event(&self, event: RuntimeEvent) -> Result<()>;
    fn assemble_context(&self, request: ContextAssemblyRequest) -> Result<AssembledContext>;
    fn build_resume_pack(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<ResumePack>;
    fn grep_history(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        query: &str,
    ) -> Result<ExpansionResult>;
    fn describe_object(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        object_ref: &str,
    ) -> Result<ExpansionResult>;
    fn expand_summary(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        summary_node_id: &str,
    ) -> Result<ExpansionResult>;
}

#[derive(Debug, Clone, Default)]
pub struct NoopContextEngine;

impl ContextEngine for NoopContextEngine {
    fn append_event(&self, _event: RuntimeEvent) -> Result<()> {
        Ok(())
    }

    fn assemble_context(&self, _request: ContextAssemblyRequest) -> Result<AssembledContext> {
        Ok(AssembledContext::default())
    }

    fn build_resume_pack(
        &self,
        _session_id: Option<&str>,
        _task_id: Option<&str>,
    ) -> Result<ResumePack> {
        Ok(ResumePack {
            workspace_id: None,
            session_id: None,
            task_id: None,
            mission_ref: None,
            active_task_ids: Vec::new(),
            latest_checkpoint_summary_id: None,
            summary_node_ids: Vec::new(),
            open_blockers: Vec::new(),
            pending_artifact_ids: Vec::new(),
            last_safe_action_boundary: None,
            next_recommended_action: None,
            assumptions: Vec::new(),
            risks: Vec::new(),
        })
    }

    fn grep_history(
        &self,
        _session_id: Option<&str>,
        _task_id: Option<&str>,
        query: &str,
    ) -> Result<ExpansionResult> {
        Ok(ExpansionResult {
            matched_refs: Vec::new(),
            distilled_text: format!("no history matches for {query}"),
        })
    }

    fn describe_object(
        &self,
        _session_id: Option<&str>,
        _task_id: Option<&str>,
        object_ref: &str,
    ) -> Result<ExpansionResult> {
        Ok(ExpansionResult {
            matched_refs: vec![object_ref.to_string()],
            distilled_text: format!("no description available for {object_ref}"),
        })
    }

    fn expand_summary(
        &self,
        _session_id: Option<&str>,
        _task_id: Option<&str>,
        summary_node_id: &str,
    ) -> Result<ExpansionResult> {
        Ok(ExpansionResult {
            matched_refs: vec![summary_node_id.to_string()],
            distilled_text: format!("no expansion available for {summary_node_id}"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceContextEngine {
    identity: WorkspaceIdentityPack,
    retrieval: RetrievalBridgeContextPlugin,
    events: EventStore,
    projections: ProjectionStore,
    schema: ContextEngineSchema,
    sqlite: Option<LcmSqliteStore>,
    sqlite_bootstrap_error: Option<String>,
    persistence_error_count: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
struct DerivedContextState {
    retrieval_query: String,
    runtime_blocks: Vec<ContextBlock>,
    summary_blocks: Vec<ContextBlock>,
    checkpoint_block: Option<ContextBlock>,
    fresh_tail_block: Option<ContextBlock>,
    summary_nodes: Vec<SummaryNode>,
    latest_checkpoint: Option<SummaryNode>,
    active_task_ids: Vec<String>,
    open_blockers: Vec<String>,
    pending_artifact_ids: Vec<String>,
    last_safe_action_boundary: Option<String>,
    next_recommended_action: Option<String>,
    assumptions: Vec<String>,
    risks: Vec<String>,
    included_refs: Vec<String>,
    omitted_refs: Vec<String>,
    compaction_reason: String,
}

#[derive(Debug, Clone)]
struct BudgetedBlocks {
    blocks: Vec<ContextBlock>,
    omitted_refs: Vec<String>,
    compaction_reason: String,
}

#[derive(Debug, Clone)]
struct TaskRuntimeState {
    active_task_ids: Vec<String>,
    latest_goal: Option<String>,
    latest_phase_label: Option<String>,
    open_blockers: Vec<String>,
    pending_artifact_ids: Vec<String>,
    source_event_ids: Vec<String>,
    last_safe_action_boundary: Option<String>,
    next_recommended_action: Option<String>,
    assumptions: Vec<String>,
    risks: Vec<String>,
}

impl WorkspaceContextEngine {
    pub fn new(
        identity: WorkspaceIdentityPack,
        workspace_paths: WorkspacePaths,
        runtime_state_root: PathBuf,
    ) -> Self {
        let (sqlite, sqlite_bootstrap_error) = match LcmSqliteStore::open(
            runtime_state_root.join("session_event_persistence.sqlite3"),
        ) {
            Ok(store) => (Some(store), None),
            Err(error) => {
                let detail = format!("lcm sqlite bootstrap failed: {error}");
                eprintln!("{detail}");
                (None, Some(detail))
            }
        };
        Self {
            identity,
            retrieval: RetrievalBridgeContextPlugin::new(&workspace_paths),
            events: EventStore::default(),
            projections: ProjectionStore::default(),
            schema: ContextEngineSchema {
                event_schema_version: "2026-04-13".into(),
                projection_schema_version: "2026-04-13".into(),
                summary_schema_version: "2026-04-13".into(),
                resume_schema_version: "2026-04-13".into(),
                checkpoint_schema_version: "2026-04-13".into(),
            },
            sqlite,
            sqlite_bootstrap_error,
            persistence_error_count: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ContextEngine for WorkspaceContextEngine {
    fn append_event(&self, event: RuntimeEvent) -> Result<()> {
        self.events.append(event);
        Ok(())
    }

    fn assemble_context(&self, request: ContextAssemblyRequest) -> Result<AssembledContext> {
        let mut derived = self.derive_context_state(
            request.session_id.as_deref(),
            request.task_id.as_deref(),
            &request.purpose,
        );

        let mut blocks = vec![
            workspace_block(
                "workspace.agent",
                ContextBlockKind::StableIdentity,
                &self.identity.agent.path.display().to_string(),
                &self.identity.agent.content,
            ),
            workspace_block(
                "workspace.soul",
                ContextBlockKind::StableIdentity,
                &self.identity.soul.path.display().to_string(),
                &self.identity.soul.content,
            ),
            workspace_block(
                "workspace.user",
                ContextBlockKind::UserProfile,
                &self.identity.user.path.display().to_string(),
                &self.identity.user.content,
            ),
            workspace_block(
                "workspace.mission",
                ContextBlockKind::Mission,
                &self.identity.mission.path.display().to_string(),
                &self.identity.mission.content,
            ),
            workspace_block(
                "workspace.rules",
                ContextBlockKind::Rule,
                &self.identity.rules.path.display().to_string(),
                &self.identity.rules.content,
            ),
            workspace_block(
                "workspace.router",
                ContextBlockKind::RuntimeDirective,
                &self.identity.router.path.display().to_string(),
                &self.identity.router.content,
            ),
        ];

        blocks.extend(derived.runtime_blocks.clone());
        if let Some(checkpoint_block) = derived.checkpoint_block.clone() {
            blocks.push(checkpoint_block);
        }
        blocks.extend(derived.summary_blocks.clone());
        if let Some(fresh_tail_block) = derived.fresh_tail_block.clone() {
            blocks.push(fresh_tail_block);
        }

        if matches!(request.retrieval_scope, RetrievalScope::Implicit)
            && !derived.retrieval_query.is_empty()
        {
            let memory_blocks = self.recall_memory_blocks(
                &derived.retrieval_query,
                request.budget_tokens as usize / 100,
            )?;
            let knowledge_blocks = self.retrieve_knowledge_blocks(
                &derived.retrieval_query,
                request.budget_tokens as usize / 150,
            )?;
            blocks.extend(memory_blocks);
            blocks.extend(knowledge_blocks);
        }

        let budgeted =
            self.enforce_budget(blocks, request.budget_tokens, &derived.compaction_reason);
        derived.omitted_refs.extend(budgeted.omitted_refs);
        derived.compaction_reason = budgeted.compaction_reason;

        let blocks = budgeted.blocks;
        let token_breakdown = self.compute_token_breakdown(&blocks);

        self.projections.replace(ContextProjection {
            projection_id: projection_id(
                request.session_id.as_deref(),
                request.task_id.as_deref(),
                &request.purpose,
            ),
            session_id: request.session_id.clone(),
            task_id: request.task_id.clone(),
            purpose: request.purpose.clone(),
            blocks: blocks.clone(),
        });

        let mut included_refs = BTreeSet::new();
        included_refs.extend(self.identity.source_paths());
        for block in &blocks {
            match &block.source {
                ContextSource::WorkspaceFile { path } => {
                    included_refs.insert(path.clone());
                }
                ContextSource::Memory { memory_ref } => {
                    included_refs.insert(memory_ref.clone());
                }
                ContextSource::Knowledge { knowledge_ref } => {
                    included_refs.insert(knowledge_ref.clone());
                }
                ContextSource::Summary { summary_node_id } => {
                    included_refs.insert(summary_node_id.clone());
                }
                ContextSource::EventLog { event_id } => {
                    included_refs.insert(event_id.clone());
                }
                ContextSource::Artifact { artifact_id } => {
                    included_refs.insert(artifact_id.clone());
                }
                _ => {}
            }
        }
        let mut included_refs_vec = included_refs.into_iter().collect::<Vec<_>>();
        included_refs_vec.extend(derived.included_refs);
        included_refs_vec.sort();
        included_refs_vec.dedup();

        let mut persistence_status = "lcm_persistence=disabled".to_string();
        match (self.projections.current(), self.sqlite.as_ref()) {
            (Some(projection), Some(sqlite)) => {
                match sqlite.persist(PersistRequest {
                    workspace_id: &self.identity.workspace_id,
                    request: &request,
                    projection: &projection,
                    token_breakdown: &token_breakdown,
                    included_refs: &included_refs_vec,
                    omitted_refs: &derived.omitted_refs,
                    summaries: &derived.summary_nodes,
                    checkpoint: derived.latest_checkpoint.as_ref(),
                    compaction_reason: &derived.compaction_reason,
                }) {
                    Ok(()) => {
                        persistence_status = format!(
                            "lcm_persistence=ok;errors={}",
                            self.persistence_error_count.load(Ordering::Relaxed)
                        );
                    }
                    Err(error) => {
                        let failures =
                            self.persistence_error_count.fetch_add(1, Ordering::Relaxed) + 1;
                        let detail = format!("lcm sqlite persist failed ({failures}): {error}");
                        eprintln!("{detail}");
                        persistence_status =
                            format!("lcm_persistence=error;errors={failures};last={error}");
                    }
                }
            }
            (_, None) => {
                if let Some(error) = self.sqlite_bootstrap_error.as_deref() {
                    persistence_status = format!("lcm_persistence=bootstrap_error;detail={error}");
                }
            }
            (None, Some(_)) => {
                persistence_status = "lcm_persistence=projection_missing".to_string();
            }
        }

        Ok(AssembledContext {
            blocks,
            token_breakdown,
            included_refs: included_refs_vec,
            omitted_refs: derived.omitted_refs,
            system_prompt_additions: vec![
                format!(
                    "workspace_id={}, purpose={:?}, retrieval_scope={:?}",
                    self.identity.workspace_id, request.purpose, request.retrieval_scope
                ),
                "lcm_summary_scope=user_message_and_assistant_message_bodies_only; tools, skills, and workspace core files are injected separately".into(),
                format!("lcm_compaction={}", derived.compaction_reason),
                persistence_status,
            ],
        })
    }

    fn build_resume_pack(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<ResumePack> {
        let derived =
            self.derive_context_state(session_id, task_id, &ContextAssemblyPurpose::Resume);
        Ok(ResumePack {
            workspace_id: Some(self.identity.workspace_id.clone()),
            session_id: session_id.map(str::to_string),
            task_id: task_id.map(str::to_string),
            mission_ref: Some(self.identity.mission.path.display().to_string()),
            active_task_ids: derived.active_task_ids,
            latest_checkpoint_summary_id: derived
                .latest_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.summary_node_id.clone()),
            summary_node_ids: derived
                .summary_nodes
                .iter()
                .map(|summary| summary.summary_node_id.clone())
                .collect(),
            open_blockers: derived.open_blockers,
            pending_artifact_ids: derived.pending_artifact_ids,
            last_safe_action_boundary: derived.last_safe_action_boundary,
            next_recommended_action: derived.next_recommended_action,
            assumptions: derived.assumptions,
            risks: derived.risks,
        })
    }

    fn grep_history(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        query: &str,
    ) -> Result<ExpansionResult> {
        let needle = query.to_ascii_lowercase();
        let messages = self.events.message_body_records(session_id, task_id);
        let mut matched_refs = Vec::new();
        let mut lines = Vec::new();
        for message in messages {
            let content = message.content.to_ascii_lowercase();
            if !content.contains(&needle) {
                continue;
            }
            matched_refs.push(message.event_id.clone());
            lines.push(format!(
                "{} [{}]: {}",
                message.role,
                message.event_id,
                truncate_for_summary(&message.content, 28)
            ));
            if lines.len() >= 8 {
                break;
            }
        }
        Ok(ExpansionResult {
            matched_refs,
            distilled_text: if lines.is_empty() {
                format!("no history matches for {query}")
            } else {
                lines.join("\n")
            },
        })
    }

    fn describe_object(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        object_ref: &str,
    ) -> Result<ExpansionResult> {
        if object_ref.starts_with("summary::") {
            return self.expand_summary(session_id, task_id, object_ref);
        }
        let scoped = self.events.list_scoped(session_id, task_id);
        if let Some(event) = scoped
            .into_iter()
            .find(|event| event.event_id == object_ref)
        {
            return Ok(ExpansionResult {
                matched_refs: vec![object_ref.to_string()],
                distilled_text: format!(
                    "event_type={:?}\noccurred_at={}\npayload={}",
                    event.event_type,
                    event.occurred_at.to_rfc3339(),
                    event.payload
                ),
            });
        }
        Ok(ExpansionResult {
            matched_refs: vec![object_ref.to_string()],
            distilled_text: format!("object not found in scoped history: {object_ref}"),
        })
    }

    fn expand_summary(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        summary_node_id: &str,
    ) -> Result<ExpansionResult> {
        let derived =
            self.derive_context_state(session_id, task_id, &ContextAssemblyPurpose::Resume);
        if let Some(summary) = derived
            .summary_nodes
            .iter()
            .find(|summary| summary.summary_node_id == summary_node_id)
        {
            let source_messages = self.events.message_body_records(session_id, task_id);
            let source_lines = source_messages
                .iter()
                .filter(|message| summary.source_event_ids.contains(&message.event_id))
                .take(16)
                .map(|message| {
                    format!(
                        "{} [{}]: {}",
                        message.role, message.event_id, message.content
                    )
                })
                .collect::<Vec<_>>();
            return Ok(ExpansionResult {
                matched_refs: summary.source_event_ids.clone(),
                distilled_text: if source_lines.is_empty() {
                    summary.content.clone()
                } else {
                    source_lines.join("\n")
                },
            });
        }
        if let Some(checkpoint) = derived.latest_checkpoint.as_ref() {
            if checkpoint.summary_node_id == summary_node_id {
                return Ok(ExpansionResult {
                    matched_refs: checkpoint.source_event_ids.clone(),
                    distilled_text: checkpoint.content.clone(),
                });
            }
        }
        Ok(ExpansionResult {
            matched_refs: vec![summary_node_id.to_string()],
            distilled_text: format!("summary not found in scoped context: {summary_node_id}"),
        })
    }
}

impl WorkspaceContextEngine {
    fn derive_context_state(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        purpose: &ContextAssemblyPurpose,
    ) -> DerivedContextState {
        let scoped_events = self.events.list_scoped(session_id, task_id);
        let message_bodies = self.events.message_body_records(session_id, task_id);
        let task_state = derive_task_runtime_state(&scoped_events, &message_bodies);
        let fresh_tail_limit = fresh_tail_limit(purpose);
        let summarized_len = message_bodies.len().saturating_sub(fresh_tail_limit);
        let summarized_messages = &message_bodies[..summarized_len];
        let fresh_tail_messages = &message_bodies[summarized_len..];

        let mut summary_nodes =
            self.build_message_summary_nodes(session_id, task_id, summarized_messages);
        let mut checkpoint =
            self.build_checkpoint_node(session_id, task_id, &task_state, &message_bodies);
        let mut risks = task_state.risks.clone();
        apply_summary_status_events(
            &scoped_events,
            &mut summary_nodes,
            checkpoint.as_mut(),
            &mut risks,
        );

        let selected_summaries = select_summary_nodes(&summary_nodes);
        let summary_blocks = selected_summaries
            .iter()
            .map(summary_block_from_node)
            .collect::<Vec<_>>();
        let checkpoint_block = checkpoint.as_ref().map(checkpoint_block_from_node);
        let fresh_tail_block = build_fresh_tail_block(fresh_tail_messages);
        let runtime_blocks = build_runtime_blocks(&task_state, purpose);

        let retrieval_query = message_bodies
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .or_else(|| message_bodies.last())
            .map(|message| message.content.clone())
            .unwrap_or_default();

        let mut included_refs = Vec::new();
        if let Some(checkpoint) = checkpoint.as_ref() {
            included_refs.push(checkpoint.summary_node_id.clone());
        }
        included_refs.extend(
            selected_summaries
                .iter()
                .map(|summary| summary.summary_node_id.clone()),
        );
        included_refs.extend(task_state.pending_artifact_ids.iter().cloned());

        let mut omitted_refs = Vec::new();
        omitted_refs.extend(
            message_bodies
                .iter()
                .take(summarized_len)
                .map(|message| message.event_id.clone()),
        );
        omitted_refs.extend(
            summary_nodes
                .iter()
                .filter(|summary| {
                    !selected_summaries
                        .iter()
                        .any(|picked| picked.summary_node_id == summary.summary_node_id)
                })
                .map(|summary| summary.summary_node_id.clone()),
        );

        let compaction_reason = if summary_nodes.is_empty() {
            "fresh_tail_only".into()
        } else {
            format!(
                "message_only_compaction:{}_messages->{}_summaries",
                summarized_len,
                summary_nodes.len()
            )
        };

        DerivedContextState {
            retrieval_query,
            runtime_blocks,
            summary_blocks,
            checkpoint_block,
            fresh_tail_block,
            summary_nodes,
            latest_checkpoint: checkpoint,
            active_task_ids: task_state.active_task_ids,
            open_blockers: task_state.open_blockers,
            pending_artifact_ids: task_state.pending_artifact_ids,
            last_safe_action_boundary: task_state.last_safe_action_boundary,
            next_recommended_action: task_state.next_recommended_action,
            assumptions: task_state.assumptions,
            risks,
            included_refs,
            omitted_refs,
            compaction_reason,
        }
    }

    fn build_message_summary_nodes(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        messages: &[MessageBodyRecord],
    ) -> Vec<SummaryNode> {
        if messages.is_empty() {
            return Vec::new();
        }

        let scope = summary_scope(session_id, task_id);
        let mut nodes = Vec::new();
        let mut leafs = Vec::new();

        for (index, chunk) in messages.chunks(LEAF_SUMMARY_CHUNK_SIZE).enumerate() {
            let summary_id = format!("summary::{scope}::leaf::{index}");
            let leaf = SummaryNode {
                meta: ObjectMeta::new(
                    summary_id.clone(),
                    self.schema.summary_schema_version.clone(),
                ),
                summary_node_id: summary_id,
                workspace_id: self.identity.workspace_id.clone(),
                session_id: session_id.map(str::to_string),
                task_id: task_id.map(str::to_string),
                depth: 0,
                summary_type: SummaryType::LeafSummary,
                content: render_message_chunk_summary("Leaf conversation summary", chunk),
                source_event_ids: chunk
                    .iter()
                    .map(|message| message.event_id.clone())
                    .collect(),
                source_artifact_ids: Vec::new(),
                earliest_at: chunk.first().map(|message| message.occurred_at),
                latest_at: chunk.last().map(|message| message.occurred_at),
                descendant_count: chunk.len() as u32,
                token_count: estimate_tokens(&render_message_chunk_summary(
                    "Leaf conversation summary",
                    chunk,
                )),
                confidence: Confidence::High,
                freshness: Freshness::Warm,
                invalidation_status: InvalidationStatus::Active,
            };
            leafs.push(leaf.clone());
            nodes.push(leaf);
        }

        if leafs.len() > 1 {
            for (index, chunk) in leafs.chunks(2).enumerate() {
                let summary_id = format!("summary::{scope}::condensed::{index}");
                let content = render_condensed_summary(chunk);
                nodes.push(SummaryNode {
                    meta: ObjectMeta::new(
                        summary_id.clone(),
                        self.schema.summary_schema_version.clone(),
                    ),
                    summary_node_id: summary_id,
                    workspace_id: self.identity.workspace_id.clone(),
                    session_id: session_id.map(str::to_string),
                    task_id: task_id.map(str::to_string),
                    depth: 1,
                    summary_type: SummaryType::CondensedSummary,
                    content: content.clone(),
                    source_event_ids: chunk
                        .iter()
                        .flat_map(|summary| summary.source_event_ids.iter().cloned())
                        .collect(),
                    source_artifact_ids: Vec::new(),
                    earliest_at: chunk.first().and_then(|summary| summary.earliest_at),
                    latest_at: chunk.last().and_then(|summary| summary.latest_at),
                    descendant_count: chunk.iter().map(|summary| summary.descendant_count).sum(),
                    token_count: estimate_tokens(&content),
                    confidence: Confidence::Medium,
                    freshness: Freshness::Stale,
                    invalidation_status: InvalidationStatus::Active,
                });
            }
        }

        nodes
    }

    fn build_checkpoint_node(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        task_state: &TaskRuntimeState,
        messages: &[MessageBodyRecord],
    ) -> Option<SummaryNode> {
        if task_state.active_task_ids.is_empty()
            && task_state.latest_goal.is_none()
            && messages.is_empty()
            && task_state.open_blockers.is_empty()
        {
            return None;
        }

        let summary_id = format!(
            "summary::{}::checkpoint::latest",
            summary_scope(session_id, task_id)
        );
        let latest_assistant_step = messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| truncate_for_summary(&message.content, 28));
        let content = render_checkpoint_content(
            task_state.latest_goal.as_deref(),
            &task_state.active_task_ids,
            latest_assistant_step.as_deref(),
            &task_state.open_blockers,
            &task_state.pending_artifact_ids,
            task_state.next_recommended_action.as_deref(),
            &task_state.assumptions,
            &task_state.risks,
        );
        let latest_at = messages.last().map(|message| message.occurred_at);

        Some(SummaryNode {
            meta: ObjectMeta::new(
                summary_id.clone(),
                self.schema.checkpoint_schema_version.clone(),
            ),
            summary_node_id: summary_id,
            workspace_id: self.identity.workspace_id.clone(),
            session_id: session_id.map(str::to_string),
            task_id: task_id.map(str::to_string),
            depth: 0,
            summary_type: SummaryType::CheckpointSummary,
            content: content.clone(),
            source_event_ids: {
                let mut refs = task_state.source_event_ids.clone();
                refs.extend(
                    messages
                        .iter()
                        .rev()
                        .take(6)
                        .map(|message| message.event_id.clone()),
                );
                refs
            },
            source_artifact_ids: task_state.pending_artifact_ids.clone(),
            earliest_at: messages.first().map(|message| message.occurred_at),
            latest_at,
            descendant_count: messages.len() as u32,
            token_count: estimate_tokens(&content),
            confidence: Confidence::High,
            freshness: Freshness::Fresh,
            invalidation_status: InvalidationStatus::Active,
        })
    }

    fn recall_memory_blocks(&self, query: &str, limit: usize) -> Result<Vec<ContextBlock>> {
        let results = self.retrieval.search_memory(
            &self.identity.memory,
            &MemorySearchRequest {
                query: query.to_string(),
                top_k: limit.max(1),
                scope: MemorySearchScope::All,
                mode: RetrievalMode::Keyword,
                include_excerpt: true,
            },
        )?;
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(index, item)| retrieved_block(index, item))
            .collect())
    }

    fn retrieve_knowledge_blocks(&self, query: &str, limit: usize) -> Result<Vec<ContextBlock>> {
        let results = self.retrieval.search_knowledge(&KnowledgeSearchRequest {
            query: query.to_string(),
            top_k: limit.max(1),
            library: None,
            libraries: Vec::new(),
            path_prefix: None,
            mode: RetrievalMode::Keyword,
            metadata_filters: None,
            include_excerpt: true,
        })?;
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(index, item)| retrieved_block(index + 100, item))
            .collect())
    }

    fn compute_token_breakdown(&self, blocks: &[ContextBlock]) -> TokenBreakdown {
        let stable_docs = token_sum(&blocks, |block| {
            matches!(
                block.kind,
                ContextBlockKind::StableIdentity
                    | ContextBlockKind::Mission
                    | ContextBlockKind::Rule
                    | ContextBlockKind::UserProfile
                    | ContextBlockKind::RuntimeDirective
            )
        });
        let runtime = token_sum(&blocks, |block| block.kind == ContextBlockKind::TaskPlan);
        let summaries = token_sum(&blocks, |block| {
            matches!(
                block.kind,
                ContextBlockKind::Summary | ContextBlockKind::Checkpoint
            )
        });
        let fresh_tail = token_sum(&blocks, |block| block.kind == ContextBlockKind::RecentEvent);
        let retrieval = token_sum(&blocks, |block| {
            matches!(
                block.kind,
                ContextBlockKind::Memory | ContextBlockKind::RetrievedKnowledge
            )
        });
        TokenBreakdown {
            total: stable_docs + runtime + summaries + fresh_tail + retrieval,
            stable_docs,
            runtime,
            summaries,
            fresh_tail,
            retrieval,
        }
    }

    fn enforce_budget(
        &self,
        mut blocks: Vec<ContextBlock>,
        budget_tokens: u32,
        base_reason: &str,
    ) -> BudgetedBlocks {
        let mut omitted_refs = Vec::new();

        if self.compute_token_breakdown(&blocks).total <= budget_tokens {
            return BudgetedBlocks {
                blocks,
                omitted_refs,
                compaction_reason: format!("{base_reason};level=1"),
            };
        }

        let mut retained = Vec::with_capacity(blocks.len());
        for block in blocks.drain(..) {
            let drop_retrieval = matches!(
                block.kind,
                ContextBlockKind::Memory | ContextBlockKind::RetrievedKnowledge
            );
            if drop_retrieval {
                omitted_refs.push(block.block_id);
            } else {
                retained.push(block);
            }
        }
        blocks = retained;

        if self.compute_token_breakdown(&blocks).total <= budget_tokens {
            return BudgetedBlocks {
                blocks,
                omitted_refs,
                compaction_reason: format!("{base_reason};level=1;retrieval_trimmed"),
            };
        }

        blocks = blocks
            .into_iter()
            .map(aggressive_compact_block)
            .collect::<Vec<_>>();
        if self.compute_token_breakdown(&blocks).total <= budget_tokens {
            return BudgetedBlocks {
                blocks,
                omitted_refs,
                compaction_reason: format!("{base_reason};level=2;aggressive"),
            };
        }

        blocks = blocks
            .into_iter()
            .map(deterministic_fallback_block)
            .collect::<Vec<_>>();

        let mut hard_trimmed = false;
        while self.compute_token_breakdown(&blocks).total > budget_tokens {
            let Some(target_idx) = blocks
                .iter()
                .enumerate()
                .max_by_key(|(_, block)| block.token_estimate.unwrap_or_default())
                .map(|(idx, _)| idx)
            else {
                break;
            };
            let compacted = deterministic_cut_once(blocks[target_idx].clone());
            if compacted.token_estimate == blocks[target_idx].token_estimate {
                let drop_idx = blocks
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, block)| block.priority)
                    .map(|(idx, _)| idx)
                    .unwrap_or(target_idx);
                let dropped = blocks.remove(drop_idx);
                omitted_refs.extend(block_omission_refs(&dropped));
                hard_trimmed = true;
                if blocks.is_empty() {
                    break;
                }
                continue;
            }
            blocks[target_idx] = compacted;
            hard_trimmed = true;
        }

        let total = self.compute_token_breakdown(&blocks).total;
        if total > budget_tokens {
            while total_exceeds_budget(&blocks, budget_tokens) && !blocks.is_empty() {
                let drop_idx = blocks
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, block)| block.priority)
                    .map(|(idx, _)| idx)
                    .expect("drop_idx exists");
                let dropped = blocks.remove(drop_idx);
                omitted_refs.extend(block_omission_refs(&dropped));
                hard_trimmed = true;
            }
        }

        BudgetedBlocks {
            blocks,
            omitted_refs,
            compaction_reason: if hard_trimmed {
                format!("{base_reason};level=3;deterministic_fallback;hard_enforced")
            } else {
                format!("{base_reason};level=3;deterministic_fallback")
            },
        }
    }
}

fn derive_task_runtime_state(
    events: &[RuntimeEvent],
    messages: &[MessageBodyRecord],
) -> TaskRuntimeState {
    let mut active_tasks = BTreeSet::new();
    let mut task_goals = BTreeMap::new();
    let mut blocker_by_task = BTreeMap::new();
    let mut pending_artifacts = BTreeSet::new();
    let mut source_event_ids = Vec::new();
    let mut latest_phase_label = None;
    let mut last_safe_action_boundary = None;
    let mut next_recommended_action = None;
    let assumptions = Vec::new();
    let mut risks = Vec::new();

    for event in events {
        match event.event_type {
            EventType::TaskStarted => {
                source_event_ids.push(event.event_id.clone());
                if let Some(task_id) = event.task_id.as_ref() {
                    active_tasks.insert(task_id.clone());
                    if let Some(goal) = event.payload.get("goal").and_then(|value| value.as_str()) {
                        task_goals.insert(task_id.clone(), goal.to_string());
                    }
                }
                latest_phase_label = Some("running".into());
            }
            EventType::TaskWaiting => {
                source_event_ids.push(event.event_id.clone());
                if let Some(task_id) = event.task_id.as_ref() {
                    active_tasks.insert(task_id.clone());
                    let reason = event
                        .payload
                        .get("reason")
                        .and_then(|value| value.as_str())
                        .unwrap_or("waiting for follow-up");
                    let hint = event
                        .payload
                        .get("resume_hint")
                        .and_then(|value| value.as_str());
                    let blocker = if let Some(hint) = hint {
                        format!("{task_id}: {reason} | resume_hint={hint}")
                    } else {
                        format!("{task_id}: {reason}")
                    };
                    blocker_by_task.insert(task_id.clone(), blocker);
                    if let Some(hint) = hint {
                        next_recommended_action = Some(hint.to_string());
                    }
                }
                latest_phase_label = Some("waiting".into());
            }
            EventType::TaskResumed => {
                source_event_ids.push(event.event_id.clone());
                if let Some(task_id) = event.task_id.as_ref() {
                    blocker_by_task.remove(task_id);
                    active_tasks.insert(task_id.clone());
                }
                latest_phase_label = Some("running".into());
            }
            EventType::TaskCheckpointed | EventType::TaskSucceeded | EventType::TurnSucceeded => {
                source_event_ids.push(event.event_id.clone());
                last_safe_action_boundary = Some(event.event_id.clone());
                if matches!(event.event_type, EventType::TaskSucceeded) {
                    if let Some(task_id) = event.task_id.as_ref() {
                        active_tasks.remove(task_id);
                        blocker_by_task.remove(task_id);
                    }
                    latest_phase_label = Some("succeeded".into());
                }
            }
            EventType::TaskFailed | EventType::TaskCancelled => {
                source_event_ids.push(event.event_id.clone());
                if let Some(task_id) = event.task_id.as_ref() {
                    active_tasks.remove(task_id);
                    blocker_by_task.remove(task_id);
                    let error = event
                        .payload
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("task failed");
                    risks.push(format!("{task_id}: {error}"));
                }
                latest_phase_label = Some("failed".into());
            }
            EventType::ArtifactCreated => {
                source_event_ids.push(event.event_id.clone());
                for artifact_id in artifact_refs_from_event(event) {
                    pending_artifacts.insert(artifact_id);
                }
            }
            EventType::SummaryInvalidated => {
                source_event_ids.push(event.event_id.clone());
                let reason = event
                    .payload
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("summary invalidated");
                risks.push(reason.to_string());
            }
            _ => {}
        }
    }

    let latest_goal = active_tasks
        .iter()
        .rev()
        .find_map(|task_id| task_goals.get(task_id).cloned())
        .or_else(|| {
            messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| truncate_for_summary(&message.content, 24))
        });

    if latest_goal.is_some() && next_recommended_action.is_none() {
        next_recommended_action = messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| truncate_for_summary(&message.content, 28));
    }

    TaskRuntimeState {
        active_task_ids: active_tasks.into_iter().collect(),
        latest_goal,
        latest_phase_label,
        open_blockers: blocker_by_task.into_values().collect(),
        pending_artifact_ids: pending_artifacts.into_iter().collect(),
        source_event_ids,
        last_safe_action_boundary,
        next_recommended_action,
        assumptions,
        risks,
    }
}

fn build_runtime_blocks(
    task_state: &TaskRuntimeState,
    purpose: &ContextAssemblyPurpose,
) -> Vec<ContextBlock> {
    let mut lines = vec![format!("purpose: {}", purpose_label(purpose))];

    if let Some(goal) = task_state.latest_goal.as_deref() {
        lines.push(format!("current_goal: {goal}"));
    }
    if let Some(phase) = task_state.latest_phase_label.as_deref() {
        lines.push(format!("task_phase: {phase}"));
    }
    if !task_state.active_task_ids.is_empty() {
        lines.push(format!(
            "active_tasks: {}",
            task_state.active_task_ids.join(", ")
        ));
    }
    if !task_state.open_blockers.is_empty() {
        lines.push(format!(
            "open_blockers: {}",
            task_state.open_blockers.join(" || ")
        ));
    }
    if !task_state.pending_artifact_ids.is_empty() {
        lines.push(format!(
            "live_artifacts: {}",
            task_state.pending_artifact_ids.join(", ")
        ));
    }
    if let Some(next_action) = task_state.next_recommended_action.as_deref() {
        lines.push(format!("next_recommended_action: {next_action}"));
    }

    if lines.len() == 1 {
        return Vec::new();
    }

    vec![ContextBlock {
        block_id: "runtime.task_state".into(),
        kind: ContextBlockKind::TaskPlan,
        source: ContextSource::Runtime,
        priority: 90,
        token_estimate: Some(estimate_tokens(&lines.join("\n"))),
        freshness: Some(Freshness::Fresh),
        confidence: Some(Confidence::High),
        content: lines.join("\n"),
    }]
}

fn build_fresh_tail_block(messages: &[MessageBodyRecord]) -> Option<ContextBlock> {
    if messages.is_empty() {
        return None;
    }

    let content = messages
        .iter()
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n");
    let latest_event_id = messages
        .last()
        .map(|message| message.event_id.clone())
        .unwrap_or_else(|| "recent-transcript".into());

    Some(ContextBlock {
        block_id: "transcript.recent".into(),
        kind: ContextBlockKind::RecentEvent,
        source: ContextSource::EventLog {
            event_id: latest_event_id,
        },
        priority: 100,
        token_estimate: Some(estimate_tokens(&content)),
        freshness: Some(Freshness::Fresh),
        confidence: Some(Confidence::High),
        content,
    })
}

fn summary_block_from_node(summary: &SummaryNode) -> ContextBlock {
    ContextBlock {
        block_id: format!("context.{}", summary.summary_node_id),
        kind: ContextBlockKind::Summary,
        source: ContextSource::Summary {
            summary_node_id: summary.summary_node_id.clone(),
        },
        priority: 80_u32.saturating_sub(summary.depth * 5),
        token_estimate: Some(summary.token_count),
        freshness: Some(summary.freshness.clone()),
        confidence: Some(summary.confidence.clone()),
        content: summary.content.clone(),
    }
}

fn checkpoint_block_from_node(summary: &SummaryNode) -> ContextBlock {
    ContextBlock {
        block_id: format!("context.{}", summary.summary_node_id),
        kind: ContextBlockKind::Checkpoint,
        source: ContextSource::Summary {
            summary_node_id: summary.summary_node_id.clone(),
        },
        priority: 95,
        token_estimate: Some(summary.token_count),
        freshness: Some(summary.freshness.clone()),
        confidence: Some(summary.confidence.clone()),
        content: summary.content.clone(),
    }
}

fn select_summary_nodes(summary_nodes: &[SummaryNode]) -> Vec<SummaryNode> {
    let condensed = summary_nodes
        .iter()
        .filter(|summary| summary.summary_type == SummaryType::CondensedSummary)
        .cloned()
        .collect::<Vec<_>>();
    if !condensed.is_empty() {
        return condensed
            .into_iter()
            .rev()
            .take(SELECTED_SUMMARY_LIMIT)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }

    summary_nodes
        .iter()
        .filter(|summary| summary.summary_type == SummaryType::LeafSummary)
        .cloned()
        .rev()
        .take(SELECTED_SUMMARY_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn apply_summary_status_events(
    events: &[RuntimeEvent],
    summary_nodes: &mut [SummaryNode],
    checkpoint: Option<&mut SummaryNode>,
    risks: &mut Vec<String>,
) {
    let mut summary_index = summary_nodes
        .iter_mut()
        .map(|summary| (summary.summary_node_id.clone(), summary))
        .collect::<BTreeMap<_, _>>();
    let mut checkpoint = checkpoint;

    for event in events {
        match event.event_type {
            EventType::SummaryInvalidated => {
                let status = invalidation_status_from_payload(event);
                let reason = event
                    .payload
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("summary invalidated");
                for summary_id in summary_targets_from_payload(event) {
                    if let Some(summary) = summary_index.get_mut(&summary_id) {
                        summary.invalidation_status = status.clone();
                        summary.freshness = Freshness::Stale;
                        risks.push(format!("{summary_id}: {reason}"));
                    }
                    if checkpoint
                        .as_ref()
                        .map(|current| current.summary_node_id.as_str() == summary_id)
                        .unwrap_or(false)
                    {
                        if let Some(current) = checkpoint.as_mut() {
                            current.invalidation_status = status.clone();
                            current.freshness = Freshness::Stale;
                        }
                    }
                }
            }
            EventType::SummaryRecomputed => {
                for summary_id in summary_targets_from_payload(event) {
                    if let Some(summary) = summary_index.get_mut(&summary_id) {
                        summary.invalidation_status = InvalidationStatus::Active;
                        summary.freshness = Freshness::Fresh;
                    }
                    if checkpoint
                        .as_ref()
                        .map(|current| current.summary_node_id.as_str() == summary_id)
                        .unwrap_or(false)
                    {
                        if let Some(current) = checkpoint.as_mut() {
                            current.invalidation_status = InvalidationStatus::Active;
                            current.freshness = Freshness::Fresh;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn summary_targets_from_payload(event: &RuntimeEvent) -> Vec<String> {
    let mut targets = Vec::new();
    for key in ["summary_node_id", "summary_id"] {
        if let Some(value) = event.payload.get(key).and_then(|value| value.as_str()) {
            targets.push(value.to_string());
        }
    }
    for key in ["summary_node_ids", "summary_ids"] {
        if let Some(values) = event.payload.get(key).and_then(|value| value.as_array()) {
            targets.extend(
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string)),
            );
        }
    }
    targets
}

fn invalidation_status_from_payload(event: &RuntimeEvent) -> InvalidationStatus {
    match event
        .payload
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("invalidated")
    {
        "stale" => InvalidationStatus::Stale,
        "contradicted" => InvalidationStatus::Contradicted,
        "active" => InvalidationStatus::Active,
        _ => InvalidationStatus::Invalidated,
    }
}

fn artifact_refs_from_event(event: &RuntimeEvent) -> Vec<String> {
    let mut refs = Vec::new();
    for key in ["artifact_id", "artifact_ref", "id"] {
        if let Some(value) = event.payload.get(key).and_then(|value| value.as_str()) {
            refs.push(value.to_string());
        }
    }
    refs
}

fn render_message_chunk_summary(title: &str, messages: &[MessageBodyRecord]) -> String {
    let lines = messages
        .iter()
        .map(|message| {
            format!(
                "- {}: {}",
                message.role,
                truncate_for_summary(&message.content, 18)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{title}\n{lines}")
}

fn render_condensed_summary(summaries: &[SummaryNode]) -> String {
    let lines = summaries
        .iter()
        .map(|summary| {
            let label = match summary.summary_type {
                SummaryType::LeafSummary => "leaf",
                SummaryType::CondensedSummary => "condensed",
                SummaryType::ArtifactRefSummary => "artifact",
                SummaryType::CheckpointSummary => "checkpoint",
            };
            format!("- {label}: {}", truncate_for_summary(&summary.content, 26))
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Condensed conversation summary\n{lines}")
}

fn render_checkpoint_content(
    goal: Option<&str>,
    active_task_ids: &[String],
    latest_successful_step: Option<&str>,
    blockers: &[String],
    artifact_ids: &[String],
    next_action: Option<&str>,
    assumptions: &[String],
    risks: &[String],
) -> String {
    let mut lines = vec!["Checkpoint summary".to_string()];

    if let Some(goal) = goal {
        lines.push(format!("current_goal: {goal}"));
    }
    if !active_task_ids.is_empty() {
        lines.push(format!("active_tasks: {}", active_task_ids.join(", ")));
    }
    if let Some(step) = latest_successful_step {
        lines.push(format!("latest_successful_step: {step}"));
    }
    if !blockers.is_empty() {
        lines.push(format!("pending_blockers: {}", blockers.join(" || ")));
    }
    if !artifact_ids.is_empty() {
        lines.push(format!("important_live_refs: {}", artifact_ids.join(", ")));
    }
    if let Some(next_action) = next_action {
        lines.push(format!("next_recommended_action: {next_action}"));
    }
    if !assumptions.is_empty() {
        lines.push(format!("assumptions: {}", assumptions.join(" || ")));
    }
    if !risks.is_empty() {
        lines.push(format!("risks: {}", risks.join(" || ")));
    }

    lines.join("\n")
}

fn workspace_block(
    block_id: &str,
    kind: ContextBlockKind,
    path: &str,
    content: &str,
) -> ContextBlock {
    ContextBlock {
        block_id: block_id.into(),
        kind,
        source: ContextSource::WorkspaceFile { path: path.into() },
        priority: 10,
        token_estimate: Some(estimate_tokens(content)),
        freshness: None,
        confidence: None,
        content: content.into(),
    }
}

fn projection_id(
    session_id: Option<&str>,
    task_id: Option<&str>,
    purpose: &ContextAssemblyPurpose,
) -> String {
    format!(
        "projection::{}::{}::{}",
        session_id.unwrap_or("workspace"),
        task_id.unwrap_or("all"),
        purpose_label(purpose)
    )
}

fn summary_scope(session_id: Option<&str>, task_id: Option<&str>) -> String {
    format!(
        "{}::{}",
        session_id.unwrap_or("workspace"),
        task_id.unwrap_or("all")
    )
}

fn purpose_label(purpose: &ContextAssemblyPurpose) -> &'static str {
    match purpose {
        ContextAssemblyPurpose::Chat => "chat",
        ContextAssemblyPurpose::Planning => "planning",
        ContextAssemblyPurpose::Execution => "execution",
        ContextAssemblyPurpose::Summarization => "summarization",
        ContextAssemblyPurpose::Resume => "resume",
    }
}

fn fresh_tail_limit(purpose: &ContextAssemblyPurpose) -> usize {
    match purpose {
        ContextAssemblyPurpose::Chat => 6,
        ContextAssemblyPurpose::Planning => 4,
        ContextAssemblyPurpose::Execution => 4,
        ContextAssemblyPurpose::Summarization => 2,
        ContextAssemblyPurpose::Resume => 3,
    }
}

fn token_sum<F>(blocks: &[ContextBlock], predicate: F) -> u32
where
    F: Fn(&ContextBlock) -> bool,
{
    blocks
        .iter()
        .filter(|block| predicate(block))
        .map(|block| block.token_estimate.unwrap_or_default())
        .sum()
}

fn estimate_tokens(content: &str) -> u32 {
    content.split_whitespace().count() as u32
}

fn truncate_for_summary(content: &str, max_words: usize) -> String {
    let words = content.split_whitespace().collect::<Vec<_>>();
    if words.len() <= max_words {
        return words.join(" ");
    }
    format!("{} ...", words[..max_words].join(" "))
}

fn retrieved_block(index: usize, item: RetrievalDocument) -> ContextBlock {
    let (kind, source, freshness, confidence) = match item.kind {
        RetrievalDocumentKind::Memory => (
            ContextBlockKind::Memory,
            ContextSource::Memory {
                memory_ref: item.stable_ref.clone(),
            },
            Some(Freshness::Warm),
            Some(Confidence::High),
        ),
        RetrievalDocumentKind::Knowledge => (
            ContextBlockKind::RetrievedKnowledge,
            ContextSource::Knowledge {
                knowledge_ref: item.stable_ref.clone(),
            },
            Some(Freshness::Fresh),
            Some(Confidence::Medium),
        ),
    };

    ContextBlock {
        block_id: format!("retrieval.{index}"),
        kind,
        source,
        priority: 40 + item.score,
        token_estimate: Some(estimate_tokens(&item.excerpt)),
        freshness,
        confidence,
        content: format!("{}:\n{}", item.title, item.excerpt),
    }
}

fn aggressive_compact_block(mut block: ContextBlock) -> ContextBlock {
    match block.kind {
        ContextBlockKind::Summary => {
            block.content = truncate_for_summary(&block.content, SUMMARY_AGGRESSIVE_WORDS);
            block.token_estimate = Some(estimate_tokens(&block.content));
        }
        ContextBlockKind::RecentEvent => {
            let lines = block
                .content
                .lines()
                .rev()
                .take(FRESH_TAIL_AGGRESSIVE_LINES)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>();
            block.content = lines.join("\n");
            block.token_estimate = Some(estimate_tokens(&block.content));
        }
        _ => {}
    }
    block
}

fn deterministic_fallback_block(mut block: ContextBlock) -> ContextBlock {
    match block.kind {
        ContextBlockKind::Summary => {
            block.content = truncate_for_summary(&block.content, SUMMARY_FALLBACK_WORDS);
            block.token_estimate = Some(estimate_tokens(&block.content));
        }
        ContextBlockKind::RecentEvent => {
            let lines = block
                .content
                .lines()
                .rev()
                .take(FRESH_TAIL_FALLBACK_LINES)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>();
            block.content = lines.join("\n");
            block.token_estimate = Some(estimate_tokens(&block.content));
        }
        _ => {}
    }
    block
}

fn deterministic_cut_once(mut block: ContextBlock) -> ContextBlock {
    let words = block.content.split_whitespace().collect::<Vec<_>>();
    if words.len() <= 6 {
        return block;
    }
    let cut = (words.len() / 2).max(6);
    block.content = format!("{} ...", words[..cut].join(" "));
    block.token_estimate = Some(estimate_tokens(&block.content));
    block
}

fn block_omission_refs(block: &ContextBlock) -> Vec<String> {
    let mut refs = vec![block.block_id.clone()];
    match &block.source {
        ContextSource::WorkspaceFile { path } => refs.push(path.clone()),
        ContextSource::EventLog { event_id } => refs.push(event_id.clone()),
        ContextSource::Summary { summary_node_id } => refs.push(summary_node_id.clone()),
        ContextSource::Memory { memory_ref } => refs.push(memory_ref.clone()),
        ContextSource::Knowledge { knowledge_ref } => refs.push(knowledge_ref.clone()),
        ContextSource::ToolTrace { tool_call_id } => refs.push(tool_call_id.clone()),
        ContextSource::Artifact { artifact_id } => refs.push(artifact_id.clone()),
        ContextSource::Runtime => {}
    }
    refs
}

fn total_exceeds_budget(blocks: &[ContextBlock], budget_tokens: u32) -> bool {
    blocks
        .iter()
        .map(|block| block.token_estimate.unwrap_or_default())
        .sum::<u32>()
        > budget_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use serde_json::json;

    use crate::{
        builtin::context::retrieval_types::RetrievalScope,
        config::WorkspaceDocument,
        domain::{ContextAssemblyPurpose, EventSource, EventType, RuntimeEvent},
    };

    #[test]
    fn assemble_context_stays_within_budget() {
        let engine = sample_engine();
        for idx in 0..16 {
            let role = if idx % 2 == 0 { "user" } else { "assistant" };
            let event_type = if role == "user" {
                EventType::MessageReceived
            } else {
                EventType::TurnSucceeded
            };
            let payload = if role == "user" {
                json!({
                    "message": { "role": role, "content": format!("request {idx} with enough words to force compaction in budget test") }
                })
            } else {
                json!({
                    "assistant_message": { "role": role, "content": format!("response {idx} with enough words to force compaction in budget test") }
                })
            };
            engine
                .append_event(RuntimeEvent {
                    event_id: format!("evt_{idx}"),
                    event_type,
                    occurred_at: Utc::now(),
                    workspace_id: Some("ws.test".into()),
                    agent_id: Some("agent.test".into()),
                    session_id: Some("session.test".into()),
                    turn_id: Some(format!("turn_{idx}")),
                    task_id: Some("task.test".into()),
                    plugin_id: None,
                    node_id: None,
                    source: EventSource::Agent,
                    causation_id: None,
                    correlation_id: None,
                    idempotency_key: None,
                    payload,
                    schema_version: "event.v1".into(),
                })
                .expect("append should succeed");
        }

        let assembled = engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some("session.test".into()),
                task_id: Some("task.test".into()),
                budget_tokens: 120,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
                retrieval_scope: RetrievalScope::Disabled,
            })
            .expect("assemble should succeed");
        assert!(assembled.token_breakdown.total <= 120);
        assert!(assembled
            .system_prompt_additions
            .iter()
            .any(|line| line.contains("lcm_compaction")));
    }

    #[test]
    fn expand_summary_returns_source_messages() {
        let engine = sample_engine();
        let events = vec![
            RuntimeEvent {
                event_id: "evt_user".into(),
                event_type: EventType::MessageReceived,
                occurred_at: Utc::now(),
                workspace_id: Some("ws.test".into()),
                agent_id: Some("agent.test".into()),
                session_id: Some("session.test".into()),
                turn_id: Some("turn_1".into()),
                task_id: Some("task.test".into()),
                plugin_id: None,
                node_id: None,
                source: EventSource::User,
                causation_id: None,
                correlation_id: None,
                idempotency_key: None,
                payload: json!({"message": {"role": "user", "content": "please remember alpha and beta details"}}),
                schema_version: "event.v1".into(),
            },
            RuntimeEvent {
                event_id: "evt_assistant".into(),
                event_type: EventType::TurnSucceeded,
                occurred_at: Utc::now(),
                workspace_id: Some("ws.test".into()),
                agent_id: Some("agent.test".into()),
                session_id: Some("session.test".into()),
                turn_id: Some("turn_1".into()),
                task_id: Some("task.test".into()),
                plugin_id: None,
                node_id: None,
                source: EventSource::Agent,
                causation_id: None,
                correlation_id: None,
                idempotency_key: None,
                payload: json!({"assistant_message": {"role": "assistant", "content": "confirmed alpha and beta details"}}),
                schema_version: "event.v1".into(),
            },
        ];
        for event in events {
            engine.append_event(event).expect("append should succeed");
        }

        let summary_id = "summary::session.test::task.test::leaf::0";
        let expansion = engine
            .expand_summary(Some("session.test"), Some("task.test"), summary_id)
            .expect("expand should succeed");
        assert!(expansion
            .matched_refs
            .iter()
            .any(|event_id| event_id == "evt_user"));
        assert!(expansion.distilled_text.contains("alpha"));
    }

    fn sample_engine() -> WorkspaceContextEngine {
        let root = std::env::temp_dir().join(format!(
            "agentjax-context-engine-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let workspace_root = root.join("workspace");
        let state_root = root.join("runtime").join("state");
        let _ = std::fs::create_dir_all(&workspace_root);
        let _ = std::fs::create_dir_all(&state_root);
        let make_doc = |name: &str, content: &str| WorkspaceDocument {
            path: workspace_root.join(name),
            content: content.into(),
        };
        WorkspaceContextEngine::new(
            WorkspaceIdentityPack {
                workspace_id: "ws.test".into(),
                agent: make_doc("AGENT.md", "agent"),
                soul: make_doc("SOUL.md", "soul"),
                user: make_doc("USER.md", "user"),
                memory: make_doc("MEMORY.md", "memory"),
                mission: make_doc("MISSION.md", "mission"),
                rules: make_doc("RULES.md", "rules"),
                router: make_doc("ROUTER.md", "router"),
            },
            WorkspacePaths::new(workspace_root),
            state_root,
        )
    }
}
