use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};

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
}

impl Dispatch {
    pub fn single(response: ServerEnvelope) -> Self {
        Self {
            response,
            followups: Vec::new(),
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
        match self.route_request(&request).await {
            Ok((result, followups)) => Dispatch {
                response: ServerEnvelope::Response(ResponseEnvelope::ok(request.id, result)),
                followups,
            },
            Err(error) => Dispatch::single(ServerEnvelope::Response(ResponseEnvelope::err(
                request.id, error,
            ))),
        }
    }

    async fn route_request(
        &self,
        request: &RequestEnvelope,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        match request.method {
            ApiMethod::RuntimePing => Ok((
                self.serialize(RuntimePingResponse {
                    pong: true,
                    daemon_time: Utc::now(),
                })?,
                Vec::new(),
            )),
            ApiMethod::RuntimeStatus => Ok((self.serialize(self.runtime_status())?, Vec::new())),
            ApiMethod::RuntimeShutdown => self.handle_runtime_shutdown(request.parse_params()?),
            ApiMethod::ConfigInspect => self.handle_config_inspect(request.parse_params()?),
            ApiMethod::ConfigValidate => self.handle_config_validate(),
            ApiMethod::ConfigReload => self.handle_config_reload(),
            ApiMethod::PluginList => self.handle_plugin_list(),
            ApiMethod::PluginInspect => self.handle_plugin_inspect(request.parse_params()?),
            ApiMethod::PluginReload => self.handle_plugin_reload(request.parse_params()?),
            ApiMethod::PluginTest => self.handle_plugin_test(request.parse_params()?),
            ApiMethod::AgentList => self.handle_agent_list(),
            ApiMethod::AgentGet => self.handle_agent_get(request.parse_params()?),
            ApiMethod::SessionList => {
                let items = self
                    .store
                    .list_sessions()
                    .map_err(internal_store_error)?
                    .into_iter()
                    .map(|state| SessionListItem {
                        session_id: state.session.session_id,
                        agent_id: state.session.agent_id,
                        title: state.session.title,
                        status: state.session.status,
                        channel_id: state.session.channel_id,
                        surface_id: state.session.surface_id,
                        last_activity_at: Some(state.session.meta.updated_at),
                    })
                    .collect();
                Ok((self.serialize(SessionListResponse { items })?, Vec::new()))
            }
            ApiMethod::SessionGet => {
                let params: SessionGetRequest = request.parse_params()?;
                let state = self
                    .store
                    .get_session(&params.session_id)
                    .map_err(internal_store_error)?
                    .ok_or_else(session_not_found)?;
                Ok((
                    self.serialize(SessionGetResponse {
                        session: state.session,
                        messages: state.messages,
                        events: state.events,
                    })?,
                    Vec::new(),
                ))
            }
            ApiMethod::SessionModelInspect => {
                self.handle_session_model_inspect(request.parse_params()?)
            }
            ApiMethod::SessionModelSwitch => {
                self.handle_session_model_switch(request.parse_params()?)
            }
            ApiMethod::SessionCancel => self.handle_session_cancel(request.parse_params()?),
            ApiMethod::SessionSubscribe => self.handle_session_subscribe(request.parse_params()?),
            ApiMethod::SessionSend => self.handle_session_send(request.parse_params()?).await,
            ApiMethod::TaskList => self.handle_task_list(),
            ApiMethod::TaskGet => self.handle_task_get(request.parse_params()?),
            ApiMethod::TaskCancel => self.handle_task_cancel(request.parse_params()?),
            ApiMethod::TaskRetry => self.handle_task_retry(request.parse_params()?),
            ApiMethod::TaskSubscribe => self.handle_task_subscribe(request.parse_params()?),
            ApiMethod::NodeList => self.handle_node_list(),
            ApiMethod::NodeGet => self.handle_node_get(request.parse_params()?),
            ApiMethod::ScheduleList => self.handle_schedule_list(),
            ApiMethod::ScheduleCreate => self.handle_schedule_create(request.parse_params()?),
            ApiMethod::ScheduleUpdate => self.handle_schedule_update(request.parse_params()?),
            ApiMethod::ScheduleDelete => self.handle_schedule_delete(request.parse_params()?),
            ApiMethod::DoctorRun => self.handle_doctor_run(),
            ApiMethod::SmokeRun => self.handle_smoke_run(request.parse_params()?),
            ApiMethod::LogsTail => self.handle_logs_tail(request.parse_params()?),
            ApiMethod::MetricsSnapshot => self.handle_metrics_snapshot(),
            ApiMethod::SubscriptionCancel => {
                self.handle_subscription_cancel(request.parse_params()?)
            }
            ApiMethod::StreamCancel => self.handle_stream_cancel(request.parse_params()?),
        }
    }

    async fn handle_session_send(
        &self,
        params: SessionSendRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let session_id = params.session_id.clone();
        self.store
            .mark_turn_active(&session_id)
            .map_err(map_store_error)?;
        let result = self.handle_session_send_inner(params).await;
        self.store.clear_turn_active(&session_id);
        result
    }

    fn handle_session_model_inspect(
        &self,
        params: SessionModelInspectRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let session = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        Ok((
            self.serialize(SessionModelInspectResponse {
                session_id: params.session_id,
                model: self.session_model_state(&session.session)?,
            })?,
            Vec::new(),
        ))
    }

    fn handle_session_model_switch(
        &self,
        params: SessionModelSwitchRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut session = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?
            .session;
        let requested = SessionModelTarget {
            provider_id: params.provider_id,
            model_id: params.model_id,
        };

        self.record_event(
            &session.session_id,
            session
                .last_turn_id
                .as_deref()
                .unwrap_or("turn.model_switch"),
            None,
            EventType::ModelSwitchRequested,
            json!({
                "requested_target": requested.clone(),
            }),
        )?;

        if self.store.is_turn_active(&session.session_id) {
            self.record_event(
                &session.session_id,
                session
                    .last_turn_id
                    .as_deref()
                    .unwrap_or("turn.model_switch"),
                None,
                EventType::ModelSwitchRejected,
                json!({
                    "reason": "active turn in progress",
                    "requested_target": requested.clone(),
                }),
            )?;
            return Ok((
                self.serialize(SessionModelSwitchResponse {
                    session_id: session.session_id.clone(),
                    result: SessionModelSwitchResult::Rejected,
                    model: self.session_model_state(&session)?,
                    reason: Some("active turn in progress".into()),
                })?,
                Vec::new(),
            ));
        }

        session.pending_model_switch = Some(requested.clone());
        self.store
            .upsert_session(session.clone())
            .map_err(internal_store_error)?;

        if let Err(error) = self
            .app
            .runtime
            .validate_provider_model_binding(&requested.provider_id, &requested.model_id)
        {
            session.pending_model_switch = None;
            self.store
                .upsert_session(session.clone())
                .map_err(internal_store_error)?;
            self.record_event(
                &session.session_id,
                session
                    .last_turn_id
                    .as_deref()
                    .unwrap_or("turn.model_switch"),
                None,
                EventType::ModelSwitchRejected,
                json!({
                    "reason": error.to_string(),
                    "requested_target": requested.clone(),
                }),
            )?;
            return Ok((
                self.serialize(SessionModelSwitchResponse {
                    session_id: session.session_id.clone(),
                    result: SessionModelSwitchResult::Rejected,
                    model: self.session_model_state(&session)?,
                    reason: Some(error.to_string()),
                })?,
                Vec::new(),
            ));
        }

        session.current_provider_id = Some(requested.provider_id.clone());
        session.current_model_id = Some(requested.model_id.clone());
        session.pending_model_switch = None;
        session.last_model_switched_at = Some(Utc::now());
        session.meta.updated_at = Utc::now();
        self.store
            .upsert_session(session.clone())
            .map_err(internal_store_error)?;

        self.record_event(
            &session.session_id,
            session
                .last_turn_id
                .as_deref()
                .unwrap_or("turn.model_switch"),
            None,
            EventType::ModelSwitchApplied,
            json!({
                "current_target": {
                    "provider_id": session.current_provider_id.clone(),
                    "model_id": session.current_model_id.clone(),
                }
            }),
        )?;

        Ok((
            self.serialize(SessionModelSwitchResponse {
                session_id: session.session_id.clone(),
                result: SessionModelSwitchResult::Applied,
                model: self.session_model_state(&session)?,
                reason: None,
            })?,
            Vec::new(),
        ))
    }

    fn handle_runtime_shutdown(
        &self,
        params: RuntimeShutdownRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let _ = params;
        self.store.set_draining(true);
        self.push_log("runtime shutdown requested");
        Ok((
            self.serialize(RuntimeShutdownResponse {
                accepted: true,
                draining: self.store.draining(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_config_inspect(
        &self,
        params: ConfigInspectRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let config = match params.section.as_str() {
            "runtime" | "core" => {
                serde_json::to_value(&self.app.runtime_config).map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("config inspect serialization failed: {error}"),
                        false,
                    )
                })?
            }
            "workspace" => {
                serde_json::to_value(&self.app.workspace_runtime.workspace).map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("workspace inspect serialization failed: {error}"),
                        false,
                    )
                })?
            }
            "plugins" => serde_json::to_value(self.plugin_descriptors()).map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("plugin inspect serialization failed: {error}"),
                    false,
                )
            })?,
            "resources" => {
                serde_json::to_value(self.app.resource_registry.all()).map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("resource inspect serialization failed: {error}"),
                        false,
                    )
                })?
            }
            other => {
                return Err(ApiError::new(
                    ApiErrorCode::InvalidRequest,
                    format!("unknown config section: {other}"),
                    false,
                ))
            }
        };
        Ok((
            self.serialize(ConfigInspectResponse {
                section: params.section,
                config,
            })?,
            Vec::new(),
        ))
    }

    fn handle_config_validate(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let report = crate::config::ConfigLoader::validate_at(
            &self.app.config_root.root,
            &self.app.runtime_config.runtime_paths.root,
            &self.app.runtime_config.workspace.paths.root,
        )
        .map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("config validation failed to execute: {error}"),
                false,
            )
        })?;
        Ok((
            self.serialize(ConfigValidateResponse {
                ok: report.ok,
                errors: report.errors,
                warnings: report.warnings,
                migrations: report
                    .migrations
                    .into_iter()
                    .map(|step| format!("{}: {}", step.file, step.summary))
                    .collect(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_config_reload(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        self.push_log("config reload requested");
        let candidate = crate::config::ConfigLoader::load_from_roots(
            &self.app.config_root.root,
            &self.app.runtime_config.runtime_paths.root,
            &self.app.runtime_config.workspace.paths.root,
        )
        .map(|loaded| loaded.config_snapshot)
        .map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("config reload failed to load candidate snapshot: {error}"),
                false,
            )
        })?;
        let diff = self.app.config_snapshot.diff(&candidate).map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("config reload diff failed: {error}"),
                false,
            )
        })?;
        Ok((
            self.serialize(ConfigReloadResponse {
                ok: !diff.reload_plan.restart_required,
                disposition: format!("{:?}", diff.reload_plan.disposition),
                reloaded_modules: diff.reload_plan.affected_modules,
                drained_modules: diff
                    .reload_plan
                    .drained_modules
                    .into_iter()
                    .map(|drain| drain.module)
                    .collect(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_plugin_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let items = self
            .plugin_descriptors()
            .into_iter()
            .map(|plugin| PluginListItem {
                id: plugin.plugin_id,
                enabled: matches!(
                    plugin.status,
                    PluginStatus::Loading
                        | PluginStatus::Loaded
                        | PluginStatus::Starting
                        | PluginStatus::Running
                ),
                healthy: !matches!(plugin.status, PluginStatus::Failed),
                capabilities: plugin.capabilities,
            })
            .collect();
        Ok((self.serialize(PluginListResponse { items })?, Vec::new()))
    }

    fn handle_plugin_inspect(
        &self,
        params: PluginInspectRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let snapshot = self
            .app
            .plugin_manager
            .snapshot(
                &params.plugin_id,
                &self.app.runtime_config.plugin_api_version,
            )
            .ok_or_else(plugin_not_found)?;
        Ok((
            self.serialize(PluginInspectResponse {
                plugin: snapshot.plugin,
                enabled: snapshot.enabled,
                default_enabled: snapshot.default_enabled,
                healthy: snapshot.healthy,
                dependencies: snapshot.dependencies,
                optional_dependencies: snapshot.optional_dependencies,
                required_permissions: snapshot.required_permissions,
                provided_resources: snapshot.provided_resources,
                config_ref: snapshot.config_ref,
                policy_flags: snapshot.policy_flags,
                reload_hint: snapshot.reload_hint,
                last_lifecycle_stage: snapshot.last_lifecycle_stage,
                last_error: snapshot.last_error,
            })?,
            Vec::new(),
        ))
    }

    fn handle_plugin_reload(
        &self,
        params: PluginReloadRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let report = self
            .app
            .plugin_manager
            .reload(
                &params.plugin_id,
                &self.app.plugin_registry,
                &self.app.resource_registry,
                &self.app.tool_registry,
                &self.app.event_bus,
                self.app.plugin_host.hooks(),
                &self.app.runtime_config,
                &self.app.workspace_host,
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::NotFound,
                    format!("plugin reload failed: {error}"),
                    false,
                )
            })?;
        self.push_log(format!("plugin reload requested: {}", params.plugin_id));
        Ok((
            self.serialize(PluginReloadResponse {
                ok: report.ok,
                plugin_id: report.plugin_id,
                status: format!("{:?}", report.status),
                lifecycle_stage: report.lifecycle_stage,
                summary: report.summary,
                checks: report.checks,
            })?,
            Vec::new(),
        ))
    }

    fn handle_plugin_test(
        &self,
        params: PluginTestRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let report = self
            .app
            .plugin_manager
            .test_plugin(
                &params.plugin_id,
                &self.app.runtime_config.plugin_api_version,
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::NotFound,
                    format!("plugin test failed: {error}"),
                    false,
                )
            })?;
        Ok((
            self.serialize(PluginTestResponse {
                ok: report.ok,
                plugin_id: report.plugin_id,
                status: format!("{:?}", report.status),
                lifecycle_stage: report.lifecycle_stage,
                summary: report.summary,
                checks: report.checks,
            })?,
            Vec::new(),
        ))
    }

    fn handle_agent_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let agent = self.default_agent_descriptor();
        Ok((
            self.serialize(AgentListResponse {
                items: vec![AgentListItem {
                    agent_id: agent.agent_id,
                    status: agent.status,
                    workspace_id: agent.workspace_id,
                }],
            })?,
            Vec::new(),
        ))
    }

    fn handle_agent_get(
        &self,
        params: AgentGetRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let agent = self.default_agent_descriptor();
        if params.agent_id != agent.agent_id {
            return Err(agent_not_found());
        }
        Ok((self.serialize(AgentGetResponse { agent })?, Vec::new()))
    }

    fn handle_session_cancel(
        &self,
        params: SessionCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut record = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        record.session.status = crate::domain::SessionStatus::Closed;
        record.session.meta.updated_at = Utc::now();
        self.store
            .upsert_session(record.session)
            .map_err(internal_store_error)?;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_session_subscribe(
        &self,
        params: SessionSubscribeRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        if self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .is_none()
        {
            return Err(session_not_found());
        }
        let subscription_id = self.store.next_subscription_id();
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .subscriptions
            .insert(
                subscription_id.clone(),
                RegisteredSubscription {
                    _kind: "session",
                    _target_id: params.session_id,
                    _accepted_events: params.events.clone(),
                },
            );
        Ok((
            self.serialize(SubscriptionResponse {
                subscription_id: subscription_id.into(),
                accepted_events: params.events,
            })?,
            Vec::new(),
        ))
    }

    fn handle_task_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let items = self
            .store
            .list_tasks()
            .map_err(internal_store_error)?
            .into_iter()
            .map(|record| TaskListItem {
                task_id: record.task.task_id.clone(),
                kind: match record.task.execution_mode {
                    ExecutionMode::EphemeralSession => "ephemeral_session".into(),
                    ExecutionMode::BoundSession => "bound_session".into(),
                    ExecutionMode::HeadlessTask => "headless_task".into(),
                },
                status: record.task.status.clone(),
                agent_id: record.task.agent_id.clone(),
                session_id: record.task.session_id.clone(),
                created_at: record.task.meta.created_at,
            })
            .collect();
        Ok((self.serialize(TaskListResponse { items })?, Vec::new()))
    }

    fn handle_task_get(
        &self,
        params: TaskGetRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let record = self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        Ok((
            self.serialize(TaskGetResponse {
                task: record.task,
                timeline: record.timeline,
                checkpoints: record.checkpoints,
            })?,
            Vec::new(),
        ))
    }

    fn handle_task_cancel(
        &self,
        params: TaskCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut task = self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        task.task.status = TaskStatus::Cancelled;
        task.task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.task)
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &params.task_id,
                crate::domain::TaskPhase::Cancelled,
                TaskStatus::Cancelled,
                None,
                None,
                "task cancelled",
            )
            .map_err(internal_store_error)?;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_task_retry(
        &self,
        params: TaskRetryRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut task = self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        task.task.status = TaskStatus::Ready;
        task.task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.task)
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &params.task_id,
                crate::domain::TaskPhase::Ready,
                TaskStatus::Ready,
                None,
                None,
                "task retried and returned to ready",
            )
            .map_err(internal_store_error)?;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_task_subscribe(
        &self,
        params: TaskSubscribeRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        if self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .is_none()
        {
            return Err(task_not_found());
        }
        let subscription_id = self.store.next_subscription_id();
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .subscriptions
            .insert(
                subscription_id.clone(),
                RegisteredSubscription {
                    _kind: "task",
                    _target_id: params.task_id,
                    _accepted_events: params.events.clone(),
                },
            );
        Ok((
            self.serialize(SubscriptionResponse {
                subscription_id: subscription_id.into(),
                accepted_events: params.events,
            })?,
            Vec::new(),
        ))
    }

    fn handle_node_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let node = self.default_node();
        Ok((
            self.serialize(NodeListResponse {
                items: vec![NodeListItem {
                    node_id: node.node_id,
                    status: node.status,
                    capabilities: node.capabilities,
                }],
            })?,
            Vec::new(),
        ))
    }

    fn handle_node_get(
        &self,
        params: NodeGetRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let node = self.default_node();
        if params.node_id != node.node_id {
            return Err(node_not_found());
        }
        Ok((self.serialize(NodeGetResponse { node })?, Vec::new()))
    }

    fn handle_schedule_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let schedules = self.control.lock().expect("control plane lock poisoned");
        let items = schedules
            .schedules
            .values()
            .map(|schedule| ScheduleListItem {
                schedule_id: schedule.schedule_id.clone(),
                kind: schedule_kind(schedule),
                enabled: schedule.enabled,
                next_run_at: None,
            })
            .collect();
        Ok((self.serialize(ScheduleListResponse { items })?, Vec::new()))
    }

    fn handle_schedule_create(
        &self,
        params: ScheduleCreateRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut control = self.control.lock().expect("control plane lock poisoned");
        control
            .schedules
            .insert(params.schedule.schedule_id.clone(), params.schedule.clone());
        Ok((
            self.serialize(ScheduleGetResponse {
                schedule: params.schedule,
            })?,
            Vec::new(),
        ))
    }

    fn handle_schedule_update(
        &self,
        params: ScheduleUpdateRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut control = self.control.lock().expect("control plane lock poisoned");
        if !control.schedules.contains_key(&params.schedule.schedule_id) {
            return Err(schedule_not_found());
        }
        control
            .schedules
            .insert(params.schedule.schedule_id.clone(), params.schedule.clone());
        Ok((
            self.serialize(ScheduleGetResponse {
                schedule: params.schedule,
            })?,
            Vec::new(),
        ))
    }

    fn handle_schedule_delete(
        &self,
        params: ScheduleDeleteRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let removed = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .schedules
            .remove(&params.schedule_id);
        if removed.is_none() {
            return Err(schedule_not_found());
        }
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_doctor_run(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        Ok((
            self.serialize(DoctorRunResponse {
                ok: true,
                checks: vec![
                    DoctorCheckResult {
                        id: "runtime_config".into(),
                        status: "ok".into(),
                    },
                    DoctorCheckResult {
                        id: "workspace_identity".into(),
                        status: "ok".into(),
                    },
                    DoctorCheckResult {
                        id: "provider_registry".into(),
                        status: "ok".into(),
                    },
                ],
            })?,
            Vec::new(),
        ))
    }

    fn handle_smoke_run(
        &self,
        params: SmokeRunRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        match params.target.as_str() {
            "daemon" | "plugins" | "plugin-manager" => {
                let snapshots = self
                    .app
                    .plugin_manager
                    .snapshots(&self.app.runtime_config.plugin_api_version);
                let plugin_count = snapshots.len();
                let enabled_count = snapshots.iter().filter(|plugin| plugin.enabled).count();
                let healthy_count = snapshots.iter().filter(|plugin| plugin.healthy).count();
                let mut checks = vec![
                    format!("plugins_discovered={plugin_count}"),
                    format!("plugins_enabled={enabled_count}"),
                    format!("plugins_healthy={healthy_count}"),
                ];

                if plugin_count == 0 {
                    checks.push("plugin_manager=empty".into());
                    return Ok((
                        self.serialize(SmokeRunResponse {
                            ok: false,
                            target: params.target,
                            summary: "plugin manager smoke failed: no plugins discovered".into(),
                            checks,
                        })?,
                        Vec::new(),
                    ));
                }

                for snapshot in &snapshots {
                    let report = self
                        .app
                        .plugin_manager
                        .test_plugin(
                            &snapshot.plugin.plugin_id,
                            &self.app.runtime_config.plugin_api_version,
                        )
                        .map_err(|error| {
                            ApiError::new(
                                ApiErrorCode::InternalError,
                                format!(
                                    "smoke run failed while validating plugin {}: {error}",
                                    snapshot.plugin.plugin_id
                                ),
                                false,
                            )
                        })?;
                    checks.push(format!(
                        "plugin={} status={:?} ok={}",
                        report.plugin_id, report.status, report.ok
                    ));
                }

                let failed_plugins: Vec<&str> = snapshots
                    .iter()
                    .filter(|snapshot| snapshot.enabled && !snapshot.healthy)
                    .map(|snapshot| snapshot.plugin.plugin_id.as_str())
                    .collect();
                if !failed_plugins.is_empty() {
                    checks.push(format!("failed_plugins={}", failed_plugins.join(",")));
                }

                let ok = !snapshots.is_empty() && failed_plugins.is_empty();
                let summary = if ok {
                    format!(
                        "plugin manager smoke passed: {healthy_count}/{plugin_count} plugins healthy"
                    )
                } else {
                    format!(
                        "plugin manager smoke failed: unhealthy plugins [{}]",
                        failed_plugins.join(", ")
                    )
                };

                Ok((
                    self.serialize(SmokeRunResponse {
                        ok,
                        target: params.target,
                        summary,
                        checks,
                    })?,
                    Vec::new(),
                ))
            }
            _ => Ok((
                self.serialize(SmokeRunResponse {
                    ok: false,
                    target: params.target.clone(),
                    summary: format!("unknown smoke target {}", params.target),
                    checks: vec!["supported_targets=daemon,plugins,plugin-manager".into()],
                })?,
                Vec::new(),
            )),
        }
    }

    fn handle_logs_tail(
        &self,
        params: LogsTailRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let logs = self.snapshot_logs();
        if !params.stream {
            return Ok((self.serialize(json!({ "lines": logs }))?, Vec::new()));
        }

        let stream_id = self.store.next_stream_id();
        self.register_stream(&stream_id, "logs.tail", StreamStatus::Active);
        let followups = build_log_stream_envelopes(&stream_id, &logs);
        self.update_stream_status(&stream_id, StreamStatus::Completed);
        Ok((
            self.serialize(json!({
                "accepted": true,
                "stream_id": stream_id,
            }))?,
            followups,
        ))
    }

    fn handle_metrics_snapshot(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let task_count = self.store.list_tasks().map_err(internal_store_error)?.len();
        let schedule_count = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .schedules
            .len();
        let session_count = self
            .store
            .list_sessions()
            .map_err(internal_store_error)?
            .len();
        Ok((
            self.serialize(MetricsSnapshotResponse {
                counters: json!({
                    "sessions_total": session_count,
                    "tasks_total": task_count,
                    "schedules_total": schedule_count,
                    "plugins_total": self.app.plugin_manager.plugin_count(),
                }),
                gauges: json!({
                    "runtime_ready": self.store.ready(),
                    "runtime_draining": self.store.draining(),
                }),
                histograms: json!({
                    "context_events_published": self.app.event_bus.snapshot().len(),
                }),
            })?,
            Vec::new(),
        ))
    }

    fn handle_subscription_cancel(
        &self,
        params: SubscriptionCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let removed = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .subscriptions
            .remove(params.subscription_id.0.as_str());
        if removed.is_none() {
            return Err(subscription_not_found());
        }
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_stream_cancel(
        &self,
        params: StreamCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut control = self.control.lock().expect("control plane lock poisoned");
        let stream = control
            .streams
            .get_mut(params.stream_id.0.as_str())
            .ok_or_else(stream_not_found)?;
        stream.status = StreamStatus::Cancelled;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    async fn handle_session_send_inner(
        &self,
        params: SessionSendRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let turn_id = self.store.next_turn_id();
        let session = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        let session_agent = self
            .app
            .runtime
            .session_agent(
                session.session.current_provider_id.as_deref(),
                session.session.current_model_id.as_deref(),
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("failed to resolve session model: {error}"),
                    false,
                )
            })?;
        let user_message = finalize_message(
            params.message,
            &session.session,
            self.store.next_message_id(),
            None,
        );
        let task_id = self.store.next_task_id();
        let mut task = Task {
            meta: ObjectMeta::new(
                task_id.clone(),
                &self.app.runtime_config.state_schema_version,
            ),
            task_id: task_id.clone(),
            workspace_id: session.session.workspace_id.clone(),
            agent_id: Some(session_agent.agent_id.clone()),
            session_id: Some(params.session_id.clone()),
            parent_task_id: None,
            definition_ref: None,
            execution_mode: ExecutionMode::BoundSession,
            status: TaskStatus::Running,
            priority: crate::domain::TaskPriority::Normal,
            goal: user_message.content.clone(),
            checkpoint_ref: None,
            waiting_until: None,
            waiting_reason: None,
            waiting_resume_hint: None,
        };
        self.store
            .create_task(task.clone())
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskStarted,
            json!({
                "task_id": task_id,
                "execution_mode": "bound_session",
                "goal": task.goal,
            }),
        )?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Running,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                "session.send accepted and task started",
            )
            .map_err(internal_store_error)?;

        self.store
            .append_message(&params.session_id, &turn_id, user_message.clone())
            .map_err(map_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::MessageReceived,
            json!({ "message": user_message }),
        )?;
        let assembled_context = self
            .app
            .context_engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some(params.session_id.clone()),
                task_id: Some(task_id.clone()),
                budget_tokens: 8_000,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
                retrieval_scope: crate::builtin::context::retrieval_types::RetrievalScope::Implicit,
            })
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("context assembly failed: {error}"),
                    false,
                )
            })?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::ContextBuilt,
            json!({
                "block_count": assembled_context.blocks.len(),
                "included_refs": assembled_context.included_refs,
                "token_breakdown": assembled_context.token_breakdown,
            }),
        )?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TurnStarted,
            json!({ "turn_id": turn_id }),
        )?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Running,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                "turn started",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::ModelCalled,
            json!({
                "provider_id": session_agent.provider_id.clone(),
                "model_id": session_agent.model.clone(),
            }),
        )?;
        let prompt_messages = recent_prompt_messages(
            &self
                .store
                .get_session(&params.session_id)
                .map_err(internal_store_error)?
                .ok_or_else(session_not_found)?
                .messages,
            8,
        );
        let stream_id = params.stream.then(|| self.store.next_stream_id());
        if let Some(stream_id) = stream_id.as_deref() {
            self.register_stream(stream_id, "session.send", StreamStatus::Active);
        }
        let mut semantic_stream = stream_id
            .clone()
            .map(|stream_id| SemanticStreamBuilder::new(stream_id, &turn_id));

        let loop_outcome = self
            .run_tool_loop(
                &params.session_id,
                &task_id,
                &turn_id,
                &session_agent,
                &session.session,
                &assembled_context,
                prompt_messages,
                semantic_stream.as_mut(),
            )
            .await
            .map_err(|error| {
                let error_message = error.message.clone();
                let _ = self.record_event(
                    &params.session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnFailed,
                    json!({ "error": error_message }),
                );
                task.status = TaskStatus::Failed;
                task.meta.updated_at = Utc::now();
                let _ = self.store.update_task(task.clone());
                let _ = self.store.append_task_timeline(
                    &task_id,
                    crate::domain::TaskPhase::Failed,
                    TaskStatus::Failed,
                    Some(&turn_id),
                    None,
                    format!("tool loop failed: {}", error.message),
                );
                let _ = self.record_event(
                    &params.session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TaskFailed,
                    json!({
                        "task_id": task_id,
                        "error": error.message.clone(),
                    }),
                );
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("session.send failed: {}", error.message),
                    false,
                )
            })?;

        let assistant_text = match loop_outcome {
            ToolLoopOutcome::Final(text) => text,
            ToolLoopOutcome::Sleeping(runtime_message) => {
                let mut runtime_message = finalize_message(
                    SessionMessage::runtime(runtime_message.clone()),
                    &session.session,
                    self.store.next_message_id(),
                    Some(vec![SessionMessageAnnotation {
                        kind: "runtime_control".into(),
                        value: "sleep".into(),
                    }]),
                );
                runtime_message.meta.actor_id = Some(session_agent.agent_id.clone());
                self.store
                    .append_message(&params.session_id, &turn_id, runtime_message.clone())
                    .map_err(map_store_error)?;
                self.record_event(
                    &params.session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnSucceeded,
                    json!({
                        "turn_id": turn_id,
                        "task_id": task_id,
                        "runtime_message": runtime_message,
                        "waiting": true,
                    }),
                )?;

                if let Some(stream) = semantic_stream.as_mut() {
                    stream.push(
                        "runtime.waiting",
                        json!({
                            "turn_id": turn_id,
                            "message": runtime_message.content,
                        }),
                    );
                }
                let followups = semantic_stream
                    .take()
                    .map(|stream| stream.finish(&turn_id, "waiting"))
                    .unwrap_or_default();
                if let Some(stream_id) = stream_id.as_deref() {
                    self.update_stream_status(stream_id, StreamStatus::Completed);
                }
                let stream_id = if followups.is_empty() {
                    None
                } else {
                    Some(
                        followups[0]
                            .stream_id()
                            .expect("stream envelope id exists")
                            .into(),
                    )
                };
                return Ok((
                    self.serialize(SessionSendResponse {
                        accepted: true,
                        turn_id,
                        stream_id,
                    })?,
                    followups,
                ));
            }
        };

        let mut assistant_message = finalize_message(
            SessionMessage::assistant(assistant_text),
            &session.session,
            self.store.next_message_id(),
            None,
        );
        assistant_message.meta.actor_id = Some(session_agent.agent_id.clone());
        self.store
            .append_message(&params.session_id, &turn_id, assistant_message.clone())
            .map_err(map_store_error)?;
        let checkpoint = self
            .app
            .context_engine
            .build_resume_pack(Some(&params.session_id), Some(&task_id))
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("resume pack build failed: {error}"),
                    false,
                )
            })?;
        self.store
            .write_task_checkpoint(
                &task_id,
                Some(&params.session_id),
                Some(&turn_id),
                assistant_message.content.clone(),
                checkpoint,
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskCheckpointed,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
            }),
        )?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Checkpointed,
                TaskStatus::Succeeded,
                Some(&turn_id),
                None,
                "checkpoint recorded from latest assistant output",
            )
            .map_err(internal_store_error)?;
        task.status = TaskStatus::Succeeded;
        task.checkpoint_ref = self
            .store
            .get_task(&task_id)
            .map_err(internal_store_error)?
            .and_then(|record| record.task.checkpoint_ref);
        task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Succeeded,
                TaskStatus::Succeeded,
                Some(&turn_id),
                None,
                "turn completed and task succeeded",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskSucceeded,
            json!({
                "task_id": task_id,
                "checkpoint_ref": task.checkpoint_ref,
            }),
        )?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TurnSucceeded,
            json!({
                "turn_id": turn_id,
                "task_id": task_id,
                "assistant_message": assistant_message,
            }),
        )?;

        if let Some(stream) = semantic_stream.as_mut() {
            stream.push(
                "assistant.completed",
                json!({
                    "turn_id": turn_id,
                    "message_id": assistant_message.meta.message_id,
                }),
            );
        }
        let followups = semantic_stream
            .take()
            .map(|stream| stream.finish(&turn_id, "completed"))
            .unwrap_or_default();
        if let Some(stream_id) = stream_id.as_deref() {
            self.update_stream_status(stream_id, StreamStatus::Completed);
        }
        let stream_id = if followups.is_empty() {
            None
        } else {
            Some(
                followups[0]
                    .stream_id()
                    .expect("stream envelope id exists")
                    .into(),
            )
        };

        Ok((
            self.serialize(SessionSendResponse {
                accepted: true,
                turn_id,
                stream_id,
            })?,
            followups,
        ))
    }

    fn record_event(
        &self,
        session_id: &str,
        turn_id: &str,
        task_id: Option<&str>,
        event_type: EventType,
        payload: Value,
    ) -> Result<(), ApiError> {
        let event = self
            .store
            .record_event(
                session_id,
                turn_id,
                task_id,
                &self.store.next_event_id(),
                event_type,
                payload,
            )
            .map_err(map_store_error)?;
        self.app
            .context_engine
            .append_event(event)
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("context engine append failed: {error}"),
                    false,
                )
            })
    }

    async fn execute_tool_call(&self, tool_call: &ToolCall) -> Result<String, ApiError> {
        self.record_shell_tool_started(tool_call)?;
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
            tool_call.task_id.as_deref(),
            EventType::ToolCalled,
            json!({
                "tool_call_id": tool_call.tool_call_id,
                "tool_name": tool_call.tool_name,
                "args": tool_call.args,
                "idempotency_key": tool_call.idempotency_key,
                "timeout_secs": tool_call.timeout_secs,
            }),
        )?;

        let tool = self
            .app
            .tool_registry
            .get(&tool_call.tool_name)
            .ok_or_else(|| ApiError::new(ApiErrorCode::NotFound, "tool not found", false))?;

        match tool.invoke(tool_call).await {
            Ok(output) => {
                self.record_shell_tool_result(tool_call, &output.metadata)?;
                self.record_event(
                    tool_call.session_id.as_deref().unwrap_or("session.default"),
                    tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
                    tool_call.task_id.as_deref(),
                    EventType::ToolCompleted,
                    json!({
                        "tool_call_id": tool_call.tool_call_id,
                        "tool_name": tool_call.tool_name,
                        "output": output.content,
                        "metadata": output.metadata,
                    }),
                )?;
                Ok(output.content)
            }
            Err(error) => {
                self.record_shell_tool_failure(tool_call, &error.to_string())?;
                self.record_event(
                    tool_call.session_id.as_deref().unwrap_or("session.default"),
                    tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
                    tool_call.task_id.as_deref(),
                    EventType::ToolFailed,
                    json!({
                        "tool_call_id": tool_call.tool_call_id,
                        "tool_name": tool_call.tool_name,
                        "error": error.to_string(),
                    }),
                )?;
                Err(ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("tool execution failed: {error}"),
                    false,
                ))
            }
        }
    }

    fn runtime_status(&self) -> RuntimeStatusResponse {
        RuntimeStatusResponse {
            status: "running".into(),
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            api_version: API_VERSION.into(),
            uptime_secs: self.store.uptime_secs(),
            ready: self.store.ready(),
            draining: self.store.draining(),
        }
    }

    fn plugin_descriptors(&self) -> Vec<PluginDescriptor> {
        self.app
            .plugin_manager
            .descriptors(&self.app.runtime_config.plugin_api_version)
    }

    fn default_agent_descriptor(&self) -> Agent {
        Agent {
            meta: ObjectMeta::new(
                self.app.runtime.default_agent().agent_id.clone(),
                &self.app.runtime_config.config_schema_version,
            ),
            agent_id: self.app.runtime.default_agent().agent_id.clone(),
            display_name: self.app.runtime.default_agent().agent_id.clone(),
            workspace_id: self.app.runtime_config.workspace.workspace_id.clone(),
            profile_ref: Some(self.app.workspace_identity.agent.path.display().to_string()),
            mission_ref: Some(
                self.app
                    .workspace_identity
                    .mission
                    .path
                    .display()
                    .to_string(),
            ),
            rules_ref: Some(self.app.workspace_identity.rules.path.display().to_string()),
            router_ref: Some(
                self.app
                    .workspace_identity
                    .router
                    .path
                    .display()
                    .to_string(),
            ),
            default_resource_bindings: self
                .app
                .resource_registry
                .all()
                .into_iter()
                .map(|resource| resource.resource_id)
                .collect(),
            autonomy_policy: AutonomyPolicy::default(),
            status: if self.store.draining() {
                AgentStatus::Draining
            } else {
                AgentStatus::Active
            },
        }
    }

    fn default_node(&self) -> Node {
        Node {
            meta: ObjectMeta::new("node.local", &self.app.runtime_config.state_schema_version),
            node_id: "node.local".into(),
            kind: NodeKind::Static,
            platform: std::env::consts::OS.into(),
            status: if self.store.draining() {
                NodeStatus::Draining
            } else {
                NodeStatus::Active
            },
            capabilities: vec![
                "daemon.control_plane".into(),
                "session.interaction".into(),
                "tool.dispatch".into(),
            ],
            resources: self
                .app
                .resource_registry
                .all()
                .into_iter()
                .map(|resource| resource.resource_id.0)
                .collect(),
            trust_level: TrustLevel::High,
            labels: BTreeMap::from([("scope".into(), "local".into())]),
        }
    }

    fn push_log(&self, line: impl Into<String>) {
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .logs
            .push(format!("{} {}", Utc::now().to_rfc3339(), line.into()));
    }

    fn snapshot_logs(&self) -> Vec<String> {
        let mut logs = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .logs
            .clone();
        if logs.is_empty() {
            logs.push(format!("{} daemon online", Utc::now().to_rfc3339()));
        }
        logs
    }

    fn register_stream(&self, stream_id: &str, source: &'static str, status: StreamStatus) {
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .streams
            .insert(
                stream_id.to_string(),
                RegisteredStream {
                    status,
                    _source: source,
                },
            );
    }

    fn update_stream_status(&self, stream_id: &str, status: StreamStatus) {
        if let Some(stream) = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .streams
            .get_mut(stream_id)
        {
            stream.status = status;
        }
    }

    fn session_model_state(&self, session: &Session) -> Result<SessionModelState, ApiError> {
        Ok(SessionModelState {
            current: SessionModelTarget {
                provider_id: session
                    .resolved_provider_id(&self.app.runtime.default_agent().provider_id)
                    .to_string(),
                model_id: session
                    .resolved_model_id(&self.app.runtime.default_agent().model)
                    .to_string(),
            },
            pending: session.pending_model_switch.clone(),
            last_switched_at: session.last_model_switched_at,
        })
    }

    fn serialize<T: Serialize>(&self, value: T) -> Result<Value, ApiError> {
        serde_json::to_value(value).map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("failed to serialize response: {error}"),
                false,
            )
        })
    }

    fn record_shell_tool_started(&self, tool_call: &ToolCall) -> Result<(), ApiError> {
        let Some(event) = shell_tool_started_event(tool_call) else {
            return Ok(());
        };
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
            tool_call.task_id.as_deref(),
            event.0,
            event.1,
        )
    }

    fn record_shell_tool_result(
        &self,
        tool_call: &ToolCall,
        metadata: &Value,
    ) -> Result<(), ApiError> {
        for (event_type, payload) in shell_tool_result_events(tool_call, metadata) {
            self.record_event(
                tool_call.session_id.as_deref().unwrap_or("session.default"),
                tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
                tool_call.task_id.as_deref(),
                event_type,
                payload,
            )?;
        }
        Ok(())
    }

    fn record_shell_tool_failure(&self, tool_call: &ToolCall, error: &str) -> Result<(), ApiError> {
        let Some((event_type, payload)) = shell_tool_failure_event(tool_call, error) else {
            return Ok(());
        };
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
            tool_call.task_id.as_deref(),
            event_type,
            payload,
        )
    }

    fn spawn_waiting_task_scheduler(&self) {
        let daemon = self.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                loop {
                    daemon.resume_ready_waiting_tasks().await;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            });
        }
    }

    async fn resume_ready_waiting_tasks(&self) {
        let now = Utc::now();
        let records = match self.store.list_tasks() {
            Ok(records) => records,
            Err(error) => {
                self.push_log(format!("waiting task scan failed: {error}"));
                return;
            }
        };

        for record in records {
            if record.task.status != TaskStatus::Waiting {
                continue;
            }
            let Some(waiting_until) = record.task.waiting_until else {
                continue;
            };
            if waiting_until > now {
                continue;
            }
            let task_id = record.task.task_id.clone();
            {
                let mut control = self.control.lock().expect("control plane lock poisoned");
                if !control.resuming_tasks.insert(task_id.clone()) {
                    continue;
                }
            }
            let daemon = self.clone();
            tokio::spawn(async move {
                let _ = daemon.resume_waiting_task(task_id.clone()).await;
                daemon
                    .control
                    .lock()
                    .expect("control plane lock poisoned")
                    .resuming_tasks
                    .remove(&task_id);
            });
        }
    }

    async fn resume_waiting_task(&self, task_id: String) -> Result<(), ApiError> {
        let record = self
            .store
            .get_task(&task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        if record.task.status != TaskStatus::Waiting {
            return Ok(());
        }
        let session_id = record.task.session_id.clone().ok_or_else(|| {
            ApiError::new(
                ApiErrorCode::InternalError,
                "waiting task missing session_id",
                false,
            )
        })?;
        let session_record = self
            .store
            .get_session(&session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        let session_agent = self
            .app
            .runtime
            .session_agent(
                session_record.session.current_provider_id.as_deref(),
                session_record.session.current_model_id.as_deref(),
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("failed to resolve session model: {error}"),
                    false,
                )
            })?;
        let turn_id = self.store.next_turn_id();

        let mut task = record.task.clone();
        let resume_reason = task.waiting_reason.clone();
        let resume_hint = task.waiting_resume_hint.clone();
        task.status = TaskStatus::Running;
        task.waiting_until = None;
        task.waiting_reason = None;
        task.waiting_resume_hint = None;
        task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Running,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                "waiting task resumed by runtime scheduler",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskResumed,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "reason": resume_reason,
                "resume_hint": resume_hint,
            }),
        )?;

        let mut resume_message = finalize_message(
            SessionMessage::runtime(format!(
                "Resume the waiting task now. {} {}",
                resume_reason
                    .as_deref()
                    .map(|value| format!("Reason: {value}."))
                    .unwrap_or_default(),
                resume_hint
                    .as_deref()
                    .map(|value| format!("Next step hint: {value}."))
                    .unwrap_or_default()
            )),
            &session_record.session,
            self.store.next_message_id(),
            Some(vec![SessionMessageAnnotation {
                kind: "runtime_control".into(),
                value: "task_resumed".into(),
            }]),
        );
        resume_message.meta.actor_id = Some(session_agent.agent_id.clone());
        self.store
            .append_message(&session_id, &turn_id, resume_message.clone())
            .map_err(map_store_error)?;

        let assembled_context = self
            .app
            .context_engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some(session_id.clone()),
                task_id: Some(task_id.clone()),
                budget_tokens: 8_000,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
                retrieval_scope: crate::builtin::context::retrieval_types::RetrievalScope::Implicit,
            })
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("context assembly failed during resume: {error}"),
                    false,
                )
            })?;
        let prompt_messages = recent_prompt_messages(
            &self
                .store
                .get_session(&session_id)
                .map_err(internal_store_error)?
                .ok_or_else(session_not_found)?
                .messages,
            8,
        );
        let outcome = self
            .run_tool_loop(
                &session_id,
                &task_id,
                &turn_id,
                &session_agent,
                &session_record.session,
                &assembled_context,
                prompt_messages,
                None,
            )
            .await?;

        match outcome {
            ToolLoopOutcome::Final(text) => {
                let mut assistant_message = finalize_message(
                    SessionMessage::assistant(text),
                    &session_record.session,
                    self.store.next_message_id(),
                    None,
                );
                assistant_message.meta.actor_id = Some(session_agent.agent_id.clone());
                self.store
                    .append_message(&session_id, &turn_id, assistant_message.clone())
                    .map_err(map_store_error)?;
                let checkpoint = self
                    .app
                    .context_engine
                    .build_resume_pack(Some(&session_id), Some(&task_id))
                    .map_err(|error| {
                        ApiError::new(
                            ApiErrorCode::InternalError,
                            format!("resume pack build failed during resume completion: {error}"),
                            false,
                        )
                    })?;
                self.store
                    .write_task_checkpoint(
                        &task_id,
                        Some(&session_id),
                        Some(&turn_id),
                        assistant_message.content.clone(),
                        checkpoint,
                    )
                    .map_err(internal_store_error)?;
                task.status = TaskStatus::Succeeded;
                task.checkpoint_ref = self
                    .store
                    .get_task(&task_id)
                    .map_err(internal_store_error)?
                    .and_then(|record| record.task.checkpoint_ref);
                task.meta.updated_at = Utc::now();
                self.store
                    .update_task(task.clone())
                    .map_err(internal_store_error)?;
                self.store
                    .append_task_timeline(
                        &task_id,
                        crate::domain::TaskPhase::Succeeded,
                        TaskStatus::Succeeded,
                        Some(&turn_id),
                        None,
                        "resumed task completed successfully",
                    )
                    .map_err(internal_store_error)?;
                self.record_event(
                    &session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TaskSucceeded,
                    json!({
                        "task_id": task_id,
                        "checkpoint_ref": task.checkpoint_ref,
                        "resumed": true,
                    }),
                )?;
                self.record_event(
                    &session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnSucceeded,
                    json!({
                        "turn_id": turn_id,
                        "task_id": task_id,
                        "assistant_message": assistant_message,
                        "resumed": true,
                    }),
                )?;
            }
            ToolLoopOutcome::Sleeping(runtime_message) => {
                let mut runtime_message = finalize_message(
                    SessionMessage::runtime(runtime_message),
                    &session_record.session,
                    self.store.next_message_id(),
                    Some(vec![SessionMessageAnnotation {
                        kind: "runtime_control".into(),
                        value: "sleep".into(),
                    }]),
                );
                runtime_message.meta.actor_id = Some(session_agent.agent_id.clone());
                self.store
                    .append_message(&session_id, &turn_id, runtime_message.clone())
                    .map_err(map_store_error)?;
                self.record_event(
                    &session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnSucceeded,
                    json!({
                        "turn_id": turn_id,
                        "task_id": task_id,
                        "runtime_message": runtime_message,
                        "waiting": true,
                        "resumed": true,
                    }),
                )?;
            }
        }

        Ok(())
    }
}

