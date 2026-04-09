use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpansionResult {
    pub matched_refs: Vec<String>,
    pub distilled_text: String,
}
pub trait HistoryExpander: Send + Sync {
    fn grep_history(&self, query: &str) -> ExpansionResult;
    fn describe_object(&self, object_ref: &str) -> ExpansionResult;
}
#[derive(Debug, Clone, Default)]
pub struct NoopHistoryExpander;
impl HistoryExpander for NoopHistoryExpander {
    fn grep_history(&self, query: &str) -> ExpansionResult {
        ExpansionResult {
            matched_refs: Vec::new(),
            distilled_text: format!("no history matches for {query}"),
        }
    }
    fn describe_object(&self, object_ref: &str) -> ExpansionResult {
        ExpansionResult {
            matched_refs: vec![object_ref.to_string()],
            distilled_text: format!("no description available for {object_ref}"),
        }
    }
}
