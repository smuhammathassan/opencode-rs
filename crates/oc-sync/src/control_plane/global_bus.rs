//! Global event bus.
//!
//! Mirrors reference/packages/opencode/src/bus/global.ts. Unlike the reference's
//! process-global singleton `EventEmitter`, this is a cloneable handle so tests
//! can use isolated buses.
//!
//! TODO(integration): replace with the shared bus once oc-core implements it.

use serde_json::Value;
use tokio::sync::broadcast;

/// A global event as carried on the bus. Mirrors the `GlobalEvent` type in
/// reference/packages/opencode/src/bus/global.ts (all fields optional except
/// `payload`).
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalEvent {
    pub directory: Option<String>,
    pub project: Option<String>,
    pub workspace: Option<String>,
    pub payload: Value,
}

#[derive(Clone)]
pub struct GlobalBus {
    tx: broadcast::Sender<GlobalEvent>,
}

impl Default for GlobalBus {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    /// The process-global bus, mirroring the `GlobalBus` singleton in the reference.
    pub fn global() -> Self {
        use std::sync::OnceLock;
        static GLOBAL: OnceLock<GlobalBus> = OnceLock::new();
        GLOBAL.get_or_init(GlobalBus::new).clone()
    }

    /// `emit`. Mirrors the `GlobalBusEmitter.emit` override: payloads that are
    /// objects without an `id` get one attached (the sync event's id, or a fresh
    /// ascending event id).
    pub fn emit(&self, mut event: GlobalEvent) {
        if event.payload.is_object() && event.payload.get("id").is_none() {
            let id = event
                .payload
                .get("syncEvent")
                .and_then(|sync| sync.get("id"))
                .cloned()
                .unwrap_or_else(|| {
                    Value::String(crate::sync::schema::create(
                        "evt",
                        false,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .expect("clock")
                            .as_millis() as u64,
                    ))
                });
            event
                .payload
                .as_object_mut()
                .expect("payload is object")
                .insert("id".into(), id);
        }
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GlobalEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::event::EventID;

    #[test]
    fn emit_attaches_id_to_sync_payload() {
        let bus = GlobalBus::new();
        let mut rx = bus.subscribe();
        let sync_id = EventID::create();
        bus.emit(GlobalEvent {
            directory: None,
            project: None,
            workspace: Some("wrk_1".into()),
            payload: serde_json::json!({
                "type": "sync",
                "syncEvent": { "id": sync_id, "type": "session.next.moved.1", "seq": 0, "aggregateID": "ses_1", "data": {} }
            }),
        });
        let event = rx.try_recv().unwrap();
        assert_eq!(event.payload.get("id"), Some(&serde_json::json!(sync_id)));
    }

    #[test]
    fn emit_attaches_fresh_id_to_plain_payload() {
        let bus = GlobalBus::new();
        let mut rx = bus.subscribe();
        bus.emit(GlobalEvent {
            directory: Some("/tmp".into()),
            project: None,
            workspace: None,
            payload: serde_json::json!({ "type": "workspace.status", "properties": { "workspaceID": "wrk_1", "status": "connecting" } }),
        });
        let event = rx.try_recv().unwrap();
        let id = event.payload.get("id").unwrap().as_str().unwrap();
        assert!(id.starts_with("evt_"), "got {id}");
    }

    #[test]
    fn emit_keeps_existing_id() {
        let bus = GlobalBus::new();
        let mut rx = bus.subscribe();
        bus.emit(GlobalEvent {
            directory: None,
            project: None,
            workspace: None,
            payload: serde_json::json!({ "id": "evt_existing", "type": "workspace.ready", "properties": { "name": "x" } }),
        });
        let event = rx.try_recv().unwrap();
        assert_eq!(
            event.payload.get("id").unwrap().as_str().unwrap(),
            "evt_existing"
        );
    }
}
