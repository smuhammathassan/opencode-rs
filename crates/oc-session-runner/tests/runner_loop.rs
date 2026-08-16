//! End-to-end agent loop: drive a full `run()` through the tool-call →
//! tool-result → continuation cycle with mock services.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use oc_session_runner::llm::event::{LLMEvent, ToolContent, ToolOutput, ToolResultValue, Usage};
use oc_session_runner::llm::message::Model;
use oc_session_runner::llm::LLMError;
use oc_session_runner::runner::llm::{RunnerDeps, SessionRunnerService};
use oc_session_runner::runner::model::SessionRunnerModel;
use oc_session_runner::session::event::SessionEvent;
use oc_session_runner::session::message::SessionMessage;
use oc_session_runner::session::schema::{Location, LocationRef, ModelRef, SessionID, SessionInfo};
use oc_session_runner::session::services::*;
use tokio_util::sync::CancellationToken;

// ---- mock services ----

#[derive(Default)]
struct MockEventBus {
    events: Mutex<Vec<SessionEvent>>,
}

impl EventBus for MockEventBus {
    fn publish(&self, event: SessionEvent) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let events = &self.events;
        Box::pin(async move {
            events.lock().unwrap().push(event);
        })
    }
}

struct MockStore {
    session: SessionInfo,
}

impl SessionStore for MockStore {
    fn get(
        &self,
        _session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = Option<SessionInfo>> + Send + '_>> {
        let session = self.session.clone();
        Box::pin(async move { Some(session) })
    }

    fn context(
        &self,
        _session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = Vec<SessionMessage>> + Send + '_>> {
        Box::pin(async { Vec::new() })
    }
}

struct MockInput {
    steer_eligible: bool,
    steer_promoted_count: AtomicUsize,
}

impl MockInput {
    fn new(steer_eligible: bool) -> Self {
        Self {
            steer_eligible,
            steer_promoted_count: AtomicUsize::new(0),
        }
    }
}

impl SessionInput for MockInput {
    fn has_pending(
        &self,
        _session_id: &SessionID,
        delivery: Delivery,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let promoted = &self.steer_promoted_count;
        let eligible = self.steer_eligible;
        Box::pin(async move {
            match delivery {
                Delivery::Steer => eligible && promoted.load(Ordering::SeqCst) == 0,
                Delivery::Queue => false,
            }
        })
    }

    fn promote_steers(
        &self,
        _session_id: &SessionID,
        _cutoff: u64,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + '_>> {
        let promoted = &self.steer_promoted_count;
        Box::pin(async move {
            if promoted.fetch_add(1, Ordering::SeqCst) == 0 {
                1
            } else {
                0
            }
        })
    }

    fn promote_next_queued(
        &self,
        _session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async { false })
    }

    fn latest_sequence(
        &self,
        _session_id: &SessionID,
    ) -> Pin<Box<dyn Future<Output = u64> + Send + '_>> {
        Box::pin(async { 0 })
    }
}

#[derive(Default)]
struct MockContextEpoch;

impl SessionContextEpoch for MockContextEpoch {
    fn initialize(
        &self,
        _session_id: &SessionID,
        _context: SystemContext,
    ) -> Pin<Box<dyn Future<Output = Option<PreparedContext>> + Send + '_>> {
        Box::pin(async { None })
    }

    fn prepare(
        &self,
        _session_id: &SessionID,
        _context: SystemContext,
    ) -> Pin<Box<dyn Future<Output = PreparedContext> + Send + '_>> {
        Box::pin(async {
            PreparedContext {
                baseline: "you are build".into(),
                baseline_seq: 0,
            }
        })
    }
}

#[derive(Default)]
struct MockHistory;

impl SessionHistory for MockHistory {
    fn entries_for_runner(
        &self,
        _session_id: &SessionID,
        _baseline_seq: u64,
    ) -> Pin<Box<dyn Future<Output = Vec<HistoryEntry>> + Send + '_>> {
        Box::pin(async { Vec::new() })
    }
}

#[derive(Default)]
struct MockAgents;

impl Agents for MockAgents {
    fn select(
        &self,
        _id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = AgentSelection> + Send + '_>> {
        Box::pin(async {
            AgentSelection {
                id: "build".into(),
                info: Some(AgentInfo {
                    system: Some("be terse".into()),
                    steps: None,
                    permissions: Vec::new(),
                }),
            }
        })
    }
}

#[derive(Default)]
struct MockModels;

impl SessionRunnerModel for MockModels {
    fn resolve(
        &self,
        _session: &SessionInfo,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Model, oc_session_runner::runner::model::ModelError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async { Ok(Model::make("gpt-4o", "openai")) })
    }
}

