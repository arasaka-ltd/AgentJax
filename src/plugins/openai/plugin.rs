use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::{
    HeaderMap as ReqwestHeaderMap, HeaderValue as ReqwestHeaderValue, AUTHORIZATION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    builtin::tools::ToolDefinition,
    config::{
        AgentDefinition, ModelCatalogSnapshot, ModelInfoSnapshot, OpenAiProviderConfig,
        ProviderModelCatalog,
    },
    core::{
        plugin::ProviderPromptRequest, BillingPlugin, Plugin, PluginContext, ProviderPlugin,
        ResourceProviderPlugin,
    },
    domain::{
        AssistantTextItem, BillingBreakdownItem, BillingCapability, BillingConfidence, BillingMode,
        BillingRecord, FinishReason, ModelOutputItem, ModelTurnOutput, ModelUsage, Permission,
        PluginCapability, PluginManifest, ProviderCapability, Resource, ResourceDescriptor,
        ResourceId, ResourceKind, ResourceStatus, ToolCallItem, UsageCategory, UsageRecord,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiRawModel {
    pub id: String,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiModelCatalog {
    pub provider_id: String,
    pub base_url: String,
    pub raw_models: Vec<OpenAiRawModel>,
    pub language_models: Vec<ModelInfoSnapshot>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiRawModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesApiResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    usage: Option<OpenAiResponseUsage>,
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Vec<OpenAiResponsesOutputItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OpenAiResponsesOutputItem {
    #[serde(rename = "message")]
    Message {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        content: Vec<OpenAiResponsesContentItem>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesContentItem {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenAiProviderAdapter {
    config: OpenAiProviderConfig,
}

impl OpenAiProviderAdapter {
    pub fn new(config: OpenAiProviderConfig) -> Self {
        Self { config }
    }

    pub fn provider_id(&self) -> &str {
        &self.config.provider_id
    }

    pub fn effective_base_url(&self) -> String {
        self.config.effective_base_url()
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

    pub async fn list_models(&self) -> Result<OpenAiModelCatalog> {
        let client = reqwest::Client::builder().build()?;
        let mut headers = ReqwestHeaderMap::new();
        let api_key = self.config.resolve_api_key()?;
        headers.insert(
            AUTHORIZATION,
            ReqwestHeaderValue::from_str(&format!("Bearer {api_key}"))?,
        );
        if let Some(organization) = &self.config.organization {
            headers.insert(
                "OpenAI-Organization",
                ReqwestHeaderValue::from_str(organization)?,
            );
        }
        if let Some(project) = &self.config.project {
            headers.insert("OpenAI-Project", ReqwestHeaderValue::from_str(project)?);
        }

        let response = client
            .get(self.config.endpoint_url("models"))
            .headers(headers)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "openai models request failed: status {}",
                response.status()
            ));
        }

        let raw: OpenAiModelsResponse = response.json().await?;
        let language_models = self.language_models_from_raw(&raw.data);
        Ok(OpenAiModelCatalog {
            provider_id: self.config.provider_id.clone(),
            base_url: self.effective_base_url(),
            raw_models: raw.data,
            language_models,
        })
    }

    pub fn language_models_from_raw(
        &self,
        raw_models: &[OpenAiRawModel],
    ) -> Vec<ModelInfoSnapshot> {
        raw_models
            .iter()
            .filter(|model| is_language_model(&model.id))
            .map(|model| normalize_model_info(&model.id))
            .collect()
    }

    pub fn to_snapshot(&self, catalog: &OpenAiModelCatalog) -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            generated_at: Some(chrono::Utc::now()),
            providers: vec![ProviderModelCatalog {
                provider_id: catalog.provider_id.clone(),
                provider_kind: "openai".into(),
                base_url: Some(catalog.base_url.clone()),
                language_models: catalog.language_models.clone(),
            }],
        }
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

    pub async fn prompt_turn(
        &self,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelTurnOutput> {
        let client = reqwest::Client::builder().build()?;
        let response = client
            .post(self.config.endpoint_url("responses"))
            .headers(self.request_headers()?)
            .json(&self.responses_request_body(agent, &request)?)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("openai prompt failed: status {status} body {body}"));
        }

        let payload: Value = response.json().await?;
        let normalized: OpenAiResponsesApiResponse = serde_json::from_value(payload)?;
        normalize_model_turn_output(normalized)
    }

    pub async fn prompt_text(&self, agent: &AgentDefinition, prompt: &str) -> Result<String> {
        Ok(self
            .prompt_turn(
                agent,
                ProviderPromptRequest {
                    prompt: prompt.to_string(),
                    ..ProviderPromptRequest::default()
                },
            )
            .await?
            .assistant_text())
    }

    fn request_headers(&self) -> Result<ReqwestHeaderMap> {
        let mut headers = ReqwestHeaderMap::new();
        let api_key = self.config.resolve_api_key()?;
        headers.insert(
            AUTHORIZATION,
            ReqwestHeaderValue::from_str(&format!("Bearer {api_key}"))?,
        );
        if let Some(organization) = &self.config.organization {
            headers.insert(
                "OpenAI-Organization",
                ReqwestHeaderValue::from_str(organization)?,
            );
        }
        if let Some(project) = &self.config.project {
            headers.insert("OpenAI-Project", ReqwestHeaderValue::from_str(project)?);
        }
        Ok(headers)
    }

    fn responses_request_body(
        &self,
        agent: &AgentDefinition,
        request: &ProviderPromptRequest,
    ) -> Result<Value> {
        let mut body = json!({
            "model": agent.model,
            "input": [{
                "role": "user",
                "content": request.prompt,
            }],
        });
        if let Some(preamble) = &agent.preamble {
            body["instructions"] = Value::String(preamble.clone());
        }
        if let Some(temperature) = agent.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = agent.max_tokens {
            body["max_output_tokens"] = json!(max_tokens);
        }
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| -> Result<Value> {
                        let ToolDefinition {
                            name,
                            description,
                            parameters,
                        } = tool.definition()?;
                        Ok(json!({
                            "type": "function",
                            "name": name,
                            "description": description,
                            "parameters": parameters,
                            "strict": false,
                        }))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(false);
        }
        Ok(body)
    }
}

fn normalize_model_turn_output(payload: OpenAiResponsesApiResponse) -> Result<ModelTurnOutput> {
    let mut items = Vec::new();

    for (index, item) in payload.output.into_iter().enumerate() {
        match item {
            OpenAiResponsesOutputItem::Message { id, content } => {
                let text = content
                    .into_iter()
                    .filter(|item| item.content_type == "output_text")
                    .filter_map(|item| item.text)
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    continue;
                }
                items.extend(parse_text_response_items(
                    &collapse_repeated_text(text),
                    id.unwrap_or_else(|| format!("msg_{index}")),
                )?);
            }
            OpenAiResponsesOutputItem::FunctionCall {
                id,
                call_id,
                name,
                arguments,
            } => {
                let args = serde_json::from_str(&arguments).unwrap_or_else(|_| {
                    json!({
                        "_raw_arguments": arguments,
                    })
                });
                let item_id = id.unwrap_or_else(|| format!("fc_{index}"));
                let tool_call_id = call_id.unwrap_or_else(|| item_id.clone());
                items.push(ModelOutputItem::ToolCall(ToolCallItem {
                    item_id,
                    tool_call_id,
                    tool_name: name,
                    args,
                    timeout_secs: None,
                }));
            }
            OpenAiResponsesOutputItem::Unknown => {}
        }
    }

    if items.is_empty() {
        if let Some(text) = payload.output_text.filter(|text| !text.trim().is_empty()) {
            items.extend(parse_text_response_items(
                &collapse_repeated_text(text),
                "out_text".into(),
            )?);
        }
    }

    if items.is_empty() {
        return Err(anyhow!("openai prompt failed: empty response output"));
    }

    let finish_reason = if items
        .iter()
        .any(|item| matches!(item, ModelOutputItem::ToolCall(_)))
    {
        FinishReason::ToolCalls
    } else {
        FinishReason::Completed
    };

    Ok(ModelTurnOutput {
        output_id: payload.id.unwrap_or_else(|| "output.openai".into()),
        items,
        finish_reason,
        usage: payload.usage.map(|usage| ModelUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }),
    })
}

fn parse_text_response_items(text: &str, item_prefix: String) -> Result<Vec<ModelOutputItem>> {
    let mut items = Vec::new();
    let assistant_text = text.trim().to_string();
    if !assistant_text.is_empty() {
        items.push(ModelOutputItem::AssistantText(AssistantTextItem {
            item_id: format!("{item_prefix}_text"),
            text: assistant_text,
            is_partial: false,
        }));
    }

    Ok(items)
}

fn collapse_repeated_text(text: String) -> String {
    let trimmed = text.trim().to_string();
    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    if len < 2 {
        return trimmed;
    }

    for unit_len in 1..=(len / 2) {
        if !len.is_multiple_of(unit_len) {
            continue;
        }
        let repeats = len / unit_len;
        if repeats < 2 {
            continue;
        }
        let unit = chars[..unit_len].iter().collect::<String>();
        if unit.repeat(repeats) == trimmed {
            return unit;
        }
    }

    trimmed
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
impl ProviderPlugin for OpenAiProviderPlugin {
    fn provider_id(&self) -> &str {
        &self.config.provider_id
    }

    async fn prompt_turn(
        &self,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelTurnOutput> {
        OpenAiProviderAdapter::new(self.config.clone())
            .prompt_turn(agent, request)
            .await
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

fn is_language_model(model_id: &str) -> bool {
    let model_id = model_id.to_ascii_lowercase();
    !model_id.contains("embed")
        && !model_id.contains("tts")
        && !model_id.contains("transcribe")
        && !model_id.contains("whisper")
        && !model_id.contains("moderation")
        && !model_id.contains("image")
}

fn normalize_model_info(model_id: &str) -> ModelInfoSnapshot {
    let lower = model_id.to_ascii_lowercase();
    let (context_length, output_limit) = if lower.contains("gpt-4.1") {
        (Some(1_047_576), Some(32_768))
    } else if lower.contains("gpt-4o") {
        (Some(128_000), Some(16_384))
    } else if lower.contains("o3") || lower.contains("o4") {
        (Some(200_000), Some(100_000))
    } else {
        (None, None)
    };

    let mut capability_tags = vec!["llm".into(), "text".into()];
    if lower.contains("mini") {
        capability_tags.push("small".into());
    }
    if lower.contains("vision") || lower.contains("4o") {
        capability_tags.push("vision".into());
    }
    if lower.starts_with('o') {
        capability_tags.push("reasoning".into());
    }

    ModelInfoSnapshot {
        model_id: model_id.into(),
        display_label: model_id.replace('-', " ").to_uppercase(),
        context_length,
        input_token_limit: context_length,
        output_token_limit: output_limit,
        capability_tags,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    use super::{
        collapse_repeated_text, normalize_model_info, normalize_model_turn_output,
        OpenAiProviderAdapter, OpenAiProviderConfig, OpenAiRawModel, OpenAiResponsesApiResponse,
        OpenAiResponsesContentItem, OpenAiResponsesOutputItem,
    };
    use crate::{
        builtin::tools::ToolDescriptor,
        config::AgentDefinition,
        core::plugin::ProviderPromptRequest,
        domain::{AssistantTextItem, ModelOutputItem},
    };

    #[tokio::test]
    async fn lists_models_and_normalizes_language_models_from_base_url() {
        let server = spawn_server(vec![(
            "GET /v1/models HTTP/1.1",
            r#"{"data":[{"id":"gpt-4o-mini"},{"id":"text-embedding-3-small"}]}"#,
        )])
        .await;
        let config = OpenAiProviderConfig {
            provider_id: "openai-default".into(),
            api_key: Some("test-key".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: Some(format!("http://{}", server.0)),
            organization: None,
            project: None,
        };
        let adapter = OpenAiProviderAdapter::new(config);

        let catalog = adapter.list_models().await.unwrap();

        assert_eq!(catalog.raw_models.len(), 2);
        assert_eq!(catalog.language_models.len(), 1);
        assert_eq!(catalog.language_models[0].model_id, "gpt-4o-mini");

        server.1.abort();
    }

    #[tokio::test]
    async fn prompt_text_uses_configured_base_url() {
        let server = spawn_server(vec![(
            "POST /v1/responses HTTP/1.1",
            r#"{"id":"resp_1","object":"response","created_at":0,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"gpt-4o-mini","usage":null,"output":[{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"mocked reply","annotations":[]}]}],"tools":[]}"#,
        )])
        .await;
        let config = OpenAiProviderConfig {
            provider_id: "openai-default".into(),
            api_key: Some("test-key".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: Some(format!("http://{}", server.0)),
            organization: None,
            project: None,
        };
        let adapter = OpenAiProviderAdapter::new(config);
        let response = adapter
            .prompt_text(&AgentDefinition::default(), "hello")
            .await
            .unwrap();

        assert_eq!(response, "mocked reply");
        server.1.abort();
    }

    #[test]
    fn filters_language_models() {
        let adapter = OpenAiProviderAdapter::new(OpenAiProviderConfig::default());
        let models = adapter.language_models_from_raw(&[
            OpenAiRawModel {
                id: "gpt-4o-mini".into(),
                created: None,
                owned_by: None,
            },
            OpenAiRawModel {
                id: "text-embedding-3-small".into(),
                created: None,
                owned_by: None,
            },
        ]);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "gpt-4o-mini");
        let normalized = normalize_model_info("o4-mini");
        assert!(normalized.capability_tags.contains(&"reasoning".into()));
    }

    #[test]
    fn collapses_exact_repeated_output_text() {
        assert_eq!(
            collapse_repeated_text("provider-okprovider-okprovider-ok".into()),
            "provider-ok"
        );
        assert_eq!(
            collapse_repeated_text("provider-okprovider-okprovider-ok".into()),
            "provider-ok"
        );
        assert_eq!(collapse_repeated_text("unique text".into()), "unique text");
    }

    #[test]
    fn collapses_exact_repeated_output_text_with_multibyte_characters() {
        assert_eq!(collapse_repeated_text("你你你".into()), "你");
        assert_eq!(
            collapse_repeated_text("你好世界你好世界".into()),
            "你好世界"
        );
    }

    #[test]
    fn normalizes_text_fallback_into_assistant_text() {
        let output = normalize_model_turn_output(OpenAiResponsesApiResponse {
            id: Some("resp_1".into()),
            usage: None,
            output_text: None,
            output: vec![OpenAiResponsesOutputItem::Message {
                id: Some("msg_1".into()),
                content: vec![OpenAiResponsesContentItem {
                    content_type: "output_text".into(),
                    text: Some(
                        "compatibility text response".into(),
                    ),
                }],
            }],
        })
        .unwrap();

        assert_eq!(output.items.len(), 1);
        assert!(matches!(
            output.items[0],
            ModelOutputItem::AssistantText(AssistantTextItem { .. })
        ));
    }

    #[test]
    fn responses_request_body_includes_native_function_tools() {
        let adapter = OpenAiProviderAdapter::new(OpenAiProviderConfig::default());
        let body = adapter.responses_request_body(
            &AgentDefinition::default(),
            &ProviderPromptRequest {
                prompt: "hello".into(),
                tools: vec![ToolDescriptor {
                    name: "read".into(),
                    description: "Read a file".into(),
                    when_to_use: String::new(),
                    when_not_to_use: String::new(),
                    arguments_schema: json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }),
                    default_timeout_secs: 5,
                    idempotent: true,
                }],
            },
        )
        .unwrap();

        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "hello");
    }

    async fn spawn_server(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            for (expected_request_line, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0_u8; 8192];
                let bytes = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..bytes]);
                assert!(request.contains(expected_request_line), "{request}");
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (addr, handle)
    }
}
