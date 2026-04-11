use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Result};

use crate::{
    config::PluginsConfig,
    core::{PluginRef, PluginRegistry, ProviderPluginRef, ResourceRegistry},
    domain::{PluginDescriptor, PluginManifest, PluginStatus, Resource},
};

#[derive(Clone)]
pub struct PluginManagerCandidate {
    pub manifest: PluginManifest,
    pub plugin: PluginRef,
    pub provider: Option<ProviderPluginRef>,
    pub resources: Vec<Resource>,
    pub default_enabled: bool,
}

impl PluginManagerCandidate {
    pub fn plugin(plugin: PluginRef, default_enabled: bool) -> Self {
        let manifest = plugin.manifest();
        Self {
            manifest,
            plugin,
            provider: None,
            resources: Vec::new(),
            default_enabled,
        }
    }

    pub fn provider(
        plugin: PluginRef,
        provider: ProviderPluginRef,
        resources: Vec<Resource>,
        default_enabled: bool,
    ) -> Self {
        let manifest = plugin.manifest();
        Self {
            manifest,
            plugin,
            provider: Some(provider),
            resources,
            default_enabled,
        }
    }
}

#[derive(Clone, Default)]
pub struct PluginManager {
    inner: Arc<Mutex<PluginManagerState>>,
}

#[derive(Default)]
struct PluginManagerState {
    config: PluginsConfig,
    discovered: BTreeMap<String, PluginManagerCandidate>,
    statuses: BTreeMap<String, PluginStatus>,
}

impl PluginManager {
    pub fn new(config: PluginsConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PluginManagerState {
                config,
                ..PluginManagerState::default()
            })),
        }
    }

    pub fn discover(&self, candidate: PluginManagerCandidate) {
        let plugin_id = candidate.manifest.id.clone();
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        let status = if state
            .config
            .is_enabled(&plugin_id, candidate.default_enabled)
        {
            PluginStatus::Discovered
        } else {
            PluginStatus::Disabled
        };
        state.statuses.insert(plugin_id.clone(), status);
        state.discovered.insert(plugin_id, candidate);
    }

    pub fn initialize(
        &self,
        registry: &mut PluginRegistry,
        resources: &mut ResourceRegistry,
    ) -> Result<()> {
        let order = {
            let state = self.inner.lock().expect("plugin manager lock poisoned");
            resolve_start_order(&state)?
        };

        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        for plugin_id in order {
            let enabled = {
                let candidate = state
                    .discovered
                    .get(&plugin_id)
                    .ok_or_else(|| anyhow!("unknown plugin {plugin_id}"))?;
                state
                    .config
                    .is_enabled(&plugin_id, candidate.default_enabled)
            };
            if !enabled {
                state.statuses.insert(plugin_id, PluginStatus::Disabled);
                continue;
            }

            let candidate = state
                .discovered
                .get(&plugin_id)
                .cloned()
                .ok_or_else(|| anyhow!("unknown plugin {plugin_id}"))?;

            state
                .statuses
                .insert(plugin_id.clone(), PluginStatus::Loading);
            registry.register(candidate.plugin.clone());
            if let Some(provider) = candidate.provider {
                registry.register_provider(provider);
            }
            resources.extend(candidate.resources);
            state
                .statuses
                .insert(plugin_id.clone(), PluginStatus::Loaded);
            state
                .statuses
                .insert(plugin_id.clone(), PluginStatus::Starting);
            state.statuses.insert(plugin_id, PluginStatus::Running);
        }
        Ok(())
    }

    pub fn descriptors(&self, api_version: &str) -> Vec<PluginDescriptor> {
        let state = self.inner.lock().expect("plugin manager lock poisoned");
        state
            .discovered
            .values()
            .map(|candidate| PluginDescriptor {
                plugin_id: candidate.manifest.id.clone(),
                version: candidate.manifest.version.clone(),
                capabilities: candidate
                    .manifest
                    .capabilities
                    .iter()
                    .map(|capability| format!("{capability:?}"))
                    .collect(),
                api_version: api_version.to_string(),
                status: state
                    .statuses
                    .get(&candidate.manifest.id)
                    .cloned()
                    .unwrap_or(PluginStatus::Discovered),
            })
            .collect()
    }

    pub fn descriptor(&self, plugin_id: &str, api_version: &str) -> Option<PluginDescriptor> {
        self.descriptors(api_version)
            .into_iter()
            .find(|descriptor| descriptor.plugin_id == plugin_id)
    }

    pub fn reload(&self, plugin_id: &str) -> Result<()> {
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        let candidate = state
            .discovered
            .get(plugin_id)
            .ok_or_else(|| anyhow!("plugin {plugin_id} not found"))?;
        if !state
            .config
            .is_enabled(plugin_id, candidate.default_enabled)
        {
            state
                .statuses
                .insert(plugin_id.to_string(), PluginStatus::Disabled);
            return Ok(());
        }
        state
            .statuses
            .insert(plugin_id.to_string(), PluginStatus::Stopping);
        state
            .statuses
            .insert(plugin_id.to_string(), PluginStatus::Starting);
        state
            .statuses
            .insert(plugin_id.to_string(), PluginStatus::Running);
        Ok(())
    }

    pub fn plugin_count(&self) -> usize {
        self.inner
            .lock()
            .expect("plugin manager lock poisoned")
            .discovered
            .len()
    }
}