#[derive(Default)]
struct MockSnapshots;

impl Snapshots for MockSnapshots {
    fn capture(&self) -> Pin<Box<dyn Future<Output = Option<String>> + Send + '_>> {
        Box::pin(async { Some("snap".into()) })
    }

    fn files(
        &self,
        _from: &str,
        _to: &str,
    ) -> Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + '_>> {
        Box::pin(async { Some(vec!["a.txt".into()]) })
    }
}

#[derive(Default)]
struct MockLocation;

impl LocationService for MockLocation {
    fn current(&self) -> Location {
        Location::new("/work", None)
    }
}

#[derive(Default)]
struct MockSystemContext;

impl SystemContextRegistry for MockSystemContext {
    fn load(&self) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>> {
        Box::pin(async { SystemContext::default() })
    }
}

#[derive(Default)]
struct MockGuidance;

impl SkillGuidance for MockGuidance {
    fn load(&self, _agent: &str) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>> {
        Box::pin(async { SystemContext::default() })
    }
}

impl ReferenceGuidance for MockGuidance {
    fn load(&self) -> Pin<Box<dyn Future<Output = SystemContext> + Send + '_>> {
        Box::pin(async { SystemContext::default() })
    }
}

#[derive(Default)]
struct MockCompaction;

impl SessionCompaction for MockCompaction {
    fn compact_if_needed(
        &self,
        _input: CompactionInput,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async { false })
    }

    fn compact_after_overflow(
        &self,
        _input: CompactionInput,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async { false })
    }
}

struct MockSettle;

impl ToolSettle for MockSettle {
    fn settle(
        &self,
        _input: ExecuteInput,
    ) -> Pin<Box<dyn Future<Output = Result<Settlement, ToolSettlementError>> + Send + '_>> {
        Box::pin(async {
            Ok(Settlement {
                result: ToolResultValue::Text {
                    value: serde_json::json!("file body"),
                },
                output: Some(ToolOutput {
                    structured: serde_json::json!({}),
                    content: vec![ToolContent::text("file body")],
                }),
                output_paths: Vec::new(),
            })
        })
    }
}

#[derive(Default)]
struct MockTools;

impl ToolRegistry for MockTools {
    fn materialize(
        &self,
        _permissions: &[String],
    ) -> Pin<Box<dyn Future<Output = Option<ToolMaterialization>> + Send + '_>> {
        Box::pin(async {
            Some(ToolMaterialization {
                definitions: Vec::new(),
                settle: Arc::new(MockSettle),
            })
        })
    }
}

struct MockLlm {
    responses: Mutex<VecDeque<Vec<LLMEvent>>>,
}

impl LlmClient for MockLlm {
    fn stream(
        &self,
        _request: oc_session_runner::llm::message::LLMRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LlmEventStream, LLMError>> + Send + '_>> {
        let response = self.responses.lock().unwrap().pop_front();
        Box::pin(async move {
            response
                .map(|events| {
                    Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as LlmEventStream
                })
                .ok_or_else(|| LLMError {
                    module: "test".into(),
                    method: "stream".into(),
                    reason: oc_session_runner::llm::LLMErrorReason::NoRoute(
                        oc_session_runner::llm::error::ReasonMessage {
                            message: "no more responses".into(),
                            provider_metadata: None,
                            http: None,
                        },
                    ),
                })
        })
    }
}

fn usage() -> Usage {
    Usage {
        non_cached_input_tokens: Some(10.0),
        output_tokens: Some(5.0),
        reasoning_tokens: Some(1.0),
        ..Default::default()
    }
}

