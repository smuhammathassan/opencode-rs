//! Golden request/response tests against a local mock server.
//! Mirrors `reference/packages/client/test/promise.test.ts`.

mod common;

use common::{assert_body, assert_request, MockServer};
use futures::StreamExt;
use oc_client::types::*;
use oc_client::{ClientOptions, OpenCode};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn make_client(server: &MockServer) -> OpenCode {
    OpenCode::make(ClientOptions {
        base_url: server.base_url.parse().expect("base url"),
        ..ClientOptions::default()
    })
    .expect("client")
}

fn session_fixture() -> serde_json::Value {
    json!({
        "data": {
            "id": "ses_test",
            "projectID": "project",
            "cost": 0,
            "tokens": { "input": 1, "output": 2, "reasoning": 3, "cache": { "read": 4, "write": 5 } },
            "time": { "created": 1717171717000i64, "updated": 1717171717000i64 },
            "title": "Test",
            "location": { "directory": "/tmp/project" }
        }
    })
}

fn admission_fixture() -> serde_json::Value {
    json!({
        "data": {
            "admittedSeq": 0,
            "id": "msg_test",
            "sessionID": "ses_test",
            "prompt": { "text": "Hello" },
            "delivery": "steer",
            "timeCreated": 1717171717000i64
        }
    })
}

fn model_switched_message() -> serde_json::Value {
    json!({
        "id": "msg_model",
        "type": "model-switched",
        "time": { "created": 1717171717000i64 },
        "model": { "id": "claude", "providerID": "anthropic" }
    })
}

fn model_switched_event() -> serde_json::Value {
    json!({
        "id": "evt_model",
        "type": "session.next.model.switched",
        "durable": { "aggregateID": "ses_test", "seq": 1, "version": 1 },
        "data": {
            "timestamp": 1717171717000i64,
            "sessionID": "ses_test",
            "messageID": "msg_model",
            "model": { "id": "claude", "providerID": "anthropic" }
        }
    })
}

