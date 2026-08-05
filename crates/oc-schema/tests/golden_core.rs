//! Golden serialization tests for the core (non-event) schema types.
//! Expected strings are derived manually from the reference zod schemas in
//! reference/packages/schema/src/.

use indexmap::IndexMap;
use oc_schema::{
    agent, command, connection, credential, file_diff, filesystem, llm, location, model, provider,
    pty, pty_ticket, question, revert, session, session_input, skill,
};
use serde_json::Value;

fn to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

#[test]
fn agent_empty() {
    let info = agent::empty("asst_123".to_string());
    assert_eq!(
        to_string(&info),
        r#"{"id":"asst_123","request":{"headers":{},"body":{}},"mode":"all","hidden":false,"permissions":[]}"#
    );
}

#[test]
fn agent_info_full() {
    let info = agent::Info {
        id: "asst_1".to_string(),
        model: Some(model::Ref {
            id: "claude".to_string(),
            provider_id: "anthropic".to_string(),
            variant: Some("v1".to_string()),
        }),
        request: provider::Request {
            headers: IndexMap::from([("authorization".to_string(), "Bearer x".to_string())]),
            body: IndexMap::from([("temperature".to_string(), Value::from(0.5))]),
        },
        system: Some("sys".to_string()),
        description: Some("desc".to_string()),
        mode: agent::Mode::Subagent,
        hidden: true,
        color: Some(agent::Color("#a1b2c3".to_string())),
        steps: Some(3),
        permissions: vec![oc_schema::permission::Rule {
            action: "edit".to_string(),
            resource: "/x".to_string(),
            effect: oc_schema::permission::Effect::Allow,
        }],
    };
    assert_eq!(
        to_string(&info),
        r##"{"id":"asst_1","model":{"id":"claude","providerID":"anthropic","variant":"v1"},"request":{"headers":{"authorization":"Bearer x"},"body":{"temperature":0.5}},"system":"sys","description":"desc","mode":"subagent","hidden":true,"color":"#a1b2c3","steps":3,"permissions":[{"action":"edit","resource":"/x","effect":"allow"}]}"##
    );
}

#[test]
fn command_info() {
    let info = command::Info {
        name: "fix".to_string(),
        template: "fix {{input}}".to_string(),
        description: Some("d".to_string()),
        agent: Some("a".to_string()),
        model: Some(model::Ref {
            id: "m".to_string(),
            provider_id: "p".to_string(),
            variant: None,
        }),
        subtask: Some(true),
    };
    assert_eq!(
        to_string(&info),
        r#"{"name":"fix","template":"fix {{input}}","description":"d","agent":"a","model":{"id":"m","providerID":"p"},"subtask":true}"#
    );
}

#[test]
fn connection_union() {
    assert_eq!(
        to_string(&connection::Info::Credential(connection::CredentialInfo {
            r#type: connection::CredentialInfoType::Value,
            id: "cred_1".to_string(),
            label: "L".to_string(),
        })),
        r#"{"type":"credential","id":"cred_1","label":"L"}"#
    );
    assert_eq!(
        to_string(&connection::Info::Env(connection::EnvInfo {
            r#type: connection::EnvInfoType::Value,
            name: "OPENAI_API_KEY".to_string(),
        })),
        r#"{"type":"env","name":"OPENAI_API_KEY"}"#
    );
}

#[test]
fn credential_union() {
    assert_eq!(
        to_string(&credential::Value::OAuth(credential::OAuth {
            r#type: credential::OAuthType::Value,
            method_id: "github".to_string(),
            refresh: "r".to_string(),
            access: "a".to_string(),
            expires: 3600,
            metadata: None,
        })),
        r#"{"type":"oauth","methodID":"github","refresh":"r","access":"a","expires":3600}"#
    );
    assert_eq!(
        to_string(&credential::Value::Key(credential::Key {
            r#type: credential::KeyType::Value,
            key: "k".to_string(),
            metadata: None,
        })),
        r#"{"type":"key","key":"k"}"#
    );
}

#[test]
fn llm_tool_content() {
    assert_eq!(
        to_string(&llm::ToolContent::Text(llm::ToolTextContent {
            r#type: llm::ToolTextContentType::Value,
            text: "hi".to_string(),
        })),
        r#"{"type":"text","text":"hi"}"#
    );
    assert_eq!(
        to_string(&llm::ToolContent::File(llm::ToolFileContent {
            r#type: llm::ToolFileContentType::Value,
            uri: "file:///a.txt".to_string(),
            mime: "text/plain".to_string(),
            name: Some("a.txt".to_string()),
        })),
        r#"{"type":"file","uri":"file:///a.txt","mime":"text/plain","name":"a.txt"}"#
    );
}

