//! Golden serialization tests for session messages and session events.
//! Expected strings are derived manually from the reference zod schemas in
//! reference/packages/schema/src/.

use indexmap::IndexMap;
use oc_schema::event;
use oc_schema::location;
use oc_schema::session_event::{self, DurableEvent, Event};
use oc_schema::session_message::{
    Assistant, AssistantContent, AssistantReasoning, AssistantText, AssistantTool,
    AssistantToolTime, Compaction, CompactionReason, Message, ModelSwitched, Shell, Synthetic,
    System, TokenCache, TokenUsage, ToolState, ToolStateCompleted, ToolStateError,
    ToolStatePending, ToolStateRunning, UnknownError, User,
};
use oc_schema::{model, prompt, session_id, session_message as sm};
use serde_json::Value;

fn to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

fn model_ref() -> model::Ref {
    model::Ref {
        id: "claude".to_string(),
        provider_id: "anthropic".to_string(),
        variant: None,
    }
}

#[test]
fn message_user() {
    let message = Message::User(User {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
        text: "hello".to_string(),
        files: None,
        agents: None,
        r#type: sm::UserType::Value,
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000},"text":"hello","type":"user"}"#
    );
}

#[test]
fn message_user_with_files() {
    let message = Message::User(User {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
        text: "hello".to_string(),
        files: Some(vec![prompt::FileAttachment {
            uri: "file:///a".to_string(),
            mime: "text/plain".to_string(),
            name: None,
            description: None,
            source: None,
        }]),
        agents: Some(vec![prompt::AgentAttachment {
            name: "sub".to_string(),
            source: None,
        }]),
        r#type: sm::UserType::Value,
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000},"text":"hello","files":[{"uri":"file:///a","mime":"text/plain"}],"agents":[{"name":"sub"}],"type":"user"}"#
    );
}

#[test]
fn message_agent_switched() {
    let message = Message::AgentSwitched(sm::AgentSwitched {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
        r#type: sm::AgentSwitchedType::Value,
        agent: "coder".to_string(),
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000},"type":"agent-switched","agent":"coder"}"#
    );
}

#[test]
fn message_model_switched() {
    let message = Message::ModelSwitched(ModelSwitched {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
        r#type: sm::ModelSwitchedType::Value,
        model: model_ref(),
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000},"type":"model-switched","model":{"id":"claude","providerID":"anthropic"}}"#
    );
}

#[test]
fn message_synthetic() {
    let message = Message::Synthetic(Synthetic {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
        session_id: "ses_1".to_string(),
        text: "hi".to_string(),
        r#type: sm::SyntheticType::Value,
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000},"sessionID":"ses_1","text":"hi","type":"synthetic"}"#
    );
}

#[test]
fn message_system() {
    let message = Message::System(System {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
        r#type: sm::SystemType::Value,
        text: "sys".to_string(),
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000},"type":"system","text":"sys"}"#
    );
}

#[test]
fn message_shell() {
    let message = Message::Shell(Shell {
        id: "msg_1".to_string(),
        metadata: None,
        r#type: sm::ShellType::Value,
        call_id: "call_1".to_string(),
        command: "ls -la".to_string(),
        output: "total 0".to_string(),
        time: sm::TimeCompleted {
            created: 1000,
            completed: Some(1005),
        },
    });
    assert_eq!(
        to_string(&message),
        r#"{"id":"msg_1","time":{"created":1000,"completed":1005},"type":"shell","callID":"call_1","command":"ls -la","output":"total 0"}"#
    );
}

