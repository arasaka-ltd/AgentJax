use serde::{Deserialize, Serialize};

use crate::{
    builtin::context::retrieval_types::RetrievalScope,
    domain::{ContextAssemblyPurpose, ContextBlock},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextAssemblyRequest {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub budget_tokens: u32,
    pub purpose: ContextAssemblyPurpose,
    pub model_profile: Option<String>,
    pub retrieval_scope: RetrievalScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TokenBreakdown {
    pub total: u32,
    pub stable_docs: u32,
    pub runtime: u32,
    pub summaries: u32,
    pub fresh_tail: u32,
    pub retrieval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AssembledContext {
    pub blocks: Vec<ContextBlock>,
    pub token_breakdown: TokenBreakdown,
    pub included_refs: Vec<String>,
    pub omitted_refs: Vec<String>,
    pub system_prompt_additions: Vec<String>,
}
