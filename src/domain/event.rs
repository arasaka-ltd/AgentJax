use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEvent {
    pub event_id: String,
    pub event_type: EventType,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub task_id: Option<String>,
    pub plugin_id: Option<String>,
    pub node_id: Option<String>,
    pub source: EventSource,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub payload: serde_json::Value,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventType {
    MessageReceived,
    TurnStarted,
    ContextBuilt,
    ModelCalled,
    ModelResponseReceived,
    ToolCalled,
    ToolCompleted,
    ToolFailed,
    ArtifactCreated,
    TaskStarted,
    TaskSucceeded,
    TaskFailed,
    TaskCheckpointed,
    TaskCancelled,
    MemoryCommitted,
    SummaryCompacted,
    SummaryInvalidated,
    SummaryRecomputed,
    PluginLoaded,
    PluginReloaded,
    PluginDrained,
    ScheduleTriggered,
    NodeStatusChanged,
    ResourceBound,
    BillingRecorded,
    UsageRecorded,
    TurnSucceeded,
    TurnFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventSource {
    User,
    Agent,
    Plugin { plugin_id: String },
    Scheduler,
    Node { node_id: String },
    Operator,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventRecord {
    pub meta: ObjectMeta,
    pub runtime: RuntimeEvent,
}
