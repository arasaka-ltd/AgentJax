use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::{WorkspaceDocument, WorkspacePaths};
use crate::{
    builtin::context::retrieval_types::{
        chunk_ref_for, doc_ref_for, memory_ref_for, KnowledgeGetRequest, KnowledgeSearchRequest,
        MemoryGetRequest, MemorySearchRequest, MemorySearchScope, RetrievalDocument,
        RetrievalDocumentKind, RetrievedContent,
    },
    core::{ContextPlugin, Plugin},
    domain::{
        ContextCapability, KnowledgeCapability, MemoryCapability, Permission, PluginCapability,
        PluginManifest,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalCollection {
    pub collection_id: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RetrievalBridgeContextPlugin {
    paths: WorkspacePaths,
    memory_collection: RetrievalCollection,
    knowledge_collection: RetrievalCollection,
}

impl RetrievalBridgeContextPlugin {
    pub fn new(paths: &WorkspacePaths) -> Self {
        Self {
            paths: paths.clone(),
            memory_collection: RetrievalCollection {
                collection_id: "memory".into(),
                root: paths.memory_dir.clone(),
            },
            knowledge_collection: RetrievalCollection {
                collection_id: "knowledge".into(),
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

    pub fn search_memory(
        &self,
        memory_document: &WorkspaceDocument,
        request: &MemorySearchRequest,
    ) -> Result<Vec<RetrievalDocument>> {
        let mut results = Vec::new();
        if matches!(
            request.scope,
            MemorySearchScope::MemoryMd | MemorySearchScope::All
        ) {
            if let Some(score) = score_content(&request.query, &memory_document.content) {
                results.push(RetrievalDocument {
                    kind: RetrievalDocumentKind::Memory,
                    stable_ref: memory_ref_for("memory_md", "memory"),
                    chunk_ref: Some(chunk_ref_for("memory", "memory", "1")),
                    library: None,
                    path: memory_document.path.display().to_string(),
                    title: "MEMORY".into(),
                    excerpt: compact_excerpt(&memory_document.content, &request.query),
                    score,
                    line_start: Some(1),
                    line_end: Some(memory_document.content.lines().count().max(1)),
                    metadata: Some(json!({ "scope": "memory_md" })),
                });
            }
        }

        let scoped_root = match request.scope {
            MemorySearchScope::Topics => self.paths.memory_topics_dir.clone(),
            MemorySearchScope::Profiles => self.paths.memory_profiles_dir.clone(),
            MemorySearchScope::Daily => self.paths.memory_daily_dir.clone(),
            _ => self.paths.memory_dir.clone(),
        };
        results.extend(self.search_memory_paths(&scoped_root, request)?);
        results.sort_by(|left, right| right.score.cmp(&left.score));
        results.truncate(request.top_k.max(1));
        Ok(results)
    }

    pub fn get_memory(&self, request: &MemoryGetRequest) -> Result<RetrievedContent> {
        let path = if let Some(memory_ref) = &request.memory_ref {
            self.resolve_memory_ref(memory_ref)?
        } else if let Some(path) = &request.path {
            PathBuf::from(path)
        } else {
            bail!("memory_get requires memory_ref or path");
        };
        let stable_ref = request
            .memory_ref
            .clone()
            .unwrap_or_else(|| memory_ref_for("path", &path.display().to_string()));
        self.get_content(
            path,
            stable_ref,
            None,
            request.start_line,
            request.end_line,
            request.max_lines,
        )
    }

    pub fn search_knowledge(
        &self,
        request: &KnowledgeSearchRequest,
    ) -> Result<Vec<RetrievalDocument>> {
        let mut roots = Vec::new();
        if let Some(library) = &request.library {
            roots.push((
                Some(library.clone()),
                self.paths.knowledge_dir.join(library),
            ));
        }
        for library in &request.libraries {
            roots.push((
                Some(library.clone()),
                self.paths.knowledge_dir.join(library),
            ));
        }
        if roots.is_empty() {
            roots.push((None, self.paths.knowledge_dir.clone()));
        }

        let mut results = Vec::new();
        for (library, root) in roots {
            results.extend(self.search_knowledge_paths(&root, library.as_deref(), request)?);
        }
        results.sort_by(|left, right| right.score.cmp(&left.score));
        results.truncate(request.top_k.max(1));
        Ok(results)
    }

    pub fn get_knowledge(&self, request: &KnowledgeGetRequest) -> Result<RetrievedContent> {
        let (path, stable_ref, library) = if let Some(doc_ref) = &request.doc_ref {
            self.resolve_doc_ref(doc_ref)?
        } else if let Some(path) = &request.path {
            (
                PathBuf::from(path),
                doc_ref_for(request.library.as_deref().unwrap_or("workspace"), path),
                request.library.clone(),
            )
        } else {
            bail!("knowledge_get requires doc_ref or path");
        };
        self.get_content(
            path,
            stable_ref,
            library,
            request.start_line,
            request.end_line,
            request.max_lines,
        )
    }

    fn search_memory_paths(
        &self,
        root: &Path,
        request: &MemorySearchRequest,
    ) -> Result<Vec<RetrievalDocument>> {
        let mut results = Vec::new();
        if request.query.trim().is_empty() || !root.exists() {
            return Ok(results);
        }

        for path in discover_text_files(root)? {
            let content = fs::read_to_string(&path)?;
            let Some(score) = score_content(&request.query, &content) else {
                continue;
            };
            let scope = memory_scope_for_path(&self.paths, &path);
            let slug = memory_slug_for_path(&self.paths, &path);
            results.push(RetrievalDocument {
                kind: RetrievalDocumentKind::Memory,
                stable_ref: memory_ref_for(scope, &slug),
                chunk_ref: Some(chunk_ref_for("memory", &slug, "1")),
                library: None,
                path: path.display().to_string(),
                title: file_title(&path),
                excerpt: compact_excerpt(&content, &request.query),
                score,
                line_start: Some(1),
                line_end: Some(content.lines().count().max(1)),
                metadata: Some(json!({ "scope": scope })),
            });
        }

        Ok(results)
    }

    fn search_knowledge_paths(
        &self,
        root: &Path,
        library: Option<&str>,
        request: &KnowledgeSearchRequest,
    ) -> Result<Vec<RetrievalDocument>> {
        let mut results = Vec::new();
        if request.query.trim().is_empty() || !root.exists() {
            return Ok(results);
        }

        for path in discover_text_files(root)? {
            let relative = path
                .strip_prefix(&self.paths.knowledge_dir)
                .unwrap_or(&path);
            let relative_str = relative.display().to_string();
            if let Some(prefix) = &request.path_prefix {
                if !relative_str.contains(prefix) {
                    continue;
                }
            }
            let content = fs::read_to_string(&path)?;
            let Some(score) = score_content(&request.query, &content) else {
                continue;
            };
            let resolved_library = library.map(str::to_string).or_else(|| {
                relative
                    .iter()
                    .next()
                    .map(|part| part.to_string_lossy().to_string())
            });
            let doc_id = relative_str
                .trim_end_matches(".md")
                .trim_end_matches(".txt");
            results.push(RetrievalDocument {
                kind: RetrievalDocumentKind::Knowledge,
                stable_ref: doc_ref_for(resolved_library.as_deref().unwrap_or("workspace"), doc_id),
                chunk_ref: Some(chunk_ref_for(
                    resolved_library.as_deref().unwrap_or("workspace"),
                    doc_id,
                    "1",
                )),
                library: resolved_library.clone(),
                path: path.display().to_string(),
                title: file_title(&path),
                excerpt: compact_excerpt(&content, &request.query),
                score,
                line_start: Some(1),
                line_end: Some(content.lines().count().max(1)),
                metadata: Some(json!({ "library": resolved_library, "path": relative_str })),
            });
        }

        Ok(results)
    }

    fn get_content(
        &self,
        path: PathBuf,
        stable_ref: String,
        library: Option<String>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        max_lines: Option<usize>,
    ) -> Result<RetrievedContent> {
        let content = fs::read_to_string(&path)?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len().max(1);
        let start = start_line.unwrap_or(1).max(1).min(total_lines);
        let mut end = end_line.unwrap_or(total_lines).max(start).min(total_lines);
        if end_line.is_none() {
            if let Some(max_lines) = max_lines {
                end = (start + max_lines.saturating_sub(1)).min(total_lines);
            }
        }
        let rendered = lines
            .iter()
            .enumerate()
            .skip(start - 1)
            .take(end - start + 1)
            .map(|(idx, line)| format!("{}| {}", idx + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(RetrievedContent {
            stable_ref,
            chunk_ref: Some(chunk_ref_for(
                library.as_deref().unwrap_or("memory"),
                &file_title(&path),
                &format!("{start}-{end}"),
            )),
            path: path.display().to_string(),
            title: file_title(&path),
            library,
            content: rendered,
            start_line: start,
            end_line: end,
            total_lines,
            truncated: end < total_lines,
        })
    }

    fn resolve_memory_ref(&self, memory_ref: &str) -> Result<PathBuf> {
        let Some(rest) = memory_ref.strip_prefix("mem:") else {
            bail!("invalid memory_ref: {memory_ref}");
        };
        let mut parts = rest.splitn(2, '/');
        let scope = parts.next().unwrap_or_default();
        let slug = parts.next().unwrap_or_default();
        let path = match scope {
            "memory_md" => self.paths.memory_file.clone(),
            "topics" => self.paths.memory_topics_dir.join(format!("{slug}.md")),
            "profiles" => self.paths.memory_profiles_dir.join(format!("{slug}.md")),
            "daily" => self.paths.memory_daily_dir.join(format!("{slug}.md")),
            _ => PathBuf::from(slug),
        };
        if path.exists() {
            Ok(path)
        } else {
            Err(anyhow!("memory_ref not found: {memory_ref}"))
        }
    }

    fn resolve_doc_ref(&self, doc_ref: &str) -> Result<(PathBuf, String, Option<String>)> {
        let Some(rest) = doc_ref.strip_prefix("doc:") else {
            bail!("invalid doc_ref: {doc_ref}");
        };
        let mut parts = rest.splitn(2, '/');
        let library = parts.next().unwrap_or("workspace").to_string();
        let doc_id = parts.next().unwrap_or_default();
        let path = self.paths.knowledge_dir.join(format!("{doc_id}.md"));
        if path.exists() {
            Ok((path, doc_ref.to_string(), Some(library)))
        } else {
            Err(anyhow!("doc_ref not found: {doc_ref}"))
        }
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
        let prefix_chars = normalized_content[..index].chars().count();
        let match_chars = normalized_query.chars().count();
        let start_char = prefix_chars.saturating_sub(80);
        let end_char = (prefix_chars + match_chars + 160).min(trimmed.chars().count());
        let excerpt = trimmed
            .chars()
            .skip(start_char)
            .take(end_char.saturating_sub(start_char))
            .collect::<String>();
        return excerpt.trim().to_string();
    }

    trimmed.lines().take(8).collect::<Vec<_>>().join("\n")
}

fn file_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("document")
        .to_string()
}

fn memory_scope_for_path(paths: &WorkspacePaths, path: &Path) -> &'static str {
    if path.starts_with(&paths.memory_topics_dir) {
        "topics"
    } else if path.starts_with(&paths.memory_profiles_dir) {
        "profiles"
    } else if path.starts_with(&paths.memory_daily_dir) {
        "daily"
    } else {
        "all"
    }
}

fn memory_slug_for_path(paths: &WorkspacePaths, path: &Path) -> String {
    let base = if path.starts_with(&paths.memory_topics_dir) {
        &paths.memory_topics_dir
    } else if path.starts_with(&paths.memory_profiles_dir) {
        &paths.memory_profiles_dir
    } else if path.starts_with(&paths.memory_daily_dir) {
        &paths.memory_daily_dir
    } else {
        &paths.memory_dir
    };
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
        .trim_end_matches(".md")
        .trim_end_matches(".txt")
        .to_string()
}
