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
pub struct ListFilesToolPlugin;

#[async_trait]
impl Plugin for ListFilesToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.list_files".into(),
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
impl ToolPlugin for ListFilesToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "list_files".into(),
            description: "List files in a local directory".into(),
            when_to_use: "Use when the user asks what files or folders exist in a path.".into(),
            when_not_to_use: "Do not use when file contents are needed; use read_file instead."
                .into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or workspace-relative directory path."
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
            .ok_or_else(|| anyhow!("list_files requires args.path"))?;
        let mut entries = fs::read_dir(path).await?;
        let mut items = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            items.push(entry.path().display().to_string());
        }
        items.sort();
        Ok(ToolOutput {
            content: items.join("\n"),
            metadata: json!({ "path": path, "count": items.len() }),
        })
    }
}
