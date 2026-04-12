use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde_json::Value;

use crate::{
    api::{SessionMessage, SessionMessageKind, SessionMessageMeta},
    config::RuntimeConfig,
    core::{EventStore, SessionRecord, SessionStore},
    domain::{
        EventSource, EventType, RuntimeEvent, Session, SessionMode, SessionModelTarget,
        SessionStatus,
    },
};

const INITIAL_MIGRATION_VERSION: &str = "2026_04_10_0001_initial_session_event_persistence";
const INITIAL_MIGRATION_DESCRIPTION: &str =
    "initial session, message, and runtime event persistence";
const MODEL_SWITCH_MIGRATION_VERSION: &str = "2026_04_10_0002_session_model_switching";
const MODEL_SWITCH_MIGRATION_DESCRIPTION: &str = "add session model binding columns";

const INITIAL_MIGRATION_SQL: &str = r#"
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    channel_id TEXT NULL,
    surface_id TEXT NULL,
    user_id TEXT NULL,
    title TEXT NULL,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    last_turn_id TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    meta_json TEXT NOT NULL
);

CREATE INDEX idx_sessions_updated_at ON sessions(updated_at);
CREATE INDEX idx_sessions_agent_id ON sessions(agent_id);

CREATE TABLE session_messages (
    message_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id TEXT NULL,
    role TEXT NOT NULL,
    content_text TEXT NOT NULL,
    message_kind TEXT NOT NULL,
    source_channel TEXT NULL,
    source_surface TEXT NULL,
    actor_id TEXT NULL,
    sequence_no INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    meta_json TEXT NOT NULL
);

CREATE INDEX idx_session_messages_session_seq ON session_messages(session_id, sequence_no);
CREATE INDEX idx_session_messages_session_created ON session_messages(session_id, created_at);
CREATE INDEX idx_session_messages_turn_id ON session_messages(turn_id);

CREATE TABLE runtime_events (
    event_id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    workspace_id TEXT NULL,
    agent_id TEXT NULL,
    session_id TEXT NULL,
    turn_id TEXT NULL,
    task_id TEXT NULL,
    plugin_id TEXT NULL,
    node_id TEXT NULL,
    source_kind TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    correlation_id TEXT NULL,
    causation_id TEXT NULL,
    idempotency_key TEXT NULL,
    payload_json TEXT NOT NULL,
    schema_version TEXT NOT NULL
);

CREATE INDEX idx_runtime_events_session_time ON runtime_events(session_id, occurred_at);
CREATE INDEX idx_runtime_events_turn_id ON runtime_events(turn_id);
CREATE INDEX idx_runtime_events_task_id ON runtime_events(task_id);
CREATE INDEX idx_runtime_events_type_time ON runtime_events(event_type, occurred_at);
"#;

const MODEL_SWITCH_MIGRATION_SQL: &str = r#"
ALTER TABLE sessions ADD COLUMN current_provider_id TEXT NULL;
ALTER TABLE sessions ADD COLUMN current_model_id TEXT NULL;
ALTER TABLE sessions ADD COLUMN pending_provider_id TEXT NULL;
ALTER TABLE sessions ADD COLUMN pending_model_id TEXT NULL;
ALTER TABLE sessions ADD COLUMN last_model_switched_at TEXT NULL;
"#;

#[derive(Debug, Clone)]
pub struct SqlitePersistence {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Default)]
pub struct SqliteSessionStore {
    backend: Option<SqlitePersistence>,
}

#[derive(Debug, Clone, Default)]
pub struct SqliteEventStore {
    backend: Option<SqlitePersistence>,
}

