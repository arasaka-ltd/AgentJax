use std::sync::{Arc, Mutex};

use crate::domain::RuntimeEvent;

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

    pub fn recent_transcript(&self, session_id: Option<&str>, limit: usize) -> String {
        self.list()
            .into_iter()
            .filter(|event| match session_id {
                Some(session_id) => event.session_id.as_deref() == Some(session_id),
                None => true,
            })
            .filter_map(|event| {
                if let Some(message) = event.payload.get("message") {
                    let role = message
                        .get("role")
                        .and_then(|value| value.as_str())
                        .or_else(|| message.get("kind").and_then(|value| value.as_str()))
                        .unwrap_or("user");
                    let content = message.get("content")?.as_str()?;
                    return Some(format!("{role}: {content}"));
                }

                if let Some(message) = event.payload.get("assistant_message") {
                    if let Some(text) = message.as_str() {
                        return Some(format!("assistant: {text}"));
                    }
                    let role = message
                        .get("role")
                        .and_then(|value| value.as_str())
                        .or_else(|| message.get("kind").and_then(|value| value.as_str()))
                        .unwrap_or("assistant");
                    let content = message.get("content")?.as_str()?;
                    return Some(format!("{role}: {content}"));
                }

                None
            })
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }
}