fn shell_tool_started_event(tool_call: &ToolCall) -> Option<(EventType, Value)> {
    match tool_call.tool_name.as_str() {
        "shell_exec" => Some((
            EventType::ShellExecutionStarted,
            json!({
                "tool_name": tool_call.tool_name,
                "mode": "stateless",
                "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                "cwd": tool_call.args.get("cwd").and_then(|value| value.as_str()),
                "timeout_secs": tool_call.args.get("timeout_secs"),
            }),
        )),
        "shell_session_exec" => Some((
            EventType::ShellExecutionStarted,
            json!({
                "tool_name": tool_call.tool_name,
                "mode": "session_bound",
                "session_id": tool_call.args.get("session_id").and_then(|value| value.as_str()),
                "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                "timeout_secs": tool_call.args.get("timeout_secs"),
                "detach": tool_call.args.get("detach").and_then(|value| value.as_bool()).unwrap_or(false),
            }),
        )),
        _ => None,
    }
}

fn shell_tool_result_events(tool_call: &ToolCall, metadata: &Value) -> Vec<(EventType, Value)> {
    let mut events = Vec::new();
    match tool_call.tool_name.as_str() {
        "shell_exec" => {
            let exit_code = metadata.get("exit_code").and_then(|value| value.as_i64());
            let timed_out = metadata
                .get("timed_out")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let event_type = if timed_out || exit_code.unwrap_or_default() != 0 {
                EventType::ShellExecutionFailed
            } else {
                EventType::ShellExecutionCompleted
            };
            events.push((
                event_type,
                json!({
                    "mode": "stateless",
                    "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                    "cwd": metadata.get("cwd"),
                    "exit_code": metadata.get("exit_code"),
                    "timed_out": timed_out,
                    "truncated": metadata.get("truncated"),
                }),
            ));
        }
        "shell_session_open" => events.push((EventType::ShellSessionOpened, metadata.clone())),
        "shell_session_read" => {
            if let Some(chunks) = metadata.get("chunks").and_then(|value| value.as_array()) {
                if !chunks.is_empty() {
                    events.push((
                        EventType::ShellOutputAppended,
                        json!({
                            "session_id": metadata.get("session_id"),
                            "seq": metadata.get("seq"),
                            "chunk_count": chunks.len(),
                            "chunks": chunks,
                        }),
                    ));
                }
            }
            if let Some(completed_exec) = metadata.get("completed_exec") {
                let event_type = match completed_exec
                    .get("status")
                    .and_then(|value| value.as_str())
                {
                    Some("interrupted") => EventType::ShellExecutionInterrupted,
                    Some("failed") | Some("timed_out") => EventType::ShellExecutionFailed,
                    _ => EventType::ShellExecutionCompleted,
                };
                events.push((event_type, completed_exec.clone()));
            }
        }
        "shell_session_close" => events.push((EventType::ShellSessionClosed, metadata.clone())),
        "shell_session_interrupt" => {
            if metadata
                .get("signaled")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                events.push((EventType::ShellExecutionInterrupted, metadata.clone()));
            }
        }
        "shell_session_resize" => {
            if metadata
                .get("resized")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                events.push((EventType::ShellSessionResized, metadata.clone()));
            }
        }
        _ => {}
    }
    events
}

