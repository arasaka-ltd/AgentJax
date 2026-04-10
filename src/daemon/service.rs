use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    api::{
        ApiError, ApiErrorCode, ApiMethod, ClientEnvelope, ConnectionId, HelloAckEnvelope,
        RequestEnvelope, ResponseEnvelope, RuntimePingResponse, RuntimeStatusResponse,
        ServerEnvelope, SessionGetRequest, SessionGetResponse, SessionListItem,
        SessionListResponse, SessionMessage, SessionMessageAnnotation, SessionModelInspectRequest,
        SessionModelInspectResponse, SessionModelState, SessionModelSwitchRequest,
        SessionModelSwitchResponse, SessionModelSwitchResult, SessionSendRequest,
        SessionSendResponse, StreamEnvelope, StreamPhase,
    },
    app::Application,
    context_engine::{
        parse_workspace_prompt_documents, render_prompt_xml, AssembledContext,
        ContextAssemblyRequest, PromptRenderRequest,
    },
    core::AgentPromptRequest,
    daemon::store::DaemonStore,
    domain::{
        ContextAssemblyPurpose, EventType, Session, SessionModelTarget, ToolCall, ToolCaller,
    },
};

pub const API_VERSION: &str = "v1";
pub const SCHEMA_VERSION: &str = "2026-04-10";

#[derive(Clone)]
pub struct Daemon {
    app: Arc<Application>,
    store: Arc<DaemonStore>,
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
        Ok(Self {
            app: Arc::new(app),
            store: Arc::new(DaemonStore::new(runtime_config)?),
        })
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
            ApiMethod::SessionSend => self.handle_session_send(request.parse_params()?).await,
            _ => Err(ApiError::new(
                ApiErrorCode::UnsupportedMethod,
                format!("method {} is not implemented yet", request.method.as_str()),
                false,
            )),
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

