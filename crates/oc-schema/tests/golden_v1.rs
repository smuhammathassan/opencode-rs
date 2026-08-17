//! Golden serialization tests for the V1 contracts (reference/packages/schema/src/v1/).

use indexmap::IndexMap;
use oc_schema::file_diff;
use oc_schema::session_v1::{
    self, AgentPart, Assistant, CompactionPart, FilePart, FilePartSource, Format, Info,
    OutputFormatJsonSchema, OutputLengthError, Part, PartDelta, RetryPart, SnapshotPart,
    StepFinishPart, StepStartPart, SubtaskPart, TextPart, ToolPart, ToolState, ToolStateCompleted,
    ToolStateError, ToolStatePending, User, WithParts,
};
use oc_schema::v1::permission as v1permission;
use oc_schema::v1::question as v1question;
use serde_json::Value;

fn to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

fn base() -> session_v1::PartBase {
    session_v1::PartBase {
        id: "prt_1".to_string(),
        session_id: "ses_1".to_string(),
        message_id: "msg_1".to_string(),
    }
}

#[test]
fn v1_output_format() {
    assert_eq!(
        to_string(&Format::Text(session_v1::OutputFormatText {
            r#type: session_v1::OutputFormatTextType::Value,
        })),
        r#"{"type":"text"}"#
    );
    assert_eq!(
        to_string(&Format::JsonSchema(OutputFormatJsonSchema {
            r#type: session_v1::OutputFormatJsonSchemaType::Value,
            schema: IndexMap::from([("type".to_string(), Value::from("object"))]),
            retry_count: Some(2),
        })),
        r#"{"type":"json_schema","schema":{"type":"object"},"retryCount":2}"#
    );
}

#[test]
fn v1_text_part() {
    let part = Part::Text(TextPart {
        base: base(),
        r#type: session_v1::TextPartType::Value,
        text: "hi".to_string(),
        synthetic: Some(true),
        ignored: None,
        time: None,
        metadata: None,
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"text","text":"hi","synthetic":true}"#
    );
}

#[test]
fn v1_reasoning_part() {
    let part = Part::Reasoning(session_v1::ReasoningPart {
        base: base(),
        r#type: session_v1::ReasoningPartType::Value,
        text: "think".to_string(),
        metadata: None,
        time: session_v1::ReasoningPartTime {
            start: 1,
            end: Some(2),
        },
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"reasoning","text":"think","time":{"start":1,"end":2}}"#
    );
}

#[test]
fn v1_file_part_with_symbol_source() {
    let part = Part::File(FilePart {
        base: base(),
        r#type: session_v1::FilePartType::Value,
        mime: "text/plain".to_string(),
        filename: Some("a.txt".to_string()),
        url: "file:///a.txt".to_string(),
        source: Some(FilePartSource::Symbol(session_v1::SymbolSource {
            text: session_v1::FilePartSourceText {
                value: "x".to_string(),
                start: oc_schema::Finite(0.0),
                end: oc_schema::Finite(1.0),
            },
            r#type: session_v1::SymbolSourceType::Value,
            path: "a.txt".to_string(),
            range: session_v1::Range {
                start: session_v1::Position {
                    line: 1,
                    character: 0,
                },
                end: session_v1::Position {
                    line: 1,
                    character: 1,
                },
            },
            name: "f".to_string(),
            kind: 2,
        })),
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"file","mime":"text/plain","filename":"a.txt","url":"file:///a.txt","source":{"text":{"value":"x","start":0,"end":1},"type":"symbol","path":"a.txt","range":{"start":{"line":1,"character":0},"end":{"line":1,"character":1}},"name":"f","kind":2}}"#
    );
}

#[test]
fn v1_agent_part() {
    let part = Part::Agent(AgentPart {
        base: base(),
        r#type: session_v1::AgentPartType::Value,
        name: "sub".to_string(),
        source: None,
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"agent","name":"sub"}"#
    );
}

#[test]
fn v1_compaction_part() {
    let part = Part::Compaction(CompactionPart {
        base: base(),
        r#type: session_v1::CompactionPartType::Value,
        auto: true,
        overflow: None,
        tail_start_id: Some("msg_0".to_string()),
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"compaction","auto":true,"tail_start_id":"msg_0"}"#
    );
}

