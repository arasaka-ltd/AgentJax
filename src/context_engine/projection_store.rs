use std::sync::{Arc, Mutex};
use crate::domain::ContextBlock;
#[derive(Debug, Clone, Default)]
pub struct ProjectionStore {
    blocks: Arc<Mutex<Vec<ContextBlock>>>,
}
impl ProjectionStore {
    pub fn replace(&self, blocks: Vec<ContextBlock>) {
        *self.blocks.lock().expect("projection store poisoned") = blocks;
    }
    pub fn current(&self) -> Vec<ContextBlock> {
        self.blocks.lock().expect("projection store poisoned").clone()
    }
}