#[tokio::test]
async fn session_methods_use_the_public_http_contract() {
    let session = session_fixture();
    let admission = admission_fixture();
    let model_switched_message = model_switched_message();
    let model_switched_event = model_switched_event();
    let history_page = std::sync::atomic::AtomicUsize::new(0);

    let responder = Arc::new(
        move |recorded: &common::RecordedRequest| -> axum::response::Response<axum::body::Body> {
            let path = &recorded.path;
            if path.contains("/event") {
                return common::sse_response(200, &format!("data: {}\n\n", model_switched_event));
            }
            if path.contains("/history") {
                let page = history_page.fetch_add(1, Ordering::SeqCst);
                let body = if page == 0 {
                    json!({ "data": [model_switched_event], "hasMore": true })
                } else {
                    json!({ "data": [], "hasMore": false })
                };
                return common::json_response(200, &body);
            }
            if path.contains("/prompt") {
                return common::json_response(200, &admission);
            }
            if path.contains("/context") {
                return common::json_response(200, &json!({ "data": [] }));
            }
            if path.contains("/message/") {
                return common::json_response(200, &json!({ "data": model_switched_message }));
            }
            if path.ends_with("/api/session/active") {
                return common::json_response(
                    200,
                    &json!({ "data": { "ses_test": { "type": "running" } } }),
                );
            }
            if recorded.method == "POST" && path.ends_with("/api/session") {
                return common::json_response(200, &session);
            }
            if recorded.method == "POST" {
                return common::no_content();
            }
            common::json_response(
                200,
                &json!({ "data": [session["data"]], "cursor": { "next": "next" } }),
            )
        },
    );
    let server = MockServer::spawn(responder).await;
    let client = make_client(&server);

    let page = client
        .sessions
        .list(
            Some(&SessionsListInput {
                limit: Some(10),
                order: Some(Order::Desc),
                ..Default::default()
            }),
            None,
        )
        .await
        .expect("sessions.list");
    assert_eq!(page.cursor.next.as_deref(), Some("next"));
    assert_eq!(page.data[0].id, "ses_test");

    let active = client.sessions.active(None).await.expect("sessions.active");
    assert_eq!(
        active.get("ses_test").map(|a| a.kind),
        Some(SessionActiveType::Running)
    );

    let created = client
        .sessions
        .create(
            Some(&SessionsCreateInput {
                location: Some(SessionCreateLocation {
                    directory: "/tmp/project".into(),
                    workspace_id: None,
                }),
                ..Default::default()
            }),
            None,
        )
        .await
        .expect("sessions.create");
    assert_eq!(created.id, "ses_test");

    client
        .sessions
        .switch_agent(
            &SessionsSwitchAgentInput {
                session_id: "ses_test".into(),
                agent: "build".into(),
            },
            None,
        )
        .await
        .expect("switchAgent");
    client
        .sessions
        .switch_model(
            &SessionsSwitchModelInput {
                session_id: "ses_test".into(),
                model: ModelRef {
                    id: "claude".into(),
                    provider_id: "anthropic".into(),
                    variant: None,
                },
            },
            None,
        )
        .await
        .expect("switchModel");

    let admitted = client
        .sessions
        .prompt(
            &SessionsPromptInput {
                session_id: "ses_test".into(),
                id: None,
                prompt: PromptInput {
                    text: "Hello".into(),
                    files: None,
                    agents: None,
                },
                delivery: None,
                resume: Some(false),
            },
            None,
        )
        .await
        .expect("prompt");
    assert_eq!(admitted.id, "msg_test");
    assert_eq!(admitted.time_created, 1717171717000i64);

    client
        .sessions
        .compact(
            &SessionIDInput {
                session_id: "ses_test".into(),
            },
            None,
        )
        .await
        .expect("compact");
    client
        .sessions
        .wait(
            &SessionIDInput {
                session_id: "ses_test".into(),
            },
            None,
        )
        .await
        .expect("wait");

    let context = client
        .sessions
        .context(
            &SessionIDInput {
                session_id: "ses_test".into(),
            },
            None,
        )
        .await
        .expect("context");
    assert!(context.is_empty());

    let history = client
        .sessions
        .history(
            &SessionsHistoryInput {
                session_id: "ses_test".into(),
                limit: Some(1),
                after: Some(0),
            },
            None,
        )
        .await
        .expect("history");
    assert!(history.has_more);
    let after = match &history.data[0] {
        SessionDurableEvent::ModelSwitched { durable, .. } => {
            durable.as_ref().map(|d| d.seq as u64)
        }
        _ => None,
    };
    let history_next = if history.has_more {
        client
            .sessions
            .history(
                &SessionsHistoryInput {
                    session_id: "ses_test".into(),
                    limit: Some(2),
                    after,
                },
                None,
            )
            .await
            .expect("history next")
    } else {
        unreachable!()
    };
    assert!(history_next.data.is_empty());
    assert!(!history_next.has_more);

    let events: Vec<SessionDurableEvent> = client
        .sessions
        .events(
            &SessionsEventsInput {
                session_id: "ses_test".into(),
                after: Some(0),
            },
            None,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|item| item.expect("sse item"))
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type(), "session.next.model.switched");

    client
        .sessions
        .interrupt(
            &SessionIDInput {
                session_id: "ses_test".into(),
            },
            None,
        )
        .await
        .expect("interrupt");

    let message = client
        .sessions
        .message(
            &SessionsMessageInput {
                session_id: "ses_test".into(),
                message_id: "msg_model".into(),
            },
            None,
        )
        .await
        .expect("message");
    match message {
        SessionMessage::ModelSwitched { model, .. } => assert_eq!(model.id, "claude"),
        _ => panic!("expected model-switched message"),
    }

    let requests = server.recorded();
    let expected = [
        "GET /api/session?limit=10&order=desc",
        "GET /api/session/active",
        "POST /api/session",
        "POST /api/session/ses_test/agent",
        "POST /api/session/ses_test/model",
        "POST /api/session/ses_test/prompt",
        "POST /api/session/ses_test/compact",
        "POST /api/session/ses_test/wait",
        "GET /api/session/ses_test/context",
        "GET /api/session/ses_test/history?limit=1&after=0",
        "GET /api/session/ses_test/history?limit=2&after=1",
        "GET /api/session/ses_test/event?after=0",
        "POST /api/session/ses_test/interrupt",
        "GET /api/session/ses_test/message/msg_model",
    ];
    let actual: Vec<String> = requests
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    assert_eq!(actual, expected);

    let prompt = requests
        .iter()
        .find(|r| r.path.ends_with("/prompt"))
        .expect("prompt request");
    assert_body(
        prompt,
        &json!({ "prompt": { "text": "Hello" }, "resume": false }),
    );

    let create = &requests[2];
    assert_body(
        create,
        &json!({ "location": { "directory": "/tmp/project" } }),
    );
}