#[test]
fn v1_subtask_part() {
    let part = Part::Subtask(SubtaskPart {
        base: base(),
        r#type: session_v1::SubtaskPartType::Value,
        prompt: "p".to_string(),
        description: "d".to_string(),
        agent: "a".to_string(),
        model: Some(session_v1::SubtaskPartModel {
            provider_id: "anthropic".to_string(),
            model_id: "claude".to_string(),
        }),
        command: None,
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"subtask","prompt":"p","description":"d","agent":"a","model":{"providerID":"anthropic","modelID":"claude"}}"#
    );
}

#[test]
fn v1_retry_part() {
    let part = Part::Retry(RetryPart {
        base: base(),
        r#type: session_v1::RetryPartType::Value,
        attempt: 2,
        error: session_v1::APIError {
            name: session_v1::APIErrorName::Value,
            data: session_v1::APIErrorData {
                message: "boom".to_string(),
                status_code: Some(500),
                is_retryable: false,
                response_headers: None,
                response_body: None,
                metadata: None,
            },
        },
        time: session_v1::RetryPartTime { created: 1 },
    });
    assert_eq!(
        to_string(&part),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"retry","attempt":2,"error":{"name":"APIError","data":{"message":"boom","statusCode":500,"isRetryable":false}},"time":{"created":1}}"#
    );
}

#[test]
fn v1_step_parts() {
    let start = Part::StepStart(StepStartPart {
        base: base(),
        r#type: session_v1::StepStartPartType::Value,
        snapshot: Some("s".to_string()),
    });
    assert_eq!(
        to_string(&start),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"step-start","snapshot":"s"}"#
    );
    let finish = Part::StepFinish(StepFinishPart {
        base: base(),
        r#type: session_v1::StepFinishPartType::Value,
        reason: "done".to_string(),
        snapshot: None,
        cost: oc_schema::Finite(0.5),
        tokens: session_v1::StepFinishTokens {
            total: None,
            input: oc_schema::Finite(10.0),
            output: oc_schema::Finite(20.0),
            reasoning: oc_schema::Finite(0.0),
            cache: session_v1::StepFinishCache {
                read: oc_schema::Finite(0.0),
                write: oc_schema::Finite(0.0),
            },
        },
    });
    assert_eq!(
        to_string(&finish),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"step-finish","reason":"done","cost":0.5,"tokens":{"input":10,"output":20,"reasoning":0,"cache":{"read":0,"write":0}}}"#
    );
}

#[test]
fn v1_snapshot_and_patch_parts() {
    let snapshot = Part::Snapshot(SnapshotPart {
        base: base(),
        r#type: session_v1::SnapshotPartType::Value,
        snapshot: "s".to_string(),
    });
    assert_eq!(
        to_string(&snapshot),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"snapshot","snapshot":"s"}"#
    );
    let patch = Part::Patch(session_v1::PatchPart {
        base: base(),
        r#type: session_v1::PatchPartType::Value,
        hash: "h".to_string(),
        files: vec!["a.txt".to_string()],
    });
    assert_eq!(
        to_string(&patch),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"patch","hash":"h","files":["a.txt"]}"#
    );
}

#[test]
fn v1_tool_part_and_states() {
    let pending = ToolState::Pending(ToolStatePending {
        status: session_v1::ToolStatePendingStatus::Value,
        input: IndexMap::from([("command".to_string(), Value::from("ls"))]),
        raw: "ls".to_string(),
    });
    assert_eq!(
        to_string(&pending),
        r#"{"status":"pending","input":{"command":"ls"},"raw":"ls"}"#
    );
    let tool = Part::Tool(ToolPart {
        base: base(),
        r#type: session_v1::ToolPartType::Value,
        call_id: "c1".to_string(),
        tool: "bash".to_string(),
        state: pending,
        metadata: None,
    });
    assert_eq!(
        to_string(&tool),
        r#"{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"tool","callID":"c1","tool":"bash","state":{"status":"pending","input":{"command":"ls"},"raw":"ls"}}"#
    );
    let completed = ToolState::Completed(ToolStateCompleted {
        status: session_v1::ToolStateCompletedStatus::Value,
        input: IndexMap::new(),
        output: "out".to_string(),
        title: "t".to_string(),
        metadata: IndexMap::new(),
        time: session_v1::ToolStateCompletedTime {
            start: 1,
            end: 2,
            compacted: None,
        },
        attachments: None,
    });
    assert_eq!(
        to_string(&completed),
        r#"{"status":"completed","input":{},"output":"out","title":"t","metadata":{},"time":{"start":1,"end":2}}"#
    );
    let error = ToolState::Error(ToolStateError {
        status: session_v1::ToolStateErrorStatus::Value,
        input: IndexMap::new(),
        error: "nope".to_string(),
        metadata: None,
        time: session_v1::ToolStateErrorTime { start: 1, end: 2 },
    });
    assert_eq!(
        to_string(&error),
        r#"{"status":"error","input":{},"error":"nope","time":{"start":1,"end":2}}"#
    );
}

