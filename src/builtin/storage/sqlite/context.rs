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
