use std::sync::Arc;

use anyhow::Result;

use crate::config::{ConfigRoot, RuntimeConfig, WorkspaceIdentityPack};
use crate::context_engine::{ContextEngine, WorkspaceContextEngine};
use crate::core::{
    ApplicationRuntime, ContextPlugin, EventBus, HookBus, PluginHost, PluginRef, PluginRegistry,
    ResourceProviderPlugin, ResourceRegistry, RuntimeHost, StoragePlugin, WorkspaceRuntime,
    WorkspaceRuntimeHost,
};
use crate::plugins::context::retrieval_bridge::RetrievalBridgeContextPlugin;
use crate::plugins::providers::openai::OpenAiProviderPlugin;
use crate::plugins::storage::{
    sqlite_backend::SqlitePersistence, sqlite_context::SqliteContextStorePlugin,
    sqlite_sessions::SqliteSessionStorePlugin,
};
use crate::plugins::tools::{
    ListFilesToolPlugin, ReadFileToolPlugin, ShellToolPlugin, ToolPlugin, ToolRegistry,
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
        let mut plugin_registry = PluginRegistry::default();
        let mut resource_registry = ResourceRegistry::default();
        let event_bus = EventBus::default();
        let hook_bus = HookBus::default();
        let mut tool_registry = ToolRegistry::default();

        for provider in &runtime_config.agent_runtime.llm.providers {
            match provider {
                crate::config::LlmProviderConfig::OpenAi(config) => {
                    let plugin = Arc::new(OpenAiProviderPlugin::new(config.clone()));
                    resource_registry.extend(plugin.provided_resources());
                    plugin_registry.register(plugin.clone() as PluginRef);
                    plugin_registry.register_provider(plugin);
                }
            }
        }

        register_tool_plugin(
            &mut plugin_registry,
            &mut tool_registry,
            Arc::new(ReadFileToolPlugin),
        );
        register_tool_plugin(
            &mut plugin_registry,
            &mut tool_registry,
            Arc::new(ListFilesToolPlugin),
        );
        register_tool_plugin(
            &mut plugin_registry,
            &mut tool_registry,
            Arc::new(ShellToolPlugin),
        );

        let context_plugin = Arc::new(RetrievalBridgeContextPlugin::new(
            &workspace_runtime.workspace.paths,
        ));
        plugin_registry.register(context_plugin.clone() as PluginRef);
        plugin_registry.register_context(context_plugin as Arc<dyn ContextPlugin>);

        let sqlite = SqlitePersistence::open(&runtime_config)?;
        let session_storage = Arc::new(SqliteSessionStorePlugin::new(sqlite.session_store()));
        plugin_registry.register(session_storage.clone() as PluginRef);
        plugin_registry.register_storage(session_storage as Arc<dyn StoragePlugin>);
        let event_storage = Arc::new(SqliteContextStorePlugin::new(sqlite.event_store()));
        plugin_registry.register(event_storage.clone() as PluginRef);
        plugin_registry.register_storage(event_storage as Arc<dyn StoragePlugin>);

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
            plugin_registry,
            resource_registry,
        })
    }
}

fn register_tool_plugin<T>(
    plugin_registry: &mut PluginRegistry,
    tool_registry: &mut ToolRegistry,
    plugin: Arc<T>,
) where
    T: ToolPlugin + 'static,
{
    plugin_registry.register(plugin.clone() as PluginRef);
    plugin_registry.register_tool(plugin.clone() as Arc<dyn ToolPlugin>);
    tool_registry.register(plugin);
}
