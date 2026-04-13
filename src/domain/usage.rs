use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageCategory {
    ModelText,
    ModelReasoning,
    Embedding,
    Reranker,
    AudioTts,
    AudioSt,
    ImageGeneration,
    VideoGeneration,
    MusicGeneration,
    ToolExecution,
    ChannelDelivery,
    ArtifactStorage,
    NodeExecution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageRecord {
    pub usage_id: String,
    pub category: UsageCategory,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub resource_id: String,
    pub endpoint_id: Option<String>,
    pub region: Option<String>,
    pub account_id: Option<String>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub plugin_id: Option<String>,
    pub request_count: u32,
    pub response_count: u32,
    pub message_count: u32,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub audio_seconds: Option<f64>,
    pub image_count: Option<u32>,
    pub video_count: Option<u32>,
    pub tool_call_count: Option<u32>,
    pub context_window_used: Option<u64>,
    pub max_context_tier_crossed: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub latency_ms: u64,
    pub retry_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;

    #[test]
    fn usage_record_round_trips_with_metering_fields() {
        let record = UsageRecord {
            usage_id: "usage_1".into(),
            category: UsageCategory::ModelText,
            provider_id: Some("openai-default".into()),
            model_id: Some("gpt-4o-mini".into()),
            resource_id: "resource.llm".into(),
            endpoint_id: Some("responses".into()),
            region: Some("global".into()),
            account_id: Some("acct_1".into()),
            project_id: Some("proj_1".into()),
            workspace_id: Some("workspace_1".into()),
            agent_id: Some("agent_1".into()),
            session_id: Some("session_1".into()),
            task_id: Some("task_1".into()),
            plugin_id: Some("provider.openai".into()),
            request_count: 1,
            response_count: 1,
            message_count: 2,
            input_tokens: Some(1200),
            output_tokens: Some(300),
            cached_tokens: Some(10),
            reasoning_tokens: Some(25),
            audio_seconds: None,
            image_count: None,
            video_count: None,
            tool_call_count: Some(1),
            context_window_used: Some(2048),
            max_context_tier_crossed: Some("standard".into()),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            latency_ms: 250,
            retry_count: 0,
        };

        let value = serde_json::to_value(&record).expect("usage record serialization failed");
        let decoded: UsageRecord =
            serde_json::from_value(value).expect("usage record deserialization failed");

        assert_eq!(decoded.usage_id, record.usage_id);
        assert_eq!(decoded.input_tokens, Some(1200));
        assert_eq!(decoded.output_tokens, Some(300));
        assert_eq!(decoded.tool_call_count, Some(1));
        assert_eq!(decoded.context_window_used, Some(2048));
    }
}
