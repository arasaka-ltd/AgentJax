use anyhow::Result;

use crate::{
    api::SessionMessage,
    domain::{RuntimeEvent, Session},
};

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRecord {
    pub session: Session,
    pub messages: Vec<SessionMessage>,
    pub events: Vec<RuntimeEvent>,
}

pub trait SessionStore: Send + Sync {
    fn upsert_session(&self, session: Session) -> Result<Session>;
    fn list_session_heads(&self) -> Result<Vec<Session>>;
    fn list_sessions(&self) -> Result<Vec<SessionRecord>>;
    fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>>;
    fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        message: SessionMessage,
    ) -> Result<Session>;
}

pub trait EventStore: Send + Sync {
    fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent>;
    fn list_events_by_session(&self, session_id: &str) -> Result<Vec<RuntimeEvent>>;
    fn list_events_by_turn(&self, turn_id: &str) -> Result<Vec<RuntimeEvent>>;
}

pub trait PersistenceStore: SessionStore + EventStore {}

impl<T> PersistenceStore for T where T: SessionStore + EventStore {}
