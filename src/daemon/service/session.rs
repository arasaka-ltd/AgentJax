use super::*;

impl Daemon {
    pub(super) async fn handle_session_send_dispatch(&self, request: RequestEnvelope) -> Dispatch {
        let params: SessionSendRequest = match request.parse_params() {
            Ok(params) => params,
            Err(error) => {
                return Dispatch::single(ServerEnvelope::Response(ResponseEnvelope::err(
                    request.id, error,
                )))
            }
        };

        if params.stream {
            match self.handle_session_send_streaming(params).await {
                Ok((result, receiver)) => Dispatch {
                    response: ServerEnvelope::Response(ResponseEnvelope::ok(request.id, result)),
                    followups: Vec::new(),
                    live_stream: Some(receiver),
                },
                Err(error) => Dispatch::single(ServerEnvelope::Response(ResponseEnvelope::err(
                    request.id, error,
                ))),
            }
        } else {
            match self.handle_session_send(params).await {
                Ok((result, followups)) => Dispatch {
                    response: ServerEnvelope::Response(ResponseEnvelope::ok(request.id, result)),
                    followups,
                    live_stream: None,
                },
                Err(error) => Dispatch::single(ServerEnvelope::Response(ResponseEnvelope::err(
                    request.id, error,
                ))),
            }
        }
    }

    pub(super) async fn handle_session_send(
        &self,
        params: SessionSendRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let session_id = params.session_id.clone();
        let turn_id = self.store.next_turn_id();
        let stream_id = params.stream.then(|| self.store.next_stream_id());
        self.store
            .mark_turn_active(&session_id)
            .map_err(map_store_error)?;
        let result = self
            .handle_session_send_inner(params, turn_id, stream_id, None)
            .await;
        self.store.clear_turn_active(&session_id);
        result
    }

    async fn handle_session_send_streaming(
        &self,
        params: SessionSendRequest,
    ) -> Result<(Value, UnboundedReceiver<ServerEnvelope>), ApiError> {
        let session_id = params.session_id.clone();
        let turn_id = self.store.next_turn_id();
        let stream_id = Some(self.store.next_stream_id());
        self.store
            .mark_turn_active(&session_id)
            .map_err(map_store_error)?;

        let (tx, rx) = mpsc::unbounded_channel();
        let daemon = self.clone();
        let response = SessionSendResponse {
            accepted: true,
            turn_id: turn_id.clone(),
            stream_id: stream_id.clone().map(Into::into),
        };

        tokio::spawn(async move {
            let result = daemon
                .handle_session_send_inner(
                    params,
                    turn_id.clone(),
                    stream_id.clone(),
                    Some(tx.clone()),
                )
                .await;
            daemon.store.clear_turn_active(&session_id);
            if let Err(error) = result {
                let _ = tx.send(stream_error_envelope(
                    stream_id.as_deref().unwrap_or("stream.error"),
                    &turn_id,
                    &error.message,
                ));
                let _ = tx.send(stream_terminal_envelope(
                    stream_id.as_deref().unwrap_or("stream.error"),
                    &turn_id,
                    "failed",
                ));
            }
        });

        Ok((self.serialize(response)?, rx))
    }

    pub(super) fn handle_session_model_inspect(
        &self,
        params: SessionModelInspectRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let session = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        Ok((
            self.serialize(SessionModelInspectResponse {
                session_id: params.session_id,
                model: self.session_model_state(&session.session)?,
            })?,
            Vec::new(),
        ))
    }

