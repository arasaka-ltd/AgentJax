use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::config::secrets::resolve_secret_refs;

#[derive(Debug, Clone, PartialEq)]
pub struct LlmProviderConfig {
    pub provider_id: String,
    pub kind: String,
    pub settings: Value,
}

impl LlmProviderConfig {
    pub fn new(
        provider_id: impl Into<String>,
        kind: impl Into<String>,
        settings: impl Serialize,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            kind: kind.into(),
            settings: serde_json::to_value(settings).expect("provider settings must serialize"),
        }
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn settings_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        Ok(serde_json::from_value(self.settings.clone())?)
    }

    pub fn settings_as_resolved<T>(&self, config_root: &Path) -> Result<T>
    where
        T: DeserializeOwned,
    {
        Ok(serde_json::from_value(resolve_secret_refs(
            self.settings.clone(),
            config_root,
        )?)?)
    }
}

impl Serialize for LlmProviderConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("provider_id", &self.provider_id)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("settings", &self.settings)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for LlmProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct GenericProviderConfig {
            provider_id: String,
            kind: String,
            #[serde(default)]
            settings: Value,
        }

        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum LegacyProviderConfig {
            OpenAi(OpenAiProviderConfig),
            Mock(MockProviderConfig),
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Generic(GenericProviderConfig),
            Legacy(LegacyProviderConfig),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Generic(config) => Ok(Self {
                provider_id: config.provider_id,
                kind: config.kind,
                settings: config.settings,
            }),
            Repr::Legacy(LegacyProviderConfig::OpenAi(config)) => {
                Ok(Self::new(config.provider_id.clone(), "openai", config))
            }
            Repr::Legacy(LegacyProviderConfig::Mock(config)) => {
                Ok(Self::new(config.provider_id.clone(), "mock", config))
            }
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
