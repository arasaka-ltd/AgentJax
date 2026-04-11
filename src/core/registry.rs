use std::collections::BTreeMap;

use anyhow::{anyhow, Result};

use crate::builtin::tools::ToolPlugin;
use crate::core::plugin::{
    BackendPluginRef, ContextPluginRef, PluginRef, ProviderPluginRef, StoragePluginRef,
};
use crate::domain::{PluginCapability, PluginManifest};

#[derive(Default, Clone)]
pub struct PluginRegistry {
    plugins: BTreeMap<String, PluginRef>,
    providers: BTreeMap<String, ProviderPluginRef>,
    tools: BTreeMap<String, std::sync::Arc<dyn ToolPlugin>>,
    storage: BTreeMap<String, StoragePluginRef>,
    contexts: BTreeMap<String, ContextPluginRef>,
    backends: BTreeMap<String, BackendPluginRef>,
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginRef) {
        let manifest = plugin.manifest();
        self.plugins.insert(manifest.id, plugin);
    }

    pub fn register_provider(&mut self, provider: ProviderPluginRef) {
        self.providers
            .insert(provider.provider_id().to_string(), provider);
    }

    pub fn register_tool(&mut self, tool: std::sync::Arc<dyn ToolPlugin>) {
        self.tools.insert(tool.descriptor().name.clone(), tool);
    }

    pub fn register_storage(&mut self, plugin: StoragePluginRef) {
        let manifest = plugin.manifest();
        self.storage.insert(manifest.id, plugin);
    }

    pub fn register_context(&mut self, plugin: ContextPluginRef) {
        let manifest = plugin.manifest();
        self.contexts.insert(manifest.id, plugin);
    }

    pub fn register_backend(&mut self, plugin: BackendPluginRef) {
        self.backends
            .insert(plugin.backend_id().to_string(), plugin);
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        self.plugins
            .values()
            .map(|plugin| plugin.manifest())
            .collect()
    }

    pub fn plugins(&self) -> Vec<PluginRef> {
        self.plugins.values().cloned().collect()
    }

    pub fn by_capability(&self, capability: &PluginCapability) -> Vec<PluginManifest> {
        self.plugins_by_capability(capability)
            .into_iter()
            .map(|plugin| plugin.manifest())
            .collect()
    }

    pub fn plugins_by_capability(&self, capability: &PluginCapability) -> Vec<PluginRef> {
        self.plugins
            .values()
            .filter(|plugin| {
                plugin
                    .manifest()
                    .capabilities
                    .iter()
                    .any(|item| item == capability)
            })
            .cloned()
            .collect()
    }

    pub fn manifest_map(&self) -> BTreeMap<String, PluginManifest> {
        self.manifests()
            .into_iter()
            .map(|manifest| (manifest.id.clone(), manifest))
            .collect()
    }

    pub fn provider(&self, provider_id: &str) -> Option<ProviderPluginRef> {
        self.providers.get(provider_id).cloned()
    }

    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn tool(&self, name: &str) -> Option<std::sync::Arc<dyn ToolPlugin>> {
        self.tools.get(name).cloned()
    }

    pub fn storage_plugins(&self) -> Vec<StoragePluginRef> {
        self.storage.values().cloned().collect()
    }

    pub fn context_plugins(&self) -> Vec<ContextPluginRef> {
        self.contexts.values().cloned().collect()
    }

    pub fn validate_dependencies(&self) -> Result<()> {
        let manifests = self.manifest_map();
        for manifest in manifests.values() {
            for dependency in &manifest.dependencies {
                if !manifests.contains_key(dependency) {
                    return Err(anyhow!(
                        "plugin {} missing required dependency {}",
                        manifest.id,
                        dependency
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn workspace_plugins(&self, _workspace_id: &str) -> Vec<PluginManifest> {
        self.manifests()
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::PluginRegistry;
    use crate::{
        builtin::tools::{ToolDescriptor, ToolOutput, ToolPlugin},
        core::{ContextPlugin, Plugin, PluginRef, StoragePlugin},
        domain::{
            ContextCapability, Permission, PluginCapability, PluginManifest, ProviderCapability,
        },
    };

    #[derive(Clone)]
    struct TestPlugin {
        manifest: PluginManifest,
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn manifest(&self) -> PluginManifest {
            self.manifest.clone()
        }
    }

    #[derive(Clone)]
    struct TestToolPlugin;

    #[async_trait]
    impl Plugin for TestToolPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: "tool.test.echo".into(),
                version: "0.1.0".into(),
                capabilities: vec![PluginCapability::Tool(crate::domain::ToolCapability::Tool)],
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
    impl ToolPlugin for TestToolPlugin {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                name: "echo".into(),
                description: String::new(),
                when_to_use: String::new(),
                when_not_to_use: String::new(),
                arguments_schema: serde_json::json!({}),
                default_timeout_secs: 1,
                idempotent: true,
            }
        }

        async fn invoke(&self, _call: &crate::domain::ToolCall) -> anyhow::Result<ToolOutput> {
            Ok(ToolOutput {
                content: "ok".into(),
                metadata: serde_json::json!({}),
            })
        }
    }

    #[derive(Clone)]
    struct TestStoragePlugin {
        manifest: PluginManifest,
    }

    #[async_trait]
    impl Plugin for TestStoragePlugin {
        fn manifest(&self) -> PluginManifest {
            self.manifest.clone()
        }
    }

    impl StoragePlugin for TestStoragePlugin {}

    #[derive(Clone)]
    struct TestContextPlugin {
        manifest: PluginManifest,
    }

    #[async_trait]
    impl Plugin for TestContextPlugin {
        fn manifest(&self) -> PluginManifest {
            self.manifest.clone()
        }
    }

    impl ContextPlugin for TestContextPlugin {}

    #[test]
    fn registry_indexes_capabilities_and_validates_dependencies() {
        let mut registry = PluginRegistry::default();
        registry.register(Arc::new(TestPlugin {
            manifest: manifest(
                "provider.openai.test",
                vec![PluginCapability::Provider(ProviderCapability::LlmText)],
                Vec::new(),
            ),
        }) as PluginRef);
        registry.register_storage(Arc::new(TestStoragePlugin {
            manifest: manifest(
                "storage.sqlite.sessions",
                vec![PluginCapability::Memory(
                    crate::domain::MemoryCapability::Archive,
                )],
                vec!["provider.openai.test".into()],
            ),
        }));
        registry.register_context(Arc::new(TestContextPlugin {
            manifest: manifest(
                "context.workspace.retrieval",
                vec![PluginCapability::Context(ContextCapability::BlockGenerator)],
                Vec::new(),
            ),
        }));
        registry.register_tool(Arc::new(TestToolPlugin));

        assert_eq!(
            registry
                .plugins_by_capability(&PluginCapability::Provider(ProviderCapability::LlmText))
                .len(),
            1
        );
        assert_eq!(registry.storage_plugins().len(), 1);
        assert_eq!(registry.context_plugins().len(), 1);
        assert!(registry.tool("echo").is_some());
        registry.validate_dependencies().unwrap();
        assert_eq!(registry.workspace_plugins("workspace-test").len(), 1);
    }

    #[test]
    fn registry_rejects_missing_dependencies() {
        let mut registry = PluginRegistry::default();
        registry.register(Arc::new(TestPlugin {
            manifest: manifest(
                "plugin.dependent",
                vec![PluginCapability::Provider(ProviderCapability::LlmText)],
                vec!["plugin.missing".into()],
            ),
        }) as PluginRef);

        let error = registry.validate_dependencies().unwrap_err();
        assert!(error.to_string().contains("missing required dependency"));
    }

    fn manifest(
        id: &str,
        capabilities: Vec<PluginCapability>,
        dependencies: Vec<String>,
    ) -> PluginManifest {
        PluginManifest {
            id: id.into(),
            version: "0.1.0".into(),
            capabilities,
            config_schema: None,
            required_permissions: vec![Permission::ReadWorkspace],
            dependencies,
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
