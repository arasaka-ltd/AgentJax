use super::*;

pub(super) fn shell_tool_started_event(tool_call: &ToolCall) -> Option<(EventType, Value)> {
    match tool_call.tool_name.as_str() {
        "shell_exec" => Some((
            EventType::ShellExecutionStarted,
            json!({
                "tool_name": tool_call.tool_name,
                "mode": "stateless",
                "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                "cwd": tool_call.args.get("cwd").and_then(|value| value.as_str()),
                "timeout_secs": tool_call.args.get("timeout_secs"),
            }),
        )),
        "shell_session_exec" => Some((
            EventType::ShellExecutionStarted,
            json!({
                "tool_name": tool_call.tool_name,
                "mode": "session_bound",
                "session_id": tool_call.args.get("session_id").and_then(|value| value.as_str()),
                "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                "timeout_secs": tool_call.args.get("timeout_secs"),
                "detach": tool_call.args.get("detach").and_then(|value| value.as_bool()).unwrap_or(false),
            }),
        )),
        _ => None,
    }
}

pub(super) fn shell_tool_result_events(
    tool_call: &ToolCall,
    metadata: &Value,
) -> Vec<(EventType, Value)> {
    let mut events = Vec::new();
    match tool_call.tool_name.as_str() {
        "shell_exec" => {
            let exit_code = metadata.get("exit_code").and_then(|value| value.as_i64());
            let timed_out = metadata
                .get("timed_out")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let event_type = if timed_out || exit_code.unwrap_or_default() != 0 {
                EventType::ShellExecutionFailed
            } else {
                EventType::ShellExecutionCompleted
            };
            events.push((
                event_type,
                json!({
                    "mode": "stateless",
                    "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                    "cwd": metadata.get("cwd"),
                    "exit_code": metadata.get("exit_code"),
                    "timed_out": timed_out,
                    "truncated": metadata.get("truncated"),
                }),
            ));
        }
        "shell_session_open" => events.push((EventType::ShellSessionOpened, metadata.clone())),
        "shell_session_read" => {
            if let Some(chunks) = metadata.get("chunks").and_then(|value| value.as_array()) {
                if !chunks.is_empty() {
                    events.push((
                        EventType::ShellOutputAppended,
                        json!({
                            "session_id": metadata.get("session_id"),
                            "seq": metadata.get("seq"),
                            "chunk_count": chunks.len(),
                            "chunks": chunks,
                        }),
                    ));
                }
            }
            if let Some(completed_exec) = metadata.get("completed_exec") {
                let event_type = match completed_exec
                    .get("status")
                    .and_then(|value| value.as_str())
                {
                    Some("interrupted") => EventType::ShellExecutionInterrupted,
                    Some("failed") | Some("timed_out") => EventType::ShellExecutionFailed,
                    _ => EventType::ShellExecutionCompleted,
                };
                events.push((event_type, completed_exec.clone()));
            }
        }
        "shell_session_close" => events.push((EventType::ShellSessionClosed, metadata.clone())),
        "shell_session_interrupt" => {
            if metadata
                .get("signaled")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                events.push((EventType::ShellExecutionInterrupted, metadata.clone()));
            }
        }
        "shell_session_resize" => {
            if metadata
                .get("resized")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                events.push((EventType::ShellSessionResized, metadata.clone()));
            }
        }
        _ => {}
    }
    events
}

pub(super) fn shell_tool_failure_event(
    tool_call: &ToolCall,
    error: &str,
) -> Option<(EventType, Value)> {
    match tool_call.tool_name.as_str() {
        "shell_exec" | "shell_session_exec" => Some((
            EventType::ShellExecutionFailed,
            json!({
                "tool_name": tool_call.tool_name,
                "command": tool_call.args.get("command").and_then(|value| value.as_str()),
                "session_id": tool_call.args.get("session_id").and_then(|value| value.as_str()),
                "error": error,
            }),
        )),
        _ => None,
    }
}

pub(super) trait StreamEnvelopeExt {
    fn stream_id(&self) -> Option<&str>;
}

