use anyhow::{anyhow, Result};

use crate::config::{AgentDefinition, LlmProviderConfig, RuntimeConfig};
use crate::plugins::providers::openai::OpenAiProviderAdapter;

#[derive(Debug, Clone)]
pub struct ApplicationRuntime {
    config: RuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct AgentPromptRequest {
    pub prompt: String,
    pub agent_id: Option<String>,
}

impl ApplicationRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }

    pub fn default_agent(&self) -> &AgentDefinition {
        &self.config.agent_runtime.default_agent
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub async fn prompt_text(&self, request: AgentPromptRequest) -> Result<String> {
        let agent = self.resolve_agent(request.agent_id.as_deref())?;

        match self.resolve_provider(&agent.provider_id)? {
            LlmProviderConfig::OpenAi(provider) => {
                OpenAiProviderAdapter::new(provider.clone())
                    .prompt_text(agent, &request.prompt)
                    .await
            }
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

    fn resolve_provider(&self, provider_id: &str) -> Result<&LlmProviderConfig> {
        self.config
            .agent_runtime
            .llm
            .providers
            .iter()
            .find(|provider| provider.provider_id() == provider_id)
            .ok_or_else(|| anyhow!("unknown provider id: {provider_id}"))
    }
}
