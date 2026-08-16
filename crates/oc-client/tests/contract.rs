//! Contract parity tests.
//! Mirrors `reference/packages/client/test/contract-identity.test.ts` and the
//! group-exposure assertions in `reference/packages/client/test/promise.test.ts`.

mod common;

use oc_client::contract::{ENDPOINT_NAMES, GROUP_NAMES, OMIT_ENDPOINTS};
use oc_client::types::*;

#[test]
fn group_names_match_the_reference_contract() {
    let names: Vec<&str> = GROUP_NAMES.iter().map(|(_, name)| *name).collect();
    assert_eq!(
        names,
        vec![
            "health",
            "location",
            "agents",
            "sessions",
            "messages",
            "models",
            "providers",
            "integrations",
            "credentials",
            "permissions",
            "files",
            "commands",
            "skills",
            "events",
            "ptys",
            "questions",
            "references",
            "projectCopies",
        ]
    );
}

#[test]
fn endpoint_names_match_the_reference_contract() {
    let mut names: Vec<&str> = ENDPOINT_NAMES
        .iter()
        .map(|(endpoint, _)| *endpoint)
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "integration.attempt.cancel",
            "integration.attempt.complete",
            "integration.attempt.status",
            "integration.connect.key",
            "integration.connect.oauth",
            "permission.request.list",
            "permission.saved.list",
            "permission.saved.remove",
            "question.request.list",
            "session.messages",
        ]
    );
}

#[test]
fn omit_endpoints_match_the_reference_contract() {
    assert_eq!(
        OMIT_ENDPOINTS,
        &["fs.read", "pty.connect", "pty.connectToken"]
    );
}

#[test]
fn shared_dtos_decode_plain_objects() {
    let prompt: PromptInput =
        serde_json::from_value(serde_json::json!({ "text": "hello" })).unwrap();
    assert_eq!(prompt.text, "hello");

    let text: AssistantContent =
        serde_json::from_value(serde_json::json!({ "type": "text", "id": "part_1", "text": "hi" }))
            .unwrap();
    match text {
        AssistantContent::Text { text, .. } => assert_eq!(text, "hi"),
        _ => panic!("expected text content"),
    }
}

#[test]
fn location_deep_object_round_trip() {
    let location = LocationQueryRef {
        directory: Some("/tmp/project".into()),
        workspace: None,
    };
    let value = serde_json::to_value(&location).unwrap();
    assert_eq!(value, serde_json::json!({ "directory": "/tmp/project" }));
}

#[test]
fn session_message_tagged_union_decodes() {
    let message: SessionMessage = serde_json::from_value(serde_json::json!({
        "id": "msg_model",
        "type": "model-switched",
        "time": { "created": 1717171717000i64 },
        "model": { "id": "claude", "providerID": "anthropic" },
    }))
    .unwrap();
    match message {
        SessionMessage::ModelSwitched { model, .. } => {
            assert_eq!(model.id, "claude");
            assert_eq!(model.provider_id, "anthropic");
        }
        _ => panic!("expected model-switched message"),
    }
}

#[test]
fn session_durable_event_decodes_from_fixture() {
    let event: SessionDurableEvent = serde_json::from_value(serde_json::json!({
        "id": "evt_model",
        "type": "session.next.model.switched",
        "durable": { "aggregateID": "ses_test", "seq": 1, "version": 1 },
        "data": {
            "timestamp": 1717171717000i64,
            "sessionID": "ses_test",
            "messageID": "msg_model",
            "model": { "id": "claude", "providerID": "anthropic" },
        },
    }))
    .unwrap();
    assert_eq!(event.event_type(), "session.next.model.switched");
    match event {
        SessionDurableEvent::ModelSwitched { durable, data, .. } => {
            let durable = durable.expect("durable");
            assert_eq!(durable.aggregate_id, "ses_test");
            assert_eq!(durable.seq, 1);
            assert_eq!(data.session_id, "ses_test");
            assert_eq!(data.model.id, "claude");
        }
        _ => panic!("expected model-switched event"),
    }
}

#[test]
fn unknown_event_type_falls_back_to_raw() {
    let value = serde_json::json!({
        "id": "evt_unknown",
        "type": "future.event.type",
        "data": { "anything": true },
    });
    let event: OpenCodeEvent = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(event.event_type(), "future.event.type");
    match event {
        OpenCodeEvent::Raw { value, .. } => assert_eq!(
            value,
            serde_json::json!({ "id": "evt_unknown", "type": "future.event.type", "data": { "anything": true } })
        ),
        _ => panic!("expected raw event"),
    }
}

#[test]
fn session_compacted_event_is_typed() {
    let event: OpenCodeEvent = serde_json::from_value(serde_json::json!({
        "id": "evt_compacted",
        "type": "session.compacted",
        "data": { "sessionID": "ses_compacted" },
    }))
    .unwrap();
    assert_eq!(event.event_type(), "session.compacted");
    match event {
        OpenCodeEvent::SessionCompacted { data, .. } => {
            assert_eq!(data.session_id, "ses_compacted");
        }
        _ => panic!("expected typed compacted event"),
    }
}
