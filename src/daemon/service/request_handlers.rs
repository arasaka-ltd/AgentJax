use super::*;

impl Daemon {
    pub(super) async fn route_request(
        &self,
        request: &RequestEnvelope,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        match request.method {
            ApiMethod::RuntimePing => Ok((
                self.serialize(RuntimePingResponse {
                    pong: true,
                    daemon_time: Utc::now(),
                })?,
                Vec::new(),
            )),
            ApiMethod::RuntimeStatus => Ok((self.serialize(self.runtime_status())?, Vec::new())),
            ApiMethod::RuntimeShutdown => self.handle_runtime_shutdown(request.parse_params()?),
            ApiMethod::ConfigInspect => self.handle_config_inspect(request.parse_params()?),
            ApiMethod::ConfigValidate => self.handle_config_validate(),
            ApiMethod::ConfigReload => self.handle_config_reload(),
            ApiMethod::PluginList => self.handle_plugin_list(),
            ApiMethod::PluginInspect => self.handle_plugin_inspect(request.parse_params()?),
            ApiMethod::PluginReload => self.handle_plugin_reload(request.parse_params()?),
            ApiMethod::PluginTest => self.handle_plugin_test(request.parse_params()?),
            ApiMethod::AgentList => self.handle_agent_list(),
            ApiMethod::AgentGet => self.handle_agent_get(request.parse_params()?),
            ApiMethod::SessionList => {
                let items = self
                    .store
                    .list_sessions()
                    .map_err(internal_store_error)?
                    .into_iter()
                    .map(|state| SessionListItem {
                        session_id: state.session.session_id,
                        agent_id: state.session.agent_id,
                        title: state.session.title,
                        status: state.session.status,
                        channel_id: state.session.channel_id,
                        surface_id: state.session.surface_id,
                        last_activity_at: Some(state.session.meta.updated_at),
                    })
                    .collect();
                Ok((self.serialize(SessionListResponse { items })?, Vec::new()))
            }
            ApiMethod::SessionGet => {
                let params: SessionGetRequest = request.parse_params()?;
                let state = self
                    .store
                    .get_session(&params.session_id)
                    .map_err(internal_store_error)?
                    .ok_or_else(session_not_found)?;
                Ok((
                    self.serialize(SessionGetResponse {
                        session: state.session,
                        messages: state.messages,
                        events: state.events,
                    })?,
                    Vec::new(),
                ))
            }
            ApiMethod::SessionModelInspect => {
                self.handle_session_model_inspect(request.parse_params()?)
            }
            ApiMethod::SessionModelSwitch => {
                self.handle_session_model_switch(request.parse_params()?)
            }
            ApiMethod::SessionCancel => self.handle_session_cancel(request.parse_params()?),
            ApiMethod::SessionSubscribe => self.handle_session_subscribe(request.parse_params()?),
            ApiMethod::SessionSend => self.handle_session_send(request.parse_params()?).await,
            ApiMethod::TaskList => self.handle_task_list(),
            ApiMethod::TaskGet => self.handle_task_get(request.parse_params()?),
            ApiMethod::TaskCancel => self.handle_task_cancel(request.parse_params()?),
            ApiMethod::TaskRetry => self.handle_task_retry(request.parse_params()?),
            ApiMethod::TaskSubscribe => self.handle_task_subscribe(request.parse_params()?),
            ApiMethod::NodeList => self.handle_node_list(),
            ApiMethod::NodeGet => self.handle_node_get(request.parse_params()?),
            ApiMethod::ScheduleList => self.handle_schedule_list(),
            ApiMethod::ScheduleCreate => self.handle_schedule_create(request.parse_params()?),
            ApiMethod::ScheduleUpdate => self.handle_schedule_update(request.parse_params()?),
            ApiMethod::ScheduleDelete => self.handle_schedule_delete(request.parse_params()?),
            ApiMethod::DoctorRun => self.handle_doctor_run(),
            ApiMethod::SmokeRun => self.handle_smoke_run(request.parse_params()?),
            ApiMethod::LogsTail => self.handle_logs_tail(request.parse_params()?),
            ApiMethod::MetricsSnapshot => self.handle_metrics_snapshot(),
            ApiMethod::SubscriptionCancel => {
                self.handle_subscription_cancel(request.parse_params()?)
            }
            ApiMethod::StreamCancel => self.handle_stream_cancel(request.parse_params()?),
        }
    }