impl StreamEnvelopeExt for ServerEnvelope {
    fn stream_id(&self) -> Option<&str> {
        match self {
            ServerEnvelope::Stream(stream) => Some(stream.stream_id.0.as_str()),
            _ => None,
        }
    }
}

pub(super) enum ToolLoopOutcome {
    Final(String),
    Sleeping(String),
}

pub(super) struct SemanticStreamBuilder {
    stream_id: String,
    seq: u64,
    envelopes: Vec<ServerEnvelope>,
    live_stream: Option<UnboundedSender<ServerEnvelope>>,
}

impl SemanticStreamBuilder {
    pub(super) fn new(
        stream_id: String,
        turn_id: &str,
        live_stream: Option<UnboundedSender<ServerEnvelope>>,
    ) -> Self {
        let mut builder = Self {
            stream_id: stream_id.clone(),
            seq: 0,
            envelopes: Vec::new(),
            live_stream,
        };
        builder.push("turn.started", json!({ "turn_id": turn_id }));
        builder
    }

    pub(super) fn push(&mut self, event: &str, data: Value) {
        let envelope = ServerEnvelope::Stream(StreamEnvelope {
            stream_id: self.stream_id.clone().into(),
            phase: if self.seq == 0 {
                StreamPhase::Start
            } else {
                StreamPhase::Chunk
            },
            event: event.into(),
            seq: self.seq,
            data,
            meta: None,
        });
        if let Some(live_stream) = self.live_stream.as_ref() {
            let _ = live_stream.send(envelope.clone());
        }
        self.envelopes.push(envelope);
        self.seq += 1;
    }

    fn push_text(&mut self, turn_id: &str, text: &str) {
        for chunk in chunk_stream_text(text) {
            self.push(
                "assistant.text.delta",
                json!({
                    "turn_id": turn_id,
                    "text": chunk,
                }),
            );
        }
    }

    pub(super) fn finish(mut self, turn_id: &str, status: &str) -> Vec<ServerEnvelope> {
        self.push(
            "turn.completed",
            json!({
                "turn_id": turn_id,
                "status": status,
            }),
        );
        let envelope = ServerEnvelope::Stream(StreamEnvelope {
            stream_id: self.stream_id.into(),
            phase: StreamPhase::End,
            event: "stream.completed".into(),
            seq: self.seq,
            data: json!({ "turn_id": turn_id, "done": true, "status": status }),
            meta: None,
        });
        if let Some(live_stream) = self.live_stream.as_ref() {
            let _ = live_stream.send(envelope.clone());
        }
        self.envelopes.push(envelope);
        self.envelopes
    }
}

pub(super) fn stream_error_envelope(
    stream_id: &str,
    turn_id: &str,
    message: &str,
) -> ServerEnvelope {
    ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::Error,
        event: "stream.error".into(),
        seq: 0,
        data: json!({
            "turn_id": turn_id,
            "message": message,
        }),
        meta: None,
    })
}

pub(super) fn stream_terminal_envelope(
    stream_id: &str,
    turn_id: &str,
    status: &str,
) -> ServerEnvelope {
    ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::End,
        event: "stream.completed".into(),
        seq: 1,
        data: json!({
            "turn_id": turn_id,
            "done": true,
            "status": status,
        }),
        meta: None,
    })
}

#[derive(Debug, Clone)]
pub(super) struct SleepDirective {
    wake_at: chrono::DateTime<chrono::Utc>,
    reason: Option<String>,
    resume_hint: Option<String>,
    duration_ms: Option<i64>,
}

