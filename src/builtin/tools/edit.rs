use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::fs;

use crate::{
    builtin::tools::{support, ToolDescriptor, ToolPlugin},
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
};

#[derive(Debug, Clone, Default)]
pub struct EditToolPlugin;

#[async_trait]
impl Plugin for EditToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.edit".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Tool(ToolCapability::Tool)],
            config_schema: None,
            required_permissions: vec![Permission::ReadWorkspace, Permission::WriteWorkspace],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}

#[async_trait]
impl ToolPlugin for EditToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "edit".into(),
            description: "Apply a precise text range edit to an existing file".into(),
            when_to_use: "Use when you need to insert, replace, or delete a specific text range."
                .into(),
            when_not_to_use: "Do not use to create a new file or modify binary/image files.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or workspace-relative text file path." },
                    "start_line": { "type": "integer", "description": "1-based inclusive start line." },
                    "start_column": { "type": "integer", "description": "1-based inclusive start column." },
                    "end_line": { "type": "integer", "description": "1-based exclusive end line coordinate." },
                    "end_column": { "type": "integer", "description": "1-based exclusive end column." },
                    "new_text": { "type": "string", "description": "Replacement text using logical \\n newlines." }
                },
                "required": ["path", "start_line", "start_column", "end_line", "end_column", "new_text"]
            }),
            default_timeout_secs: 5,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let path = support::parse_path(&call.args, "edit")?;
        let start_line = support::parse_required_usize(&call.args, "start_line", "edit")?;
        let start_column = support::parse_required_usize(&call.args, "start_column", "edit")?;
        let end_line = support::parse_required_usize(&call.args, "end_line", "edit")?;
        let end_column = support::parse_required_usize(&call.args, "end_column", "edit")?;
        let new_text = call
            .args
            .get("new_text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("edit requires args.new_text"))?;

        let document = support::TextDocument::load(&path).await?;
        let effective_newline = document.newline;
        let (updated, new_range) = document.replace_range(
            start_line,
            start_column,
            end_line,
            end_column,
            &support::normalize_newlines(new_text),
        )?;
        let encoded = support::encode_with_style(&updated, effective_newline);
        fs::write(&path, encoded).await?;

        support::json_tool_output(json!({
            "path": path,
            "applied": true,
            "newline": effective_newline.as_str(),
            "new_range": new_range,
        }))
    }
}
