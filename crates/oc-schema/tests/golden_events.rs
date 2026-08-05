//! Golden serialization tests for the event modules.
//! Expected strings are derived manually from the reference zod schemas in
//! reference/packages/schema/src/.

use serde_json::Value;

fn to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

#[test]
fn catalog_updated() {
    let e = oc_schema::catalog::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::catalog::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"catalog.updated","data":{}}"#
    );
}

#[test]
fn models_dev_refreshed() {
    let e = oc_schema::models_dev::Refreshed {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::models_dev::RefreshedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"models-dev.refreshed","data":{}}"#
    );
}

#[test]
fn lsp_updated() {
    let e = oc_schema::lsp_event::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::lsp_event::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"lsp.updated","data":{}}"#
    );
}

#[test]
fn server_events() {
    let connected = oc_schema::server_event::Connected {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::server_event::ConnectedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&connected),
        r#"{"id":"evt_1","type":"server.connected","data":{}}"#
    );
    let disposed = oc_schema::server_event::Disposed {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::server_event::DisposedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&disposed),
        r#"{"id":"evt_1","type":"global.disposed","data":{}}"#
    );
}

#[test]
fn reference_updated_and_info() {
    let updated = oc_schema::reference::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::reference::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&updated),
        r#"{"id":"evt_1","type":"reference.updated","data":{}}"#
    );
    let info = oc_schema::reference::Info {
        name: "r".to_string(),
        path: "/r".to_string(),
        description: Some("d".to_string()),
        hidden: None,
        source: oc_schema::reference::Source::Git(oc_schema::reference::GitSource {
            r#type: oc_schema::reference::GitSourceType::Value,
            repository: "repo".to_string(),
            branch: Some("main".to_string()),
            description: None,
            hidden: None,
        }),
    };
    assert_eq!(
        to_string(&info),
        r#"{"name":"r","path":"/r","description":"d","source":{"type":"git","repository":"repo","branch":"main"}}"#
    );
}

#[test]
fn integration_events() {
    let updated = oc_schema::integration::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::integration::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::schema::Empty {},
    };
    assert_eq!(
        to_string(&updated),
        r#"{"id":"evt_1","type":"integration.updated","data":{}}"#
    );
    let connection = oc_schema::integration::ConnectionUpdated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::integration::ConnectionUpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::integration::ConnectionUpdatedData {
            integration_id: "github".to_string(),
        },
    };
    assert_eq!(
        to_string(&connection),
        r#"{"id":"evt_1","type":"integration.connection.updated","data":{"integrationID":"github"}}"#
    );
}

#[test]
fn integration_method_union() {
    let method = oc_schema::integration::Method::OAuth(oc_schema::integration::OAuthMethod {
        id: "oauth".to_string(),
        r#type: oc_schema::integration::OAuthMethodType::Value,
        label: "OAuth".to_string(),
        prompts: Some(vec![oc_schema::integration::Prompt::Select(
            oc_schema::integration::SelectPrompt {
                r#type: oc_schema::integration::SelectPromptType::Value,
                key: "scope".to_string(),
                message: "Pick".to_string(),
                options: vec![oc_schema::integration::SelectOption {
                    label: "l".to_string(),
                    value: "v".to_string(),
                    hint: Some("h".to_string()),
                }],
                when: None,
            },
        )]),
    });
    assert_eq!(
        to_string(&method),
        r#"{"id":"oauth","type":"oauth","label":"OAuth","prompts":[{"type":"select","key":"scope","message":"Pick","options":[{"label":"l","value":"v","hint":"h"}]}]}"#
    );
}

#[test]
fn integration_attempt_status() {
    let status = oc_schema::integration::AttemptStatus::Failed(
        oc_schema::integration::AttemptStatusFailed {
            status: oc_schema::integration::AttemptStatusFailedStatus::Value,
            message: "nope".to_string(),
            time: oc_schema::integration::AttemptTime {
                created: oc_schema::Finite(1.0),
                expires: oc_schema::Finite(2.0),
            },
        },
    );
    assert_eq!(
        to_string(&status),
        r#"{"status":"failed","message":"nope","time":{"created":1,"expires":2}}"#
    );
}