fn shell_tool_failure_event(tool_call: &ToolCall, error: &str) -> Option<(EventType, Value)> {
    match tool_call.tool_name.as_str() {
        "shell_exec" | "shell_session_exec" => Some((
            EventType::ShellExecutionFailed,
            json!({
                "tool_name": tool_call.tool_name,
                "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                "session_id": tool_call.args.get("session_id").and_then(|value| value.as_str()),
                "error": error,
            }),
        )),
        _ => None,
    }
}

trait StreamEnvelopeExt {
    fn stream_id(&self) -> Option<&str>;
}

impl StreamEnvelopeExt for ServerEnvelope {
    fn stream_id(&self) -> Option<&str> {
        match self {
            ServerEnvelope::Stream(stream) => Some(stream.stream_id.0.as_str()),
            _ => None,
        }
    }
}

fn session_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::SessionNotFound, "session not found", false)
}

fn task_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::TaskNotFound, "task not found", false)
}

fn agent_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::AgentNotFound, "agent not found", false)
}

fn plugin_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::PluginNotFound, "plugin not found", false)
}

fn node_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::NodeNotFound, "node not found", false)
}

fn schedule_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::ScheduleNotFound, "schedule not found", false)
}

fn subscription_not_found() -> ApiError {
    ApiError::new(
        ApiErrorCode::SubscriptionNotFound,
        "subscription not found",
        false,
    )
}

