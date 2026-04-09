use std::sync::{Arc, Mutex};

use crate::domain::HookPoint;

#[derive(Debug, Clone, Default)]
pub struct HookBus {
    hooks: Arc<Mutex<Vec<HookPoint>>>,
}

impl HookBus {
    pub fn register(&self, hook: HookPoint) {
        self.hooks.lock().expect("hook bus poisoned").push(hook);
    }

    pub fn snapshot(&self) -> Vec<HookPoint> {
        self.hooks.lock().expect("hook bus poisoned").clone()
    }
}
