use super::*;

impl Daemon {
    pub(super) fn runtime_status(&self) -> RuntimeStatusResponse {
        RuntimeStatusResponse {
            status: "running".into(),
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            api_version: API_VERSION.into(),
            uptime_secs: self.store.uptime_secs(),
            ready: self.store.ready(),
            draining: self.store.draining(),
        }
    }

    pub(super) fn plugin_descriptors(&self) -> Vec<PluginDescriptor> {
        self.app
            .plugin_manager
            .descriptors(&self.app.runtime_config.plugin_api_version)
    }

    pub(super) fn default_agent_descriptor(&self) -> Agent {
        Agent {
            meta: ObjectMeta::new(
                self.app.runtime.default_agent().agent_id.clone(),
                &self.app.runtime_config.config_schema_version,
            ),
            agent_id: self.app.runtime.default_agent().agent_id.clone(),
            display_name: self.app.runtime.default_agent().agent_id.clone(),
            workspace_id: self.app.runtime_config.workspace.workspace_id.clone(),
            profile_ref: Some(self.app.workspace_identity.agent.path.display().to_string()),
            mission_ref: Some(
                self.app
                    .workspace_identity
                    .mission
                    .path
                    .display()
                    .to_string(),
            ),
            rules_ref: Some(self.app.workspace_identity.rules.path.display().to_string()),
            router_ref: Some(
                self.app
                    .workspace_identity
                    .router
                    .path
                    .display()
                    .to_string(),
            ),
            default_resource_bindings: self
                .app
                .resource_registry
                .all()
                .into_iter()
                .map(|resource| resource.resource_id)
                .collect(),
            autonomy_policy: AutonomyPolicy::default(),
            status: if self.store.draining() {
                AgentStatus::Draining
            } else {
                AgentStatus::Active
            },
        }
    }

    pub(super) fn default_node(&self) -> Node {
        Node {
            meta: ObjectMeta::new("node.local", &self.app.runtime_config.state_schema_version),
            node_id: "node.local".into(),
            kind: NodeKind::Static,
            platform: std::env::consts::OS.into(),
            status: if self.store.draining() {
                NodeStatus::Draining
            } else {
                NodeStatus::Active
            },
            capabilities: vec![
                "daemon.control_plane".into(),
                "session.interaction".into(),
                "tool.dispatch".into(),
            ],
            resources: self
                .app
                .resource_registry
                .all()
                .into_iter()
                .map(|resource| resource.resource_id.0)
                .collect(),
            trust_level: TrustLevel::High,
            labels: BTreeMap::from([("scope".into(), "local".into())]),
        }
    }

    pub(super) fn push_log(&self, line: impl Into<String>) {
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .logs
            .push(format!("{} {}", Utc::now().to_rfc3339(), line.into()));
    }

    pub(super) fn snapshot_logs(&self) -> Vec<String> {
        let mut logs = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .logs
            .clone();
        if logs.is_empty() {
            logs.push(format!("{} daemon online", Utc::now().to_rfc3339()));
        }
        logs
    }

    pub(super) fn register_stream(
        &self,
        stream_id: &str,
        source: &'static str,
        status: StreamStatus,
    ) {
        self.control
            .lock()
            .expect("control plane lock poisoned")
            .streams
            .insert(
                stream_id.to_string(),
                RegisteredStream {
                    status,
                    _source: source,
                },
            );
    }

    pub(super) fn update_stream_status(&self, stream_id: &str, status: StreamStatus) {
        if let Some(stream) = self
            .control
            .lock()
            .expect("control plane lock poisoned")
            .streams
            .get_mut(stream_id)
        {
            stream.status = status;
        }
    }

    pub(super) fn session_model_state(
        &self,
        session: &Session,
    ) -> Result<SessionModelState, ApiError> {
        Ok(SessionModelState {
            current: SessionModelTarget {
                provider_id: session
                    .resolved_provider_id(&self.app.runtime.default_agent().provider_id)
                    .to_string(),
                model_id: session
                    .resolved_model_id(&self.app.runtime.default_agent().model)
                    .to_string(),
            },
            pending: session.pending_model_switch.clone(),
            last_switched_at: session.last_model_switched_at,
        })
    }

    pub(super) fn serialize<T: Serialize>(&self, value: T) -> Result<Value, ApiError> {
        serde_json::to_value(value).map_err(|error| {
            ApiError::new(
                ApiErrorCode::InternalError,
                format!("failed to serialize response: {error}"),
                false,
            )
        })
    }

    pub(super) fn record_shell_tool_started(&self, tool_call: &ToolCall) -> Result<(), ApiError> {
        let Some(event) = shell_tool_started_event(tool_call) else {
            return Ok(());
        };
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
            tool_call.task_id.as_deref(),
            event.0,
            event.1,
        )
    }

    pub(super) fn record_shell_tool_result(
        &self,
        tool_call: &ToolCall,
        metadata: &Value,
    ) -> Result<(), ApiError> {
        for (event_type, payload) in shell_tool_result_events(tool_call, metadata) {
            self.record_event(
                tool_call.session_id.as_deref().unwrap_or("session.default"),
                tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
                tool_call.task_id.as_deref(),
                event_type,
                payload,
            )?;
        }
        Ok(())
    }

    pub(super) fn record_shell_tool_failure(
        &self,
        tool_call: &ToolCall,
        error: &str,
    ) -> Result<(), ApiError> {
        let Some((event_type, payload)) = shell_tool_failure_event(tool_call, error) else {
            return Ok(());
        };
        self.record_event(
            tool_call.session_id.as_deref().unwrap_or("session.default"),
            tool_call.turn_id.as_deref().unwrap_or("turn.unknown"),
            tool_call.task_id.as_deref(),
            event_type,
            payload,
        )
    }
}