fn stream_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::StreamNotFound, "stream not found", false)
}

fn internal_store_error(error: anyhow::Error) -> ApiError {
    ApiError::new(
        ApiErrorCode::InternalError,
        format!("store operation failed: {error}"),
        false,
    )
}

fn map_store_error(error: anyhow::Error) -> ApiError {
    if error.to_string().contains("session not found") {
        session_not_found()
    } else {
        internal_store_error(error)
    }
}

fn build_context_prompt(
    app: &Application,
    assembled: &AssembledContext,
    conversation_messages: Vec<SessionMessage>,
    allow_tool_calls: bool,
) -> String {
    render_prompt_xml(PromptRenderRequest {
        prompt_documents: parse_workspace_prompt_documents(&app.workspace_identity),
        assembled_context: assembled.clone(),
        tools: app.tool_registry.descriptors(),
        conversation_messages,
        allow_tool_calls,
    })
}

fn build_tool_followup_prompt(
    app: &Application,
    assembled: &AssembledContext,
    conversation_messages: Vec<SessionMessage>,
) -> String {
    build_context_prompt(app, assembled, conversation_messages, true)
}

enum ToolLoopOutcome {
    Final(String),
    Sleeping(String),
}

struct SemanticStreamBuilder {
    stream_id: String,
    seq: u64,
    envelopes: Vec<ServerEnvelope>,
}

