use anyhow::Result;

use super::{ConfigRoot, RuntimeConfig, RuntimePaths, WorkspaceConfig, WorkspacePaths};

#[derive(Debug, Clone, Default)]
pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load_default() -> Result<(ConfigRoot, RuntimeConfig)> {
        let config_root = ConfigRoot::new("./config");
        let runtime_paths = RuntimePaths::new("./runtime");
        let workspace_paths = WorkspacePaths::new("./workspace");
        let workspace = WorkspaceConfig::new("default-workspace", workspace_paths);
        let runtime_config = RuntimeConfig::new("agentjax", runtime_paths, workspace);
        Ok((config_root, runtime_config))
    }
}
