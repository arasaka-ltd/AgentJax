use crate::app::Application;
use crate::config::ConfigLoader;

pub fn bootstrap_application() -> anyhow::Result<Application> {
    let loaded = ConfigLoader::load_default()?;
    Application::new_with_snapshot(
        loaded.config_root,
        loaded.config_snapshot,
        loaded.runtime_config,
        loaded.workspace_identity,
    )
}
