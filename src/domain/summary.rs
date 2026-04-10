use crate::domain::{Confidence, Freshness, ObjectMeta};
use serde::{Deserialize, Serialize};
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
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub depth: u32,
    pub summary_type: SummaryType,
    pub content: String,
    pub source_event_ids: Vec<String>,
    pub source_artifact_ids: Vec<String>,
    pub earliest_at: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_at: Option<chrono::DateTime<chrono::Utc>>,
    pub descendant_count: u32,
    pub token_count: u32,
    pub confidence: Confidence,
    pub freshness: Freshness,
    pub invalidation_status: InvalidationStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumePack {
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub mission_ref: Option<String>,
    pub active_task_ids: Vec<String>,
    pub latest_checkpoint_summary_id: Option<String>,
    pub summary_node_ids: Vec<String>,
    pub open_blockers: Vec<String>,
    pub pending_artifact_ids: Vec<String>,
    pub last_safe_action_boundary: Option<String>,
    pub next_recommended_action: Option<String>,
    pub assumptions: Vec<String>,
    pub risks: Vec<String>,
}
