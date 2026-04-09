use std::collections::BTreeMap;

use crate::domain::{Resource, ResourceId};

#[derive(Debug, Clone, Default)]
pub struct ResourceRegistry {
    resources: BTreeMap<ResourceId, Resource>,
}

impl ResourceRegistry {
    pub fn register(&mut self, resource: Resource) {
        self.resources
            .insert(resource.resource_id.clone(), resource);
    }

    pub fn get(&self, id: &ResourceId) -> Option<&Resource> {
        self.resources.get(id)
    }

    pub fn all(&self) -> Vec<Resource> {
        self.resources.values().cloned().collect()
    }
}
