//! SSE framing and event streams.
//!
//! Wire format matches `effect/unstable/encoding/Sse` as used by
//! reference/packages/server/src/handlers/event.ts: `event:` + `data:` lines with a
//! blank line between events, and `: heartbeat\n\n` keep-alive comments.

use std::time::Duration;

use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::event::{server_connected, Event, EventBus};
use crate::state::AppState;

/// Shared SSE response headers from the reference event handlers.
pub const SSE_HEADERS: [(&str, &str); 3] = [
    ("cache-control", "no-cache, no-transform"),
    ("x-accel-buffering", "no"),
    ("x-content-type-options", "nosniff"),
];

/// Encode one event as an SSE message. From reference/packages/server/src/handlers/event.ts
/// (`eventData`): `event: message` with `data:` being the JSON payload.
fn event_data(data: serde_json::Value) -> SseEvent {
    SseEvent::default()
        .event("message")
        .data(serde_json::to_string(&data).unwrap_or_default())
}

fn sse_response<S, E>(stream: S, heartbeat_secs: u64) -> Response
where
    S: tokio_stream::Stream<Item = Result<SseEvent, E>> + Send + 'static,
    E: Into<axum::BoxError>,
{
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(heartbeat_secs))
                .text("heartbeat"),
        )
        .into_response();
    for (key, value) in SSE_HEADERS {
        if let Ok(value) = value.parse() {
            response.headers_mut().insert(key, value);
        }
    }
    response
}

fn sse_of(first: SseEvent, bus: &EventBus, heartbeat_secs: u64) -> Response {
    let receiver = BroadcastStream::new(bus.subscribe());
    let live = receiver
        .filter_map(move |result| match result {
            Ok(event) => Some(event_data(serde_json::to_value(event).unwrap_or_default())),
            Err(_) => None,
        })
        .map(Ok::<SseEvent, std::convert::Infallible>);
    let stream = tokio_stream::iter([Ok::<SseEvent, std::convert::Infallible>(first)]).chain(live);
    sse_response(stream, heartbeat_secs)
}

/// The v2 `/api/event` stream. From reference/packages/server/src/handlers/event.ts:
/// emits `server.connected` first, then all live bus events, plus a 15s heartbeat.
pub fn v2_event_stream(bus: EventBus) -> Response {
    sse_of(
        event_data(serde_json::to_value(server_connected()).unwrap()),
        &bus,
        15,
    )
}

/// The v1 `/event` stream. From reference/packages/opencode/src/server/routes/instance/
/// httpapi/handlers/event.ts: emits `server.connected`, then events shaped
/// `{ id, type, properties }`, plus a 10s heartbeat.
pub fn v1_event_stream(bus: EventBus) -> Response {
    let connected = event_data(serde_json::to_value(server_connected()).unwrap());
    let receiver = BroadcastStream::new(bus.subscribe());
    let live = receiver
        .filter_map(move |result| match result {
            Ok(Event {
                id, r#type, data, ..
            }) => Some(event_data(
                serde_json::json!({ "id": id, "type": r#type, "properties": data }),
            )),
            Err(_) => None,
        })
        .map(Ok::<SseEvent, std::convert::Infallible>);
    let stream =
        tokio_stream::iter([Ok::<SseEvent, std::convert::Infallible>(connected)]).chain(live);
    sse_response(stream, 10)
}

/// The v2 `/api/session/:sessionID/event` stream. From reference/packages/server/src/
/// handlers/session.ts (`session.events`): continues with new durable events.
pub fn session_event_stream(state: AppState) -> Response {
    sse_of(SseEvent::default(), &state.events, 15)
}

/// Build a stream of raw SSE frames for one published event (used by tests).
pub fn sse_frame(event: &Event) -> String {
    let data = serde_json::to_string(event).unwrap_or_default();
    format!("event: message\ndata: {data}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::event_id;

    #[test]
    fn frame_uses_event_and_data_lines() {
        let event = Event {
            id: event_id(),
            metadata: None,
            r#type: "session.next.text.ended".into(),
            durable: None,
            location: None,
            data: serde_json::json!({ "text": "hi" }),
        };
        let frame = sse_frame(&event);
        assert!(frame.starts_with("event: message\n"), "frame: {frame:?}");
        let json = frame
            .strip_prefix("event: message\ndata: ")
            .and_then(|rest| rest.strip_suffix("\n\n"))
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(payload["type"], "session.next.text.ended");
        assert_eq!(payload["data"]["text"], "hi");
    }

    #[test]
    fn connected_event_frame_matches_reference() {
        let event = server_connected();
        assert_eq!(event.r#type, "server.connected");
        let frame = sse_frame(&event);
        let json = frame
            .strip_prefix("event: message\ndata: ")
            .and_then(|rest| rest.strip_suffix("\n\n"))
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(payload["type"], "server.connected");
        assert_eq!(payload["data"], serde_json::json!({}));
        assert!(payload["id"].as_str().unwrap().starts_with("evt_"));
    }
}
