use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use anyhow::Result;
use chrono::Utc;
use serde_json::Value;

use crate::{
    api::SessionMessage,
    builtin::storage::sqlite::SqlitePersistence,
    config::RuntimeConfig,
    core::{EventStore, PersistenceStore, SessionRecord, SessionStore},
    daemon::task_store::{initial_task_record, StoredTaskRecord, TaskStore},
    domain::{
        EventSource, EventType, ObjectMeta, ResumePack, RuntimeEvent, Session, SessionMode,
        SessionStatus, Task, TaskCheckpoint, TaskPhase, TaskTimelineEntry,
    },
    surface::CoreSurface,
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
    next_subscription: AtomicU64,
    next_task: AtomicU64,
    next_checkpoint: AtomicU64,
    active_turn_sessions: Mutex<BTreeSet<String>>,
    persistence: Arc<dyn PersistenceStore>,
    tasks: TaskStore,
}

impl DaemonStore {
    pub fn new(runtime_config: RuntimeConfig) -> Result<Self> {
        let sqlite = SqlitePersistence::open(&runtime_config)?;
        let persistence: Arc<dyn PersistenceStore> = Arc::new(SqlitePersistenceBridge::new(sqlite));
        let tasks = TaskStore::open(&runtime_config)?;

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
            next_subscription: AtomicU64::new(1),
            next_task: AtomicU64::new(1),
            next_checkpoint: AtomicU64::new(1),
            active_turn_sessions: Mutex::new(BTreeSet::new()),
            persistence,
            tasks,
        };
        store.reseed_identity_counters()?;
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

