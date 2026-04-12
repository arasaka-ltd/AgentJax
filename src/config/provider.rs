use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LlmProviderConfig {
    OpenAi(OpenAiProviderConfig),
    Mock(MockProviderConfig),
}

impl LlmProviderConfig {
    pub fn provider_id(&self) -> &str {
        match self {
            Self::OpenAi(config) => &config.provider_id,
            Self::Mock(config) => &config.provider_id,
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiProviderConfig {
    pub provider_id: String,
    pub api_key: Option<String>,
    pub api_key_env: String,
    pub base_url: Option<String>,
    pub organization: Option<String>,
    pub project: Option<String>,
}

impl Default for OpenAiProviderConfig {
    fn default() -> Self {
        Self {
            provider_id: "openai-default".into(),
            api_key: None,
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: None,
            organization: None,
            project: None,
        }
    }
}

impl OpenAiProviderConfig {
    pub fn resolve_api_key(&self) -> Result<String> {
        if let Some(api_key) = &self.api_key {
            if !api_key.is_empty() {
                return Ok(api_key.clone());
            }
        }

        std::env::var(&self.api_key_env).map_err(|_| {
            anyhow!(
                "missing OpenAI API key: set {} or provide config.providers.openai.api_key",
                self.api_key_env
            )
        })
    }

    pub fn effective_base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into())
    }

    pub fn endpoint_url(&self, path: &str) -> String {
        let base = self.effective_base_url();
        let base = base.trim_end_matches('/');
        let path = path.trim_start_matches('/');

        if base.ends_with("/v1") {
            format!("{base}/{path}")
        } else {
            format!("{base}/v1/{path}")
        }
    }
}
