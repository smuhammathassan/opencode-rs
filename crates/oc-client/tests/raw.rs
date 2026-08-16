//! Generic plugin/forward-compatible HTTP and SSE client tests.

mod common;

use common::MockServer;
use futures::StreamExt;
use oc_client::{ClientOptions, OpenCode, RawRequest};
use reqwest::Method;
use serde_json::json;
use std::sync::Arc;

fn make_client(server: &MockServer) -> OpenCode {
    OpenCode::make(ClientOptions {
        base_url: server.base_url.parse().expect("base url"),
        ..ClientOptions::default()
    })
    .expect("client")
}

#[tokio::test]
async fn generic_request_supports_plugin_routes_and_json_bodies() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(200, &json!({ "ok": true, "version": 2 }))
    }))
    .await;
    let client = make_client(&server);

    let value: serde_json::Value = client
        .request(
            &RawRequest::new(Method::POST, "/experimental/plugin/inspect")
                .with_query("plugin", json!("demo"))
                .with_body(json!({ "includeHooks": true })),
            None,
        )
        .await
        .expect("generic request");

    assert_eq!(value, json!({ "ok": true, "version": 2 }));
    let requests = server.recorded();
    let request = common::assert_request(
        &requests,
        0,
        "POST",
        "/experimental/plugin/inspect?plugin=demo",
    );
    common::assert_body(request, &json!({ "includeHooks": true }));
}

#[tokio::test]
async fn generic_sse_is_lazy_and_decodes_plugin_events() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::sse_response(
            200,
            "event: plugin\ndata: {\"type\":\"plugin.updated\",\"id\":7}\n\n",
        )
    }))
    .await;
    let client = make_client(&server);

    let mut stream = client.sse::<serde_json::Value>(
        RawRequest::new(Method::GET, "/experimental/plugin/events"),
        None,
    );
    assert!(
        server.recorded().is_empty(),
        "SSE should start on first poll"
    );

    let event = stream
        .next()
        .await
        .expect("first event")
        .expect("event should decode");
    assert_eq!(event, json!({ "type": "plugin.updated", "id": 7 }));
    assert_eq!(server.recorded()[0].path, "/experimental/plugin/events");
}

#[tokio::test]
async fn generic_request_maps_json_rpc_errors() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::error_response(409, "ConflictError", &[("message", &json!("busy"))])
    }))
    .await;
    let client = make_client(&server);

    let err = client
        .request::<serde_json::Value>(
            &RawRequest::new(Method::POST, "/experimental/plugin/reload"),
            None,
        )
        .await
        .expect_err("request should fail");
    assert!(matches!(
        err,
        oc_client::Error::Api(oc_client::ApiError::Protocol(
            oc_client::ProtocolError::ConflictError { .. }
        ))
    ));
}
