use std::sync::Arc;

use anyhow::Result;

use crate::builtin::tools::ToolRegistry;
use crate::config::{ConfigRoot, RuntimeConfig, WorkspaceIdentityPack};
use crate::context_engine::{ContextEngine, WorkspaceContextEngine};
use crate::core::{
    ApplicationRuntime, EventBus, HookBus, PluginHost, PluginManager, PluginManagerCandidate,
    PluginRef, PluginRegistry, ResourceProviderPlugin, ResourceRegistry, RuntimeHost,
    WorkspaceRuntime, WorkspaceRuntimeHost,
};
use crate::plugins::{
    local_scheduler::LocalSchedulerPlugin, openai::OpenAiProviderPlugin,
    static_nodes::StaticNodeRegistryPlugin, telegram::TelegramChannelPlugin,
};

#[derive(Clone)]
pub struct Application {
    pub config_root: ConfigRoot,
    pub runtime_config: RuntimeConfig,
    pub workspace_identity: WorkspaceIdentityPack,
    pub plugin_host: PluginHost,
    pub workspace_host: WorkspaceRuntimeHost,
    pub runtime_host: RuntimeHost,
    pub tool_registry: ToolRegistry,
    pub event_bus: EventBus,
    pub context_engine: Arc<dyn ContextEngine>,
    pub runtime: Arc<ApplicationRuntime>,
    pub workspace_runtime: WorkspaceRuntime,
    pub plugin_manager: PluginManager,
    pub plugin_registry: PluginRegistry,
    pub resource_registry: ResourceRegistry,
}
impl Application {
    pub fn new(
        config_root: ConfigRoot,
        runtime_config: RuntimeConfig,
        workspace_identity: WorkspaceIdentityPack,
    ) -> Result<Self> {
        let workspace_host =
            WorkspaceRuntimeHost::new(runtime_config.workspace.clone(), workspace_identity.clone());
        let workspace_runtime = workspace_host.workspace_runtime.clone();
        let plugin_manager = PluginManager::new(runtime_config.plugins.clone());
        let mut plugin_registry = PluginRegistry::default();
        let mut resource_registry = ResourceRegistry::default();
        let event_bus = EventBus::default();
        let hook_bus = HookBus::default();
        let tool_registry = ToolRegistry::builtins();

        for provider in &runtime_config.agent_runtime.llm.providers {
            match provider {
                crate::config::LlmProviderConfig::OpenAi(config) => {
                    let plugin = Arc::new(OpenAiProviderPlugin::new(config.clone()));
                    plugin_manager.discover(PluginManagerCandidate::provider(
                        plugin.clone() as PluginRef,
                        plugin.clone(),
                        plugin.provided_resources(),
                        true,
                    ));
                }
            }
        }

        plugin_manager.discover(PluginManagerCandidate::plugin(
            Arc::new(TelegramChannelPlugin) as PluginRef,
            false,
        ));
        plugin_manager.discover(PluginManagerCandidate::plugin(
            Arc::new(LocalSchedulerPlugin) as PluginRef,
            false,
        ));
        plugin_manager.discover(PluginManagerCandidate::plugin(
            Arc::new(StaticNodeRegistryPlugin) as PluginRef,
            false,
        ));
        plugin_manager.initialize(&mut plugin_registry, &mut resource_registry)?;

        let plugin_host = PluginHost::new(
            plugin_registry.clone(),
            tool_registry.clone(),
            resource_registry.clone(),
            event_bus.clone(),
            hook_bus,
        );
        let runtime = Arc::new(ApplicationRuntime::new(
            runtime_config.clone(),
            plugin_host.clone(),
            workspace_host.clone(),
        ));
        let runtime_host = RuntimeHost::new((*runtime).clone());

        Ok(Self {
            config_root,
            runtime_config,
            workspace_identity: workspace_identity.clone(),
            plugin_host,
            workspace_host,
            runtime_host,
            tool_registry,
            event_bus,
            context_engine: Arc::new(WorkspaceContextEngine::new(
                workspace_identity,
                workspace_runtime.workspace.paths.clone(),
            )),
            runtime,
            workspace_runtime,
            plugin_manager,
            plugin_registry,
            resource_registry,
        })
    }
}
