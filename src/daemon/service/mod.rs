mod helpers;
mod request_handlers;
mod runtime_support;
mod session;
mod tool_loop;

use self::{helpers::*, tool_loop::*};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use chrono::Utc;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::{
    api::{
        AgentGetRequest, AgentGetResponse, AgentListItem, AgentListResponse, ApiError,
        ApiErrorCode, ApiMethod, ClientEnvelope, ConfigInspectRequest, ConfigInspectResponse,
        ConfigReloadResponse, ConfigValidateResponse, ConnectionId, DoctorCheckResult,
        DoctorRunResponse, HelloAckEnvelope, LogsTailRequest, MetricsSnapshotResponse,
        NodeGetRequest, NodeGetResponse, NodeListItem, NodeListResponse, PluginInspectRequest,
        PluginInspectResponse, PluginListItem, PluginListResponse, PluginReloadRequest,
        PluginReloadResponse, PluginTestRequest, PluginTestResponse, RequestEnvelope,
        ResponseEnvelope, RuntimePingResponse, RuntimeShutdownRequest, RuntimeShutdownResponse,
        RuntimeStatusResponse, ScheduleCreateRequest, ScheduleDeleteRequest, ScheduleGetResponse,
        ScheduleListItem, ScheduleListResponse, ScheduleUpdateRequest, ServerEnvelope,
        SessionCancelRequest, SessionGetRequest, SessionGetResponse, SessionListItem,
        SessionListResponse, SessionMessage, SessionMessageAnnotation, SessionModelInspectRequest,
        SessionModelInspectResponse, SessionModelState, SessionModelSwitchRequest,
        SessionModelSwitchResponse, SessionModelSwitchResult, SessionSendRequest,
        SessionSendResponse, SessionSubscribeRequest, SmokeRunRequest, SmokeRunResponse,
        StreamCancelRequest, StreamEnvelope, StreamPhase, SubscriptionCancelRequest,
        SubscriptionResponse, TaskCancelRequest, TaskGetRequest, TaskGetResponse, TaskListItem,
        TaskListResponse, TaskRetryRequest, TaskSubscribeRequest,
    },
    app::Application,
    context_engine::{
        parse_workspace_prompt_documents, render_prompt_xml, AssembledContext,
        ContextAssemblyRequest, PromptRenderRequest,
    },
    core::AgentPromptRequest,
    daemon::store::DaemonStore,
    domain::{
        Agent, AgentStatus, AutonomyPolicy, ContextAssemblyPurpose, EventType, ExecutionMode,
        ModelOutputItem, ModelTurnOutput, NodeSelector, ObjectMeta, PluginDescriptor, PluginStatus,
        Schedule, Session, SessionModelTarget, Task, TaskStatus, ToolCall, ToolCallItem,
        ToolCaller, ToolResultItem, TrustLevel,
    },
};

pub const API_VERSION: &str = "v1";
pub const SCHEMA_VERSION: &str = "2026-04-10";
const MAX_TOOL_LOOP_STEPS: usize = 8;

#[derive(Clone)]
pub struct Daemon {
    app: Arc<Application>,
    store: Arc<DaemonStore>,
    control: Arc<Mutex<ControlPlaneState>>,
}

#[derive(Default)]
struct ControlPlaneState {
    subscriptions: BTreeMap<String, RegisteredSubscription>,
    streams: BTreeMap<String, RegisteredStream>,
    logs: Vec<String>,
    resuming_tasks: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct RegisteredSubscription {
    _kind: &'static str,
    _target_id: String,
    _accepted_events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamStatus {
    Active,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone)]
struct RegisteredStream {
    status: StreamStatus,
    _source: &'static str,
}

pub struct Dispatch {
    pub response: ServerEnvelope,
    pub followups: Vec<ServerEnvelope>,
    pub live_stream: Option<UnboundedReceiver<ServerEnvelope>>,
}

impl Dispatch {
    pub fn single(response: ServerEnvelope) -> Self {
        Self {
            response,
            followups: Vec::new(),
            live_stream: None,
        }
    }
}

impl Daemon {
    pub fn new(app: Application) -> anyhow::Result<Self> {
        let runtime_config = app.runtime_config.clone();
        let daemon = Self {
            app: Arc::new(app),
            store: Arc::new(DaemonStore::new(runtime_config)?),
            control: Arc::new(Mutex::new(ControlPlaneState::default())),
        };
        daemon.spawn_waiting_task_scheduler();
        daemon.spawn_schedule_executor();
        Ok(daemon)
    }

    pub fn connection_id(&self) -> ConnectionId {
        ConnectionId(self.store.next_connection_id())
    }

