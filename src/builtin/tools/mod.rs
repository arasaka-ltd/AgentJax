use std::{collections::BTreeMap, sync::Arc};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{core::Plugin, domain::ToolCall};

pub mod edit;
pub mod knowledge_get;
pub mod knowledge_search;
pub mod memory_get;
pub mod memory_search;
pub mod read;
pub mod runtime_control;
pub mod shell;
pub mod support;
pub mod write;

pub use edit::EditToolPlugin;
pub use knowledge_get::KnowledgeGetToolPlugin;
pub use knowledge_search::KnowledgeSearchToolPlugin;
pub use memory_get::MemoryGetToolPlugin;
pub use memory_search::MemorySearchToolPlugin;
pub use read::ReadToolPlugin;
pub use runtime_control::SleepToolPlugin;
pub use shell::{
    ShellExecToolPlugin, ShellSessionCloseToolPlugin, ShellSessionExecToolPlugin,
    ShellSessionInterruptToolPlugin, ShellSessionListToolPlugin, ShellSessionOpenToolPlugin,
    ShellSessionReadToolPlugin, ShellSessionResizeToolPlugin,
};
pub use write::WriteToolPlugin;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub when_not_to_use: String,
    pub arguments_schema: Value,
    pub default_timeout_secs: u64,
    pub idempotent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDescriptor {
    pub fn validate_definition(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(anyhow!("tool name must not be empty"));
        }
        if !self
            .name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        {
            return Err(anyhow!(
                "tool name contains unsupported characters: {}",
                self.name
            ));
        }
        if self.description.trim().is_empty() {
            return Err(anyhow!("tool description must not be empty"));
        }
        if self
            .arguments_schema
            .get("type")
            .and_then(|value| value.as_str())
            != Some("object")
        {
            return Err(anyhow!(
                "tool {} schema must be a JSON object schema",
                self.name
            ));
        }
        Ok(())
    }

    pub fn definition(&self) -> Result<ToolDefinition> {
        self.validate_definition()?;
        Ok(ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.arguments_schema.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub content: String,
    pub metadata: Value,
}

#[async_trait]
pub trait ToolPlugin: Plugin + Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;

    async fn invoke(&self, call: &ToolCall) -> Result<ToolOutput>;
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn ToolPlugin>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn ToolPlugin>) {
        self.tools.insert(tool.descriptor().name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolPlugin>> {
        self.tools.get(name).cloned()
    }

    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|tool| tool.descriptor()).collect()
    }

    pub fn builtins() -> Self {
        let mut registry = Self::default();
        registry.register(Arc::new(ReadToolPlugin));
        registry.register(Arc::new(EditToolPlugin));
        registry.register(Arc::new(WriteToolPlugin));
        registry.register(Arc::new(MemorySearchToolPlugin));
        registry.register(Arc::new(MemoryGetToolPlugin));
        registry.register(Arc::new(KnowledgeSearchToolPlugin));
        registry.register(Arc::new(KnowledgeGetToolPlugin));
        registry.register(Arc::new(ShellExecToolPlugin));
        registry.register(Arc::new(ShellSessionOpenToolPlugin));
        registry.register(Arc::new(ShellSessionExecToolPlugin));
        registry.register(Arc::new(ShellSessionReadToolPlugin));
        registry.register(Arc::new(ShellSessionListToolPlugin));
        registry.register(Arc::new(ShellSessionCloseToolPlugin));
        registry.register(Arc::new(ShellSessionInterruptToolPlugin));
        registry.register(Arc::new(ShellSessionResizeToolPlugin));
        registry.register(Arc::new(SleepToolPlugin));
        registry
    }
}
