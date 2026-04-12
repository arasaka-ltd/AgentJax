pub use super::backend::SqliteSessionStore;

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    core::{Plugin, SessionStore, StoragePlugin},
    domain::{
        Permission, PluginCapability, PluginManifest, RagCapability, ResourceDescriptor, ResourceId,
    },
};

#[derive(Debug, Clone, Default)]
pub struct SqliteSessionStorePlugin {
    store: Option<SqliteSessionStore>,
}

impl SqliteSessionStorePlugin {
    pub fn new(store: SqliteSessionStore) -> Self {
        Self { store: Some(store) }
    }
}

#[async_trait]
impl Plugin for SqliteSessionStorePlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "storage.sqlite.sessions".into(),
            version: "0.1.0".into(),
            capabilities: vec![PluginCapability::Rag(RagCapability::BackendDriver)],
            config_schema: None,
            required_permissions: vec![Permission::ReadState, Permission::WriteState],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: vec![ResourceDescriptor {
                resource_id: ResourceId("store:session".into()),
                kind: "sqlite.session_store".into(),
                description: Some("SQLite-backed session persistence store".into()),
            }],
            hooks: Vec::new(),
        }
    }
}

impl StoragePlugin for SqliteSessionStorePlugin {
    fn session_store(&self) -> Option<Arc<dyn SessionStore>> {
        self.store
            .as_ref()
            .cloned()
            .map(|store| Arc::new(store) as Arc<dyn SessionStore>)
    }
}
