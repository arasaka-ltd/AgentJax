use async_trait::async_trait;

use crate::{
    core::{ContextPlugin, Plugin},
    domain::{
        ContextCapability, Permission, PluginCapability, PluginManifest, ResourceDescriptor,
        ResourceId,
    },
};

#[derive(Debug, Clone, Default)]
pub struct SummaryLoaderContextPlugin;

#[async_trait]
impl Plugin for SummaryLoaderContextPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "context.summary_loader".into(),
            version: "0.1.0".into(),
            capabilities: vec![
                PluginCapability::Context(ContextCapability::BlockGenerator),
                PluginCapability::Context(ContextCapability::PromptRenderer),
            ],
            config_schema: None,
            required_permissions: vec![Permission::ReadState],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: vec![ResourceDescriptor {
                resource_id: ResourceId("context:summary_loader".into()),
                kind: "context.summary_loader".into(),
                description: Some("Summary and checkpoint context projection loader".into()),
            }],
            hooks: Vec::new(),
        }
    }
}

impl ContextPlugin for SummaryLoaderContextPlugin {
    fn collections(&self) -> Vec<String> {
        vec!["runtime/summaries".into(), "runtime/checkpoints".into()]
    }
}
