pub mod local_scheduler;
pub mod mock;
pub mod openai;
pub mod static_nodes;
pub mod telegram;

use crate::plugins::{
    local_scheduler::LocalSchedulerPlugin, static_nodes::StaticNodeRegistryPlugin,
    telegram::TelegramChannelPlugin,
};
use crate::{core::Plugin, domain::PluginManifest};

pub fn known_plugin_manifests() -> Vec<PluginManifest> {
    vec![
        TelegramChannelPlugin.manifest(),
        LocalSchedulerPlugin.manifest(),
        StaticNodeRegistryPlugin.manifest(),
    ]
}
