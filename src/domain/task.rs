use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

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
}