impl SqlitePersistence {
    pub fn open(runtime_config: &RuntimeConfig) -> Result<Self> {
        let db_path = persistence_db_path(runtime_config);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create sqlite state directory at {}",
                    parent.display()
                )
            })?;
        }

        let connection = Connection::open(&db_path)
            .with_context(|| format!("failed to open sqlite database at {}", db_path.display()))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .context("failed to enable sqlite WAL mode")?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .context("failed to enable sqlite foreign keys")?;

        let backend = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        backend.bootstrap_schema()?;
        Ok(backend)
    }

    pub fn session_store(&self) -> SqliteSessionStore {
        SqliteSessionStore {
            backend: Some(self.clone()),
        }
    }

    pub fn event_store(&self) -> SqliteEventStore {
        SqliteEventStore {
            backend: Some(self.clone()),
        }
    }

    fn bootstrap_schema(&self) -> Result<()> {
        let mut connection = self
            .connection
            .lock()
            .expect("sqlite connection lock poisoned");
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version TEXT PRIMARY KEY,
                    applied_at TEXT NOT NULL,
                    description TEXT NULL
                );",
            )
            .context("failed to bootstrap schema_migrations table")?;

        let applied_versions = {
            let mut stmt = connection
                .prepare("SELECT version FROM schema_migrations")
                .context("failed to query applied schema migrations")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .context("failed to iterate applied schema migrations")?;
            let mut versions = BTreeSet::new();
            for version in rows {
                versions.insert(version.context("failed to decode schema migration version")?);
            }
            versions
        };

        for (version, description, sql) in [
            (
                INITIAL_MIGRATION_VERSION,
                INITIAL_MIGRATION_DESCRIPTION,
                INITIAL_MIGRATION_SQL,
            ),
            (
                MODEL_SWITCH_MIGRATION_VERSION,
                MODEL_SWITCH_MIGRATION_DESCRIPTION,
                MODEL_SWITCH_MIGRATION_SQL,
            ),
        ] {
            if applied_versions.contains(version) {
                continue;
            }

            let tx = connection
                .transaction()
                .context("failed to begin sqlite migration transaction")?;
            tx.execute_batch(sql)
                .with_context(|| format!("failed to apply sqlite migration {version}"))?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at, description) VALUES (?1, ?2, ?3)",
                params![version, Utc::now().to_rfc3339(), description],
            )
            .with_context(|| format!("failed to record sqlite migration {version}"))?;
            tx.commit()
                .with_context(|| format!("failed to commit sqlite migration {version}"))?;
        }

        Ok(())
    }

    fn with_connection<T>(&self, op: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let connection = self
            .connection
            .lock()
            .expect("sqlite connection lock poisoned");
        op(&connection)
    }

    fn with_transaction<T>(&self, op: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut connection = self
            .connection
            .lock()
            .expect("sqlite connection lock poisoned");
        let tx = connection
            .transaction()
            .context("failed to begin sqlite transaction")?;
        let value = op(&tx)?;
        tx.commit().context("failed to commit sqlite transaction")?;
        Ok(value)
    }

    fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        self.with_connection(|connection| {
            let mut stmt = connection.prepare(
                "SELECT session_id, workspace_id, agent_id, channel_id, surface_id, user_id,
                        title, mode, status, last_turn_id, current_provider_id, current_model_id,
                        pending_provider_id, pending_model_id, last_model_switched_at,
                        created_at, updated_at, schema_version, meta_json
                 FROM sessions
                 ORDER BY updated_at DESC, created_at DESC, session_id ASC",
            )?;
            let rows = stmt.query_map([], read_session_row)?;
            let mut sessions = Vec::new();
            for row in rows {
                let session = row.context("failed to decode session row")?;
                let session_id = session.session_id.clone();
                sessions.push(self.load_session_record(connection, &session_id, session)?);
            }
            Ok(sessions)
        })
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        self.with_connection(|connection| {
            let session = select_session(connection, session_id)?;
            session
                .map(|session| self.load_session_record(connection, session_id, session))
                .transpose()
        })
    }

    fn upsert_session(&self, session: Session) -> Result<Session> {
        self.with_transaction(|tx| {
            upsert_session_row(tx, &session)?;
            Ok(session)
        })
    }

    fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        mut message: SessionMessage,
    ) -> Result<Session> {
        self.with_transaction(|tx| {
            let mut session =
                select_session(tx, session_id)?.ok_or_else(|| anyhow!("session not found: {session_id}"))?;

            let sequence_no: i64 = tx.query_row(
                "SELECT COALESCE(MAX(sequence_no), 0) + 1 FROM session_messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;

            let now = Utc::now();
            if message.meta.message_id.is_none() {
                return Err(anyhow!("message missing meta.message_id"));
            }
            if message.meta.timestamp.is_none() {
                message.meta.timestamp = Some(now);
            }

            tx.execute(
                "INSERT INTO session_messages (
                    message_id, session_id, turn_id, role, content_text, message_kind,
                    source_channel, source_surface, actor_id, sequence_no, created_at, meta_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    message.meta.message_id.clone().unwrap_or_default(),
                    session_id,
                    nullable_str(turn_id),
                    message.display_role(),
                    &message.content,
                    message_kind_to_str(&message.normalized_kind()),
                    message.meta.channel.clone(),
                    message.meta.surface.clone(),
                    message.meta.actor_id.clone(),
                    sequence_no,
                    message.meta.timestamp.as_ref().unwrap().to_rfc3339(),
                    serde_json::to_string(&message)
                        .context("failed to serialize session message to sqlite")?,
                ],
            )
            .context("failed to insert session message")?;

            session.last_turn_id = Some(turn_id.to_string());
            session.status = SessionStatus::Active;
            session.meta.updated_at = now;
            if session.title.is_none() {
                let mut title = message.content.trim().replace('\n', " ");
                if title.len() > 48 {
                    title.truncate(48);
                }
                if !title.is_empty() {
                    session.title = Some(title);
                }
            }

            upsert_session_row(tx, &session)?;
            Ok(session)
        })
    }

    fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.with_transaction(|tx| {
            if let Some(session_id) = event.session_id.as_deref() {
                select_session(tx, session_id)?
                    .ok_or_else(|| anyhow!("session not found: {session_id}"))?;
            }

            tx.execute(
                "INSERT INTO runtime_events (
                    event_id, event_type, workspace_id, agent_id, session_id, turn_id, task_id,
                    plugin_id, node_id, source_kind, occurred_at, correlation_id, causation_id,
                    idempotency_key, payload_json, schema_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    &event.event_id,
                    event_type_to_str(&event.event_type),
                    &event.workspace_id,
                    &event.agent_id,
                    &event.session_id,
                    &event.turn_id,
                    &event.task_id,
                    &event.plugin_id,
                    &event.node_id,
                    source_kind_to_str(&event.source),
                    event.occurred_at.to_rfc3339(),
                    &event.correlation_id,
                    &event.causation_id,
                    &event.idempotency_key,
                    serde_json::to_string(&event.payload)
                        .context("failed to serialize runtime event payload")?,
                    &event.schema_version,
                ],
            )
            .context("failed to insert runtime event")?;

            Ok(event)
        })
    }

    fn list_events_by_session(&self, session_id: &str) -> Result<Vec<RuntimeEvent>> {
        self.with_connection(|connection| {
            select_events(connection, "session_id = ?1", params![session_id])
        })
    }

    fn list_events_by_turn(&self, turn_id: &str) -> Result<Vec<RuntimeEvent>> {
        self.with_connection(|connection| {
            select_events(connection, "turn_id = ?1", params![turn_id])
        })
    }

    fn load_session_record(
        &self,
        connection: &Connection,
        session_id: &str,
        session: Session,
    ) -> Result<SessionRecord> {
        Ok(SessionRecord {
            session,
            messages: select_messages(connection, session_id)?,
            events: select_events(connection, "session_id = ?1", params![session_id])?,
        })
    }
}