#[tokio::test]
async fn middleware_errors_remain_declared_client_errors() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::error_response(
            401,
            "UnauthorizedError",
            &[("message", &json!("Authentication required"))],
        )
    }))
    .await;
    let client = make_client(&server);
    let err = client
        .sessions
        .create(None, None)
        .await
        .expect_err("should fail");
    assert!(err.is_unauthorized());
}

#[tokio::test]
async fn sessions_history_decodes_session_not_found() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::error_response(
            404,
            "SessionNotFoundError",
            &[
                ("sessionID", &json!("ses_missing")),
                ("message", &json!("Session not found")),
            ],
        )
    }))
    .await;
    let client = make_client(&server);
    let err = client
        .sessions
        .history(
            &SessionsHistoryInput {
                session_id: "ses_missing".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect_err("should fail");
    assert!(err.is_session_not_found());
}

#[tokio::test]
async fn undeclared_status_becomes_unexpected_status() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(418, &json!({ "weird": true }))
    }))
    .await;
    let client = make_client(&server);
    let err = client.sessions.active(None).await.expect_err("should fail");
    match err {
        oc_client::Error::Client(oc_client::ClientError::UnexpectedStatus(418)) => {}
        other => panic!("expected UnexpectedStatus(418), got {other:?}"),
    }
}

#[tokio::test]
async fn location_query_encodes_as_deep_object() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(
            200,
            &json!({ "directory": "/tmp/project", "project": { "id": "global", "directory": "/tmp/project" } }),
        )
    }))
    .await;
    let client = make_client(&server);
    let info = client
        .location
        .get(
            Some(&LocationInput {
                location: Some(LocationQueryRef {
                    directory: Some("/tmp/project".into()),
                    workspace: None,
                }),
            }),
            None,
        )
        .await
        .expect("location.get");
    assert_eq!(info.directory, "/tmp/project");

    let requests = server.recorded();
    assert_request(
        &requests,
        0,
        "GET",
        "/api/location?location%5Bdirectory%5D=%2Ftmp%2Fproject",
    );
}

#[tokio::test]
async fn location_scoped_list_endpoints_send_location_query() {
    let location_body = json!({
        "location": {
            "directory": "/tmp/project",
            "project": { "id": "global", "directory": "/tmp/project" }
        },
        "data": []
    });
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        common::json_response(200, &location_body)
    }))
    .await;
    let client = make_client(&server);
    let input = LocationInput {
        location: Some(LocationQueryRef {
            directory: Some("/tmp/project".into()),
            workspace: None,
        }),
    };

    client
        .agents
        .list(Some(&input), None)
        .await
        .expect("agents.list");
    client
        .models
        .list(Some(&input), None)
        .await
        .expect("models.list");
    client
        .providers
        .list(Some(&input), None)
        .await
        .expect("providers.list");
    client
        .integrations
        .list(Some(&input), None)
        .await
        .expect("integrations.list");
    client
        .commands
        .list(Some(&input), None)
        .await
        .expect("commands.list");
    client
        .skills
        .list(Some(&input), None)
        .await
        .expect("skills.list");
    client
        .permissions
        .list_requests(Some(&input), None)
        .await
        .expect("permissions.listRequests");
    client
        .questions
        .list_requests(Some(&input), None)
        .await
        .expect("questions.listRequests");
    client
        .references
        .list(Some(&input), None)
        .await
        .expect("references.list");
    client
        .ptys
        .list(Some(&input), None)
        .await
        .expect("ptys.list");

    for (i, path) in [
        "/api/agent?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/model?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/provider?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/integration?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/command?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/skill?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/permission/request?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/question/request?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/reference?location%5Bdirectory%5D=%2Ftmp%2Fproject",
        "/api/pty?location%5Bdirectory%5D=%2Ftmp%2Fproject",
    ]
    .iter()
    .enumerate()
    {
        assert_request(&server.recorded(), i, "GET", path);
    }
}