#[test]
fn permission_request_and_events() {
    let request = oc_schema::permission::Request {
        id: "per_1".to_string(),
        session_id: "ses_1".to_string(),
        action: "write".to_string(),
        resources: vec!["/a".to_string()],
        save: Some(vec!["once".to_string()]),
        metadata: None,
        source: Some(oc_schema::permission::Source {
            r#type: oc_schema::permission::SourceType::Value,
            message_id: "msg_1".to_string(),
            call_id: "c1".to_string(),
        }),
    };
    assert_eq!(
        to_string(&request),
        r#"{"id":"per_1","sessionID":"ses_1","action":"write","resources":["/a"],"save":["once"],"source":{"type":"tool","messageID":"msg_1","callID":"c1"}}"#
    );
    let asked = oc_schema::permission::Asked {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::permission::AskedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::permission::RequestFields {
            session_id: "ses_1".to_string(),
            action: "write".to_string(),
            resources: vec!["/a".to_string()],
            save: None,
            metadata: None,
            source: None,
        },
    };
    assert_eq!(
        to_string(&asked),
        r#"{"id":"evt_1","type":"permission.v2.asked","data":{"sessionID":"ses_1","action":"write","resources":["/a"]}}"#
    );
}

#[test]
fn permission_saved_info() {
    let info = oc_schema::permission_saved::Info {
        id: "psv_1".to_string(),
        project_id: "global".to_string(),
        action: "write".to_string(),
        resource: "/a".to_string(),
    };
    assert_eq!(
        to_string(&info),
        r#"{"id":"psv_1","projectID":"global","action":"write","resource":"/a"}"#
    );
}

#[test]
fn plugin_added() {
    let e = oc_schema::plugin::Added {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::plugin::AddedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::plugin::AddedData {
            id: "plugin_1".to_string(),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"plugin.added","data":{"id":"plugin_1"}}"#
    );
}

#[test]
fn project_info_and_event() {
    let info = oc_schema::project::Info {
        id: "global".to_string(),
        worktree: "/w".to_string(),
        vcs: Some(oc_schema::project::Vcs::Git),
        name: Some("proj".to_string()),
        icon: Some(oc_schema::project::Icon {
            url: Some("icon.png".to_string()),
            override_: Some("glyph".to_string()),
            color: Some("#fff".to_string()),
        }),
        commands: Some(oc_schema::project::Commands {
            start: Some("npm run dev".to_string()),
        }),
        time: oc_schema::project::Time {
            created: 1,
            updated: 2,
            initialized: None,
        },
        sandboxes: vec!["sb1".to_string()],
    };
    assert_eq!(
        to_string(&info),
        r##"{"id":"global","worktree":"/w","vcs":"git","name":"proj","icon":{"url":"icon.png","override":"glyph","color":"#fff"},"commands":{"start":"npm run dev"},"time":{"created":1,"updated":2},"sandboxes":["sb1"]}"##
    );
    let e = oc_schema::project::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::project::UpdatedTag::Value,
        durable: None,
        location: None,
        data: info,
    };
    assert_eq!(
        to_string(&e),
        r##"{"id":"evt_1","type":"project.updated","data":{"id":"global","worktree":"/w","vcs":"git","name":"proj","icon":{"url":"icon.png","override":"glyph","color":"#fff"},"commands":{"start":"npm run dev"},"time":{"created":1,"updated":2},"sandboxes":["sb1"]}}"##
    );
}