fn resolve_start_order(state: &PluginManagerState) -> Result<Vec<String>> {
    let mut ordered = Vec::new();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();

    for plugin_id in state.discovered.keys() {
        visit_plugin(plugin_id, state, &mut visiting, &mut visited, &mut ordered)?;
    }

    Ok(ordered)
}

fn visit_plugin(
    plugin_id: &str,
    state: &PluginManagerState,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    ordered: &mut Vec<String>,
) -> Result<()> {
    if visited.contains(plugin_id) {
        return Ok(());
    }
    if !visiting.insert(plugin_id.to_string()) {
        return Err(anyhow!("plugin dependency cycle detected at {plugin_id}"));
    }

    let candidate = state
        .discovered
        .get(plugin_id)
        .ok_or_else(|| anyhow!("plugin {plugin_id} not found"))?;

    for dependency in &candidate.manifest.dependencies {
        if let Some(dependency_candidate) = state.discovered.get(dependency) {
            if !state
                .config
                .is_enabled(dependency, dependency_candidate.default_enabled)
            {
                return Err(anyhow!(
                    "plugin {} depends on disabled plugin {}",
                    plugin_id,
                    dependency
                ));
            }
            visit_plugin(dependency, state, visiting, visited, ordered)?;
        }
    }

    visiting.remove(plugin_id);
    visited.insert(plugin_id.to_string());
    ordered.push(plugin_id.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::{PluginManager, PluginManagerCandidate};
    use crate::{
        config::PluginsConfig,
        core::{
            Plugin, PluginRef, PluginRegistry, ProviderPlugin, ProviderPluginRef, ResourceRegistry,
        },
        domain::{Permission, PluginCapability, PluginManifest, PluginStatus, ProviderCapability},
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

    #[async_trait]
    impl ProviderPlugin for TestPlugin {
        fn provider_id(&self) -> &str {
            "provider.test"
        }

        async fn prompt_text(
            &self,
            _agent: &crate::config::AgentDefinition,
            _prompt: &str,
        ) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[test]
    fn plugin_manager_marks_disabled_plugins_from_config() {
        let mut config = PluginsConfig::default();
        config.disabled.insert("plugin.test".into());
        let manager = PluginManager::new(config);
        let plugin = Arc::new(TestPlugin {
            manifest: manifest("plugin.test", Vec::new()),
        });
        manager.discover(PluginManagerCandidate::plugin(plugin as PluginRef, true));

        let descriptor = manager
            .descriptor("plugin.test", "plugin-api.v1")
            .expect("descriptor");
        assert_eq!(descriptor.status, PluginStatus::Disabled);
    }

    #[test]
    fn plugin_manager_loads_enabled_provider_plugins_into_registry() {
        let manager = PluginManager::new(PluginsConfig::default());
        let plugin = Arc::new(TestPlugin {
            manifest: manifest(
                "provider.test",
                vec![PluginCapability::Provider(ProviderCapability::LlmText)],
            ),
        });
        manager.discover(PluginManagerCandidate::provider(
            plugin.clone() as PluginRef,
            plugin as ProviderPluginRef,
            Vec::new(),
            true,
        ));

        let mut registry = PluginRegistry::default();
        let mut resources = ResourceRegistry::default();
        manager.initialize(&mut registry, &mut resources).unwrap();

        assert!(registry.provider("provider.test").is_some());
        assert_eq!(
            manager
                .descriptor("provider.test", "plugin-api.v1")
                .expect("descriptor")
                .status,
            PluginStatus::Running
        );
    }

    #[test]
    fn plugin_manager_reload_keeps_enabled_plugin_running() {
        let manager = PluginManager::new(PluginsConfig::default());
        let plugin = Arc::new(TestPlugin {
            manifest: manifest("plugin.reloadable", Vec::new()),
        });
        manager.discover(PluginManagerCandidate::plugin(plugin as PluginRef, true));

        let mut registry = PluginRegistry::default();
        let mut resources = ResourceRegistry::default();
        manager.initialize(&mut registry, &mut resources).unwrap();
        manager.reload("plugin.reloadable").unwrap();

        assert_eq!(
            manager
                .descriptor("plugin.reloadable", "plugin-api.v1")
                .expect("descriptor")
                .status,
            PluginStatus::Running
        );
    }

    fn manifest(id: &str, capabilities: Vec<PluginCapability>) -> PluginManifest {
        PluginManifest {
            id: id.into(),
            version: "0.1.0".into(),
            capabilities,
            config_schema: None,
            required_permissions: vec![Permission::EmitEvents],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}
