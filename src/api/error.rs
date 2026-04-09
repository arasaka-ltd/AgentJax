use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Timeout,
    Busy,
    UnsupportedMethod,
    UnsupportedVersion,
    ProtocolViolation,
    InternalError,
    RuntimeNotReady,
    DaemonDraining,
    SessionNotFound,
    TaskNotFound,
    AgentNotFound,
    PluginNotFound,
    NodeNotFound,
    ScheduleNotFound,
    SubscriptionNotFound,
    StreamNotFound,
    ConfigInvalid,
    ReloadFailed,
    PluginTestFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    pub retryable: bool,
}

impl ApiError {
    pub fn new(code: ApiErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            retryable,
        }
    }
}
