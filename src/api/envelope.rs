use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{ApiError, ApiMethod, ConnectionId, RequestId, StreamId, SubscriptionId, TraceId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActorIdentity {
    pub kind: String,
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RequestMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<TraceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<ActorIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EventEnvelopeMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<TraceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StreamEnvelopeMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<TraceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloEnvelope {
    pub api_version: String,
    pub client: ActorIdentity,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelloAckEnvelope {
    pub ok: bool,
    pub api_version: String,
    pub schema_version: String,
    pub daemon_version: String,
    pub connection_id: ConnectionId,
}

impl HelloAckEnvelope {
    pub fn new(connection_id: ConnectionId, api_version: impl Into<String>) -> Self {
        Self {
            ok: true,
            api_version: api_version.into(),
            schema_version: "2026-04-10".into(),
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            connection_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestEnvelope {
    pub id: RequestId,
    pub method: ApiMethod,
    pub params: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

impl RequestEnvelope {
    pub fn parse_params<T>(&self) -> Result<T, ApiError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.params.clone()).map_err(|error| {
            ApiError::new(
                crate::api::ApiErrorCode::InvalidRequest,
                format!("invalid params for {}: {error}", self.method.as_str()),
                false,
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseEnvelope {
    pub id: RequestId,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl ResponseEnvelope {
    pub fn ok(id: RequestId, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
            meta: None,
        }
    }

    pub fn err(id: RequestId, error: ApiError) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error),
            meta: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventPayload {
    pub event: String,
    pub subscription_id: SubscriptionId,
    pub seq: u64,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<EventEnvelopeMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamPhase {
    Start,
    Chunk,
    End,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamPayload {
    pub stream_id: StreamId,
    pub phase: StreamPhase,
    pub event: String,
    pub seq: u64,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<StreamEnvelopeMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEnvelope {
    Hello(HelloEnvelope),
    Request(RequestEnvelope),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEnvelope {
    HelloAck(HelloAckEnvelope),
    Response(ResponseEnvelope),
    Event(EventPayload),
    Stream(StreamPayload),
    Error(ApiErrorEnvelope),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiErrorEnvelope {
    pub error: ApiError,
}

impl ApiErrorEnvelope {
    pub fn new(error: ApiError) -> Self {
        Self { error }
    }
}

pub type EventEnvelope = EventPayload;
pub type StreamEnvelope = StreamPayload;
