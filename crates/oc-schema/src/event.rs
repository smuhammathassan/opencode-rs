//! From reference/packages/schema/src/event.ts

use crate::identifier::ascending;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Event.ID` — starts with `evt_`.
pub type EventID = String;

/// `Event.ID.create()`.
pub fn create_id() -> EventID {
    format!("evt_{}", ascending())
}

/// Arbitrary per-event metadata; `optional(Schema.Record(Schema.String, Schema.Unknown))`.
pub type Metadata = IndexMap<String, Value>;

/// The optional `durable` property of an encoded event instance.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DurableRef {
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub seq: i64,
    pub version: i64,
}

/// A definition registered for an event type: its wire type plus durable metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub r#type: &'static str,
    pub durable: Option<DurableVersion>,
}

/// Durable metadata attached to an event definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DurableVersion {
    pub version: i32,
    pub aggregate: &'static str,
}

/// `versionedType(type, version)` — `"${type}.${version}"`.
pub fn versioned_type(r#type: &str, version: i32) -> String {
    format!("{}.{}", r#type, version)
}

/// `inventory(...)` — a frozen ordered list of definitions.
pub fn inventory<const N: usize>(definitions: [Definition; N]) -> Vec<Definition> {
    definitions.to_vec()
}

/// `latest(definitions)` — keeps the highest durable version per type and rejects
/// duplicate non-durable definitions.
pub fn latest(definitions: &[Definition]) -> Vec<Definition> {
    let mut result: Vec<Definition> = Vec::new();
    for definition in definitions {
        if let Some(existing) = result.iter_mut().find(|d| d.r#type == definition.r#type) {
            match (&existing.durable, &definition.durable) {
                (Some(a), Some(b)) => {
                    if b.version > a.version {
                        *existing = definition.clone();
                    }
                }
                (Some(_), None) | (None, Some(_)) | (None, None) => {
                    if existing != definition {
                        panic!("Duplicate latest event definition for {}", definition.r#type);
                    }
                }
            }
        } else {
            result.push(definition.clone());
        }
    }
    result
}

/// `durable(definitions)` — builds a map keyed by `"type.version"` for durable
/// definitions, panicking on duplicates.
pub fn durable(definitions: &[Definition]) -> IndexMap<String, Definition> {
    let mut result = IndexMap::new();
    for definition in definitions {
        let Some(version) = &definition.durable else {
            continue;
        };
        let key = versioned_type(definition.r#type, version.version);
        if result.contains_key(&key) {
            panic!("Duplicate durable event definition for {key}");
        }
        result.insert(key, definition.clone());
    }
    result
}

/// Generates an event struct mirroring `Event.define(...)`:
/// `{ id, metadata?, type, durable?, location, data }`. The `tag` name must be a
/// new identifier for the generated type-literal enum.
#[macro_export]
macro_rules! define_event {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            tag: $tag:ident,
            r#type: $type:literal,
            durable: $aggregate:literal, $version:literal,
            data: $data:ty,
        }
    ) => {
        $crate::__define_event! {
            $(#[$meta])*
            pub struct $name {
                tag: $tag,
                r#type: $type,
                durable_expr: Some($crate::event::DurableVersion { version: $version, aggregate: $aggregate }),
                data: $data,
            }
        }
    };
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            tag: $tag:ident,
            r#type: $type:literal,
            data: $data:ty,
        }
    ) => {
        $crate::__define_event! {
            $(#[$meta])*
            pub struct $name {
                tag: $tag,
                r#type: $type,
                durable_expr: None,
                data: $data,
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __define_event {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            tag: $tag:ident,
            r#type: $type:literal,
            durable_expr: $durable_expr:expr,
            data: $data:ty,
        }
    ) => {
        #[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
        pub enum $tag {
            #[serde(rename = $type)]
            Value,
        }

        #[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
        $(#[$meta])*
        pub struct $name {
            pub id: $crate::event::EventID,
            #[serde(skip_serializing_if = "Option::is_none", default)]
            pub metadata: Option<$crate::event::Metadata>,
            #[serde(rename = "type")]
            pub r#type: $tag,
            #[serde(skip_serializing_if = "Option::is_none", default)]
            pub durable: Option<$crate::event::DurableRef>,
            #[serde(skip_serializing_if = "Option::is_none", default)]
            pub location: Option<$crate::location::Ref>,
            pub data: $data,
        }

        impl $name {
            /// The definition registered for this event (mirrors the `statics` on `define`).
            pub const fn definition() -> $crate::event::Definition {
                $crate::event::Definition {
                    r#type: $type,
                    durable: $durable_expr,
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_keeps_newer_durable_version() {
        let defs = vec![
            Definition {
                r#type: "session.next.step.ended",
                durable: Some(DurableVersion { version: 1, aggregate: "sessionID" }),
            },
            Definition {
                r#type: "session.next.step.ended",
                durable: Some(DurableVersion { version: 2, aggregate: "sessionID" }),
            },
        ];
        let out = latest(&defs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].durable.as_ref().unwrap().version, 2);
    }

    #[test]
    fn durable_map_keys() {
        let defs = vec![Definition {
            r#type: "session.created",
            durable: Some(DurableVersion { version: 1, aggregate: "sessionID" }),
        }];
        let map = durable(&defs);
        assert!(map.contains_key("session.created.1"));
    }
}