    pub(super) fn handle_session_model_switch(
        &self,
        params: SessionModelSwitchRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut session = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?
            .session;
        let requested = SessionModelTarget {
            provider_id: params.provider_id,
            model_id: params.model_id,
        };

        self.record_event(
            &session.session_id,
            session
                .last_turn_id
                .as_deref()
                .unwrap_or("turn.model_switch"),
            None,
            EventType::ModelSwitchRequested,
            json!({
                "requested_target": requested.clone(),
            }),
        )?;

        if self.store.is_turn_active(&session.session_id) {
            self.record_event(
                &session.session_id,
                session
                    .last_turn_id
                    .as_deref()
                    .unwrap_or("turn.model_switch"),
                None,
                EventType::ModelSwitchRejected,
                json!({
                    "reason": "active turn in progress",
                    "requested_target": requested.clone(),
                }),
            )?;
            return Ok((
                self.serialize(SessionModelSwitchResponse {
                    session_id: session.session_id.clone(),
                    result: SessionModelSwitchResult::Rejected,
                    model: self.session_model_state(&session)?,
                    reason: Some("active turn in progress".into()),
                })?,
                Vec::new(),
            ));
        }

        session.pending_model_switch = Some(requested.clone());
        self.store
            .upsert_session(session.clone())
            .map_err(internal_store_error)?;

        if let Err(error) = self
            .app
            .runtime
            .validate_provider_model_binding(&requested.provider_id, &requested.model_id)
        {
            session.pending_model_switch = None;
            self.store
                .upsert_session(session.clone())
                .map_err(internal_store_error)?;
            self.record_event(
                &session.session_id,
                session
                    .last_turn_id
                    .as_deref()
                    .unwrap_or("turn.model_switch"),
                None,
                EventType::ModelSwitchRejected,
                json!({
                    "reason": error.to_string(),
                    "requested_target": requested.clone(),
                }),
            )?;
            return Ok((
                self.serialize(SessionModelSwitchResponse {
                    session_id: session.session_id.clone(),
                    result: SessionModelSwitchResult::Rejected,
                    model: self.session_model_state(&session)?,
                    reason: Some(error.to_string()),
                })?,
                Vec::new(),
            ));
        }

        session.current_provider_id = Some(requested.provider_id.clone());
        session.current_model_id = Some(requested.model_id.clone());
        session.pending_model_switch = None;
        session.last_model_switched_at = Some(Utc::now());
        session.meta.updated_at = Utc::now();
        self.store
            .upsert_session(session.clone())
            .map_err(internal_store_error)?;

        self.record_event(
            &session.session_id,
            session
                .last_turn_id
                .as_deref()
                .unwrap_or("turn.model_switch"),
            None,
            EventType::ModelSwitchApplied,
            json!({
                "current_target": {
                    "provider_id": session.current_provider_id.clone(),
                    "model_id": session.current_model_id.clone(),
                }
            }),
        )?;

        Ok((
            self.serialize(SessionModelSwitchResponse {
                session_id: session.session_id.clone(),
                result: SessionModelSwitchResult::Applied,
                model: self.session_model_state(&session)?,
                reason: None,
            })?,
            Vec::new(),
        ))
    }