#[tokio::test]
async fn files_list_and_find_encode_query_params() {
    let files_body = json!({
        "location": {
            "directory": "/tmp/project",
            "project": { "id": "global", "directory": "/tmp/project" }
        },
        "data": [ { "path": "src/main.rs", "type": "file" } ]
    });
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        common::json_response(200, &files_body)
    }))
    .await;
    let client = make_client(&server);

    let entries = client
        .files
        .list(
            Some(&FilesListInput {
                location: None,
                path: Some("src".into()),
            }),
            None,
        )
        .await
        .expect("files.list");
    assert_eq!(entries.data[0].path, "src/main.rs");

    client
        .files
        .find(
            &FilesFindInput {
                location: None,
                query: "main".into(),
                kind: Some(FileSystemEntryType::File),
                limit: Some(5),
            },
            None,
        )
        .await
        .expect("files.find");

    let requests = server.recorded();
    assert_request(&requests, 0, "GET", "/api/fs/list?path=src");
    assert_request(
        &requests,
        1,
        "GET",
        "/api/fs/find?query=main&type=file&limit=5",
    );
}

#[tokio::test]
async fn permissions_send_flattened_create_body() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(200, &json!({ "data": { "id": "per_1", "effect": "ask" } }))
    }))
    .await;
    let client = make_client(&server);

    client
        .permissions
        .create(
            &PermissionsCreateInput {
                session_id: "ses_test".into(),
                id: None,
                action: "bash".into(),
                resources: vec!["pwd".into()],
                save: Some(vec!["pwd".into()]),
                metadata: None,
                source: Some(PermissionSource {
                    kind: PermissionSourceType::Tool,
                    message_id: "msg_1".into(),
                    call_id: "call_1".into(),
                }),
                agent: None,
            },
            None,
        )
        .await
        .expect("permissions.create");

    let requests = server.recorded();
    assert_request(&requests, 0, "POST", "/api/session/ses_test/permission");
    assert_body(
        &requests[0],
        &json!({
            "action": "bash",
            "resources": ["pwd"],
            "save": ["pwd"],
            "source": { "type": "tool", "messageID": "msg_1", "callID": "call_1" }
        }),
    );
}

#[tokio::test]
async fn integrations_connect_oauth_serializes_body() {
    let attempt = json!({
        "location": {
            "directory": "/tmp/project",
            "project": { "id": "global", "directory": "/tmp/project" }
        },
        "data": {
            "attemptID": "con_1",
            "url": "https://example.com/authorize",
            "instructions": "Open the URL",
            "mode": "code",
            "time": { "created": 1717171717000i64, "expires": 1717171817000i64 }
        }
    });
    let server = MockServer::spawn(Arc::new(move |recorded: &common::RecordedRequest| {
        if recorded.path.contains("/complete") {
            common::no_content()
        } else if recorded.method == "POST" {
            common::json_response(200, &attempt)
        } else {
            common::no_content()
        }
    }))
    .await;
    let client = make_client(&server);

    let result = client
        .integrations
        .connect_oauth(
            &IntegrationsConnectOauthInput {
                integration_id: "github".into(),
                location: None,
                method_id: "oauth".into(),
                inputs: [("client_id".to_string(), "abc".to_string())]
                    .into_iter()
                    .collect(),
                label: Some("work".into()),
            },
            None,
        )
        .await
        .expect("connectOauth");
    assert_eq!(result.data.attempt_id, "con_1");

    let requests = server.recorded();
    assert_request(
        &requests,
        0,
        "POST",
        "/api/integration/github/connect/oauth",
    );
    assert_body(
        &requests[0],
        &json!({ "methodID": "oauth", "inputs": { "client_id": "abc" }, "label": "work" }),
    );

    client
        .integrations
        .attempt_complete(
            &IntegrationsAttemptCompleteInput {
                attempt_id: "con_1".into(),
                location: None,
                code: Some("1234".into()),
            },
            None,
        )
        .await
        .expect("attemptComplete");
    assert_request(
        &server.recorded(),
        1,
        "POST",
        "/api/integration/attempt/con_1/complete",
    );
    assert_body(&server.recorded()[1], &json!({ "code": "1234" }));
}