    pub fn hello_ack(&self, connection_id: ConnectionId) -> HelloAckEnvelope {
        HelloAckEnvelope::new(connection_id, API_VERSION)
    }

    pub async fn handle_client_envelope(
        &self,
        envelope: ClientEnvelope,
    ) -> Result<Dispatch, ApiError> {
        match envelope {
            ClientEnvelope::Hello(_) => Err(ApiError::new(
                ApiErrorCode::ProtocolViolation,
                "hello is only valid during handshake",
                false,
            )),
            ClientEnvelope::Request(request) => Ok(self.handle_request(request).await),
        }
    }

    pub async fn handle_request(&self, request: RequestEnvelope) -> Dispatch {
        if matches!(request.method, ApiMethod::SessionSend) {
            return self.handle_session_send_dispatch(request).await;
        }
        match self.route_request(&request).await {
            Ok((result, followups)) => Dispatch {
                response: ServerEnvelope::Response(ResponseEnvelope::ok(request.id, result)),
                followups,
                live_stream: None,
            },
            Err(error) => Dispatch::single(ServerEnvelope::Response(ResponseEnvelope::err(
                request.id, error,
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde::de::DeserializeOwned;
    use serde_json::json;

    use crate::{
        api::{
            ApiMethod, LogsTailRequest, RequestEnvelope, RequestId, ScheduleCreateRequest,
            ScheduleDeleteRequest, ScheduleGetResponse, ScheduleListResponse,
            ScheduleUpdateRequest, SessionSendRequest, SessionSendResponse,
            SessionSubscribeRequest, StreamCancelRequest, SubscriptionCancelRequest,
            SubscriptionResponse, TaskListResponse, TaskSubscribeRequest,
        },
        domain::{ObjectMeta, Schedule, TaskTarget, TaskTrigger},
        test_support::TestHarness,
    };

    #[tokio::test]
    async fn scheduler_and_node_requests_round_trip() {
        let harness = TestHarness::new("scheduler-node");
        let daemon = harness.daemon.clone();
        let schedule = sample_schedule("schedule.test");

        let created_dispatch = daemon
            .handle_request(request(
                "req_schedule_create",
                ApiMethod::ScheduleCreate,
                ScheduleCreateRequest {
                    schedule: schedule.clone(),
                },
            ))
            .await;
        let created: ScheduleGetResponse = ok_result(&created_dispatch.response);
        assert_eq!(created.schedule.schedule_id, schedule.schedule_id);

        let listed_dispatch = daemon
            .handle_request(request(
                "req_schedule_list",
                ApiMethod::ScheduleList,
                json!({}),
            ))
            .await;
        let listed: ScheduleListResponse = ok_result(&listed_dispatch.response);
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].schedule_id, schedule.schedule_id);
        assert_eq!(listed.items[0].kind, "interval");

        let node_list_dispatch = daemon
            .handle_request(request("req_node_list", ApiMethod::NodeList, json!({})))
            .await;
        let nodes: crate::api::NodeListResponse = ok_result(&node_list_dispatch.response);
        assert_eq!(nodes.items.len(), 1);
        assert_eq!(nodes.items[0].node_id, "node.local");
        assert!(nodes.items[0]
            .capabilities
            .iter()
            .any(|capability| capability == "session.interaction"));

        let node_get_dispatch = daemon
            .handle_request(request(
                "req_node_get",
                ApiMethod::NodeGet,
                json!({ "node_id": "node.local" }),
            ))
            .await;
        let node: crate::api::NodeGetResponse = ok_result(&node_get_dispatch.response);
        assert_eq!(node.node.node_id, "node.local");

        let mut updated_schedule = schedule.clone();
        updated_schedule.enabled = false;
        let updated_dispatch = daemon
            .handle_request(request(
                "req_schedule_update",
                ApiMethod::ScheduleUpdate,
                ScheduleUpdateRequest {
                    schedule: updated_schedule.clone(),
                },
            ))
            .await;
        let updated: ScheduleGetResponse = ok_result(&updated_dispatch.response);
        assert!(!updated.schedule.enabled);

        let deleted_dispatch = daemon
            .handle_request(request(
                "req_schedule_delete",
                ApiMethod::ScheduleDelete,
                ScheduleDeleteRequest {
                    schedule_id: updated_schedule.schedule_id,
                },
            ))
            .await;
        let deleted: serde_json::Value = ok_result(&deleted_dispatch.response);
        assert_eq!(deleted["accepted"], true);
    }

    #[tokio::test]
    async fn followup_streams_and_cancellations_are_tracked() {
        let harness = TestHarness::new("followups-cancel");
        let daemon = harness.daemon.clone();

        let session_sub_dispatch = daemon
            .handle_request(request(
                "req_session_subscribe",
                ApiMethod::SessionSubscribe,
                SessionSubscribeRequest {
                    session_id: "session.default".into(),
                    events: vec!["session.updated".into()],
                },
            ))
            .await;
        let session_subscription: SubscriptionResponse = ok_result(&session_sub_dispatch.response);
        assert!(daemon
            .control
            .lock()
            .expect("control lock poisoned")
            .subscriptions
            .contains_key(session_subscription.subscription_id.0.as_str()));

        let session_send_dispatch = daemon
            .handle_request(request(
                "req_session_send",
                ApiMethod::SessionSend,
                SessionSendRequest {
                    session_id: "session.default".into(),
                    message: crate::api::SessionMessage::user("create a task"),
                    stream: false,
                },
            ))
            .await;
        let _: SessionSendResponse = ok_result(&session_send_dispatch.response);
        let task_list_dispatch = daemon
            .handle_request(request("req_task_list", ApiMethod::TaskList, json!({})))
            .await;
        let task_list: TaskListResponse = ok_result(&task_list_dispatch.response);
        let task_id = task_list
            .items
            .last()
            .expect("expected task to be created")
            .task_id
            .clone();

        let task_sub_dispatch = daemon
            .handle_request(request(
                "req_task_subscribe",
                ApiMethod::TaskSubscribe,
                TaskSubscribeRequest {
                    task_id,
                    events: vec!["task.updated".into()],
                },
            ))
            .await;
        let task_subscription: SubscriptionResponse = ok_result(&task_sub_dispatch.response);

        let logs_dispatch = daemon
            .handle_request(request(
                "req_logs_tail",
                ApiMethod::LogsTail,
                LogsTailRequest {
                    stream: true,
                    level: None,
                },
            ))
            .await;
        let logs_response: serde_json::Value = ok_result(&logs_dispatch.response);
        let stream_id = logs_response["stream_id"]
            .as_str()
            .expect("logs.tail missing stream_id")
            .to_owned();
        assert!(!logs_dispatch.followups.is_empty());
        assert!(matches!(
            logs_dispatch.followups.first(),
            Some(ServerEnvelope::Stream(stream)) if matches!(stream.phase, StreamPhase::Start)
        ));
        assert!(matches!(
            logs_dispatch.followups.last(),
            Some(ServerEnvelope::Stream(stream)) if matches!(stream.phase, StreamPhase::End)
        ));

        let stream_cancel_dispatch = daemon
            .handle_request(request(
                "req_stream_cancel",
                ApiMethod::StreamCancel,
                StreamCancelRequest {
                    stream_id: stream_id.clone().into(),
                },
            ))
            .await;
        let _: serde_json::Value = ok_result(&stream_cancel_dispatch.response);
        assert_eq!(
            daemon
                .control
                .lock()
                .expect("control lock poisoned")
                .streams
                .get(stream_id.as_str())
                .expect("stream missing after cancellation")
                .status,
            StreamStatus::Cancelled
        );

        let session_cancel_dispatch = daemon
            .handle_request(request(
                "req_session_sub_cancel",
                ApiMethod::SubscriptionCancel,
                SubscriptionCancelRequest {
                    subscription_id: session_subscription.subscription_id,
                },
            ))
            .await;
        let _: serde_json::Value = ok_result(&session_cancel_dispatch.response);
        let task_cancel_dispatch = daemon
            .handle_request(request(
                "req_task_sub_cancel",
                ApiMethod::SubscriptionCancel,
                SubscriptionCancelRequest {
                    subscription_id: task_subscription.subscription_id,
                },
            ))
            .await;
        let _: serde_json::Value = ok_result(&task_cancel_dispatch.response);
        assert!(daemon
            .control
            .lock()
            .expect("control lock poisoned")
            .subscriptions
            .is_empty());
    }

    #[tokio::test]
    async fn session_send_streaming_dispatch_exposes_live_stream() {
        let harness = TestHarness::new("live-stream-dispatch");
        let daemon = harness.daemon.clone();

        let dispatch = daemon
            .handle_request(request(
                "req_streaming_send",
                ApiMethod::SessionSend,
                SessionSendRequest {
                    session_id: "session.default".into(),
                    message: crate::api::SessionMessage::user("stream this turn"),
                    stream: true,
                },
            ))
            .await;
        let response: SessionSendResponse = ok_result(&dispatch.response);
        let stream_id = response
            .stream_id
            .expect("streaming session.send missing stream_id")
            .0;
        let mut live_stream = dispatch
            .live_stream
            .expect("streaming dispatch missing live stream receiver");

        let mut phases = Vec::new();
        let mut events = Vec::new();
        while let Some(envelope) = live_stream.recv().await {
            match envelope {
                ServerEnvelope::Stream(stream) => {
                    assert_eq!(stream.stream_id.0, stream_id);
                    phases.push(stream.phase.clone());
                    events.push(stream.event);
                    if matches!(stream.phase, StreamPhase::End) {
                        break;
                    }
                }
                other => panic!("expected stream envelope, got {other:?}"),
            }
        }

        assert_eq!(phases.first(), Some(&StreamPhase::Start));
        assert_eq!(phases.last(), Some(&StreamPhase::End));
        assert!(events.iter().any(|event| event == "turn.started"));
        assert!(events.iter().any(|event| event == "stream.completed"));
    }

    #[tokio::test]
    async fn session_send_records_usage_ledger_entries() {
        let harness = TestHarness::new("usage-ledger");
        let daemon = harness.daemon.clone();

        let dispatch = daemon
            .handle_request(request(
                "req_usage_send",
                ApiMethod::SessionSend,
                SessionSendRequest {
                    session_id: "session.default".into(),
                    message: crate::api::SessionMessage::user("record usage"),
                    stream: false,
                },
            ))
            .await;
        let _: SessionSendResponse = ok_result(&dispatch.response);

        let usage_records = daemon
            .store
            .list_usage_records()
            .expect("usage ledger list failed");
        assert_eq!(usage_records.len(), 1);
        assert_eq!(
            usage_records[0].provider_id.as_deref(),
            Some("mock-default")
        );
        assert_eq!(usage_records[0].input_tokens, Some(64));
        assert_eq!(usage_records[0].output_tokens, Some(16));
    }

    #[tokio::test]
    async fn interval_schedule_executes_into_headless_task() {
        let harness = TestHarness::new("schedule-exec");
        let daemon = harness.daemon.clone();
        let schedule = Schedule {
            meta: ObjectMeta::new("schedule.interval", "state.v1"),
            schedule_id: "schedule.interval".into(),
            name: "interval-check".into(),
            trigger: TaskTrigger::Interval { seconds: 0 },
            target: TaskTarget::WorkflowRef {
                workflow_id: "workflow.nightly".into(),
            },
            enabled: true,
        };

        let created_dispatch = daemon
            .handle_request(request(
                "req_schedule_exec_create",
                ApiMethod::ScheduleCreate,
                ScheduleCreateRequest {
                    schedule: schedule.clone(),
                },
            ))
            .await;
        let _: ScheduleGetResponse = ok_result(&created_dispatch.response);

        tokio::time::sleep(std::time::Duration::from_millis(700)).await;

        let tasks = daemon.store.list_tasks().expect("task list failed");
        let scheduled_task = tasks
            .into_iter()
            .find(|record| {
                record.task.definition_ref.as_deref() == Some("workflow:workflow.nightly")
            })
            .expect("expected scheduled task to execute");
        assert_eq!(
            scheduled_task.task.execution_mode,
            ExecutionMode::HeadlessTask
        );
        assert_eq!(scheduled_task.task.status, TaskStatus::Succeeded);

        let events = daemon
            .store
            .get_session("session.default")
            .expect("session load failed")
            .expect("default session missing")
            .events;
        assert!(events
            .iter()
            .any(|event| event.event_type == EventType::ScheduleTriggered));
        assert!(events.iter().any(|event| {
            event.event_type == EventType::ScheduleTriggered
                && event.payload["selected_node_id"].as_str() == Some("node.local")
        }));
    }

    fn request<T>(id: &str, method: ApiMethod, params: T) -> RequestEnvelope
    where
        T: serde::Serialize,
    {
        RequestEnvelope {
            id: RequestId(id.into()),
            method,
            params: serde_json::to_value(params).expect("failed to serialize request params"),
            meta: None,
        }
    }

    fn ok_result<T>(response: &ServerEnvelope) -> T
    where
        T: DeserializeOwned,
    {
        match response {
            ServerEnvelope::Response(response) => {
                assert!(response.ok, "expected ok response, got {response:?}");
                serde_json::from_value(response.result.clone().expect("missing response result"))
                    .expect("failed to decode response result")
            }
            other => panic!("expected response envelope, got {other:?}"),
        }
    }

    fn sample_schedule(schedule_id: &str) -> Schedule {
        Schedule {
            meta: ObjectMeta::new(schedule_id, "state.v1"),
            schedule_id: schedule_id.into(),
            name: "Test Schedule".into(),
            trigger: TaskTrigger::Interval { seconds: 60 },
            target: TaskTarget::WorkflowRef {
                workflow_id: "workflow.test".into(),
            },
            enabled: true,
        }
    }
}
