pub mod local_scheduler;
pub mod mock;
pub mod openai;
pub mod static_nodes;
pub mod telegram;

use std::path::Path;

use anyhow::{anyhow, Result};

use crate::plugins::{
    local_scheduler::LocalSchedulerPlugin, static_nodes::StaticNodeRegistryPlugin,
    telegram::TelegramChannelPlugin,
};
use crate::{
    config::LlmProviderConfig,
    core::{Plugin, PluginManagerCandidate},
    domain::PluginManifest,
};

pub fn known_plugin_manifests() -> Vec<PluginManifest> {
    vec![
        TelegramChannelPlugin.manifest(),
        LocalSchedulerPlugin.manifest(),
        StaticNodeRegistryPlugin.manifest(),
    ]
}

pub fn provider_candidate(
    provider: &LlmProviderConfig,
    config_root: &Path,
) -> Result<PluginManagerCandidate> {
    match provider.kind() {
        "openai" => openai::provider_candidate(provider, config_root),
        "mock" => mock::provider_candidate(provider),
        other => Err(anyhow!("unknown provider kind: {other}")),
    }
}
