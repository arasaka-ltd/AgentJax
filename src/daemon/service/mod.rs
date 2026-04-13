mod helpers;
mod request_handlers;
mod runtime_support;
mod session;
mod tool_loop;

use self::{helpers::*, tool_loop::*};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use chrono::Utc;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::{
    api::{
        AgentGetRequest, AgentGetResponse, AgentListItem, AgentListResponse, ApiError,
        ApiErrorCode, ApiMethod, ClientEnvelope, ConfigInspectRequest, ConfigInspectResponse,
        ConfigReloadResponse, ConfigValidateResponse, ConnectionId, DoctorCheckResult,
        DoctorRunResponse, HelloAckEnvelope, LogsTailRequest, MetricsSnapshotResponse,
        NodeGetRequest, NodeGetResponse, NodeListItem, NodeListResponse, PluginInspectRequest,
        PluginInspectResponse, PluginListItem, PluginListResponse, PluginReloadRequest,
        PluginReloadResponse, PluginTestRequest, PluginTestResponse, RequestEnvelope,
        ResponseEnvelope, RuntimePingResponse, RuntimeShutdownRequest, RuntimeShutdownResponse,
        RuntimeStatusResponse, ScheduleCreateRequest, ScheduleDeleteRequest, ScheduleGetResponse,
        ScheduleListItem, ScheduleListResponse, ScheduleUpdateRequest, ServerEnvelope,
        SessionCancelRequest, SessionGetRequest, SessionGetResponse, SessionListItem,
        SessionListResponse, SessionMessage, SessionMessageAnnotation, SessionModelInspectRequest,
        SessionModelInspectResponse, SessionModelState, SessionModelSwitchRequest,
        SessionModelSwitchResponse, SessionModelSwitchResult, SessionSendRequest,
        SessionSendResponse, SessionSubscribeRequest, SmokeRunRequest, SmokeRunResponse,
        StreamCancelRequest, StreamEnvelope, StreamPhase, SubscriptionCancelRequest,
        SubscriptionResponse, TaskCancelRequest, TaskGetRequest, TaskGetResponse, TaskListItem,
        TaskListResponse, TaskRetryRequest, TaskSubscribeRequest,
    },
    app::Application,
    context_engine::{
        parse_workspace_prompt_documents, render_prompt_xml, AssembledContext,
        ContextAssemblyRequest, PromptRenderRequest,
    },
    core::AgentPromptRequest,
    daemon::store::DaemonStore,
    domain::{
        Agent, AgentStatus, AutonomyPolicy, ContextAssemblyPurpose, EventType, ExecutionMode,
        ModelOutputItem, ModelTurnOutput, Node, NodeKind, NodeStatus, ObjectMeta, PluginDescriptor,
        PluginStatus, Schedule, Session, SessionModelTarget, Task, TaskStatus, ToolCall,
        ToolCallItem, ToolCaller, ToolResultItem, TrustLevel,
    },
};

pub const API_VERSION: &str = "v1";
pub const SCHEMA_VERSION: &str = "2026-04-10";
const MAX_TOOL_LOOP_STEPS: usize = 8;

#[derive(Clone)]
pub struct Daemon {
    app: Arc<Application>,
    store: Arc<DaemonStore>,
    control: Arc<Mutex<ControlPlaneState>>,
}

#[derive(Default)]
struct ControlPlaneState {
    schedules: BTreeMap<String, Schedule>,
    subscriptions: BTreeMap<String, RegisteredSubscription>,
    streams: BTreeMap<String, RegisteredStream>,
    logs: Vec<String>,
    resuming_tasks: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct RegisteredSubscription {
    _kind: &'static str,
    _target_id: String,
    _accepted_events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamStatus {
    Active,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone)]
struct RegisteredStream {
    status: StreamStatus,
    _source: &'static str,
}

pub struct Dispatch {
    pub response: ServerEnvelope,
    pub followups: Vec<ServerEnvelope>,
    pub live_stream: Option<UnboundedReceiver<ServerEnvelope>>,
}

impl Dispatch {
    pub fn single(response: ServerEnvelope) -> Self {
        Self {
            response,
            followups: Vec::new(),
            live_stream: None,
        }
    }
}

impl Daemon {
    pub fn new(app: Application) -> anyhow::Result<Self> {
        let runtime_config = app.runtime_config.clone();
        let daemon = Self {
            app: Arc::new(app),
            store: Arc::new(DaemonStore::new(runtime_config)?),
            control: Arc::new(Mutex::new(ControlPlaneState::default())),
        };
        daemon.spawn_waiting_task_scheduler();
        Ok(daemon)
    }

    pub fn connection_id(&self) -> ConnectionId {
        ConnectionId(self.store.next_connection_id())
    }

    pub fn hello_ack(&self, connection_id: ConnectionId) -> HelloAckEnvelope {
        HelloAckEnvelope::new(connection_id, API_VERSION)
    }

    pub async fn handle_client_envelope(
        &self,
        envelope: ClientEnvelope,
    ) -> Result<Dispatch, ApiError> {
        match envelope {
            ClientEnvelope::Hello(_) => Err(ApiError::new(
                ApiErrorCode::ProtocolViolation,
                "hello is only valid during handshake",
                false,
            )),
            ClientEnvelope::Request(request) => Ok(self.handle_request(request).await),
        }
    }

    pub async fn handle_request(&self, request: RequestEnvelope) -> Dispatch {
        if matches!(request.method, ApiMethod::SessionSend) {
            return self.handle_session_send_dispatch(request).await;
        }
        match self.route_request(&request).await {
            Ok((result, followups)) => Dispatch {
                response: ServerEnvelope::Response(ResponseEnvelope::ok(request.id, result)),
                followups,
                live_stream: None,
            },
            Err(error) => Dispatch::single(ServerEnvelope::Response(ResponseEnvelope::err(
                request.id, error,
            ))),
        }
    }
}
