use anyhow::{Result, anyhow};
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::header::{
    AUTHORIZATION, HeaderMap as ReqwestHeaderMap, HeaderValue as ReqwestHeaderValue,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    builtin::tools::ToolDefinition,
    config::{
        AgentDefinition, LlmProviderConfig, ModelCatalogSnapshot, ModelInfoSnapshot,
        ProviderModelCatalog,
    },
    core::{
        BillingPlugin, Plugin, PluginContext, PluginManagerCandidate, PluginRef, ProviderPlugin,
        ResourceProviderPlugin,
        plugin::{
            ModelEventStream, ProviderPromptMessage, ProviderPromptRequest, stream_model_turn,
        },
    },
    domain::{
        AssistantTextItem, BillingBreakdownItem, BillingCapability, BillingConfidence, BillingMode,
        BillingRecord, FinishReason, ModelOutputItem, ModelTurnOutput, ModelUsage, Permission,
        PluginCapability, PluginManifest, ProviderCapability, Resource, ResourceDescriptor,
        ResourceId, ResourceKind, ResourceStatus, ToolCallItem, UsageCategory, UsageRecord,
    },
};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiProviderConfig {
    pub provider_id: String,
    pub api_key: Option<String>,
    pub api_key_env: String,
    pub base_url: Option<String>,
    pub organization: Option<String>,
    pub project: Option<String>,
    #[serde(default = "default_true")]
    pub use_structured_input: bool,
    #[serde(default = "default_true")]
    pub tool_strict: bool,
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
            use_structured_input: true,
            tool_strict: true,
        }
    }
}

