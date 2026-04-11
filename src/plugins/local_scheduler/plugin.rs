use async_trait::async_trait;

use crate::{
    core::Plugin,
    domain::{Permission, PluginCapability, PluginManifest, WorkflowCapability},
};

#[derive(Debug, Clone, Default)]
pub struct LocalSchedulerPlugin;

#[async_trait]
impl Plugin for LocalSchedulerPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "workflow.local_scheduler".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Workflow(WorkflowCapability::Scheduler)],
            config_schema: None,
            required_permissions: vec![Permission::ReadState, Permission::WriteState],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
