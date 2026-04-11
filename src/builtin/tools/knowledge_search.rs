use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::{
    builtin::{
        context::{
            retrieval_bridge::RetrievalBridgeContextPlugin,
            retrieval_types::{KnowledgeSearchRequest, RetrievalMode},
        },
        tools::{ToolDescriptor, ToolPlugin},
    },
    config::WorkspacePaths,
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
};

#[derive(Debug, Clone, Default)]
pub struct KnowledgeSearchToolPlugin;

#[async_trait]
impl Plugin for KnowledgeSearchToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.knowledge_search".into(),
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
impl ToolPlugin for KnowledgeSearchToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "knowledge.search".into(),
            description: "Search domain knowledge libraries for evidence candidates.".into(),
            when_to_use: "Use for evidence-oriented questions over project or domain knowledge.".into(),
            when_not_to_use: "Do not use when you already know the exact document and can call knowledge.get directly.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "top_k": { "type": "integer" },
                    "library": { "type": "string" },
                    "libraries": { "type": "array", "items": { "type": "string" } },
                    "path_prefix": { "type": "string" },
                    "mode": { "type": "string" },
                    "metadata_filters": { "type": "object" },
                    "include_excerpt": { "type": "boolean" }
                },
                "required": ["query"]
            }),
            default_timeout_secs: 5,
            idempotent: true,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let workspace_root = std::env::current_dir()?;
        let bridge = RetrievalBridgeContextPlugin::new(&WorkspacePaths::new(&workspace_root));
        let include_excerpt = call
            .args
            .get("include_excerpt")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let libraries = call
            .args
            .get("libraries")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        let results = bridge.search_knowledge(&KnowledgeSearchRequest {
            query: call
                .args
                .get("query")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            top_k: call
                .args
                .get("top_k")
                .and_then(|value| value.as_u64())
                .unwrap_or(5) as usize,
            library: call
                .args
                .get("library")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            libraries,
            path_prefix: call
                .args
                .get("path_prefix")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            mode: RetrievalMode::from_str(
                call.args
                    .get("mode")
                    .and_then(|value| value.as_str())
                    .unwrap_or("keyword"),
            ),
            metadata_filters: call.args.get("metadata_filters").cloned(),
            include_excerpt,
        })?;

        let results = results
            .into_iter()
            .map(|item| {
                json!({
                    "doc_ref": item.stable_ref,
                    "library": item.library,
                    "path": item.path,
                    "title": item.title,
                    "score": item.score,
                    "excerpt": if include_excerpt { item.excerpt } else { String::new() },
                    "chunk_ref": item.chunk_ref,
                    "metadata": item.metadata
                })
            })
            .collect::<Vec<_>>();

        super::support::json_tool_output(json!({ "results": results }))
    }
}
