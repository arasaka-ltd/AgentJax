use std::path::PathBuf;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigRoot {
    pub root: PathBuf,
    pub core_config: PathBuf,
    pub plugins_config: PathBuf,
    pub providers_config: PathBuf,
    pub models_config: PathBuf,
    pub resources_config: PathBuf,
    pub channels_config: PathBuf,
    pub nodes_config: PathBuf,
    pub scheduler_config: PathBuf,
    pub skills_config: PathBuf,
}
impl ConfigRoot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            core_config: root.join("core.toml"),
            plugins_config: root.join("plugins.toml"),
            providers_config: root.join("providers.toml"),
            models_config: root.join("models.toml"),
            resources_config: root.join("resources.toml"),
            channels_config: root.join("channels.toml"),
            nodes_config: root.join("nodes.toml"),
            scheduler_config: root.join("scheduler.toml"),
            skills_config: root.join("skills.toml"),
            root,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePaths {
    pub root: PathBuf,
    pub agent_file: PathBuf,
    pub soul_file: PathBuf,
    pub user_file: PathBuf,
    pub memory_file: PathBuf,
    pub mission_file: PathBuf,
    pub rules_file: PathBuf,
    pub router_file: PathBuf,
    pub skills_dir: PathBuf,
    pub memory_dir: PathBuf,
    pub memory_daily_dir: PathBuf,
    pub memory_topics_dir: PathBuf,
    pub memory_profiles_dir: PathBuf,
    pub memory_scratch_dir: PathBuf,
    pub knowledge_dir: PathBuf,
    pub prompts_dir: PathBuf,
}
impl WorkspacePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let memory_dir = root.join("memory");
        Self {
            agent_file: root.join("AGENT.md"),
            soul_file: root.join("SOUL.md"),
            user_file: root.join("USER.md"),
            memory_file: root.join("MEMORY.md"),
            mission_file: root.join("MISSION.md"),
            rules_file: root.join("RULES.md"),
            router_file: root.join("ROUTER.md"),
            skills_dir: root.join("skills"),
            memory_daily_dir: memory_dir.join("daily"),
            memory_topics_dir: memory_dir.join("topics"),
            memory_profiles_dir: memory_dir.join("profiles"),
            memory_scratch_dir: memory_dir.join("scratch"),
            knowledge_dir: root.join("knowledge"),
            prompts_dir: root.join("prompts"),
            memory_dir,
            root,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub state_root: PathBuf,
    pub sessions_dir: PathBuf,
    pub tasks_dir: PathBuf,
    pub lcm_dir: PathBuf,
    pub checkpoints_dir: PathBuf,
    pub leases_dir: PathBuf,
    pub plugin_state_dir: PathBuf,
    pub artifacts_root: PathBuf,
    pub logs_root: PathBuf,
    pub cache_root: PathBuf,
    pub tmp_root: PathBuf,
}
impl RuntimePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let state_root = root.join("state");
        Self {
            sessions_dir: state_root.join("sessions"),
            tasks_dir: state_root.join("tasks"),
            lcm_dir: state_root.join("lcm"),
            checkpoints_dir: state_root.join("checkpoints"),
            leases_dir: state_root.join("leases"),
            plugin_state_dir: state_root.join("plugin-state"),
            artifacts_root: root.join("artifacts"),
            logs_root: root.join("logs"),
            cache_root: root.join("cache"),
            tmp_root: root.join("tmp"),
            state_root,
            root,
        }
    }
}
