use async_trait::async_trait;
use serde_json::json;

use crate::{
    core::Plugin,
    domain::{NodeCapability, Permission, PluginCapability, PluginManifest},
};

#[derive(Debug, Clone, Default)]
pub struct StaticNodeRegistryPlugin;

#[async_trait]
impl Plugin for StaticNodeRegistryPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "node.static_registry".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Node(NodeCapability::MachineNode)],
            config_schema: Some(json!({
                "type": "object",
                "properties": {
                    "nodes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "node_id": { "type": "string", "minLength": 1 },
                                "label": { "type": "string" },
                                "tags": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            },
                            "required": ["node_id"],
                            "additionalProperties": true
                        }
                    }
                },
                "additionalProperties": true
            })),
            required_permissions: vec![Permission::ReadState],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