fn text_events(text: &str) -> Vec<LLMEvent> {
    vec![
        LLMEvent::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        LLMEvent::TextDelta {
            id: "t1".into(),
            text: text.into(),
            provider_metadata: None,
        },
        LLMEvent::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_call_continuation_round_trip() {
    let events = Arc::new(MockEventBus::default());
    let llm = Arc::new(MockLlm {
        responses: Mutex::new(VecDeque::from(vec![
            // Turn 1: tool call.
            {
                let mut turn = text_events("let me read that");
                turn.extend(vec![
                    LLMEvent::ToolCall {
                        id: "call_1".into(),
                        name: "read".into(),
                        input: serde_json::json!({ "path": "a.txt" }),
                        provider_executed: None,
                        provider_metadata: None,
                    },
                    LLMEvent::StepFinish {
                        index: 0.0,
                        reason: "tool-calls".into(),
                        usage: Some(usage()),
                        provider_metadata: None,
                    },
                    LLMEvent::Finish {
                        reason: "tool-calls".into(),
                        usage: Some(usage()),
                        provider_metadata: None,
                    },
                ]);
                turn
            },
            // Turn 2: wrap up with text only.
            {
                let mut turn = text_events("done");
                turn.extend(vec![
                    LLMEvent::StepFinish {
                        index: 0.0,
                        reason: "stop".into(),
                        usage: Some(usage()),
                        provider_metadata: None,
                    },
                    LLMEvent::Finish {
                        reason: "stop".into(),
                        usage: Some(usage()),
                        provider_metadata: None,
                    },
                ]);
                turn
            },
        ])),
    });

    let deps = RunnerDeps {
        events: events.clone(),
        llm,
        agents: Arc::new(MockAgents::default()),
        tools: Arc::new(MockTools::default()),
        models: Arc::new(MockModels::default()),
        store: Arc::new(MockStore {
            session: SessionInfo {
                id: "ses_abc".into(),
                agent: Some("build".into()),
                model: Some(ModelRef {
                    id: "gpt-4o".into(),
                    provider_id: "openai".into(),
                    variant: None,
                }),
                location: LocationRef {
                    directory: "/work".into(),
                    workspace_id: None,
                },
            },
        }),
        location: Arc::new(MockLocation::default()),
        system_context: Arc::new(MockSystemContext::default()),
        skill_guidance: Arc::new(MockGuidance::default()),
        reference_guidance: Arc::new(MockGuidance::default()),
        snapshots: Arc::new(MockSnapshots::default()),
        input: Arc::new(MockInput::new(true)),
        history: Arc::new(MockHistory::default()),
        context_epoch: Arc::new(MockContextEpoch::default()),
        compaction: Arc::new(MockCompaction::default()),
    };

    let runner = SessionRunnerService::new(deps);
    let token = CancellationToken::new();
    runner
        .run(&"ses_abc".to_string(), false, &token)
        .await
        .unwrap();

    let emitted = events.events.lock().unwrap().clone();
    let types = emitted
        .iter()
        .map(|event| {
            serde_json::to_value(event)
                .unwrap()
                .get("type")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();

    // Two full turns happened: step started + text + tool lifecycle + step
    // ended, then a second step started + text + step ended.
    assert!(types.contains(&"session.next.step.started".to_string()));
    assert!(types.contains(&"session.next.tool.called".to_string()));
    assert!(types.contains(&"session.next.tool.success".to_string()));
    assert_eq!(
        types
            .iter()
            .filter(|t| *t == "session.next.step.started")
            .count(),
        2,
        "continuation ran a second provider turn"
    );
    assert_eq!(
        types
            .iter()
            .filter(|t| *t == "session.next.step.ended")
            .count(),
        2
    );

    // The tool result content reached the model in the second turn (the mock
    // llm does not assert history, but the settle published the success event).
    let success = emitted
        .iter()
        .find(|event| matches!(event, SessionEvent::ToolSuccess { .. }))
        .unwrap();
    let value = serde_json::to_value(success).unwrap();
    assert_eq!(value.get("callID").unwrap(), &serde_json::json!("call_1"));
    assert_eq!(value.get("tool").map(|_| true), None); // tool field is on ToolCalled
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_eligible_work_is_a_noop() {
    // No pending steer/queue and not forced: run returns without any turn.
    let events = Arc::new(MockEventBus::default());
    let input = MockInput::new(false);
    let deps = RunnerDeps {
        events: events.clone(),
        llm: Arc::new(MockLlm {
            responses: Mutex::new(VecDeque::new()),
        }),
        agents: Arc::new(MockAgents::default()),
        tools: Arc::new(MockTools::default()),
        models: Arc::new(MockModels::default()),
        store: Arc::new(MockStore {
            session: SessionInfo {
                id: "ses_abc".into(),
                agent: None,
                model: None,
                location: LocationRef {
                    directory: "/work".into(),
                    workspace_id: None,
                },
            },
        }),
        location: Arc::new(MockLocation::default()),
        system_context: Arc::new(MockSystemContext::default()),
        skill_guidance: Arc::new(MockGuidance::default()),
        reference_guidance: Arc::new(MockGuidance::default()),
        snapshots: Arc::new(MockSnapshots::default()),
        input: Arc::new(input),
        history: Arc::new(MockHistory::default()),
        context_epoch: Arc::new(MockContextEpoch::default()),
        compaction: Arc::new(MockCompaction::default()),
    };
    let runner = SessionRunnerService::new(deps);
    let token = CancellationToken::new();
    runner
        .run(&"ses_abc".to_string(), false, &token)
        .await
        .unwrap();
    assert!(events.events.lock().unwrap().is_empty());
}
