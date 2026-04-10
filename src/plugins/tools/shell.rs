use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;
use tokio::{
    process::Command,
    time::{timeout, Duration},
};

use crate::{
    core::Plugin,
    domain::ToolCall,
    domain::{Permission, PluginCapability, PluginManifest, ToolCapability},
    plugins::tools::{ToolDescriptor, ToolOutput, ToolPlugin},
};

#[derive(Debug, Clone, Default)]
pub struct ShellToolPlugin;

#[async_trait]
impl Plugin for ShellToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.shell".into(),
            version: "0.1.0".into(),
            capabilities: vec![
                PluginCapability::Tool(ToolCapability::Tool),
                PluginCapability::Tool(ToolCapability::Executor),
            ],
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
impl ToolPlugin for ShellToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell".into(),
            description: "Run a local shell command".into(),
            when_to_use:
                "Use for local inspection or implementation tasks that require a shell command."
                    .into(),
            when_not_to_use:
                "Do not use for destructive commands unless the task clearly requires it.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to run with zsh -lc."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Optional timeout override in seconds."
                    }
                },
                "required": ["command"]
            }),
            default_timeout_secs: 10,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<ToolOutput> {
        let command = call
            .args
            .get("command")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("shell requires args.command"))?;
        let timeout_secs = call
            .timeout_secs
            .unwrap_or(self.descriptor().default_timeout_secs);
        let output = timeout(
            Duration::from_secs(timeout_secs),
            Command::new("zsh").arg("-lc").arg(command).output(),
        )
        .await
        .map_err(|_| anyhow!("shell command timed out after {timeout_secs}s"))??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(ToolOutput {
            content: format!(
                "exit_code={}\nstdout:\n{}\nstderr:\n{}",
                output.status.code().unwrap_or_default(),
                stdout.trim(),
                stderr.trim()
            ),
            metadata: json!({
                "command": command,
                "exit_code": output.status.code(),
            }),
        })
    }
}
