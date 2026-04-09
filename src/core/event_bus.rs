use std::sync::{Arc, Mutex};

use crate::domain::RuntimeEvent;

#[derive(Debug, Clone, Default)]
pub struct EventBus {
    events: Arc<Mutex<Vec<RuntimeEvent>>>,
}

impl EventBus {
    pub fn publish(&self, event: RuntimeEvent) {
        self.events.lock().expect("event bus poisoned").push(event);
    }

    pub fn snapshot(&self) -> Vec<RuntimeEvent> {
        self.events.lock().expect("event bus poisoned").clone()
    }
}
