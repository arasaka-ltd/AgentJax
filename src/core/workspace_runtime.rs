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

#[derive(Debug, Clone)]
pub struct WorkspaceRuntimeHost {
    pub workspace_runtime: WorkspaceRuntime,
}

impl WorkspaceRuntimeHost {
    pub fn new(workspace: WorkspaceConfig) -> Self {
        Self {
            workspace_runtime: WorkspaceRuntime::new(workspace),
        }
    }

    pub fn workspace_id(&self) -> &str {
        self.workspace_runtime.workspace_id()
    }

    pub fn workspace(&self) -> &WorkspaceConfig {
        &self.workspace_runtime.workspace
    }
}
