use async_trait::async_trait;
use serde_json::json;

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
            config_schema: Some(json!({
                "type": "object",
                "properties": {
                    "bot_token": { "type": "string", "minLength": 1 },
                    "webhook_url": { "type": "string", "minLength": 1 },
                    "parse_mode": { "type": "string" }
                },
                "additionalProperties": true
            })),
            required_permissions: vec![Permission::EmitEvents],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