impl SessionStore for SqliteSessionStore {
    fn upsert_session(&self, session: Session) -> Result<Session> {
        self.backend()?.upsert_session(session)
    }

    fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        self.backend()?.list_sessions()
    }

    fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        self.backend()?.get_session(session_id)
    }

    fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        message: SessionMessage,
    ) -> Result<Session> {
        self.backend()?.append_message(session_id, turn_id, message)
    }
}

impl EventStore for SqliteEventStore {
    fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.backend()?.append_event(event)
    }

    fn list_events_by_session(&self, session_id: &str) -> Result<Vec<RuntimeEvent>> {
        self.backend()?.list_events_by_session(session_id)
    }

    fn list_events_by_turn(&self, turn_id: &str) -> Result<Vec<RuntimeEvent>> {
        self.backend()?.list_events_by_turn(turn_id)
    }
}

impl SqliteSessionStore {
    fn backend(&self) -> Result<&SqlitePersistence> {
        self.backend
            .as_ref()
            .ok_or_else(|| anyhow!("sqlite session store backend is not initialized"))
    }
}

impl SqliteEventStore {
    fn backend(&self) -> Result<&SqlitePersistence> {
        self.backend
            .as_ref()
            .ok_or_else(|| anyhow!("sqlite event store backend is not initialized"))
    }
}

