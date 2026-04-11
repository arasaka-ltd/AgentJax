use async_trait::async_trait;

use crate::{
    core::{ContextPlugin, Plugin},
    domain::{
        ContextCapability, Permission, PluginCapability, PluginManifest, ResourceDescriptor,
        ResourceId,
    },
};

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIdentityContextPlugin;

#[async_trait]
impl Plugin for WorkspaceIdentityContextPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "context.workspace_identity".into(),
            version: "0.1.0".into(),
            capabilities: vec![
                PluginCapability::Context(ContextCapability::BlockGenerator),
                PluginCapability::Context(ContextCapability::Selector),
            ],
            config_schema: None,
            required_permissions: vec![Permission::ReadWorkspace],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: vec![ResourceDescriptor {
                resource_id: ResourceId("context:workspace_identity".into()),
                kind: "context.workspace_identity".into(),
                description: Some(
                    "Workspace identity document loader for AGENT/SOUL/USER/MISSION/RULES/ROUTER"
                        .into(),
                ),
            }],
            hooks: Vec::new(),
        }
    }
}

impl ContextPlugin for WorkspaceIdentityContextPlugin {
    fn collections(&self) -> Vec<String> {
        vec![
            "workspace/AGENT.md".into(),
            "workspace/SOUL.md".into(),
            "workspace/USER.md".into(),
            "workspace/MISSION.md".into(),
            "workspace/RULES.md".into(),
            "workspace/ROUTER.md".into(),
        ]
    }
}