impl Daemon {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn run_tool_loop(
        &self,
        session_id: &str,
        task_id: &str,
        turn_id: &str,
        session_agent: &crate::config::AgentDefinition,
        _session: &Session,
        assembled_context: &AssembledContext,
        conversation_messages: Vec<SessionMessage>,
        mut stream: Option<&mut SemanticStreamBuilder>,
    ) -> Result<ToolLoopOutcome, ApiError> {
        let mut loop_messages = conversation_messages;

        for iteration in 0..=MAX_TOOL_LOOP_STEPS {
            let prompt =
                build_tool_followup_prompt(&self.app, assembled_context, loop_messages.clone());
            let request_started_at = Utc::now();
            let request_started = std::time::Instant::now();
            self.record_event(
                session_id,
                turn_id,
                Some(task_id),
                EventType::ModelCalled,
                json!({
                    "provider_id": session_agent.provider_id.clone(),
                    "model_id": session_agent.model.clone(),
                    "phase": if iteration == 0 { "initial" } else { "tool_loop" },
                    "iteration": iteration,
                }),
            )?;
            let response = if stream.is_some() {
                let mut model_stream = self
                    .app
                    .runtime
                    .stream_turn(AgentPromptRequest {
                        prompt,
                        agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
                        agent_override: Some(session_agent.clone()),
                        tools: self.app.tool_registry.descriptors(),
                    })
                    .await
                    .map_err(|error| {
                        ApiError::new(
                            ApiErrorCode::InternalError,
                            format!("model stream failed in tool loop: {error}"),
                            false,
                        )
                    })?;
                let mut final_output = None;
                while let Some(event) = model_stream.next().await {
                    let event = event.map_err(|error| {
                        ApiError::new(
                            ApiErrorCode::InternalError,
                            format!("model stream event failed in tool loop: {error}"),
                            false,
                        )
                    })?;
                    match event {
                        crate::domain::ModelStreamEvent::AssistantTextDelta(item) => {
                            if let Some(stream) = stream.as_deref_mut() {
                                if !item.text.is_empty() {
                                    stream.push_text(turn_id, &item.text);
                                }
                            }
                        }
                        crate::domain::ModelStreamEvent::Completed(output) => {
                            final_output = Some(output);
                        }
                        crate::domain::ModelStreamEvent::ToolCall(_)
                        | crate::domain::ModelStreamEvent::ToolResult(_)
                        | crate::domain::ModelStreamEvent::RuntimeControl(_)
                        | crate::domain::ModelStreamEvent::Usage(_) => {}
                    }
                }
                final_output.ok_or_else(|| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        "model stream finished without final output",
                        false,
                    )
                })?
            } else {
                self.app
                    .runtime
                    .prompt_turn(AgentPromptRequest {
                        prompt,
                        agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
                        agent_override: Some(session_agent.clone()),
                        tools: self.app.tool_registry.descriptors(),
                    })
                    .await
                    .map_err(|error| {
                        ApiError::new(
                            ApiErrorCode::InternalError,
                            format!("model call failed in tool loop: {error}"),
                            false,
                        )
                    })?
            };
            self.record_event(
                session_id,
                turn_id,
                Some(task_id),
                EventType::ModelResponseReceived,
                json!({
                    "output_id": response.output_id,
                    "items": response.items,
                    "finish_reason": response.finish_reason,
                    "usage": response.usage,
                    "iteration": iteration,
                }),
            )?;
            if let Some(usage) = response.usage.as_ref() {
                let latency_ms = request_started
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                self.record_usage_and_billing(
                    session_id,
                    task_id,
                    turn_id,
                    session_agent,
                    usage,
                    loop_messages.len() as u32 + response.items.len() as u32,
                    request_started_at,
                    latency_ms,
                )
                .await?;
            }
            let assistant_text = response.assistant_text();
            let tool_calls = collect_tool_calls(
                &response,
                session_id,
                task_id,
                turn_id,
                &session_agent.agent_id,
            );

            if tool_calls.is_empty() {
                return Ok(ToolLoopOutcome::Final(assistant_text));
            }

            if !assistant_text.trim().is_empty() {
                loop_messages.push(SessionMessage::assistant(assistant_text.clone()));
            }

