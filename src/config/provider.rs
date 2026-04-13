use std::path::Path;

use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::config::secrets::resolve_secret_refs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmProviderConfig {
    pub provider_id: String,
    pub kind: String,
    #[serde(flatten)]
    pub settings: Value,
}

impl LlmProviderConfig {
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn settings_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.settings.clone())
    }

    pub fn settings_as_resolved<T: DeserializeOwned>(&self, config_root: &Path) -> Result<T> {
        let resolved = resolve_secret_refs(self.settings.clone(), config_root)?;
        Ok(serde_json::from_value(resolved)?)
    }
}