#[test]
fn project_copy_inputs() {
    let create = oc_schema::project_copy::CreateInput {
        project_id: "global".to_string(),
        strategy: "clone".to_string(),
        source_directory: "/src".to_string(),
        directory: "/dst".to_string(),
        name: Some("copy".to_string()),
    };
    assert_eq!(
        to_string(&create),
        r#"{"projectID":"global","strategy":"clone","sourceDirectory":"/src","directory":"/dst","name":"copy"}"#
    );
    let remove = oc_schema::project_copy::RemoveInput {
        project_id: "global".to_string(),
        directory: "/dst".to_string(),
        force: true,
    };
    assert_eq!(
        to_string(&remove),
        r#"{"projectID":"global","directory":"/dst","force":true}"#
    );
}

#[test]
fn project_directories_updated() {
    let e = oc_schema::project_directories::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::project_directories::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::project_directories::UpdatedData {
            project_id: "global".to_string(),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"project.directories.updated","data":{"projectID":"global"}}"#
    );
}

#[test]
fn filesystem_and_watcher_events() {
    let edited = oc_schema::filesystem::Edited {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::filesystem::EditedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::filesystem::EditedData {
            file: "a.txt".to_string(),
        },
    };
    assert_eq!(
        to_string(&edited),
        r#"{"id":"evt_1","type":"file.edited","data":{"file":"a.txt"}}"#
    );
    let watcher = oc_schema::filesystem_watcher::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::filesystem_watcher::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::filesystem_watcher::UpdatedData {
            file: "a.txt".to_string(),
            event: oc_schema::filesystem_watcher::WatcherEvent::Change,
        },
    };
    assert_eq!(
        to_string(&watcher),
        r#"{"id":"evt_1","type":"file.watcher.updated","data":{"file":"a.txt","event":"change"}}"#
    );
}

#[test]
fn pty_events() {
    let created = oc_schema::pty::Created {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::pty::CreatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::pty::InfoEventData {
            info: oc_schema::pty::Info {
                id: "pty_1".to_string(),
                title: "t".to_string(),
                command: "cmd".to_string(),
                args: Vec::new(),
                cwd: "/c".to_string(),
                status: oc_schema::pty::Status::Running,
                pid: 1,
                exit_code: None,
            },
        },
    };
    assert_eq!(
        to_string(&created),
        r#"{"id":"evt_1","type":"pty.created","data":{"info":{"id":"pty_1","title":"t","command":"cmd","args":[],"cwd":"/c","status":"running","pid":1}}}"#
    );
    let exited = oc_schema::pty::Exited {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::pty::ExitedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::pty::ExitedData {
            id: "pty_1".to_string(),
            exit_code: 0,
        },
    };
    assert_eq!(
        to_string(&exited),
        r#"{"id":"evt_1","type":"pty.exited","data":{"id":"pty_1","exitCode":0}}"#
    );
}

#[test]
fn question_events() {
    let replied = oc_schema::question::Replied {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::question::RepliedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::question::RepliedData {
            session_id: "ses_1".to_string(),
            request_id: "que_1".to_string(),
            answers: vec![vec!["yes".to_string()]],
        },
    };
    assert_eq!(
        to_string(&replied),
        r#"{"id":"evt_1","type":"question.v2.replied","data":{"sessionID":"ses_1","requestID":"que_1","answers":[["yes"]]}}"#
    );
}

#[test]
fn installation_events() {
    let updated = oc_schema::installation_event::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::installation_event::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::installation_event::VersionData {
            version: "1.18.13".to_string(),
        },
    };
    assert_eq!(
        to_string(&updated),
        r#"{"id":"evt_1","type":"installation.updated","data":{"version":"1.18.13"}}"#
    );
    let available = oc_schema::installation_event::UpdateAvailable {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::installation_event::UpdateAvailableTag::Value,
        durable: None,
        location: None,
        data: oc_schema::installation_event::VersionData {
            version: "1.18.14".to_string(),
        },
    };
    assert_eq!(
        to_string(&available),
        r#"{"id":"evt_1","type":"installation.update-available","data":{"version":"1.18.14"}}"#
    );
}

