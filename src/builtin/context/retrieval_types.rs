use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetrievalScope {
    Disabled,
    Implicit,
    ExplicitOnly,
}

impl Default for RetrievalScope {
    fn default() -> Self {
        Self::Implicit
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetrievalMode {
    Keyword,
    Semantic,
    Hybrid,
}

impl RetrievalMode {
    pub fn from_str(value: &str) -> Self {
        match value {
            "semantic" => Self::Semantic,
            "hybrid" => Self::Hybrid,
            _ => Self::Keyword,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemorySearchScope {
    MemoryMd,
    Topics,
    Profiles,
    Daily,
    All,
}

impl MemorySearchScope {
    pub fn from_str(value: &str) -> Self {
        match value {
            "memory_md" => Self::MemoryMd,
            "topics" => Self::Topics,
            "profiles" => Self::Profiles,
            "daily" => Self::Daily,
            _ => Self::All,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemorySearchRequest {
    pub query: String,
    pub top_k: usize,
    pub scope: MemorySearchScope,
    pub mode: RetrievalMode,
    pub include_excerpt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryGetRequest {
    pub memory_ref: Option<String>,
    pub path: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub max_lines: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeSearchRequest {
    pub query: String,
    pub top_k: usize,
    pub library: Option<String>,
    pub libraries: Vec<String>,
    pub path_prefix: Option<String>,
    pub mode: RetrievalMode,
    pub metadata_filters: Option<Value>,
    pub include_excerpt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeGetRequest {
    pub doc_ref: Option<String>,
    pub path: Option<String>,
    pub library: Option<String>,
    pub chunk_ref: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub max_lines: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetrievalDocumentKind {
    Memory,
    Knowledge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievalDocument {
    pub kind: RetrievalDocumentKind,
    pub stable_ref: String,
    pub chunk_ref: Option<String>,
    pub library: Option<String>,
    pub path: String,
    pub title: String,
    pub excerpt: String,
    pub score: u32,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetrievedContent {
    pub stable_ref: String,
    pub chunk_ref: Option<String>,
    pub path: String,
    pub title: String,
    pub library: Option<String>,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub truncated: bool,
}

pub fn memory_ref_for(scope: &str, slug: &str) -> String {
    format!("mem:{scope}/{slug}")
}

pub fn doc_ref_for(library: &str, doc_id: &str) -> String {
    format!("doc:{library}/{doc_id}")
}

pub fn chunk_ref_for(library: &str, doc_id: &str, chunk_id: &str) -> String {
    format!("chunk:{library}/{doc_id}/{chunk_id}")
}
