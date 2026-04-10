use std::{fs, path::PathBuf};

use anyhow::Result;
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

    pub fn ensure_workspace_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.paths.root)?;
        fs::create_dir_all(&self.paths.skills_dir)?;
        fs::create_dir_all(&self.paths.memory_daily_dir)?;
        fs::create_dir_all(&self.paths.memory_topics_dir)?;
        fs::create_dir_all(&self.paths.memory_profiles_dir)?;
        fs::create_dir_all(&self.paths.memory_scratch_dir)?;
        fs::create_dir_all(&self.paths.knowledge_dir)?;
        fs::create_dir_all(&self.paths.prompts_dir)?;

        for path in self.persona_files() {
            if !path.exists() {
                fs::write(path, "")?;
            }
        }

        Ok(())
    }

    pub fn persona_files(&self) -> Vec<&PathBuf> {
        vec![
            &self.paths.agent_file,
            &self.paths.soul_file,
            &self.paths.user_file,
            &self.paths.memory_file,
            &self.paths.mission_file,
            &self.paths.rules_file,
            &self.paths.router_file,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDocument {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceIdentityPack {
    pub workspace_id: String,
    pub agent: WorkspaceDocument,
    pub soul: WorkspaceDocument,
    pub user: WorkspaceDocument,
    pub memory: WorkspaceDocument,
    pub mission: WorkspaceDocument,
    pub rules: WorkspaceDocument,
    pub router: WorkspaceDocument,
}

impl WorkspaceIdentityPack {
    pub fn source_paths(&self) -> Vec<String> {
        vec![
            self.agent.path.display().to_string(),
            self.soul.path.display().to_string(),
            self.user.path.display().to_string(),
            self.memory.path.display().to_string(),
            self.mission.path.display().to_string(),
            self.rules.path.display().to_string(),
            self.router.path.display().to_string(),
        ]
    }
}
