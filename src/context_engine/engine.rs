use crate::builtin::context::retrieval_bridge::{
    RetrievalBridgeContextPlugin, RetrievalCollectionKind, RetrievedDocument,
};
use crate::config::{WorkspaceIdentityPack, WorkspacePaths};
use crate::context_engine::{
    assembler::{AssembledContext, ContextAssemblyRequest, TokenBreakdown},
    event_store::EventStore,
    projection_store::ProjectionStore,
};
use crate::domain::{
    Confidence, ContextBlock, ContextBlockKind, ContextSource, Freshness, ResumePack, RuntimeEvent,
};
use anyhow::Result;
pub trait ContextEngine: Send + Sync {
    fn append_event(&self, event: RuntimeEvent) -> Result<()>;
    fn assemble_context(&self, request: ContextAssemblyRequest) -> Result<AssembledContext>;
    fn build_resume_pack(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<ResumePack>;
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
}

#[derive(Debug, Clone)]
pub struct WorkspaceContextEngine {
    identity: WorkspaceIdentityPack,
    retrieval: RetrievalBridgeContextPlugin,
    events: EventStore,
    projections: ProjectionStore,
}

impl WorkspaceContextEngine {
    pub fn new(identity: WorkspaceIdentityPack, workspace_paths: WorkspacePaths) -> Self {
        Self {
            identity,
            retrieval: RetrievalBridgeContextPlugin::new(&workspace_paths),
            events: EventStore::default(),
            projections: ProjectionStore::default(),
        }
    }
}

impl ContextEngine for WorkspaceContextEngine {
    fn append_event(&self, event: RuntimeEvent) -> Result<()> {
        self.events.append(event);
        Ok(())
    }

    fn assemble_context(&self, request: ContextAssemblyRequest) -> Result<AssembledContext> {
        let transcript = self
            .events
            .recent_transcript(request.session_id.as_deref(), 8);
        let retrieval_query = transcript
            .lines()
            .last()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .unwrap_or_default()
            .to_string();

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

        let memory_blocks =
            self.recall_memory_blocks(&retrieval_query, request.budget_tokens as usize / 100)?;
        let knowledge_blocks =
            self.retrieve_knowledge_blocks(&retrieval_query, request.budget_tokens as usize / 150)?;
        blocks.extend(memory_blocks);
        blocks.extend(knowledge_blocks);

        if !transcript.is_empty() {
            blocks.push(ContextBlock {
                block_id: "transcript.recent".into(),
                kind: ContextBlockKind::RecentEvent,
                source: ContextSource::EventLog {
                    event_id: "recent-transcript".into(),
                },
                priority: 100,
                token_estimate: Some(estimate_tokens(&transcript)),
                freshness: None,
                confidence: None,
                content: transcript,
            });
        }

        self.projections.replace(blocks.clone());

        let stable_docs = blocks
            .iter()
            .filter(|block| {
                matches!(
                    block.kind,
                    ContextBlockKind::StableIdentity
                        | ContextBlockKind::Mission
                        | ContextBlockKind::Rule
                        | ContextBlockKind::UserProfile
                        | ContextBlockKind::RuntimeDirective
                )
            })
            .map(|block| block.token_estimate.unwrap_or_default())
            .sum();
        let retrieval = blocks
            .iter()
            .filter(|block| {
                matches!(
                    block.kind,
                    ContextBlockKind::Memory | ContextBlockKind::RetrievedKnowledge
                )
            })
            .map(|block| block.token_estimate.unwrap_or_default())
            .sum();
        let fresh_tail = blocks
            .iter()
            .filter(|block| block.kind == ContextBlockKind::RecentEvent)
            .map(|block| block.token_estimate.unwrap_or_default())
            .sum();

        let included_refs = self
            .identity
            .source_paths()
            .into_iter()
            .chain(blocks.iter().filter_map(|block| match &block.source {
                ContextSource::WorkspaceFile { path } => Some(path.clone()),
                ContextSource::Memory { memory_ref } => Some(memory_ref.clone()),
                ContextSource::Knowledge { knowledge_ref } => Some(knowledge_ref.clone()),
                _ => None,
            }))
            .collect();

        Ok(AssembledContext {
            blocks,
            token_breakdown: TokenBreakdown {
                total: stable_docs + retrieval + fresh_tail,
                stable_docs,
                runtime: 0,
                summaries: 0,
                fresh_tail,
                retrieval,
            },
            included_refs,
            omitted_refs: Vec::new(),
            system_prompt_additions: vec![format!(
                "workspace_id={}, purpose={:?}",
                self.identity.workspace_id, request.purpose
            )],
        })
    }

