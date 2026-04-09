use crate::app::Application;
use crate::config::{ConfigRoot, RuntimeConfig, RuntimePaths, WorkspaceConfig, WorkspacePaths};

pub fn bootstrap_application() -> anyhow::Result<Application> {
    let config_root = ConfigRoot::new("./config");
    let runtime_paths = RuntimePaths::new("./runtime");
    let workspace_paths = WorkspacePaths::new("./workspace");
    let workspace = WorkspaceConfig::new("default-workspace", workspace_paths);
    let runtime_config = RuntimeConfig::new("agentjax", runtime_paths, workspace);
    Ok(Application::new(config_root, runtime_config))
}