impl SemanticStreamBuilder {
    fn new(stream_id: String, turn_id: &str) -> Self {
        let mut builder = Self {
            stream_id: stream_id.clone(),
            seq: 0,
            envelopes: Vec::new(),
        };
        builder.push("turn.started", json!({ "turn_id": turn_id }));
        builder
    }

    fn push(&mut self, event: &str, data: Value) {
        self.envelopes.push(ServerEnvelope::Stream(StreamEnvelope {
            stream_id: self.stream_id.clone().into(),
            phase: if self.seq == 0 {
                StreamPhase::Start
            } else {
                StreamPhase::Chunk
            },
            event: event.into(),
            seq: self.seq,
            data,
            meta: None,
        }));
        self.seq += 1;
    }

    fn push_text(&mut self, turn_id: &str, text: &str) {
        for chunk in chunk_stream_text(text) {
            self.push(
                "assistant.text.delta",
                json!({
                    "turn_id": turn_id,
                    "text": chunk,
                }),
            );
        }
    }

    fn finish(mut self, turn_id: &str, status: &str) -> Vec<ServerEnvelope> {
        self.push(
            "turn.completed",
            json!({
                "turn_id": turn_id,
                "status": status,
            }),
        );
        self.envelopes.push(ServerEnvelope::Stream(StreamEnvelope {
            stream_id: self.stream_id.into(),
            phase: StreamPhase::End,
            event: "stream.completed".into(),
            seq: self.seq,
            data: json!({ "turn_id": turn_id, "done": true, "status": status }),
            meta: None,
        }));
        self.envelopes
    }
}

#[derive(Debug, Clone)]
struct SleepDirective {
    wake_at: chrono::DateTime<chrono::Utc>,
    reason: Option<String>,
    resume_hint: Option<String>,
    duration_ms: Option<i64>,
}

impl Daemon {
    #[allow(clippy::too_many_arguments)]
    async fn run_tool_loop(
        &self,
        session_id: &str,
        task_id: &str,
        turn_id: &str,
        session_agent: &crate::config::AgentDefinition,
        _session: &Session,
        assembled_context: &AssembledContext,
        conversation_messages: Vec<SessionMessage>,
        mut stream: Option<&mut SemanticStreamBuilder>,
    ) -> Result<ToolLoopOutcome, ApiError> {
        let mut loop_messages = conversation_messages;

        for iteration in 0..=MAX_TOOL_LOOP_STEPS {
            let prompt =
                build_tool_followup_prompt(&self.app, assembled_context, loop_messages.clone());
            self.record_event(
                session_id,
                turn_id,
                Some(task_id),
                EventType::ModelCalled,
                json!({
                    "provider_id": session_agent.provider_id.clone(),
                    "model_id": session_agent.model.clone(),
                    "phase": if iteration == 0 { "initial" } else { "tool_loop" },
                    "iteration": iteration,
                }),
            )?;
            let response = self
                .app
                .runtime
                .prompt_turn(AgentPromptRequest {
                    prompt,
                    agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
                    agent_override: Some(session_agent.clone()),
                    tools: self.app.tool_registry.descriptors(),
                })
                .await
                .map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("model call failed in tool loop: {error}"),
                        false,
                    )
                })?;
            self.record_event(
                session_id,
                turn_id,
                Some(task_id),
                EventType::ModelResponseReceived,
                json!({
                    "output_id": response.output_id,
                    "items": response.items,
                    "finish_reason": response.finish_reason,
                    "usage": response.usage,
                    "iteration": iteration,
                }),
            )?;

            let assistant_text = response.assistant_text();
            if let Some(stream) = stream.as_deref_mut() {
                if !assistant_text.trim().is_empty() {
                    stream.push_text(turn_id, &assistant_text);
                }
            }
            let tool_calls = collect_tool_calls(
                &response,
                session_id,
                task_id,
                turn_id,
                &session_agent.agent_id,
            );

            if tool_calls.is_empty() {
                return Ok(ToolLoopOutcome::Final(assistant_text));
            }

            if !assistant_text.trim().is_empty() {
                loop_messages.push(SessionMessage::assistant(assistant_text.clone()));
            }

            for tool_call in tool_calls {
                self.record_event(
                    session_id,
                    turn_id,
                    Some(task_id),
                    EventType::ToolCallRequested,
                    json!({
                        "tool_call_id": tool_call.tool_call_id,
                        "tool_name": tool_call.tool_name,
                        "args": tool_call.args,
                        "timeout_secs": tool_call.timeout_secs,
                        "iteration": iteration,
                    }),
                )?;
                if let Some(stream) = stream.as_deref_mut() {
                    stream.push(
                        "tool_call.started",
                        json!({
                            "turn_id": turn_id,
                            "tool_call_id": tool_call.tool_call_id,
                            "tool_name": tool_call.tool_name,
                            "args": tool_call.args,
                        }),
                    );
                }
                let tool_result =
                    execute_tool_call_item(&self.execute_tool_call(&tool_call).await?, &tool_call);
                let mut tool_result_message = SessionMessage::tool_result(tool_result.content.clone());
                tool_result_message.annotations = vec![
                    SessionMessageAnnotation {
                        kind: "tool_name".into(),
                        value: tool_call.tool_name.clone(),
                    },
                    SessionMessageAnnotation {
                        kind: "tool_call_id".into(),
                        value: tool_call.tool_call_id.clone(),
                    },
                    SessionMessageAnnotation {
                        kind: "tool_error".into(),
                        value: tool_result.is_error.to_string(),
                    },
                ];
                loop_messages.push(tool_result_message);
                if let Some(stream) = stream.as_deref_mut() {
                    stream.push(
                        "tool_call.completed",
                        json!({
                            "turn_id": turn_id,
                            "tool_call_id": tool_call.tool_call_id,
                            "tool_name": tool_call.tool_name,
                            "content": tool_result.content,
                            "metadata": tool_result.metadata,
                            "is_error": tool_result.is_error,
                        }),
                    );
                }
                if let Some(sleep) = parse_sleep_directive(&tool_call, &tool_result)? {
                    self.apply_sleep_directive(session_id, task_id, turn_id, sleep.clone())
                        .await?;
                    return Ok(ToolLoopOutcome::Sleeping(format!(
                        "Runtime scheduled resume at {}. {}",
                        sleep.wake_at.to_rfc3339(),
                        sleep
                            .resume_hint
                            .unwrap_or_else(|| "The task is now waiting.".into())
                    )));
                }
            }
        }

        Err(ApiError::new(
            ApiErrorCode::InternalError,
            format!("tool loop exceeded max iterations ({MAX_TOOL_LOOP_STEPS})"),
            false,
        ))
    }

    async fn apply_sleep_directive(
        &self,
        session_id: &str,
        task_id: &str,
        turn_id: &str,
        sleep: SleepDirective,
    ) -> Result<(), ApiError> {
        self.record_event(
            session_id,
            turn_id,
            Some(task_id),
            EventType::SleepRequested,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "wake_at": sleep.wake_at,
                "reason": sleep.reason,
                "resume_hint": sleep.resume_hint,
                "duration_ms": sleep.duration_ms,
            }),
        )?;

        let checkpoint = self
            .app
            .context_engine
            .build_resume_pack(Some(session_id), Some(task_id))
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("resume pack build failed: {error}"),
                    false,
                )
            })?;
        self.store
            .write_task_checkpoint(
                task_id,
                Some(session_id),
                Some(turn_id),
                sleep
                    .resume_hint
                    .clone()
                    .unwrap_or_else(|| format!("wake at {}", sleep.wake_at.to_rfc3339())),
                checkpoint,
            )
            .map_err(internal_store_error)?;

        let mut task = self
            .store
            .get_task(task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        task.task.status = TaskStatus::Waiting;
        task.task.waiting_until = Some(sleep.wake_at);
        task.task.waiting_reason = sleep.reason.clone();
        task.task.waiting_resume_hint = sleep.resume_hint.clone();
        task.task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                task_id,
                crate::domain::TaskPhase::Waiting,
                TaskStatus::Waiting,
                Some(turn_id),
                None,
                format!(
                    "runtime sleep scheduled until {} ({})",
                    sleep.wake_at.to_rfc3339(),
                    sleep.reason.unwrap_or_else(|| "no reason provided".into())
                ),
            )
            .map_err(internal_store_error)?;
        self.record_event(
            session_id,
            turn_id,
            Some(task_id),
            EventType::TaskWaiting,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "wake_at": sleep.wake_at,
                "resume_hint": sleep.resume_hint,
            }),
        )?;
        Ok(())
    }
}

fn parse_sleep_directive(
    tool_call: &ToolCall,
    tool_result: &ToolResultItem,
) -> Result<Option<SleepDirective>, ApiError> {
    if tool_call.tool_name != "sleep" {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&tool_result.content).map_err(|error| {
        ApiError::new(
            ApiErrorCode::InternalError,
            format!("invalid sleep tool output: {error}"),
            false,
        )
    })?;
    let wake_at = value
        .get("wake_at")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ApiError::new(
                ApiErrorCode::InternalError,
                "sleep tool output missing wake_at",
                false,
            )
        })?;
    let wake_at = chrono::DateTime::parse_from_rfc3339(wake_at)
        .map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("invalid sleep wake_at: {error}"),
                false,
            )
        })?
        .with_timezone(&Utc);
    Ok(Some(SleepDirective {
        wake_at,
        reason: value
            .get("reason")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        resume_hint: value
            .get("resume_hint")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        duration_ms: value.get("duration_ms").and_then(|value| value.as_i64()),
    }))
}

fn collect_tool_calls(
    output: &ModelTurnOutput,
    session_id: &str,
    task_id: &str,
    turn_id: &str,
    agent_id: &str,
) -> Vec<ToolCall> {
    output
        .items
        .iter()
        .filter_map(|item| match item {
            ModelOutputItem::ToolCall(tool_call) => Some(to_runtime_tool_call(
                tool_call, session_id, task_id, turn_id, agent_id,
            )),
            _ => None,
        })
        .collect()
}

fn to_runtime_tool_call(
    item: &ToolCallItem,
    session_id: &str,
    task_id: &str,
    turn_id: &str,
    agent_id: &str,
) -> ToolCall {
    ToolCall {
        tool_call_id: item.tool_call_id.clone(),
        tool_name: item.tool_name.clone(),
        args: item.args.clone(),
        requested_by: ToolCaller::Agent {
            agent_id: agent_id.into(),
        },
        session_id: Some(session_id.into()),
        task_id: Some(task_id.into()),
        turn_id: Some(turn_id.into()),
        idempotency_key: Some(format!("{turn_id}:{}", item.tool_call_id)),
        timeout_secs: item.timeout_secs,
    }
}

fn execute_tool_call_item(content: &str, tool_call: &ToolCall) -> ToolResultItem {
    ToolResultItem {
        item_id: format!("tool_result_{}", tool_call.tool_call_id),
        tool_call_id: tool_call.tool_call_id.clone(),
        tool_name: tool_call.tool_name.clone(),
        content: content.to_string(),
        metadata: json!({ "ok": true }),
        is_error: false,
    }
}

fn recent_prompt_messages(messages: &[SessionMessage], limit: usize) -> Vec<SessionMessage> {
    messages
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn finalize_message(
    mut message: SessionMessage,
    session: &Session,
    message_id: String,
    annotations: Option<Vec<SessionMessageAnnotation>>,
) -> SessionMessage {
    if message.role.is_none() {
        message.role = Some(message.normalized_kind().as_role_str().into());
    }
    if message.meta.message_id.is_none() {
        message.meta.message_id = Some(message_id);
    }
    if message.meta.session_id.is_none() {
        message.meta.session_id = Some(session.session_id.clone());
    }
    if message.meta.channel.is_none() {
        message.meta.channel = session.channel_id.clone();
    }
    if message.meta.surface.is_none() {
        message.meta.surface = session.surface_id.clone();
    }
    if message.meta.actor_id.is_none() {
        message.meta.actor_id = session.user_id.clone();
    }
    if message.meta.timestamp.is_none() {
        message.meta.timestamp = Some(Utc::now());
    }
    if let Some(annotations) = annotations {
        message.annotations.extend(annotations);
    }
    message
}

#[cfg(test)]
fn build_stream_envelopes(stream_id: &str, turn_id: &str, content: &str) -> Vec<ServerEnvelope> {
    let mut followups = Vec::new();
    followups.push(ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::Start,
        event: "session.output".into(),
        seq: 0,
        data: json!({ "turn_id": turn_id }),
        meta: None,
    }));

    let text_chunks = chunk_stream_text(content);
    for (index, chunk) in text_chunks.iter().enumerate() {
        followups.push(ServerEnvelope::Stream(StreamEnvelope {
            stream_id: stream_id.into(),
            phase: StreamPhase::Chunk,
            event: "token".into(),
            seq: index as u64 + 1,
            data: json!({ "text": chunk }),
            meta: None,
        }));
    }

    followups.push(ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::End,
        event: "session.output".into(),
        seq: text_chunks.len() as u64 + 1,
        data: json!({ "turn_id": turn_id, "done": true }),
        meta: None,
    }));
    followups
}

