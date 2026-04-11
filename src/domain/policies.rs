use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AutonomyPolicy {
    pub may_send_messages: bool,
    pub may_spend_budget: bool,
    pub may_create_tasks: bool,
    pub may_install_plugins: bool,
    pub may_modify_config: bool,
    pub may_modify_memory: bool,
    pub may_modify_rules: bool,
    pub may_modify_mission: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionMode {
    EphemeralSession,
    BoundSession,
    HeadlessTask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackoffStrategy {
    Fixed,
    Exponential,
    ExponentialWithJitter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff: BackoffStrategy,
    pub retryable_categories: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 250,
            max_delay_ms: 5_000,
            backoff: BackoffStrategy::ExponentialWithJitter,
            retryable_categories: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BreakerStatus {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CircuitBreakerState {
    pub breaker_id: String,
    pub status: BreakerStatus,
    pub opened_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
    pub failure_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextBudgetPolicy {
    pub total_budget_tokens: u32,
    pub stable_docs_budget: u32,
    pub mission_rules_budget: u32,
    pub current_task_budget: u32,
    pub recent_events_budget: u32,
    pub lcm_expansion_budget: u32,
    pub retrieved_memory_budget: u32,
    pub tool_traces_budget: u32,
}

impl Default for ContextBudgetPolicy {
    fn default() -> Self {
        Self {
            total_budget_tokens: 16_000,
            stable_docs_budget: 3_000,
            mission_rules_budget: 2_000,
            current_task_budget: 2_000,
            recent_events_budget: 3_000,
            lcm_expansion_budget: 3_000,
            retrieved_memory_budget: 2_000,
            tool_traces_budget: 1_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPlanPolicy {
    pub allow_multistep: bool,
    pub allow_interruption: bool,
    pub allow_resume: bool,
    pub allow_delegation: bool,
    pub allow_parallelism: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRoutingPolicy {
    pub fast_model: String,
    pub deep_model: String,
    pub cheap_model: String,
    pub compression_model: String,
    pub tool_calling_model: String,
    pub summarization_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillTriggerMode {
    RuleBased,
    SemanticMatch,
    ExplicitUserRequest,
    TaskPolicy,
    PluginRecommendation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextAssemblyPurpose {
    Chat,
    Planning,
    Execution,
    Summarization,
    Resume,
}
