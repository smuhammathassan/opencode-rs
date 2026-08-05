//! Golden tests: exact ID formats and event JSON serialization derived from
//! the reference (opencode v1.18.13).

use oc_core::event::{Definition, Durable, DurableInfo, Payload};
use oc_core::id;
use oc_core::ids::EventId;
use oc_core::location::LocationRef;
use oc_core::schema::AbsolutePath;
use serde_json::{Map, Value};

#[test]
fn id_format_is_prefix_underscore_26() {
    // Reference: `prefix + "_" + createIdentifier(direction, timestamp)`.
    let job = id::ascending("job", None).unwrap();
    let event = id::ascending("event", None).unwrap();
    let session = id::ascending("session", None).unwrap();
    let workspace = id::ascending("workspace", None).unwrap();

    for (value, prefix) in [
        (&job, "job"),
        (&event, "evt"),
        (&session, "ses"),
        (&workspace, "wrk"),
    ] {
        let rest = value.strip_prefix(&format!("{prefix}_")).expect("prefix");
        assert_eq!(rest.len(), 26, "{prefix}: {value}");
        let (time, random) = rest.split_at(12);
        assert!(
            time.bytes().all(|b| b.is_ascii_hexdigit()),
            "{prefix}: time {time}"
        );
        assert!(
            random.bytes().all(|b| b.is_ascii_alphanumeric()),
            "{prefix}: random"
        );
    }
}

#[test]
fn id_timestamp_matches_reference() {
    // create("evt", ascending, 100000, counter=1) -> time field = 100000 * 0x1000 + 1
    let id_value = format!(
        "evt_{}",
        oc_core::identifier::create_with_counter(false, 100000, 1)
    );
    let rest = &id_value["evt_".len()..];
    let time = &rest[..12];
    assert_eq!(time, format!("{:012x}", 100000u64 * 0x1000 + 1));
}

#[test]
fn descending_id_is_complemented() {
    let ascending = oc_core::identifier::create_with_counter(false, 100000, 1);
    let descending = oc_core::identifier::create_with_counter(true, 100000, 1);
    let asc = &ascending[..12];
    let desc = &descending[..12];
    assert_eq!(
        u64::from_str_radix(desc, 16).unwrap(),
        0xffff_ffff_ffff - u64::from_str_radix(asc, 16).unwrap()
    );
}

#[test]
fn event_payload_serializes_like_reference() {
    // `define({ type: "catalog.updated", schema: {} })` payload.
    let payload = Payload {
        id: EventId("evt_abc".to_string()),
        metadata: None,
        r#type: "catalog.updated".to_string(),
        durable: None,
        location: None,
        data: Map::new(),
    };
    assert_eq!(
        serde_json::to_value(&payload).unwrap(),
        Value::Object(Map::from_iter([
            ("id".to_string(), Value::from("evt_abc")),
            ("type".to_string(), Value::from("catalog.updated")),
            ("data".to_string(), Value::Object(Map::new())),
        ]))
    );
}

#[test]
fn durable_event_payload_field_order() {
    // Full payload: id, metadata, type, durable, location, data.
    let mut metadata = Map::new();
    metadata.insert("background".to_string(), Value::from(true));
    let mut data = Map::new();
    data.insert("sessionID".to_string(), Value::from("ses_1"));
    let payload = Payload {
        id: EventId("evt_1".to_string()),
        metadata: Some(metadata),
        r#type: "session.created".to_string(),
        durable: Some(DurableInfo {
            aggregateID: "ses_1".to_string(),
            seq: 0,
            version: 1,
        }),
        location: Some(LocationRef {
            directory: AbsolutePath("/repo".to_string()),
            workspace_id: None,
        }),
        data,
    };
    let json = serde_json::to_string(&payload).unwrap();
    assert_eq!(
        json,
        r#"{"id":"evt_1","metadata":{"background":true},"type":"session.created","durable":{"aggregateID":"ses_1","seq":0,"version":1},"location":{"directory":"/repo"},"data":{"sessionID":"ses_1"}}"#
    );
}

#[test]
fn versioned_type_matches_reference() {
    assert_eq!(
        oc_core::event::versioned_type("session.created", 1),
        "session.created.1"
    );
}

#[test]
fn definition_registry_resolves_durable() {
    let registry = oc_core::event::DurableRegistry::default();
    registry.register(&[Definition::define(
        "session.created",
        Some(Durable {
            version: 1,
            aggregate: "sessionID".to_string(),
        }),
        vec!["sessionID".to_string()],
    )]);
    let resolved = registry.get_durable("session.created.1").expect("durable");
    assert_eq!(resolved.r#type, "session.created");
    assert_eq!(resolved.durable.as_ref().unwrap().version, 1);
    assert_eq!(resolved.durable.as_ref().unwrap().aggregate, "sessionID");
}
