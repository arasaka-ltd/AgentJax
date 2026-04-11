use async_trait::async_trait;

use crate::{
    core::{ContextPlugin, Plugin},
    domain::{
        ContextCapability, Permission, PluginCapability, PluginManifest, ResourceDescriptor,
        ResourceId,
    },
};

#[derive(Debug, Clone, Default)]
pub struct TaskStateContextPlugin;

#[async_trait]
impl Plugin for TaskStateContextPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "context.task_state".into(),
            version: "0.1.0".into(),
            capabilities: vec![
                PluginCapability::Context(ContextCapability::BlockGenerator),
                PluginCapability::Context(ContextCapability::Selector),
            ],
            config_schema: None,
            required_permissions: vec![Permission::ReadState],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: vec![ResourceDescriptor {
                resource_id: ResourceId("context:task_state".into()),
                kind: "context.task_state".into(),
                description: Some(
                    "Task runtime state block provider for checkpoints and active task metadata"
                        .into(),
                ),
            }],
            hooks: Vec::new(),
        }
    }
}

impl ContextPlugin for TaskStateContextPlugin {
    fn collections(&self) -> Vec<String> {
        vec!["runtime/tasks".into(), "runtime/checkpoints".into()]
    }
}
