use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;

use crate::{
    builtin::tools::{support, ToolDescriptor, ToolPlugin},
    core::Plugin,
    domain::{PluginCapability, PluginManifest, ToolCall, ToolCapability},
};

const MIN_SLEEP_MS: i64 = 100;
const MAX_SLEEP_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Default)]
pub struct SleepToolPlugin;

#[async_trait]
impl Plugin for SleepToolPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "tool.builtin.sleep".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Tool(ToolCapability::Tool)],
            config_schema: None,
            required_permissions: vec![],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}

#[async_trait]
impl ToolPlugin for SleepToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "sleep".into(),
            description: "Request runtime suspension and future resumption of the current execution chain."
                .into(),
            when_to_use: "Use when a future re-check is more appropriate than busy-waiting, especially with shell sessions or external jobs."
                .into(),
            when_not_to_use: "Do not use for immediate retries, error masking, or meaningless short waits."
                .into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "duration_ms": { "type": "integer" },
                    "duration_secs": { "type": "integer" },
                    "until": { "type": "string", "description": "RFC3339 timestamp" },
                    "reason": { "type": "string" },
                    "resume_hint": { "type": "string" }
                }
            }),
            default_timeout_secs: 5,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let (wake_at, duration_ms) = resolve_wake_at(call)?;
        let reason = support::parse_optional_string(&call.args, "reason").map(str::to_string);
        let resume_hint =
            support::parse_optional_string(&call.args, "resume_hint").map(str::to_string);
        support::json_tool_output(json!({
            "accepted": true,
            "status": "scheduled",
            "control_action": "sleep",
            "wake_at": wake_at,
            "duration_ms": duration_ms,
            "task_id": call.task_id,
            "turn_id": call.turn_id,
            "reason": reason,
            "resume_hint": resume_hint,
        }))
    }
}

fn resolve_wake_at(call: &ToolCall) -> Result<(DateTime<Utc>, i64)> {
    if let Some(until) = support::parse_optional_string(&call.args, "until") {
        let wake_at = DateTime::parse_from_rfc3339(until)
            .map_err(|error| anyhow!("invalid sleep until timestamp: {error}"))?
            .with_timezone(&Utc);
        let duration_ms = (wake_at - Utc::now()).num_milliseconds();
        validate_sleep_duration(duration_ms)?;
        return Ok((wake_at, duration_ms));
    }

    let duration_ms = if let Some(ms) = support::parse_optional_usize(&call.args, "duration_ms")? {
        i64::try_from(ms).map_err(|_| anyhow!("sleep duration_ms is too large"))?
    } else if let Some(secs) = support::parse_optional_usize(&call.args, "duration_secs")? {
        let secs = i64::try_from(secs).map_err(|_| anyhow!("sleep duration_secs is too large"))?;
        secs.saturating_mul(1_000)
    } else {
        bail!("sleep requires args.duration_ms, args.duration_secs, or args.until");
    };
    validate_sleep_duration(duration_ms)?;
    Ok((
        Utc::now() + Duration::milliseconds(duration_ms),
        duration_ms,
    ))
}

fn validate_sleep_duration(duration_ms: i64) -> Result<()> {
    if duration_ms < MIN_SLEEP_MS {
        bail!("sleep duration must be at least {MIN_SLEEP_MS} ms");
    }
    if duration_ms > MAX_SLEEP_MS {
        bail!("sleep duration must be at most {MAX_SLEEP_MS} ms");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::SleepToolPlugin;
    use crate::{
        builtin::tools::ToolPlugin,
        domain::{ToolCall, ToolCaller},
    };

    #[tokio::test]
    async fn sleep_tool_returns_scheduled_metadata() {
        let output = SleepToolPlugin
            .invoke(&tool_call(json!({
                "duration_secs": 1,
                "reason": "wait for shell output",
                "resume_hint": "check shsess_1",
            })))
            .await
            .unwrap();
        assert_eq!(output.metadata["accepted"], true);
        assert_eq!(output.metadata["control_action"], "sleep");
        assert_eq!(output.metadata["status"], "scheduled");
    }

    fn tool_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: "call-sleep".into(),
            tool_name: "sleep".into(),
            args,
            requested_by: ToolCaller::Operator {
                operator_id: "test".into(),
            },
            session_id: Some("session.default".into()),
            task_id: Some("task_1".into()),
            turn_id: Some("turn_1".into()),
            idempotency_key: None,
            timeout_secs: None,
        }
    }
}
