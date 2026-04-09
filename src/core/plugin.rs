use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::config::RuntimeConfig;
use crate::core::{EventBus, HookBus, ResourceRegistry};
use crate::domain::PluginManifest;

#[derive(Clone)]
pub struct PluginContext {
    pub runtime_config: RuntimeConfig,
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

pub type PluginRef = Arc<dyn Plugin>;

#[derive(Default, Clone)]
pub struct PluginIndex {
    pub manifests: BTreeMap<String, PluginManifest>,
}
