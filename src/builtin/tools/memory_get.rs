use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::{
    builtin::{
        context::{
            retrieval_bridge::RetrievalBridgeContextPlugin, retrieval_types::MemoryGetRequest,
        },
        tools::{ToolDescriptor, ToolPlugin},
    },
    config::WorkspacePaths,
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
};

#[derive(Debug, Clone, Default)]
pub struct MemoryGetToolPlugin;

#[async_trait]
impl Plugin for MemoryGetToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.memory_get".into(),
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
impl ToolPlugin for MemoryGetToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "memory.get".into(),
            description: "Read a memory document by stable ref or path.".into(),
            when_to_use:
                "Use after memory.search or when you already know the target memory entry.".into(),
            when_not_to_use: "Do not use when you need to discover candidate memory entries first."
                .into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "memory_ref": { "type": "string" },
                    "path": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "end_line": { "type": "integer" },
                    "max_lines": { "type": "integer" }
                }
            }),
            default_timeout_secs: 5,
            idempotent: true,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let workspace_root = std::env::current_dir()?;
        let bridge = RetrievalBridgeContextPlugin::new(&WorkspacePaths::new(&workspace_root));
        let content = bridge.get_memory(&MemoryGetRequest {
            memory_ref: call
                .args
                .get("memory_ref")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            path: call
                .args
                .get("path")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            start_line: super::support::parse_optional_usize(&call.args, "start_line")?,
            end_line: super::support::parse_optional_usize(&call.args, "end_line")?,
            max_lines: super::support::parse_optional_usize(&call.args, "max_lines")?,
        })?;
        super::support::json_tool_output(json!({
            "memory_ref": content.stable_ref,
            "path": content.path,
            "title": content.title,
            "content": content.content,
            "start_line": content.start_line,
            "end_line": content.end_line,
            "total_lines": content.total_lines,
            "truncated": content.truncated
        }))
    }
}
