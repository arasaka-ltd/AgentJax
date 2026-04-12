use serde::{Deserialize, Serialize};

use crate::domain::{
    AssistantTextItem, ModelTurnOutput, ModelUsage, RuntimeControlItem, ToolCallItem,
    ToolResultItem,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelStreamEvent {
    AssistantTextDelta(AssistantTextItem),
    ToolCall(ToolCallItem),
    ToolResult(ToolResultItem),
    RuntimeControl(RuntimeControlItem),
    Usage(ModelUsage),
    Completed(ModelTurnOutput),
}