fn chunk_stream_text(content: &str) -> Vec<String> {
    const CHUNK_CHARS: usize = 96;

    if content.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();

    for segment in content.split_inclusive('\n') {
        let mut buffer = String::new();
        let mut count = 0usize;

        for ch in segment.chars() {
            buffer.push(ch);
            count += 1;
            if ch == '\n' || count >= CHUNK_CHARS {
                chunks.push(std::mem::take(&mut buffer));
                count = 0;
            }
        }

        if !buffer.is_empty() {
            chunks.push(buffer);
        }
    }

    chunks
}

fn build_log_stream_envelopes(stream_id: &str, lines: &[String]) -> Vec<ServerEnvelope> {
    let mut followups = vec![ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::Start,
        event: "logs.tail".into(),
        seq: 0,
        data: json!({ "source": "daemon.logs" }),
        meta: None,
    })];

    for (index, line) in lines.iter().enumerate() {
        followups.push(ServerEnvelope::Stream(StreamEnvelope {
            stream_id: stream_id.into(),
            phase: StreamPhase::Chunk,
            event: "log.line".into(),
            seq: index as u64 + 1,
            data: json!({ "line": line }),
            meta: None,
        }));
    }

    followups.push(ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::End,
        event: "logs.tail".into(),
        seq: lines.len() as u64 + 1,
        data: json!({ "done": true }),
        meta: None,
    }));
    followups
}