    fn build_resume_pack(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<ResumePack> {
        Ok(ResumePack {
            workspace_id: Some(self.identity.workspace_id.clone()),
            session_id: session_id.map(str::to_string),
            task_id: task_id.map(str::to_string),
            mission_ref: Some(self.identity.mission.path.display().to_string()),
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
}

impl WorkspaceContextEngine {
    fn recall_memory_blocks(&self, query: &str, limit: usize) -> Result<Vec<ContextBlock>> {
        let results = self
            .retrieval
            .recall_memory(&self.identity.memory, query, limit.max(1))?;
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(index, item)| retrieved_block(index, item))
            .collect())
    }

    fn retrieve_knowledge_blocks(&self, query: &str, limit: usize) -> Result<Vec<ContextBlock>> {
        let results = self.retrieval.retrieve_knowledge(query, limit.max(1))?;
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(index, item)| retrieved_block(index + 100, item))
            .collect())
    }
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

fn estimate_tokens(content: &str) -> u32 {
    content.split_whitespace().count() as u32
}

fn retrieved_block(index: usize, item: RetrievedDocument) -> ContextBlock {
    let (kind, source, freshness, confidence) = match item.kind {
        RetrievalCollectionKind::Memory => (
            ContextBlockKind::Memory,
            ContextSource::Memory {
                memory_ref: item.document_ref.clone(),
            },
            Some(Freshness::Warm),
            Some(Confidence::High),
        ),
        RetrievalCollectionKind::Knowledge => (
            ContextBlockKind::RetrievedKnowledge,
            ContextSource::Knowledge {
                knowledge_ref: item.document_ref.clone(),
            },
            Some(Freshness::Fresh),
            Some(Confidence::Medium),
        ),
    };

    ContextBlock {
        block_id: format!("retrieval.{}.{}", item.collection_id, index),
        kind,
        source,
        priority: 40 + item.score,
        token_estimate: Some(estimate_tokens(&item.content)),
        freshness,
        confidence,
        content: format!("{}:\n{}", item.title, item.content),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{ContextEngine, WorkspaceContextEngine};
    use crate::{
        config::{WorkspaceDocument, WorkspaceIdentityPack, WorkspacePaths},
        context_engine::ContextAssemblyRequest,
        domain::{ContextAssemblyPurpose, ContextBlockKind, EventSource, EventType, RuntimeEvent},
    };

    #[test]
    fn assembles_memory_recall_knowledge_retrieval_and_recent_transcript_blocks() {
        let root = temp_path("context-engine");
        let paths = WorkspacePaths::new(&root);
        fs::create_dir_all(&paths.memory_topics_dir).unwrap();
        fs::create_dir_all(&paths.knowledge_dir).unwrap();
        fs::write(
            paths.memory_topics_dir.join("project-alpha.md"),
            "Project Alpha uses Rust heavily for automation.",
        )
        .unwrap();
        fs::write(
            paths.knowledge_dir.join("api.md"),
            "Health endpoint documentation lives at /health and /ready.",
        )
        .unwrap();

        let engine = WorkspaceContextEngine::new(
            WorkspaceIdentityPack {
                workspace_id: "ws-test".into(),
                agent: doc("AGENT.md", "agent identity"),
                soul: doc("SOUL.md", "calm and direct"),
                user: doc("USER.md", "prefers concise answers"),
                memory: WorkspaceDocument {
                    path: paths.memory_file.clone(),
                    content: "Stable fact: Project Alpha prefers Rust.".into(),
                },
                mission: doc("MISSION.md", "ship useful agents"),
                rules: doc("RULES.md", "do not guess"),
                router: doc("ROUTER.md", "use memory when relevant"),
            },
            paths.clone(),
        );

        engine
            .append_event(RuntimeEvent {
                event_id: "evt_1".into(),
                event_type: EventType::MessageReceived,
                occurred_at: Utc::now(),
                workspace_id: Some("ws-test".into()),
                agent_id: Some("default-agent".into()),
                session_id: Some("session.default".into()),
                turn_id: Some("turn_1".into()),
                task_id: None,
                plugin_id: None,
                node_id: None,
                source: EventSource::Operator,
                causation_id: None,
                correlation_id: None,
                idempotency_key: None,
                payload: json!({
                    "message": {
                        "role": "user",
                        "content": "where is the health endpoint and does project alpha use rust"
                    }
                }),
                schema_version: "event.v1".into(),
            })
            .expect("append user event");

        let assembled = engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some("session.default".into()),
                task_id: None,
                budget_tokens: 8000,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
            })
            .expect("assemble context");

        assert!(assembled
            .blocks
            .iter()
            .any(|block| block.block_id == "workspace.agent"));
        assert!(assembled
            .blocks
            .iter()
            .any(|block| block.kind == ContextBlockKind::Memory));
        assert!(assembled
            .blocks
            .iter()
            .any(|block| block.kind == ContextBlockKind::RetrievedKnowledge));
        assert!(assembled
            .blocks
            .iter()
            .any(|block| block.block_id == "transcript.recent"));
        assert!(assembled.token_breakdown.total > 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_resume_pack_preserves_workspace_session_and_task_refs() {
        let root = temp_path("resume-pack");
        let paths = WorkspacePaths::new(&root);
        fs::create_dir_all(&paths.root).unwrap();

        let engine = WorkspaceContextEngine::new(
            WorkspaceIdentityPack {
                workspace_id: "ws-test".into(),
                agent: doc("AGENT.md", "agent identity"),
                soul: doc("SOUL.md", "calm and direct"),
                user: doc("USER.md", "prefers concise answers"),
                memory: doc("MEMORY.md", "Stable fact: Project Alpha prefers Rust."),
                mission: doc("MISSION.md", "ship useful agents"),
                rules: doc("RULES.md", "do not guess"),
                router: doc("ROUTER.md", "use memory when relevant"),
            },
            paths,
        );

        let resume = engine
            .build_resume_pack(Some("session.default"), Some("task_42"))
            .expect("build resume pack");

        assert_eq!(resume.workspace_id.as_deref(), Some("ws-test"));
        assert_eq!(resume.session_id.as_deref(), Some("session.default"));
        assert_eq!(resume.task_id.as_deref(), Some("task_42"));
        assert!(resume
            .mission_ref
            .as_deref()
            .is_some_and(|path| path.ends_with("MISSION.md")));

        let _ = fs::remove_dir_all(root);
    }

    fn doc(path: &str, content: &str) -> WorkspaceDocument {
        WorkspaceDocument {
            path: std::path::PathBuf::from(path),
            content: content.into(),
        }
    }

    fn temp_path(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("agentjax-{prefix}-{nanos}"))
    }
}
