use serde::{Deserialize, Serialize};

use super::WorkspacePaths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceBootstrapPolicy {
    pub stable_files: Vec<String>,
    pub on_demand_roots: Vec<String>,
}

impl Default for WorkspaceBootstrapPolicy {
    fn default() -> Self {
        Self {
            stable_files: vec![
                "AGENT.md".into(),
                "SOUL.md".into(),
                "MISSION.md".into(),
                "RULES.md".into(),
                "USER.md".into(),
                "ROUTER.md".into(),
            ],
            on_demand_roots: vec![
                "MEMORY.md".into(),
                "memory/".into(),
                "skills/".into(),
                "knowledge/".into(),
                "prompts/".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub workspace_id: String,
    pub paths: WorkspacePaths,
    pub bootstrap_policy: WorkspaceBootstrapPolicy,
    pub workspace_schema_version: String,
}

impl WorkspaceConfig {
    pub fn new(workspace_id: impl Into<String>, paths: WorkspacePaths) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            paths,
            bootstrap_policy: WorkspaceBootstrapPolicy::default(),
            workspace_schema_version: "workspace.v1".into(),
        }
    }
}
