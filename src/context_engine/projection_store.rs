use crate::domain::ContextProjection;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct ProjectionStore {
    projection: Arc<Mutex<Option<ContextProjection>>>,
}

impl ProjectionStore {
    pub fn replace(&self, projection: ContextProjection) {
        *self.projection.lock().expect("projection store poisoned") = Some(projection);
    }

    pub fn current(&self) -> Option<ContextProjection> {
        self.projection
            .lock()
            .expect("projection store poisoned")
            .clone()
    }
}
