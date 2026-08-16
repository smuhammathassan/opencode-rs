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
    assert_eq!(
        result["params"]["textDocument"]["uri"]
            .as_str()
            .unwrap()
            .ends_with("/src/lib.rs"),
        true
    );

    adapter.shutdown().await.expect("language server shutdown");
}
