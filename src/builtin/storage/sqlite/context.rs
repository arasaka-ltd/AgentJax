pub use super::backend::SqliteEventStore;

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    core::{EventStore, Plugin, StoragePlugin},
    domain::{
        Permission, PluginCapability, PluginManifest, RagCapability, ResourceDescriptor, ResourceId,
    },
};

#[derive(Debug, Clone, Default)]
pub struct SqliteContextStorePlugin {
    store: Option<SqliteEventStore>,
}

impl SqliteContextStorePlugin {
    pub fn new(store: SqliteEventStore) -> Self {
        Self { store: Some(store) }
    }
}

#[async_trait]
impl Plugin for SqliteContextStorePlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "storage.sqlite.runtime_events".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Rag(RagCapability::BackendDriver)],
            config_schema: None,
            required_permissions: vec![Permission::ReadState, Permission::WriteState],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: vec![ResourceDescriptor {
                resource_id: ResourceId("store:runtime_events".into()),
                kind: "sqlite.event_store".into(),
                description: Some("SQLite-backed runtime event persistence store".into()),
            }],
            hooks: Vec::new(),
        }
    }
}

impl StoragePlugin for SqliteContextStorePlugin {
    fn event_store(&self) -> Option<Arc<dyn EventStore>> {
        self.store
            .as_ref()
            .cloned()
            .map(|store| Arc::new(store) as Arc<dyn EventStore>)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        builtin::storage::sqlite::{SqliteContextStorePlugin, SqlitePersistence},
        config::{RuntimeConfig, RuntimePaths, WorkspaceConfig, WorkspacePaths},
        core::StoragePlugin,
    };

    #[test]
    fn sqlite_event_store_plugin_exposes_storage_handle() {
        let root = std::env::temp_dir().join(format!(
            "agentjax-sqlite-context-plugin-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let runtime = RuntimeConfig::new(
            "AgentJax",
            RuntimePaths::new(root.join("runtime")),
            WorkspaceConfig::new(
                "workspace-test",
                WorkspacePaths::new(root.join("workspace")),
            ),
        );
        let persistence = SqlitePersistence::open(&runtime).unwrap();
        let plugin = SqliteContextStorePlugin::new(persistence.event_store());

        assert!(plugin.event_store().is_some());
    }
}
