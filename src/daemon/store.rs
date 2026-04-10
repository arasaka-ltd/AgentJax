use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::Instant,
};

use chrono::Utc;
use serde_json::Value;

use crate::{
    api::SessionMessage,
    config::RuntimeConfig,
    domain::{
        EventSource, EventType, ObjectMeta, RuntimeEvent, Session, SessionMode, SessionStatus,
    },
};

pub struct DaemonStore {
    runtime_config: RuntimeConfig,
    started_at: Instant,
    ready: AtomicBool,
    draining: AtomicBool,
    next_connection: AtomicU64,
    next_message: AtomicU64,
    next_turn: AtomicU64,
    next_event: AtomicU64,
    next_stream: AtomicU64,
    sessions: Mutex<BTreeMap<String, SessionState>>,
}

#[derive(Clone)]
pub struct SessionState {
    pub session: Session,
    pub messages: Vec<SessionMessage>,
    pub events: Vec<RuntimeEvent>,
}

impl DaemonStore {
    pub fn new(runtime_config: RuntimeConfig) -> Self {
        let default_session = default_session(&runtime_config);
        let mut sessions = BTreeMap::new();
        sessions.insert(
            default_session.session_id.clone(),
            SessionState {
                session: default_session,
                messages: Vec::new(),
                events: Vec::new(),
            },
        );

        Self {
            runtime_config,
            started_at: Instant::now(),
            ready: AtomicBool::new(true),
            draining: AtomicBool::new(false),
            next_connection: AtomicU64::new(1),
            next_message: AtomicU64::new(1),
            next_turn: AtomicU64::new(1),
            next_event: AtomicU64::new(1),
            next_stream: AtomicU64::new(1),
            sessions: Mutex::new(sessions),
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    pub fn draining(&self) -> bool {
        self.draining.load(Ordering::Relaxed)
    }

    pub fn next_connection_id(&self) -> String {
        let id = self.next_connection.fetch_add(1, Ordering::Relaxed);
        format!("conn_{id}")
    }

    pub fn next_turn_id(&self) -> String {
        let id = self.next_turn.fetch_add(1, Ordering::Relaxed);
        format!("turn_{id}")
    }

    pub fn next_message_id(&self) -> String {
        let id = self.next_message.fetch_add(1, Ordering::Relaxed);
        format!("msg_{id}")
    }

    pub fn next_event_id(&self) -> String {
        let id = self.next_event.fetch_add(1, Ordering::Relaxed);
        format!("evt_{id}")
    }

    pub fn next_stream_id(&self) -> String {
        let id = self.next_stream.fetch_add(1, Ordering::Relaxed);
        format!("str_{id}")
    }

    pub fn list_sessions(&self) -> Vec<SessionState> {
        self.sessions
            .lock()
            .expect("sessions lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn get_session(&self, session_id: &str) -> Option<SessionState> {
        self.sessions
            .lock()
            .expect("sessions lock poisoned")
            .get(session_id)
            .cloned()
    }

    pub fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        message: SessionMessage,
    ) -> Option<Session> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        let state = sessions.get_mut(session_id)?;
        state.messages.push(message);
        state.session.last_turn_id = Some(turn_id.to_string());
        state.session.status = SessionStatus::Active;
        state.session.meta.updated_at = Utc::now();

        if state.session.title.is_none() {
            let mut title = state
                .messages
                .first()
                .map(|item| item.content.clone())
                .unwrap_or_else(|| "Session".into())
                .trim()
                .replace('\n', " ");
            if title.len() > 48 {
                title.truncate(48);
            }
            state.session.title = Some(title);
        }

        Some(state.session.clone())
    }

    pub fn record_event(
        &self,
        session_id: &str,
        turn_id: &str,
        event_id: &str,
        event_type: EventType,
        payload: Value,
    ) -> Option<RuntimeEvent> {
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        let state = sessions.get_mut(session_id)?;
        let event = RuntimeEvent {
            event_id: event_id.into(),
            event_type,
            occurred_at: Utc::now(),
            workspace_id: Some(self.runtime_config.workspace.workspace_id.clone()),
            agent_id: Some(
                self.runtime_config
                    .agent_runtime
                    .default_agent
                    .agent_id
                    .clone(),
            ),
            session_id: Some(session_id.into()),
            turn_id: Some(turn_id.into()),
            task_id: None,
            plugin_id: None,
            node_id: None,
            source: EventSource::Operator,
            causation_id: None,
            correlation_id: Some(turn_id.into()),
            idempotency_key: None,
            payload,
            schema_version: self.runtime_config.event_schema_version.clone(),
        };
        state.events.push(event.clone());
        Some(event)
    }
}

fn default_session(runtime_config: &RuntimeConfig) -> Session {
    let session_id = "session.default".to_string();
    Session {
        meta: ObjectMeta::new(session_id.clone(), "2026-04-10"),
        session_id,
        workspace_id: runtime_config.workspace.workspace_id.clone(),
        agent_id: runtime_config.agent_runtime.default_agent.agent_id.clone(),
        channel_id: None,
        surface_id: Some("cli.local".into()),
        user_id: Some("operator.local".into()),
        title: Some("Default Session".into()),
        mode: SessionMode::Interactive,
        status: SessionStatus::Idle,
        last_turn_id: None,
    }
}
