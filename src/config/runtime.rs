use serde::{Deserialize, Serialize};

use super::{RuntimePaths, WorkspaceConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub app_name: String,
    pub runtime_paths: RuntimePaths,
    pub workspace: WorkspaceConfig,
    pub config_schema_version: String,
    pub state_schema_version: String,
    pub event_schema_version: String,
    pub plugin_api_version: String,
    pub skill_spec_version: String,
}

impl RuntimeConfig {
    pub fn new(
        app_name: impl Into<String>,
        runtime_paths: RuntimePaths,
        workspace: WorkspaceConfig,
    ) -> Self {
        Self {
            app_name: app_name.into(),
            runtime_paths,
            workspace,
            config_schema_version: "config.v1".into(),
            state_schema_version: "state.v1".into(),
            event_schema_version: "event.v1".into(),
            plugin_api_version: "plugin-api.v1".into(),
            skill_spec_version: "skill-spec.v1".into(),
        }
    }
}