#[tokio::test]
async fn credentials_update_uses_patch_and_body() {
    let server =
        MockServer::spawn(Arc::new(|_: &common::RecordedRequest| common::no_content())).await;
    let client = make_client(&server);

    client
        .credentials
        .update(
            &CredentialsUpdateInput {
                credential_id: "cred_1".into(),
                location: None,
                label: "work".into(),
            },
            None,
        )
        .await
        .expect("credentials.update");

    let requests = server.recorded();
    assert_request(&requests, 0, "PATCH", "/api/credential/cred_1");
    assert_body(&requests[0], &json!({ "label": "work" }));
}

#[tokio::test]
async fn ptys_create_and_update_serialize_bodies() {
    let pty = json!({
        "location": {
            "directory": "/tmp/project",
            "project": { "id": "global", "directory": "/tmp/project" }
        },
        "data": {
            "id": "pty_1",
            "title": "bash",
            "command": "bash",
            "args": [],
            "cwd": "/tmp/project",
            "status": "running",
            "pid": 42
        }
    });
    let server = MockServer::spawn(Arc::new(move |_recorded: &common::RecordedRequest| {
        common::json_response(200, &pty)
    }))
    .await;
    let client = make_client(&server);

    let created = client
        .ptys
        .create(
            Some(&PtyCreateInput {
                location: None,
                command: Some("bash".into()),
                cwd: Some("/tmp/project".into()),
                title: Some("bash".into()),
                ..Default::default()
            }),
            None,
        )
        .await
        .expect("ptys.create");
    assert_eq!(created.data.pid, 42);

    client
        .ptys
        .update(
            &PtyUpdateInput {
                pty_id: "pty_1".into(),
                location: None,
                title: Some("new title".into()),
                size: Some(PtySize { rows: 24, cols: 80 }),
            },
            None,
        )
        .await
        .expect("ptys.update");

    let requests = server.recorded();
    assert_request(&requests, 0, "POST", "/api/pty");
    assert_body(
        &requests[0],
        &json!({ "command": "bash", "cwd": "/tmp/project", "title": "bash" }),
    );
    assert_request(&requests, 1, "PUT", "/api/pty/pty_1");
    assert_body(
        &requests[1],
        &json!({ "title": "new title", "size": { "rows": 24, "cols": 80 } }),
    );
}

#[tokio::test]
async fn questions_reply_and_reject() {
    let server =
        MockServer::spawn(Arc::new(|_: &common::RecordedRequest| common::no_content())).await;
    let client = make_client(&server);

    client
        .questions
        .reply(
            &QuestionsReplyInput {
                session_id: "ses_test".into(),
                request_id: "que_1".into(),
                answers: vec![vec!["Yes".into()]],
            },
            None,
        )
        .await
        .expect("questions.reply");
    client
        .questions
        .reject(
            &QuestionsRejectInput {
                session_id: "ses_test".into(),
                request_id: "que_1".into(),
            },
            None,
        )
        .await
        .expect("questions.reject");

    let requests = server.recorded();
    assert_request(
        &requests,
        0,
        "POST",
        "/api/session/ses_test/question/que_1/reply",
    );
    assert_body(&requests[0], &json!({ "answers": [["Yes"]] }));
    assert_request(
        &requests,
        1,
        "POST",
        "/api/session/ses_test/question/que_1/reject",
    );
}

