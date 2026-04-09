use serde::{Deserialize, Serialize};

use crate::domain::{AutonomyPolicy, ObjectMeta, ResourceId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentStatus {
    Active,
    Suspended,
    Draining,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub meta: ObjectMeta,
    pub agent_id: String,
    pub display_name: String,
    pub workspace_id: String,
    pub profile_ref: Option<String>,
    pub mission_ref: Option<String>,
    pub rules_ref: Option<String>,
    pub router_ref: Option<String>,
    pub default_resource_bindings: Vec<ResourceId>,
    pub autonomy_policy: AutonomyPolicy,
    pub status: AgentStatus,
}
