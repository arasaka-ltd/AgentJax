use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::config::{AgentDefinition, RuntimeConfig};
use crate::core::{EventBus, HookBus, ResourceRegistry, WorkspaceRuntimeHost};
use crate::domain::{BillingRecord, PluginManifest, Resource, UsageRecord};

#[derive(Clone)]
pub struct PluginContext {
    pub runtime_config: RuntimeConfig,
    pub workspace_runtime: WorkspaceRuntimeHost,
    pub resources: ResourceRegistry,
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

    async fn prompt_text(&self, agent: &AgentDefinition, prompt: &str) -> Result<String>;
}

#[async_trait]
pub trait BillingPlugin: Plugin {
    async fn estimate_billing(&self, _usage: &UsageRecord) -> Result<Option<BillingRecord>> {
        Ok(None)
    }
}

pub type PluginRef = Arc<dyn Plugin>;
pub type ProviderPluginRef = Arc<dyn ProviderPlugin>;

#[derive(Clone)]
pub struct PluginHost {
    registry: crate::core::PluginRegistry,
    resources: ResourceRegistry,
    events: EventBus,
    hooks: HookBus,
}

impl PluginHost {
    pub fn new(
        registry: crate::core::PluginRegistry,
        resources: ResourceRegistry,
        events: EventBus,
        hooks: HookBus,
    ) -> Self {
        Self {
            registry,
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
    ) -> PluginContext {
        PluginContext {
            runtime_config,
            workspace_runtime,
            resources: self.resources.clone(),
            events: self.events.clone(),
            hooks: self.hooks.clone(),
        }
    }

    pub fn manifests(&self) -> Vec<PluginManifest> {
        self.registry.manifests()
    }
}