fn default_true() -> bool {
    true
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
                "missing OpenAI API key: set {} or provide provider settings.api_key",
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
    #[serde(rename = "refusal")]
    Refusal {
        #[serde(default)]
        id: Option<String>,
        refusal: String,
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
    #[serde(default)]
    refusal: Option<String>,
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

    pub async fn stream_turn(
        &self,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelEventStream> {
        let client = reqwest::Client::builder().build()?;
        let mut body = self.responses_request_body(agent, &request)?;
        body["stream"] = json!(true);

        let response = client
            .post(self.config.endpoint_url("responses"))
            .headers(self.request_headers()?)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::FORBIDDEN
                || status == reqwest::StatusCode::UNAUTHORIZED
            {
                let output = self.prompt_turn(agent, request).await?;
                return Ok(stream_model_turn(output));
            }
            return Err(anyhow!(
                "openai stream request failed: status {status} body {body_text}"
            ));
        }

        let fallback_adapter = self.clone();
        let fallback_agent = agent.clone();
        let fallback_request = request.clone();

        let event_stream: ModelEventStream = Box::pin(try_stream! {
            // SSE 行缓冲
            let mut line_buf = String::new();
            let mut assistant_text = String::new();
            let mut tool_calls: Vec<ToolCallItem> = Vec::new();
            // 按 item_id 追踪函数调用的参数积累
            let mut pending_fn_calls: std::collections::HashMap<String, (String, String)> =
                std::collections::HashMap::new();
            let mut usage: Option<ModelUsage> = None;
            let mut output_id: Option<String> = None;
            let mut emitted_events = false;
            let mut text_index = 0_u64;

            let mut response = response;
            loop {
                let chunk_opt = response.chunk().await.map_err(|e| anyhow!("stream read error: {e}"))?;
                let chunk = match chunk_opt {
                    Some(c) => c,
                    None => break,
                };
                let text = String::from_utf8_lossy(&chunk).into_owned();

                for ch in text.chars() {
                    if ch == '\n' {
                        let line = std::mem::take(&mut line_buf);
                        let line = line.trim_end_matches('\r').to_string();

                        if line.starts_with("data: ") {
                            let data = &line[6..];
                            if data == "[DONE]" {
                                break;
                            }
                            if let Ok(obj) = serde_json::from_str::<Value>(data) {
                                let event_type = obj["type"].as_str().unwrap_or("");

                                match event_type {
                                    // 文本 delta — 实时 token
                                    "response.output_text.delta" => {
                                        if let Some(delta) = obj["delta"].as_str() {
                                            assistant_text.push_str(delta);
                                            let item = AssistantTextItem {
                                                item_id: format!("stream_text_{text_index}"),
                                                text: delta.to_string(),
                                                is_partial: true,
                                            };
                                            text_index += 1;
                                            emitted_events = true;
                                            yield crate::domain::ModelStreamEvent::AssistantTextDelta(item);
                                        }
                                    }

                                    // 函数调用开始（output_item.added 里的 function_call）
                                    "response.output_item.added" => {
                                        if obj["item"]["type"].as_str() == Some("function_call") {
                                            let item = &obj["item"];
                                            let id = item["id"].as_str().unwrap_or("").to_string();
                                            let name = item["name"].as_str().unwrap_or("").to_string();
                                            let call_id = item["call_id"].as_str().unwrap_or(&id).to_string();
                                            pending_fn_calls.insert(id, (name, call_id));
                                        }
                                    }

                                    // 函数调用参数 delta
                                    "response.function_call_arguments.delta" => {
                                        let item_id = obj["item_id"].as_str().unwrap_or("").to_string();
                                        if let Some((_name, _call_id)) = pending_fn_calls.get_mut(&item_id) {
                                            // 参数在 done 事件里汇总，此处忽略 delta
                                            let _ = obj["delta"].as_str();
                                        }
                                    }

                                    // 函数调用完成
                                    "response.output_item.done" => {
                                        if obj["item"]["type"].as_str() == Some("function_call") {
                                            let item = &obj["item"];
                                            let id = item["id"].as_str().unwrap_or("").to_string();
                                            let name = item["name"].as_str().unwrap_or("").to_string();
                                            let call_id = item["call_id"]
                                                .as_str()
                                                .unwrap_or(&id)
                                                .to_string();
                                            let arguments_raw =
                                                item["arguments"].as_str().unwrap_or("{}");
                                            let args = serde_json::from_str(arguments_raw)
                                                .unwrap_or_else(|_| {
                                                    json!({ "_raw_arguments": arguments_raw })
                                                });
                                            let tc = ToolCallItem {
                                                item_id: id.clone(),
                                                tool_call_id: call_id,
                                                tool_name: name,
                                                args,
                                                timeout_secs: None,
                                            };
                                            tool_calls.push(tc.clone());
                                            pending_fn_calls.remove(&id);
                                            emitted_events = true;
                                            yield crate::domain::ModelStreamEvent::ToolCall(tc);
                                        }
                                    }

                                    // 最终 usage（在 response.completed 内）
                                    "response.completed" => {
                                        output_id = obj["response"]["id"]
                                            .as_str()
                                            .map(str::to_string)
                                            .or(output_id);
                                        if let Some(u) = obj["response"]["usage"].as_object() {
                                            let model_usage = ModelUsage {
                                                input_tokens: u["input_tokens"]
                                                    .as_u64()
                                                    .map(|v| v),
                                                output_tokens: u["output_tokens"]
                                                    .as_u64()
                                                    .map(|v| v),
                                            };
                                            usage = Some(model_usage.clone());
                                            yield crate::domain::ModelStreamEvent::Usage(model_usage);
                                        }
                                    }

                                    // 初始错误 — 流开始前就失败，走 fallback
                                    "error" if !emitted_events => {
                                        let output = fallback_adapter
                                            .prompt_turn(&fallback_agent, fallback_request.clone())
                                            .await
                                            .map_err(|e| anyhow!("stream error and fallback failed: {e}"))?;
                                        for evt in model_turn_to_events(output) {
                                            yield evt?;
                                        }
                                        return;
                                    }

                                    _ => {}
                                }
                            }
                        }
                    } else {
                        line_buf.push(ch);
                    }
                }

            }

            // 组装最终输出
            let mut items = Vec::new();
                if !assistant_text.trim().is_empty() {
                    let text = collapse_repeated_text(assistant_text);
                    items.push(ModelOutputItem::AssistantText(AssistantTextItem {
                        item_id: "assistant.final".into(),
                        text,
                        is_partial: false,
                    }));
                }
                items.extend(tool_calls.into_iter().map(ModelOutputItem::ToolCall));

                let finish_reason = if items
                    .iter()
                    .any(|item| matches!(item, ModelOutputItem::ToolCall(_)))
                {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Completed
                };

                yield crate::domain::ModelStreamEvent::Completed(ModelTurnOutput {
                    output_id: output_id.unwrap_or_else(|| "output.openai.stream".into()),
                    items,
                    finish_reason,
                    usage,
                });
        });

        Ok(event_stream)
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
        let input_messages = self.input_messages_for_request(request);
        let mut body = json!({
            "model": agent.model,
            "input": input_messages,
        });

        let instructions = request
            .instructions
            .as_ref()
            .cloned()
            .or_else(|| agent.preamble.clone());
        if let Some(instructions) = instructions {
            body["instructions"] = Value::String(instructions);
        }

        if let Some(previous_response_id) = request.previous_response_id.as_ref() {
            body["previous_response_id"] = Value::String(previous_response_id.clone());
        }
        if let Some(store) = request.store {
            body["store"] = json!(store);
        }

        if let Some(format_value) = request
            .text_format
            .as_ref()
            .or(request.response_format.as_ref())
        {
            if format_value.get("format").is_some() {
                body["text"] = format_value.clone();
            } else {
                body["text"] = json!({
                    "format": format_value,
                });
            }
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
                            "strict": self.config.tool_strict,
                        }))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(false);
        }
        Ok(body)
    }

    fn input_messages_for_request(&self, request: &ProviderPromptRequest) -> Value {
        if self.config.use_structured_input && !request.messages.is_empty() {
            return Value::Array(
                request
                    .messages
                    .iter()
                    .map(openai_input_message_value)
                    .collect(),
            );
        }

        if !request.prompt.trim().is_empty() {
            return json!([{
                "role": "user",
                "content": request.prompt,
            }]);
        }

        Value::Array(vec![])
    }
}

