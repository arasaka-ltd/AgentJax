use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectMeta {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub labels: BTreeMap<String, String>,
    pub metadata: Map<String, Value>,
    pub schema_version: String,
}

impl ObjectMeta {
    pub fn new(id: impl Into<String>, schema_version: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            created_at: now,
            updated_at: now,
            labels: BTreeMap::new(),
            metadata: Map::new(),
            schema_version: schema_version.into(),
        }
    }
}
