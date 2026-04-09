use anyhow::{anyhow, Result};
use async_trait::async_trait;
use rig::{
    completion::Prompt,
    http_client::{HeaderMap, HeaderValue},
    prelude::CompletionClient,
    providers::openai,
};

use crate::{
    config::{AgentDefinition, OpenAiProviderConfig},
    core::{BillingPlugin, Plugin, PluginContext, ResourceProviderPlugin},
    domain::{
        BillingBreakdownItem, BillingCapability, BillingConfidence, BillingMode, BillingRecord,
        Permission, PluginCapability, PluginManifest, ProviderCapability, Resource,
        ResourceDescriptor, ResourceId, ResourceKind, ResourceStatus, UsageCategory, UsageRecord,
    },
};

#[derive(Debug, Clone)]
pub struct OpenAiProviderAdapter {
    config: OpenAiProviderConfig,
}

impl OpenAiProviderAdapter {
    pub fn new(config: OpenAiProviderConfig) -> Self {
        Self { config }
    }

    pub fn resources(&self) -> Vec<Resource> {
        vec![
            self.make_resource(
                "model:text",
                ResourceKind::ModelText,
                vec!["llm.text", "rig.agent", "provider.openai"],
            ),
            self.make_resource(
                "model:reasoning",
                ResourceKind::ModelReasoning,
                vec!["llm.reasoning", "provider.openai"],
            ),
            self.make_resource(
                "model:embedding",
                ResourceKind::ModelEmbedding,
                vec!["embedding", "provider.openai"],
            ),
            self.make_resource(
                "audio:tts",
                ResourceKind::AudioTts,
                vec!["audio.tts", "provider.openai"],
            ),
            self.make_resource(
                "audio:st",
                ResourceKind::AudioSt,
                vec!["audio.st", "provider.openai"],
            ),
            self.make_resource(
                "media:image",
                ResourceKind::MediaImageGeneration,
                vec!["image.generation", "provider.openai"],
            ),
        ]
    }

    fn make_resource(
        &self,
        suffix: &str,
        resource_kind: ResourceKind,
        capabilities: Vec<&str>,
    ) -> Resource {
        Resource {
            resource_id: ResourceId(format!("provider:{}:{suffix}", self.config.provider_id)),
            resource_kind,
            binding_target: format!("rig.openai://{}/{}", self.config.provider_id, suffix),
            capabilities: capabilities.into_iter().map(str::to_string).collect(),
            labels: std::collections::BTreeMap::from([
                ("provider".into(), "openai".into()),
                ("provider_id".into(), self.config.provider_id.clone()),
                ("resource_suffix".into(), suffix.into()),
            ]),
            status: ResourceStatus::Active,
        }
    }

    pub async fn prompt_text(&self, agent: &AgentDefinition, prompt: &str) -> Result<String> {
        let client = self.build_client()?;
        let mut builder = client.agent(&agent.model);

        if let Some(preamble) = &agent.preamble {
            builder = builder.preamble(preamble);
        }

        if let Some(temperature) = agent.temperature {
            builder = builder.temperature(temperature);
        }

        if let Some(max_tokens) = agent.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }

