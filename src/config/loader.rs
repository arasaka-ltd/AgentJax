use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{
    AgentDefinition, ConfigRoot, LlmProviderConfig, ModelCatalogSnapshot, RuntimeConfig,
    RuntimePaths, WorkspaceConfig, WorkspaceDocument, WorkspaceIdentityPack, WorkspacePaths,
};

#[derive(Debug, Clone, Default)]
pub struct ConfigLoader;

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config_root: ConfigRoot,
    pub runtime_config: RuntimeConfig,
    pub workspace_identity: WorkspaceIdentityPack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum InitMode {
    #[default]
    Minimal,
    LocalDev,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CoreTomlConfig {
    app_name: Option<String>,
    workspace_id: Option<String>,
    runtime_root: Option<String>,
    workspace_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProvidersTomlConfig {
    llm: ProvidersLlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProvidersLlmConfig {
    default_provider_id: String,
    providers: Vec<LlmProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelsTomlConfig {
    defaults: ModelsDefaultsConfig,
    #[serde(default)]
    snapshot: ModelCatalogSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelsDefaultsConfig {
    provider_id: String,
    model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResourcesTomlConfig {
    resources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonTomlConfig {
    unix_socket: String,
    websocket_bind: String,
}

impl ConfigLoader {
    pub fn load_default() -> Result<LoadedConfig> {
        Self::load_from_roots("./config", "./runtime", "./workspace")
    }

    pub fn initialize_default(mode: InitMode) -> Result<()> {
        Self::initialize_at("./config", "./runtime", "./workspace", mode)
    }

    pub fn initialize_at(
        config_root: impl AsRef<Path>,
        runtime_root: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
        mode: InitMode,
    ) -> Result<()> {
        let config_root = ConfigRoot::new(config_root.as_ref());
        fs::create_dir_all(&config_root.root)?;

        let runtime_root = runtime_root.as_ref().to_string_lossy().into_owned();
        let workspace_root = workspace_root.as_ref().to_string_lossy().into_owned();
        let core = CoreTomlConfig {
            app_name: Some(match mode {
                InitMode::Minimal => "agentjax".into(),
                InitMode::LocalDev => "agentjax-local-dev".into(),
            }),
            workspace_id: Some("default-workspace".into()),
            runtime_root: Some(runtime_root.clone()),
            workspace_root: Some(workspace_root.clone()),
        };
        write_toml_if_missing(&config_root.core_config, &core)?;

        let providers = ProvidersTomlConfig {
            llm: ProvidersLlmConfig {
                default_provider_id: "openai-default".into(),
                providers: vec![LlmProviderConfig::OpenAi(Default::default())],
            },
        };
        write_toml_if_missing(&config_root.providers_config, &providers)?;

        let models = ModelsTomlConfig {
            defaults: ModelsDefaultsConfig {
                provider_id: "openai-default".into(),
                model_id: "gpt-4o-mini".into(),
            },
            snapshot: ModelCatalogSnapshot::default(),
        };
        write_toml_if_missing(&config_root.models_config, &models)?;

        write_toml_if_missing(
            &config_root.resources_config,
            &ResourcesTomlConfig {
                resources: Vec::new(),
            },
        )?;
        write_toml_if_missing(
            &config_root.daemon_config,
            &DaemonTomlConfig {
                unix_socket: PathBuf::from(&runtime_root)
                    .join("run")
                    .join("daemon.sock")
                    .display()
                    .to_string(),
                websocket_bind: "127.0.0.1:4080".into(),
            },
        )?;

        let workspace =
            WorkspaceConfig::new("default-workspace", WorkspacePaths::new(workspace_root));
        workspace.ensure_workspace_layout()?;
        Ok(())
    }

    pub fn load_from_roots(
        config_root: impl AsRef<Path>,
        runtime_root: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<LoadedConfig> {
        Self::initialize_at(
            config_root.as_ref(),
            runtime_root.as_ref(),
            workspace_root.as_ref(),
            InitMode::Minimal,
        )?;

        let config_root = ConfigRoot::new(config_root.as_ref());
        let core: CoreTomlConfig = toml::from_str(&fs::read_to_string(&config_root.core_config)?)?;
        let providers: ProvidersTomlConfig =
            toml::from_str(&fs::read_to_string(&config_root.providers_config)?)?;
        let models: ModelsTomlConfig =
            toml::from_str(&fs::read_to_string(&config_root.models_config)?)?;

        let runtime_root = core
            .runtime_root
            .unwrap_or_else(|| runtime_root.as_ref().to_string_lossy().into_owned());
        let workspace_root = core
            .workspace_root
            .unwrap_or_else(|| workspace_root.as_ref().to_string_lossy().into_owned());
        let workspace = WorkspaceConfig::new(
            core.workspace_id
                .unwrap_or_else(|| "default-workspace".into()),
            WorkspacePaths::new(workspace_root),
        );
        workspace.ensure_workspace_layout()?;

        let workspace_identity = Self::load_workspace_identity(&workspace)?;
        let mut runtime_config = RuntimeConfig::new(
            core.app_name.unwrap_or_else(|| "agentjax".into()),
            RuntimePaths::new(runtime_root),
            workspace,
        );
        runtime_config.agent_runtime.default_agent = AgentDefinition {
            provider_id: models.defaults.provider_id.clone(),
            model: models.defaults.model_id.clone(),
            ..runtime_config.agent_runtime.default_agent
        };
        runtime_config.agent_runtime.llm.default_provider_id = providers.llm.default_provider_id;
        runtime_config.agent_runtime.llm.providers = providers.llm.providers;
        runtime_config.agent_runtime.llm.model_catalog = models.snapshot;

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

    pub fn write_model_snapshot(
        config_root: &ConfigRoot,
        _provider_id: &str,
        snapshot: ModelCatalogSnapshot,
    ) -> Result<()> {
        let existing: ModelsTomlConfig = if config_root.models_config.exists() {
            toml::from_str(&fs::read_to_string(&config_root.models_config)?)?
        } else {
            ModelsTomlConfig {
                defaults: ModelsDefaultsConfig {
                    provider_id: "openai-default".into(),
                    model_id: "gpt-4o-mini".into(),
                },
                snapshot: ModelCatalogSnapshot::default(),
            }
        };

        let mut providers = existing.snapshot.providers;
        for incoming in snapshot.providers {
            if let Some(index) = providers
                .iter()
                .position(|provider| provider.provider_id == incoming.provider_id)
            {
                providers[index] = incoming;
            } else {
                providers.push(incoming);
            }
        }

        let config = ModelsTomlConfig {
            defaults: existing.defaults,
            snapshot: ModelCatalogSnapshot {
                generated_at: snapshot.generated_at,
                providers,
            },
        };
        fs::write(&config_root.models_config, toml::to_string_pretty(&config)?)?;
        Ok(())
    }

    fn read_document(path: &Path) -> Result<WorkspaceDocument> {
        Ok(WorkspaceDocument {
            path: path.to_path_buf(),
            content: fs::read_to_string(path)?,
        })
    }
}

fn write_toml_if_missing<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if !path.exists() {
        fs::write(path, toml::to_string_pretty(value)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::config::{
        ConfigRoot, ModelCatalogSnapshot, ModelInfoSnapshot, ProviderModelCatalog,
    };

    use super::{ConfigLoader, InitMode};

    #[test]
    fn initializes_config_root_idempotently() {
        let root = temp_path("config-init");
        let config_root = root.join("config");
        let runtime_root = root.join("runtime");
        let workspace_root = root.join("workspace");

        ConfigLoader::initialize_at(
            &config_root,
            &runtime_root,
            &workspace_root,
            InitMode::LocalDev,
        )
        .unwrap();
        let first = fs::read_to_string(config_root.join("core.toml")).unwrap();
        ConfigLoader::initialize_at(
            &config_root,
            &runtime_root,
            &workspace_root,
            InitMode::Minimal,
        )
        .unwrap();
        let second = fs::read_to_string(config_root.join("core.toml")).unwrap();

        assert_eq!(first, second);
        assert!(config_root.join("providers.toml").exists());
        assert!(config_root.join("models.toml").exists());
        assert!(config_root.join("resources.toml").exists());
        assert!(config_root.join("daemon.toml").exists());
        assert!(workspace_root.join("AGENT.md").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_model_snapshot_preserves_existing_defaults() {
        let root = temp_path("model-snapshot");
        let config_root = root.join("config");
        let runtime_root = root.join("runtime");
        let workspace_root = root.join("workspace");

        ConfigLoader::initialize_at(
            &config_root,
            &runtime_root,
            &workspace_root,
            InitMode::Minimal,
        )
        .unwrap();

        let root_config = ConfigRoot::new(&config_root);
        ConfigLoader::write_model_snapshot(
            &root_config,
            "openai-alt",
            ModelCatalogSnapshot {
                generated_at: Some(chrono::Utc::now()),
                providers: vec![ProviderModelCatalog {
                    provider_id: "openai-alt".into(),
                    provider_kind: "openai".into(),
                    base_url: Some("http://127.0.0.1:9999/v1".into()),
                    language_models: vec![ModelInfoSnapshot {
                        model_id: "gpt-alt".into(),
                        display_label: "GPT Alt".into(),
                        context_length: Some(128000),
                        input_token_limit: Some(128000),
                        output_token_limit: Some(16384),
                        capability_tags: vec!["text".into()],
                    }],
                }],
            },
        )
        .unwrap();

        let models_toml = fs::read_to_string(root_config.models_config).unwrap();
        assert!(models_toml.contains("provider_id = \"openai-default\""));
        assert!(models_toml.contains("model_id = \"gpt-4o-mini\""));
        assert!(models_toml.contains("provider_id = \"openai-alt\""));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_from_roots_reads_workspace_identity_documents() {
        let root = temp_path("workspace-identity");
        let config_root = root.join("config");
        let runtime_root = root.join("runtime");
        let workspace_root = root.join("workspace");

        ConfigLoader::initialize_at(
            &config_root,
            &runtime_root,
            &workspace_root,
            InitMode::Minimal,
        )
        .unwrap();

        fs::write(workspace_root.join("AGENT.md"), "agent profile").unwrap();
        fs::write(workspace_root.join("SOUL.md"), "calm and direct").unwrap();
        fs::write(workspace_root.join("USER.md"), "prefers concise answers").unwrap();
        fs::write(
            workspace_root.join("MEMORY.md"),
            "remember the current repo status",
        )
        .unwrap();
        fs::write(workspace_root.join("MISSION.md"), "ship the runtime").unwrap();
        fs::write(workspace_root.join("RULES.md"), "do not guess").unwrap();
        fs::write(
            workspace_root.join("ROUTER.md"),
            "prefer the daemon control plane",
        )
        .unwrap();

        let loaded =
            ConfigLoader::load_from_roots(&config_root, &runtime_root, &workspace_root).unwrap();

        assert_eq!(loaded.workspace_identity.agent.content, "agent profile");
        assert_eq!(
            loaded.workspace_identity.router.content,
            "prefer the daemon control plane"
        );
        assert!(loaded
            .runtime_config
            .workspace
            .paths
            .memory_topics_dir
            .exists());
        assert!(loaded.runtime_config.workspace.paths.knowledge_dir.exists());
        assert!(loaded.runtime_config.workspace.paths.prompts_dir.exists());

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
