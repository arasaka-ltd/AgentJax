use serde::{Deserialize, Serialize};
use crate::domain::{Confidence, Freshness, ObjectMeta};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SummaryType {
    LeafSummary,
    CondensedSummary,
    ArtifactRefSummary,
    CheckpointSummary,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SummaryStatus {
    Fresh,
    Stale,
    Contradicted,
    Invalidated,
    Recomputing,
    Archived,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InvalidationStatus {
    Active,
    Stale,
    Contradicted,
    Invalidated,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SummaryNode {
    pub meta: ObjectMeta,
    pub summary_node_id: String,
    pub workspace_id: String,
    pub summary_type: SummaryType,
    pub content: String,
    pub source_event_ids: Vec<String>,
    pub source_artifact_ids: Vec<String>,
    pub confidence: Confidence,
    pub freshness: Freshness,
    pub invalidation_status: InvalidationStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumePack {
    pub mission_ref: Option<String>,
    pub active_task_ids: Vec<String>,
    pub latest_checkpoint_summary_id: Option<String>,
    pub summary_node_ids: Vec<String>,
    pub open_blockers: Vec<String>,
    pub pending_artifact_ids: Vec<String>,
    pub last_safe_action_boundary: Option<String>,
}
