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

        if matches!(request.retrieval_scope, RetrievalScope::Implicit) {
            let memory_blocks =
                self.recall_memory_blocks(&retrieval_query, request.budget_tokens as usize / 100)?;
            let knowledge_blocks = self.retrieve_knowledge_blocks(
                &retrieval_query,
                request.budget_tokens as usize / 150,
            )?;
            blocks.extend(memory_blocks);
            blocks.extend(knowledge_blocks);
        }

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
                "workspace_id={}, purpose={:?}, retrieval_scope={:?}",
                self.identity.workspace_id, request.purpose, request.retrieval_scope
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
        block_id: format!("retrieval.{}", index),
        kind,
        source,
        priority: 40 + item.score,
        token_estimate: Some(estimate_tokens(&item.excerpt)),
        freshness,
        confidence,
        content: format!("{}:\n{}", item.title, item.excerpt),
    }
}