#[tokio::test]
async fn project_copies_create_and_remove() {
    let server = MockServer::spawn(Arc::new(|recorded: &common::RecordedRequest| {
        if recorded.method == "POST" && recorded.path.contains("/copy/refresh") {
            common::no_content()
        } else if recorded.method == "POST" {
            common::json_response(200, &json!({ "directory": "/copies/project" }))
        } else {
            common::no_content()
        }
    }))
    .await;
    let client = make_client(&server);

    let copy = client
        .project_copies
        .create(
            &ProjectCopyCreateInput {
                project_id: "global".into(),
                location: None,
                strategy: "cp".into(),
                directory: "/copies/project".into(),
                name: None,
            },
            None,
        )
        .await
        .expect("projectCopies.create");
    assert_eq!(copy.directory, "/copies/project");

    client
        .project_copies
        .remove(
            &ProjectCopyRemoveInput {
                project_id: "global".into(),
                location: None,
                directory: "/copies/project".into(),
                force: true,
            },
            None,
        )
        .await
        .expect("projectCopies.remove");
    client
        .project_copies
        .refresh(
            &ProjectCopyRefreshInput {
                project_id: "global".into(),
                location: None,
            },
            None,
        )
        .await
        .expect("projectCopies.refresh");

    let requests = server.recorded();
    assert_request(&requests, 0, "POST", "/experimental/project/global/copy");
    assert_body(
        &requests[0],
        &json!({ "strategy": "cp", "directory": "/copies/project" }),
    );
    assert_request(&requests, 1, "DELETE", "/experimental/project/global/copy");
    assert_body(
        &requests[1],
        &json!({ "directory": "/copies/project", "force": true }),
    );
    assert_request(
        &requests,
        2,
        "POST",
        "/experimental/project/global/copy/refresh",
    );
}

#[tokio::test]
async fn custom_request_headers_are_sent() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(200, &json!({ "healthy": true }))
    }))
    .await;
    let client = make_client(&server);
    let mut options = oc_client::RequestOptions::default();
    options
        .headers
        .insert("x-opencode-directory", "dir".parse().unwrap());
    client.health.get(Some(&options)).await.expect("health.get");
    let requests = server.recorded();
    assert_eq!(
        requests[0]
            .headers
            .get("x-opencode-directory")
            .map(|v| v.to_str().unwrap()),
        Some("dir")
    );
}

#[tokio::test]
async fn provider_get_encodes_path_and_location() {
    let provider = json!({
        "location": {
            "directory": "/tmp/project",
            "project": { "id": "global", "directory": "/tmp/project" }
        },
        "data": {
            "id": "anthropic",
            "name": "Anthropic",
            "api": { "type": "native", "settings": {} },
            "request": { "headers": {}, "body": {} }
        }
    });
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        common::json_response(200, &provider)
    }))
    .await;
    let client = make_client(&server);
    client
        .providers
        .get(
            &ProvidersGetInput {
                provider_id: "anthropic".into(),
                location: None,
            },
            None,
        )
        .await
        .expect("providers.get");
    assert_request(&server.recorded(), 0, "GET", "/api/provider/anthropic");
}

#[tokio::test]
async fn permission_saved_list_filters_by_project() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(200, &json!({ "data": [] }))
    }))
    .await;
    let client = make_client(&server);
    client
        .permissions
        .list_saved(
            Some(&PermissionsListSavedInput {
                project_id: Some("global".into()),
            }),
            None,
        )
        .await
        .expect("permissions.listSaved");
    assert_request(
        &server.recorded(),
        0,
        "GET",
        "/api/permission/saved?projectID=global",
    );
}

#[tokio::test]
async fn retry_policy_retries_5xx_responses() {
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attempts_shared = attempts.clone();
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        let attempt = attempts_shared.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            common::error_response(
                503,
                "ServiceUnavailableError",
                &[("message", &json!("warming up"))],
            )
        } else {
            common::json_response(200, &json!({ "healthy": true }))
        }
    }))
    .await;
    let client = make_client(&server);

    let options = oc_client::RequestOptions {
        retry: Some(oc_client::RetryPolicy {
            max_attempts: 2,
            base_delay: std::time::Duration::from_millis(5),
        }),
        ..Default::default()
    };
    let health = client
        .health
        .get(Some(&options))
        .await
        .expect("health.get after retry");
    assert!(health.healthy);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(server.recorded().len(), 2);
}
