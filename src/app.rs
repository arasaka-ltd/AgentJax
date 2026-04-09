use crate::config::{ConfigRoot, RuntimeConfig};
use crate::context_engine::{ContextEngine, NoopContextEngine};
use crate::core::{
    ApplicationRuntime, EventBus, PluginRegistry, ResourceProviderPlugin, ResourceRegistry,
    WorkspaceRuntime,
};
use crate::plugins::providers::openai::OpenAiProviderPlugin;

pub struct Application {
    pub config_root: ConfigRoot,
    pub runtime_config: RuntimeConfig,
    pub runtime: ApplicationRuntime,
    pub workspace_runtime: WorkspaceRuntime,
    pub plugin_registry: PluginRegistry,
    pub resource_registry: ResourceRegistry,
    pub event_bus: EventBus,
    pub context_engine: Box<dyn ContextEngine>,
}
impl Application {
    pub fn new(config_root: ConfigRoot, runtime_config: RuntimeConfig) -> Self {
        let workspace_runtime = WorkspaceRuntime::new(runtime_config.workspace.clone());
        let runtime = ApplicationRuntime::new(runtime_config.clone());
        let mut plugin_registry = PluginRegistry::default();
        let mut resource_registry = ResourceRegistry::default();

        for provider in &runtime_config.agent_runtime.llm.providers {
            match provider {
                crate::config::LlmProviderConfig::OpenAi(config) => {
                    let plugin = OpenAiProviderPlugin::new(config.clone());
                    for resource in plugin.provided_resources() {
                        resource_registry.register(resource);
                    }
                    plugin_registry.register(std::sync::Arc::new(plugin));
                }
            }
        }

        Self {
            config_root,
            runtime_config,
            runtime,
            workspace_runtime,
            plugin_registry,
            resource_registry,
            event_bus: EventBus::default(),
            context_engine: Box::new(NoopContextEngine::default()),
        }
    }
}