        let rig_agent = builder.build();
        rig_agent
            .prompt(prompt)
            .await
            .map_err(|error| anyhow!("openai prompt failed: {error}"))
    }

    fn build_client(&self) -> Result<openai::Client> {
        let api_key = self.config.resolve_api_key()?;
        let mut builder = openai::Client::builder().api_key(api_key);

        if let Some(base_url) = &self.config.base_url {
            builder = builder.base_url(base_url);
        }

        if let Some(organization) = &self.config.organization {
            let mut headers = HeaderMap::new();
            headers.insert(
                "OpenAI-Organization",
                HeaderValue::from_str(organization)
                    .map_err(|error| anyhow!("invalid OpenAI organization header: {error}"))?,
            );
            builder = builder.http_headers(headers);
        }

        builder
            .build()
            .map_err(|error| anyhow!("failed to build Rig OpenAI client: {error}"))
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiProviderPlugin {
    config: OpenAiProviderConfig,
}

impl OpenAiProviderPlugin {
    pub fn new(config: OpenAiProviderConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Plugin for OpenAiProviderPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: format!("provider.openai.{}", self.config.provider_id),
            version: "0.1.0".into(),
            capabilities: vec![
                PluginCapability::Provider(ProviderCapability::LlmText),
                PluginCapability::Provider(ProviderCapability::LlmReasoning),
                PluginCapability::Provider(ProviderCapability::Embedding),
                PluginCapability::Provider(ProviderCapability::Tts),
                PluginCapability::Provider(ProviderCapability::St),
                PluginCapability::Provider(ProviderCapability::ImageGeneration),
                PluginCapability::Billing(BillingCapability::UsageMeter),
                PluginCapability::Billing(BillingCapability::CostEstimator),
            ],
            config_schema: None,
            required_permissions: vec![Permission::EmitEvents],
            dependencies: vec!["rig-core".into()],
            optional_dependencies: Vec::new(),
            provided_resources: OpenAiProviderAdapter::new(self.config.clone())
                .resources()
                .into_iter()
                .map(|resource| ResourceDescriptor {
                    resource_id: resource.resource_id,
                    kind: format!("{:?}", resource.resource_kind),
                    description: Some("Rig-backed OpenAI provider resource".into()),
                })
                .collect(),
            hooks: Vec::new(),
        }
    }

    async fn on_load(&self, _ctx: PluginContext) -> Result<()> {
        Ok(())
    }
}

impl ResourceProviderPlugin for OpenAiProviderPlugin {
    fn provided_resources(&self) -> Vec<Resource> {
        OpenAiProviderAdapter::new(self.config.clone()).resources()
    }
}

#[async_trait]
impl BillingPlugin for OpenAiProviderPlugin {
    async fn estimate_billing(&self, usage: &UsageRecord) -> Result<Option<BillingRecord>> {
        if usage.provider_id.as_deref() != Some(self.config.provider_id.as_str()) {
            return Ok(None);
        }

        let mut amount = 0.0_f64;
        let mut breakdown = Vec::new();

        match usage.category {
            UsageCategory::ModelText | UsageCategory::ModelReasoning => {
                if let Some(input_tokens) = usage.input_tokens {
                    let subtotal = input_tokens as f64 / 1_000_000.0 * 0.15;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "input_tokens".into(),
                        quantity: input_tokens.to_string(),
                        unit_price: Some("0.15 / 1M tokens".into()),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("placeholder local estimate".into()),
                    });
                }

                if let Some(output_tokens) = usage.output_tokens {
                    let subtotal = output_tokens as f64 / 1_000_000.0 * 0.60;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "output_tokens".into(),
                        quantity: output_tokens.to_string(),
                        unit_price: Some("0.60 / 1M tokens".into()),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("placeholder local estimate".into()),
                    });
                }
            }
            UsageCategory::Embedding => {
                if let Some(input_tokens) = usage.input_tokens {
                    let subtotal = input_tokens as f64 / 1_000_000.0 * 0.02;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "embedding_tokens".into(),
                        quantity: input_tokens.to_string(),
                        unit_price: Some("0.02 / 1M tokens".into()),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("placeholder local estimate".into()),
                    });
                }
            }
            UsageCategory::AudioTts | UsageCategory::AudioSt | UsageCategory::ImageGeneration => {
                breakdown.push(BillingBreakdownItem {
                    item_type: "unsupported_local_estimate".into(),
                    quantity: "1".into(),
                    unit_price: None,
                    subtotal: "0".into(),
                    note: Some("pricing rule not implemented yet".into()),
                });
            }
            _ => return Ok(None),
        }

        Ok(Some(BillingRecord {
            billing_id: format!("billing-estimate-{}", usage.usage_id),
            usage_id: usage.usage_id.clone(),
            amount: format!("{amount:.8}"),
            currency: "USD".into(),
            mode: BillingMode::Estimated,
            rule_id: Some("openai.local.placeholder.v1".into()),
            confidence: BillingConfidence::Low,
            breakdown,
            generated_at: chrono::Utc::now(),
        }))
    }
}
