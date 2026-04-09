use serde::{Deserialize, Serialize};

use crate::domain::ObjectMeta;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Skill {
    pub meta: ObjectMeta,
    pub skill_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub manifest_ref: Option<String>,
    pub markdown_ref: Option<String>,
    pub compatibility_version: String,
    pub triggers: Vec<SkillTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillTrigger {
    pub mode: crate::domain::SkillTriggerMode,
    pub pattern: String,
}
