use crate::config::LlmProviderConfig;
use crate::core::plugin::ProviderPromptRequest;
use crate::core::{Plugin, PluginContext, PluginManagerCandidate, PluginRef, ProviderPlugin};
use crate::domain::{ModelTurnOutput, PluginCapability, PluginManifest};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MockProviderConfig {
    pub provider_id: String,
}

impl Default for MockProviderConfig {
    fn default() -> Self {
        Self {
            provider_id: "mock-default".into(),
        }
    }
}

pub struct MockProviderPlugin {
    config: MockProviderConfig,
    pub calls: Arc<Mutex<Vec<String>>>,
    pub next_response: Arc<Mutex<Option<ModelTurnOutput>>>,
}

impl MockProviderPlugin {
    pub fn new(config: MockProviderConfig) -> Self {
        Self {
            config,
            calls: Arc::new(Mutex::new(Vec::new())),
            next_response: Arc::new(Mutex::new(None)),
        }
    }
}

pub fn provider_candidate(provider: &LlmProviderConfig) -> Result<PluginManagerCandidate> {
    let config: MockProviderConfig = provider.settings_as()?;
    let plugin = Arc::new(MockProviderPlugin::new(config));
    Ok(PluginManagerCandidate::provider(
        plugin.clone() as PluginRef,
        plugin.clone(),
        None,
        vec![],
        true,
    ))
}

#[async_trait]
impl Plugin for MockProviderPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: self.config.provider_id.clone(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Provider(
                crate::domain::ProviderCapability::LlmText,
            )],
            dependencies: vec![],
            optional_dependencies: vec![],
            required_permissions: vec![],
            hooks: vec![],
            config_schema: None,
            provided_resources: vec![],
        }
    }

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

#[async_trait]
impl ProviderPlugin for MockProviderPlugin {
    fn provider_id(&self) -> &str {
        &self.config.provider_id
    }

    async fn prompt_turn(
        &self,
        _agent: &crate::config::AgentDefinition,
        _request: ProviderPromptRequest,
    ) -> Result<ModelTurnOutput> {
        self.calls.lock().unwrap().push("prompt_turn".into());
        let res = self.next_response.lock().unwrap().take();
        if let Some(res) = res {
            Ok(res)
        } else {
            Ok(ModelTurnOutput {
                output_id: "mock_output".into(),
                items: vec![],
                finish_reason: crate::domain::FinishReason::Completed,
                usage: Some(crate::domain::ModelUsage {
                    input_tokens: Some(64),
                    output_tokens: Some(16),
                }),
            })
        }
    }
}
