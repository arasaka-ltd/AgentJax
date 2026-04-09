use crate::config::{ConfigRoot, RuntimeConfig};
use crate::context_engine::{ContextEngine, NoopContextEngine};
use crate::core::{EventBus, PluginRegistry, ResourceRegistry, WorkspaceRuntime};
pub struct Application {
    pub config_root: ConfigRoot,
    pub runtime_config: RuntimeConfig,
    pub workspace_runtime: WorkspaceRuntime,
    pub plugin_registry: PluginRegistry,
    pub resource_registry: ResourceRegistry,
    pub event_bus: EventBus,
    pub context_engine: Box<dyn ContextEngine>,
}
impl Application {
    pub fn new(config_root: ConfigRoot, runtime_config: RuntimeConfig) -> Self {
        let workspace_runtime = WorkspaceRuntime::new(runtime_config.workspace.clone());
        Self {
            config_root,
            runtime_config,
            workspace_runtime,
            plugin_registry: PluginRegistry::default(),
            resource_registry: ResourceRegistry::default(),
            event_bus: EventBus::default(),
            context_engine: Box::new(NoopContextEngine::default()),
        }
    }
}
