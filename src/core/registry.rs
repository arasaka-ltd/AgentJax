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
