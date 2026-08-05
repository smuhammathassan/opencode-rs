//! EventV2 wire types: the durable event-sourcing contract used by the sync
//! system. Mirrors reference/packages/schema/src/event.ts (ID, Definition,
//! Payload, versioned types) and reference/packages/core/src/event.ts
//! (SerializedEvent).
//!
//! TODO(integration): promote to oc-schema/oc-core once those crates are
//! implemented; this is a private mirror of `@opencode-ai/schema/event`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::schema::Prefix;

/// Event ID, `evt_<26 chars>` from reference/packages/schema/src/event.ts:
/// `ID.create()` is `"evt_" + ascending()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventID(pub String);

impl EventID {
    pub fn create() -> Self {
        Self(
            super::schema::ascending(Prefix::Event, None).expect("event id generation cannot fail"),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EventID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for EventID {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for EventID {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// `versionedType` from reference/packages/schema/src/event.ts.
pub fn versioned_type(r#type: &str, version: i64) -> String {
    format!("{type}.{version}")
}

/// `durable` descriptor of an event definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Durable {
    pub version: i64,
    pub aggregate: String,
}

/// An event definition, mirroring `Event.Definition` in
/// reference/packages/schema/src/event.ts.
#[derive(Debug, Clone)]
pub struct Definition {
    pub r#type: &'static str,
    pub durable: Option<Durable>,
}

impl Definition {
    /// A non-durable event definition.
    pub fn new(r#type: &'static str) -> Self {
        Self {
            r#type,
            durable: None,
        }
    }

    /// A durable event definition with an aggregate field and version.
    pub fn durable(r#type: &'static str, aggregate: &'static str, version: i64) -> Self {
        Self {
            r#type,
            durable: Some(Durable {
                version,
                aggregate: aggregate.to_string(),
            }),
        }
    }

    /// The storage type: `versionedType(type, version)` for durable events,
    /// plain `type` otherwise.
    pub fn storage_type(&self) -> String {
        match &self.durable {
            Some(durable) => versioned_type(self.r#type, durable.version),
            None => self.r#type.to_string(),
        }
    }
}

/// The `durable` envelope attached to committed payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableEnvelope {
    pub aggregate_id: String,
    pub seq: i64,
    pub version: i64,
}

/// Location ref for a payload, mirroring `Location.Ref` in
/// reference/packages/schema/src/location.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationRef {
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

/// A published event payload, mirroring `Event.Payload` in
/// reference/packages/schema/src/event.ts. Field order matches the reference
/// wire shape: id, metadata, type, durable, location, data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub id: EventID,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable: Option<DurableEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRef>,
    pub data: Value,
}

impl Payload {
    pub fn aggregate_id(&self) -> Option<&str> {
        self.durable.as_ref().map(|d| d.aggregate_id.as_str())
    }
}

/// A serialized event for replay / sync transport. Mirrors
/// `EventV2.SerializedEvent` in reference/packages/core/src/event.ts:
/// `{ id, type, seq, aggregateID, data }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerializedEvent {
    pub id: EventID,
    pub r#type: String,
    pub seq: i64,
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub data: Value,
}

/// A sync event row as returned by the `/sync/history` endpoint. Mirrors
/// `HistoryEvent` in reference/packages/opencode/src/server/routes/instance/httpapi/handlers/sync.ts
/// (and `groups/sync.ts`): note the snake_case `aggregate_id` here vs the
/// camelCase `aggregateID` on `SerializedEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub id: EventID,
    pub aggregate_id: String,
    pub seq: i64,
    pub r#type: String,
    pub data: Value,
}

impl From<HistoryEvent> for SerializedEvent {
    fn from(event: HistoryEvent) -> Self {
        SerializedEvent {
            id: event.id,
            r#type: event.r#type,
            seq: event.seq,
            aggregate_id: event.aggregate_id,
            data: event.data,
        }
    }
}

/// The global event stream envelope (`GlobalEvent` schema) as it travels over
/// `/global/event`. Mirrors the `GlobalEventSchema` in
/// reference/packages/opencode/src/server/routes/instance/httpapi/groups/global.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalEvent {
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub payload: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_id_create_prefix() {
        let id = EventID::create();
        assert!(id.0.starts_with("evt_"));
    }

    #[test]
    fn versioned_type_format() {
        assert_eq!(
            versioned_type("session.next.moved", 1),
            "session.next.moved.1"
        );
    }

    #[test]
    fn storage_type() {
        let def = Definition::durable("session.next.moved", "sessionID", 1);
        assert_eq!(def.storage_type(), "session.next.moved.1");
        let plain = Definition::new("workspace.ready");
        assert_eq!(plain.storage_type(), "workspace.ready");
    }

    #[test]
    fn serialized_event_json_shape() {
        let event = SerializedEvent {
            id: EventID("evt_abc".into()),
            r#type: "session.next.moved.1".into(),
            seq: 3,
            aggregate_id: "ses_123".into(),
            data: serde_json::json!({ "sessionID": "ses_123" }),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"id":"evt_abc","type":"session.next.moved.1","seq":3,"aggregateID":"ses_123","data":{"sessionID":"ses_123"}}"#
        );
    }

    #[test]
    fn history_event_json_shape() {
        let event = HistoryEvent {
            id: EventID("evt_abc".into()),
            aggregate_id: "ses_123".into(),
            seq: 3,
            r#type: "session.next.moved.1".into(),
            data: serde_json::json!({ "sessionID": "ses_123" }),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"id":"evt_abc","aggregate_id":"ses_123","seq":3,"type":"session.next.moved.1","data":{"sessionID":"ses_123"}}"#
        );
    }

    #[test]
    fn payload_json_shape_omits_absent_fields() {
        let payload = Payload {
            id: EventID("evt_abc".into()),
            metadata: None,
            r#type: "workspace.ready".into(),
            durable: None,
            location: None,
            data: serde_json::json!({ "name": "foo" }),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            json,
            r#"{"id":"evt_abc","type":"workspace.ready","data":{"name":"foo"}}"#
        );
    }

    #[test]
    fn payload_json_shape_with_durable_and_location() {
        let payload = Payload {
            id: EventID("evt_abc".into()),
            metadata: None,
            r#type: "session.next.moved".into(),
            durable: Some(DurableEnvelope {
                aggregate_id: "ses_123".into(),
                seq: 0,
                version: 1,
            }),
            location: Some(LocationRef {
                directory: "/tmp/a".into(),
                workspace_id: None,
            }),
            data: serde_json::json!({}),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            json,
            r#"{"id":"evt_abc","type":"session.next.moved","durable":{"aggregate_id":"ses_123","seq":0,"version":1},"location":{"directory":"/tmp/a"},"data":{}}"#
        );
    }

    #[test]
    fn history_event_converts_to_serialized_event() {
        let history = HistoryEvent {
            id: EventID("evt_abc".into()),
            aggregate_id: "ses_123".into(),
            seq: 1,
            r#type: "session.next.moved.1".into(),
            data: serde_json::json!({}),
        };
        let serialized: SerializedEvent = history.into();
        assert_eq!(serialized.aggregate_id, "ses_123");
        assert_eq!(serialized.r#type, "session.next.moved.1");
    }
}
