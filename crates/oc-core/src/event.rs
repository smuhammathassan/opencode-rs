//! Event definitions, payloads, and the durable event registry.
//!
//! From reference/packages/schema/src/event.ts and
//! reference/packages/core/src/event.ts.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ids::EventId;
use crate::location::LocationRef;

/// Durable event metadata (`definition.durable`).
/// From reference/packages/schema/src/event.ts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Durable {
    pub version: u32,
    pub aggregate: String,
}

/// A typed event definition, mirroring `Event.define(...)`.
/// From reference/packages/schema/src/event.ts
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub r#type: String,
    pub durable: Option<Durable>,
    /// Ordered field names of the `data` struct schema. Used to validate and
    /// normalize encoded data.
    pub data_fields: Vec<String>,
}

impl Definition {
    /// Mirrors `Event.define(...)`, yielding a definition with a plain object
    /// `data` schema.
    pub fn define(r#type: &str, durable: Option<Durable>, data_fields: Vec<String>) -> Self {
        Definition {
            r#type: r#type.to_string(),
            durable,
            data_fields,
        }
    }
}

/// `Event.versionedType(type, version)` — the stored type for a durable event.
/// From reference/packages/schema/src/event.ts
pub fn versioned_type(r#type: &str, version: u32) -> String {
    format!("{}.{}", r#type, version)
}

/// Validate and normalize `data` against a definition's data schema.
///
/// Mirrors `Schema.encodeUnknownSync(definition.data)`: required fields must be
/// present, excess fields are dropped, and the result is keyed in schema field
/// order.
pub fn encode_data(
    definition: &Definition,
    data: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for field in &definition.data_fields {
        match data.get(field) {
            Some(value) => {
                out.insert(field.clone(), value.clone());
            }
            None => {
                return Err(format!(
                    "data is missing required field `{field}` for event `{}`",
                    definition.r#type
                ))
            }
        }
    }
    Ok(out)
}

/// The `durable` field of a payload (`{ aggregateID, seq, version }`).
/// From reference/packages/schema/src/event.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableInfo {
    pub aggregateID: String,
    pub seq: i64,
    pub version: u32,
}

/// A serialized event payload. Field order matches the zod `define` struct:
/// `id, metadata, type, durable, location, data`.
/// From reference/packages/schema/src/event.ts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub id: EventId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable: Option<DurableInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationRef>,
    pub data: Map<String, Value>,
}

/// A stored event row used for replay. JSON names: `id, type, seq,
/// aggregateID, data`.
/// From reference/packages/core/src/event.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedEvent {
    pub id: EventId,
    pub r#type: String,
    pub seq: i64,
    pub aggregateID: String,
    pub data: Map<String, Value>,
}

/// Registry of durable event definitions keyed by stored (versioned) type.
///
/// Mirrors `Durable` in `reference/packages/schema/src/durable-event-manifest.ts`.
/// The durable session definitions are owned by oc-session; register them
/// during integration.
/// TODO(integration): seed with session durable definitions.
#[derive(Debug, Default)]
pub struct DurableRegistry {
    // unversioned type -> definition
    by_type: std::sync::RwLock<std::collections::HashMap<String, Definition>>,
    // versioned type -> definition
    by_versioned: std::sync::RwLock<std::collections::HashMap<String, Definition>>,
}

impl DurableRegistry {
    /// Mirrors `Event.latest(definitions)` + `Event.durable(definitions)`.
    /// Registers non-durable definitions by type and durable definitions by
    /// both type and `versionedType`. For durable definitions with different
    /// versions of the same type, the highest version wins.
    pub fn register(&self, definitions: &[Definition]) {
        let mut by_type = self.by_type.write().unwrap();
        let mut by_versioned = self.by_versioned.write().unwrap();
        for definition in definitions {
            match by_type.get(&definition.r#type) {
                None => {
                    by_type.insert(definition.r#type.clone(), definition.clone());
                }
                Some(existing) => match (&existing.durable, &definition.durable) {
                    (Some(a), Some(b)) if b.version > a.version => {
                        by_type.insert(definition.r#type.clone(), definition.clone());
                    }
                    (Some(_), Some(_)) | (None, None) if existing == definition => {}
                    _ => {
                        tracing::warn!(
                            "duplicate event definition for {} (reference errors here)",
                            definition.r#type
                        );
                    }
                },
            }
            if let Some(durable) = &definition.durable {
                by_versioned.insert(
                    versioned_type(&definition.r#type, durable.version),
                    definition.clone(),
                );
            }
        }
    }

    /// Look up a durable definition by its stored (versioned) type.
    pub fn get_durable(&self, versioned: &str) -> Option<Definition> {
        self.by_versioned.read().unwrap().get(versioned).cloned()
    }

    /// Look up a definition by its unversioned type.
    pub fn get(&self, r#type: &str) -> Option<Definition> {
        self.by_type.read().unwrap().get(r#type).cloned()
    }
}

/// Mirrors `InvalidDurableEventError`.
/// From reference/packages/core/src/event.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidDurableEventError {
    pub _tag: String,
    pub r#type: String,
    pub message: String,
}

impl InvalidDurableEventError {
    pub fn new(r#type: impl Into<String>, message: impl Into<String>) -> Self {
        InvalidDurableEventError {
            _tag: "EventV2.InvalidDurableEvent".to_string(),
            r#type: r#type.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for InvalidDurableEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.r#type, self.message)
    }
}

impl std::error::Error for InvalidDurableEventError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_data_orders_fields_and_drops_excess() {
        let definition =
            Definition::define("test.event", None, vec!["b".to_string(), "a".to_string()]);
        let mut data = Map::new();
        data.insert("a".to_string(), Value::from(1));
        data.insert("x".to_string(), Value::from(9));
        data.insert("b".to_string(), Value::from(2));
        let encoded = encode_data(&definition, &data).unwrap();
        let keys: Vec<&String> = encoded.keys().collect();
        assert_eq!(keys, vec!["b", "a"]);
        assert!(!encoded.contains_key("x"));
    }

    #[test]
    fn encode_data_missing_field_errors() {
        let definition = Definition::define("test.event", None, vec!["a".to_string()]);
        let err = encode_data(&definition, &Map::new()).unwrap_err();
        assert!(err.contains("missing required field `a`"));
    }

    #[test]
    fn payload_json_shape() {
        let payload = Payload {
            id: EventId("evt_abc".to_string()),
            metadata: None,
            r#type: "catalog.updated".to_string(),
            durable: None,
            location: None,
            data: Map::new(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            json,
            r#"{"id":"evt_abc","type":"catalog.updated","data":{}}"#
        );
    }

    #[test]
    fn payload_json_with_durable_and_location() {
        let payload = Payload {
            id: EventId("evt_abc".to_string()),
            metadata: Some(Map::from_iter([(
                "background".to_string(),
                Value::from(true),
            )])),
            r#type: "session.created".to_string(),
            durable: Some(DurableInfo {
                aggregateID: "ses_1".to_string(),
                seq: 3,
                version: 1,
            }),
            location: Some(LocationRef {
                directory: crate::schema::AbsolutePath("/repo".to_string()),
                workspace_id: None,
            }),
            data: Map::from_iter([("sessionID".to_string(), Value::from("ses_1"))]),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            json,
            r#"{"id":"evt_abc","metadata":{"background":true},"type":"session.created","durable":{"aggregateID":"ses_1","seq":3,"version":1},"location":{"directory":"/repo"},"data":{"sessionID":"ses_1"}}"#
        );
    }
}
