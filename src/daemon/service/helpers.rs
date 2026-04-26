use super::*;

pub(super) fn session_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::SessionNotFound, "session not found", false)
}

pub(super) fn task_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::TaskNotFound, "task not found", false)
}

pub(super) fn agent_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::AgentNotFound, "agent not found", false)
}

pub(super) fn plugin_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::PluginNotFound, "plugin not found", false)
}

pub(super) fn node_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::NodeNotFound, "node not found", false)
}

pub(super) fn schedule_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::ScheduleNotFound, "schedule not found", false)
}

pub(super) fn subscription_not_found() -> ApiError {
    ApiError::new(
        ApiErrorCode::SubscriptionNotFound,
        "subscription not found",
        false,
    )
}

pub(super) fn stream_not_found() -> ApiError {
    ApiError::new(ApiErrorCode::StreamNotFound, "stream not found", false)
}

pub(super) fn internal_store_error(error: anyhow::Error) -> ApiError {
    ApiError::new(
        ApiErrorCode::InternalError,
        format!("store operation failed: {error}"),
        false,
    )
}

pub(super) fn map_store_error(error: anyhow::Error) -> ApiError {
    if error.to_string().contains("session not found") {
        session_not_found()
    } else {
        internal_store_error(error)
    }
}

pub(super) fn build_context_prompt(
    app: &Application,
    assembled: &AssembledContext,
    conversation_messages: Vec<SessionMessage>,
    allow_tool_calls: bool,
) -> String {
    crate::context_engine::render_prompt_role_payload(crate::context_engine::PromptRenderRequest {
        prompt_documents: crate::context_engine::parse_workspace_prompt_documents(
            &app.workspace_identity,
        ),
        assembled_context: assembled.clone(),
        tools: app.tool_registry.descriptors(),
        conversation_messages,
        allow_tool_calls,
    })
    .full_xml
}

pub(super) fn build_context_prompt_request(
    app: &Application,
    assembled: &AssembledContext,
    conversation_messages: Vec<SessionMessage>,
    allow_tool_calls: bool,
    previous_response_id: Option<String>,
) -> crate::core::plugin::ProviderPromptRequest {
    let payload = crate::context_engine::render_prompt_role_payload(
        crate::context_engine::PromptRenderRequest {
            prompt_documents: crate::context_engine::parse_workspace_prompt_documents(
                &app.workspace_identity,
            ),
            assembled_context: assembled.clone(),
            tools: app.tool_registry.descriptors(),
            conversation_messages,
            allow_tool_calls,
        },
    );
    crate::core::plugin::ProviderPromptRequest {
        instructions: Some(payload.instructions_xml.clone()),
        messages: vec![
            crate::core::plugin::ProviderPromptMessage {
                role: "developer".into(),
                content: payload.instructions_xml,
            },
            crate::core::plugin::ProviderPromptMessage {
                role: "user".into(),
                content: payload.user_xml,
            },
        ],
        previous_response_id,
        text_format: None,
        response_format: None,
        store: None,
        prompt: payload.full_xml,
        tools: app.tool_registry.descriptors(),
    }
}

pub(super) fn build_tool_followup_prompt_request(
    app: &Application,
    assembled: &AssembledContext,
    conversation_messages: Vec<SessionMessage>,
    previous_response_id: Option<String>,
) -> crate::core::plugin::ProviderPromptRequest {
    build_context_prompt_request(
        app,
        assembled,
        conversation_messages,
        true,
        previous_response_id,
    )
}

pub(super) fn recent_prompt_messages(
    messages: &[SessionMessage],
    limit: usize,
) -> Vec<SessionMessage> {
    messages
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

pub(super) fn finalize_message(
    mut message: SessionMessage,
    session: &Session,
    message_id: String,
    annotations: Option<Vec<SessionMessageAnnotation>>,
) -> SessionMessage {
    if message.role.is_none() {
        message.role = Some(message.normalized_kind().as_role_str().into());
    }
    if message.meta.message_id.is_none() {
        message.meta.message_id = Some(message_id);
    }
    if message.meta.session_id.is_none() {
        message.meta.session_id = Some(session.session_id.clone());
    }
    if message.meta.channel.is_none() {
        message.meta.channel = session.channel_id.clone();
    }
    if message.meta.surface.is_none() {
        message.meta.surface = session.surface_id.clone();
    }
    if message.meta.actor_id.is_none() {
        message.meta.actor_id = session.user_id.clone();
    }
    if message.meta.timestamp.is_none() {
        message.meta.timestamp = Some(Utc::now());
    }
    if let Some(annotations) = annotations {
        message.annotations.extend(annotations);
    }
    message
}

pub(super) fn schedule_kind(schedule: &Schedule) -> String {
    match &schedule.trigger {
        crate::domain::TaskTrigger::Cron { .. } => "cron",
        crate::domain::TaskTrigger::Interval { .. } => "interval",
        crate::domain::TaskTrigger::Event { .. } => "event",
        crate::domain::TaskTrigger::Manual => "manual",
    }
    .into()
}