fn persistence_db_path(runtime_config: &RuntimeConfig) -> PathBuf {
    runtime_config
        .runtime_paths
        .state_root
        .join("session_event_persistence.sqlite3")
}

fn nullable_str(value: &str) -> Option<&str> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn select_session(conn: &Connection, session_id: &str) -> Result<Option<Session>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, workspace_id, agent_id, channel_id, surface_id, user_id,
                title, mode, status, last_turn_id, current_provider_id, current_model_id,
                pending_provider_id, pending_model_id, last_model_switched_at,
                created_at, updated_at, schema_version, meta_json
         FROM sessions
         WHERE session_id = ?1",
    )?;
    stmt.query_row(params![session_id], read_session_row)
        .optional()
        .context("failed to fetch session row")
}

fn select_messages(conn: &Connection, session_id: &str) -> Result<Vec<SessionMessage>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, role, content_text, message_kind, source_channel, source_surface,
                actor_id, sequence_no, created_at, meta_json
         FROM session_messages
         WHERE session_id = ?1
         ORDER BY sequence_no ASC, created_at ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        let meta_json: String = row.get("meta_json")?;
        let stored_message = serde_json::from_str::<SessionMessage>(&meta_json).ok();
        if let Some(message) = stored_message {
            return Ok(message);
        }

        let role: String = row.get("role")?;
        let kind_text: String = row.get("message_kind")?;
        let created_at: String = row.get("created_at")?;
        Ok(SessionMessage {
            kind: parse_message_kind(&kind_text).map_err(into_sql_err)?,
            meta: SessionMessageMeta {
                message_id: Some(row.get("message_id")?),
                session_id: Some(session_id.to_string()),
                channel: row.get("source_channel")?,
                surface: row.get("source_surface")?,
                actor_id: row.get("actor_id")?,
                timestamp: Some(parse_datetime(&created_at).map_err(into_sql_err)?),
                locale: None,
                extra: Default::default(),
            },
            content: row.get("content_text")?,
            annotations: Vec::new(),
            role: Some(role),
        })
    })?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.context("failed to decode session message row")?);
    }
    Ok(messages)
}

fn select_events<P>(conn: &Connection, predicate: &str, params: P) -> Result<Vec<RuntimeEvent>>
where
    P: rusqlite::Params,
{
    let sql = format!(
        "SELECT event_id, event_type, workspace_id, agent_id, session_id, turn_id, task_id,
                plugin_id, node_id, source_kind, occurred_at, correlation_id, causation_id,
                idempotency_key, payload_json, schema_version
         FROM runtime_events
         WHERE {predicate}
         ORDER BY occurred_at ASC, event_id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, |row| {
        let occurred_at: String = row.get("occurred_at")?;
        let plugin_id: Option<String> = row.get("plugin_id")?;
        let node_id: Option<String> = row.get("node_id")?;
        let source_kind: String = row.get("source_kind")?;
        let payload_json: String = row.get("payload_json")?;
        Ok(RuntimeEvent {
            event_id: row.get("event_id")?,
            event_type: parse_event_type(&row.get::<_, String>("event_type")?)
                .map_err(into_sql_err)?,
            occurred_at: parse_datetime(&occurred_at).map_err(into_sql_err)?,
            workspace_id: row.get("workspace_id")?,
            agent_id: row.get("agent_id")?,
            session_id: row.get("session_id")?,
            turn_id: row.get("turn_id")?,
            task_id: row.get("task_id")?,
            plugin_id: plugin_id.clone(),
            node_id: node_id.clone(),
            source: parse_event_source(&source_kind, plugin_id, node_id).map_err(into_sql_err)?,
            causation_id: row.get("causation_id")?,
            correlation_id: row.get("correlation_id")?,
            idempotency_key: row.get("idempotency_key")?,
            payload: serde_json::from_str::<Value>(&payload_json)
                .map_err(|error| into_sql_err(anyhow!(error)))
                .map_err(|error| match error {
                    rusqlite::Error::FromSqlConversionFailure(_, _, _) => error,
                    other => into_sql_err(anyhow!(other.to_string())),
                })?,
            schema_version: row.get("schema_version")?,
        })
    })?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row.context("failed to decode runtime event row")?);
    }
    Ok(events)
}

