use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Confidence {
    Low,
    Medium,
    High,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Warm,
    Stale,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextSource {
    WorkspaceFile { path: String },
    EventLog { event_id: String },
    Summary { summary_node_id: String },
    Memory { memory_ref: String },
    ToolTrace { tool_call_id: String },
    Artifact { artifact_id: String },
    Runtime,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextBlockKind {
    StableIdentity,
    Mission,
    Rule,
    UserProfile,
    Memory,
    RetrievedKnowledge,
    RecentEvent,
    ToolTrace,
    TaskPlan,
    Summary,
    SkillInstruction,
    Checkpoint,
    RuntimeDirective,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextBlock {
    pub block_id: String,
    pub kind: ContextBlockKind,
    pub source: ContextSource,
    pub priority: u32,
    pub token_estimate: Option<u32>,
    pub freshness: Option<Freshness>,
    pub confidence: Option<Confidence>,
    pub content: String,
}