#[test]
fn v1_user_message() {
    let user = Info::User(User {
        id: "msg_1".to_string(),
        session_id: "ses_1".to_string(),
        role: session_v1::UserRole::Value,
        time: session_v1::UserTime {
            created: oc_schema::Finite(1000.0),
        },
        format: None,
        summary: None,
        agent: "a".to_string(),
        model: session_v1::UserModel {
            provider_id: "anthropic".to_string(),
            model_id: "claude".to_string(),
            variant: None,
        },
        system: None,
        tools: None,
    });
    assert_eq!(
        to_string(&user),
        r#"{"id":"msg_1","sessionID":"ses_1","role":"user","time":{"created":1000},"agent":"a","model":{"providerID":"anthropic","modelID":"claude"}}"#
    );
}

#[test]
fn v1_assistant_message() {
    let assistant = Info::Assistant(Assistant {
        id: "msg_1".to_string(),
        session_id: "ses_1".to_string(),
        role: session_v1::AssistantRole::Value,
        time: session_v1::AssistantTime {
            created: oc_schema::Finite(1000.0),
            completed: Some(oc_schema::Finite(2000.0)),
        },
        error: None,
        parent_id: "msg_0".to_string(),
        model_id: "claude".to_string(),
        provider_id: "anthropic".to_string(),
        mode: "build".to_string(),
        agent: "a".to_string(),
        path: session_v1::AssistantPath {
            cwd: "/w".to_string(),
            root: "/r".to_string(),
        },
        summary: Some(true),
        cost: oc_schema::Finite(0.0),
        tokens: session_v1::AssistantTokens {
            total: Some(oc_schema::Finite(30.0)),
            input: oc_schema::Finite(10.0),
            output: oc_schema::Finite(20.0),
            reasoning: oc_schema::Finite(0.0),
            cache: session_v1::AssistantCache {
                read: oc_schema::Finite(0.0),
                write: oc_schema::Finite(0.0),
            },
        },
        structured: None,
        variant: Some("v1".to_string()),
        finish: None,
    });
    assert_eq!(
        to_string(&assistant),
        r#"{"id":"msg_1","sessionID":"ses_1","role":"assistant","time":{"created":1000,"completed":2000},"parentID":"msg_0","modelID":"claude","providerID":"anthropic","mode":"build","agent":"a","path":{"cwd":"/w","root":"/r"},"summary":true,"cost":0,"tokens":{"total":30,"input":10,"output":20,"reasoning":0,"cache":{"read":0,"write":0}},"variant":"v1"}"#
    );
}

#[test]
fn v1_error_union() {
    let err: session_v1::AssistantError =
        session_v1::AssistantError::OutputLength(OutputLengthError {
            name: session_v1::OutputLengthErrorName::Value,
            data: session_v1::OutputLengthErrorData {},
        });
    assert_eq!(
        to_string(&err),
        r#"{"name":"MessageOutputLengthError","data":{}}"#
    );
}

#[test]
fn v1_with_parts() {
    let with = WithParts {
        info: Info::User(User {
            id: "msg_1".to_string(),
            session_id: "ses_1".to_string(),
            role: session_v1::UserRole::Value,
            time: session_v1::UserTime {
                created: oc_schema::Finite(1000.0),
            },
            format: None,
            summary: None,
            agent: "a".to_string(),
            model: session_v1::UserModel {
                provider_id: "anthropic".to_string(),
                model_id: "claude".to_string(),
                variant: None,
            },
            system: None,
            tools: None,
        }),
        parts: vec![Part::Text(TextPart {
            base: base(),
            r#type: session_v1::TextPartType::Value,
            text: "hi".to_string(),
            synthetic: None,
            ignored: None,
            time: None,
            metadata: None,
        })],
    };
    assert_eq!(
        to_string(&with),
        r#"{"info":{"id":"msg_1","sessionID":"ses_1","role":"user","time":{"created":1000},"agent":"a","model":{"providerID":"anthropic","modelID":"claude"}},"parts":[{"id":"prt_1","sessionID":"ses_1","messageID":"msg_1","type":"text","text":"hi"}]}"#
    );
}

