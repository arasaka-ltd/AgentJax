use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::Result;
use chrono::Utc;
use serde_json::Value;

use crate::{
    api::SessionMessage,
    config::RuntimeConfig,
    core::{EventStore, PersistenceStore, SessionRecord, SessionStore},
    domain::{
        EventSource, EventType, ObjectMeta, RuntimeEvent, Session, SessionMode, SessionStatus,
    },
    plugins::storage::sqlite_backend::SqlitePersistence,
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
    persistence: Arc<dyn PersistenceStore>,
}

impl DaemonStore {
    pub fn new(runtime_config: RuntimeConfig) -> Result<Self> {
        let sqlite = SqlitePersistence::open(&runtime_config)?;
        let persistence: Arc<dyn PersistenceStore> = Arc::new(SqlitePersistenceBridge::new(sqlite));

        let store = Self {
            runtime_config,
            started_at: Instant::now(),
            ready: AtomicBool::new(true),
            draining: AtomicBool::new(false),
            next_connection: AtomicU64::new(1),
            next_message: AtomicU64::new(1),
            next_turn: AtomicU64::new(1),
            next_event: AtomicU64::new(1),
            next_stream: AtomicU64::new(1),
            persistence,
        };
        store.ensure_default_session()?;
        Ok(store)
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

    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        self.persistence.list_sessions()
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        self.persistence.get_session(session_id)
    }

    pub fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        message: SessionMessage,
    ) -> Result<Session> {
        self.persistence
            .append_message(session_id, turn_id, message)
    }

    pub fn record_event(
        &self,
        session_id: &str,
        turn_id: &str,
        event_id: &str,
        event_type: EventType,
        payload: Value,
    ) -> Result<RuntimeEvent> {
        self.persistence.append_event(RuntimeEvent {
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
        })
    }

    fn ensure_default_session(&self) -> Result<()> {
        let session_id = "session.default";
        if self.persistence.get_session(session_id)?.is_none() {
            self.persistence
                .upsert_session(default_session(&self.runtime_config))?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SqlitePersistenceBridge {
    session_store: crate::plugins::storage::sqlite_sessions::SqliteSessionStore,
    event_store: crate::plugins::storage::sqlite_context::SqliteEventStore,
}

impl SqlitePersistenceBridge {
    fn new(sqlite: SqlitePersistence) -> Self {
        Self {
            session_store: sqlite.session_store(),
            event_store: sqlite.event_store(),
        }
    }
}

impl SessionStore for SqlitePersistenceBridge {
    fn upsert_session(&self, session: Session) -> Result<Session> {
        self.session_store.upsert_session(session)
    }

    fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        self.session_store.list_sessions()
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        self.session_store.get_session(session_id)
    }

    fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        message: SessionMessage,
    ) -> Result<Session> {
        self.session_store
            .append_message(session_id, turn_id, message)
    }
}

impl EventStore for SqlitePersistenceBridge {
    fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.event_store.append_event(event)
    }

    fn list_events_by_session(&self, session_id: &str) -> Result<Vec<RuntimeEvent>> {
        self.event_store.list_events_by_session(session_id)
    }

    fn list_events_by_turn(&self, turn_id: &str) -> Result<Vec<RuntimeEvent>> {
        self.event_store.list_events_by_turn(turn_id)
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
