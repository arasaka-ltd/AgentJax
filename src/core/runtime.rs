use anyhow::{anyhow, Result};

use crate::config::{AgentDefinition, RuntimeConfig};
use crate::core::{plugin::ProviderPromptRequest, PluginHost, WorkspaceRuntimeHost};
use crate::domain::ModelTurnOutput;

#[derive(Clone)]
pub struct ApplicationRuntime {
    config: RuntimeConfig,
    plugin_host: PluginHost,
    workspace_host: WorkspaceRuntimeHost,
}

#[derive(Debug, Clone)]
pub struct AgentPromptRequest {
    pub prompt: String,
    pub agent_id: Option<String>,
    pub agent_override: Option<AgentDefinition>,
    pub tools: Vec<crate::builtin::tools::ToolDescriptor>,
}

impl ApplicationRuntime {
    pub fn new(
        config: RuntimeConfig,
        plugin_host: PluginHost,
        workspace_host: WorkspaceRuntimeHost,
    ) -> Self {
        Self {
            config,
            plugin_host,
            workspace_host,
        }
    }

    pub fn default_agent(&self) -> &AgentDefinition {
        &self.config.agent_runtime.default_agent
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub fn plugin_host(&self) -> &PluginHost {
        &self.plugin_host
    }

    pub fn workspace_host(&self) -> &WorkspaceRuntimeHost {
        &self.workspace_host
    }

    pub async fn prompt_text(&self, request: AgentPromptRequest) -> Result<String> {
        Ok(self.prompt_turn(request).await?.assistant_text())
    }

    pub async fn prompt_turn(&self, request: AgentPromptRequest) -> Result<ModelTurnOutput> {
        if let Some(agent) = request.agent_override.as_ref() {
            self.validate_agent_binding(agent)?;
        }
        let agent = match request.agent_override.as_ref() {
            Some(agent) => agent,
            None => self.resolve_agent(request.agent_id.as_deref())?,
        };
        self.resolve_provider(&agent.provider_id)?
            .prompt_turn(
                agent,
                ProviderPromptRequest {
                    prompt: request.prompt,
                    tools: request.tools,
                },
            )
            .await
    }

    pub fn session_agent(
        &self,
        provider_id: Option<&str>,
        model_id: Option<&str>,
    ) -> Result<AgentDefinition> {
        let mut agent = self.default_agent().clone();
        if let Some(provider_id) = provider_id {
            agent.provider_id = provider_id.to_string();
        }
        if let Some(model_id) = model_id {
            agent.model = model_id.to_string();
        }
        self.resolve_provider(&agent.provider_id)?;
        Ok(agent)
    }

    pub fn validate_provider_model_binding(&self, provider_id: &str, model_id: &str) -> Result<()> {
        self.resolve_provider(provider_id)?;
        let provider_snapshot = self
            .config
            .agent_runtime
            .llm
            .model_catalog
            .providers
            .iter()
            .find(|provider| provider.provider_id == provider_id)
            .ok_or_else(|| anyhow!("provider snapshot not found: {provider_id}"))?;

        if provider_snapshot.language_models.is_empty() {
            return Err(anyhow!(
                "provider snapshot has no language models: {provider_id}"
            ));
        }

        if provider_snapshot
            .language_models
            .iter()
            .any(|model| model.model_id == model_id)
        {
            Ok(())
        } else {
            Err(anyhow!(
                "model {model_id} is not available for provider {provider_id}"
            ))
        }
    }

    fn resolve_agent(&self, agent_id: Option<&str>) -> Result<&AgentDefinition> {
        let default_agent = self.default_agent();

        match agent_id {
            None => Ok(default_agent),
            Some(agent_id) if agent_id == default_agent.agent_id => Ok(default_agent),
            Some(agent_id) => Err(anyhow!("unknown agent id: {agent_id}")),
        }
    }

    fn resolve_provider(
        &self,
        provider_id: &str,
    ) -> Result<crate::core::plugin::ProviderPluginRef> {
        self.plugin_host
            .registry()
            .provider(provider_id)
            .ok_or_else(|| anyhow!("unknown provider id: {provider_id}"))
    }

    fn validate_agent_binding(&self, agent: &AgentDefinition) -> Result<()> {
        self.resolve_provider(&agent.provider_id)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct RuntimeHost {
    runtime: ApplicationRuntime,
}

impl RuntimeHost {
    pub fn new(runtime: ApplicationRuntime) -> Self {
        Self { runtime }
    }

    pub fn runtime(&self) -> &ApplicationRuntime {
        &self.runtime
    }
}
