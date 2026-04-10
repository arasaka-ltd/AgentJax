use crate::domain::ResumePack;
pub trait ResumeBuilder: Send + Sync {
    fn build_resume_pack(&self) -> ResumePack;
}
#[derive(Debug, Clone, Default)]
pub struct NoopResumeBuilder;
impl ResumeBuilder for NoopResumeBuilder {
    fn build_resume_pack(&self) -> ResumePack {
        ResumePack {
            workspace_id: None,
            session_id: None,
            task_id: None,
            mission_ref: None,
            active_task_ids: Vec::new(),
            latest_checkpoint_summary_id: None,
            summary_node_ids: Vec::new(),
            open_blockers: Vec::new(),
            pending_artifact_ids: Vec::new(),
            last_safe_action_boundary: None,
            next_recommended_action: None,
            assumptions: Vec::new(),
            risks: Vec::new(),
        }
    }
}
