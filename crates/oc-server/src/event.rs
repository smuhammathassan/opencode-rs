//! Event bus and event payloads.
//!
//! Mirrors `EventV2` from reference/packages/core/src/event and the `server.connected`
//! handshake from reference/packages/server/src/handlers/event.ts.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Event payload. From reference/packages/schema/src/event.ts (`Event.Payload`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable: Option<Durable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<crate::schema::LocationRef>,
    pub data: serde_json::Value,
}

/// Durable replay metadata. From reference/packages/schema/src/event.ts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Durable {
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub seq: i64,
    pub version: i64,
}

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn ascending_id(prefix: &str) -> String {
    // Mirrors `ascending()` in reference/packages/schema/src/identifier.ts: base36
    // timestamp + per-process counter for uniqueness.
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let counter = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let encoded = format!("{millis:x}{counter:04x}");
    format!("{prefix}_{encoded}")
}

pub fn event_id() -> String {
    ascending_id("evt")
}

pub fn session_message_id() -> String {
    ascending_id("msg")
}

pub fn session_id() -> String {
    ascending_id("ses")
}

pub fn pty_id() -> String {
    ascending_id("pty")
}

pub fn permission_id() -> String {
    ascending_id("perm")
}

pub fn question_id() -> String {
    ascending_id("qst")
}

/// Broadcast event bus. The reference registers one listener per SSE subscriber
/// with a bounded buffer (`EventV2.allBounded`); tokio's broadcast channel gives us
/// the same fan-out semantics.
#[derive(Debug, Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

#[derive(Debug)]
struct EventBusInner {
    tx: broadcast::Sender<Event>,
    capacity: usize,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        EventBus {
            inner: Arc::new(EventBusInner { tx, capacity }),
        }
    }

    /// Publish one event to all live subscribers.
    pub fn emit(&self, event: Event) {
        let _ = self.inner.tx.send(event);
    }

    /// Subscribe, replaying nothing. Mirrors `EventV2.allBounded`.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.tx.subscribe()
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

/// Build a `server.connected` event. From reference/packages/server/src/handlers/event.ts.
pub fn server_connected() -> Event {
    Event {
        id: event_id(),
        metadata: None,
        r#type: "server.connected".into(),
        durable: None,
        location: None,
        data: serde_json::json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_are_unique_and_prefixed() {
        assert!(event_id().starts_with("evt_"));
        assert!(session_message_id().starts_with("msg_"));
        assert_ne!(event_id(), event_id());
    }

    #[tokio::test]
    async fn bus_fans_out_to_subscribers() {
        let bus = EventBus::new(16);
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();
        bus.emit(server_connected());
        let a = first.recv().await.unwrap();
        let b = second.recv().await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a.r#type, "server.connected");
    }
}
