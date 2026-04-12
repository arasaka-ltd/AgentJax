use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::{
    builtin::{
        context::{
            retrieval_bridge::RetrievalBridgeContextPlugin,
            retrieval_types::{MemorySearchRequest, MemorySearchScope, RetrievalMode},
        },
        tools::{ToolDescriptor, ToolPlugin},
    },
    config::WorkspacePaths,
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
};

#[derive(Debug, Clone, Default)]
pub struct MemorySearchToolPlugin;

#[async_trait]
impl Plugin for MemorySearchToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.memory_search".into(),
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
impl ToolPlugin for MemorySearchToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "memory_search".into(),
            description: "Search long-term memory documents for relevant candidates.".into(),
            when_to_use: "Use when you need user preferences, long-term decisions, or stable project facts.".into(),
            when_not_to_use: "Do not use when you already know the exact memory entry and can call memory_get directly.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "top_k": { "type": "integer" },
                    "scope": { "type": "string" },
                    "mode": { "type": "string" },
                    "include_excerpt": { "type": "boolean" }
                },
                "required": ["query"]
            }),
            default_timeout_secs: 5,
            idempotent: true,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let query = call
            .args
            .get("query")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("memory_search requires args.query"))?;
        let top_k = call
            .args
            .get("top_k")
            .and_then(|value| value.as_u64())
            .unwrap_or(5) as usize;
        let scope = MemorySearchScope::parse(
            call.args
                .get("scope")
                .and_then(|value| value.as_str())
                .unwrap_or("all"),
        );
        let mode = RetrievalMode::parse(
            call.args
                .get("mode")
                .and_then(|value| value.as_str())
                .unwrap_or("keyword"),
        );
        let include_excerpt = call
            .args
            .get("include_excerpt")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);

        let workspace_root = std::env::current_dir()?;
        let paths = WorkspacePaths::new(&workspace_root);
        let bridge = RetrievalBridgeContextPlugin::new(&paths);
        let memory_content = std::fs::read_to_string(&paths.memory_file).unwrap_or_default();
        let memory_document = crate::config::WorkspaceDocument {
            path: paths.memory_file.clone(),
            content: memory_content,
        };
        let results = bridge.search_memory(
            &memory_document,
            &MemorySearchRequest {
                query: query.to_string(),
                top_k,
                scope,
                mode,
                include_excerpt,
            },
        )?;

        let results = results
            .into_iter()
            .map(|item| {
                json!({
                    "memory_ref": item.stable_ref,
                    "title": item.title,
                    "path": item.path,
                    "score": item.score,
                    "excerpt": if include_excerpt { item.excerpt } else { String::new() },
                    "section_hint": item.metadata.as_ref().and_then(|meta| meta.get("scope")).cloned(),
                    "reason": "lexical retrieval match"
                })
            })
            .collect::<Vec<_>>();

        super::support::json_tool_output(json!({ "results": results }))
    }
}