fn upsert_session_row(conn: &Connection, session: &Session) -> Result<()> {
    conn.execute(
        "INSERT INTO sessions (
            session_id, workspace_id, agent_id, channel_id, surface_id, user_id, title, mode,
            status, last_turn_id, current_provider_id, current_model_id, pending_provider_id,
            pending_model_id, last_model_switched_at, created_at, updated_at, schema_version, meta_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT(session_id) DO UPDATE SET
            workspace_id = excluded.workspace_id,
            agent_id = excluded.agent_id,
            channel_id = excluded.channel_id,
            surface_id = excluded.surface_id,
            user_id = excluded.user_id,
            title = excluded.title,
            mode = excluded.mode,
            status = excluded.status,
            last_turn_id = excluded.last_turn_id,
            current_provider_id = excluded.current_provider_id,
            current_model_id = excluded.current_model_id,
            pending_provider_id = excluded.pending_provider_id,
            pending_model_id = excluded.pending_model_id,
            last_model_switched_at = excluded.last_model_switched_at,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            schema_version = excluded.schema_version,
            meta_json = excluded.meta_json",
        params![
            &session.session_id,
            &session.workspace_id,
            &session.agent_id,
            &session.channel_id,
            &session.surface_id,
            &session.user_id,
            &session.title,
            session_mode_to_str(&session.mode),
            session_status_to_str(&session.status),
            &session.last_turn_id,
            &session.current_provider_id,
            &session.current_model_id,
            session
                .pending_model_switch
                .as_ref()
                .map(|target| target.provider_id.clone()),
            session
                .pending_model_switch
                .as_ref()
                .map(|target| target.model_id.clone()),
            session
                .last_model_switched_at
                .map(|timestamp| timestamp.to_rfc3339()),
            session.meta.created_at.to_rfc3339(),
            session.meta.updated_at.to_rfc3339(),
            &session.meta.schema_version,
            serde_json::to_string(&session.meta)
                .context("failed to serialize session meta json")?,
        ],
    )
    .context("failed to upsert session row")?;
    Ok(())
}

fn read_session_row(row: &Row<'_>) -> rusqlite::Result<Session> {
    let meta_json: String = row.get("meta_json")?;
    let mut meta =
        serde_json::from_str::<crate::domain::ObjectMeta>(&meta_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                meta_json.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    meta.created_at = parse_datetime(&row.get::<_, String>("created_at")?).map_err(into_sql_err)?;
    meta.updated_at = parse_datetime(&row.get::<_, String>("updated_at")?).map_err(into_sql_err)?;
    meta.schema_version = row.get("schema_version")?;

    Ok(Session {
        meta,
        session_id: row.get("session_id")?,
        workspace_id: row.get("workspace_id")?,
        agent_id: row.get("agent_id")?,
        channel_id: row.get("channel_id")?,
        surface_id: row.get("surface_id")?,
        user_id: row.get("user_id")?,
        title: row.get("title")?,
        mode: parse_session_mode(&row.get::<_, String>("mode")?).map_err(into_sql_err)?,
        status: parse_session_status(&row.get::<_, String>("status")?).map_err(into_sql_err)?,
        last_turn_id: row.get("last_turn_id")?,
        current_provider_id: row.get("current_provider_id")?,
        current_model_id: row.get("current_model_id")?,
        pending_model_switch: pending_model_switch_from_row(row)?,
        last_model_switched_at: row
            .get::<_, Option<String>>("last_model_switched_at")?
            .map(|timestamp| parse_datetime(&timestamp).map_err(into_sql_err))
            .transpose()?,
    })
}

fn pending_model_switch_from_row(row: &Row<'_>) -> rusqlite::Result<Option<SessionModelTarget>> {
    let pending_provider_id: Option<String> = row.get("pending_provider_id")?;
    let pending_model_id: Option<String> = row.get("pending_model_id")?;
    match (pending_provider_id, pending_model_id) {
        (Some(provider_id), Some(model_id)) => Ok(Some(SessionModelTarget {
            provider_id,
            model_id,
        })),
        (None, None) => Ok(None),
        _ => Err(into_sql_err(anyhow!(
            "invalid session pending model switch columns"
        ))),
    }
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 timestamp: {value}"))?
        .with_timezone(&Utc))
}

fn into_sql_err(error: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(error.to_string())),
    )
}

