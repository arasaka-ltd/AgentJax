#[derive(Debug, Clone, Default)]
pub struct ContextEngineSchema {
    pub event_schema_version: String,
    pub projection_schema_version: String,
    pub summary_schema_version: String,
    pub resume_schema_version: String,
    pub checkpoint_schema_version: String,
}
