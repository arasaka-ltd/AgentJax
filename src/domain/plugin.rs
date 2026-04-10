use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::ResourceId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginCapability {
    Provider(ProviderCapability),
    Rag(RagCapability),
    Knowledge(KnowledgeCapability),
    Billing(BillingCapability),
    Memory(MemoryCapability),
    Context(ContextCapability),
    Tool(ToolCapability),
    Channel(ChannelCapability),
    Node(NodeCapability),
    Skill(SkillCapability),
    Command(CommandCapability),
    Hook(HookCapability),
    Ui(UiCapability),
    Workflow(WorkflowCapability),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderCapability {
    LlmText,
    LlmReasoning,
    Embedding,
    Reranker,
    Tts,
    St,
    ImageGeneration,
    VideoGeneration,
    MusicGeneration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RagCapability {
    Query,
    Indexing,
    BackendDriver,
    EvidencePack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum KnowledgeCapability {
    Corpus,
    IngestPolicy,
    RetrievalPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BillingCapability {
    UsageMeter,
    CostEstimator,
    ProviderReportedBilling,
    Reconciliation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryCapability {
    Recall,
    Promotion,
    ConflictResolution,
    FreshnessPolicy,
    Archive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextCapability {
    BlockGenerator,
    Selector,
    PromptRenderer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolCapability {
    Tool,
    Executor,
    McpBridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelCapability {
    Telegram,
    Discord,
    Qq,
    Email,
    Slack,
    Webhook,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeCapability {
    RemoteWorker,
    MachineNode,
    DeviceNode,
    BrowserNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCapability {
    SkillManifest,
    SkillLoader,
    TriggerRouter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandCapability {
    CliCommand,
    AdminOperation,
    Diagnostic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookCapability {
    Lifecycle,
    EventSubscription,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UiCapability {
    DashboardPane,
    Inspector,
    DebugView,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowCapability {
    Scheduler,
    Automation,
    RecurringJob,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Permission {
    ReadWorkspace,
    WriteWorkspace,
    ReadState,
    WriteState,
    EmitEvents,
    UseResource(ResourceId),
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookPoint {
    OnLoad,
    OnStartup,
    OnShutdown,
    OnConfigChanged,
    BeforeTurn,
    AfterTurn,
    BeforeModelRequest,
    AfterModelResponse,
    BeforeToolCall,
    AfterToolCall,
    BeforeContextBuild,
    AfterContextBuild,
    BeforeMemoryCommit,
    AfterMemoryCommit,
    OnMessage,
    OnTaskCreated,
    OnTaskStarted,
    OnTaskFailed,
    OnTaskSucceeded,
    OnScheduleTick,
    OnArtifactCreated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookRegistration {
    pub plugin_id: String,
    pub point: HookPoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookInput {
    pub point: HookPoint,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginStatus {
    Loaded,
    Initialized,
    Running,
    Draining,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceDescriptor {
    pub resource_id: ResourceId,
    pub kind: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub capabilities: Vec<PluginCapability>,
    pub config_schema: Option<serde_json::Value>,
    pub required_permissions: Vec<Permission>,
    pub dependencies: Vec<String>,
    pub optional_dependencies: Vec<String>,
    pub provided_resources: Vec<ResourceDescriptor>,
    pub hooks: Vec<HookPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub plugin_id: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub api_version: String,
    pub status: PluginStatus,
}
