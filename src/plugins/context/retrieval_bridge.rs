use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use async_trait::async_trait;

use crate::config::{WorkspaceDocument, WorkspacePaths};
use crate::{
    core::{ContextPlugin, Plugin},
    domain::{
        ContextCapability, KnowledgeCapability, MemoryCapability, Permission, PluginCapability,
        PluginManifest,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalCollectionKind {
    Memory,
    Knowledge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalCollection {
    pub collection_id: String,
    pub kind: RetrievalCollectionKind,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedDocument {
    pub collection_id: String,
    pub kind: RetrievalCollectionKind,
    pub document_ref: String,
    pub title: String,
    pub content: String,
    pub score: u32,
}

#[derive(Debug, Clone)]
pub struct RetrievalBridgeContextPlugin {
    memory_collection: RetrievalCollection,
    knowledge_collection: RetrievalCollection,
}

impl RetrievalBridgeContextPlugin {
    pub fn new(paths: &WorkspacePaths) -> Self {
        Self {
            memory_collection: RetrievalCollection {
                collection_id: "memory".into(),
                kind: RetrievalCollectionKind::Memory,
                root: paths.memory_topics_dir.clone(),
            },
            knowledge_collection: RetrievalCollection {
                collection_id: "knowledge".into(),
                kind: RetrievalCollectionKind::Knowledge,
                root: paths.knowledge_dir.clone(),
            },
        }
    }

    pub fn collections(&self) -> Vec<RetrievalCollection> {
        vec![
            self.memory_collection.clone(),
            self.knowledge_collection.clone(),
        ]
    }

    pub fn recall_memory(
        &self,
        memory_document: &WorkspaceDocument,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RetrievedDocument>> {
        let mut results = Vec::new();

        if let Some(score) = score_content(query, &memory_document.content) {
            results.push(RetrievedDocument {
                collection_id: self.memory_collection.collection_id.clone(),
                kind: RetrievalCollectionKind::Memory,
                document_ref: memory_document.path.display().to_string(),
                title: file_title(&memory_document.path),
                content: compact_excerpt(&memory_document.content, query),
                score,
            });
        }

        results.extend(self.search_collection(&self.memory_collection, query, limit)?);
        results.sort_by(|left, right| right.score.cmp(&left.score));
        results.truncate(limit);
        Ok(results)
    }

    pub fn retrieve_knowledge(&self, query: &str, limit: usize) -> Result<Vec<RetrievedDocument>> {
        self.search_collection(&self.knowledge_collection, query, limit)
    }

    fn search_collection(
        &self,
        collection: &RetrievalCollection,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RetrievedDocument>> {
        let mut results = Vec::new();
        if query.trim().is_empty() || !collection.root.exists() {
            return Ok(results);
        }

        for path in discover_text_files(&collection.root)? {
            let content = fs::read_to_string(&path)?;
            let Some(score) = score_content(query, &content) else {
                continue;
            };
            results.push(RetrievedDocument {
                collection_id: collection.collection_id.clone(),
                kind: collection.kind.clone(),
                document_ref: path.display().to_string(),
                title: file_title(&path),
                content: compact_excerpt(&content, query),
                score,
            });
        }

        results.sort_by(|left, right| right.score.cmp(&left.score));
        results.truncate(limit);
        Ok(results)
    }
}

#[async_trait]
impl Plugin for RetrievalBridgeContextPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "context.workspace.retrieval_bridge".into(),
            version: "0.1.0".into(),
            capabilities: vec![
                PluginCapability::Context(ContextCapability::BlockGenerator),
                PluginCapability::Memory(MemoryCapability::Recall),
                PluginCapability::Knowledge(KnowledgeCapability::RetrievalPolicy),
            ],
            config_schema: None,
            required_permissions: vec![Permission::ReadWorkspace],
            dependencies: Vec::new(),
            optional_dependencies: Vec::new(),
            provided_resources: Vec::new(),
            hooks: Vec::new(),
        }
    }
}

impl ContextPlugin for RetrievalBridgeContextPlugin {
    fn collections(&self) -> Vec<String> {
        RetrievalBridgeContextPlugin::collections(self)
            .into_iter()
            .map(|collection| collection.collection_id)
            .collect()
    }
}

fn discover_text_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(discover_text_files(&path)?);
            continue;
        }
        if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("md" | "txt")
        ) {
            files.push(path);
        }
    }

    Ok(files)
}

fn score_content(query: &str, content: &str) -> Option<u32> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return None;
    }

    let haystack = content.to_lowercase();
    let phrase_hits = haystack.matches(&normalized_query).count() as u32;
    let term_hits: u32 = normalized_query
        .split_whitespace()
        .map(|term| haystack.matches(term).count() as u32)
        .sum();

    let score = phrase_hits * 10 + term_hits;
    if score == 0 {
        None
    } else {
        Some(score)
    }
}

fn compact_excerpt(content: &str, query: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized_query = query.to_lowercase();
    let normalized_content = trimmed.to_lowercase();
    if let Some(index) = normalized_content.find(&normalized_query) {
        let start = index.saturating_sub(80);
        let end = (index + normalized_query.len() + 160).min(trimmed.len());
        return trimmed[start..end].trim().to_string();
    }

    trimmed.lines().take(8).collect::<Vec<_>>().join("\n")
}

fn file_title(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{RetrievalBridgeContextPlugin, RetrievalCollectionKind};
    use crate::{
        config::{WorkspaceDocument, WorkspacePaths},
        core::{ContextPlugin, Plugin},
    };

    #[test]
    fn distinguishes_memory_recall_from_knowledge_retrieval() {
        let root = temp_path("retrieval-bridge");
        let paths = WorkspacePaths::new(&root);
        fs::create_dir_all(&paths.memory_topics_dir).unwrap();
        fs::create_dir_all(&paths.knowledge_dir).unwrap();
        fs::write(
            paths.memory_topics_dir.join("project-alpha.md"),
            "Project Alpha prefers Rust for tools.",
        )
        .unwrap();
        fs::write(
            paths.knowledge_dir.join("api-guide.md"),
            "The project API exposes a /health endpoint for checks.",
        )
        .unwrap();

        let bridge = RetrievalBridgeContextPlugin::new(&paths);
        let memory = bridge
            .recall_memory(
                &WorkspaceDocument {
                    path: paths.memory_file.clone(),
                    content: "Stable fact: user prefers Rust.".into(),
                },
                "Rust project",
                4,
            )
            .unwrap();
        let knowledge = bridge.retrieve_knowledge("health endpoint", 4).unwrap();

        assert!(memory
            .iter()
            .all(|item| item.kind == RetrievalCollectionKind::Memory));
        assert!(knowledge
            .iter()
            .all(|item| item.kind == RetrievalCollectionKind::Knowledge));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retrieval_bridge_exposes_context_plugin_manifest_and_collections() {
        let root = temp_path("retrieval-bridge-plugin");
        let paths = WorkspacePaths::new(&root);
        let bridge = RetrievalBridgeContextPlugin::new(&paths);

        assert_eq!(bridge.manifest().id, "context.workspace.retrieval_bridge");
        assert_eq!(
            ContextPlugin::collections(&bridge),
            vec!["memory".to_string(), "knowledge".to_string()]
        );
    }

    fn temp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("agentjax-{prefix}-{nanos}"))
    }
}