#[test]
fn location_ref() {
    assert_eq!(
        to_string(&location::Ref {
            directory: "/home/u/proj".to_string(),
            workspace_id: None,
        }),
        r#"{"directory":"/home/u/proj"}"#
    );
    assert_eq!(
        to_string(&location::Ref {
            directory: "/home/u/proj".to_string(),
            workspace_id: Some("wrk_1".to_string()),
        }),
        r#"{"directory":"/home/u/proj","workspaceID":"wrk_1"}"#
    );
}

#[test]
fn location_info() {
    let info = location::Info {
        directory: "/d".to_string(),
        workspace_id: None,
        project: location::Project {
            id: "global".to_string(),
            directory: "/d".to_string(),
        },
    };
    assert_eq!(
        to_string(&info),
        r#"{"directory":"/d","project":{"id":"global","directory":"/d"}}"#
    );
}

#[test]
fn provider_empty() {
    let info = provider::empty("anthropic".to_string());
    assert_eq!(
        to_string(&info),
        r#"{"id":"anthropic","name":"anthropic","api":{"type":"native","settings":{}},"request":{"headers":{},"body":{}}}"#
    );
}

#[test]
fn provider_api_union() {
    assert_eq!(
        to_string(&provider::Api::Aisdk(provider::AISDK {
            r#type: provider::AISDKType::Value,
            package: "@ai-sdk/openai".to_string(),
            url: Some("https://api.openai.com".to_string()),
            settings: None,
        })),
        r#"{"type":"aisdk","package":"@ai-sdk/openai","url":"https://api.openai.com"}"#
    );
}

#[test]
fn provider_info_full() {
    let info = provider::Info {
        id: "azure".to_string(),
        integration_id: Some("azure".to_string()),
        name: "Azure".to_string(),
        disabled: Some(false),
        api: provider::Api::Native(provider::Native {
            r#type: provider::NativeType::Value,
            url: Some("https://x".to_string()),
            settings: IndexMap::from([("apiVersion".to_string(), Value::from("2024-06-01"))]),
        }),
        request: provider::Request {
            headers: IndexMap::new(),
            body: IndexMap::new(),
        },
    };
    assert_eq!(
        to_string(&info),
        r#"{"id":"azure","integrationID":"azure","name":"Azure","disabled":false,"api":{"type":"native","url":"https://x","settings":{"apiVersion":"2024-06-01"}},"request":{"headers":{},"body":{}}}"#
    );
}

#[test]
fn model_empty() {
    let info = model::empty("anthropic".to_string(), "claude-3".to_string());
    assert_eq!(
        to_string(&info),
        r#"{"id":"claude-3","providerID":"anthropic","name":"claude-3","api":{"id":"claude-3","type":"native","settings":{}},"capabilities":{"tools":false,"input":[],"output":[]},"request":{"headers":{},"body":{}},"variants":[],"time":{"released":0},"cost":[],"status":"active","enabled":true,"limit":{"context":0,"output":0}}"#
    );
}

#[test]
fn model_api_union() {
    assert_eq!(
        to_string(&model::Api::Aisdk(model::AisdkApi {
            id: "m1".to_string(),
            r#type: model::AisdkApiType::Value,
            package: "@ai-sdk/x".to_string(),
            url: None,
            settings: None,
        })),
        r#"{"id":"m1","type":"aisdk","package":"@ai-sdk/x"}"#
    );
    assert_eq!(
        to_string(&model::Api::Native(model::NativeApi {
            id: "m1".to_string(),
            r#type: model::NativeApiType::Value,
            url: None,
            settings: IndexMap::new(),
        })),
        r#"{"id":"m1","type":"native","settings":{}}"#
    );
}

#[test]
fn model_cost() {
    let cost = model::Cost {
        tier: Some(model::CostTier {
            r#type: model::CostTierType::Value,
            size: 200_000,
        }),
        input: oc_schema::Finite(3e-7),
        output: oc_schema::Finite(15e-7),
        cache: model::CostCache {
            read: oc_schema::Finite(1.5e-7),
            write: oc_schema::Finite(3.0e-7),
        },
    };
    assert_eq!(
        to_string(&cost),
        r#"{"tier":{"type":"context","size":200000},"input":3e-7,"output":0.0000015,"cache":{"read":1.5e-7,"write":3e-7}}"#
    );
}

