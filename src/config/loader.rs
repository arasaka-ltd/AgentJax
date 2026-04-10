use std::{fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

use super::{
    ConfigRoot, RuntimeConfig, RuntimePaths, WorkspaceConfig, WorkspaceDocument,
    WorkspaceIdentityPack, WorkspacePaths,
};

#[derive(Debug, Clone, Default)]
pub struct ConfigLoader;

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config_root: ConfigRoot,
    pub runtime_config: RuntimeConfig,
    pub workspace_identity: WorkspaceIdentityPack,
}

#[derive(Debug, Deserialize, Default)]
struct CoreTomlConfig {
    app_name: Option<String>,
    workspace_id: Option<String>,
    runtime_root: Option<String>,
    workspace_root: Option<String>,
}

impl ConfigLoader {
    pub fn load_default() -> Result<LoadedConfig> {
        Self::load_from_roots("./config", "./runtime", "./workspace")
    }

    pub fn load_from_roots(
        config_root: impl AsRef<Path>,
        runtime_root: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<LoadedConfig> {
        let config_root = ConfigRoot::new(config_root.as_ref());
        fs::create_dir_all(&config_root.root)?;

        let core_config = if config_root.core_config.exists() {
            toml::from_str::<CoreTomlConfig>(&fs::read_to_string(&config_root.core_config)?)?
        } else {
            CoreTomlConfig::default()
        };

        let runtime_root = core_config
            .runtime_root
            .unwrap_or_else(|| runtime_root.as_ref().to_string_lossy().into_owned());
        let workspace_root = core_config
            .workspace_root
            .unwrap_or_else(|| workspace_root.as_ref().to_string_lossy().into_owned());

        let runtime_paths = RuntimePaths::new(runtime_root);
        let workspace = WorkspaceConfig::new(
            core_config
                .workspace_id
                .unwrap_or_else(|| "default-workspace".into()),
            WorkspacePaths::new(workspace_root),
        );
        workspace.ensure_workspace_layout()?;

        fs::create_dir_all(&runtime_paths.run_root)?;
        fs::create_dir_all(&runtime_paths.state_root)?;
        fs::create_dir_all(&runtime_paths.artifacts_root)?;
        fs::create_dir_all(&runtime_paths.logs_root)?;
        fs::create_dir_all(&runtime_paths.cache_root)?;
        fs::create_dir_all(&runtime_paths.tmp_root)?;

        let workspace_identity = Self::load_workspace_identity(&workspace)?;
        let runtime_config = RuntimeConfig::new(
            core_config.app_name.unwrap_or_else(|| "agentjax".into()),
            runtime_paths,
            workspace,
        );

        Ok(LoadedConfig {
            config_root,
            runtime_config,
            workspace_identity,
        })
    }

    pub fn load_workspace_identity(workspace: &WorkspaceConfig) -> Result<WorkspaceIdentityPack> {
        Ok(WorkspaceIdentityPack {
            workspace_id: workspace.workspace_id.clone(),
            agent: Self::read_document(&workspace.paths.agent_file)?,
            soul: Self::read_document(&workspace.paths.soul_file)?,
            user: Self::read_document(&workspace.paths.user_file)?,
            memory: Self::read_document(&workspace.paths.memory_file)?,
            mission: Self::read_document(&workspace.paths.mission_file)?,
            rules: Self::read_document(&workspace.paths.rules_file)?,
            router: Self::read_document(&workspace.paths.router_file)?,
        })
    }

    fn read_document(path: &Path) -> Result<WorkspaceDocument> {
        Ok(WorkspaceDocument {
            path: path.to_path_buf(),
            content: fs::read_to_string(path)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::ConfigLoader;

    #[test]
    fn creates_workspace_template_and_loads_identity() {
        let root = temp_path("config-loader");
        let config_root = root.join("config");
        let runtime_root = root.join("runtime");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&config_root).expect("create config dir");
        fs::write(
            config_root.join("core.toml"),
            format!(
                "app_name = \"agentjax-test\"\nworkspace_id = \"ws-test\"\nruntime_root = \"{}\"\nworkspace_root = \"{}\"\n",
                runtime_root.display(),
                workspace_root.display(),
            ),
        )
        .expect("write core.toml");

        let loaded =
            ConfigLoader::load_from_roots(&config_root, &runtime_root, &workspace_root).unwrap();

        assert_eq!(loaded.runtime_config.app_name, "agentjax-test");
        assert_eq!(loaded.runtime_config.workspace.workspace_id, "ws-test");
        assert!(workspace_root.join("AGENT.md").exists());
        assert!(workspace_root.join("SOUL.md").exists());
        assert!(workspace_root.join("USER.md").exists());
        assert!(workspace_root.join("MISSION.md").exists());
        assert!(workspace_root.join("RULES.md").exists());
        assert!(workspace_root.join("ROUTER.md").exists());
        assert!(workspace_root.join("MEMORY.md").exists());
        assert_eq!(loaded.workspace_identity.workspace_id, "ws-test");

        let _ = fs::remove_dir_all(root);
    }

    fn temp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("agentjax-{prefix}-{nanos}"))
    }
}
