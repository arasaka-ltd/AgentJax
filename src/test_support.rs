use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use crate::{
    app::Application,
    config::{ConfigLoader, InitMode, LlmProviderConfig},
    daemon::Daemon,
};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct TestHarness {
    root: PathBuf,
    pub daemon: Daemon,
}

impl TestHarness {
    pub(crate) fn new(name: &str) -> Self {
        let root = unique_test_root(name);
        let config_root = root.join("config");
        let runtime_root = root.join("runtime");
        let workspace_root = root.join("workspace");

        ConfigLoader::initialize_at(
            &config_root,
            &runtime_root,
            &workspace_root,
            InitMode::Minimal,
        )
        .expect("failed to initialize test config");
        let mut loaded =
            ConfigLoader::load_from_roots(&config_root, &runtime_root, &workspace_root)
                .expect("failed to load test config");
        loaded.runtime_config.agent_runtime.llm.default_provider_id = "mock-default".into();
        loaded.runtime_config.agent_runtime.llm.providers = vec![LlmProviderConfig {
            provider_id: "mock-default".into(),
            kind: "mock".into(),
            settings: json!({
                "provider_id": "mock-default",
            }),
        }];
        loaded
            .runtime_config
            .agent_runtime
            .default_agent
            .provider_id = "mock-default".into();
        loaded.runtime_config.agent_runtime.default_agent.model = "mock-model".into();

        let app = Application::new(
            loaded.config_root.clone(),
            loaded.runtime_config,
            loaded.workspace_identity,
        )
        .expect("failed to build test application");
        let daemon = Daemon::new(app).expect("failed to build test daemon");

        Self { root, daemon }
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unique_test_root(name: &str) -> PathBuf {
    let counter = TEST_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("agentjax-{name}-{nanos}-{counter}"))
}