#[test]
fn session_info() {
    let info = session::Info {
        id: "ses_1".to_string(),
        parent_id: None,
        project_id: "global".to_string(),
        agent: None,
        model: None,
        cost: oc_schema::Finite(0.0),
        tokens: session::TokenUsage {
            input: oc_schema::Finite(10.0),
            output: oc_schema::Finite(5.0),
            reasoning: oc_schema::Finite(0.0),
            cache: session::TokenCache {
                read: oc_schema::Finite(1.0),
                write: oc_schema::Finite(0.0),
            },
        },
        time: session::Time {
            created: 1_700_000_000_000,
            updated: 1_700_000_000_001,
            archived: None,
        },
        title: "Fix".to_string(),
        location: location::Ref {
            directory: "/home/u/proj".to_string(),
            workspace_id: None,
        },
        subpath: None,
        revert: None,
    };
    assert_eq!(
        to_string(&info),
        r#"{"id":"ses_1","projectID":"global","cost":0,"tokens":{"input":10,"output":5,"reasoning":0,"cache":{"read":1,"write":0}},"time":{"created":1700000000000,"updated":1700000000001},"title":"Fix","location":{"directory":"/home/u/proj"}}"#
    );
}

#[test]
fn session_list_anchor() {
    let anchor = session::ListAnchor {
        id: "ses_1".to_string(),
        time: oc_schema::Finite(1_700_000_000_000.0),
        direction: session::Direction::Next,
    };
    assert_eq!(
        to_string(&anchor),
        r#"{"id":"ses_1","time":1700000000000,"direction":"next"}"#
    );
}

#[test]
fn session_input_admitted() {
    let admitted = session_input::Admitted {
        admitted_seq: 0,
        id: "msg_1".to_string(),
        session_id: "ses_1".to_string(),
        prompt: oc_schema::Prompt {
            text: "hi".to_string(),
            files: None,
            agents: None,
        },
        delivery: oc_schema::session_delivery::Delivery::Steer,
        time_created: 1_700_000_000_000,
        promoted_seq: None,
    };
    assert_eq!(
        to_string(&admitted),
        r#"{"admittedSeq":0,"id":"msg_1","sessionID":"ses_1","prompt":{"text":"hi"},"delivery":"steer","timeCreated":1700000000000}"#
    );
}

