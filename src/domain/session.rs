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
pub struct SessionModelTarget {
    pub provider_id: String,
    pub model_id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_model_switch: Option<SessionModelTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_model_switched_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Session {
    pub fn resolved_provider_id<'a>(&'a self, default_provider_id: &'a str) -> &'a str {
        self.current_provider_id
            .as_deref()
            .unwrap_or(default_provider_id)
    }

    pub fn resolved_model_id<'a>(&'a self, default_model_id: &'a str) -> &'a str {
        self.current_model_id.as_deref().unwrap_or(default_model_id)
    }
}
