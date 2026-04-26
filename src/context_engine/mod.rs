pub mod assembler;
pub mod compactor;
pub mod engine;
pub mod event_store;
pub mod expander;
pub mod projection_store;
pub mod prompt;
pub mod resume;
pub mod schema;

pub use assembler::{AssembledContext, ContextAssemblyRequest, TokenBreakdown};
pub use compactor::{CompactionDecision, CompactionEvaluator};
pub use engine::{ContextEngine, NoopContextEngine, WorkspaceContextEngine};
pub use event_store::EventStore;
pub use expander::HistoryExpander;
pub use projection_store::ProjectionStore;
pub use prompt::{
    PromptDocument, PromptFragment, PromptRenderRequest, PromptRolePayload, PromptSection,
    parse_workspace_prompt_documents, render_prompt_role_payload, render_prompt_xml,
};
pub use resume::ResumeBuilder;
pub use schema::ContextEngineSchema;
