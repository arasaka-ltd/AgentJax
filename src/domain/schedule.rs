use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Schedule {
    pub meta: ObjectMeta,
    pub schedule_id: String,
    pub name: String,
    pub trigger: TaskTrigger,
    pub target: TaskTarget,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskTrigger {
    Cron { expression: String },
    Interval { seconds: u64 },
    Event { event_type: String },
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskTarget {
    TaskRef { definition_ref: String },
    SkillRef { skill_id: String },
    WorkflowRef { workflow_id: String },
}
