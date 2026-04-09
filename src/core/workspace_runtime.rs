use crate::config::WorkspaceConfig;

#[derive(Debug, Clone)]
pub struct WorkspaceRuntime {
    pub workspace: WorkspaceConfig,
}

impl WorkspaceRuntime {
    pub fn new(workspace: WorkspaceConfig) -> Self {
        Self { workspace }
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace.workspace_id
    }
}
