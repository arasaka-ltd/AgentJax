use serde::{Deserialize, Serialize};

use crate::domain::{ObjectMeta, ResumePack};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Ready,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskPhase {
    Created,
    Ready,
    Scheduled,
    Leased,
    Running,
    Waiting,
    Checkpointed,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub meta: ObjectMeta,
    pub task_id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub definition_ref: Option<String>,
    pub execution_mode: crate::domain::ExecutionMode,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub goal: String,
    pub checkpoint_ref: Option<String>,
    pub waiting_until: Option<chrono::DateTime<chrono::Utc>>,
    pub waiting_reason: Option<String>,
    pub waiting_resume_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskTimelineEntry {
    pub entry_id: String,
    pub task_id: String,
    pub phase: TaskPhase,
    pub status: TaskStatus,
    pub turn_id: Option<String>,
    pub event_id: Option<String>,
    pub note: String,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskCheckpoint {
    pub checkpoint_id: String,
    pub task_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub summary: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub resume_pack: ResumePack,
}
