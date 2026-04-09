use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolCaller {
    Agent { agent_id: String },
    Plugin { plugin_id: String },
    Operator { operator_id: String },
    Scheduler,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub requested_by: ToolCaller,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub turn_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRecord {
    pub meta: ObjectMeta,
    pub call: ToolCall,
}
