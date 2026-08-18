//! In-memory cross-workspace sync transport.
//!
//! A pluggable [`SyncApi`] implementation that pairs two workspace instances
//! through a shared in-process control plane. It implements the same wire
//! contract as the remote control-plane HTTP surface (reference
//! `packages/opencode/src/server/routes/instance/httpapi/groups/sync.ts`):
//!
//! - `POST /sync/history` — given the caller's cursor state (`aggregateID ->
//!   seq`), return the events published after that cursor.
//! - `POST /sync/replay` — accept a batch of serialized events into the plane.
//! - `POST /sync/steal` — claim a session for the calling workspace.
//! - `GET /global/event` — a live SSE stream of published sync events.
//! - `GET /vcs/diff/raw` / `POST /vcs/apply` — the session-warp copy-changes
//!   surface.
//!
//! Tests use this transport instead of a scripted fake so the cross-workspace
//! replay/steal semantics are exercised against one stateful plane.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::broadcast;

use crate::sync::event::SerializedEvent;

use super::sync_api::{
    Method, ReplayPayload, SessionPayload, SyncApi, SyncHttpError, SyncHttpRequest,
    SyncHttpResponse,
};

/// The shared state behind one in-memory control plane.
pub struct MemoryControlPlane {
    /// session id -> committed events, ordered by `seq`.
    events: Mutex<HashMap<String, Vec<SerializedEvent>>>,
    /// session id -> owning workspace id.
    ownership: Mutex<HashMap<String, String>>,
    /// Live sync events, re-broadcast to `/global/event` subscribers.
    live: broadcast::Sender<Value>,
    history_calls: AtomicU64,
    replay_calls: AtomicU64,
    steal_calls: AtomicU64,
}

impl MemoryControlPlane {
    /// Create a new shared control plane.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Publish a sync event into the plane: commit it to the session history
    /// and broadcast it to live subscribers in the global-event envelope the
    /// workspace's `consume_event_stream` expects.
    pub fn publish(&self, event: &SerializedEvent) {
        self.events
            .lock()
            .expect("memory plane events poisoned")
            .entry(event.aggregate_id.clone())
            .or_default()
            .push(event.clone());
        let envelope = serde_json::json!({
            "directory": "global",
            "payload": {
                "type": "sync",
                "syncEvent": event,
            }
        });
        let _ = self.live.send(envelope);
    }

    /// The events committed for a session.
    pub fn history(&self, session_id: &str) -> Vec<SerializedEvent> {
        self.events
            .lock()
            .expect("memory plane events poisoned")
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// The workspace currently owning a session, if claimed.
    pub fn owner(&self, session_id: &str) -> Option<String> {
        self.ownership
            .lock()
            .expect("memory plane ownership poisoned")
            .get(session_id)
            .cloned()
    }

    /// Claim a session for `owner`.
    pub fn claim(&self, session_id: &str, owner: &str) {
        self.ownership
            .lock()
            .expect("memory plane ownership poisoned")
            .insert(session_id.to_string(), owner.to_string());
    }

    /// Number of `/sync/history` requests served.
    pub fn history_calls(&self) -> u64 {
        self.history_calls.load(Ordering::Relaxed)
    }

    /// Number of `/sync/replay` requests served.
    pub fn replay_calls(&self) -> u64 {
        self.replay_calls.load(Ordering::Relaxed)
    }

    /// Number of `/sync/steal` requests served.
    pub fn steal_calls(&self) -> u64 {
        self.steal_calls.load(Ordering::Relaxed)
    }
}

impl Default for MemoryControlPlane {
    fn default() -> Self {
        Self {
            events: Mutex::new(HashMap::new()),
            ownership: Mutex::new(HashMap::new()),
            live: broadcast::channel(256).0,
            history_calls: AtomicU64::new(0),
            replay_calls: AtomicU64::new(0),
            steal_calls: AtomicU64::new(0),
        }
    }
}

/// A [`SyncApi`] client view over a shared [`MemoryControlPlane`].
#[derive(Clone)]
pub struct MemorySyncApi {
    plane: Arc<MemoryControlPlane>,
}

impl MemorySyncApi {
    pub fn new(plane: Arc<MemoryControlPlane>) -> Self {
        Self { plane }
    }
}

impl MemorySyncApi {
    fn handle_history(&self, body: Option<&Value>) -> SyncHttpResponse {
        self.plane.history_calls.fetch_add(1, Ordering::Relaxed);
        let cursors: HashMap<String, u64> = body
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        let events = self.plane.events.lock().expect("memory plane poisoned");
        let mut result = Vec::new();
        for (session_id, history) in events.iter() {
            // Mirrors the reference: aggregates absent from the cursor payload
            // have nothing excluded, so all their events are returned.
            let cursor = cursors
                .get(session_id)
                .copied()
                .map(|seq| seq as i64)
                .unwrap_or(-1);
            for event in history {
                if event.seq > cursor {
                    result.push(serialized_to_history(event));
                }
            }
        }
        result.sort_by_key(|event| event.seq);
        SyncHttpResponse {
            status: 200,
            text: None,
            json: Some(serde_json::to_value(result).expect("history serializes")),
        }
    }