fn session_mode_to_str(mode: &SessionMode) -> &'static str {
    match mode {
        SessionMode::Interactive => "interactive",
        SessionMode::BackgroundBound => "background_bound",
        SessionMode::Imported => "imported",
    }
}

fn parse_session_mode(value: &str) -> Result<SessionMode> {
    match value {
        "interactive" => Ok(SessionMode::Interactive),
        "background_bound" => Ok(SessionMode::BackgroundBound),
        "imported" => Ok(SessionMode::Imported),
        _ => Err(anyhow!("unknown session mode: {value}")),
    }
}

fn session_status_to_str(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "active",
        SessionStatus::Idle => "idle",
        SessionStatus::Closed => "closed",
        SessionStatus::Archived => "archived",
    }
}

fn parse_session_status(value: &str) -> Result<SessionStatus> {
    match value {
        "active" => Ok(SessionStatus::Active),
        "idle" => Ok(SessionStatus::Idle),
        "closed" => Ok(SessionStatus::Closed),
        "archived" => Ok(SessionStatus::Archived),
        _ => Err(anyhow!("unknown session status: {value}")),
    }
}

fn message_kind_to_str(kind: &SessionMessageKind) -> &'static str {
    match kind {
        SessionMessageKind::User => "user",
        SessionMessageKind::Assistant => "assistant",
        SessionMessageKind::ToolResult => "tool_result",
        SessionMessageKind::System => "system",
        SessionMessageKind::Runtime => "runtime",
    }
}

fn parse_message_kind(value: &str) -> Result<SessionMessageKind> {
    match value {
        "user" => Ok(SessionMessageKind::User),
        "assistant" => Ok(SessionMessageKind::Assistant),
        "tool_result" => Ok(SessionMessageKind::ToolResult),
        "system" => Ok(SessionMessageKind::System),
        "runtime" => Ok(SessionMessageKind::Runtime),
        _ => Err(anyhow!("unknown session message kind: {value}")),
    }
}

fn event_type_to_str(value: &EventType) -> &'static str {
    match value {
        EventType::MessageReceived => "message_received",
        EventType::TurnStarted => "turn_started",
        EventType::ContextBuilt => "context_built",
        EventType::ModelCalled => "model_called",
        EventType::ModelResponseReceived => "model_response_received",
        EventType::ToolCalled => "tool_called",
        EventType::ToolCompleted => "tool_completed",
        EventType::ToolFailed => "tool_failed",
        EventType::ShellSessionOpened => "shell_session_opened",
        EventType::ShellSessionClosed => "shell_session_closed",
        EventType::ShellSessionResized => "shell_session_resized",
        EventType::ShellExecutionStarted => "shell_execution_started",
        EventType::ShellOutputAppended => "shell_output_appended",
        EventType::ShellExecutionCompleted => "shell_execution_completed",
        EventType::ShellExecutionFailed => "shell_execution_failed",
        EventType::ShellExecutionInterrupted => "shell_execution_interrupted",
        EventType::SleepRequested => "sleep_requested",
        EventType::TaskWaiting => "task_waiting",
        EventType::TaskResumed => "task_resumed",
        EventType::ArtifactCreated => "artifact_created",
        EventType::TaskStarted => "task_started",
        EventType::TaskSucceeded => "task_succeeded",
        EventType::TaskFailed => "task_failed",
        EventType::TaskCheckpointed => "task_checkpointed",
        EventType::TaskCancelled => "task_cancelled",
        EventType::MemoryCommitted => "memory_committed",
        EventType::SummaryCompacted => "summary_compacted",
        EventType::SummaryInvalidated => "summary_invalidated",
        EventType::SummaryRecomputed => "summary_recomputed",
        EventType::PluginLoaded => "plugin_loaded",
        EventType::PluginReloaded => "plugin_reloaded",
        EventType::PluginDrained => "plugin_drained",
        EventType::ScheduleTriggered => "schedule_triggered",
        EventType::NodeStatusChanged => "node_status_changed",
        EventType::ResourceBound => "resource_bound",
        EventType::BillingRecorded => "billing_recorded",
        EventType::UsageRecorded => "usage_recorded",
        EventType::TurnSucceeded => "turn_succeeded",
        EventType::TurnFailed => "turn_failed",
        EventType::ModelSwitchRequested => "model_switch_requested",
        EventType::ModelSwitchApplied => "model_switch_applied",
        EventType::ModelSwitchRejected => "model_switch_rejected",
    }
}