#[test]
fn v1_session_info() {
    let info = session_v1::SessionInfo {
        id: "ses_1".to_string(),
        slug: "slug".to_string(),
        project_id: "global".to_string(),
        workspace_id: None,
        directory: "/d".to_string(),
        path: None,
        parent_id: None,
        summary: Some(session_v1::SessionSummary {
            additions: oc_schema::Finite(1.0),
            deletions: oc_schema::Finite(0.0),
            files: oc_schema::Finite(1.0),
            diffs: Some(vec![file_diff::Info {
                file: None,
                patch: None,
                additions: oc_schema::Finite(1.0),
                deletions: oc_schema::Finite(0.0),
                status: None,
            }]),
        }),
        cost: Some(oc_schema::Finite(0.5)),
        tokens: None,
        share: None,
        title: "t".to_string(),
        agent: Some("a".to_string()),
        model: None,
        version: "1.18.13".to_string(),
        metadata: None,
        time: session_v1::SessionInfoTime {
            created: 1000,
            updated: 2000,
            compacting: None,
            archived: None,
        },
        permission: None,
        revert: None,
    };
    assert_eq!(
        to_string(&info),
        r#"{"id":"ses_1","slug":"slug","projectID":"global","directory":"/d","summary":{"additions":1,"deletions":0,"files":1,"diffs":[{"additions":1,"deletions":0}]},"cost":0.5,"title":"t","agent":"a","version":"1.18.13","time":{"created":1000,"updated":2000}}"#
    );
}

#[test]
fn v1_session_events() {
    let created = session_v1::Created {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_v1::CreatedTag::Value,
        durable: Some(oc_schema::event::DurableRef {
            aggregate_id: "ses_1".to_string(),
            seq: 1,
            version: 1,
        }),
        location: None,
        data: session_v1::SessionEventData {
            session_id: "ses_1".to_string(),
            info: session_v1::SessionInfo {
                id: "ses_1".to_string(),
                slug: "s".to_string(),
                project_id: "global".to_string(),
                workspace_id: None,
                directory: "/d".to_string(),
                path: None,
                parent_id: None,
                summary: None,
                cost: None,
                tokens: None,
                share: None,
                title: "t".to_string(),
                agent: None,
                model: None,
                version: "1".to_string(),
                metadata: None,
                time: session_v1::SessionInfoTime {
                    created: 1000,
                    updated: 1000,
                    compacting: None,
                    archived: None,
                },
                permission: None,
                revert: None,
            },
        },
    };
    assert_eq!(
        to_string(&created),
        r#"{"id":"evt_1","type":"session.created","durable":{"aggregateID":"ses_1","seq":1,"version":1},"data":{"sessionID":"ses_1","info":{"id":"ses_1","slug":"s","projectID":"global","directory":"/d","title":"t","version":"1","time":{"created":1000,"updated":1000}}}}"#
    );
}

#[test]
fn v1_part_delta() {
    let e = PartDelta {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_v1::PartDeltaTag::Value,
        durable: None,
        location: None,
        data: session_v1::PartDeltaData {
            session_id: "ses_1".to_string(),
            message_id: "msg_1".to_string(),
            part_id: "prt_1".to_string(),
            field: "text".to_string(),
            delta: "x".to_string(),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"message.part.delta","data":{"sessionID":"ses_1","messageID":"msg_1","partID":"prt_1","field":"text","delta":"x"}}"#
    );
}

#[test]
fn v1_session_error_event() {
    let e = session_v1::ErrorEvent {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: session_v1::ErrorEventTag::Value,
        durable: None,
        location: None,
        data: session_v1::ErrorEventData {
            session_id: Some("ses_1".to_string()),
            error: Some(session_v1::AssistantError::Unknown(
                session_v1::UnknownError {
                    name: session_v1::UnknownErrorName::Value,
                    data: session_v1::UnknownErrorData {
                        message: "m".to_string(),
                        r#ref: None,
                    },
                },
            )),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"session.error","data":{"sessionID":"ses_1","error":{"name":"UnknownError","data":{"message":"m"}}}}"#
    );
}

