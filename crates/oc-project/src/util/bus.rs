/// Minimal in-process event bus ported from `@/bus/global` (GlobalBus.emit)
/// and `@/event-v2-bridge` (EventV2Bridge.listen/publish). Events carry a
/// generic `payload { type, properties | data }` mirroring the reference bus
/// envelope; listeners match on `payload.type` and `location.directory`.
///
/// TODO(integration): replace with the real oc-core bus / EventV2 bridge once
/// oc-core lands; this is a compatible local shim.
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventPayload {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<EventLocation>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventLocation {
    pub directory: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BusEvent {
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub payload: EventPayload,
}

#[derive(Clone)]
pub struct Bus {
    sender: broadcast::Sender<BusEvent>,
}

impl Bus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Bus { sender }
    }

    pub fn emit(&self, event: BusEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.sender.subscribe()
    }

    pub fn listener(&self) -> EventListener {
        EventListener {
            receiver: self.sender.subscribe(),
        }
    }
}

/// EventV2Bridge-style listener: wraps a broadcast receiver and skips lagged
/// events, matching the reference's `events.listen` semantics closely enough.
pub struct EventListener {
    receiver: broadcast::Receiver<BusEvent>,
}

impl EventListener {
    /// Waits for the next matching event. Returns `None` when the bus is
    /// closed. `matches` receives the payload type and location directory.
    pub async fn next<F>(&mut self, matches: F) -> Option<BusEvent>
    where
        F: Fn(&str, Option<&str>) -> bool,
    {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    let location = event
                        .payload
                        .location
                        .as_ref()
                        .map(|l| l.directory.as_str());
                    if matches(&event.payload.r#type, location) {
                        return Some(event);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

/// Sink type used to emit project/instance events (GlobalBus.emit equivalent).
pub type EventSink = Arc<Bus>;
