use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    config::RuntimeConfig,
    core::ResourceRegistry,
    domain::{Node, NodeKind, NodeSelector, NodeStatus, ObjectMeta, TrustLevel},
};

#[derive(Debug, Clone)]
pub struct NodeRegistry {
    nodes: Vec<Node>,
}

impl NodeRegistry {
    pub fn from_runtime(
        runtime_config: &RuntimeConfig,
        resource_registry: &ResourceRegistry,
        draining: bool,
    ) -> Self {
        let mut nodes = vec![local_node(runtime_config, resource_registry, draining)];
        nodes.extend(static_nodes(runtime_config));
        nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        nodes.dedup_by(|left, right| left.node_id == right.node_id);
        Self { nodes }
    }

    pub fn list(&self) -> Vec<Node> {
        self.nodes.clone()
    }

    pub fn get(&self, node_id: &str) -> Option<Node> {
        self.nodes
            .iter()
            .find(|node| node.node_id == node_id)
            .cloned()
    }

    pub fn select(&self, selector: &NodeSelector) -> Vec<Node> {
        let mut candidates = self
            .nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Active)
            .filter(|node| {
                selector.required_capabilities.iter().all(|required| {
                    node.capabilities
                        .iter()
                        .any(|capability| capability == required)
                })
            })
            .filter(|node| {
                selector
                    .min_trust_level
                    .as_ref()
                    .is_none_or(|minimum| node.trust_level.rank() >= minimum.rank())
            })
            .cloned()
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| {
            selector_score(right, selector)
                .cmp(&selector_score(left, selector))
                .then_with(|| left.node_id.cmp(&right.node_id))
        });
        candidates
    }
}

#[derive(Debug, Deserialize, Default)]
struct StaticNodeRegistryConfig {
    #[serde(default)]
    nodes: Vec<StaticNodeConfig>,
}

#[derive(Debug, Deserialize)]
struct StaticNodeConfig {
    node_id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    trust_level: Option<TrustLevel>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

fn local_node(
    runtime_config: &RuntimeConfig,
    resource_registry: &ResourceRegistry,
    draining: bool,
) -> Node {
    Node {
        meta: ObjectMeta::new("node.local", &runtime_config.state_schema_version),
        node_id: "node.local".into(),
        kind: NodeKind::Static,
        platform: std::env::consts::OS.into(),
        status: if draining {
            NodeStatus::Draining
        } else {
            NodeStatus::Active
        },
        capabilities: vec![
            "daemon.control_plane".into(),
            "session.interaction".into(),
            "tool.dispatch".into(),
            "scheduler.tick".into(),
        ],
        resources: resource_registry
            .all()
            .into_iter()
            .map(|resource| resource.resource_id.0)
            .collect(),
        trust_level: TrustLevel::High,
        labels: BTreeMap::from([("scope".into(), "local".into())]),
    }
}

fn static_nodes(runtime_config: &RuntimeConfig) -> Vec<Node> {
    let Some(raw) = runtime_config
        .plugins
        .config_fragment("node.static_registry")
        .cloned()
    else {
        return Vec::new();
    };

    let parsed = serde_json::from_value::<StaticNodeRegistryConfig>(raw).unwrap_or_default();
    parsed
        .nodes
        .into_iter()
        .map(|config| {
            let mut labels = config.labels;
            if let Some(label) = config.label {
                labels.insert("label".into(), label);
            }
            if !config.tags.is_empty() {
                labels.insert("tags".into(), config.tags.join(","));
            }

            Node {
                meta: ObjectMeta::new(&config.node_id, &runtime_config.state_schema_version),
                node_id: config.node_id,
                kind: NodeKind::Static,
                platform: config.platform.unwrap_or_else(|| "unknown".into()),
                status: NodeStatus::Active,
                capabilities: if config.capabilities.is_empty() {
                    vec!["static.registry".into()]
                } else {
                    config.capabilities
                },
                resources: config.resources,
                trust_level: config.trust_level.unwrap_or(TrustLevel::Medium),
                labels,
            }
        })
        .collect()
}

fn selector_score(node: &Node, selector: &NodeSelector) -> usize {
    selector
        .preferred_labels
        .iter()
        .filter(|(key, value)| node.labels.get(*key) == Some(*value))
        .count()
}