#[test]
fn v1_part_inputs() {
    let text = session_v1::TextPartInput {
        id: None,
        r#type: session_v1::TextPartInputType::Value,
        text: "hi".to_string(),
        synthetic: None,
        ignored: None,
        time: None,
        metadata: None,
    };
    assert_eq!(to_string(&text), r#"{"type":"text","text":"hi"}"#);
    let file = session_v1::FilePartInput {
        id: Some("prt_9".to_string()),
        r#type: session_v1::FilePartInputType::Value,
        mime: "text/plain".to_string(),
        filename: None,
        url: "file:///a".to_string(),
        source: None,
    };
    assert_eq!(
        to_string(&file),
        r#"{"id":"prt_9","type":"file","mime":"text/plain","url":"file:///a"}"#
    );
}

#[test]
fn v1_permission_ask_and_reply() {
    let ask = v1permission::AskInput {
        id: None,
        session_id: "ses_1".to_string(),
        permission: "write".to_string(),
        patterns: vec!["/a".to_string()],
        metadata: IndexMap::new(),
        always: vec!["once".to_string()],
        tool: None,
        ruleset: vec![v1permission::Rule {
            permission: "write".to_string(),
            pattern: "/a".to_string(),
            action: v1permission::Action::Allow,
        }],
    };
    assert_eq!(
        to_string(&ask),
        r#"{"sessionID":"ses_1","permission":"write","patterns":["/a"],"metadata":{},"always":["once"],"ruleset":[{"permission":"write","pattern":"/a","action":"allow"}]}"#
    );
    let reply = v1permission::ReplyInput {
        request_id: "per_1".to_string(),
        reply: v1permission::Reply::Always,
        message: Some("ok".to_string()),
    };
    assert_eq!(
        to_string(&reply),
        r#"{"requestID":"per_1","reply":"always","message":"ok"}"#
    );
    let approval = v1permission::Approval {
        project_id: "global".to_string(),
        patterns: vec!["/a".to_string()],
    };
    assert_eq!(
        to_string(&approval),
        r#"{"projectID":"global","patterns":["/a"]}"#
    );
}

#[test]
fn v1_permission_events() {
    let asked = v1permission::Asked {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: v1permission::AskedTag::Value,
        durable: None,
        location: None,
        data: v1permission::Request {
            id: "per_1".to_string(),
            session_id: "ses_1".to_string(),
            permission: "write".to_string(),
            patterns: vec!["/a".to_string()],
            metadata: IndexMap::new(),
            always: vec!["always".to_string()],
            tool: Some(v1permission::Tool {
                message_id: "msg_1".to_string(),
                call_id: "c1".to_string(),
            }),
        },
    };
    assert_eq!(
        to_string(&asked),
        r#"{"id":"evt_1","type":"permission.asked","data":{"id":"per_1","sessionID":"ses_1","permission":"write","patterns":["/a"],"metadata":{},"always":["always"],"tool":{"messageID":"msg_1","callID":"c1"}}}"#
    );
}

#[test]
fn v1_question_events() {
    let replied = v1question::RepliedEvent {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: v1question::RepliedEventTag::Value,
        durable: None,
        location: None,
        data: v1question::Replied {
            session_id: "ses_1".to_string(),
            request_id: "que_1".to_string(),
            answers: vec![vec!["yes".to_string()]],
        },
    };
    assert_eq!(
        to_string(&replied),
        r#"{"id":"evt_1","type":"question.replied","data":{"sessionID":"ses_1","requestID":"que_1","answers":[["yes"]]}}"#
    );
}

#[test]
fn legacy_command_executed() {
    let e = oc_schema::legacy_event::CommandExecuted {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::legacy_event::CommandExecutedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::legacy_event::CommandExecutedData {
            name: "help".to_string(),
            session_id: "ses_1".to_string(),
            arguments: "".to_string(),
            message_id: "msg_1".to_string(),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"command.executed","data":{"name":"help","sessionID":"ses_1","arguments":"","messageID":"msg_1"}}"#
    );
}
