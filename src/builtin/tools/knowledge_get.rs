use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::{
    builtin::{
        context::{
            retrieval_bridge::RetrievalBridgeContextPlugin, retrieval_types::KnowledgeGetRequest,
        },
        tools::{ToolDescriptor, ToolPlugin},
    },
    config::WorkspacePaths,
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
};

#[derive(Debug, Clone, Default)]
pub struct KnowledgeGetToolPlugin;

#[async_trait]
impl Plugin for KnowledgeGetToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.knowledge_get".into(),
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
impl ToolPlugin for KnowledgeGetToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "knowledge.get".into(),
            description: "Read a knowledge document by stable ref or path.".into(),
            when_to_use: "Use after knowledge.search or when the exact document is already known."
                .into(),
            when_not_to_use:
                "Do not use when you first need to discover relevant knowledge candidates.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "doc_ref": { "type": "string" },
                    "path": { "type": "string" },
                    "library": { "type": "string" },
                    "chunk_ref": { "type": "string" },
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
        let content = bridge.get_knowledge(&KnowledgeGetRequest {
            doc_ref: call
                .args
                .get("doc_ref")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            path: call
                .args
                .get("path")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            library: call
                .args
                .get("library")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            chunk_ref: call
                .args
                .get("chunk_ref")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            start_line: super::support::parse_optional_usize(&call.args, "start_line")?,
            end_line: super::support::parse_optional_usize(&call.args, "end_line")?,
            max_lines: super::support::parse_optional_usize(&call.args, "max_lines")?,
        })?;
        super::support::json_tool_output(json!({
            "doc_ref": content.stable_ref,
            "chunk_ref": content.chunk_ref,
            "library": content.library,
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
