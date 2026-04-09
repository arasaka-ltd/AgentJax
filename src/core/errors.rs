use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub category: ErrorCategory,
    pub source: Option<String>,
    pub details: serde_json::Value,
    pub cause_chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCategory {
    ConfigError,
    ProviderError,
    NetworkError,
    AuthError,
    Timeout,
    RateLimit,
    ToolFailure,
    PluginFailure,
    StateConflict,
    BudgetExceeded,
}
