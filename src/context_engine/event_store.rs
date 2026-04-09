use std::sync::{Arc, Mutex};

use crate::domain::RuntimeEvent;

#[derive(Debug, Clone, Default)]
pub struct EventStore {
    events: Arc<Mutex<Vec<RuntimeEvent>>>,
}

impl EventStore {
    pub fn append(&self, event: RuntimeEvent) {
        self.events.lock().expect("event store poisoned").push(event);
    }

    pub fn list(&self) -> Vec<RuntimeEvent> {
        self.events.lock().expect("event store poisoned").clone()
    }
}