        self.store
            .append_message(&params.session_id, &turn_id, user_message.clone())
            .map_err(map_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            EventType::MessageReceived,
            json!({ "message": user_message }),
        )?;
        let assembled_context = self
            .app
            .context_engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some(params.session_id.clone()),
                task_id: None,
                budget_tokens: 8_000,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
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
            EventType::TurnStarted,
            json!({ "turn_id": turn_id }),
        )?;
        self.record_event(
            &params.session_id,
            &turn_id,
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

        let prompt =
            build_context_prompt(&self.app, &assembled_context, prompt_messages.clone(), true);
        let first_response = self
            .app
            .runtime
            .prompt_text(AgentPromptRequest {
                prompt,
                agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
                agent_override: Some(session_agent.clone()),
            })
            .await
            .map_err(|error| {
                let _ = self.record_event(
                    &params.session_id,
                    &turn_id,
                    EventType::TurnFailed,
                    json!({ "error": error.to_string() }),
                );
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("session.send failed: {error}"),
                    false,
                )
            })?;
        self.record_event(
            &params.session_id,
            &turn_id,
            EventType::ModelResponseReceived,
            json!({ "message": first_response }),
        )?;

        let assistant_text = if let Some(tool_call) = parse_tool_call(
            &first_response,
            &params.session_id,
            &turn_id,
            &session_agent.agent_id,
        )? {
            let tool_result = self.execute_tool_call(&tool_call).await?;
            let followup_prompt = build_tool_followup_prompt(
                &self.app,
                &assembled_context,
                prompt_messages,
                &tool_call,
                &tool_result,
                &session.session,
                self.store.next_message_id(),
            );
            self.record_event(
                &params.session_id,
                &turn_id,
                EventType::ModelCalled,
                json!({
                    "provider_id": session_agent.provider_id.clone(),
                    "model_id": session_agent.model.clone(),
                    "phase": "after_tool",
                }),
            )?;
            let second_response = self
                .app
                .runtime
                .prompt_text(AgentPromptRequest {
                    prompt: followup_prompt,
                    agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
                    agent_override: Some(session_agent.clone()),
                })
                .await
                .map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("tool follow-up model call failed: {error}"),
                        false,
                    )
                })?;
            self.record_event(
                &params.session_id,
                &turn_id,
                EventType::ModelResponseReceived,
                json!({
                    "message": second_response,
                    "phase": "after_tool",
                }),
            )?;
            second_response
        } else {
            first_response
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
        self.record_event(
            &params.session_id,
            &turn_id,
            EventType::TurnSucceeded,
            json!({
                "turn_id": turn_id,
                "assistant_message": assistant_message,
            }),
        )?;

        let followups = if let Some(stream_id) = params.stream.then(|| self.store.next_stream_id())
        {
            build_stream_envelopes(&stream_id, &turn_id, &assistant_message.content)
        } else {
            Vec::new()
        };
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
        event_type: EventType,
        payload: Value,
    ) -> Result<(), ApiError> {
        let event = self
            .store
            .record_event(
                session_id,
                turn_id,
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
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
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
                self.record_event(
                    tool_call.session_id.as_deref().unwrap_or("session.default"),
                    tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
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
                self.record_event(
                    tool_call.session_id.as_deref().unwrap_or("session.default"),
                    tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
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
    mut conversation_messages: Vec<SessionMessage>,
    tool_call: &ToolCall,
    tool_result: &str,
    session: &Session,
    next_message_id: String,
) -> String {
    let mut tool_result_message = finalize_message(
        SessionMessage::tool_result(tool_result),
        session,
        next_message_id.clone(),
        Some(vec![
            SessionMessageAnnotation {
                kind: "tool_name".into(),
                value: tool_call.tool_name.clone(),
            },
            SessionMessageAnnotation {
                kind: "tool_call_id".into(),
                value: tool_call.tool_call_id.clone(),
            },
        ]),
    );
    tool_result_message.meta.actor_id = Some(app.runtime.default_agent().agent_id.clone());
    conversation_messages.push(tool_result_message);
    conversation_messages.push(finalize_message(
        SessionMessage::runtime(
            "Answer the user directly using the tool result. Do not request another tool.",
        ),
        session,
        format!("{next_message_id}.runtime"),
        Some(vec![SessionMessageAnnotation {
            kind: "phase".into(),
            value: "after_tool".into(),
        }]),
    ));
    build_context_prompt(app, assembled, conversation_messages, false)
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

fn parse_tool_call(
    response: &str,
    session_id: &str,
    turn_id: &str,
    agent_id: &str,
) -> Result<Option<ToolCall>, ApiError> {
    let trimmed = response.trim();
    let Some(payload) = trimmed.strip_prefix("TOOL_CALL ") else {
        return Ok(None);
    };
    let value: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
        ApiError::new(
            ApiErrorCode::InvalidRequest,
            format!("invalid tool call payload from model: {error}"),
            false,
        )
    })?;
    let tool_name = value
        .get("tool")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ApiError::new(
                ApiErrorCode::InvalidRequest,
                "tool call missing tool",
                false,
            )
        })?;
    let args = value.get("args").cloned().unwrap_or_else(|| json!({}));
    let timeout_secs = value.get("timeout_secs").and_then(|value| value.as_u64());

    Ok(Some(ToolCall {
        tool_call_id: format!("toolcall_{turn_id}_{tool_name}"),
        tool_name: tool_name.into(),
        args,
        requested_by: ToolCaller::Agent {
            agent_id: agent_id.into(),
        },
        session_id: Some(session_id.into()),
        task_id: None,
        turn_id: Some(turn_id.into()),
        idempotency_key: Some(format!("{turn_id}:{tool_name}")),
        timeout_secs,
    }))
}

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

    for (index, chunk) in content.split_whitespace().enumerate() {
        followups.push(ServerEnvelope::Stream(StreamEnvelope {
            stream_id: stream_id.into(),
            phase: StreamPhase::Chunk,
            event: "token".into(),
            seq: index as u64 + 1,
            data: json!({ "text": format!("{chunk} ") }),
            meta: None,
        }));
    }

    followups.push(ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::End,
        event: "session.output".into(),
        seq: content.split_whitespace().count() as u64 + 1,
        data: json!({ "turn_id": turn_id, "done": true }),
        meta: None,
    }));
    followups
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::SocketAddr,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    use crate::{
        api::{
            ApiMethod, RequestEnvelope, RequestId, SessionModelInspectResponse,
            SessionModelSwitchResponse, SessionModelSwitchResult,
        },
        app::Application,
        config::{
            ConfigRoot, ModelCatalogSnapshot, ModelInfoSnapshot, OpenAiProviderConfig,
            ProviderModelCatalog, RuntimeConfig, WorkspaceDocument, WorkspaceIdentityPack,
            WorkspacePaths,
        },
    };

    use super::Daemon;

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
                ],
                format!(
                    r#"{{"id":"resp_1","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{{"type":"output_text","text":"TOOL_CALL {{\"tool\":\"list_files\",\"args\":{{\"path\":\"{}\"}}}}","annotations":[]}}]}}],"tools":[]}}"#,
                    sample_dir.display()
                ),
            ),
            (
                "POST /v1/responses HTTP/1.1".to_string(),
                vec![
                    "<message kind=\\\"tool_result\\\">".to_string(),
                    "Answer the user directly using the tool result.".to_string(),
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
        );
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

        let daemon = Daemon::new(Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config.clone(),
            identity.clone(),
        ))
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

        let restarted = Daemon::new(Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config,
            identity,
        ))
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
        let daemon = Daemon::new(Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config.clone(),
            identity.clone(),
        ))
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
        let restarted = Daemon::new(Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config,
            identity,
        ))
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

        let daemon = Daemon::new(Application::new(
            ConfigRoot::new(root.join("config")),
            runtime_config,
            test_identity(&workspace_root),
        ))
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
                let mut buffer = vec![0_u8; 16384];
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