#[test]
fn tool_state_variants() {
    let pending = ToolState::Pending(ToolStatePending {
        status: sm::ToolStatePendingStatus::Value,
        input: "raw".to_string(),
    });
    assert_eq!(to_string(&pending), r#"{"status":"pending","input":"raw"}"#);

    let running = ToolState::Running(ToolStateRunning {
        status: sm::ToolStateRunningStatus::Value,
        input: IndexMap::new(),
        structured: IndexMap::new(),
        content: vec![oc_schema::llm::ToolContent::Text(
            oc_schema::llm::ToolTextContent {
                r#type: oc_schema::llm::ToolTextContentType::Value,
                text: "x".to_string(),
            },
        )],
    });
    assert_eq!(
        to_string(&running),
        r#"{"status":"running","input":{},"structured":{},"content":[{"type":"text","text":"x"}]}"#
    );

    let completed = ToolState::Completed(ToolStateCompleted {
        status: sm::ToolStateCompletedStatus::Value,
        input: IndexMap::new(),
        attachments: None,
        content: Vec::new(),
        output_paths: Some(vec!["/out.txt".to_string()]),
        structured: IndexMap::new(),
        result: Some(Value::from(42)),
    });
    assert_eq!(
        to_string(&completed),
        r#"{"status":"completed","input":{},"content":[],"outputPaths":["/out.txt"],"structured":{},"result":42}"#
    );

    let error = ToolState::Error(ToolStateError {
        status: sm::ToolStateErrorStatus::Value,
        input: IndexMap::new(),
        content: Vec::new(),
        structured: IndexMap::new(),
        error: UnknownError {
            r#type: sm::UnknownErrorType::Value,
            message: "boom".to_string(),
        },
        result: None,
    });
    assert_eq!(
        to_string(&error),
        r#"{"status":"error","input":{},"content":[],"structured":{},"error":{"type":"unknown","message":"boom"}}"#
    );
}

#[test]
fn assistant_content_variants() {
    let text = AssistantContent::Text(AssistantText {
        r#type: sm::AssistantTextType::Value,
        id: "t1".to_string(),
        text: "hi".to_string(),
    });
    assert_eq!(to_string(&text), r#"{"type":"text","id":"t1","text":"hi"}"#);

    let reasoning = AssistantContent::Reasoning(AssistantReasoning {
        r#type: sm::AssistantReasoningType::Value,
        id: "r1".to_string(),
        text: "think".to_string(),
        provider_metadata: None,
        time: None,
    });
    assert_eq!(
        to_string(&reasoning),
        r#"{"type":"reasoning","id":"r1","text":"think"}"#
    );

    let tool = AssistantContent::Tool(AssistantTool {
        r#type: sm::AssistantToolType::Value,
        id: "t1".to_string(),
        name: "bash".to_string(),
        provider: None,
        state: ToolState::Completed(ToolStateCompleted {
            status: sm::ToolStateCompletedStatus::Value,
            input: IndexMap::new(),
            attachments: None,
            content: Vec::new(),
            output_paths: None,
            structured: IndexMap::new(),
            result: None,
        }),
        time: AssistantToolTime {
            created: 1000,
            ran: Some(1001),
            completed: Some(1002),
            pruned: None,
        },
    });
    assert_eq!(
        to_string(&tool),
        r#"{"type":"tool","id":"t1","name":"bash","state":{"status":"completed","input":{},"content":[],"structured":{}},"time":{"created":1000,"ran":1001,"completed":1002}}"#
    );
}

#[test]
fn message_assistant() {
    let tokens = TokenUsage {
        input: oc_schema::Finite(100.0),
        output: oc_schema::Finite(50.0),
        reasoning: oc_schema::Finite(5.0),
        cache: TokenCache {
            read: oc_schema::Finite(0.0),
            write: oc_schema::Finite(0.0),
        },
    };
    let assistant = Message::Assistant(Assistant {
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCompleted {
            created: 1000,
            completed: Some(2000),
        },
        r#type: sm::AssistantType::Value,
        agent: "coder".to_string(),
        model: model_ref(),
        content: vec![AssistantContent::Text(AssistantText {
            r#type: sm::AssistantTextType::Value,
            id: "t1".to_string(),
            text: "hi".to_string(),
        })],
        snapshot: None,
        finish: Some("done".to_string()),
        cost: Some(oc_schema::Finite(0.00042)),
        tokens: Some(tokens),
        error: None,
    });
    assert_eq!(
        to_string(&assistant),
        r#"{"id":"msg_1","time":{"created":1000,"completed":2000},"type":"assistant","agent":"coder","model":{"id":"claude","providerID":"anthropic"},"content":[{"type":"text","id":"t1","text":"hi"}],"finish":"done","cost":0.00042,"tokens":{"input":100,"output":50,"reasoning":5,"cache":{"read":0,"write":0}}}"#
    );
}

#[test]
fn message_compaction() {
    let message = Message::Compaction(Compaction {
        r#type: sm::CompactionType::Value,
        reason: CompactionReason::Manual,
        summary: "sum".to_string(),
        recent: "rec".to_string(),
        id: "msg_1".to_string(),
        metadata: None,
        time: sm::TimeCreated { created: 1000 },
    });
    assert_eq!(
        to_string(&message),
        r#"{"type":"compaction","reason":"manual","summary":"sum","recent":"rec","id":"msg_1","time":{"created":1000}}"#
    );
}

#[test]
fn message_roundtrip() {
    let json = r#"{"id":"msg_1","time":{"created":1},"type":"shell","callID":"c","command":"ls","output":"o"}"#;
    let message: Message = serde_json::from_str(json).unwrap();
    assert_eq!(to_string(&message), json);

    let json = r#"{"id":"msg_1","time":{"created":1},"text":"hi","type":"user"}"#;
    let message: Message = serde_json::from_str(json).unwrap();
    assert_eq!(to_string(&message), json);

    // A user message without the required text field is rejected.
    let json = r#"{"id":"msg_1","time":{"created":1},"type":"user"}"#;
    assert!(serde_json::from_str::<Message>(json).is_err());
}

#[test]
fn session_event_prompted() {
    let event = Event::Prompted(session_event::Prompted {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_event::PromptedTag::Value,
        durable: Some(event::DurableRef {
            aggregate_id: "ses_1".to_string(),
            seq: 3,
            version: 1,
        }),
        location: None,
        data: session_event::PromptFields {
            timestamp: 1000,
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            prompt: prompt::Prompt {
                text: "hi".to_string(),
                files: None,
                agents: None,
            },
            delivery: oc_schema::session_delivery::Delivery::Steer,
        },
    });
    assert_eq!(
        to_string(&event),
        r#"{"id":"evt_1","type":"session.next.prompted","durable":{"aggregateID":"ses_1","seq":3,"version":1},"data":{"timestamp":1000,"sessionID":"ses_1","messageID":"msg_1","prompt":{"text":"hi"},"delivery":"steer"}}"#
    );
}

#[test]
fn session_event_text_delta() {
    let event = Event::TextDelta(session_event::TextDelta {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_event::TextDeltaTag::Value,
        durable: None,
        location: None,
        data: session_event::TextDeltaData {
            timestamp: 1000,
            session_id: "ses_1".to_string(),
            assistant_message_id: "msg_1".to_string(),
            text_id: "t1".to_string(),
            delta: "hi".to_string(),
        },
    });
    assert_eq!(
        to_string(&event),
        r#"{"id":"evt_1","type":"session.next.text.delta","data":{"timestamp":1000,"sessionID":"ses_1","assistantMessageID":"msg_1","textID":"t1","delta":"hi"}}"#
    );
}

#[test]
fn session_event_step_ended() {
    let event = DurableEvent::StepEnded(session_event::StepEnded {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_event::StepEndedTag::Value,
        durable: Some(event::DurableRef {
            aggregate_id: "ses_1".to_string(),
            seq: 7,
            version: 2,
        }),
        location: Some(location::Ref {
            directory: "/d".to_string(),
            workspace_id: None,
        }),
        data: session_event::StepEndedData {
            timestamp: 1000,
            session_id: "ses_1".to_string(),
            assistant_message_id: "msg_1".to_string(),
            finish: "done".to_string(),
            cost: oc_schema::Finite(1.5),
            tokens: TokenUsage {
                input: oc_schema::Finite(10.0),
                output: oc_schema::Finite(20.0),
                reasoning: oc_schema::Finite(0.0),
                cache: TokenCache {
                    read: oc_schema::Finite(0.0),
                    write: oc_schema::Finite(0.0),
                },
            },
            snapshot: Some("snap".to_string()),
            files: None,
        },
    });
    assert_eq!(
        to_string(&event),
        r#"{"id":"evt_1","type":"session.next.step.ended","durable":{"aggregateID":"ses_1","seq":7,"version":2},"location":{"directory":"/d"},"data":{"timestamp":1000,"sessionID":"ses_1","assistantMessageID":"msg_1","finish":"done","cost":1.5,"tokens":{"input":10,"output":20,"reasoning":0,"cache":{"read":0,"write":0}},"snapshot":"snap"}}"#
    );
}

#[test]
fn session_event_tool_called() {
    let event = Event::ToolCalled(session_event::ToolCalled {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_event::ToolCalledTag::Value,
        durable: None,
        location: None,
        data: session_event::ToolCalledData {
            base: session_event::ToolBase {
                timestamp: 1000,
                session_id: "ses_1".to_string(),
                assistant_message_id: "msg_1".to_string(),
                call_id: "c1".to_string(),
            },
            tool: "bash".to_string(),
            input: IndexMap::from([("command".to_string(), Value::from("ls"))]),
            provider: session_event::ToolProvider {
                executed: true,
                metadata: None,
            },
        },
    });
    assert_eq!(
        to_string(&event),
        r#"{"id":"evt_1","type":"session.next.tool.called","data":{"timestamp":1000,"sessionID":"ses_1","assistantMessageID":"msg_1","callID":"c1","tool":"bash","input":{"command":"ls"},"provider":{"executed":true}}}"#
    );
}

#[test]
fn session_event_retried() {
    let event = Event::Retried(session_event::Retried {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_event::RetriedTag::Value,
        durable: None,
        location: None,
        data: session_event::RetriedData {
            timestamp: 1000,
            session_id: "ses_1".to_string(),
            attempt: oc_schema::Finite(2.0),
            error: session_event::RetryError {
                message: "m".to_string(),
                status_code: Some(oc_schema::Finite(429.0)),
                is_retryable: true,
                response_headers: None,
                response_body: None,
                metadata: None,
            },
        },
    });
    assert_eq!(
        to_string(&event),
        r#"{"id":"evt_1","type":"session.next.retried","data":{"timestamp":1000,"sessionID":"ses_1","attempt":2,"error":{"message":"m","statusCode":429,"isRetryable":true}}}"#
    );
}

#[test]
fn session_event_roundtrip() {
    let json = r#"{"id":"evt_1","type":"session.next.prompted","data":{"timestamp":1000,"sessionID":"ses_1","messageID":"msg_1","prompt":{"text":"hi"},"delivery":"queue"}}"#;
    let event: Event = serde_json::from_str(json).unwrap();
    assert_eq!(to_string(&event), json);

    let json = r#"{"id":"evt_1","type":"session.next.compaction.delta","data":{"timestamp":1,"sessionID":"ses_1","messageID":"msg_1","text":"x"}}"#;
    let event: Event = serde_json::from_str(json).unwrap();
    assert_eq!(to_string(&event), json);

    let json = r#"{"id":"evt_1","type":"session.next.step.failed","durable":{"aggregateID":"ses_1","seq":1,"version":2},"data":{"timestamp":1,"sessionID":"ses_1","assistantMessageID":"msg_1","error":{"type":"unknown","message":"boom"}}}"#;
    let event: Event = serde_json::from_str(json).unwrap();
    assert_eq!(to_string(&event), json);
}

#[test]
fn session_id_creation_shape() {
    let id = session_id::create();
    assert_eq!(id.len(), 30);
    assert!(id.starts_with("ses_"));
}
