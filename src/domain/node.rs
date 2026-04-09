use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeKind {
    RemoteWorker,
    Machine,
    Device,
    Browser,
    Static,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    Active,
    Draining,
    Offline,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrustLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub meta: ObjectMeta,
    pub node_id: String,
    pub kind: NodeKind,
    pub platform: String,
    pub status: NodeStatus,
    pub capabilities: Vec<String>,
    pub resources: Vec<String>,
    pub trust_level: TrustLevel,
    pub labels: std::collections::BTreeMap<String, String>,
}