    fn handle_runtime_shutdown(
        &self,
        params: RuntimeShutdownRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let _ = params;
        self.store.set_draining(true);
        self.push_log("runtime shutdown requested");
        Ok((
            self.serialize(RuntimeShutdownResponse {
                accepted: true,
                draining: self.store.draining(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_config_inspect(
        &self,
        params: ConfigInspectRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let config = match params.section.as_str() {
            "runtime" | "core" => {
                serde_json::to_value(&self.app.runtime_config).map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("config inspect serialization failed: {error}"),
                        false,
                    )
                })?
            }
            "workspace" => {
                serde_json::to_value(&self.app.workspace_runtime.workspace).map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("workspace inspect serialization failed: {error}"),
                        false,
                    )
                })?
            }
            "plugins" => serde_json::to_value(self.plugin_descriptors()).map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("plugin inspect serialization failed: {error}"),
                    false,
                )
            })?,
            "resources" => {
                serde_json::to_value(self.app.resource_registry.all()).map_err(|error| {
                    ApiError::new(
                        ApiErrorCode::InternalError,
                        format!("resource inspect serialization failed: {error}"),
                        false,
                    )
                })?
            }
            "usage" => serde_json::to_value(
                self.store
                    .list_usage_records()
                    .map_err(internal_store_error)?,
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("usage inspect serialization failed: {error}"),
                    false,
                )
            })?,
            "billing" => serde_json::to_value(
                self.store
                    .list_billing_records()
                    .map_err(internal_store_error)?,
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("billing inspect serialization failed: {error}"),
                    false,
                )
            })?,
            "schedules" => {
                serde_json::to_value(self.store.list_schedules().map_err(internal_store_error)?)
                    .map_err(|error| {
                        ApiError::new(
                            ApiErrorCode::InternalError,
                            format!("schedule inspect serialization failed: {error}"),
                            false,
                        )
                    })?
            }
            "nodes" => serde_json::to_value(self.node_registry().list()).map_err(|error| {
                ApiError::new(
                    ApiErrorCode::InternalError,
                    format!("node inspect serialization failed: {error}"),
                    false,
                )
            })?,
            other => {
                return Err(ApiError::new(
                    ApiErrorCode::InvalidRequest,
                    format!("unknown config section: {other}"),
                    false,
                ))
            }
        };
        Ok((
            self.serialize(ConfigInspectResponse {
                section: params.section,
                config,
            })?,
            Vec::new(),
        ))
    }

    fn handle_config_validate(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let report = crate::config::ConfigLoader::validate_at(
            &self.app.config_root.root,
            &self.app.runtime_config.runtime_paths.root,
            &self.app.runtime_config.workspace.paths.root,
        )
        .map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("config validation failed to execute: {error}"),
                false,
            )
        })?;
        Ok((
            self.serialize(ConfigValidateResponse {
                ok: report.ok,
                errors: report.errors,
                warnings: report.warnings,
                migrations: report
                    .migrations
                    .into_iter()
                    .map(|step| format!("{}: {}", step.file, step.summary))
                    .collect(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_config_reload(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        self.push_log("config reload requested");
        let candidate = crate::config::ConfigLoader::load_from_roots(
            &self.app.config_root.root,
            &self.app.runtime_config.runtime_paths.root,
            &self.app.runtime_config.workspace.paths.root,
        )
        .map(|loaded| loaded.config_snapshot)
        .map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("config reload failed to load candidate snapshot: {error}"),
                false,
            )
        })?;
        let diff = self.app.config_snapshot.diff(&candidate).map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("config reload diff failed: {error}"),
                false,
            )
        })?;
        Ok((
            self.serialize(ConfigReloadResponse {
                ok: !diff.reload_plan.restart_required,
                disposition: format!("{:?}", diff.reload_plan.disposition),
                reloaded_modules: diff.reload_plan.affected_modules,
                drained_modules: diff
                    .reload_plan
                    .drained_modules
                    .into_iter()
                    .map(|drain| drain.module)
                    .collect(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_plugin_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let items = self
            .plugin_descriptors()
            .into_iter()
            .map(|plugin| PluginListItem {
                id: plugin.plugin_id,
                enabled: matches!(
                    plugin.status,
                    PluginStatus::Loading
                        | PluginStatus::Loaded
                        | PluginStatus::Starting
                        | PluginStatus::Running
                ),
                healthy: !matches!(plugin.status, PluginStatus::Failed),
                capabilities: plugin.capabilities,
            })
            .collect();
        Ok((self.serialize(PluginListResponse { items })?, Vec::new()))
    }

    fn handle_plugin_inspect(
        &self,
        params: PluginInspectRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let snapshot = self
            .app
            .plugin_manager
            .snapshot(
                &params.plugin_id,
                &self.app.runtime_config.plugin_api_version,
            )
            .ok_or_else(plugin_not_found)?;
        Ok((
            self.serialize(PluginInspectResponse {
                plugin: snapshot.plugin,
                enabled: snapshot.enabled,
                default_enabled: snapshot.default_enabled,
                healthy: snapshot.healthy,
                dependencies: snapshot.dependencies,
                optional_dependencies: snapshot.optional_dependencies,
                required_permissions: snapshot.required_permissions,
                provided_resources: snapshot.provided_resources,
                config_ref: snapshot.config_ref,
                policy_flags: snapshot.policy_flags,
                reload_hint: snapshot.reload_hint,
                last_lifecycle_stage: snapshot.last_lifecycle_stage,
                last_error: snapshot.last_error,
            })?,
            Vec::new(),
        ))
    }

    fn handle_plugin_reload(
        &self,
        params: PluginReloadRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let report = self
            .app
            .plugin_manager
            .reload(
                &params.plugin_id,
                &self.app.plugin_registry,
                &self.app.resource_registry,
                &self.app.tool_registry,
                &self.app.event_bus,
                self.app.plugin_host.hooks(),
                &self.app.runtime_config,
                &self.app.workspace_host,
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::NotFound,
                    format!("plugin reload failed: {error}"),
                    false,
                )
            })?;
        self.push_log(format!("plugin reload requested: {}", params.plugin_id));
        Ok((
            self.serialize(PluginReloadResponse {
                ok: report.ok,
                plugin_id: report.plugin_id,
                status: format!("{:?}", report.status),
                lifecycle_stage: report.lifecycle_stage,
                summary: report.summary,
                checks: report.checks,
            })?,
            Vec::new(),
        ))
    }

    fn handle_plugin_test(
        &self,
        params: PluginTestRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let report = self
            .app
            .plugin_manager
            .test_plugin(
                &params.plugin_id,
                &self.app.runtime_config.plugin_api_version,
            )
            .map_err(|error| {
                ApiError::new(
                    ApiErrorCode::NotFound,
                    format!("plugin test failed: {error}"),
                    false,
                )
            })?;
        Ok((
            self.serialize(PluginTestResponse {
                ok: report.ok,
                plugin_id: report.plugin_id,
                status: format!("{:?}", report.status),
                lifecycle_stage: report.lifecycle_stage,
                summary: report.summary,
                checks: report.checks,
            })?,
            Vec::new(),
        ))
    }

    fn handle_agent_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let agent = self.default_agent_descriptor();
        Ok((
            self.serialize(AgentListResponse {
                items: vec![AgentListItem {
                    agent_id: agent.agent_id,
                    status: agent.status,
                    workspace_id: agent.workspace_id,
                }],
            })?,
            Vec::new(),
        ))
    }

    fn handle_agent_get(
        &self,
        params: AgentGetRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let agent = self.default_agent_descriptor();
        if params.agent_id != agent.agent_id {
            return Err(agent_not_found());
        }
        Ok((self.serialize(AgentGetResponse { agent })?, Vec::new()))
    }

    fn handle_task_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let items = self
            .store
            .list_tasks()
            .map_err(internal_store_error)?
            .into_iter()
            .map(|record| TaskListItem {
                task_id: record.task.task_id.clone(),
                kind: match record.task.execution_mode {
                    ExecutionMode::EphemeralSession => "ephemeral_session".into(),
                    ExecutionMode::BoundSession => "bound_session".into(),
                    ExecutionMode::HeadlessTask => "headless_task".into(),
                },
                status: record.task.status.clone(),
                agent_id: record.task.agent_id.clone(),
                session_id: record.task.session_id.clone(),
                created_at: record.task.meta.created_at,
            })
            .collect();
        Ok((self.serialize(TaskListResponse { items })?, Vec::new()))
    }

    fn handle_task_get(
        &self,
        params: TaskGetRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let record = self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        Ok((
            self.serialize(TaskGetResponse {
                task: record.task,
                timeline: record.timeline,
                checkpoints: record.checkpoints,
            })?,
            Vec::new(),
        ))
    }

    fn handle_task_cancel(
        &self,
        params: TaskCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut task = self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        task.task.status = TaskStatus::Cancelled;
        task.task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.task)
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &params.task_id,
                crate::domain::TaskPhase::Cancelled,
                TaskStatus::Cancelled,
                None,
                None,
                "task cancelled",
            )
            .map_err(internal_store_error)?;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_task_retry(
        &self,
        params: TaskRetryRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut task = self
            .store
            .get_task(&params.task_id)
            .map_err(internal_store_error)?
            .ok_or_else(task_not_found)?;
        task.task.status = TaskStatus::Ready;
        task.task.meta.updated_at = Utc::now();
        self.store
            .update_task(task.task)
            .map_err(internal_store_error)?;
        self.store
            .append_task_timeline(
                &params.task_id,
                crate::domain::TaskPhase::Ready,
                TaskStatus::Ready,
                None,
                None,
                "task retried and returned to ready",
            )
            .map_err(internal_store_error)?;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_node_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let nodes = self.node_registry().list();
        Ok((
            self.serialize(NodeListResponse {
                items: nodes
                    .into_iter()
                    .map(|node| NodeListItem {
                        node_id: node.node_id,
                        status: node.status,
                        capabilities: node.capabilities,
                    })
                    .collect(),
            })?,
            Vec::new(),
        ))
    }

    fn handle_node_get(
        &self,
        params: NodeGetRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let node = self
            .node_registry()
            .get(&params.node_id)
            .ok_or_else(node_not_found)?;
        Ok((self.serialize(NodeGetResponse { node })?, Vec::new()))
    }

    fn handle_schedule_list(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let items = self
            .store
            .list_schedules()
            .map_err(internal_store_error)?
            .into_iter()
            .map(|record| ScheduleListItem {
                schedule_id: record.schedule.schedule_id.clone(),
                kind: schedule_kind(&record.schedule),
                enabled: record.schedule.enabled,
                next_run_at: record.next_run_at,
            })
            .collect();
        Ok((self.serialize(ScheduleListResponse { items })?, Vec::new()))
    }

    fn handle_schedule_create(
        &self,
        params: ScheduleCreateRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let record = self
            .store
            .upsert_schedule(params.schedule.clone())
            .map_err(internal_store_error)?;
        self.push_log(format!(
            "schedule created: {} next_run_at={:?}",
            record.schedule.schedule_id, record.next_run_at
        ));
        Ok((
            self.serialize(ScheduleGetResponse {
                schedule: params.schedule,
            })?,
            Vec::new(),
        ))
    }

    fn handle_schedule_update(
        &self,
        params: ScheduleUpdateRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        if self
            .store
            .get_schedule(&params.schedule.schedule_id)
            .map_err(internal_store_error)?
            .is_none()
        {
            return Err(schedule_not_found());
        }
        let record = self
            .store
            .upsert_schedule(params.schedule.clone())
            .map_err(internal_store_error)?;
        self.push_log(format!(
            "schedule updated: {} next_run_at={:?}",
            record.schedule.schedule_id, record.next_run_at
        ));
        Ok((
            self.serialize(ScheduleGetResponse {
                schedule: params.schedule,
            })?,
            Vec::new(),
        ))
    }

    fn handle_schedule_delete(
        &self,
        params: ScheduleDeleteRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let removed = self
            .store
            .delete_schedule(&params.schedule_id)
            .map_err(internal_store_error)?;
        if removed.is_none() {
            return Err(schedule_not_found());
        }
        self.push_log(format!("schedule deleted: {}", params.schedule_id));
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_doctor_run(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        Ok((
            self.serialize(DoctorRunResponse {
                ok: true,
                checks: vec![
                    DoctorCheckResult {
                        id: "runtime_config".into(),
                        status: "ok".into(),
                    },
                    DoctorCheckResult {
                        id: "workspace_identity".into(),
                        status: "ok".into(),
                    },
                    DoctorCheckResult {
                        id: "provider_registry".into(),
                        status: "ok".into(),
                    },
                ],
            })?,
            Vec::new(),
        ))
    }

    fn handle_smoke_run(
        &self,
        params: SmokeRunRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        match params.target.as_str() {
            "daemon" | "plugins" | "plugin-manager" => {
                let snapshots = self
                    .app
                    .plugin_manager
                    .snapshots(&self.app.runtime_config.plugin_api_version);
                let plugin_count = snapshots.len();
                let enabled_count = snapshots.iter().filter(|plugin| plugin.enabled).count();
                let healthy_count = snapshots.iter().filter(|plugin| plugin.healthy).count();
                let mut checks = vec![
                    format!("plugins_discovered={plugin_count}"),
                    format!("plugins_enabled={enabled_count}"),
                    format!("plugins_healthy={healthy_count}"),
                ];

                if plugin_count == 0 {
                    checks.push("plugin_manager=empty".into());
                    return Ok((
                        self.serialize(SmokeRunResponse {
                            ok: false,
                            target: params.target,
                            summary: "plugin manager smoke failed: no plugins discovered".into(),
                            checks,
                        })?,
                        Vec::new(),
                    ));
                }

                for snapshot in &snapshots {
                    let report = self
                        .app
                        .plugin_manager
                        .test_plugin(
                            &snapshot.plugin.plugin_id,
                            &self.app.runtime_config.plugin_api_version,
                        )
                        .map_err(|error| {
                            ApiError::new(
                                ApiErrorCode::InternalError,
                                format!(
                                    "smoke run failed while validating plugin {}: {error}",
                                    snapshot.plugin.plugin_id
                                ),
                                false,
                            )
                        })?;
                    checks.push(format!(
                        "plugin={} status={:?} ok={}",
                        report.plugin_id, report.status, report.ok
                    ));
                }

                let failed_plugins: Vec<&str> = snapshots
                    .iter()
                    .filter(|snapshot| snapshot.enabled && !snapshot.healthy)
                    .map(|snapshot| snapshot.plugin.plugin_id.as_str())
                    .collect();
                if !failed_plugins.is_empty() {
                    checks.push(format!("failed_plugins={}", failed_plugins.join(",")));
                }

                let ok = !snapshots.is_empty() && failed_plugins.is_empty();
                let summary = if ok {
                    format!(
                        "plugin manager smoke passed: {healthy_count}/{plugin_count} plugins healthy"
                    )
                } else {
                    format!(
                        "plugin manager smoke failed: unhealthy plugins [{}]",
                        failed_plugins.join(", ")
                    )
                };

                Ok((
                    self.serialize(SmokeRunResponse {
                        ok,
                        target: params.target,
                        summary,
                        checks,
                    })?,
                    Vec::new(),
                ))
            }
            _ => Ok((
                self.serialize(SmokeRunResponse {
                    ok: false,
                    target: params.target.clone(),
                    summary: format!("unknown smoke target {}", params.target),
                    checks: vec!["supported_targets=daemon,plugins,plugin-manager".into()],
                })?,
                Vec::new(),
            )),
        }
    }

    fn handle_logs_tail(
        &self,
        params: LogsTailRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let logs = self.snapshot_logs();
        if !params.stream {
            return Ok((self.serialize(json!({ "lines": logs }))?, Vec::new()));
        }

        let stream_id = self.store.next_stream_id();
        self.register_stream(&stream_id, "logs.tail", StreamStatus::Active);
        let followups = build_log_stream_envelopes(&stream_id, &logs);
        self.update_stream_status(&stream_id, StreamStatus::Completed);
        Ok((
            self.serialize(json!({
                "accepted": true,
                "stream_id": stream_id,
            }))?,
            followups,
        ))
    }

    fn handle_metrics_snapshot(&self) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let task_count = self.store.list_tasks().map_err(internal_store_error)?.len();
        let schedule_count = self
            .store
            .list_schedules()
            .map_err(internal_store_error)?
            .len();
        let session_count = self
            .store
            .list_sessions()
            .map_err(internal_store_error)?
            .len();
        let node_count = self.node_registry().list().len();
        let usage_summary = self.store.usage_summary().map_err(internal_store_error)?;
        Ok((
            self.serialize(MetricsSnapshotResponse {
                counters: json!({
                    "sessions_total": session_count,
                    "tasks_total": task_count,
                    "schedules_total": schedule_count,
                    "nodes_total": node_count,
                    "plugins_total": self.app.plugin_manager.plugin_count(),
                    "usage_records_total": usage_summary.records_total,
                }),
                gauges: json!({
                    "runtime_ready": self.store.ready(),
                    "runtime_draining": self.store.draining(),
                    "usage_estimated_usd_total": usage_summary.estimated_usd_total,
                }),
                histograms: json!({
                    "context_events_published": self.app.event_bus.snapshot().len(),
                    "usage_input_tokens_total": usage_summary.input_tokens_total,
                    "usage_output_tokens_total": usage_summary.output_tokens_total,
                }),
            })?,
            Vec::new(),
        ))
    }

    fn handle_subscription_cancel(
        &self,
        params: SubscriptionCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let removed = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .subscriptions
            .remove(params.subscription_id.0.as_str());
        if removed.is_none() {
            return Err(subscription_not_found());
        }
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }

    fn handle_stream_cancel(
        &self,
        params: StreamCancelRequest,
    ) -> Result<(Value, Vec<ServerEnvelope>), ApiError> {
        let mut control = self.control.lock().expect("control plane lock poisoned");
        let stream = control
            .streams
            .get_mut(params.stream_id.0.as_str())
            .ok_or_else(stream_not_found)?;
        stream.status = StreamStatus::Cancelled;
        Ok((self.serialize(json!({ "accepted": true }))?, Vec::new()))
    }
}
