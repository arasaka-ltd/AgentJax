use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionMode {
    Interactive,
    BackgroundBound,
    Imported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Idle,
    Closed,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub meta: ObjectMeta,
    pub session_id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub channel_id: Option<String>,
    pub surface_id: Option<String>,
    pub user_id: Option<String>,
    pub title: Option<String>,
    pub mode: SessionMode,
    pub status: SessionStatus,
    pub last_turn_id: Option<String>,
}