#[test]
fn ide_installed() {
    let e = oc_schema::ide_event::Installed {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::ide_event::InstalledTag::Value,
        durable: None,
        location: None,
        data: oc_schema::ide_event::InstalledData {
            ide: "vscode".to_string(),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"ide.installed","data":{"ide":"vscode"}}"#
    );
}

#[test]
fn mcp_events() {
    let tools = oc_schema::mcp_event::ToolsChanged {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::mcp_event::ToolsChangedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::mcp_event::ToolsChangedData {
            server: "srv".to_string(),
        },
    };
    assert_eq!(
        to_string(&tools),
        r#"{"id":"evt_1","type":"mcp.tools.changed","data":{"server":"srv"}}"#
    );
    let failed = oc_schema::mcp_event::BrowserOpenFailed {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::mcp_event::BrowserOpenFailedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::mcp_event::BrowserOpenFailedData {
            mcp_name: "srv".to_string(),
            url: "https://x".to_string(),
        },
    };
    assert_eq!(
        to_string(&failed),
        r#"{"id":"evt_1","type":"mcp.browser.open.failed","data":{"mcpName":"srv","url":"https://x"}}"#
    );
}

#[test]
fn session_status_events() {
    let status = oc_schema::session_status_event::Status {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::session_status_event::StatusTag::Value,
        durable: None,
        location: None,
        data: oc_schema::session_status_event::StatusData {
            session_id: "ses_1".to_string(),
            status: oc_schema::session_status_event::Info::Retry(
                oc_schema::session_status_event::RetryInfo {
                    r#type: oc_schema::session_status_event::RetryInfoType::Value,
                    attempt: 1,
                    message: "m".to_string(),
                    action: Some(oc_schema::session_status_event::Action {
                        reason: "r".to_string(),
                        provider: "p".to_string(),
                        title: "t".to_string(),
                        message: "m".to_string(),
                        label: "l".to_string(),
                        link: None,
                    }),
                    next: 2,
                },
            ),
        },
    };
    assert_eq!(
        to_string(&status),
        r#"{"id":"evt_1","type":"session.status","data":{"sessionID":"ses_1","status":{"type":"retry","attempt":1,"message":"m","action":{"reason":"r","provider":"p","title":"t","message":"m","label":"l"},"next":2}}}"#
    );
    let idle =
        oc_schema::session_status_event::Info::Idle(oc_schema::session_status_event::IdleInfo {
            r#type: oc_schema::session_status_event::IdleInfoType::Value,
        });
    assert_eq!(to_string(&idle), r#"{"type":"idle"}"#);
}

#[test]
fn session_todo_events() {
    let info = oc_schema::session_todo::Info {
        content: "do it".to_string(),
        status: "in_progress".to_string(),
        priority: "high".to_string(),
    };
    assert_eq!(
        to_string(&info),
        r#"{"content":"do it","status":"in_progress","priority":"high"}"#
    );
    let e = oc_schema::session_todo::Updated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::session_todo::UpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::session_todo::UpdatedData {
            session_id: "ses_1".to_string(),
            todos: vec![info],
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"todo.updated","data":{"sessionID":"ses_1","todos":[{"content":"do it","status":"in_progress","priority":"high"}]}}"#
    );
}

#[test]
fn tui_events() {
    let toast = oc_schema::tui_event::ToastShow {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::tui_event::ToastShowTag::Value,
        durable: None,
        location: None,
        data: oc_schema::tui_event::ToastShowData {
            title: Some("t".to_string()),
            message: "m".to_string(),
            variant: oc_schema::tui_event::ToastVariant::Success,
            duration: 5000,
        },
    };
    assert_eq!(
        to_string(&toast),
        r#"{"id":"evt_1","type":"tui.toast.show","data":{"title":"t","message":"m","variant":"success","duration":5000}}"#
    );
    let execute = oc_schema::tui_event::CommandExecute {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::tui_event::CommandExecuteTag::Value,
        durable: None,
        location: None,
        data: oc_schema::tui_event::CommandExecuteData {
            command: "session.list".to_string(),
        },
    };
    assert_eq!(
        to_string(&execute),
        r#"{"id":"evt_1","type":"tui.command.execute","data":{"command":"session.list"}}"#
    );
    let select = oc_schema::tui_event::SessionSelect {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::tui_event::SessionSelectTag::Value,
        durable: None,
        location: None,
        data: oc_schema::tui_event::SessionSelectData {
            session_id: "ses_1".to_string(),
        },
    };
    assert_eq!(
        to_string(&select),
        r#"{"id":"evt_1","type":"tui.session.select","data":{"sessionID":"ses_1"}}"#
    );
}

