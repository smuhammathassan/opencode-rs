use std::time::Duration;

use oc_project::lsp::{LspAdapter, LspEvent, LspServerConfig};
use serde_json::json;

#[tokio::test]
async fn process_client_initializes_correlates_requests_and_shuts_down() {
    let adapter = LspAdapter::start(
        LspServerConfig::new(env!("CARGO_BIN_EXE_fake_lsp_server"))
            .with_timeout(Duration::from_secs(2)),
        std::env::current_dir().unwrap(),
    )
    .await
    .expect("fake language server should start");

    let client = adapter.client().clone();
    let (slow, fast) = tokio::join!(
        client.request("test/slow", json!({})),
        client.request("test/fast", json!({})),
    );
    assert_eq!(slow.unwrap()["value"], "slow");
    assert_eq!(fast.unwrap()["value"], "fast");

    adapter.shutdown().await.expect("language server shutdown");
}

#[tokio::test]
async fn server_events_are_observable_without_blocking_request_correlation() {
    let adapter = LspAdapter::start(
        LspServerConfig::new(env!("CARGO_BIN_EXE_fake_lsp_server"))
            .with_timeout(Duration::from_secs(2)),
        std::env::current_dir().unwrap(),
    )
    .await
    .expect("fake language server should start");

    let mut events = adapter.client().subscribe();
    let notification_response = adapter
        .client()
        .request("test/server-notification", json!({}))
        .await
        .expect("notification-triggering request should complete");
    assert_eq!(notification_response["value"], "ack");
    assert_eq!(
        events.recv().await.expect("server notification event"),
        LspEvent::Notification {
            method: "window/logMessage".to_string(),
            params: json!({ "type": 3, "message": "server notification" }),
        }
    );

    let request_response = adapter
        .client()
        .request("test/server-request", json!({}))
        .await
        .expect("server-request-triggering request should complete");
    assert_eq!(request_response["value"], "ack");
    assert_eq!(
        events.recv().await.expect("server request event"),
        LspEvent::Request {
            id: json!(9001),
            method: "workspace/configuration".to_string(),
            params: json!({ "items": [] }),
        }
    );

    adapter.shutdown().await.expect("language server shutdown");
}

#[tokio::test]
async fn call_hierarchy_operations_prepare_then_request_calls() {
    let adapter = LspAdapter::start(
        LspServerConfig::new(env!("CARGO_BIN_EXE_fake_lsp_server"))
            .with_timeout(Duration::from_secs(2)),
        std::env::current_dir().unwrap(),
    )
    .await
    .expect("fake language server should start");

    let incoming = adapter
        .request_operation(
            oc_project::lsp::LspOperation::IncomingCalls,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("incoming call hierarchy should be supported");
    assert_eq!(incoming[0]["from"]["name"], "caller");

    let outgoing = adapter
        .request_operation(
            oc_project::lsp::LspOperation::OutgoingCalls,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("outgoing call hierarchy should be supported");
    assert_eq!(outgoing[0]["from"]["name"], "caller");

    adapter.shutdown().await.expect("language server shutdown");
}

#[tokio::test]
async fn position_operation_synchronizes_document_lifecycle() {
    let adapter = LspAdapter::start(
        LspServerConfig::new(env!("CARGO_BIN_EXE_fake_lsp_server"))
            .with_timeout(Duration::from_secs(2)),
        std::env::current_dir().unwrap(),
    )
    .await
    .expect("fake language server should start");

    let result = adapter
        .request_operation(
            oc_project::lsp::LspOperation::Hover,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("hover should observe an opened document");
    assert_eq!(result["opened"], true);
    assert!(result["params"]["textDocument"]["uri"]
        .as_str()
        .unwrap()
        .ends_with("/src/lib.rs"));

    adapter.shutdown().await.expect("language server shutdown");
}

/// Every operation the reference `lsp.ts` declares is routed by the adapter to
/// the JSON-RPC process adapter; the fake server echoes enough for each so the
/// declared-vs-implemented surface is exercised end to end.
#[tokio::test]
async fn all_declared_operations_are_routed_to_the_process_adapter() {
    let adapter = LspAdapter::start(
        LspServerConfig::new(env!("CARGO_BIN_EXE_fake_lsp_server"))
            .with_timeout(Duration::from_secs(2)),
        std::env::current_dir().unwrap(),
    )
    .await
    .expect("fake language server should start");

    let definition = adapter
        .request_operation(
            oc_project::lsp::LspOperation::GoToDefinition,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("goToDefinition");
    assert!(definition[0]["opened"] == true);

    let references = adapter
        .request_operation(
            oc_project::lsp::LspOperation::FindReferences,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("findReferences");
    assert!(references[0]["uri"].as_str().unwrap().ends_with("/lib.rs"));

    let implementation = adapter
        .request_operation(
            oc_project::lsp::LspOperation::GoToImplementation,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("goToImplementation");
    assert!(implementation[0]["uri"]
        .as_str()
        .unwrap()
        .ends_with("/impl.rs"));

    let document_symbol = adapter
        .request_operation(
            oc_project::lsp::LspOperation::DocumentSymbol,
            "src/lib.rs",
            1,
            1,
            None,
        )
        .await
        .expect("documentSymbol");
    assert_eq!(document_symbol[0]["name"], "main");

    let workspace_symbol = adapter
        .request_operation(
            oc_project::lsp::LspOperation::WorkspaceSymbol,
            "src/lib.rs",
            1,
            1,
            Some("main"),
        )
        .await
        .expect("workspaceSymbol");
    assert_eq!(workspace_symbol[0]["name"], "main");

    adapter.shutdown().await.expect("language server shutdown");
}
