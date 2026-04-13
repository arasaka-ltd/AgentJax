use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Result};

use crate::{
    builtin::tools::ToolRegistry,
    config::{PluginsConfig, RuntimeConfig},
    core::{
        EventBus, HookBus, PluginHost, PluginRef, PluginRegistry, ProviderPluginRef,
        ResourceRegistry, WorkspaceRuntimeHost,
    },
    domain::{PluginDescriptor, PluginManifest, PluginStatus, Resource},
};

#[derive(Clone)]
pub struct PluginManagerCandidate {
    pub manifest: PluginManifest,
    pub plugin: PluginRef,
    pub provider: Option<ProviderPluginRef>,
    pub billing: Option<crate::core::plugin::BillingPluginRef>,
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
            billing: None,
            resources: Vec::new(),
            default_enabled,
        }
    }

    pub fn provider(
        plugin: PluginRef,
        provider: ProviderPluginRef,
        billing: Option<crate::core::plugin::BillingPluginRef>,
        resources: Vec<Resource>,
        default_enabled: bool,
    ) -> Self {
        let manifest = plugin.manifest();
        Self {
            manifest,
            plugin,
            provider: Some(provider),
            billing,
            resources,
            default_enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRuntimeSnapshot {
    pub plugin: PluginDescriptor,
    pub enabled: bool,
    pub default_enabled: bool,
    pub dependencies: Vec<String>,
    pub optional_dependencies: Vec<String>,
    pub required_permissions: Vec<String>,
    pub provided_resources: Vec<String>,
    pub config_ref: Option<String>,
    pub policy_flags: BTreeMap<String, bool>,
    pub reload_hint: Option<String>,
    pub last_lifecycle_stage: Option<String>,
    pub last_error: Option<String>,
    pub healthy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginActionReport {
    pub plugin_id: String,
    pub ok: bool,
    pub status: PluginStatus,
    pub lifecycle_stage: Option<String>,
    pub summary: String,
    pub checks: Vec<String>,
}

#[derive(Clone, Default)]
pub struct PluginManager {
    inner: Arc<Mutex<PluginManagerState>>,
}

#[derive(Default)]
struct PluginManagerState {
    config: PluginsConfig,
    discovered: BTreeMap<String, PluginManagerCandidate>,
    runtime: BTreeMap<String, PluginRuntimeRecord>,
}

#[derive(Debug, Clone)]
struct PluginRuntimeRecord {
    status: PluginStatus,
    enabled: bool,
    default_enabled: bool,
    config_ref: Option<String>,
    policy_flags: BTreeMap<String, bool>,
    reload_hint: Option<String>,
    last_lifecycle_stage: Option<String>,
    last_error: Option<String>,
    hooks_registered: bool,
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
        let enabled = state
            .config
            .is_enabled(&plugin_id, candidate.default_enabled);
        let config_ref = state.config.config_ref(&plugin_id).map(str::to_string);
        let policy_flags = state.config.policy_flags_for(&plugin_id);
        let reload_hint = state.config.reload_hint(&plugin_id).map(str::to_string);
        state.runtime.insert(
            plugin_id.clone(),
            PluginRuntimeRecord {
                status: if enabled {
                    PluginStatus::Discovered
                } else {
                    PluginStatus::Disabled
                },
                enabled,
                default_enabled: candidate.default_enabled,
                config_ref,
                policy_flags,
                reload_hint,
                last_lifecycle_stage: None,
                last_error: None,
                hooks_registered: false,
            },
        );
        state.discovered.insert(plugin_id, candidate);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        &self,
        registry: &mut PluginRegistry,
        resources: &mut ResourceRegistry,
        tools: &ToolRegistry,
        events: &EventBus,
        hooks: &HookBus,
        runtime_config: &RuntimeConfig,
        workspace_runtime: &WorkspaceRuntimeHost,
    ) -> Result<()> {
        let order = {
            let state = self.inner.lock().expect("plugin manager lock poisoned");
            resolve_start_order(&state)?
        };

        for plugin_id in order {
            let candidate = {
                let mut state = self.inner.lock().expect("plugin manager lock poisoned");
                let candidate = state
                    .discovered
                    .get(&plugin_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("unknown plugin {plugin_id}"))?;
                let enabled = state
                    .config
                    .is_enabled(&plugin_id, candidate.default_enabled);
                let config_ref = state.config.config_ref(&plugin_id).map(str::to_string);
                let policy_flags = state.config.policy_flags_for(&plugin_id);
                let reload_hint = state.config.reload_hint(&plugin_id).map(str::to_string);
                let runtime = state
                    .runtime
                    .get_mut(&plugin_id)
                    .ok_or_else(|| anyhow!("unknown plugin runtime {plugin_id}"))?;
                runtime.enabled = enabled;
                runtime.config_ref = config_ref;
                runtime.policy_flags = policy_flags;
                runtime.reload_hint = reload_hint;
                runtime.default_enabled = candidate.default_enabled;
                if !runtime.enabled {
                    runtime.status = PluginStatus::Disabled;
                    runtime.last_lifecycle_stage = None;
                    runtime.last_error = None;
                }
                candidate
            };

            let enabled = self
                .snapshot(&plugin_id, &runtime_config.plugin_api_version)
                .ok_or_else(|| anyhow!("unknown plugin snapshot {plugin_id}"))?
                .enabled;
            if !enabled {
                continue;
            }

            self.register_candidate(&candidate, registry, resources);
            self.run_lifecycle(
                &plugin_id,
                &candidate,
                registry,
                resources,
                tools,
                events,
                hooks,
                runtime_config,
                workspace_runtime,
                &[LifecycleStage::Load, LifecycleStage::Startup],
            )?;
        }
        Ok(())
    }

    pub fn descriptors(&self, api_version: &str) -> Vec<PluginDescriptor> {
        let state = self.inner.lock().expect("plugin manager lock poisoned");
        state
            .discovered
            .values()
            .map(|candidate| descriptor_for_candidate(candidate, &state, api_version))
            .collect()
    }

    pub fn descriptor(&self, plugin_id: &str, api_version: &str) -> Option<PluginDescriptor> {
        self.snapshot(plugin_id, api_version)
            .map(|snapshot| snapshot.plugin)
    }

    pub fn snapshot(&self, plugin_id: &str, api_version: &str) -> Option<PluginRuntimeSnapshot> {
        let state = self.inner.lock().expect("plugin manager lock poisoned");
        let candidate = state.discovered.get(plugin_id)?;
        let runtime = state.runtime.get(plugin_id)?;
        Some(snapshot_for_candidate(candidate, runtime, api_version))
    }

    pub fn snapshots(&self, api_version: &str) -> Vec<PluginRuntimeSnapshot> {
        let state = self.inner.lock().expect("plugin manager lock poisoned");
        state
            .discovered
            .values()
            .filter_map(|candidate| {
                state
                    .runtime
                    .get(&candidate.manifest.id)
                    .map(|runtime| snapshot_for_candidate(candidate, runtime, api_version))
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reload(
        &self,
        plugin_id: &str,
        registry: &PluginRegistry,
        resources: &ResourceRegistry,
        tools: &ToolRegistry,
        events: &EventBus,
        hooks: &HookBus,
        runtime_config: &RuntimeConfig,
        workspace_runtime: &WorkspaceRuntimeHost,
    ) -> Result<PluginActionReport> {
        let candidate = {
            let mut state = self.inner.lock().expect("plugin manager lock poisoned");
            let candidate = state
                .discovered
                .get(plugin_id)
                .cloned()
                .ok_or_else(|| anyhow!("plugin {plugin_id} not found"))?;
            let enabled = state
                .config
                .is_enabled(plugin_id, candidate.default_enabled);
            let config_ref = state.config.config_ref(plugin_id).map(str::to_string);
            let policy_flags = state.config.policy_flags_for(plugin_id);
            let reload_hint = state.config.reload_hint(plugin_id).map(str::to_string);
            let runtime = state
                .runtime
                .get_mut(plugin_id)
                .ok_or_else(|| anyhow!("plugin runtime {plugin_id} not found"))?;
            runtime.enabled = enabled;
            runtime.config_ref = config_ref;
            runtime.policy_flags = policy_flags;
            runtime.reload_hint = reload_hint;
            candidate
        };

        let snapshot = self
            .snapshot(plugin_id, &runtime_config.plugin_api_version)
            .ok_or_else(|| anyhow!("plugin {plugin_id} not found"))?;
        if !snapshot.enabled {
            self.set_status(plugin_id, PluginStatus::Disabled, None, None);
            return Ok(PluginActionReport {
                plugin_id: plugin_id.to_string(),
                ok: true,
                status: PluginStatus::Disabled,
                lifecycle_stage: None,
                summary: format!("plugin {plugin_id} is disabled by plugins.toml"),
                checks: vec!["enabled=false".into()],
            });
        }

        self.run_lifecycle(
            plugin_id,
            &candidate,
            registry,
            resources,
            tools,
            events,
            hooks,
            runtime_config,
            workspace_runtime,
            &[
                LifecycleStage::Shutdown,
                LifecycleStage::Load,
                LifecycleStage::Startup,
            ],
        )?;
        let updated = self
            .snapshot(plugin_id, &runtime_config.plugin_api_version)
            .ok_or_else(|| anyhow!("plugin {plugin_id} not found after reload"))?;
        let status = updated.plugin.status.clone();
        Ok(PluginActionReport {
            plugin_id: plugin_id.to_string(),
            ok: updated.healthy,
            status: status.clone(),
            lifecycle_stage: updated.last_lifecycle_stage,
            summary: format!("plugin {plugin_id} reloaded with status {:?}", status),
            checks: vec![
                "shutdown completed".into(),
                "load completed".into(),
                "startup completed".into(),
            ],
        })
    }

    pub fn test_plugin(&self, plugin_id: &str, api_version: &str) -> Result<PluginActionReport> {
        let snapshot = self
            .snapshot(plugin_id, api_version)
            .ok_or_else(|| anyhow!("plugin {plugin_id} not found"))?;
        let mut checks = Vec::new();
        checks.push(if snapshot.enabled {
            "enabled=true".into()
        } else {
            "enabled=false".into()
        });
        checks.push(format!("status={:?}", snapshot.plugin.status));
        if let Some(config_ref) = &snapshot.config_ref {
            checks.push(format!("config_ref={config_ref}"));
        } else {
            checks.push("config_ref=none".into());
        }
        if let Some(reload_hint) = &snapshot.reload_hint {
            checks.push(format!("reload_hint={reload_hint}"));
        } else {
            checks.push("reload_hint=none".into());
        }
        if snapshot.policy_flags.is_empty() {
            checks.push("policy_flags=none".into());
        } else {
            checks.push(format!("policy_flags={}", snapshot.policy_flags.len()));
        }
        if snapshot.dependencies.is_empty() {
            checks.push("dependencies=none".into());
        } else {
            checks.push(format!("dependencies={}", snapshot.dependencies.join(",")));
        }
        if let Some(last_error) = &snapshot.last_error {
            checks.push(format!("last_error={last_error}"));
        }

        Ok(PluginActionReport {
            plugin_id: plugin_id.to_string(),
            ok: snapshot.enabled && snapshot.healthy,
            status: snapshot.plugin.status,
            lifecycle_stage: snapshot.last_lifecycle_stage,
            summary: if snapshot.enabled {
                format!("plugin {plugin_id} is manager-visible and lifecycle-ready")
            } else {
                format!("plugin {plugin_id} is disabled by plugins.toml")
            },
            checks,
        })
    }

    pub fn plugin_count(&self) -> usize {
        self.inner
            .lock()
            .expect("plugin manager lock poisoned")
            .discovered
            .len()
    }

    fn register_candidate(
        &self,
        candidate: &PluginManagerCandidate,
        registry: &mut PluginRegistry,
        resources: &mut ResourceRegistry,
    ) {
        registry.register(candidate.plugin.clone());
        if let Some(provider) = &candidate.provider {
            registry.register_provider(provider.clone());
        }
        if let Some(billing) = &candidate.billing {
            registry.register_billing(candidate.manifest.id.clone(), billing.clone());
        }
        resources.extend(candidate.resources.clone());
    }

    #[allow(clippy::too_many_arguments)]
    fn run_lifecycle(
        &self,
        plugin_id: &str,
        candidate: &PluginManagerCandidate,
        registry: &PluginRegistry,
        resources: &ResourceRegistry,
        tools: &ToolRegistry,
        events: &EventBus,
        hooks: &HookBus,
        runtime_config: &RuntimeConfig,
        workspace_runtime: &WorkspaceRuntimeHost,
        stages: &[LifecycleStage],
    ) -> Result<()> {
        for stage in stages {
            self.begin_stage(plugin_id, stage.status(), stage.label());
            if matches!(stage, LifecycleStage::Load) {
                self.register_hooks_once(plugin_id, &candidate.manifest, hooks);
            }
            let host = PluginHost::new(
                registry.clone(),
                tools.clone(),
                resources.clone(),
                events.clone(),
                hooks.clone(),
            );
            let ctx = host.build_context(
                runtime_config.clone(),
                workspace_runtime.clone(),
                Some(plugin_id.to_string()),
                None,
                None,
            );
            let plugin = candidate.plugin.clone();
            let stage_label = stage.label().to_string();
            let result = run_async(stage.run(plugin, ctx));
            match result {
                Ok(()) => self.finish_stage(plugin_id, stage.success_status(), &stage_label),
                Err(error) => {
                    self.fail_stage(plugin_id, &stage_label, &error.to_string());
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn begin_stage(&self, plugin_id: &str, status: PluginStatus, stage: &str) {
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        if let Some(runtime) = state.runtime.get_mut(plugin_id) {
            runtime.status = status;
            runtime.last_lifecycle_stage = Some(stage.to_string());
            runtime.last_error = None;
        }
    }

    fn finish_stage(&self, plugin_id: &str, status: PluginStatus, stage: &str) {
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        if let Some(runtime) = state.runtime.get_mut(plugin_id) {
            runtime.status = status;
            runtime.last_lifecycle_stage = Some(stage.to_string());
            runtime.last_error = None;
        }
    }

    fn fail_stage(&self, plugin_id: &str, stage: &str, error: &str) {
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        if let Some(runtime) = state.runtime.get_mut(plugin_id) {
            runtime.status = PluginStatus::Failed;
            runtime.last_lifecycle_stage = Some(stage.to_string());
            runtime.last_error = Some(error.to_string());
        }
    }

    fn set_status(
        &self,
        plugin_id: &str,
        status: PluginStatus,
        stage: Option<&str>,
        error: Option<&str>,
    ) {
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        if let Some(runtime) = state.runtime.get_mut(plugin_id) {
            runtime.status = status;
            runtime.last_lifecycle_stage = stage.map(str::to_string);
            runtime.last_error = error.map(str::to_string);
        }
    }

    fn register_hooks_once(&self, plugin_id: &str, manifest: &PluginManifest, hooks: &HookBus) {
        let mut state = self.inner.lock().expect("plugin manager lock poisoned");
        let Some(runtime) = state.runtime.get_mut(plugin_id) else {
            return;
        };
        if runtime.hooks_registered {
            return;
        }
        for hook in &manifest.hooks {
            hooks.register(manifest.id.clone(), hook.clone());
        }
        runtime.hooks_registered = true;
    }
}

#[derive(Clone, Copy)]
enum LifecycleStage {
    Load,
    Startup,
    Shutdown,
}

impl LifecycleStage {
    fn label(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Startup => "startup",
            Self::Shutdown => "shutdown",
        }
    }

    fn status(self) -> PluginStatus {
        match self {
            Self::Load => PluginStatus::Loading,
            Self::Startup => PluginStatus::Starting,
            Self::Shutdown => PluginStatus::Stopping,
        }
    }

    fn success_status(self) -> PluginStatus {
        match self {
            Self::Load => PluginStatus::Loaded,
            Self::Startup => PluginStatus::Running,
            Self::Shutdown => PluginStatus::Stopped,
        }
    }

    async fn run(self, plugin: PluginRef, ctx: crate::core::PluginContext) -> Result<()> {
        match self {
            Self::Load => plugin.on_load(ctx).await,
            Self::Startup => plugin.on_startup(ctx).await,
            Self::Shutdown => plugin.on_shutdown(ctx).await,
        }
    }
}

fn descriptor_for_candidate(
    candidate: &PluginManagerCandidate,
    state: &PluginManagerState,
    api_version: &str,
) -> PluginDescriptor {
    let runtime = state.runtime.get(&candidate.manifest.id);
    PluginDescriptor {
        plugin_id: candidate.manifest.id.clone(),
        version: candidate.manifest.version.clone(),
        capabilities: candidate
            .manifest
            .capabilities
            .iter()
            .map(|capability| format!("{capability:?}"))
            .collect(),
        api_version: api_version.to_string(),
        status: runtime
            .map(|runtime| runtime.status.clone())
            .unwrap_or(PluginStatus::Discovered),
    }
}

fn snapshot_for_candidate(
    candidate: &PluginManagerCandidate,
    runtime: &PluginRuntimeRecord,
    api_version: &str,
) -> PluginRuntimeSnapshot {
    PluginRuntimeSnapshot {
        plugin: PluginDescriptor {
            plugin_id: candidate.manifest.id.clone(),
            version: candidate.manifest.version.clone(),
            capabilities: candidate
                .manifest
                .capabilities
                .iter()
                .map(|capability| format!("{capability:?}"))
                .collect(),
            api_version: api_version.to_string(),
            status: runtime.status.clone(),
        },
        enabled: runtime.enabled,
        default_enabled: runtime.default_enabled,
        dependencies: candidate.manifest.dependencies.clone(),
        optional_dependencies: candidate.manifest.optional_dependencies.clone(),
        required_permissions: candidate
            .manifest
            .required_permissions
            .iter()
            .map(|permission| format!("{permission:?}"))
            .collect(),
        provided_resources: candidate
            .resources
            .iter()
            .map(|resource| resource.resource_id.0.clone())
            .collect(),
        config_ref: runtime.config_ref.clone(),
        policy_flags: runtime.policy_flags.clone(),
        reload_hint: runtime.reload_hint.clone(),
        last_lifecycle_stage: runtime.last_lifecycle_stage.clone(),
        last_error: runtime.last_error.clone(),
        healthy: runtime.enabled && !matches!(runtime.status, PluginStatus::Failed),
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

fn run_async<F>(future: F) -> Result<()>
where
    F: std::future::Future<Output = Result<()>> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| handle.block_on(future))
            }
            tokio::runtime::RuntimeFlavor::CurrentThread => std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(future)
            })
            .join()
            .map_err(|_| anyhow!("plugin lifecycle thread panicked"))?,
            _ => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(future),
        }
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(future)
    }
}
