use crate::config::{WorkspaceConfig, WorkspaceIdentityPack};

#[derive(Debug, Clone)]
pub struct WorkspaceRuntime {
    pub workspace: WorkspaceConfig,
    pub identity: WorkspaceIdentityPack,
}

impl WorkspaceRuntime {
    pub fn new(workspace: WorkspaceConfig, identity: WorkspaceIdentityPack) -> Self {
        Self {
            workspace,
            identity,
        }
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
    pub fn new(workspace: WorkspaceConfig, identity: WorkspaceIdentityPack) -> Self {
        Self {
            workspace_runtime: WorkspaceRuntime::new(workspace, identity),
        }
    }

    pub fn workspace_id(&self) -> &str {
        self.workspace_runtime.workspace_id()
    }

    pub fn workspace(&self) -> &WorkspaceConfig {
        &self.workspace_runtime.workspace
    }

    pub fn identity(&self) -> &WorkspaceIdentityPack {
        &self.workspace_runtime.identity
    }
}