fn openai_input_message_value(message: &ProviderPromptMessage) -> Value {
    let role = normalize_openai_input_role(&message.role);
    json!({
        "role": role,
        "content": message.content,
    })
}

fn normalize_openai_input_role(role: &str) -> &str {
    match role {
        "developer" | "user" | "assistant" | "system" => role,
        _ => "user",
    }
}

#[allow(dead_code)]
fn should_fallback_to_non_streaming(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("403 forbidden")
        || lower.contains("401 unauthorized")
        || lower.contains("415 unsupported media type")
}

fn model_turn_to_events(output: ModelTurnOutput) -> Vec<Result<crate::domain::ModelStreamEvent>> {
    let mut events = Vec::new();
    for item in output.items.iter().cloned() {
        match item {
            ModelOutputItem::AssistantText(item) => {
                events.push(Ok(crate::domain::ModelStreamEvent::AssistantTextDelta(
                    item,
                )));
            }
            ModelOutputItem::ToolCall(item) => {
                events.push(Ok(crate::domain::ModelStreamEvent::ToolCall(item)));
            }
            ModelOutputItem::ToolResult(item) => {
                events.push(Ok(crate::domain::ModelStreamEvent::ToolResult(item)));
            }
            ModelOutputItem::RuntimeControl(item) => {
                events.push(Ok(crate::domain::ModelStreamEvent::RuntimeControl(item)));
            }
        }
    }
    if let Some(usage) = output.usage.clone() {
        events.push(Ok(crate::domain::ModelStreamEvent::Usage(usage)));
    }
    events.push(Ok(crate::domain::ModelStreamEvent::Completed(output)));
    events
}