    pub(super) fn handle_session_cancel(
        &self,
        params: SessionCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut record = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        record.session.status = crate::domain::SessionStatus::Closed;
        record.session.meta.updated_at = Utc::now();
        self.store
            .upsert_session(record.session)
            .map_err(internal_store_error)?;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    pub(super) fn handle_session_subscribe(
        &self,
        params: SessionSubscribeRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        if self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .is_none()
        {
            return Err(session_not_found());
        }
        let subscription_id = self.store.next_subscription_id();
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .subscriptions
            .insert(
                subscription_id.clone(),
                RegisteredSubscription {
                    _kind: "session",
                    _target_id: params.session_id,
                    _accepted_events: params.events.clone(),
                },
            );
        Ok((
            self.serialize(SubscriptionResponse {
                subscription_id: subscription_id.into(),
                accepted_events: params.events,
            })?,
            Vec::new(),
        ))
    }

    pub(super) fn handle_task_subscribe(
        &self,
        params: TaskSubscribeRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        if self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .is_none()
        {
            return Err(task_not_found());
        }
        let subscription_id = self.store.next_subscription_id();
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .subscriptions
            .insert(
                subscription_id.clone(),
                RegisteredSubscription {
                    _kind: "task",
                    _target_id: params.task_id,
                    _accepted_events: params.events.clone(),
                },
            );
        Ok((
            self.serialize(SubscriptionResponse {
                subscription_id: subscription_id.into(),
                accepted_events: params.events,
            })?,
            Vec::new(),
        ))
    }

    async fn handle_session_send_inner(
        &self,
        params: SessionSendRequest,
        turn_id: String,
        stream_id: Option<String>,
        live_stream: Option<UnboundedSender<ServerEnvelope>>,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let session = self
            .store
            .get_session(&params.session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        let session_agent = self
            .app
            .runtime
            .session_agent(
                session.session.current_provider_id.as_deref(),
                session.session.current_model_id.as_deref(),
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("failed to resolve session model: {error}"),
                    false,
                )
            })?;
        let user_message = finalize_message(
            params.message,
            &session.session,
            self.store.next_message_id(),
            None,
        );
        let task_id = self.store.next_task_id();
        let mut task = Task {
            meta: ObjectMeta::new(
                task_id.clone(),
                &self.app.runtime_config.state_schema_version,
            ),
            task_id: task_id.clone(),
            workspace_id: session.session.workspace_id.clone(),
            agent_id: Some(session_agent.agent_id.clone()),
            session_id: Some(params.session_id.clone()),
            parent_task_id: None,
            definition_ref: None,
            execution_mode: ExecutionMode::BoundSession,
            status: TaskStatus::Running,
            priority: crate::domain::TaskPriority::Normal,
            goal: user_message.content.clone(),
            checkpoint_ref: None,
            waiting_until: None,
            waiting_reason: None,
            waiting_resume_hint: None,
        };
        self.store
            .create_task(task.clone())
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskStarted,
            json!({
                "task_id": task_id,
                "execution_mode": "bound_session",
                "goal": task.goal,
            }),
        )?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Running,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                "session.send accepted and task started",
            )
            .map_err(internal_store_error)?;

        self.store
            .append_message(&params.session_id, &turn_id, user_message.clone())
            .map_err(map_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::MessageReceived,
            json!({ "message": user_message }),
        )?;
        let assembled_context = self
            .app
            .context_engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some(params.session_id.clone()),
                task_id: Some(task_id.clone()),
                budget_tokens: 8_000,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
                retrieval_scope: crate::builtin::context::retrieval_types::RetrievalScope::Implicit,
            })
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("context assembly failed: {error}"),
                    false,
                )
            })?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::ContextBuilt,
            json!({
                "block_count": assembled_context.blocks.len(),
                "included_refs": assembled_context.included_refs,
                "token_breakdown": assembled_context.token_breakdown,
            }),
        )?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TurnStarted,
            json!({ "turn_id": turn_id }),
        )?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Running,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                "turn started",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::ModelCalled,
            json!({
                "provider_id": session_agent.provider_id.clone(),
                "model_id": session_agent.model.clone(),
            }),
        )?;
        let prompt_messages = recent_prompt_messages(
            &self
                .store
                .get_session(&params.session_id)
                .map_err(internal_store_error)?
                .ok_or_else(session_not_found)?
                .messages,
            8,
        );
        if let Some(stream_id) = stream_id.as_deref() {
            self.register_stream(stream_id, "session.send", StreamStatus::Active);
        }
        let mut semantic_stream = stream_id
            .clone()
            .map(|stream_id| SemanticStreamBuilder::new(stream_id, &turn_id, live_stream.clone()));

        let loop_outcome = self
            .run_tool_loop(
                &params.session_id,
                &task_id,
                &turn_id,
                &session_agent,
                &session.session,
                &assembled_context,
                prompt_messages,
                semantic_stream.as_mut(),
            )
            .await
            .map_err(|error| {
                let error_message = error.message.clone();
                let _ = self.record_event(
                    &params.session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnFailed,
                    json!({ "error": error_message }),
                );
                task.status = TaskStatus::Failed;
                task.meta.updated_at = Utc::now();
                let _ = self.store.update_task(task.clone());
                let _ = self.store.append_task_timeline(
                    &task_id,
                    crate::domain::TaskPhase::Failed,
                    TaskStatus::Failed,
                    Some(&turn_id),
                    None,
                    format!("tool loop failed: {}", error.message),
                );
                let _ = self.record_event(
                    &params.session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TaskFailed,
                    json!({
                        "task_id": task_id,
                        "error": error.message.clone(),
                    }),
                );
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("session.send failed: {}", error.message),
                    false,
                )
            })?;

        let assistant_text = match loop_outcome {
            ToolLoopOutcome::Final(text) => text,
            ToolLoopOutcome::Sleeping(runtime_message) => {
                let mut runtime_message = finalize_message(
                    SessionMessage::runtime(runtime_message.clone()),
                    &session.session,
                    self.store.next_message_id(),
                    Some(vec![SessionMessageAnnotation {
                        kind: "runtime_control".into(),
                        value: "sleep".into(),
                    }]),
                );
                runtime_message.meta.actor_id = Some(session_agent.agent_id.clone());
                self.store
                    .append_message(&params.session_id, &turn_id, runtime_message.clone())
                    .map_err(map_store_error)?;
                self.record_event(
                    &params.session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnSucceeded,
                    json!({
                        "turn_id": turn_id,
                        "task_id": task_id,
                        "runtime_message": runtime_message,
                        "waiting": true,
                    }),
                )?;

                if let Some(stream) = semantic_stream.as_mut() {
                    stream.push(
                        "runtime.waiting",
                        json!({
                            "turn_id": turn_id,
                            "message": runtime_message.content,
                        }),
                    );
                }
                let followups = semantic_stream
                    .take()
                    .map(|stream| stream.finish(&turn_id, "waiting"))
                    .unwrap_or_default();
                if let Some(stream_id) = stream_id.as_deref() {
                    self.update_stream_status(stream_id, StreamStatus::Completed);
                }
                let stream_id = if followups.is_empty() {
                    None
                } else {
                    Some(
                        followups[0]
                            .stream_id()
                            .expect("stream envelope id exists")
                            .into(),
                    )
                };
                return Ok((
                    self.serialize(SessionSendResponse {
                        accepted: true,
                        turn_id,
                        stream_id,
                    })?,
                    followups,
                ));
            }
        };

        let mut assistant_message = finalize_message(
            SessionMessage::assistant(assistant_text),
            &session.session,
            self.store.next_message_id(),
            None,
        );
        assistant_message.meta.actor_id = Some(session_agent.agent_id.clone());
        self.store
            .append_message(&params.session_id, &turn_id, assistant_message.clone())
            .map_err(map_store_error)?;
        let checkpoint = self
            .app
            .context_engine
            .build_resume_pack(Some(&params.session_id), Some(&task_id))
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("resume pack build failed: {error}"),
                    false,
                )
            })?;
        self.store
            .write_task_checkpoint(
                &task_id,
                Some(&params.session_id),
                Some(&turn_id),
                assistant_message.content.clone(),
                checkpoint,
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskCheckpointed,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
            }),
        )?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Checkpointed,
                TaskStatus::Succeeded,
                Some(&turn_id),
                None,
                "checkpoint recorded from latest assistant output",
            )
            .map_err(internal_store_error)?;
        task.status = TaskStatus::Succeeded;
        task.checkpoint_ref = self
            .store
            .get_task(&task_id)
            .map_err(internal_store_error)?
            .and_then(|record| record.task.checkpoint_ref);
        task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Succeeded,
                TaskStatus::Succeeded,
                Some(&turn_id),
                None,
                "turn completed and task succeeded",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskSucceeded,
            json!({
                "task_id": task_id,
                "checkpoint_ref": task.checkpoint_ref,
            }),
        )?;
        self.record_event(
            &params.session_id,
            &turn_id,
            Some(&task_id),
            EventType::TurnSucceeded,
            json!({
                "turn_id": turn_id,
                "task_id": task_id,
                "assistant_message": assistant_message,
            }),
        )?;

        if let Some(stream) = semantic_stream.as_mut() {
            stream.push(
                "assistant.completed",
                json!({
                    "turn_id": turn_id,
                    "message_id": assistant_message.meta.message_id,
                }),
            );
        }
        let followups = semantic_stream
            .take()
            .map(|stream| stream.finish(&turn_id, "completed"))
            .unwrap_or_default();
        if let Some(stream_id) = stream_id.as_deref() {
            self.update_stream_status(stream_id, StreamStatus::Completed);
        }
        let stream_id = if followups.is_empty() {
            None
        } else {
            Some(
                followups[0]
                    .stream_id()
                    .expect("stream envelope id exists")
                    .into(),
            )
        };

        Ok((
            self.serialize(SessionSendResponse {
                accepted: true,
                turn_id,
                stream_id,
            })?,
            followups,
        ))
    }

    pub(super) fn record_event(
        &self,
        session_id: &str,
        turn_id: &str,
        task_id: Option<&str>,
        event_type: EventType,
        payload: Value,
    ) -> Result<(), ApiError> {
        let event = self
            .store
            .record_event(
                session_id,
                turn_id,
                task_id,
                &self.store.next_event_id(),
                event_type,
                payload,
            )
            .map_err(map_store_error)?;
        self.app
            .context_engine
            .append_event(event)
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("context engine append failed: {error}"),
                    false,
                )
            })
    }

    pub(super) async fn record_usage_and_billing(
        &self,
        session_id: &str,
        task_id: &str,
        turn_id: &str,
        session_agent: &crate::config::AgentDefinition,
        usage: &crate::domain::ModelUsage,
        message_count: u32,
        started_at: chrono::DateTime<chrono::Utc>,
        latency_ms: u64,
    ) -> Result<(), ApiError> {
        let record = crate::domain::UsageRecord {
            usage_id: self.store.next_usage_id(),
            category: usage_category_for_model(&session_agent.model),
            provider_id: Some(session_agent.provider_id.clone()),
            model_id: Some(session_agent.model.clone()),
            resource_id: format!("provider:{}:model:text", session_agent.provider_id),
            endpoint_id: Some("model.turn".into()),
            region: None,
            account_id: None,
            project_id: None,
            workspace_id: Some(self.app.runtime_config.workspace.workspace_id.clone()),
            agent_id: Some(session_agent.agent_id.clone()),
            session_id: Some(session_id.into()),
            task_id: Some(task_id.into()),
            plugin_id: None,
            request_count: 1,
            response_count: 1,
            message_count,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_tokens: None,
            reasoning_tokens: None,
            audio_seconds: None,
            image_count: None,
            video_count: None,
            tool_call_count: None,
            context_window_used: Some(
                usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0),
            ),
            max_context_tier_crossed: None,
            started_at,
            ended_at: chrono::Utc::now(),
            latency_ms,
            retry_count: 0,
        };
        let stored = self
            .store
            .append_usage_record(record.clone())
            .map_err(internal_store_error)?;
        self.record_event(
            session_id,
            turn_id,
            Some(task_id),
            EventType::UsageRecorded,
            json!({
                "usage_id": stored.usage_id,
                "provider_id": stored.provider_id,
                "model_id": stored.model_id,
                "input_tokens": stored.input_tokens,
                "output_tokens": stored.output_tokens,
                "latency_ms": stored.latency_ms,
            }),
        )?;

        for plugin in self.app.plugin_registry.billing_plugins() {
            let Some(mut billing) = plugin.estimate_billing(&stored).await.map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("billing estimation failed: {error}"),
                    false,
                )
            })?
            else {
                continue;
            };
            billing.billing_id = self.store.next_billing_id();
            billing.usage_id = stored.usage_id.clone();
            let billing = self
                .store
                .append_billing_record(billing)
                .map_err(internal_store_error)?;
            self.record_event(
                session_id,
                turn_id,
                Some(task_id),
                EventType::BillingRecorded,
                json!({
                    "billing_id": billing.billing_id,
                    "usage_id": billing.usage_id,
                    "amount": billing.amount,
                    "currency": billing.currency,
                    "mode": billing.mode,
                    "rule_id": billing.rule_id,
                    "confidence": billing.confidence,
                }),
            )?;
        }

        Ok(())
    }

    pub(super) async fn execute_tool_call(&self, tool_call: &ToolCall) -> Result<String, ApiError> {
        self.record_shell_tool_started(tool_call)?;
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
            tool_call.task_id.as_deref(),
            EventType::ToolCalled,
            json!({
                "tool_call_id": tool_call.tool_call_id,
                "tool_name": tool_call.tool_name,
                "args": tool_call.args,
                "idempotency_key": tool_call.idempotency_key,
                "timeout_secs": tool_call.timeout_secs,
            }),
        )?;

        let tool = self
            .app
            .tool_registry
            .get(&tool_call.tool_name)
            .ok_or_else(|| ApiError::new(ApiErrorCode::NotFound, "tool not found", false))?;

        match tool.invoke(tool_call).await {
            Ok(output) => {
                self.record_shell_tool_result(tool_call, &output.metadata)?;
                self.record_event(
                    tool_call.session_id.as_deref().unwrap_or("session.default"),
                    tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
                    tool_call.task_id.as_deref(),
                    EventType::ToolCompleted,
                    json!({
                        "tool_call_id": tool_call.tool_call_id,
                        "tool_name": tool_call.tool_name,
                        "output": output.content,
                        "metadata": output.metadata,
                    }),
                )?;
                Ok(output.content)
            }
            Err(error) => {
                self.record_shell_tool_failure(tool_call, &error.to_string())?;
                self.record_event(
                    tool_call.session_id.as_deref().unwrap_or("session.default"),
                    tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
                    tool_call.task_id.as_deref(),
                    EventType::ToolFailed,
                    json!({
                        "tool_call_id": tool_call.tool_call_id,
                        "tool_name": tool_call.tool_name,
                        "error": error.to_string(),
                    }),
                )?;
                Err(ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("tool execution failed: {error}"),
                    false,
                ))
            }
        }
    }

    pub(super) fn spawn_waiting_task_scheduler(&self) {
        let daemon = self.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                loop {
                    daemon.resume_ready_waiting_tasks().await;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            });
        }
    }

    pub(super) fn spawn_schedule_executor(&self) {
        let daemon = self.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                loop {
                    daemon.run_due_schedules().await;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            });
        }
    }

    async fn run_due_schedules(&self) {
        let now = Utc::now();
        let due = match self.store.due_schedules(now) {
            Ok(records) => records,
            Err(error) => {
                self.push_log(format!("schedule scan failed: {error}"));
                return;
            }
        };

        for record in due {
            if let Err(error) = self.execute_schedule(record, now).await {
                self.push_log(format!("schedule execution failed: {}", error.message));
            }
        }
    }

    async fn execute_schedule(
        &self,
        record: crate::daemon::schedule_store::StoredScheduleRecord,
        triggered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ApiError> {
        let schedule = record.schedule.clone();
        let turn_id = self.store.next_turn_id();
        let task_id = self.store.next_task_id();
        let selected_node = self
            .node_registry()
            .select(&NodeSelector {
                required_capabilities: vec!["scheduler.tick".into()],
                preferred_labels: std::collections::BTreeMap::from([(
                    "scope".into(),
                    "local".into(),
                )]),
                min_trust_level: Some(TrustLevel::High),
            })
            .into_iter()
            .next()
            .or_else(|| self.node_registry().list().into_iter().next());

        let updated = self
            .store
            .mark_schedule_triggered(&schedule.schedule_id, Some(&task_id), triggered_at)
            .map_err(internal_store_error)?;
        let mut task = Task {
            meta: ObjectMeta::new(
                task_id.clone(),
                &self.app.runtime_config.state_schema_version,
            ),
            task_id: task_id.clone(),
            workspace_id: self.app.runtime_config.workspace.workspace_id.clone(),
            agent_id: Some(self.app.runtime.default_agent().agent_id.clone()),
            session_id: None,
            parent_task_id: None,
            definition_ref: Some(schedule_target_ref(&schedule.target)),
            execution_mode: ExecutionMode::HeadlessTask,
            status: TaskStatus::Running,
            priority: crate::domain::TaskPriority::Normal,
            goal: schedule_goal(&schedule),
            checkpoint_ref: None,
            waiting_until: None,
            waiting_reason: None,
            waiting_resume_hint: None,
        };
        self.store
            .create_task(task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Scheduled,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                format!("schedule {} triggered", schedule.schedule_id),
            )
            .map_err(internal_store_error)?;
        self.record_event(
            "session.default",
            &turn_id,
            Some(&task_id),
            EventType::ScheduleTriggered,
            json!({
                "schedule_id": schedule.schedule_id,
                "schedule_name": schedule.name,
                "trigger": schedule.trigger,
                "next_run_at": updated.next_run_at,
                "selected_node_id": selected_node.as_ref().map(|node| node.node_id.clone()),
            }),
        )?;
        self.record_event(
            "session.default",
            &turn_id,
            Some(&task_id),
            EventType::TaskStarted,
            json!({
                "task_id": task_id,
                "execution_mode": "headless_task",
                "goal": task.goal,
                "definition_ref": task.definition_ref,
            }),
        )?;
        task.status = TaskStatus::Succeeded;
        task.meta.updated_at = Utc::now();
        self.store.update_task(task).map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Succeeded,
                TaskStatus::Succeeded,
                Some(&turn_id),
                None,
                "scheduled task executed by local scheduler",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            "session.default",
            &turn_id,
            Some(&task_id),
            EventType::TaskSucceeded,
            json!({
                "task_id": task_id,
                "schedule_id": schedule.schedule_id,
                "selected_node_id": selected_node.as_ref().map(|node| node.node_id.clone()),
            }),
        )?;
        self.push_log(format!(
            "schedule {} executed as headless task {}",
            schedule.schedule_id, task_id
        ));
        Ok(())
    }

    async fn resume_ready_waiting_tasks(&self) {
        let now = Utc::now();
        let records = match self.store.list_tasks() {
            Ok(records) => records,
            Err(error) => {
                self.push_log(format!("waiting task scan failed: {error}"));
                return;
            }
        };

        for record in records {
            if record.task.status != TaskStatus::Waiting {
                continue;
            }
            let Some(waiting_until) = record.task.waiting_until else {
                continue;
            };
            if waiting_until > now {
                continue;
            }
            let task_id = record.task.task_id.clone();
            {
                let mut control = self.control.lock().expect("control plane lock poisoned");
                if !control.resuming_tasks.insert(task_id.clone()) {
                    continue;
                }
            }
            let daemon = self.clone();
            tokio::spawn(async move {
                let _ = daemon.resume_waiting_task(task_id.clone()).await;
                daemon
                    .control
                    .lock()
                    .expect("control plane lock poisoned")
                    .resuming_tasks
                    .remove(&task_id);
            });
        }
    }

    async fn resume_waiting_task(&self, task_id: String) -> Result<(), ApiError> {
        let record = self
            .store
            .get_task(&task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        if record.task.status != TaskStatus::Waiting {
            return Ok(());
        }
        let session_id = record.task.session_id.clone().ok_or_else(|| {
            ApiError::new(
                ApiErrorCode::InternalError,
                "waiting task missing session_id",
                false,
            )
        })?;
        let session_record = self
            .store
            .get_session(&session_id)
            .map_err(internal_store_error)?
            .ok_or_else(session_not_found)?;
        let session_agent = self
            .app
            .runtime
            .session_agent(
                session_record.session.current_provider_id.as_deref(),
                session_record.session.current_model_id.as_deref(),
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("failed to resolve session model: {error}"),
                    false,
                )
            })?;
        let turn_id = self.store.next_turn_id();

        let mut task = record.task.clone();
        let resume_reason = task.waiting_reason.clone();
        let resume_hint = task.waiting_resume_hint.clone();
        task.status = TaskStatus::Running;
        task.waiting_until = None;
        task.waiting_reason = None;
        task.waiting_resume_hint = None;
        task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.clone())
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &task_id,
                crate::domain::TaskPhase::Running,
                TaskStatus::Running,
                Some(&turn_id),
                None,
                "waiting task resumed by runtime scheduler",
            )
            .map_err(internal_store_error)?;
        self.record_event(
            &session_id,
            &turn_id,
            Some(&task_id),
            EventType::TaskResumed,
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "reason": resume_reason,
                "resume_hint": resume_hint,
            }),
        )?;

        let mut resume_message = finalize_message(
            SessionMessage::runtime(format!(
                "Resume the waiting task now. {} {}",
                resume_reason
                    .as_deref()
                    .map(|value| format!("Reason: {value}."))
                    .unwrap_or_default(),
                resume_hint
                    .as_deref()
                    .map(|value| format!("Next step hint: {value}."))
                    .unwrap_or_default()
            )),
            &session_record.session,
            self.store.next_message_id(),
            Some(vec![SessionMessageAnnotation {
                kind: "runtime_control".into(),
                value: "task_resumed".into(),
            }]),
        );
        resume_message.meta.actor_id = Some(session_agent.agent_id.clone());
        self.store
            .append_message(&session_id, &turn_id, resume_message.clone())
            .map_err(map_store_error)?;

        let assembled_context = self
            .app
            .context_engine
            .assemble_context(ContextAssemblyRequest {
                session_id: Some(session_id.clone()),
                task_id: Some(task_id.clone()),
                budget_tokens: 8_000,
                purpose: ContextAssemblyPurpose::Chat,
                model_profile: None,
                retrieval_scope: crate::builtin::context::retrieval_types::RetrievalScope::Implicit,
            })
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("context assembly failed during resume: {error}"),
                    false,
                )
            })?;
        let prompt_messages = recent_prompt_messages(
            &self
                .store
                .get_session(&session_id)
                .map_err(internal_store_error)?
                .ok_or_else(session_not_found)?
                .messages,
            8,
        );
        let outcome = self
            .run_tool_loop(
                &session_id,
                &task_id,
                &turn_id,
                &session_agent,
                &session_record.session,
                &assembled_context,
                prompt_messages,
                None,
            )
            .await?;

        match outcome {
            ToolLoopOutcome::Final(text) => {
                let mut assistant_message = finalize_message(
                    SessionMessage::assistant(text),
                    &session_record.session,
                    self.store.next_message_id(),
                    None,
                );
                assistant_message.meta.actor_id = Some(session_agent.agent_id.clone());
                self.store
                    .append_message(&session_id, &turn_id, assistant_message.clone())
                    .map_err(map_store_error)?;
                let checkpoint = self
                    .app
                    .context_engine
                    .build_resume_pack(Some(&session_id), Some(&task_id))
                    .map_err(|error| {
                        ApiError::new(
                            ApiErrorCode::InternalError,
                            format!("resume pack build failed during resume completion: {error}"),
                            false,
                        )
                    })?;
                self.store
                    .write_task_checkpoint(
                        &task_id,
                        Some(&session_id),
                        Some(&turn_id),
                        assistant_message.content.clone(),
                        checkpoint,
                    )
                    .map_err(internal_store_error)?;
                task.status = TaskStatus::Succeeded;
                task.checkpoint_ref = self
                    .store
                    .get_task(&task_id)
                    .map_err(internal_store_error)?
                    .and_then(|record| record.task.checkpoint_ref);
                task.meta.updated_at = Utc::now();
                self.store
                    .update_task(task.clone())
                    .map_err(internal_store_error)?;
                self.store
                    .append_task_timeline(
                        &task_id,
                        crate::domain::TaskPhase::Succeeded,
                        TaskStatus::Succeeded,
                        Some(&turn_id),
                        None,
                        "resumed task completed successfully",
                    )
                    .map_err(internal_store_error)?;
                self.record_event(
                    &session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TaskSucceeded,
                    json!({
                        "task_id": task_id,
                        "checkpoint_ref": task.checkpoint_ref,
                        "resumed": true,
                    }),
                )?;
                self.record_event(
                    &session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnSucceeded,
                    json!({
                        "turn_id": turn_id,
                        "task_id": task_id,
                        "assistant_message": assistant_message,
                        "resumed": true,
                    }),
                )?;
            }
            ToolLoopOutcome::Sleeping(runtime_message) => {
                let mut runtime_message = finalize_message(
                    SessionMessage::runtime(runtime_message),
                    &session_record.session,
                    self.store.next_message_id(),
                    Some(vec![SessionMessageAnnotation {
                        kind: "runtime_control".into(),
                        value: "sleep".into(),
                    }]),
                );
                runtime_message.meta.actor_id = Some(session_agent.agent_id.clone());
                self.store
                    .append_message(&session_id, &turn_id, runtime_message.clone())
                    .map_err(map_store_error)?;
                self.record_event(
                    &session_id,
                    &turn_id,
                    Some(&task_id),
                    EventType::TurnSucceeded,
                    json!({
                        "turn_id": turn_id,
                        "task_id": task_id,
                        "runtime_message": runtime_message,
                        "waiting": true,
                        "resumed": true,
                    }),
                )?;
            }
        }

        Ok(())
    }
}

fn usage_category_for_model(model_id: &str) -> crate::domain::UsageCategory {
    if model_id.to_ascii_lowercase().starts_with('o') {
        crate::domain::UsageCategory::ModelReasoning
    } else {
        crate::domain::UsageCategory::ModelText
    }
}

fn schedule_target_ref(target: &crate::domain::TaskTarget) -> String {
    match target {
        crate::domain::TaskTarget::TaskRef { definition_ref } => definition_ref.clone(),
        crate::domain::TaskTarget::SkillRef { skill_id } => format!("skill:{skill_id}"),
        crate::domain::TaskTarget::WorkflowRef { workflow_id } => {
            format!("workflow:{workflow_id}")
        }
    }
}

fn schedule_goal(schedule: &Schedule) -> String {
    match &schedule.target {
        crate::domain::TaskTarget::TaskRef { definition_ref } => {
            format!("Execute scheduled task definition {definition_ref}")
        }
        crate::domain::TaskTarget::SkillRef { skill_id } => {
            format!("Execute scheduled skill {skill_id}")
        }
        crate::domain::TaskTarget::WorkflowRef { workflow_id } => {
            format!("Execute scheduled workflow {workflow_id}")
        }
    }
}