fn parse_event_type(value: &str) -> Result<EventType> {
    match value {
        "message_received" => Ok(EventType::MessageReceived),
        "turn_started" => Ok(EventType::TurnStarted),
        "context_built" => Ok(EventType::ContextBuilt),
        "model_called" => Ok(EventType::ModelCalled),
        "model_response_received" => Ok(EventType::ModelResponseReceived),
        "tool_called" => Ok(EventType::ToolCalled),
        "tool_completed" => Ok(EventType::ToolCompleted),
        "tool_failed" => Ok(EventType::ToolFailed),
        "shell_session_opened" => Ok(EventType::ShellSessionOpened),
        "shell_session_closed" => Ok(EventType::ShellSessionClosed),
        "shell_session_resized" => Ok(EventType::ShellSessionResized),
        "shell_execution_started" => Ok(EventType::ShellExecutionStarted),
        "shell_output_appended" => Ok(EventType::ShellOutputAppended),
        "shell_execution_completed" => Ok(EventType::ShellExecutionCompleted),
        "shell_execution_failed" => Ok(EventType::ShellExecutionFailed),
        "shell_execution_interrupted" => Ok(EventType::ShellExecutionInterrupted),
        "sleep_requested" => Ok(EventType::SleepRequested),
        "task_waiting" => Ok(EventType::TaskWaiting),
        "task_resumed" => Ok(EventType::TaskResumed),
        "artifact_created" => Ok(EventType::ArtifactCreated),
        "task_started" => Ok(EventType::TaskStarted),
        "task_succeeded" => Ok(EventType::TaskSucceeded),
        "task_failed" => Ok(EventType::TaskFailed),
        "task_checkpointed" => Ok(EventType::TaskCheckpointed),
        "task_cancelled" => Ok(EventType::TaskCancelled),
        "memory_committed" => Ok(EventType::MemoryCommitted),
        "summary_compacted" => Ok(EventType::SummaryCompacted),
        "summary_invalidated" => Ok(EventType::SummaryInvalidated),
        "summary_recomputed" => Ok(EventType::SummaryRecomputed),
        "plugin_loaded" => Ok(EventType::PluginLoaded),
        "plugin_reloaded" => Ok(EventType::PluginReloaded),
        "plugin_drained" => Ok(EventType::PluginDrained),
        "schedule_triggered" => Ok(EventType::ScheduleTriggered),
        "node_status_changed" => Ok(EventType::NodeStatusChanged),
        "resource_bound" => Ok(EventType::ResourceBound),
        "billing_recorded" => Ok(EventType::BillingRecorded),
        "usage_recorded" => Ok(EventType::UsageRecorded),
        "turn_succeeded" => Ok(EventType::TurnSucceeded),
        "turn_failed" => Ok(EventType::TurnFailed),
        "model_switch_requested" => Ok(EventType::ModelSwitchRequested),
        "model_switch_applied" => Ok(EventType::ModelSwitchApplied),
        "model_switch_rejected" => Ok(EventType::ModelSwitchRejected),
        _ => Err(anyhow!("unknown runtime event type: {value}")),
    }
}

fn source_kind_to_str(value: &EventSource) -> &'static str {
    match value {
        EventSource::User => "user",
        EventSource::Agent => "agent",
        EventSource::Plugin { .. } => "plugin",
        EventSource::Scheduler => "scheduler",
        EventSource::Node { .. } => "node",
        EventSource::Operator => "operator",
        EventSource::System => "system",
    }
}

fn parse_event_source(
    source_kind: &str,
    plugin_id: Option<String>,
    node_id: Option<String>,
) -> Result<EventSource> {
    match source_kind {
        "user" => Ok(EventSource::User),
        "agent" => Ok(EventSource::Agent),
        "plugin" => Ok(EventSource::Plugin {
            plugin_id: plugin_id.ok_or_else(|| anyhow!("plugin source missing plugin_id"))?,
        }),
        "scheduler" => Ok(EventSource::Scheduler),
        "node" => Ok(EventSource::Node {
            node_id: node_id.ok_or_else(|| anyhow!("node source missing node_id"))?,
        }),
        "operator" => Ok(EventSource::Operator),
        "system" => Ok(EventSource::System),
        _ => Err(anyhow!("unknown runtime event source kind: {source_kind}")),
    }
}