            for tool_call in tool_calls {
                self.record_event(
                    session_id,
                    turn_id,
                    Some(task_id),
                    EventType::ToolCallRequested,
                    json!({
                        "tool_call_id": tool_call.tool_call_id,
                        "tool_name": tool_call.tool_name,
                        "args": tool_call.args,
                        "timeout_secs": tool_call.timeout_secs,
                        "iteration": iteration,
                    }),
                )?;
                if let Some(stream) = stream.as_deref_mut() {
                    stream.push(
                        "tool_call.started",
                        json!({
                            "turn_id": turn_id,
                            "tool_call_id": tool_call.tool_call_id,
                            "tool_name": tool_call.tool_name,
                            "args": tool_call.args,
                        }),
                    );
                }
                let tool_result =
                    execute_tool_call_item(&self.execute_tool_call(&tool_call).await?, &tool_call);
                let mut tool_result_message =
                    SessionMessage::tool_result(tool_result.content.clone());
                tool_result_message.annotations = vec![
                    SessionMessageAnnotation {
                        kind: "tool_name".into(),
                        value: tool_call.tool_name.clone(),
                    },
                    SessionMessageAnnotation {
                        kind: "tool_call_id".into(),
                        value: tool_call.tool_call_id.clone(),
                    },
                    SessionMessageAnnotation {
                        kind: "tool_error".into(),
                        value: tool_result.is_error.to_string(),
                    },
                ];
                loop_messages.push(tool_result_message);
                if let Some(stream) = stream.as_deref_mut() {
                    stream.push(
                        "tool_call.completed",
                        json!({
                            "turn_id": turn_id,
                            "tool_call_id": tool_call.tool_call_id,
                            "tool_name": tool_call.tool_name,
                            "content": tool_result.content,
                            "metadata": tool_result.metadata,
                            "is_error": tool_result.is_error,
                        }),
                    );
                }
                if let Some(sleep) = parse_sleep_directive(&tool_call, &tool_result)? {
                    self.apply_sleep_directive(session_id, task_id, turn_id, sleep.clone())
                        .await?;
                    return Ok(ToolLoopOutcome::Sleeping(format!(
                        "Runtime scheduled resume at {}. {}",
                        sleep.wake_at.to_rfc3339(),
                        sleep
                            .resume_hint
                            .unwrap_or_else(|| "The task is now waiting.".into())
                    )));
                }
            }
        }

        Err(ApiError::new(
            ApiErrorCode::InternalError,
            format!("tool loop exceeded max iterations ({MAX_TOOL_LOOP_STEPS})"),
            false,
        ))
    }

    async fn apply_sleep_directive(
        &self,
        session_id: &str,
        task_id: &str,
        turn_id: &str,
        sleep: SleepDirective,
    ) -> Result<(), ApiError> {
        self.record_event(
            session_id,
            turn_id,
            Some(task_id),
            EventType::SleepRequested,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "wake_at": sleep.wake_at,
                "reason": sleep.reason,
                "resume_hint": sleep.resume_hint,
                "duration_ms": sleep.duration_ms,
            }),
        )?;

        let checkpoint = self
            .app
            .context_engine
            .build_resume_pack(Some(session_id), Some(task_id))
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("resume pack build failed: {error}"),
                    false,
                )
            })?;
        self.store
            .write_task_checkpoint(
                task_id,
                Some(session_id),
                Some(turn_id),
                sleep
                    .resume_hint
                    .clone()
                    .unwrap_or_else(|| format!("wake at {}", sleep.wake_at.to_rfc3339())),
                checkpoint,
            )
            .map_err(internal_store_error)?;

        let mut task = self
            .store
            .get_task(task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        task.task.status = TaskStatus::Waiting;
        task.task.waiting_until = Some(sleep.wake_at);
        task.task.waiting_reason = sleep.reason.clone();
        task.task.waiting_resume_hint = sleep.resume_hint.clone();
        task.task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                task_id,
                crate::domain::TaskPhase::Waiting,
                TaskStatus::Waiting,
                Some(turn_id),
                None,
                format!(
                    "runtime sleep scheduled until {} ({})",
                    sleep.wake_at.to_rfc3339(),
                    sleep.reason.unwrap_or_else(|| "no reason provided".into())
                ),
            )
            .map_err(internal_store_error)?;
        self.record_event(
            session_id,
            turn_id,
            Some(task_id),
            EventType::TaskWaiting,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "wake_at": sleep.wake_at,
                "resume_hint": sleep.resume_hint,
            }),
        )?;
        Ok(())
    }
}