#[test]
fn vcs_branch_updated() {
    let e = oc_schema::vcs_event::BranchUpdated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::vcs_event::BranchUpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::vcs_event::BranchUpdatedData {
            branch: Some("main".to_string()),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"vcs.branch.updated","data":{"branch":"main"}}"#
    );
    let none = oc_schema::vcs_event::BranchUpdated {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::vcs_event::BranchUpdatedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::vcs_event::BranchUpdatedData { branch: None },
    };
    assert_eq!(
        to_string(&none),
        r#"{"id":"evt_1","type":"vcs.branch.updated","data":{}}"#
    );
}

#[test]
fn workspace_events() {
    let ready = oc_schema::workspace_event::Ready {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::workspace_event::ReadyTag::Value,
        durable: None,
        location: None,
        data: oc_schema::workspace_event::ReadyData {
            name: "w".to_string(),
        },
    };
    assert_eq!(
        to_string(&ready),
        r#"{"id":"evt_1","type":"workspace.ready","data":{"name":"w"}}"#
    );
    let status = oc_schema::workspace_event::StatusEvent {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::workspace_event::StatusEventTag::Value,
        durable: None,
        location: None,
        data: oc_schema::workspace_event::ConnectionStatus {
            workspace_id: "wrk_1".to_string(),
            status: oc_schema::workspace_event::Status::Connected,
        },
    };
    assert_eq!(
        to_string(&status),
        r#"{"id":"evt_1","type":"workspace.status","data":{"workspaceID":"wrk_1","status":"connected"}}"#
    );
}

#[test]
fn worktree_events() {
    let ready = oc_schema::worktree_event::Ready {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::worktree_event::ReadyTag::Value,
        durable: None,
        location: None,
        data: oc_schema::worktree_event::ReadyData {
            name: "w".to_string(),
            branch: Some("main".to_string()),
        },
    };
    assert_eq!(
        to_string(&ready),
        r#"{"id":"evt_1","type":"worktree.ready","data":{"name":"w","branch":"main"}}"#
    );
}

#[test]
fn session_compacted() {
    let e = oc_schema::session_compaction_event::Compacted {
        id: "evt_1".to_string(),
        metadata: None,
        r#type: oc_schema::session_compaction_event::CompactedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::session_compaction_event::CompactedData {
            session_id: "ses_1".to_string(),
        },
    };
    assert_eq!(
        to_string(&e),
        r#"{"id":"evt_1","type":"session.compacted","data":{"sessionID":"ses_1"}}"#
    );
}

#[test]
fn metadata_roundtrip() {
    let mut metadata = indexmap::IndexMap::new();
    metadata.insert("k".to_string(), Value::from("v"));
    let asked = oc_schema::permission::Asked {
        id: "evt_1".to_string(),
        metadata: Some(metadata),
        r#type: oc_schema::permission::AskedTag::Value,
        durable: None,
        location: None,
        data: oc_schema::permission::RequestFields {
            session_id: "ses_1".to_string(),
            action: "a".to_string(),
            resources: Vec::new(),
            save: None,
            metadata: None,
            source: None,
        },
    };
    assert_eq!(
        to_string(&asked),
        r#"{"id":"evt_1","metadata":{"k":"v"},"type":"permission.v2.asked","data":{"sessionID":"ses_1","action":"a","resources":[]}}"#
    );
}
