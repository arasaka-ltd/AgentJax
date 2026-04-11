use async_trait::async_trait;

use crate::{
    core::Plugin,
    domain::{ChannelCapability, Permission, PluginCapability, PluginManifest},
};

#[derive(Debug, Clone, Default)]
pub struct TelegramChannelPlugin;

#[async_trait]
impl Plugin for TelegramChannelPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "channel.telegram".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Channel(ChannelCapability::Telegram)],
            config_schema: None,
            required_permissions: vec![Permission::EmitEvents],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
