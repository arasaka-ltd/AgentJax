use anyhow::{anyhow, bail, Result};
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
pub struct WriteToolPlugin;

#[async_trait]
impl Plugin for WriteToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.write".into(),
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
impl ToolPlugin for WriteToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "write".into(),
            description: "Create or overwrite a text file in the local workspace".into(),
            when_to_use: "Use when you need to create a file or replace its full contents.".into(),
            when_not_to_use: "Do not use for partial edits when edit would be more precise.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or workspace-relative text file path." },
                    "content": { "type": "string", "description": "File content using logical \\n newlines." },
                    "create_dirs": { "type": "boolean", "description": "Create missing parent directories with mkdir -p semantics." },
                    "overwrite": { "type": "boolean", "description": "Whether to overwrite an existing file." },
                    "encoding": { "type": "string", "description": "Optional encoding hint; only utf-8 is currently supported." },
                    "newline": { "type": "string", "description": "One of preserve_if_exists, lf, or crlf." }
                },
                "required": ["path", "content"]
            }),
            default_timeout_secs: 5,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let path = support::parse_path(&call.args, "write")?;
        let content = call
            .args
            .get("content")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("write requires args.content"))?;
        let create_dirs = support::parse_optional_bool(&call.args, "create_dirs").unwrap_or(false);
        let overwrite = support::parse_optional_bool(&call.args, "overwrite").unwrap_or(false);

        let parent = support::ensure_parent_dir(&path)?;
        if let Some(parent) = &parent {
            if !parent.exists() {
                if create_dirs {
                    fs::create_dir_all(parent).await?;
                } else {
                    bail!("parent directory does not exist for {path}");
                }
            }
        }

        let existed = fs::try_exists(&path).await?;
        if existed && !overwrite {
            bail!("write refused to overwrite existing file: {path}");
        }

        let existing_text = if existed {
            Some(String::from_utf8(fs::read(&path).await?)?)
        } else {
            None
        };
        let newline = support::choose_write_newline(
            &path,
            support::parse_optional_string(&call.args, "newline"),
            existed,
            existing_text.as_deref().unwrap_or(""),
        )?;
        let normalized = support::normalize_newlines(content);
        let encoded = support::encode_with_style(&normalized, newline);
        fs::write(&path, encoded.as_bytes()).await?;

        support::json_tool_output(json!({
            "path": path,
            "created": !existed,
            "overwritten": existed,
            "bytes_written": encoded.len(),
            "newline": newline.as_str(),
        }))
    }
}
