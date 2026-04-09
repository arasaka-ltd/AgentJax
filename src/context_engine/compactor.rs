use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionDecision {
    pub should_compact: bool,
    pub reason: String,
}

pub trait CompactionEvaluator: Send + Sync {
    fn evaluate(&self) -> CompactionDecision;
}

#[derive(Debug, Clone, Default)]
pub struct NoopCompactionEvaluator;

impl CompactionEvaluator for NoopCompactionEvaluator {
    fn evaluate(&self) -> CompactionDecision {
        CompactionDecision {
            should_compact: false,
            reason: "compaction not enabled".into(),
        }
    }
}
