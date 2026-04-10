use serde::{Deserialize, Serialize};

use super::{LlmProviderConfig, OpenAiProviderConfig, RuntimePaths, WorkspaceConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeConfig {
    pub app_name: String,
    pub runtime_paths: RuntimePaths,
    pub workspace: WorkspaceConfig,
    pub api_version: String,
    pub daemon_version: String,
    pub agent_runtime: AgentRuntimeConfig,
    pub config_schema_version: String,
    pub state_schema_version: String,
    pub event_schema_version: String,
    pub plugin_api_version: String,
    pub skill_spec_version: String,
}

impl RuntimeConfig {
    pub fn new(
        app_name: impl Into<String>,
        runtime_paths: RuntimePaths,
        workspace: WorkspaceConfig,
    ) -> Self {
        Self {
            app_name: app_name.into(),
            runtime_paths,
            workspace,
            api_version: "v1".into(),
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            agent_runtime: AgentRuntimeConfig::default(),
            config_schema_version: "config.v1".into(),
            state_schema_version: "state.v1".into(),
            event_schema_version: "event.v1".into(),
            plugin_api_version: "plugin-api.v1".into(),
            skill_spec_version: "skill-spec.v1".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentRuntimeConfig {
    pub default_agent: AgentDefinition,
    pub llm: LlmRuntimeConfig,
}

impl Default for AgentRuntimeConfig {
    fn default() -> Self {
        Self {
            default_agent: AgentDefinition::default(),
            llm: LlmRuntimeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinition {
    pub agent_id: String,
    pub provider_id: String,
    pub model: String,
    pub preamble: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
}

impl Default for AgentDefinition {
    fn default() -> Self {
        Self {
            agent_id: "default-agent".into(),
            provider_id: "openai-default".into(),
            model: "gpt-4o-mini".into(),
            preamble: Some(
                "You are the default AgentJax runtime agent. Respond concisely and accurately."
                    .into(),
            ),
            temperature: Some(0.2),
            max_tokens: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmRuntimeConfig {
    pub default_provider_id: String,
    pub providers: Vec<LlmProviderConfig>,
    pub model_catalog: ModelCatalogSnapshot,
}

impl Default for LlmRuntimeConfig {
    fn default() -> Self {
        Self {
            default_provider_id: "openai-default".into(),
            providers: vec![LlmProviderConfig::OpenAi(OpenAiProviderConfig::default())],
            model_catalog: ModelCatalogSnapshot::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ModelCatalogSnapshot {
    pub generated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub providers: Vec<ProviderModelCatalog>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModelCatalog {
    pub provider_id: String,
    pub provider_kind: String,
    pub base_url: Option<String>,
    pub language_models: Vec<ModelInfoSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfoSnapshot {
    pub model_id: String,
    pub display_label: String,
    pub context_length: Option<u32>,
    pub input_token_limit: Option<u32>,
    pub output_token_limit: Option<u32>,
    pub capability_tags: Vec<String>,
}
