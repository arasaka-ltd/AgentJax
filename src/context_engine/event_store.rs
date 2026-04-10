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
                    let role = message.get("role")?.as_str()?;
                    let content = message.get("content")?.as_str()?;
                    return Some(format!("{role}: {content}"));
                }

                if let Some(message) = event.payload.get("assistant_message")?.as_str() {
                    return Some(format!("assistant: {message}"));
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
