use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::api::{StreamId, SubscriptionId};
use crate::domain::{
    Agent, AgentStatus, Node, NodeStatus, PluginDescriptor, Schedule, Session, SessionModelTarget,
    SessionStatus, Task, TaskCheckpoint, TaskStatus, TaskTimelineEntry,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApiMethod {
    #[serde(rename = "runtime.ping")]
    RuntimePing,
    #[serde(rename = "runtime.status")]
    RuntimeStatus,
    #[serde(rename = "runtime.shutdown")]
    RuntimeShutdown,
    #[serde(rename = "config.inspect")]
    ConfigInspect,
    #[serde(rename = "config.validate")]
    ConfigValidate,
    #[serde(rename = "config.reload")]
    ConfigReload,
    #[serde(rename = "plugin.list")]
    PluginList,
    #[serde(rename = "plugin.inspect")]
    PluginInspect,
    #[serde(rename = "plugin.reload")]
    PluginReload,
    #[serde(rename = "plugin.test")]
    PluginTest,
    #[serde(rename = "agent.list")]
    AgentList,
    #[serde(rename = "agent.get")]
    AgentGet,
    #[serde(rename = "session.list")]
    SessionList,
    #[serde(rename = "session.get")]
    SessionGet,
    #[serde(rename = "session.send")]
    SessionSend,
    #[serde(rename = "session.model.inspect")]
    SessionModelInspect,
    #[serde(rename = "session.model.switch")]
    SessionModelSwitch,
    #[serde(rename = "session.cancel")]
    SessionCancel,
    #[serde(rename = "session.subscribe")]
    SessionSubscribe,
    #[serde(rename = "task.list")]
    TaskList,
    #[serde(rename = "task.get")]
    TaskGet,
    #[serde(rename = "task.cancel")]
    TaskCancel,
    #[serde(rename = "task.retry")]
    TaskRetry,
    #[serde(rename = "task.subscribe")]
    TaskSubscribe,
    #[serde(rename = "node.list")]
    NodeList,
    #[serde(rename = "node.get")]
    NodeGet,
    #[serde(rename = "schedule.list")]
    ScheduleList,
    #[serde(rename = "schedule.create")]
    ScheduleCreate,
    #[serde(rename = "schedule.update")]
    ScheduleUpdate,
    #[serde(rename = "schedule.delete")]
    ScheduleDelete,
    #[serde(rename = "doctor.run")]
    DoctorRun,
    #[serde(rename = "smoke.run")]
    SmokeRun,
    #[serde(rename = "logs.tail")]
    LogsTail,
    #[serde(rename = "metrics.snapshot")]
    MetricsSnapshot,
    #[serde(rename = "subscription.cancel")]
    SubscriptionCancel,
    #[serde(rename = "stream.cancel")]
    StreamCancel,
}

impl ApiMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RuntimePing => "runtime.ping",
            Self::RuntimeStatus => "runtime.status",
            Self::RuntimeShutdown => "runtime.shutdown",
            Self::ConfigInspect => "config.inspect",
            Self::ConfigValidate => "config.validate",
            Self::ConfigReload => "config.reload",
            Self::PluginList => "plugin.list",
            Self::PluginInspect => "plugin.inspect",
            Self::PluginReload => "plugin.reload",
            Self::PluginTest => "plugin.test",
            Self::AgentList => "agent.list",
            Self::AgentGet => "agent.get",
            Self::SessionList => "session.list",
            Self::SessionGet => "session.get",
            Self::SessionSend => "session.send",
            Self::SessionModelInspect => "session.model.inspect",
            Self::SessionModelSwitch => "session.model.switch",
            Self::SessionCancel => "session.cancel",
            Self::SessionSubscribe => "session.subscribe",
            Self::TaskList => "task.list",
            Self::TaskGet => "task.get",
            Self::TaskCancel => "task.cancel",
            Self::TaskRetry => "task.retry",
            Self::TaskSubscribe => "task.subscribe",
            Self::NodeList => "node.list",
            Self::NodeGet => "node.get",
            Self::ScheduleList => "schedule.list",
            Self::ScheduleCreate => "schedule.create",
            Self::ScheduleUpdate => "schedule.update",
            Self::ScheduleDelete => "schedule.delete",
            Self::DoctorRun => "doctor.run",
            Self::SmokeRun => "smoke.run",
            Self::LogsTail => "logs.tail",
            Self::MetricsSnapshot => "metrics.snapshot",
            Self::SubscriptionCancel => "subscription.cancel",
            Self::StreamCancel => "stream.cancel",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePingResponse {
    pub pong: bool,
    pub daemon_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeStatusResponse {
    pub status: String,
    pub daemon_version: String,
    pub api_version: String,
    pub uptime_secs: u64,
    pub ready: bool,
    pub draining: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeShutdownRequest {
    pub graceful: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeShutdownResponse {
    pub accepted: bool,
    pub draining: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigInspectRequest {
    pub section: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigInspectResponse {
    pub section: String,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigValidateResponse {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigReloadResponse {
    pub ok: bool,
    pub reloaded_modules: Vec<String>,
    pub drained_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginListItem {
    pub id: String,
    pub enabled: bool,
    pub healthy: bool,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginListResponse {
    pub items: Vec<PluginListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginInspectRequest {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginInspectResponse {
    pub plugin: PluginDescriptor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginReloadRequest {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginReloadResponse {
    pub ok: bool,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginTestRequest {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginTestResponse {
    pub ok: bool,
    pub plugin_id: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentListItem {
    pub agent_id: String,
    pub status: AgentStatus,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentListResponse {
    pub items: Vec<AgentListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentGetRequest {
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentGetResponse {
    pub agent: Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionListItem {
    pub session_id: String,
    pub agent_id: String,
    pub title: Option<String>,
    pub status: SessionStatus,
    pub channel_id: Option<String>,
    pub surface_id: Option<String>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionListResponse {
    pub items: Vec<SessionListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionGetRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionGetResponse {
    pub session: Session,
    pub messages: Vec<SessionMessage>,
    pub events: Vec<crate::domain::RuntimeEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionMessageKind {
    #[default]
    User,
    Assistant,
    ToolResult,
    System,
    Runtime,
}

impl SessionMessageKind {
    pub fn as_role_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::ToolResult => "tool_result",
            Self::System => "system",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionMessageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMessageAnnotation {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMessage {
    #[serde(default)]
    pub kind: SessionMessageKind,
    #[serde(default)]
    pub meta: SessionMessageMeta,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<SessionMessageAnnotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl SessionMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            kind: SessionMessageKind::User,
            meta: SessionMessageMeta::default(),
            content: content.into(),
            annotations: Vec::new(),
            role: Some("user".into()),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            kind: SessionMessageKind::Assistant,
            meta: SessionMessageMeta::default(),
            content: content.into(),
            annotations: Vec::new(),
            role: Some("assistant".into()),
        }
    }

    pub fn tool_result(content: impl Into<String>) -> Self {
        Self {
            kind: SessionMessageKind::ToolResult,
            meta: SessionMessageMeta::default(),
            content: content.into(),
            annotations: Vec::new(),
            role: Some("tool_result".into()),
        }
    }

    pub fn runtime(content: impl Into<String>) -> Self {
        Self {
            kind: SessionMessageKind::Runtime,
            meta: SessionMessageMeta::default(),
            content: content.into(),
            annotations: Vec::new(),
            role: Some("runtime".into()),
        }
    }

    pub fn normalized_kind(&self) -> SessionMessageKind {
        match (self.kind.clone(), self.role.as_deref()) {
            (SessionMessageKind::User, Some("assistant")) => SessionMessageKind::Assistant,
            (SessionMessageKind::User, Some("tool_result")) => SessionMessageKind::ToolResult,
            (SessionMessageKind::User, Some("system")) => SessionMessageKind::System,
            (SessionMessageKind::User, Some("runtime")) => SessionMessageKind::Runtime,
            (kind, _) => kind,
        }
    }

    pub fn display_role(&self) -> &str {
        self.role
            .as_deref()
            .unwrap_or_else(|| self.normalized_kind().as_role_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSendRequest {
    pub session_id: String,
    pub message: SessionMessage,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSendResponse {
    pub accepted: bool,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<StreamId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionModelInspectRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionModelState {
    pub current: SessionModelTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<SessionModelTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_switched_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionModelInspectResponse {
    pub session_id: String,
    pub model: SessionModelState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionModelSwitchRequest {
    pub session_id: String,
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionModelSwitchResult {
    Pending,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionModelSwitchResponse {
    pub session_id: String,
    pub result: SessionModelSwitchResult,
    pub model: SessionModelState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionCancelRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSubscribeRequest {
    pub session_id: String,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListItem {
    pub task_id: String,
    pub kind: String,
    pub status: TaskStatus,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListResponse {
    pub items: Vec<TaskListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskGetRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskGetResponse {
    pub task: Task,
    #[serde(default)]
    pub timeline: Vec<TaskTimelineEntry>,
    #[serde(default)]
    pub checkpoints: Vec<TaskCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskCancelRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRetryRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskSubscribeRequest {
    pub task_id: String,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeListItem {
    pub node_id: String,
    pub status: NodeStatus,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeListResponse {
    pub items: Vec<NodeListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeGetRequest {
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeGetResponse {
    pub node: Node,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleListItem {
    pub schedule_id: String,
    pub kind: String,
    pub enabled: bool,
    pub next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleListResponse {
    pub items: Vec<ScheduleListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleCreateRequest {
    pub schedule: Schedule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleUpdateRequest {
    pub schedule: Schedule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleDeleteRequest {
    pub schedule_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleGetResponse {
    pub schedule: Schedule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorCheckResult {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorRunResponse {
    pub ok: bool,
    pub checks: Vec<DoctorCheckResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmokeRunRequest {
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmokeRunResponse {
    pub ok: bool,
    pub target: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogsTailRequest {
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LogLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsSnapshotResponse {
    pub counters: Value,
    pub gauges: Value,
    pub histograms: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionCancelRequest {
    pub subscription_id: SubscriptionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamCancelRequest {
    pub stream_id: StreamId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionResponse {
    pub subscription_id: SubscriptionId,
    pub accepted_events: Vec<String>,
}
