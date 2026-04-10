use std::sync::{Arc, Mutex};

use crate::domain::{HookInput, HookPoint, HookRegistration};

#[derive(Debug, Clone, Default)]
pub struct HookBus {
    hooks: Arc<Mutex<Vec<HookRegistration>>>,
    emissions: Arc<Mutex<Vec<HookInput>>>,
}

impl HookBus {
    pub fn register(&self, plugin_id: impl Into<String>, point: HookPoint) {
        self.hooks
            .lock()
            .expect("hook bus poisoned")
            .push(HookRegistration {
                plugin_id: plugin_id.into(),
                point,
            });
    }

    pub fn emit(&self, input: HookInput) {
        self.emissions
            .lock()
            .expect("hook bus poisoned")
            .push(input);
    }

    pub fn registrations(&self) -> Vec<HookRegistration> {
        self.hooks.lock().expect("hook bus poisoned").clone()
    }

    pub fn emissions(&self) -> Vec<HookInput> {
        self.emissions.lock().expect("hook bus poisoned").clone()
    }
}
