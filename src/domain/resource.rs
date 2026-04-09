use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(pub String);

impl From<&str> for ResourceId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceKind {
    ModelText,
    ModelReasoning,
    ModelEmbedding,
    ModelReranker,
    AudioTts,
    AudioSt,
    MediaImageGeneration,
    MediaVideoGeneration,
    MediaMusicGeneration,
    StoreMemory,
    StoreVector,
    StoreArtifact,
    ExecShell,
    ExecBrowser,
    NetHttp,
    ChannelTelegram,
    ChannelDiscord,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceStatus {
    Active,
    Degraded,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resource {
    pub resource_id: ResourceId,
    pub resource_kind: ResourceKind,
    pub binding_target: String,
    pub capabilities: Vec<String>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub status: ResourceStatus,
}
