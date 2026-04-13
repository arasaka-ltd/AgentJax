use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use crate::domain::{EventType, RuntimeEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBodyRecord {
    pub event_id: String,
    pub role: String,
    pub content: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct EventStore {
    events: Arc<Mutex<Vec<RuntimeEvent>>>,
}

impl EventStore {
    pub fn append(&self, event: RuntimeEvent) {
        self.events
            .lock()
            .expect("event store poisoned")
            .push(event);
    }

    pub fn list(&self) -> Vec<RuntimeEvent> {
        self.events.lock().expect("event store poisoned").clone()
    }

    pub fn list_scoped(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Vec<RuntimeEvent> {
        self.list()
            .into_iter()
            .filter(|event| match session_id {
                Some(session_id) => event.session_id.as_deref() == Some(session_id),
                None => true,
            })
            .filter(|event| match task_id {
                Some(task_id) => event.task_id.as_deref() == Some(task_id),
                None => true,
            })
            .collect()
    }

    pub fn message_body_records(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Vec<MessageBodyRecord> {
        self.list_scoped(session_id, task_id)
            .into_iter()
            .filter_map(|event| extract_message_body(&event))
            .collect()
    }

    pub fn recent_transcript(
        &self,
        session_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
    ) -> String {
        self.message_body_records(session_id, task_id)
            .into_iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn extract_message_body(event: &RuntimeEvent) -> Option<MessageBodyRecord> {
    match event.event_type {
        EventType::MessageReceived => {
            let message = event.payload.get("message")?;
            let role = message
                .get("role")
                .and_then(|value| value.as_str())
                .or_else(|| message.get("kind").and_then(|value| value.as_str()))
                .unwrap_or("user");
            if role != "user" {
                return None;
            }
            Some(MessageBodyRecord {
                event_id: event.event_id.clone(),
                role: role.into(),
                content: message.get("content")?.as_str()?.trim().to_string(),
                occurred_at: event.occurred_at,
            })
        }
        EventType::TurnSucceeded => {
            let message = event.payload.get("assistant_message")?;
            if let Some(content) = message.as_str() {
                let content = content.trim();
                if content.is_empty() {
                    return None;
                }
                return Some(MessageBodyRecord {
                    event_id: event.event_id.clone(),
                    role: "assistant".into(),
                    content: content.into(),
                    occurred_at: event.occurred_at,
                });
            }
            let role = message
                .get("role")
                .and_then(|value| value.as_str())
                .or_else(|| message.get("kind").and_then(|value| value.as_str()))
                .unwrap_or("assistant");
            if role != "assistant" {
                return None;
            }
            Some(MessageBodyRecord {
                event_id: event.event_id.clone(),
                role: role.into(),
                content: message.get("content")?.as_str()?.trim().to_string(),
                occurred_at: event.occurred_at,
            })
        }
        _ => None,
    }
    .filter(|message| !message.content.is_empty())
}