fn schedule_kind(schedule: &Schedule) -> String {
    match &schedule.trigger {
        crate::domain::TaskTrigger::Cron { .. } => "cron",
        crate::domain::TaskTrigger::Interval { .. } => "interval",
        crate::domain::TaskTrigger::Event { .. } => "event",
        crate::domain::TaskTrigger::Manual => "manual",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::SocketAddr,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::Utc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    use crate::{
        api::{
            ApiMethod, PluginInspectResponse, PluginReloadResponse, PluginTestResponse,
            RequestEnvelope, RequestId, ScheduleCreateRequest, ScheduleUpdateRequest,
            SessionModelInspectResponse, SessionModelSwitchResponse, SessionModelSwitchResult,
            SessionSendResponse, SmokeRunResponse, TaskGetResponse,
        },
        app::Application,
        config::{
            ConfigRoot, ModelCatalogSnapshot, ModelInfoSnapshot, OpenAiProviderConfig,
            ProviderModelCatalog, RuntimeConfig, WorkspaceDocument, WorkspaceIdentityPack,
            WorkspacePaths,
        },
        domain::{ExecutionMode, ObjectMeta, Schedule, Task, TaskStatus},
    };

    use super::{
        build_stream_envelopes, chunk_stream_text, collect_tool_calls, Daemon, SleepDirective,
    };

    #[tokio::test]
    async fn session_send_runs_tool_loop_and_records_tool_events() {
        let root = temp_path("tool-loop");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();
        let sample_dir = root.join("sample");
        fs::create_dir_all(&sample_dir).unwrap();
        fs::write(sample_dir.join("a.txt"), "hello").unwrap();

        let server = spawn_server(vec![
            (
                "POST /v1/responses HTTP/1.1".to_string(),
                vec![
                    "<agentjax_prompt version=\\\"v1\\\">".to_string(),
                    "<tools>".to_string(),
                    "<message kind=\\\"user\\\">".to_string(),
                    "<content>show files</content>".to_string(),
                    "\"type\":\"function\"".to_string(),
                    "\"tool_choice\":\"auto\"".to_string(),
                ],
                format!(
                    r#"{{"id":"resp_1","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{{"id":"fc_1","type":"function_call","call_id":"fc_1_read","name":"read","arguments":"{{\"path\":\"{}\",\"start_line\":1,\"end_line\":1}}"}}],"tools":[]}}"#,
                    sample_dir.join("a.txt").display()
                ),
            ),
            (
                "POST /v1/responses HTTP/1.1".to_string(),
                vec![
                    "<message kind=\\\"tool_result\\\">".to_string(),
                    "tool_name".to_string(),
                    sample_dir.join("a.txt").display().to_string(),
                ],
                r#"{"id":"resp_2","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{"id":"msg_2","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"final answer after tool","annotations":[]}]}],"tools":[]}"#.to_string(),
            ),
        ])
        .await;

        let mut runtime_config = RuntimeConfig::new(
            "agentjax-test",
            crate::config::RuntimePaths::new(root.join("runtime")),
            crate::config::WorkspaceConfig::new(
                "default-workspace",
                WorkspacePaths::new(&workspace_root),
            ),
        );
        runtime_config.agent_runtime.llm.providers =
            vec![crate::config::LlmProviderConfig::OpenAi(
                OpenAiProviderConfig {
                    provider_id: "openai-default".into(),
                    api_key: Some("test-key".into()),
                    api_key_env: "OPENAI_API_KEY".into(),
                    base_url: Some(format!("http://{}", server.0)),
                    organization: None,
                    project: None,
                },
            )];
        let identity = WorkspaceIdentityPack {
            workspace_id: "default-workspace".into(),
            agent: doc(workspace_root.join("AGENT.md"), ""),
            soul: doc(workspace_root.join("SOUL.md"), ""),
            user: doc(workspace_root.join("USER.md"), ""),
            memory: doc(workspace_root.join("MEMORY.md"), ""),
            mission: doc(workspace_root.join("MISSION.md"), ""),
            rules: doc(workspace_root.join("RULES.md"), ""),
            router: doc(workspace_root.join("ROUTER.md"), ""),
        };
        let app = Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config,
            identity,
        )
        .unwrap();
        let daemon = Daemon::new(app).unwrap();

        let send = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_send".into()),
                method: ApiMethod::SessionSend,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "message": { "role": "user", "content": "show files" },
                    "stream": false,
                }),
                meta: None,
            })
            .await;
        assert!(matches!(
            send.response,
            crate::api::ServerEnvelope::Response(_)
        ));

        let get = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_get".into()),
                method: ApiMethod::SessionGet,
                params: serde_json::json!({ "session_id": "session.default" }),
                meta: None,
            })
            .await;

        let crate::api::ServerEnvelope::Response(response) = get.response else {
            panic!("expected response envelope");
        };
        let result = response.result.unwrap();
        let session: crate::api::SessionGetResponse = serde_json::from_value(result).unwrap();
        assert!(session
            .events
            .iter()
            .any(|event| event.event_type == crate::domain::EventType::ToolCalled));
        assert!(session
            .events
            .iter()
            .any(|event| event.event_type == crate::domain::EventType::ToolCompleted));

        server.1.abort();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn session_send_streams_semantic_events() {
        let root = temp_path("tool-loop-stream");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();
        let sample_dir = root.join("sample");
        fs::create_dir_all(&sample_dir).unwrap();
        fs::write(sample_dir.join("a.txt"), "hello").unwrap();

        let server = spawn_server(vec![
            (
                "POST /v1/responses HTTP/1.1".to_string(),
                vec![
                    "\"type\":\"function\"".to_string(),
                    "\"tool_choice\":\"auto\"".to_string(),
                ],
                format!(
                    r#"{{"id":"resp_1","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{{"id":"msg_1","type":"function_call","call_id":"msg_1_call_0_read","name":"read","arguments":"{{\"path\":\"{}\",\"start_line\":1,\"end_line\":1}}"}}],"tools":[]}}"#,
                    sample_dir.join("a.txt").display()
                ),
            ),
            (
                "POST /v1/responses HTTP/1.1".to_string(),
                vec![
                    "<message kind=\\\"tool_result\\\">".to_string(),
                    "tool_name".to_string(),
                ],
                r#"{"id":"resp_2","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{"id":"msg_2","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"final answer after tool","annotations":[]}]}],"tools":[]}"#.to_string(),
            ),
        ])
        .await;

        let mut runtime_config = RuntimeConfig::new(
            "agentjax-test",
            crate::config::RuntimePaths::new(root.join("runtime")),
            crate::config::WorkspaceConfig::new(
                "default-workspace",
                WorkspacePaths::new(&workspace_root),
            ),
        );
        runtime_config.agent_runtime.llm.providers =
            vec![crate::config::LlmProviderConfig::OpenAi(
                OpenAiProviderConfig {
                    provider_id: "openai-default".into(),
                    api_key: Some("test-key".into()),
                    api_key_env: "OPENAI_API_KEY".into(),
                    base_url: Some(format!("http://{}", server.0)),
                    organization: None,
                    project: None,
                },
            )];
        let identity = WorkspaceIdentityPack {
            workspace_id: "default-workspace".into(),
            agent: doc(workspace_root.join("AGENT.md"), ""),
            soul: doc(workspace_root.join("SOUL.md"), ""),
            user: doc(workspace_root.join("USER.md"), ""),
            memory: doc(workspace_root.join("MEMORY.md"), ""),
            mission: doc(workspace_root.join("MISSION.md"), ""),
            rules: doc(workspace_root.join("RULES.md"), ""),
            router: doc(workspace_root.join("ROUTER.md"), ""),
        };
        let app = Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config,
            identity,
        )
        .unwrap();
        let daemon = Daemon::new(app).unwrap();

        let send = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_send_stream".into()),
                method: ApiMethod::SessionSend,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "message": { "role": "user", "content": "show files" },
                    "stream": true,
                }),
                meta: None,
            })
            .await;

        let events = send
            .followups
            .iter()
            .filter_map(|envelope| match envelope {
                crate::api::ServerEnvelope::Stream(stream) => Some(stream.event.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(events.contains(&"turn.started"));
        assert!(events.contains(&"tool_call.started"));
        assert!(events.contains(&"tool_call.completed"));
        assert!(events.contains(&"assistant.text.delta"));
        assert!(events.contains(&"assistant.completed"));
        assert!(events.contains(&"turn.completed"));

        server.1.abort();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stream_chunking_preserves_newlines_and_content() {
        let content = "line one\n- bullet a\n- bullet b\n\nparagraph two";
        let chunks = chunk_stream_text(content);
        assert!(chunks.iter().any(|chunk| chunk.contains('\n')));
        assert_eq!(chunks.concat(), content);

        let envelopes = build_stream_envelopes("stream_1", "turn_1", content);
        let streamed = envelopes
            .iter()
            .filter_map(|envelope| match envelope {
                crate::api::ServerEnvelope::Stream(stream)
                    if matches!(stream.phase, crate::api::StreamPhase::Chunk) =>
                {
                    stream.data.get("text").and_then(|value| value.as_str())
                }
                _ => None,
            })
            .collect::<String>();
        assert_eq!(streamed, content);
    }

    #[test]
    fn collects_multiple_structured_tool_calls() {
        let output = crate::domain::ModelTurnOutput {
            output_id: "out_1".into(),
            items: vec![
                crate::domain::ModelOutputItem::ToolCall(crate::domain::ToolCallItem {
                    item_id: "item_1".into(),
                    tool_call_id: "call_1".into(),
                    tool_name: "read".into(),
                    args: serde_json::json!({ "path": "Cargo.toml" }),
                    timeout_secs: Some(5),
                }),
                crate::domain::ModelOutputItem::ToolCall(crate::domain::ToolCallItem {
                    item_id: "item_2".into(),
                    tool_call_id: "call_2".into(),
                    tool_name: "memory_search".into(),
                    args: serde_json::json!({ "query": "defaults" }),
                    timeout_secs: None,
                }),
            ],
            finish_reason: crate::domain::FinishReason::ToolCalls,
            usage: None,
        };

        let tool_calls = collect_tool_calls(
            &output,
            "session.default",
            "task_1",
            "turn_1",
            "agent.default",
        );
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].tool_call_id, "call_1");
        assert_eq!(tool_calls[1].tool_call_id, "call_2");
    }

    #[tokio::test]
    async fn session_persistence_survives_daemon_restart() {
        let root = temp_path("sqlite-persistence");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let server = spawn_server(vec![(
            "POST /v1/responses HTTP/1.1".to_string(),
            vec![
                "<agentjax_prompt version=\\\"v1\\\">".to_string(),
                "<message kind=\\\"user\\\">".to_string(),
                "<content>persist this session</content>".to_string(),
            ],
            r#"{"id":"resp_persist","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{"id":"msg_persist","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"persistent assistant reply","annotations":[]}]}],"tools":[]}"#.to_string(),
        )])
        .await;

        let runtime_config =
            test_runtime_config(&root, &workspace_root, Some(format!("http://{}", server.0)));
        let identity = test_identity(&workspace_root);

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config.clone(),
                identity.clone(),
            )
            .unwrap(),
        )
        .unwrap();

        let send = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_persist_send".into()),
                method: ApiMethod::SessionSend,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "message": { "role": "user", "content": "persist this session" },
                    "stream": false,
                }),
                meta: None,
            })
            .await;
        assert!(matches!(
            send.response,
            crate::api::ServerEnvelope::Response(_)
        ));

        drop(daemon);
        server.1.abort();

        let restarted = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config,
                identity,
            )
            .unwrap(),
        )
        .unwrap();

        let list = restarted
            .handle_request(RequestEnvelope {
                id: RequestId("req_persist_list".into()),
                method: ApiMethod::SessionList,
                params: serde_json::json!({}),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(list_response) = list.response else {
            panic!("expected response envelope");
        };
        let listed: crate::api::SessionListResponse =
            serde_json::from_value(list_response.result.unwrap()).unwrap();
        assert!(listed
            .items
            .iter()
            .any(|item| item.session_id == "session.default"));

        let get = restarted
            .handle_request(RequestEnvelope {
                id: RequestId("req_persist_get".into()),
                method: ApiMethod::SessionGet,
                params: serde_json::json!({ "session_id": "session.default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(get_response) = get.response else {
            panic!("expected response envelope");
        };
        let session: crate::api::SessionGetResponse =
            serde_json::from_value(get_response.result.unwrap()).unwrap();
        assert!(session
            .messages
            .iter()
            .any(|message| message.content == "persist this session"));
        assert!(session
            .messages
            .iter()
            .any(|message| message.content == "persistent assistant reply"));
        assert!(session
            .events
            .iter()
            .any(|event| event.event_type == crate::domain::EventType::MessageReceived));
        assert!(session
            .events
            .iter()
            .any(|event| event.event_type == crate::domain::EventType::TurnSucceeded));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn session_send_creates_task_timeline_and_checkpoint() {
        let root = temp_path("task-runtime-session-send");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let server = spawn_server(vec![(
            "POST /v1/responses HTTP/1.1".to_string(),
            vec![
                "<agentjax_prompt version=\\\"v1\\\">".to_string(),
                "<content>create task runtime state</content>".to_string(),
            ],
            r#"{"id":"resp_task_runtime","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{"id":"msg_task_runtime","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"task runtime assistant reply","annotations":[]}]}],"tools":[]}"#.to_string(),
        )])
        .await;

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root, Some(format!("http://{}", server.0))),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        let send = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_runtime_send".into()),
                method: ApiMethod::SessionSend,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "message": { "role": "user", "content": "create task runtime state" },
                    "stream": false,
                }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(send_response) = send.response else {
            panic!("expected response envelope");
        };
        let send_result: SessionSendResponse =
            serde_json::from_value(send_response.result.unwrap()).unwrap();

        let list = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_runtime_list".into()),
                method: ApiMethod::TaskList,
                params: serde_json::json!({}),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(list_response) = list.response else {
            panic!("expected response envelope");
        };
        let listed: crate::api::TaskListResponse =
            serde_json::from_value(list_response.result.unwrap()).unwrap();
        let task_id = listed
            .items
            .iter()
            .find(|item| item.session_id.as_deref() == Some("session.default"))
            .map(|item| item.task_id.clone())
            .expect("session.send should create a bound task");

        let get = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_runtime_get".into()),
                method: ApiMethod::TaskGet,
                params: serde_json::json!({ "task_id": task_id }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(get_response) = get.response else {
            panic!("expected response envelope");
        };
        let task: TaskGetResponse = serde_json::from_value(get_response.result.unwrap()).unwrap();

        assert_eq!(task.task.status, TaskStatus::Succeeded);
        assert_eq!(task.task.execution_mode, ExecutionMode::BoundSession);
        assert!(task
            .timeline
            .iter()
            .any(|entry| entry.phase == crate::domain::TaskPhase::Running));
        assert!(task
            .timeline
            .iter()
            .any(|entry| entry.phase == crate::domain::TaskPhase::Checkpointed));
        assert!(task
            .timeline
            .iter()
            .any(|entry| entry.phase == crate::domain::TaskPhase::Succeeded));
        assert_eq!(task.checkpoints.len(), 1);
        assert_eq!(
            task.checkpoints[0].resume_pack.task_id.as_deref(),
            Some(task.task.task_id.as_str())
        );
        assert_eq!(
            task.checkpoints[0].turn_id.as_deref(),
            Some(send_result.turn_id.as_str())
        );

        server.1.abort();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sleep_directive_marks_task_waiting_with_wake_metadata() {
        let root = temp_path("sleep-waiting");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into())),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        daemon
            .store
            .create_task(Task {
                meta: ObjectMeta::new("task_sleep", "2026-04-10"),
                task_id: "task_sleep".into(),
                workspace_id: "default-workspace".into(),
                agent_id: Some("default-agent".into()),
                session_id: Some("session.default".into()),
                parent_task_id: None,
                definition_ref: None,
                execution_mode: ExecutionMode::BoundSession,
                status: TaskStatus::Running,
                priority: crate::domain::TaskPriority::Normal,
                goal: "wait".into(),
                checkpoint_ref: None,
                waiting_until: None,
                waiting_reason: None,
                waiting_resume_hint: None,
            })
            .unwrap();

        daemon
            .apply_sleep_directive(
                "session.default",
                "task_sleep",
                "turn_sleep",
                SleepDirective {
                    wake_at: Utc::now() + chrono::Duration::seconds(5),
                    reason: Some("wait for shell output".into()),
                    resume_hint: Some("check shell session".into()),
                    duration_ms: Some(5_000),
                },
            )
            .await
            .unwrap();

        let task = daemon.store.get_task("task_sleep").unwrap().unwrap();
        assert_eq!(task.task.status, TaskStatus::Waiting);
        assert!(task.task.waiting_until.is_some());
        assert_eq!(
            task.task.waiting_reason.as_deref(),
            Some("wait for shell output")
        );
        assert_eq!(
            task.task.waiting_resume_hint.as_deref(),
            Some("check shell session")
        );
    }

    #[tokio::test]
    async fn task_cancel_and_retry_survive_daemon_restart() {
        let root = temp_path("task-runtime-restart");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let runtime_config =
            test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into()));
        let identity = test_identity(&workspace_root);

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config.clone(),
                identity.clone(),
            )
            .unwrap(),
        )
        .unwrap();

        daemon
            .store
            .create_task(Task {
                meta: ObjectMeta::new("task_restart", "2026-04-11"),
                task_id: "task_restart".into(),
                workspace_id: "default-workspace".into(),
                agent_id: Some("default-agent".into()),
                session_id: Some("session.default".into()),
                parent_task_id: None,
                definition_ref: None,
                execution_mode: ExecutionMode::BoundSession,
                status: TaskStatus::Pending,
                priority: crate::domain::TaskPriority::Normal,
                goal: "persist task runtime state".into(),
                checkpoint_ref: None,
                waiting_until: None,
                waiting_reason: None,
                waiting_resume_hint: None,
            })
            .unwrap();

        let cancel = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_cancel".into()),
                method: ApiMethod::TaskCancel,
                params: serde_json::json!({ "task_id": "task_restart" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(cancel_response) = cancel.response else {
            panic!("expected response envelope");
        };
        assert!(cancel_response.ok);

        drop(daemon);

        let restarted = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config,
                identity,
            )
            .unwrap(),
        )
        .unwrap();

        let cancelled = restarted
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_get_cancelled".into()),
                method: ApiMethod::TaskGet,
                params: serde_json::json!({ "task_id": "task_restart" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(cancelled_response) = cancelled.response else {
            panic!("expected response envelope");
        };
        let cancelled_task: TaskGetResponse =
            serde_json::from_value(cancelled_response.result.unwrap()).unwrap();
        assert_eq!(cancelled_task.task.status, TaskStatus::Cancelled);
        assert!(cancelled_task
            .timeline
            .iter()
            .any(|entry| entry.phase == crate::domain::TaskPhase::Cancelled));

        let retry = restarted
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_retry".into()),
                method: ApiMethod::TaskRetry,
                params: serde_json::json!({ "task_id": "task_restart" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(retry_response) = retry.response else {
            panic!("expected response envelope");
        };
        assert!(retry_response.ok);

        let retried = restarted
            .handle_request(RequestEnvelope {
                id: RequestId("req_task_get_retried".into()),
                method: ApiMethod::TaskGet,
                params: serde_json::json!({ "task_id": "task_restart" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(retried_response) = retried.response else {
            panic!("expected response envelope");
        };
        let retried_task: TaskGetResponse =
            serde_json::from_value(retried_response.result.unwrap()).unwrap();
        assert_eq!(retried_task.task.status, TaskStatus::Ready);
        assert!(retried_task
            .timeline
            .iter()
            .any(|entry| entry.phase == crate::domain::TaskPhase::Ready));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn session_model_switch_persists_and_routes_session_send() {
        let root = temp_path("session-model-switch");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let active_server = spawn_server(vec![(
            "POST /v1/responses HTTP/1.1".to_string(),
            vec!["\"model\":\"gpt-4.1-mini\"".to_string()],
            r#"{"id":"resp_switch","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4.1-mini","usage":null,"output":[{"id":"msg_switch","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"switched model reply","annotations":[]}]}],"tools":[]}"#.to_string(),
        )])
        .await;

        let mut runtime_config =
            test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into()));
        runtime_config.agent_runtime.llm.providers = vec![
            crate::config::LlmProviderConfig::OpenAi(OpenAiProviderConfig {
                provider_id: "openai-default".into(),
                api_key: Some("test-key".into()),
                api_key_env: "OPENAI_API_KEY".into(),
                base_url: Some("http://127.0.0.1:1".into()),
                organization: None,
                project: None,
            }),
            crate::config::LlmProviderConfig::OpenAi(OpenAiProviderConfig {
                provider_id: "openai-alt".into(),
                api_key: Some("test-key".into()),
                api_key_env: "OPENAI_API_KEY".into(),
                base_url: Some(format!("http://{}", active_server.0)),
                organization: None,
                project: None,
            }),
        ];
        runtime_config.agent_runtime.llm.model_catalog = model_catalog_snapshot(vec![
            provider_snapshot(
                "openai-default",
                "http://127.0.0.1:1/v1",
                vec!["gpt-4o-mini"],
            ),
            provider_snapshot(
                "openai-alt",
                &format!("http://{}/v1", active_server.0),
                vec!["gpt-4.1-mini"],
            ),
        ]);

        let identity = test_identity(&workspace_root);
        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config.clone(),
                identity.clone(),
            )
            .unwrap(),
        )
        .unwrap();

        let switch = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_switch".into()),
                method: ApiMethod::SessionModelSwitch,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "provider_id": "openai-alt",
                    "model_id": "gpt-4.1-mini",
                }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(switch_response) = switch.response else {
            panic!("expected response envelope");
        };
        let switch_result: SessionModelSwitchResponse =
            serde_json::from_value(switch_response.result.unwrap()).unwrap();
        assert_eq!(switch_result.result, SessionModelSwitchResult::Applied);
        assert_eq!(switch_result.model.current.provider_id, "openai-alt");
        assert_eq!(switch_result.model.current.model_id, "gpt-4.1-mini");

        let send = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_switch_send".into()),
                method: ApiMethod::SessionSend,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "message": { "role": "user", "content": "use switched model" },
                    "stream": false,
                }),
                meta: None,
            })
            .await;
        assert!(matches!(
            send.response,
            crate::api::ServerEnvelope::Response(_)
        ));

        let inspect = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_inspect".into()),
                method: ApiMethod::SessionModelInspect,
                params: serde_json::json!({ "session_id": "session.default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(inspect_response) = inspect.response else {
            panic!("expected response envelope");
        };
        let inspect_result: SessionModelInspectResponse =
            serde_json::from_value(inspect_response.result.unwrap()).unwrap();
        assert_eq!(inspect_result.model.current.provider_id, "openai-alt");
        assert_eq!(inspect_result.model.current.model_id, "gpt-4.1-mini");

        let get = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_switch_get".into()),
                method: ApiMethod::SessionGet,
                params: serde_json::json!({ "session_id": "session.default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(get_response) = get.response else {
            panic!("expected response envelope");
        };
        let session: crate::api::SessionGetResponse =
            serde_json::from_value(get_response.result.unwrap()).unwrap();
        assert_eq!(
            session.session.current_provider_id.as_deref(),
            Some("openai-alt")
        );
        assert_eq!(
            session.session.current_model_id.as_deref(),
            Some("gpt-4.1-mini")
        );
        assert!(session
            .events
            .iter()
            .any(|event| event.event_type == crate::domain::EventType::ModelSwitchRequested));
        assert!(session
            .events
            .iter()
            .any(|event| event.event_type == crate::domain::EventType::ModelSwitchApplied));

        drop(daemon);
        let restarted = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config,
                identity,
            )
            .unwrap(),
        )
        .unwrap();
        let inspect_after_restart = restarted
            .handle_request(RequestEnvelope {
                id: RequestId("req_inspect_restart".into()),
                method: ApiMethod::SessionModelInspect,
                params: serde_json::json!({ "session_id": "session.default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(inspect_restart_response) =
            inspect_after_restart.response
        else {
            panic!("expected response envelope");
        };
        let inspect_after_restart_result: SessionModelInspectResponse =
            serde_json::from_value(inspect_restart_response.result.unwrap()).unwrap();
        assert_eq!(
            inspect_after_restart_result.model.current.provider_id,
            "openai-alt"
        );
        assert_eq!(
            inspect_after_restart_result.model.current.model_id,
            "gpt-4.1-mini"
        );

        active_server.1.abort();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn session_model_switch_rejects_when_turn_is_active() {
        let root = temp_path("session-model-switch-reject");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let mut runtime_config =
            test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into()));
        runtime_config.agent_runtime.llm.model_catalog =
            model_catalog_snapshot(vec![provider_snapshot(
                "openai-default",
                "http://127.0.0.1:1/v1",
                vec!["gpt-4o-mini"],
            )]);

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                runtime_config,
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        daemon.store.mark_turn_active("session.default").unwrap();
        let switch = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_switch_reject".into()),
                method: ApiMethod::SessionModelSwitch,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "provider_id": "openai-default",
                    "model_id": "gpt-4o-mini",
                }),
                meta: None,
            })
            .await;
        daemon.store.clear_turn_active("session.default");

        let crate::api::ServerEnvelope::Response(response) = switch.response else {
            panic!("expected response envelope");
        };
        let result: SessionModelSwitchResponse =
            serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.result, SessionModelSwitchResult::Rejected);
        assert_eq!(result.reason.as_deref(), Some("active turn in progress"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plugin_control_plane_handlers_return_runtime_details() {
        let root = temp_path("daemon-plugin-control");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into())),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        let inspect = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_plugin_inspect".into()),
                method: ApiMethod::PluginInspect,
                params: serde_json::json!({ "plugin_id": "provider.openai.openai-default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(inspect_response) = inspect.response else {
            panic!("expected response envelope");
        };
        let inspect_result: PluginInspectResponse =
            serde_json::from_value(inspect_response.result.unwrap()).unwrap();
        assert_eq!(
            inspect_result.plugin.plugin_id,
            "provider.openai.openai-default"
        );
        assert!(inspect_result.enabled);
        assert!(inspect_result.healthy);
        assert!(inspect_result
            .provided_resources
            .iter()
            .any(|resource| resource.starts_with("provider:openai-default:")));

        let reload = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_plugin_reload".into()),
                method: ApiMethod::PluginReload,
                params: serde_json::json!({ "plugin_id": "provider.openai.openai-default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(reload_response) = reload.response else {
            panic!("expected response envelope");
        };
        let reload_result: PluginReloadResponse =
            serde_json::from_value(reload_response.result.unwrap()).unwrap();
        assert!(reload_result.ok);
        assert_eq!(reload_result.plugin_id, "provider.openai.openai-default");
        assert_eq!(reload_result.status, "Running");
        assert!(reload_result
            .checks
            .iter()
            .any(|check| check == "shutdown completed"));

        let test = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_plugin_test".into()),
                method: ApiMethod::PluginTest,
                params: serde_json::json!({ "plugin_id": "provider.openai.openai-default" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(test_response) = test.response else {
            panic!("expected response envelope");
        };
        let test_result: PluginTestResponse =
            serde_json::from_value(test_response.result.unwrap()).unwrap();
        assert!(test_result.ok);
        assert_eq!(test_result.plugin_id, "provider.openai.openai-default");
        assert_eq!(test_result.status, "Running");
        assert!(test_result
            .checks
            .iter()
            .any(|check| check == "enabled=true"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn smoke_run_validates_plugin_manager_runtime() {
        let root = temp_path("daemon-smoke-plugin-manager");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into())),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        let smoke = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_smoke_plugins".into()),
                method: ApiMethod::SmokeRun,
                params: serde_json::json!({ "target": "plugin-manager" }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(smoke_response) = smoke.response else {
            panic!("expected response envelope");
        };
        let smoke_result: SmokeRunResponse =
            serde_json::from_value(smoke_response.result.unwrap()).unwrap();
        assert!(smoke_result.ok);
        assert_eq!(smoke_result.target, "plugin-manager");
        assert!(smoke_result.summary.contains("plugin manager smoke passed"));
        assert!(smoke_result
            .checks
            .iter()
            .any(|check| check.starts_with("plugins_discovered=")));
        assert!(smoke_result
            .checks
            .iter()
            .any(|check| check.contains("provider.openai.openai-default")));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn control_plane_handlers_return_structured_responses() {
        let root = temp_path("daemon-api-control");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into())),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        daemon
            .store
            .create_task(Task {
                meta: ObjectMeta::new("task_1", "2026-04-10"),
                task_id: "task_1".into(),
                workspace_id: "default-workspace".into(),
                agent_id: Some("default-agent".into()),
                session_id: Some("session.default".into()),
                parent_task_id: None,
                definition_ref: Some("defs/test".into()),
                execution_mode: ExecutionMode::BoundSession,
                status: TaskStatus::Pending,
                priority: crate::domain::TaskPriority::Normal,
                goal: "test task".into(),
                checkpoint_ref: None,
                waiting_until: None,
                waiting_reason: None,
                waiting_resume_hint: None,
            })
            .unwrap();

        let methods = vec![
            (
                "config.inspect",
                ApiMethod::ConfigInspect,
                serde_json::json!({ "section": "runtime" }),
            ),
            (
                "config.validate",
                ApiMethod::ConfigValidate,
                serde_json::json!({}),
            ),
            (
                "config.reload",
                ApiMethod::ConfigReload,
                serde_json::json!({}),
            ),
            ("plugin.list", ApiMethod::PluginList, serde_json::json!({})),
            (
                "plugin.inspect",
                ApiMethod::PluginInspect,
                serde_json::json!({ "plugin_id": "provider.openai.openai-default" }),
            ),
            (
                "plugin.reload",
                ApiMethod::PluginReload,
                serde_json::json!({ "plugin_id": "provider.openai.openai-default" }),
            ),
            (
                "plugin.test",
                ApiMethod::PluginTest,
                serde_json::json!({ "plugin_id": "provider.openai.openai-default" }),
            ),
            ("agent.list", ApiMethod::AgentList, serde_json::json!({})),
            (
                "agent.get",
                ApiMethod::AgentGet,
                serde_json::json!({ "agent_id": "default-agent" }),
            ),
            (
                "session.subscribe",
                ApiMethod::SessionSubscribe,
                serde_json::json!({ "session_id": "session.default", "events": ["session.updated"] }),
            ),
            ("task.list", ApiMethod::TaskList, serde_json::json!({})),
            (
                "task.get",
                ApiMethod::TaskGet,
                serde_json::json!({ "task_id": "task_1" }),
            ),
            (
                "task.cancel",
                ApiMethod::TaskCancel,
                serde_json::json!({ "task_id": "task_1" }),
            ),
            (
                "task.retry",
                ApiMethod::TaskRetry,
                serde_json::json!({ "task_id": "task_1" }),
            ),
            (
                "task.subscribe",
                ApiMethod::TaskSubscribe,
                serde_json::json!({ "task_id": "task_1", "events": ["task.updated"] }),
            ),
            ("node.list", ApiMethod::NodeList, serde_json::json!({})),
            (
                "node.get",
                ApiMethod::NodeGet,
                serde_json::json!({ "node_id": "node.local" }),
            ),
            (
                "schedule.create",
                ApiMethod::ScheduleCreate,
                serde_json::to_value(ScheduleCreateRequest {
                    schedule: Schedule {
                        meta: ObjectMeta::new("sched_1", "2026-04-10"),
                        schedule_id: "sched_1".into(),
                        name: "Nightly".into(),
                        trigger: crate::domain::TaskTrigger::Manual,
                        target: crate::domain::TaskTarget::TaskRef {
                            definition_ref: "defs/nightly".into(),
                        },
                        enabled: true,
                    },
                })
                .unwrap(),
            ),
            (
                "schedule.list",
                ApiMethod::ScheduleList,
                serde_json::json!({}),
            ),
            (
                "schedule.update",
                ApiMethod::ScheduleUpdate,
                serde_json::to_value(ScheduleUpdateRequest {
                    schedule: Schedule {
                        meta: ObjectMeta::new("sched_1", "2026-04-10"),
                        schedule_id: "sched_1".into(),
                        name: "Nightly Updated".into(),
                        trigger: crate::domain::TaskTrigger::Interval { seconds: 60 },
                        target: crate::domain::TaskTarget::TaskRef {
                            definition_ref: "defs/nightly".into(),
                        },
                        enabled: false,
                    },
                })
                .unwrap(),
            ),
            (
                "schedule.delete",
                ApiMethod::ScheduleDelete,
                serde_json::json!({ "schedule_id": "sched_1" }),
            ),
            ("doctor.run", ApiMethod::DoctorRun, serde_json::json!({})),
            (
                "smoke.run",
                ApiMethod::SmokeRun,
                serde_json::json!({ "target": "daemon" }),
            ),
            (
                "logs.tail",
                ApiMethod::LogsTail,
                serde_json::json!({ "stream": false }),
            ),
            (
                "metrics.snapshot",
                ApiMethod::MetricsSnapshot,
                serde_json::json!({}),
            ),
            (
                "runtime.shutdown",
                ApiMethod::RuntimeShutdown,
                serde_json::json!({ "graceful": true }),
            ),
        ];

        for (label, method, params) in methods {
            let dispatch = daemon
                .handle_request(RequestEnvelope {
                    id: RequestId(format!("req_{label}")),
                    method,
                    params,
                    meta: None,
                })
                .await;
            let crate::api::ServerEnvelope::Response(response) = dispatch.response else {
                panic!("expected response for {label}");
            };
            assert!(response.ok, "{label} failed: {response:?}");
        }

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stream_and_subscription_cancel_handlers_work() {
        let root = temp_path("daemon-api-cancel");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root, Some("http://127.0.0.1:1".into())),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();

        let session_subscribe = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_subscribe".into()),
                method: ApiMethod::SessionSubscribe,
                params: serde_json::json!({
                    "session_id": "session.default",
                    "events": ["session.updated"]
                }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(subscribe_response) = session_subscribe.response
        else {
            panic!("expected response");
        };
        let subscription: crate::api::SubscriptionResponse =
            serde_json::from_value(subscribe_response.result.unwrap()).unwrap();

        let cancel_sub = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_cancel_sub".into()),
                method: ApiMethod::SubscriptionCancel,
                params: serde_json::json!({
                    "subscription_id": subscription.subscription_id
                }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(cancel_sub_response) = cancel_sub.response else {
            panic!("expected response");
        };
        assert!(cancel_sub_response.ok);

        let logs_tail = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_logs".into()),
                method: ApiMethod::LogsTail,
                params: serde_json::json!({ "stream": true }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(logs_response) = logs_tail.response else {
            panic!("expected response");
        };
        let stream_id = logs_response
            .result
            .unwrap()
            .get("stream_id")
            .and_then(|value| value.as_str())
            .unwrap()
            .to_string();
        assert!(!logs_tail.followups.is_empty());

        let cancel_stream = daemon
            .handle_request(RequestEnvelope {
                id: RequestId("req_cancel_stream".into()),
                method: ApiMethod::StreamCancel,
                params: serde_json::json!({ "stream_id": stream_id }),
                meta: None,
            })
            .await;
        let crate::api::ServerEnvelope::Response(cancel_stream_response) = cancel_stream.response
        else {
            panic!("expected response");
        };
        assert!(cancel_stream_response.ok);

        let _ = fs::remove_dir_all(root);
    }

    fn doc(path: PathBuf, content: &str) -> WorkspaceDocument {
        WorkspaceDocument {
            path,
            content: content.into(),
        }
    }

    fn temp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("agentjax-{prefix}-{nanos}"))
    }

    fn test_runtime_config(
        root: &std::path::Path,
        workspace_root: &std::path::Path,
        base_url: Option<String>,
    ) -> RuntimeConfig {
        let mut runtime_config = RuntimeConfig::new(
            "agentjax-test",
            crate::config::RuntimePaths::new(root.join("runtime")),
            crate::config::WorkspaceConfig::new(
                "default-workspace",
                WorkspacePaths::new(workspace_root),
            ),
        );
        runtime_config.agent_runtime.llm.providers =
            vec![crate::config::LlmProviderConfig::OpenAi(
                OpenAiProviderConfig {
                    provider_id: "openai-default".into(),
                    api_key: Some("test-key".into()),
                    api_key_env: "OPENAI_API_KEY".into(),
                    base_url,
                    organization: None,
                    project: None,
                },
            )];
        runtime_config
    }

    fn test_identity(workspace_root: &std::path::Path) -> WorkspaceIdentityPack {
        WorkspaceIdentityPack {
            workspace_id: "default-workspace".into(),
            agent: doc(workspace_root.join("AGENT.md"), ""),
            soul: doc(workspace_root.join("SOUL.md"), ""),
            user: doc(workspace_root.join("USER.md"), ""),
            memory: doc(workspace_root.join("MEMORY.md"), ""),
            mission: doc(workspace_root.join("MISSION.md"), ""),
            rules: doc(workspace_root.join("RULES.md"), ""),
            router: doc(workspace_root.join("ROUTER.md"), ""),
        }
    }

    fn model_catalog_snapshot(providers: Vec<ProviderModelCatalog>) -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            generated_at: Some(chrono::Utc::now()),
            providers,
        }
    }

    fn provider_snapshot(
        provider_id: &str,
        base_url: &str,
        model_ids: Vec<&str>,
    ) -> ProviderModelCatalog {
        ProviderModelCatalog {
            provider_id: provider_id.into(),
            provider_kind: "openai".into(),
            base_url: Some(base_url.into()),
            language_models: model_ids
                .into_iter()
                .map(|model_id| ModelInfoSnapshot {
                    model_id: model_id.into(),
                    display_label: model_id.into(),
                    context_length: Some(128000),
                    input_token_limit: Some(128000),
                    output_token_limit: Some(16384),
                    capability_tags: vec!["text".into()],
                })
                .collect(),
        }
    }

    async fn spawn_server(
        responses: Vec<(String, Vec<String>, String)>,
    ) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            for (expected_request_line, expected_substrings, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0_u8; 65536];
                let bytes = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes]);
                assert!(request.contains(&expected_request_line), "{request}");
                for expected in expected_substrings {
                    assert!(request.contains(&expected), "{request}");
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (addr, handle)
    }
}