    fn handle_replay(&self, body: Option<&Value>) -> SyncHttpResponse {
        self.plane.replay_calls.fetch_add(1, Ordering::Relaxed);
        let payload: ReplayPayload = body
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or(ReplayPayload {
                directory: String::new(),
                events: Vec::new(),
            });
        let session_id = payload
            .events
            .first()
            .map(|event| event.aggregate_id.clone())
            .unwrap_or_default();
        for event in payload.events {
            self.plane.publish(&event);
        }
        SyncHttpResponse {
            status: 200,
            text: None,
            json: Some(serde_json::json!({ "sessionID": session_id })),
        }
    }

    fn handle_steal(&self, body: Option<&Value>) -> SyncHttpResponse {
        self.plane.steal_calls.fetch_add(1, Ordering::Relaxed);
        let payload: SessionPayload = body
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or(SessionPayload {
                session_id: String::new(),
            });
        // The caller claims ownership through the target workspace; record the
        // handoff on the plane.
        self.plane.claim(&payload.session_id, "remote");
        SyncHttpResponse {
            status: 200,
            text: None,
            json: Some(serde_json::json!({ "sessionID": payload.session_id })),
        }
    }
}

fn serialized_to_history(event: &SerializedEvent) -> crate::sync::event::HistoryEvent {
    crate::sync::event::HistoryEvent {
        id: event.id.clone(),
        aggregate_id: event.aggregate_id.clone(),
        seq: event.seq,
        r#type: event.r#type.clone(),
        data: event.data.clone(),
    }
}

#[async_trait::async_trait]
impl SyncApi for MemorySyncApi {
    async fn execute(&self, request: SyncHttpRequest) -> Result<SyncHttpResponse, SyncHttpError> {
        let path = request.url.split('?').next().unwrap_or(&request.url);
        let response = match (request.method, path) {
            (Method::Post, url) if url.ends_with("/sync/history") => {
                self.handle_history(request.body.as_ref())
            }
            (Method::Post, url) if url.ends_with("/sync/replay") => {
                self.handle_replay(request.body.as_ref())
            }
            (Method::Post, url) if url.ends_with("/sync/steal") => {
                self.handle_steal(request.body.as_ref())
            }
            (Method::Get, url) if url.ends_with("/vcs/diff/raw") => SyncHttpResponse {
                status: 200,
                text: Some("PATCH".into()),
                json: None,
            },
            (Method::Post, url) if url.ends_with("/vcs/apply") => SyncHttpResponse {
                status: 200,
                text: None,
                json: Some(serde_json::json!({ "applied": true })),
            },
            _ => SyncHttpResponse {
                status: 404,
                text: Some("not found".into()),
                json: None,
            },
        };
        Ok(response)
    }

    async fn event_stream(
        &self,
        _url: &str,
        _headers: &[(String, String)],
    ) -> Result<Box<dyn tokio::io::AsyncBufRead + Send + Unpin>, SyncHttpError> {
        use bytes::Bytes;

        let receiver = self.plane.live.subscribe();
        let stream = futures::stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let line = format!("data: {}\n\n", event);
                        return Some((Ok::<_, std::io::Error>(Bytes::from(line)), receiver));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        let reader = tokio_util::io::StreamReader::new(Box::pin(stream));
        Ok(Box::new(reader))
    }
}

/// Test helpers: counters exposed for assertions.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::sync_api::ResponseKind;
    use crate::sync::event::{Definition, EventID};
    use crate::sync::store::Store;

