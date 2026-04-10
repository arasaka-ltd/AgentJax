use std::collections::BTreeMap;

use crate::core::plugin::{PluginRef, ProviderPluginRef};
use crate::domain::{PluginCapability, PluginManifest};

#[derive(Default, Clone)]
pub struct PluginRegistry {
    plugins: Vec<PluginRef>,
    providers: BTreeMap<String, ProviderPluginRef>,
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginRef) {
        self.plugins.push(plugin);
    }

    pub fn register_provider(&mut self, provider: ProviderPluginRef) {
        self.providers
            .insert(provider.provider_id().to_string(), provider);
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        self.plugins
            .iter()
            .map(|plugin| plugin.manifest())
            .collect()
    }

    pub fn plugins(&self) -> &[PluginRef] {
        &self.plugins
    }

    pub fn by_capability(&self, capability: &PluginCapability) -> Vec<PluginManifest> {
        self.manifests()
            .into_iter()
            .filter(|manifest| manifest.capabilities.iter().any(|item| item == capability))
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

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}
