use std::collections::BTreeMap;

use crate::core::plugin::PluginRef;
use crate::domain::{PluginCapability, PluginManifest};

#[derive(Default, Clone)]
pub struct PluginRegistry {
    plugins: Vec<PluginRef>,
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: PluginRef) {
        self.plugins.push(plugin);
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        self.plugins.iter().map(|plugin| plugin.manifest()).collect()
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
}