    pub fn set_draining(&self, draining: bool) {
        self.draining.store(draining, Ordering::Relaxed);
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

    pub fn next_subscription_id(&self) -> String {
        let id = self.next_subscription.fetch_add(1, Ordering::Relaxed);
        format!("sub_{id}")
    }

    pub fn next_task_id(&self) -> String {
        let id = self.next_task.fetch_add(1, Ordering::Relaxed);
        format!("task_{id}")
    }

    pub fn next_checkpoint_id(&self) -> String {
        let id = self.next_checkpoint.fetch_add(1, Ordering::Relaxed);
        format!("checkpoint_{id}")
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

    pub fn upsert_session(&self, session: Session) -> Result<Session> {
        self.persistence.upsert_session(session)
    }

    pub fn list_tasks(&self) -> Result<Vec<StoredTaskRecord>> {
        self.tasks.list()
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<StoredTaskRecord>> {
        self.tasks.get(task_id)
    }

    pub fn create_task(&self, task: Task) -> Result<StoredTaskRecord> {
        self.tasks.upsert(initial_task_record(task))
    }

    pub fn update_task(&self, task: Task) -> Result<StoredTaskRecord> {
        self.tasks.update_task(task)
    }

    pub fn append_task_timeline(
        &self,
        task_id: &str,
        phase: TaskPhase,
        status: crate::domain::TaskStatus,
        turn_id: Option<&str>,
        event_id: Option<&str>,
        note: impl Into<String>,
    ) -> Result<StoredTaskRecord> {
        self.tasks.append_timeline(
            task_id,
            TaskTimelineEntry {
                entry_id: format!(
                    "timeline_{}_{}_{}",
                    task_id,
                    turn_id.unwrap_or("task"),
                    self.next_event_id()
                ),
                task_id: task_id.into(),
                phase,
                status,
                turn_id: turn_id.map(str::to_string),
                event_id: event_id.map(str::to_string),
                note: note.into(),
                recorded_at: Utc::now(),
            },
        )
    }

    pub fn write_task_checkpoint(
        &self,
        task_id: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        summary: impl Into<String>,
        resume_pack: ResumePack,
    ) -> Result<StoredTaskRecord> {
        self.tasks.write_checkpoint(
            task_id,
            TaskCheckpoint {
                checkpoint_id: self.next_checkpoint_id(),
                task_id: task_id.into(),
                session_id: session_id.map(str::to_string),
                turn_id: turn_id.map(str::to_string),
                summary: summary.into(),
                created_at: Utc::now(),
                resume_pack,
            },
        )
    }

    pub fn mark_turn_active(&self, session_id: &str) -> Result<()> {
        let mut active = self
            .active_turn_sessions
            .lock()
            .expect("active_turn_sessions lock poisoned");
        if !active.insert(session_id.to_string()) {
            return Err(anyhow::anyhow!("session turn already active: {session_id}"));
        }
        Ok(())
    }

    pub fn clear_turn_active(&self, session_id: &str) {
        self.active_turn_sessions
            .lock()
            .expect("active_turn_sessions lock poisoned")
            .remove(session_id);
    }

    pub fn is_turn_active(&self, session_id: &str) -> bool {
        self.active_turn_sessions
            .lock()
            .expect("active_turn_sessions lock poisoned")
            .contains(session_id)
    }

    pub fn record_event(
        &self,
        session_id: &str,
        turn_id: &str,
        task_id: Option<&str>,
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
            task_id: task_id.map(str::to_string),
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

    fn reseed_identity_counters(&self) -> Result<()> {
        let sessions = self.persistence.list_sessions()?;
        let tasks = self.tasks.list()?;

        let mut next_message = 1_u64;
        let mut next_turn = 1_u64;
        let mut next_event = 1_u64;
        let mut next_task = 1_u64;
        let mut next_checkpoint = 1_u64;

        for record in &sessions {
            if let Some(last_turn_id) = record.session.last_turn_id.as_deref() {
                next_turn = next_turn.max(next_counter_value(last_turn_id, "turn_"));
            }

            for message in &record.messages {
                if let Some(message_id) = message.meta.message_id.as_deref() {
                    next_message = next_message.max(next_counter_value(message_id, "msg_"));
                }
            }

            for event in &record.events {
                next_event = next_event.max(next_counter_value(&event.event_id, "evt_"));
                if let Some(turn_id) = event.turn_id.as_deref() {
                    next_turn = next_turn.max(next_counter_value(turn_id, "turn_"));
                }
            }
        }

        for record in &tasks {
            next_task = next_task.max(next_counter_value(&record.task.task_id, "task_"));
            if let Some(checkpoint_id) = record.task.checkpoint_ref.as_deref() {
                next_checkpoint =
                    next_checkpoint.max(next_counter_value(checkpoint_id, "checkpoint_"));
            }
            for timeline in &record.timeline {
                if let Some(turn_id) = timeline.turn_id.as_deref() {
                    next_turn = next_turn.max(next_counter_value(turn_id, "turn_"));
                }
                if let Some(event_id) = timeline.event_id.as_deref() {
                    next_event = next_event.max(next_counter_value(event_id, "evt_"));
                }
            }
            for checkpoint in &record.checkpoints {
                next_checkpoint = next_checkpoint
                    .max(next_counter_value(&checkpoint.checkpoint_id, "checkpoint_"));
                if let Some(turn_id) = checkpoint.turn_id.as_deref() {
                    next_turn = next_turn.max(next_counter_value(turn_id, "turn_"));
                }
            }
        }

        self.next_message.store(next_message, Ordering::Relaxed);
        self.next_turn.store(next_turn, Ordering::Relaxed);
        self.next_event.store(next_event, Ordering::Relaxed);
        self.next_task.store(next_task, Ordering::Relaxed);
        self.next_checkpoint
            .store(next_checkpoint, Ordering::Relaxed);
        Ok(())
    }
}

fn next_counter_value(id: &str, prefix: &str) -> u64 {
    id.strip_prefix(prefix)
        .map(|suffix| {
            suffix
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
        })
        .and_then(|digits| {
            if digits.is_empty() {
                None
            } else {
                digits.parse::<u64>().ok()
            }
        })
        .map_or(1, |value| value.saturating_add(1))
}

#[derive(Clone)]
struct SqlitePersistenceBridge {
    session_store: crate::builtin::storage::sqlite::SqliteSessionStore,
    event_store: crate::builtin::storage::sqlite::SqliteEventStore,
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
        surface_id: Some(CoreSurface::CliLocal.id().into()),
        user_id: Some("operator.local".into()),
        title: Some("Default Session".into()),
        mode: SessionMode::Interactive,
        status: SessionStatus::Idle,
        last_turn_id: None,
        current_provider_id: Some(
            runtime_config
                .agent_runtime
                .default_agent
                .provider_id
                .clone(),
        ),
        current_model_id: Some(runtime_config.agent_runtime.default_agent.model.clone()),
        pending_model_switch: None,
        last_model_switched_at: None,
    }
}