#[test]
fn question_info_and_reply() {
    let info = question::Info {
        question: "q".to_string(),
        header: "h".to_string(),
        options: vec![question::Option_ {
            label: "l".to_string(),
            description: "d".to_string(),
        }],
        multiple: Some(true),
        custom: Some(true),
    };
    assert_eq!(
        to_string(&info),
        r#"{"question":"q","header":"h","options":[{"label":"l","description":"d"}],"multiple":true,"custom":true}"#
    );
    let reply = question::Reply {
        answers: vec![vec!["l".to_string()]],
    };
    assert_eq!(to_string(&reply), r#"{"answers":[["l"]]}"#);
}

#[test]
fn question_request() {
    let request = question::Request {
        id: "que_1".to_string(),
        session_id: "ses_1".to_string(),
        questions: vec![question::Info {
            question: "q".to_string(),
            header: "h".to_string(),
            options: Vec::new(),
            multiple: None,
            custom: None,
        }],
        tool: Some(question::Tool {
            message_id: "msg_1".to_string(),
            call_id: "call_1".to_string(),
        }),
    };
    assert_eq!(
        to_string(&request),
        r#"{"id":"que_1","sessionID":"ses_1","questions":[{"question":"q","header":"h","options":[]}],"tool":{"messageID":"msg_1","callID":"call_1"}}"#
    );
}

#[test]
fn revert_state() {
    let state = revert::State {
        message_id: "msg_1".to_string(),
        part_id: None,
        snapshot: Some("s".to_string()),
        diff: Some("d".to_string()),
        files: None,
    };
    assert_eq!(
        to_string(&state),
        r#"{"messageID":"msg_1","snapshot":"s","diff":"d"}"#
    );
}

#[test]
fn revert_file_diff() {
    let diff = revert::FileDiff {
        path: "a.txt".to_string(),
        status: revert::FileDiffStatus::Modified,
        additions: 2,
        deletions: 1,
        patch: "+x\n-y".to_string(),
    };
    assert_eq!(
        to_string(&diff),
        r#"{"path":"a.txt","status":"modified","additions":2,"deletions":1,"patch":"+x\n-y"}"#
    );
}

#[test]
fn file_diff_info() {
    let diff = file_diff::Info {
        file: Some("a.txt".to_string()),
        patch: None,
        additions: oc_schema::Finite(1.0),
        deletions: oc_schema::Finite(0.0),
        status: Some(file_diff::Status::Added),
    };
    assert_eq!(
        to_string(&diff),
        r#"{"file":"a.txt","additions":1,"deletions":0,"status":"added"}"#
    );
}

#[test]
fn filesystem_entry_and_match() {
    let entry = filesystem::Entry {
        path: "src/main.rs".to_string(),
        r#type: filesystem::EntryType::File,
    };
    assert_eq!(to_string(&entry), r#"{"path":"src/main.rs","type":"file"}"#);
    let m = filesystem::Match {
        entry: entry,
        line: 3,
        offset: 12,
        text: "fn main".to_string(),
        submatches: vec![filesystem::Submatch {
            text: "main".to_string(),
            start: 3,
            end: 7,
        }],
    };
    assert_eq!(
        to_string(&m),
        r#"{"entry":{"path":"src/main.rs","type":"file"},"line":3,"offset":12,"text":"fn main","submatches":[{"text":"main","start":3,"end":7}]}"#
    );
}

#[test]
fn pty_info_and_inputs() {
    let info = pty::Info {
        id: "pty_1".to_string(),
        title: "t".to_string(),
        command: "cmd".to_string(),
        args: vec!["a".to_string()],
        cwd: "/c".to_string(),
        status: pty::Status::Running,
        pid: 1234,
        exit_code: None,
    };
    assert_eq!(
        to_string(&info),
        r#"{"id":"pty_1","title":"t","command":"cmd","args":["a"],"cwd":"/c","status":"running","pid":1234}"#
    );
    let input = pty::CreateInput {
        command: Some("ls".to_string()),
        args: None,
        cwd: None,
        title: None,
        env: Some(IndexMap::from([("A".to_string(), "1".to_string())])),
    };
    assert_eq!(to_string(&input), r#"{"command":"ls","env":{"A":"1"}}"#);
}

#[test]
fn pty_ticket() {
    let token = pty_ticket::ConnectToken {
        ticket: "t".to_string(),
        expires_in: 60,
    };
    assert_eq!(to_string(&token), r#"{"ticket":"t","expires_in":60}"#);
}

#[test]
fn skill_sources() {
    let dir = skill::Source::Directory(skill::DirectorySource {
        r#type: skill::DirectorySourceType::Value,
        path: "/s".to_string(),
    });
    assert_eq!(to_string(&dir), r#"{"type":"directory","path":"/s"}"#);
    assert_eq!(dir.key(), "directory:/s");
    let embedded = skill::Source::Embedded(skill::EmbeddedSource {
        r#type: skill::EmbeddedSourceType::Value,
        skill: skill::Info {
            name: "n".to_string(),
            description: None,
            slash: None,
            location: "/l".to_string(),
            content: "c".to_string(),
        },
    });
    assert_eq!(
        to_string(&embedded),
        r#"{"type":"embedded","skill":{"name":"n","location":"/l","content":"c"}}"#
    );
    assert_eq!(embedded.key(), "embedded:n");
    assert!(!skill::Source::equals(&dir, &embedded));
}

#[test]
fn prompt_and_attachments() {
    let prompt = oc_schema::Prompt {
        text: "hi".to_string(),
        files: Some(vec![oc_schema::FileAttachment {
            uri: "file:///a.txt".to_string(),
            mime: "text/plain".to_string(),
            name: Some("a.txt".to_string()),
            description: None,
            source: Some(oc_schema::Source {
                start: oc_schema::Finite(0.0),
                end: oc_schema::Finite(5.0),
                text: "hello".to_string(),
            }),
        }]),
        agents: Some(vec![oc_schema::AgentAttachment {
            name: "sub".to_string(),
            source: None,
        }]),
    };
    assert_eq!(
        to_string(&prompt),
        r#"{"text":"hi","files":[{"uri":"file:///a.txt","mime":"text/plain","name":"a.txt","source":{"start":0,"end":5,"text":"hello"}}],"agents":[{"name":"sub"}]}"#
    );
}
