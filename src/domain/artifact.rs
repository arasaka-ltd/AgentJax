use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetentionPolicy {
    Ephemeral,
    KeepUntil(String),
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactProducer {
    Agent { agent_id: String },
    Plugin { plugin_id: String },
    Tool { tool_call_id: String },
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub meta: ObjectMeta,
    pub artifact_id: String,
    pub producer: ArtifactProducer,
    pub mime: String,
    pub uri: String,
    pub size_bytes: Option<u64>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub source_event_id: Option<String>,
    pub retention_policy: Option<RetentionPolicy>,
    pub tags: Vec<String>,
}
