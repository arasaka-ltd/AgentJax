use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnStatus {
    Started,
    ContextBuilt,
    Running,
    WaitingTool,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnPhase {
    Accepted,
    Started,
    BuildingContext,
    CallingModel,
    ExecutingTool,
    FinalizingOutput,
    CommittingMemory,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSummary {
    pub output_text: Option<String>,
    pub tool_call_count: u32,
    pub artifact_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Turn {
    pub meta: ObjectMeta,
    pub turn_id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_id: String,
    pub input_event_id: String,
    pub status: TurnStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub summary: Option<TurnSummary>,
}
