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
pub struct ReadToolPlugin;

#[async_trait]
impl Plugin for ReadToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.read".into(),
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
impl ToolPlugin for ReadToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "read".into(),
            description: "Read a text or image file from the local workspace".into(),
            when_to_use:
                "Use when the user asks for file contents, line ranges, or image inspection.".into(),
            when_not_to_use: "Do not use for modifying files or listing directories.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or workspace-relative file path." },
                    "start_line": { "type": "integer", "description": "1-based inclusive start line for text reads." },
                    "end_line": { "type": "integer", "description": "1-based inclusive end line for text reads." },
                    "start_column": { "type": "integer", "description": "Optional 1-based inclusive start column for text reads." },
                    "end_column": { "type": "integer", "description": "Optional 1-based inclusive end column for text reads." },
                    "max_lines": { "type": "integer", "description": "Optional line budget for text reads." },
                    "encoding": { "type": "string", "description": "Optional encoding hint; only utf-8 is currently supported." }
                },
                "required": ["path"]
            }),
            default_timeout_secs: 5,
            idempotent: true,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let path = support::parse_path(&call.args, "read")?;
        let bytes = fs::read(&path).await?;

        if let Ok(image) = support::image_metadata(&path, &bytes) {
            return support::json_tool_output(image);
        }

        let document = support::TextDocument::from_bytes(&path, &bytes)?;
        let start_line = support::parse_optional_usize(&call.args, "start_line")?.unwrap_or(1);
        let max_lines = support::parse_optional_usize(&call.args, "max_lines")?;
        let mut end_line = support::parse_optional_usize(&call.args, "end_line")?
            .unwrap_or_else(|| document.total_lines());
        if call.args.get("end_line").is_none() {
            if let Some(max_lines) = max_lines {
                end_line = (start_line + max_lines.saturating_sub(1)).min(document.total_lines());
            }
        }

        let content = document.slice_lines(start_line, end_line)?;
        support::json_tool_output(json!({
            "path": path,
            "kind": "text",
            "content": content,
            "start_line": start_line,
            "end_line": end_line,
            "total_lines": document.total_lines(),
            "newline": document.newline.as_str(),
            "truncated": end_line < document.total_lines(),
        }))
    }
}
