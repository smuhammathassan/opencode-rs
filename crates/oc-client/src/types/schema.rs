//! Primitive schema types.
//! From reference/packages/schema/src/schema.ts.

/// JSON values. Mirrors `JsonValue` in `reference/packages/client/src/generated/types.ts`.
pub type JsonValue = serde_json::Value;

/// An absolute filesystem path. Branded string in the reference (`AbsolutePath`).
pub type AbsolutePath = String;

/// A relative filesystem path. Branded string in the reference (`RelativePath`).
pub type RelativePath = String;

/// A monotonic identifier-like value. From reference/packages/schema/src/identifier.ts.
pub type ID = String;

/// Millisecond epoch timestamps. From `DateTimeUtcFromMillis` in
/// reference/packages/schema/src/schema.ts.
pub type DateTimeMillis = i64;

/// A request/response order direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    Asc,
    Desc,
}

/// A server-delivery mode for session inputs.
/// From reference/packages/schema/src/session-delivery.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    Steer,
    Queue,
}
