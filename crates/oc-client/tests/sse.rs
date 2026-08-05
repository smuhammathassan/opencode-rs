//! SSE streaming tests.
//! Mirrors `reference/packages/client/test/promise.test.ts` (events.subscribe
//! tests) and the sse helper in `reference/packages/client/src/generated/client.ts`.

mod common;

use common::MockServer;
use futures::StreamExt;
use oc_client::types::OpenCodeEvent;
use oc_client::{ClientOptions, OpenCode};
use serde_json::json;
use std::sync::Arc;

fn make_client(server: &MockServer) -> OpenCode {
    OpenCode::make(ClientOptions {
        base_url: server.base_url.parse().expect("base url"),
        ..ClientOptions::default()
    })
    .expect("client")
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
async fn events_subscribe_streams_and_decodes_server_events() {
    let model_switched = model_switched_event();
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        let body = format!(
            ": heartbeat\n\ndata: {}\n\ndata: {}\n\n",
            json!({ "id": "evt_connected", "type": "server.connected", "data": {} }),
            model_switched
        );
        common::sse_response(200, &body)
    }))
    .await;
    let client = make_client(&server);

    let events: Vec<OpenCodeEvent> = client
        .events
        .subscribe(None)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|item| item.expect("sse item"))
        .collect();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type(), "server.connected");
    assert_eq!(events[1].event_type(), "session.next.model.switched");

    let requests = server.recorded();
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/event");
}

#[tokio::test]
async fn events_subscribe_splits_events_across_chunks() {
    let model_switched = model_switched_event();
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        // The two events share a single chunk but are delimited by \n\n.
        let body = format!(
            "data: {}\n\ndata: {}\n\n",
            json!({ "id": "evt_connected", "type": "server.connected", "data": {} }),
            model_switched
        );
        common::sse_response(200, &body)
    }))
    .await;
    let client = make_client(&server);

    let mut stream = client.events.subscribe(None);
    let first = stream.next().await.expect("first event").expect("ok");
    let second = stream.next().await.expect("second event").expect("ok");
    assert_eq!(first.event_type(), "server.connected");
    assert_eq!(second.event_type(), "session.next.model.switched");
}

#[tokio::test]
async fn events_subscribe_terminates_on_malformed_sse_data() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::sse_response(200, "data: {not-json}\n\n")
    }))
    .await;
    let client = make_client(&server);

    let err = client
        .events
        .subscribe(None)
        .next()
        .await
        .expect("first item")
        .expect_err("should fail");
    match err {
        oc_client::Error::Client(oc_client::ClientError::MalformedResponse(_)) => {}
        other => panic!("expected MalformedResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn sse_rejects_non_event_stream_content_type() {
    let server = MockServer::spawn(Arc::new(|_: &common::RecordedRequest| {
        common::json_response(200, &json!({ "healthy": true }))
    }))
    .await;
    let client = make_client(&server);

    let err = client
        .events
        .subscribe(None)
        .next()
        .await
        .expect("first item")
        .expect_err("should fail");
    match err {
        oc_client::Error::Client(oc_client::ClientError::UnsupportedContentType) => {}
        other => panic!("expected UnsupportedContentType, got {other:?}"),
    }
}

#[tokio::test]
async fn session_events_passes_after_query() {
    let model_switched = model_switched_event();
    let server = MockServer::spawn(Arc::new(move |_: &common::RecordedRequest| {
        common::sse_response(200, &format!("data: {model_switched}\n\n"))
    }))
    .await;
    let client = make_client(&server);

    let events: Vec<oc_client::types::SessionDurableEvent> = client
        .sessions
        .events(
            &oc_client::types::SessionsEventsInput {
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

    let requests = server.recorded();
    assert_eq!(requests[0].path, "/api/session/ses_test/event?after=0");
}
