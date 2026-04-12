use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
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
pub mod shell;
pub mod support;
pub mod write;

pub use edit::EditToolPlugin;
pub use knowledge_get::KnowledgeGetToolPlugin;
pub use knowledge_search::KnowledgeSearchToolPlugin;
pub use memory_get::MemoryGetToolPlugin;
pub use memory_search::MemorySearchToolPlugin;
pub use read::ReadToolPlugin;
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
        registry
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::{json, Value};

    use super::ToolRegistry;
    use crate::domain::{ToolCall, ToolCaller};

    #[tokio::test]
    async fn builtins_cover_read_edit_and_write() {
        let root = temp_path("tools");
        fs::create_dir_all(&root).unwrap();
        let memory_file = root.join("MEMORY.md");
        fs::write(&memory_file, "## Stable Facts\r\nhello\r\ntools\r\n").unwrap();
        let knowledge_file = root.join("knowledge/rust/notes/ownership.md");

        let registry = ToolRegistry::builtins();
        assert!(registry.get("read").is_some());
        assert!(registry.get("edit").is_some());
        assert!(registry.get("write").is_some());

        let read = registry
            .get("read")
            .unwrap()
            .invoke(&tool_call(
                "read",
                json!({ "path": memory_file.display().to_string(), "start_line": 1, "end_line": 2 }),
            ))
            .await
            .unwrap();
        let read_json = parse_output(&read.content);
        assert_eq!(read_json["newline"], "crlf");
        assert_eq!(read_json["kind"], "text");
        assert!(read_json["content"]
            .as_str()
            .unwrap()
            .contains("1| ## Stable Facts"));

        let edit = registry
            .get("edit")
            .unwrap()
            .invoke(&tool_call(
                "edit",
                json!({
                    "path": memory_file.display().to_string(),
                    "start_line": 2,
                    "start_column": 1,
                    "end_line": 2,
                    "end_column": 6,
                    "new_text": "world"
                }),
            ))
            .await
            .unwrap();
        let edit_json = parse_output(&edit.content);
        assert_eq!(edit_json["applied"], true);
        assert_eq!(
            fs::read_to_string(&memory_file).unwrap(),
            "## Stable Facts\r\nworld\r\ntools\r\n"
        );

        let write = registry
            .get("write")
            .unwrap()
            .invoke(&tool_call(
                "write",
                json!({
                    "path": knowledge_file.display().to_string(),
                    "content": "# Ownership\n\nRust ownership rules...\n",
                    "create_dirs": true
                }),
            ))
            .await
            .unwrap();
        let write_json = parse_output(&write.content);
        assert_eq!(write_json["created"], true);
        assert_eq!(
            fs::read_to_string(&knowledge_file).unwrap(),
            "# Ownership\n\nRust ownership rules...\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn tool_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: format!("call-{name}"),
            tool_name: name.into(),
            args,
            requested_by: ToolCaller::Operator {
                operator_id: "test".into(),
            },
            session_id: None,
            task_id: None,
            turn_id: None,
            idempotency_key: Some(format!("idempotent-{name}")),
            timeout_secs: Some(2),
        }
    }

    fn parse_output(content: &str) -> Value {
        serde_json::from_str(content).unwrap()
    }

    fn temp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("agentjax-{prefix}-{nanos}"))
    }
}
