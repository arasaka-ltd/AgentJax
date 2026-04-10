use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;
use tokio::fs;

use crate::{
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
    plugins::tools::{ToolDescriptor, ToolOutput, ToolPlugin},
};

#[derive(Debug, Clone, Default)]
pub struct ReadFileToolPlugin;

#[async_trait]
impl Plugin for ReadFileToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.read_file".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Tool(ToolCapability::Tool)],
            config_schema: None,
            required_permissions: vec![Permission::ReadWorkspace],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}

#[async_trait]
impl ToolPlugin for ReadFileToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "read_file".into(),
            description: "Read a UTF-8 text file from the local workspace".into(),
            when_to_use: "Use when the user asks for exact file contents or code inspection."
                .into(),
            when_not_to_use: "Do not use for directory listing or command execution.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or workspace-relative path to a UTF-8 text file."
                    }
                },
                "required": ["path"]
            }),
            default_timeout_secs: 5,
            idempotent: true,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<ToolOutput> {
        let path = call
            .args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("read_file requires args.path"))?;
        let content = fs::read_to_string(path).await?;
        Ok(ToolOutput {
            content,
            metadata: json!({ "path": path }),
        })
    }
}
