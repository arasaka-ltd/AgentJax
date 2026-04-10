use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReloadInstruction {
    pub module: String,
    pub reason: String,
    pub requires_drain: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReloadPlan {
    pub instructions: Vec<ReloadInstruction>,
}