    #[tokio::test]
    async fn replay_and_steal_round_trip_through_the_plane() {
        let plane = MemoryControlPlane::new();
        let api = MemorySyncApi::new(plane.clone());
        let store = Store::new();

        // Publish 3 events for ses_1 in the source workspace.
        let def = Definition::durable("session.next.moved", "sessionID", 1);
        let mut events = Vec::new();
        for i in 0..3 {
            let event = store
                .publish(
                    &def,
                    serde_json::json!({ "sessionID": "ses_1", "i": i }),
                    Default::default(),
                )
                .unwrap();
            let serialized = SerializedEvent {
                id: EventID(event.id.0.clone()),
                r#type: def.storage_type(),
                seq: event.durable.as_ref().unwrap().seq,
                aggregate_id: "ses_1".into(),
                data: serde_json::json!({ "sessionID": "ses_1", "i": i }),
            };
            events.push(serialized);
        }

        // Replay into the target via the transport.
        let response = api
            .execute(SyncHttpRequest {
                method: Method::Post,
                url: "http://memory/sync/replay".into(),
                headers: vec![],
                body: Some(
                    serde_json::to_value(ReplayPayload {
                        directory: "/tmp".into(),
                        events: events.clone(),
                    })
                    .unwrap(),
                ),
                response: ResponseKind::Json,
            })
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.json.unwrap()["sessionID"], "ses_1");
        assert_eq!(plane.history("ses_1").len(), 3);

        // History with a cursor returns only the events after it.
        let mut cursors = HashMap::new();
        cursors.insert("ses_1".to_string(), 1);
        let response = api
            .execute(SyncHttpRequest {
                method: Method::Post,
                url: "http://memory/sync/history".into(),
                headers: vec![],
                body: Some(serde_json::to_value(&cursors).unwrap()),
                response: ResponseKind::Json,
            })
            .await
            .unwrap();
        let history = response.json.unwrap();
        let history = history.as_array().unwrap();
        assert_eq!(history.len(), 1, "only the event past seq 1");
        assert!(history
            .iter()
            .all(|event| event["seq"].as_i64().unwrap() > 1));

        // Steal moves the ownership on the plane.
        let response = api
            .execute(SyncHttpRequest {
                method: Method::Post,
                url: "http://memory/sync/steal".into(),
                headers: vec![],
                body: Some(
                    serde_json::to_value(SessionPayload {
                        session_id: "ses_1".into(),
                    })
                    .unwrap(),
                ),
                response: ResponseKind::Json,
            })
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(plane.owner("ses_1").as_deref(), Some("remote"));

        // Metrics reflect the round trip.
        assert_eq!(plane.replay_calls(), 1);
        assert_eq!(plane.history_calls(), 1);
        assert_eq!(plane.steal_calls(), 1);
    }

    #[tokio::test]
    async fn live_event_stream_delivers_published_events() {
        use tokio::io::AsyncBufReadExt;

        let plane = MemoryControlPlane::new();
        let api = MemorySyncApi::new(plane.clone());
        let reader = api
            .event_stream("http://memory/global/event", &[])
            .await
            .expect("event stream opens");

        let event = SerializedEvent {
            id: EventID("evt_live".into()),
            r#type: "session.next.moved.1".into(),
            seq: 0,
            aggregate_id: "ses_live".into(),
            data: serde_json::json!({ "sessionID": "ses_live" }),
        };
        plane.publish(&event);

        let mut lines = reader.lines();
        let line = tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
            .await
            .expect("event line should arrive")
            .expect("stream open")
            .expect("line");
        assert!(line.starts_with("data: "), "unexpected SSE line {line:?}");
        let parsed: Value = serde_json::from_str(line.strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(parsed["payload"]["type"], "sync");
        assert_eq!(
            parsed["payload"]["syncEvent"]["type"],
            "session.next.moved.1"
        );
    }
}
