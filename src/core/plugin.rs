use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::builtin::tools::{ToolDescriptor, ToolRegistry};
use crate::config::{AgentDefinition, RuntimeConfig};
use crate::core::{
    EventBus, EventStore, HookBus, PluginRegistry, ResourceRegistry, SessionStore,
    WorkspaceRuntimeHost,
};
use crate::domain::{
    BillingRecord, ModelTurnOutput, PluginCapability, PluginManifest, Resource, UsageRecord,
};

#[derive(Clone)]
pub struct WorkspaceHandle {
    pub runtime: WorkspaceRuntimeHost,
}

impl WorkspaceHandle {
    pub fn workspace_id(&self) -> &str {
        self.runtime.workspace_id()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionHandle {
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TurnHandle {
    pub turn_id: Option<String>,
}

#[derive(Clone)]
pub struct ModelClient {
    registry: PluginRegistry,
}

impl ModelClient {
    pub fn new(registry: PluginRegistry) -> Self {
        Self { registry }
    }

    pub fn provider_ids(&self) -> Vec<String> {
        self.registry.provider_ids()
    }

    pub fn provider(&self, provider_id: &str) -> Option<ProviderPluginRef> {
        self.registry.provider(provider_id)
    }
}

#[derive(Clone)]
pub struct ToolClient {
    registry: ToolRegistry,
}

impl ToolClient {
    pub fn new(registry: ToolRegistry) -> Self {
        Self { registry }
    }

    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.registry.descriptors()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.registry.get(name).is_some()
    }
}

#[derive(Clone)]
pub struct MemoryClient {
    registry: PluginRegistry,
}

impl MemoryClient {
    pub fn new(registry: PluginRegistry) -> Self {
        Self { registry }
    }

    pub fn plugin_ids(&self) -> Vec<String> {
        self.registry
            .plugins_by_capability(&PluginCapability::Memory(
                crate::domain::MemoryCapability::Recall,
            ))
            .into_iter()
            .map(|plugin| plugin.manifest().id)
            .collect()
    }
}

#[derive(Clone)]
pub struct KnowledgeClient {
    registry: PluginRegistry,
}

impl KnowledgeClient {
    pub fn new(registry: PluginRegistry) -> Self {
        Self { registry }
    }

    pub fn plugin_ids(&self) -> Vec<String> {
        self.registry
            .plugins_by_capability(&PluginCapability::Knowledge(
                crate::domain::KnowledgeCapability::RetrievalPolicy,
            ))
            .into_iter()
            .map(|plugin| plugin.manifest().id)
            .collect()
    }
}

#[derive(Clone)]
pub struct PluginContext {
    pub runtime_config: RuntimeConfig,
    pub workspace_runtime: WorkspaceRuntimeHost,
    pub workspace: WorkspaceHandle,
    pub session: SessionHandle,
    pub turn: TurnHandle,
    pub resources: ResourceRegistry,
    pub models: ModelClient,
    pub tools: ToolClient,
    pub memory: MemoryClient,
    pub knowledge: KnowledgeClient,
    pub events: EventBus,
    pub hooks: HookBus,
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> PluginManifest;

    async fn on_load(&self, _ctx: PluginContext) -> Result<()> {
        Ok(())
    }

    async fn on_startup(&self, _ctx: PluginContext) -> Result<()> {
        Ok(())
    }

    async fn on_shutdown(&self, _ctx: PluginContext) -> Result<()> {
        Ok(())
    }
}

pub trait ResourceProviderPlugin: Plugin {
    fn provided_resources(&self) -> Vec<Resource>;
}

#[async_trait]
pub trait ProviderPlugin: Plugin {
    fn provider_id(&self) -> &str;

    async fn prompt_turn(&self, agent: &AgentDefinition, prompt: &str) -> Result<ModelTurnOutput>;

    async fn prompt_text(&self, agent: &AgentDefinition, prompt: &str) -> Result<String> {
        Ok(self.prompt_turn(agent, prompt).await?.assistant_text())
    }
}

pub trait StoragePlugin: Plugin {
    fn session_store(&self) -> Option<Arc<dyn SessionStore>> {
        None
    }

    fn event_store(&self) -> Option<Arc<dyn EventStore>> {
        None
    }
}

pub trait ContextPlugin: Plugin {
    fn collections(&self) -> Vec<String> {
        Vec::new()
    }
}

pub trait BackendPlugin: Plugin {
    fn backend_id(&self) -> &str;
}

#[async_trait]
pub trait BillingPlugin: Plugin {
    async fn estimate_billing(&self, _usage: &UsageRecord) -> Result<Option<BillingRecord>> {
        Ok(None)
    }
}

pub type PluginRef = Arc<dyn Plugin>;
pub type ProviderPluginRef = Arc<dyn ProviderPlugin>;
pub type StoragePluginRef = Arc<dyn StoragePlugin>;
pub type ContextPluginRef = Arc<dyn ContextPlugin>;
pub type BackendPluginRef = Arc<dyn BackendPlugin>;

#[derive(Clone)]
pub struct PluginHost {
    registry: crate::core::PluginRegistry,
    tools: ToolRegistry,
    resources: ResourceRegistry,
    events: EventBus,
    hooks: HookBus,
}

impl PluginHost {
    pub fn new(
        registry: crate::core::PluginRegistry,
        tools: ToolRegistry,
        resources: ResourceRegistry,
        events: EventBus,
        hooks: HookBus,
    ) -> Self {
        Self {
            registry,
            tools,
            resources,
            events,
            hooks,
        }
    }

    pub fn registry(&self) -> &crate::core::PluginRegistry {
        &self.registry
    }

    pub fn resources(&self) -> &ResourceRegistry {
        &self.resources
    }

    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub fn events(&self) -> &EventBus {
        &self.events
    }

    pub fn hooks(&self) -> &HookBus {
        &self.hooks
    }

    pub fn build_context(
        &self,
        runtime_config: RuntimeConfig,
        workspace_runtime: WorkspaceRuntimeHost,
        session_id: Option<String>,
        turn_id: Option<String>,
    ) -> PluginContext {
        PluginContext {
            runtime_config,
            workspace: WorkspaceHandle {
                runtime: workspace_runtime.clone(),
            },
            session: SessionHandle { session_id },
            turn: TurnHandle { turn_id },
            models: ModelClient::new(self.registry.clone()),
            tools: ToolClient::new(self.tools.clone()),
            memory: MemoryClient::new(self.registry.clone()),
            knowledge: KnowledgeClient::new(self.registry.clone()),
            workspace_runtime,
            resources: self.resources.clone(),
            events: self.events.clone(),
            hooks: self.hooks.clone(),
        }
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        self.registry.manifests()
    }

    pub async fn on_load(
        &self,
        runtime_config: RuntimeConfig,
        workspace_runtime: WorkspaceRuntimeHost,
    ) -> Result<()> {
        self.run_lifecycle("load", runtime_config, workspace_runtime)
            .await
    }

    pub async fn on_startup(
        &self,
        runtime_config: RuntimeConfig,
        workspace_runtime: WorkspaceRuntimeHost,
    ) -> Result<()> {
        self.run_lifecycle("startup", runtime_config, workspace_runtime)
            .await
    }

    pub async fn on_shutdown(
        &self,
        runtime_config: RuntimeConfig,
        workspace_runtime: WorkspaceRuntimeHost,
    ) -> Result<()> {
        self.run_lifecycle("shutdown", runtime_config, workspace_runtime)
            .await
    }

    async fn run_lifecycle(
        &self,
        stage: &str,
        runtime_config: RuntimeConfig,
        workspace_runtime: WorkspaceRuntimeHost,
    ) -> Result<()> {
        self.registry.validate_dependencies()?;
        for plugin in self.registry.plugins() {
            let manifest = plugin.manifest();
            if stage == "load" {
                for hook in &manifest.hooks {
                    self.hooks.register(manifest.id.clone(), hook.clone());
                }
            }
            let ctx = self.build_context(
                runtime_config.clone(),
                workspace_runtime.clone(),
                None,
                None,
            );
            match stage {
                "load" => plugin.on_load(ctx).await?,
                "startup" => plugin.on_startup(ctx).await?,
                "shutdown" => plugin.on_shutdown(ctx).await?,
                _ => unreachable!("unsupported plugin lifecycle stage"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::{anyhow, Result};
    use async_trait::async_trait;

    use super::{Plugin, PluginHost, PluginRef};
    use crate::{
        builtin::tools::ToolRegistry,
        config::{RuntimeConfig, RuntimePaths, WorkspaceConfig, WorkspaceDocument, WorkspacePaths},
        core::{
            EventBus, HookBus, PluginContext, PluginRegistry, ResourceRegistry,
            WorkspaceRuntimeHost,
        },
        domain::{HookPoint, Permission, PluginCapability, PluginManifest},
    };

    #[derive(Clone)]
    struct RecordingPlugin {
        id: String,
        hooks: Vec<HookPoint>,
        fail_on_startup: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Plugin for RecordingPlugin {
        fn manifest(&self) -> PluginManifest {
            PluginManifest {
                id: self.id.clone(),
                version: "0.1.0".into(),
                capabilities: vec![PluginCapability::Hook(
                    crate::domain::HookCapability::Lifecycle,
                )],
                config_schema: None,
                required_permissions: vec![Permission::EmitEvents],
                dependencies: Vec::new(),
                optional_dependencies: Vec::new(),
                provided_resources: Vec::new(),
                hooks: self.hooks.clone(),
            }
        }

        async fn on_load(&self, ctx: PluginContext) -> Result<()> {
            self.calls
                .lock()
                .expect("calls lock poisoned")
                .push(format!("load:{}:{}", self.id, ctx.workspace.workspace_id()));
            Ok(())
        }

        async fn on_startup(&self, _ctx: PluginContext) -> Result<()> {
            if self.fail_on_startup {
                Err(anyhow!("startup failed for {}", self.id))
            } else {
                self.calls
                    .lock()
                    .expect("calls lock poisoned")
                    .push(format!("startup:{}", self.id));
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn plugin_host_runs_lifecycle_and_registers_manifest_hooks() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let plugin = Arc::new(RecordingPlugin {
            id: "plugin.test".into(),
            hooks: vec![HookPoint::BeforeTurn, HookPoint::AfterTurn],
            fail_on_startup: false,
            calls: calls.clone(),
        });

        let mut registry = PluginRegistry::default();
        registry.register(plugin as PluginRef);
        let hook_bus = HookBus::default();
        let host = PluginHost::new(
            registry,
            ToolRegistry::default(),
            ResourceRegistry::default(),
            EventBus::default(),
            hook_bus.clone(),
        );

        host.on_load(runtime_config(), workspace_runtime())
            .await
            .unwrap();
        host.on_startup(runtime_config(), workspace_runtime())
            .await
            .unwrap();

        assert_eq!(
            calls.lock().expect("calls lock poisoned").clone(),
            vec![
                "load:plugin.test:workspace-test".to_string(),
                "startup:plugin.test".to_string(),
            ]
        );
        assert_eq!(hook_bus.registrations().len(), 2);
    }

    #[tokio::test]
    async fn plugin_host_propagates_lifecycle_errors() {
        let plugin = Arc::new(RecordingPlugin {
            id: "plugin.fail".into(),
            hooks: Vec::new(),
            fail_on_startup: true,
            calls: Arc::new(Mutex::new(Vec::new())),
        });

        let mut registry = PluginRegistry::default();
        registry.register(plugin as PluginRef);
        let host = PluginHost::new(
            registry,
            ToolRegistry::default(),
            ResourceRegistry::default(),
            EventBus::default(),
            HookBus::default(),
        );

        let error = host
            .on_startup(runtime_config(), workspace_runtime())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("startup failed"));
    }

    fn runtime_config() -> RuntimeConfig {
        let root = std::env::temp_dir().join("agentjax-plugin-host-runtime");
        RuntimeConfig::new(
            "AgentJax",
            RuntimePaths::new(root.join("runtime")),
            WorkspaceConfig::new(
                "workspace-test",
                WorkspacePaths::new(root.join("workspace")),
            ),
        )
    }

    fn workspace_runtime() -> WorkspaceRuntimeHost {
        let root = std::env::temp_dir().join("agentjax-plugin-host-workspace");
        let paths = WorkspacePaths::new(root);
        WorkspaceRuntimeHost::new(
            WorkspaceConfig::new("workspace-test", paths.clone()),
            crate::config::WorkspaceIdentityPack {
                workspace_id: "workspace-test".into(),
                agent: WorkspaceDocument {
                    path: paths.agent_file.clone(),
                    content: String::new(),
                },
                soul: WorkspaceDocument {
                    path: paths.soul_file.clone(),
                    content: String::new(),
                },
                user: WorkspaceDocument {
                    path: paths.user_file.clone(),
                    content: String::new(),
                },
                memory: WorkspaceDocument {
                    path: paths.memory_file.clone(),
                    content: String::new(),
                },
                mission: WorkspaceDocument {
                    path: paths.mission_file.clone(),
                    content: String::new(),
                },
                rules: WorkspaceDocument {
                    path: paths.rules_file.clone(),
                    content: String::new(),
                },
                router: WorkspaceDocument {
                    path: paths.router_file.clone(),
                    content: String::new(),
                },
            },
        )
    }
}
