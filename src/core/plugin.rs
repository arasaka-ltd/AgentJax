use std::{pin::Pin, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use futures_util::{stream, Stream};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::builtin::tools::{ToolDescriptor, ToolRegistry};
use crate::config::{secrets::resolve_secret_refs, AgentDefinition, RuntimeConfig};
use crate::core::{
    EventBus, EventStore, HookBus, PluginRegistry, ResourceRegistry, SessionStore,
    WorkspaceRuntimeHost,
};
use crate::domain::{
    BillingRecord, ModelOutputItem, ModelStreamEvent, ModelTurnOutput, PluginCapability,
    PluginManifest, Resource, UsageRecord,
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

pub type ModelEventStream = Pin<Box<dyn Stream<Item = Result<ModelStreamEvent>> + Send>>;

#[derive(Debug, Clone, Default)]
pub struct ProviderPromptRequest {
    pub prompt: String,
    pub tools: Vec<ToolDescriptor>,
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

    pub async fn generate(
        &self,
        provider_id: &str,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelTurnOutput> {
        let provider = self
            .provider(provider_id)
            .ok_or_else(|| anyhow::anyhow!("unknown provider id: {provider_id}"))?;
        provider.prompt_turn(agent, request).await
    }

    pub async fn stream(
        &self,
        provider_id: &str,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelEventStream> {
        let provider = self
            .provider(provider_id)
            .ok_or_else(|| anyhow::anyhow!("unknown provider id: {provider_id}"))?;
        provider.stream_turn(agent, request).await
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
    pub config: PluginConfigHandle,
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

#[derive(Debug, Clone, Default)]
pub struct PluginConfigHandle {
    plugin_id: Option<String>,
    raw: Option<Value>,
    config_root: std::path::PathBuf,
}

impl PluginConfigHandle {
    pub fn new(
        plugin_id: Option<String>,
        raw: Option<Value>,
        config_root: std::path::PathBuf,
    ) -> Self {
        Self {
            plugin_id,
            raw,
            config_root,
        }
    }

    pub fn plugin_id(&self) -> Option<&str> {
        self.plugin_id.as_deref()
    }

    pub fn raw(&self) -> Option<&Value> {
        self.raw.as_ref()
    }

    pub fn get<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(raw) = &self.raw else {
            return Ok(None);
        };
        let resolved = resolve_secret_refs(raw.clone(), &self.config_root)?;
        Ok(Some(serde_json::from_value(resolved)?))
    }
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

    async fn prompt_turn(
        &self,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelTurnOutput>;

    async fn stream_turn(
        &self,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelEventStream> {
        let output = self.prompt_turn(agent, request).await?;
        Ok(stream_model_turn(output))
    }

    async fn prompt_text(&self, agent: &AgentDefinition, prompt: &str) -> Result<String> {
        Ok(self
            .prompt_turn(
                agent,
                ProviderPromptRequest {
                    prompt: prompt.to_string(),
                    ..ProviderPromptRequest::default()
                },
            )
            .await?
            .assistant_text())
    }
}

pub fn stream_model_turn(output: ModelTurnOutput) -> ModelEventStream {
    let mut events = Vec::new();
    for item in output.items.iter().cloned() {
        match item {
            ModelOutputItem::AssistantText(item) => {
                events.push(Ok(ModelStreamEvent::AssistantTextDelta(item)));
            }
            ModelOutputItem::ToolCall(item) => {
                events.push(Ok(ModelStreamEvent::ToolCall(item)));
            }
            ModelOutputItem::ToolResult(item) => {
                events.push(Ok(ModelStreamEvent::ToolResult(item)));
            }
            ModelOutputItem::RuntimeControl(item) => {
                events.push(Ok(ModelStreamEvent::RuntimeControl(item)));
            }
        }
    }
    if let Some(usage) = output.usage.clone() {
        events.push(Ok(ModelStreamEvent::Usage(usage)));
    }
    events.push(Ok(ModelStreamEvent::Completed(output)));
    Box::pin(stream::iter(events))
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
pub type BillingPluginRef = Arc<dyn BillingPlugin>;

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
        plugin_id: Option<String>,
        session_id: Option<String>,
        turn_id: Option<String>,
    ) -> PluginContext {
        let config_root = runtime_config.config_root.clone();
        let raw_config = plugin_id
            .as_ref()
            .and_then(|plugin_id| runtime_config.plugins.config_fragment(plugin_id).cloned());
        PluginContext {
            runtime_config,
            config: PluginConfigHandle::new(plugin_id, raw_config, config_root),
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
                Some(manifest.id.clone()),
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
