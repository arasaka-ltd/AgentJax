use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{core::Plugin, domain::ToolCall};

pub mod list_files;
pub mod read_file;
pub mod shell;

pub use list_files::ListFilesToolPlugin;
pub use read_file::ReadFileToolPlugin;
pub use shell::ShellToolPlugin;

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
        registry.register(Arc::new(ReadFileToolPlugin::default()));
        registry.register(Arc::new(ListFilesToolPlugin::default()));
        registry.register(Arc::new(ShellToolPlugin::default()));
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

    use serde_json::json;

    use super::ToolRegistry;
    use crate::domain::{ToolCall, ToolCaller};

    #[tokio::test]
    async fn builtins_cover_read_file_list_files_and_shell() {
        let root = temp_path("tools");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("note.txt");
        fs::write(&file, "hello tools").unwrap();

        let registry = ToolRegistry::builtins();
        assert!(registry.get("read_file").is_some());
        assert!(registry.get("list_files").is_some());
        assert!(registry.get("shell").is_some());

        let read = registry
            .get("read_file")
            .unwrap()
            .invoke(&tool_call(
                "read_file",
                json!({ "path": file.display().to_string() }),
            ))
            .await
            .unwrap();
        assert_eq!(read.content, "hello tools");

        let list = registry
            .get("list_files")
            .unwrap()
            .invoke(&tool_call(
                "list_files",
                json!({ "path": root.display().to_string() }),
            ))
            .await
            .unwrap();
        assert!(list.content.contains("note.txt"));

        let shell = registry
            .get("shell")
            .unwrap()
            .invoke(&tool_call("shell", json!({ "command": "printf tool-ok" })))
            .await
            .unwrap();
        assert!(shell.content.contains("tool-ok"));

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

    fn temp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("agentjax-{prefix}-{nanos}"))
    }
}