fn parse_sleep_directive(
    tool_call: &ToolCall,
    tool_result: &ToolResultItem,
) -> Result<Option<SleepDirective>, ApiError> {
    if tool_call.tool_name != "sleep" {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&tool_result.content).map_err(|error| {
        ApiError::new(
            ApiErrorCode::InternalError,
            format!("invalid sleep tool output: {error}"),
            false,
        )
    })?;
    let wake_at = value
        .get("wake_at")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ApiError::new(
                ApiErrorCode::InternalError,
                "sleep tool output missing wake_at",
                false,
            )
        })?;
    let wake_at = chrono::DateTime::parse_from_rfc3339(wake_at)
        .map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("invalid sleep wake_at: {error}"),
                false,
            )
        })?
        .with_timezone(&Utc);
    Ok(Some(SleepDirective {
        wake_at,
        reason: value
            .get("reason")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        resume_hint: value
            .get("resume_hint")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        duration_ms: value.get("duration_ms").and_then(|value| value.as_i64()),
    }))
}

pub(super) fn collect_tool_calls(
    output: &ModelTurnOutput,
    session_id: &str,
    task_id: &str,
    turn_id: &str,
    agent_id: &str,
) -> Vec<ToolCall> {
    output
        .items
        .iter()
        .filter_map(|item| match item {
            ModelOutputItem::ToolCall(tool_call) => Some(to_runtime_tool_call(
                tool_call, session_id, task_id, turn_id, agent_id,
            )),
            _ => None,
        })
        .collect()
}

fn to_runtime_tool_call(
    item: &ToolCallItem,
    session_id: &str,
    task_id: &str,
    turn_id: &str,
    agent_id: &str,
) -> ToolCall {
    ToolCall {
        tool_call_id: item.tool_call_id.clone(),
        tool_name: item.tool_name.clone(),
        args: item.args.clone(),
        requested_by: ToolCaller::Agent {
            agent_id: agent_id.into(),
        },
        session_id: Some(session_id.into()),
        task_id: Some(task_id.into()),
        turn_id: Some(turn_id.into()),
        idempotency_key: Some(format!("{turn_id}:{}", item.tool_call_id)),
        timeout_secs: item.timeout_secs,
    }
}

fn execute_tool_call_item(content: &str, tool_call: &ToolCall) -> ToolResultItem {
    ToolResultItem {
        item_id: format!("tool_result_{}", tool_call.tool_call_id),
        tool_call_id: tool_call.tool_call_id.clone(),
        tool_name: tool_call.tool_name.clone(),
        content: content.to_string(),
        metadata: json!({ "ok": true }),
        is_error: false,
    }
}

pub(super) fn chunk_stream_text(content: &str) -> Vec<String> {
    const CHUNK_CHARS: usize = 96;

    if content.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();

    for segment in content.split_inclusive('\n') {
        let mut buffer = String::new();
        let mut count = 0usize;

        for ch in segment.chars() {
            buffer.push(ch);
            count += 1;
            if ch == '\n' || count >= CHUNK_CHARS {
                chunks.push(std::mem::take(&mut buffer));
                count = 0;
            }
        }

        if !buffer.is_empty() {
            chunks.push(buffer);
        }
    }

    chunks
}

pub(super) fn build_log_stream_envelopes(stream_id: &str, lines: &[String]) -> Vec<ServerEnvelope> {
    let mut followups = vec![ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::Start,
        event: "logs.tail".into(),
        seq: 0,
        data: json!({ "source": "daemon.logs" }),
        meta: None,
    })];

    for (index, line) in lines.iter().enumerate() {
        followups.push(ServerEnvelope::Stream(StreamEnvelope {
            stream_id: stream_id.into(),
            phase: StreamPhase::Chunk,
            event: "log.line".into(),
            seq: index as u64 + 1,
            data: json!({ "line": line }),
            meta: None,
        }));
    }

    followups.push(ServerEnvelope::Stream(StreamEnvelope {
        stream_id: stream_id.into(),
        phase: StreamPhase::End,
        event: "logs.tail".into(),
        seq: lines.len() as u64 + 1,
        data: json!({ "done": true }),
        meta: None,
    }));
    followups
}
