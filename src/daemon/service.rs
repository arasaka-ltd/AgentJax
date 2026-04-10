use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    api::{
        ApiError, ApiErrorCode, ApiMethod, ClientEnvelope, ConnectionId, HelloAckEnvelope,
        RequestEnvelope, ResponseEnvelope, RuntimePingResponse, RuntimeStatusResponse,
        ServerEnvelope, SessionGetRequest, SessionGetResponse, SessionListItem,
        SessionListResponse, SessionMessage, SessionSendRequest, SessionSendResponse,
        StreamEnvelope, StreamPhase,
    },
    app::Application,
    core::AgentPromptRequest,
    daemon::store::DaemonStore,
    domain::EventType,
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
    pub fn new(app: Application) -> Self {
        let runtime_config = app.runtime_config.clone();
        Self {
            app: Arc::new(app),
            store: Arc::new(DaemonStore::new(runtime_config)),
        }
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
        let turn_id = self.store.next_turn_id();
        let user_message = params.message;

        self.store
            .append_message(&params.session_id, &turn_id, user_message.clone())
            .ok_or_else(session_not_found)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            EventType::MessageReceived,
            json!({ "message": user_message }),
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
            json!({ "provider_id": self.app.runtime.default_agent().provider_id }),
        )?;

        let transcript = self
            .store
            .get_session(&params.session_id)
            .ok_or_else(session_not_found)?
            .messages;
        let prompt = build_transcript_prompt(&transcript);

        let assistant_text = match self
            .app
            .runtime
            .prompt_text(AgentPromptRequest {
                prompt,
                agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
            })
            .await
        {
            Ok(text) => text,
            Err(error) => {
                self.record_event(
                    &params.session_id,
                    &turn_id,
                    EventType::TurnFailed,
                    json!({ "error": error.to_string() }),
                )?;
                return Err(ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("session.send failed: {error}"),
                    false,
                ));
            }
        };

        let assistant_message = SessionMessage {
            role: "assistant".into(),
            content: assistant_text,
        };
        self.store
            .append_message(&params.session_id, &turn_id, assistant_message.clone())
            .ok_or_else(session_not_found)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            EventType::ModelResponseReceived,
            json!({ "message": assistant_message.content }),
        )?;
        self.record_event(
            &params.session_id,
            &turn_id,
            EventType::TurnSucceeded,
            json!({
                "turn_id": turn_id,
                "assistant_message": assistant_message.content,
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
        self.store
            .record_event(
                session_id,
                turn_id,
                &self.store.next_event_id(),
                event_type,
                payload,
            )
            .map(|_| ())
            .ok_or_else(session_not_found)
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

fn build_transcript_prompt(messages: &[SessionMessage]) -> String {
    let mut prompt = String::from(
        "You are continuing an existing conversation. Reply to the latest user message while staying consistent with the prior transcript.\n\nTranscript:\n",
    );

    for message in messages {
        let role = match message.role.as_str() {
            "assistant" => "Assistant",
            _ => "User",
        };
        prompt.push_str(role);
        prompt.push_str(": ");
        prompt.push_str(message.content.trim());
        prompt.push_str("\n\n");
    }

    prompt.push_str("Reply as the assistant to the latest user message only.");
    prompt
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