fn normalize_model_turn_output(payload: OpenAiResponsesApiResponse) -> Result<ModelTurnOutput> {
    let mut items = Vec::new();

    for (index, item) in payload.output.into_iter().enumerate() {
        match item {
            OpenAiResponsesOutputItem::Message { id, content } => {
                let mut message_parts = Vec::new();
                let mut refusal_parts = Vec::new();
                for content_item in content {
                    match content_item.content_type.as_str() {
                        "output_text" => {
                            if let Some(text) = content_item.text {
                                if !text.trim().is_empty() {
                                    message_parts.push(text);
                                }
                            }
                        }
                        "refusal" => {
                            if let Some(refusal) = content_item.refusal {
                                if !refusal.trim().is_empty() {
                                    refusal_parts.push(refusal);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !message_parts.is_empty() {
                    items.extend(parse_text_response_items(
                        &collapse_repeated_text(message_parts.join("\n")),
                        id.clone().unwrap_or_else(|| format!("msg_{index}")),
                    )?);
                }
                if !refusal_parts.is_empty() {
                    items.push(ModelOutputItem::AssistantText(AssistantTextItem {
                        item_id: format!(
                            "{}_refusal",
                            id.unwrap_or_else(|| format!("msg_{index}"))
                        ),
                        text: refusal_parts.join("\n"),
                        is_partial: false,
                    }));
                }
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
            OpenAiResponsesOutputItem::Refusal { id, refusal } => {
                if !refusal.trim().is_empty() {
                    items.push(ModelOutputItem::AssistantText(AssistantTextItem {
                        item_id: id.unwrap_or_else(|| format!("refusal_{index}")),
                        text: refusal,
                        is_partial: false,
                    }));
                }
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

pub fn provider_candidate(
    provider: &LlmProviderConfig,
    config_root: &Path,
) -> Result<PluginManagerCandidate> {
    let config: OpenAiProviderConfig = provider.settings_as_resolved(config_root)?;
    let plugin = std::sync::Arc::new(OpenAiProviderPlugin::new(config));
    Ok(PluginManagerCandidate::provider(
        plugin.clone() as PluginRef,
        plugin.clone(),
        Some(plugin.clone()),
        plugin.provided_resources(),
        true,
    ))
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

    async fn stream_turn(
        &self,
        agent: &AgentDefinition,
        request: ProviderPromptRequest,
    ) -> Result<ModelEventStream> {
        OpenAiProviderAdapter::new(self.config.clone())
            .stream_turn(agent, request)
            .await
    }
}

#[async_trait]
impl BillingPlugin for OpenAiProviderPlugin {
    async fn estimate_billing(&self, usage: &UsageRecord) -> Result<Option<BillingRecord>> {
        if usage.provider_id.as_deref() != Some(self.config.provider_id.as_str()) {
            return Ok(None);
        }

        let pricing = usage
            .model_id
            .as_deref()
            .and_then(openai_pricing_rule_for_model)
            .or_else(|| {
                matches!(usage.category, UsageCategory::Embedding).then_some(OpenAiPricingRule {
                    rule_id: "openai.embedding.generic.v2026-04-13",
                    input_per_million: 0.02,
                    cached_input_per_million: None,
                    output_per_million: 0.0,
                    confidence: BillingConfidence::Low,
                })
            });
        let Some(pricing) = pricing else {
            return Ok(None);
        };

        let mut amount = 0.0_f64;
        let mut breakdown = Vec::new();

        match usage.category {
            UsageCategory::ModelText | UsageCategory::ModelReasoning => {
                if let Some(input_tokens) = usage.input_tokens {
                    let subtotal = input_tokens as f64 / 1_000_000.0 * pricing.input_per_million;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "input_tokens".into(),
                        quantity: input_tokens.to_string(),
                        unit_price: Some(format!("{:.4} / 1M tokens", pricing.input_per_million)),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("standard processing estimate".into()),
                    });
                }

                if let (Some(cached_tokens), Some(cached_rate)) =
                    (usage.cached_tokens, pricing.cached_input_per_million)
                {
                    let subtotal = cached_tokens as f64 / 1_000_000.0 * cached_rate;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "cached_input_tokens".into(),
                        quantity: cached_tokens.to_string(),
                        unit_price: Some(format!("{cached_rate:.4} / 1M tokens")),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("prompt caching estimate".into()),
                    });
                }

                if let Some(output_tokens) = usage.output_tokens {
                    let subtotal = output_tokens as f64 / 1_000_000.0 * pricing.output_per_million;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "output_tokens".into(),
                        quantity: output_tokens.to_string(),
                        unit_price: Some(format!("{:.4} / 1M tokens", pricing.output_per_million)),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("standard processing estimate".into()),
                    });
                }
            }
            UsageCategory::Embedding => {
                if let Some(input_tokens) = usage.input_tokens {
                    let subtotal = input_tokens as f64 / 1_000_000.0 * pricing.input_per_million;
                    amount += subtotal;
                    breakdown.push(BillingBreakdownItem {
                        item_type: "embedding_tokens".into(),
                        quantity: input_tokens.to_string(),
                        unit_price: Some(format!("{:.4} / 1M tokens", pricing.input_per_million)),
                        subtotal: format!("{subtotal:.8}"),
                        note: Some("generic embedding estimate".into()),
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
            rule_id: Some(pricing.rule_id.into()),
            confidence: pricing.confidence,
            breakdown,
            generated_at: chrono::Utc::now(),
        }))
    }
}

#[derive(Debug)]
struct OpenAiPricingRule {
    rule_id: &'static str,
    input_per_million: f64,
    cached_input_per_million: Option<f64>,
    output_per_million: f64,
    confidence: BillingConfidence,
}

fn openai_pricing_rule_for_model(model_id: &str) -> Option<OpenAiPricingRule> {
    let model_id = model_id.to_ascii_lowercase();
    let rules = [
        (
            "gpt-5.4-mini",
            OpenAiPricingRule {
                rule_id: "openai.gpt-5.4-mini.standard.v2026-04-13",
                input_per_million: 0.75,
                cached_input_per_million: Some(0.075),
                output_per_million: 4.50,
                confidence: BillingConfidence::High,
            },
        ),
        (
            "gpt-5.4-nano",
            OpenAiPricingRule {
                rule_id: "openai.gpt-5.4-nano.standard.v2026-04-13",
                input_per_million: 0.20,
                cached_input_per_million: Some(0.02),
                output_per_million: 1.25,
                confidence: BillingConfidence::High,
            },
        ),
        (
            "gpt-5.4",
            OpenAiPricingRule {
                rule_id: "openai.gpt-5.4.standard.v2026-04-13",
                input_per_million: 2.50,
                cached_input_per_million: Some(0.25),
                output_per_million: 15.00,
                confidence: BillingConfidence::High,
            },
        ),
        (
            "gpt-5.3-codex",
            OpenAiPricingRule {
                rule_id: "openai.gpt-5.3-codex.standard.v2026-04-13",
                input_per_million: 1.75,
                cached_input_per_million: Some(0.175),
                output_per_million: 14.00,
                confidence: BillingConfidence::High,
            },
        ),
        (
            "gpt-5.3-chat-latest",
            OpenAiPricingRule {
                rule_id: "openai.gpt-5.3-chat.standard.v2026-04-13",
                input_per_million: 1.75,
                cached_input_per_million: Some(0.175),
                output_per_million: 14.00,
                confidence: BillingConfidence::High,
            },
        ),
        (
            "gpt-4.1-mini",
            OpenAiPricingRule {
                rule_id: "openai.gpt-4.1-mini.standard.v2026-04-13",
                input_per_million: 0.40,
                cached_input_per_million: Some(0.10),
                output_per_million: 1.60,
                confidence: BillingConfidence::Medium,
            },
        ),
        (
            "gpt-4.1",
            OpenAiPricingRule {
                rule_id: "openai.gpt-4.1.standard.v2026-04-13",
                input_per_million: 2.00,
                cached_input_per_million: Some(0.50),
                output_per_million: 8.00,
                confidence: BillingConfidence::Medium,
            },
        ),
        (
            "gpt-4o-mini",
            OpenAiPricingRule {
                rule_id: "openai.gpt-4o-mini.standard.v2026-04-13",
                input_per_million: 0.15,
                cached_input_per_million: Some(0.075),
                output_per_million: 0.60,
                confidence: BillingConfidence::Medium,
            },
        ),
        (
            "gpt-4o",
            OpenAiPricingRule {
                rule_id: "openai.gpt-4o.standard.v2026-04-13",
                input_per_million: 2.50,
                cached_input_per_million: Some(1.25),
                output_per_million: 10.00,
                confidence: BillingConfidence::Medium,
            },
        ),
        (
            "gpt-realtime-1.5",
            OpenAiPricingRule {
                rule_id: "openai.gpt-realtime-1.5.text.standard.v2026-04-13",
                input_per_million: 4.00,
                cached_input_per_million: Some(0.40),
                output_per_million: 16.00,
                confidence: BillingConfidence::High,
            },
        ),
        (
            "gpt-realtime-mini",
            OpenAiPricingRule {
                rule_id: "openai.gpt-realtime-mini.text.standard.v2026-04-13",
                input_per_million: 0.60,
                cached_input_per_million: Some(0.06),
                output_per_million: 2.40,
                confidence: BillingConfidence::High,
            },
        ),
    ];

    rules
        .into_iter()
        .find_map(|(prefix, rule)| model_id.starts_with(prefix).then_some(rule))
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
    use super::*;

    use crate::core::BillingPlugin;
    use crate::domain::{FinishReason, ModelOutputItem, ModelTurnOutput, ModelUsage, ToolCallItem};

    fn test_agent() -> AgentDefinition {
        AgentDefinition {
            agent_id: "test-agent".into(),
            provider_id: "openai-default".into(),
            model: "gpt-4o-mini".into(),
            preamble: Some("default preamble".into()),
            temperature: Some(0.1),
            max_tokens: Some(256),
        }
    }

    #[tokio::test]
    async fn estimate_billing_uses_model_pricing_rule_for_text_usage() {
        let plugin = OpenAiProviderPlugin::new(OpenAiProviderConfig {
            provider_id: "openai-default".into(),
            api_key: Some("test-key".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: None,
            organization: None,
            project: None,
            use_structured_input: true,
            tool_strict: true,
        });
        let usage = UsageRecord {
            usage_id: "usage_1".into(),
            category: UsageCategory::ModelText,
            provider_id: Some("openai-default".into()),
            model_id: Some("gpt-4o-mini".into()),
            resource_id: "resource.openai.responses".into(),
            endpoint_id: Some("responses".into()),
            region: None,
            account_id: None,
            project_id: None,
            workspace_id: Some("workspace_1".into()),
            agent_id: Some("agent_1".into()),
            session_id: Some("session_1".into()),
            task_id: Some("task_1".into()),
            plugin_id: Some("provider.openai.openai-default".into()),
            request_count: 1,
            response_count: 1,
            message_count: 2,
            input_tokens: Some(2_000),
            output_tokens: Some(1_000),
            cached_tokens: Some(500),
            reasoning_tokens: None,
            audio_seconds: None,
            image_count: None,
            video_count: None,
            tool_call_count: None,
            context_window_used: Some(3_000),
            max_context_tier_crossed: None,
            started_at: chrono::Utc::now(),
            ended_at: chrono::Utc::now(),
            latency_ms: 120,
            retry_count: 0,
        };

        let billing = plugin
            .estimate_billing(&usage)
            .await
            .expect("billing estimation failed")
            .expect("expected billing estimate");

        assert_eq!(billing.mode, BillingMode::Estimated);
        assert_eq!(billing.currency, "USD");
        assert_eq!(
            billing.rule_id.as_deref(),
            Some("openai.gpt-4o-mini.standard.v2026-04-13")
        );
        assert_eq!(billing.breakdown.len(), 3);
        assert_eq!(billing.breakdown[0].item_type, "input_tokens");
        assert_eq!(billing.breakdown[1].item_type, "cached_input_tokens");
        assert_eq!(billing.breakdown[2].item_type, "output_tokens");
        assert_eq!(billing.amount, "0.00093750");
        assert_eq!(billing.confidence, BillingConfidence::Medium);
    }

    #[test]
    fn pricing_rule_matches_current_flagship_models() {
        let gpt_54 = openai_pricing_rule_for_model("gpt-5.4").expect("gpt-5.4 rule exists");
        assert_eq!(gpt_54.input_per_million, 2.50);
        assert_eq!(gpt_54.cached_input_per_million, Some(0.25));
        assert_eq!(gpt_54.output_per_million, 15.00);

        let gpt_54_mini =
            openai_pricing_rule_for_model("gpt-5.4-mini").expect("gpt-5.4-mini rule exists");
        assert_eq!(gpt_54_mini.input_per_million, 0.75);
        assert_eq!(gpt_54_mini.cached_input_per_million, Some(0.075));
        assert_eq!(gpt_54_mini.output_per_million, 4.50);
    }

    #[test]
    fn responses_request_body_uses_structured_messages_and_structured_fields() {
        let adapter = OpenAiProviderAdapter::new(OpenAiProviderConfig {
            provider_id: "openai-default".into(),
            api_key: Some("test-key".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: None,
            organization: None,
            project: None,
            use_structured_input: true,
            tool_strict: true,
        });
        let body = adapter
            .responses_request_body(
                &test_agent(),
                &ProviderPromptRequest {
                    instructions: Some("runtime instructions".into()),
                    messages: vec![
                        ProviderPromptMessage {
                            role: "developer".into(),
                            content: "<developer>rules</developer>".into(),
                        },
                        ProviderPromptMessage {
                            role: "user".into(),
                            content: "<user>task</user>".into(),
                        },
                    ],
                    previous_response_id: Some("resp_prev_123".into()),
                    text_format: Some(json!({
                        "type": "json_schema",
                        "name": "tool_result",
                        "schema": {
                            "type": "object",
                            "properties": { "ok": { "type": "boolean" } },
                            "required": ["ok"]
                        }
                    })),
                    response_format: None,
                    store: Some(true),
                    prompt: "<legacy_prompt />".into(),
                    tools: vec![],
                },
            )
            .expect("request body");

        assert_eq!(body["instructions"], "runtime instructions");
        assert_eq!(body["previous_response_id"], "resp_prev_123");
        assert_eq!(body["store"], true);
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["role"], "user");
        assert_eq!(body["input"][1]["content"], "<user>task</user>");
        assert_eq!(body["text"]["format"]["type"], "json_schema");
    }

    #[test]
    fn responses_request_body_supports_legacy_prompt_path() {
        let adapter = OpenAiProviderAdapter::new(OpenAiProviderConfig {
            provider_id: "openai-default".into(),
            api_key: Some("test-key".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: None,
            organization: None,
            project: None,
            use_structured_input: false,
            tool_strict: true,
        });
        let body = adapter
            .responses_request_body(
                &test_agent(),
                &ProviderPromptRequest {
                    prompt: "<agentjax_prompt />".into(),
                    ..ProviderPromptRequest::default()
                },
            )
            .expect("request body");

        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "<agentjax_prompt />");
    }

    #[test]
    fn responses_request_body_tool_strict_respects_config() {
        let adapter = OpenAiProviderAdapter::new(OpenAiProviderConfig {
            provider_id: "openai-default".into(),
            api_key: Some("test-key".into()),
            api_key_env: "OPENAI_API_KEY".into(),
            base_url: None,
            organization: None,
            project: None,
            use_structured_input: true,
            tool_strict: false,
        });
        let body = adapter
            .responses_request_body(
                &test_agent(),
                &ProviderPromptRequest {
                    prompt: "test".into(),
                    tools: vec![crate::builtin::tools::ToolDescriptor {
                        name: "echo".into(),
                        description: "echo".into(),
                        when_to_use: "when needed".into(),
                        when_not_to_use: "never".into(),
                        arguments_schema: json!({
                            "type": "object",
                            "properties": {
                                "value": { "type": "string" }
                            }
                        }),
                        idempotent: true,
                        default_timeout_secs: 10,
                    }],
                    ..ProviderPromptRequest::default()
                },
            )
            .expect("request body");

        assert_eq!(body["tools"][0]["strict"], false);
    }

    #[test]
    fn normalize_model_turn_output_parses_function_call_and_refusal() {
        let output = normalize_model_turn_output(OpenAiResponsesApiResponse {
            id: Some("resp_1".into()),
            usage: None,
            output_text: None,
            output: vec![
                OpenAiResponsesOutputItem::Message {
                    id: Some("msg_1".into()),
                    content: vec![OpenAiResponsesContentItem {
                        content_type: "refusal".into(),
                        text: None,
                        refusal: Some("I can't help with that.".into()),
                    }],
                },
                OpenAiResponsesOutputItem::FunctionCall {
                    id: Some("fc_1".into()),
                    call_id: Some("call_1".into()),
                    name: "shell_exec".into(),
                    arguments: "{\"command\":\"pwd\"}".into(),
                },
            ],
        })
        .expect("normalize output");

        assert_eq!(output.output_id, "resp_1");
        assert!(
            output
                .items
                .iter()
                .any(|item| matches!(item, ModelOutputItem::ToolCall(_)))
        );
        assert!(output
            .items
            .iter()
            .any(|item| matches!(item, ModelOutputItem::AssistantText(text) if text.text.contains("can't help"))));
        assert_eq!(output.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn model_turn_to_events_keeps_tool_call_events() {
        let output = ModelTurnOutput {
            output_id: "resp_stream".into(),
            items: vec![ModelOutputItem::ToolCall(ToolCallItem {
                item_id: "fc_1".into(),
                tool_call_id: "call_1".into(),
                tool_name: "echo".into(),
                args: json!({"value":"ok"}),
                timeout_secs: None,
            })],
            finish_reason: FinishReason::ToolCalls,
            usage: Some(ModelUsage {
                input_tokens: Some(12),
                output_tokens: Some(4),
            }),
        };

        let events = model_turn_to_events(output);
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Ok(crate::domain::ModelStreamEvent::ToolCall(ToolCallItem { tool_name, .. })) if tool_name == "echo"
            )
        }));
        assert!(
            events.iter().any(|event| {
                matches!(event, Ok(crate::domain::ModelStreamEvent::Completed(_)))
            })
        );
    }
}
