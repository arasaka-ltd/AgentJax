use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::{
    migrator::ConfigMigrator,
    normalizer::ConfigNormalizer,
    snapshot::RuntimeConfigSnapshot,
    validator::{ConfigValidationReport, ConfigValidator},
    ConfigRoot, LlmProviderConfig, ModelCatalogSnapshot, RuntimeConfig, WorkspaceConfig,
    WorkspaceDocument, WorkspaceIdentityPack, WorkspacePaths,
};

pub const CURRENT_CONFIG_SCHEMA_VERSION: &str = "config.v1";

#[derive(Debug, Clone, Default)]
pub struct ConfigLoader;

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config_root: ConfigRoot,
    pub runtime_config: RuntimeConfig,
    pub workspace_identity: WorkspaceIdentityPack,
    pub config_snapshot: RuntimeConfigSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum InitMode {
    #[default]
    Minimal,
    LocalDev,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct CoreTomlConfig {
    #[serde(default)]
    pub schema_version: Option<String>,
    pub app_name: Option<String>,
    pub workspace_id: Option<String>,
    pub runtime_root: Option<String>,
    pub workspace_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ProvidersTomlConfig {
    #[serde(default)]
    pub schema_version: Option<String>,
    pub llm: ProvidersLlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProvidersLlmConfig {
    pub default_provider_id: String,
    pub providers: Vec<LlmProviderConfig>,
}

impl Default for ProvidersLlmConfig {
    fn default() -> Self {
        Self {
            default_provider_id: "openai-default".into(),
            providers: vec![LlmProviderConfig::OpenAi(Default::default())],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ModelsTomlConfig {
    #[serde(default)]
    pub schema_version: Option<String>,
    pub defaults: ModelsDefaultsConfig,
    #[serde(default)]
    pub snapshot: ModelCatalogSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModelsDefaultsConfig {
    pub provider_id: String,
    pub model_id: String,
}

impl Default for ModelsDefaultsConfig {
    fn default() -> Self {
        Self {
            provider_id: "openai-default".into(),
            model_id: "gpt-4o-mini".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ResourcesTomlConfig {
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct DaemonTomlConfig {
    #[serde(default)]
    pub schema_version: Option<String>,
    pub unix_socket: String,
    pub websocket_bind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PluginsTomlConfig {
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub enabled: BTreeSet<String>,
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    #[serde(default)]
    pub config_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub policy_flags: BTreeMap<String, bool>,
    #[serde(default)]
    pub reload_hints: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedConfigBundle {
    pub config_root: ConfigRoot,
    pub core: CoreTomlConfig,
    pub providers: ProvidersTomlConfig,
    pub models: ModelsTomlConfig,
    pub resources: ResourcesTomlConfig,
    pub daemon: DaemonTomlConfig,
    pub plugins: PluginsTomlConfig,
}

impl ConfigLoader {
    pub fn load_default() -> Result<LoadedConfig> {
        Self::load_from_roots("./config", "./runtime", "./workspace")
    }

    pub fn validate_default() -> Result<ConfigValidationReport> {
        Self::validate_at("./config", "./runtime", "./workspace")
    }

    pub fn load_snapshot_default() -> Result<RuntimeConfigSnapshot> {
        Self::load_default().map(|loaded| loaded.config_snapshot)
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
            schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
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
            schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
            llm: ProvidersLlmConfig::default(),
        };
        write_toml_if_missing(&config_root.providers_config, &providers)?;
        write_toml_if_missing(
            &config_root.plugins_config,
            &PluginsTomlConfig {
                schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
                ..PluginsTomlConfig::default()
            },
        )?;

        let models = ModelsTomlConfig {
            schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
            defaults: ModelsDefaultsConfig::default(),
            snapshot: ModelCatalogSnapshot::default(),
        };
        write_toml_if_missing(&config_root.models_config, &models)?;

        write_toml_if_missing(
            &config_root.resources_config,
            &ResourcesTomlConfig {
                schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
                resources: Vec::new(),
            },
        )?;
        write_toml_if_missing(
            &config_root.daemon_config,
            &DaemonTomlConfig {
                schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
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

    pub fn validate_at(
        config_root: impl AsRef<Path>,
        runtime_root: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<ConfigValidationReport> {
        let parsed = Self::parse_at(
            config_root.as_ref(),
            runtime_root.as_ref(),
            workspace_root.as_ref(),
        )?;
        let (migrated, migration) = ConfigMigrator::migrate(parsed);
        let mut report = ConfigValidator::validate(&migrated)?;
        report.warnings.extend(migration.warnings);
        report.migrations = migration.steps;
        report.ok = report.errors.is_empty();
        Ok(report)
    }

    pub fn load_from_roots(
        config_root: impl AsRef<Path>,
        runtime_root: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<LoadedConfig> {
        let parsed = Self::parse_at(
            config_root.as_ref(),
            runtime_root.as_ref(),
            workspace_root.as_ref(),
        )?;
        let (migrated, migration) = ConfigMigrator::migrate(parsed);
        let validation = ConfigValidator::validate(&migrated)?;
        if !validation.ok {
            return Err(anyhow!(
                "config validation failed: {}",
                validation.errors.join("; ")
            ));
        }

        let normalized = ConfigNormalizer::normalize(migrated)?;
        let workspace_identity =
            Self::load_workspace_identity(&normalized.runtime_config.workspace)?;
        let config_snapshot = RuntimeConfigSnapshot::build(
            normalized.schema_version.clone(),
            normalized.runtime_config.clone(),
            normalized.daemon.clone(),
        )?;

        if !migration.warnings.is_empty() {
            let _ = migration;
        }

        Ok(LoadedConfig {
            config_root: normalized.config_root,
            runtime_config: normalized.runtime_config,
            workspace_identity,
            config_snapshot,
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
                schema_version: Some(CURRENT_CONFIG_SCHEMA_VERSION.into()),
                defaults: ModelsDefaultsConfig::default(),
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
            schema_version: existing
                .schema_version
                .or_else(|| Some(CURRENT_CONFIG_SCHEMA_VERSION.into())),
            defaults: existing.defaults,
            snapshot: ModelCatalogSnapshot {
                generated_at: snapshot.generated_at,
                providers,
            },
        };
        fs::write(&config_root.models_config, toml::to_string_pretty(&config)?)?;
        Ok(())
    }

    fn parse_at(
        config_root: &Path,
        runtime_root: &Path,
        workspace_root: &Path,
    ) -> Result<ParsedConfigBundle> {
        Self::initialize_at(config_root, runtime_root, workspace_root, InitMode::Minimal)?;

        let config_root = ConfigRoot::new(config_root);
        Ok(ParsedConfigBundle {
            core: toml::from_str(&fs::read_to_string(&config_root.core_config)?)?,
            providers: toml::from_str(&fs::read_to_string(&config_root.providers_config)?)?,
            plugins: toml::from_str(&fs::read_to_string(&config_root.plugins_config)?)?,
            models: toml::from_str(&fs::read_to_string(&config_root.models_config)?)?,
            resources: toml::from_str(&fs::read_to_string(&config_root.resources_config)?)?,
            daemon: toml::from_str(&fs::read_to_string(&config_root.daemon_config)?)?,
            config_root,
        })
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
