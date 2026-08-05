//! Server-sent events parsing.
//!
//! Ports `parseSSE` in reference/packages/opencode/src/control-plane/workspace.ts:
//! line-based `field: value` parsing, `data` accumulation joined by `\n`, `id`
//! persistence across events, `retry` (default 1000), flush on blank lines / end
//! of stream, then `JSON.parse` of the payload with an `sse.message` fallback.

use serde_json::Value;

/// A raw SSE event before JSON parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub data: String,
    pub id: Option<String>,
    pub retry: u64,
}

#[derive(Debug, Clone)]
struct ParseState {
    data: Vec<String>,
    id: Option<String>,
    retry: u64,
}

impl ParseState {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            id: None,
            retry: 1000,
        }
    }

    fn handle_line(&mut self, line: &str, out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if !self.data.is_empty() {
                out.push(self.flush());
            }
            return;
        }
        let (field, value) = match line.find(':') {
            Some(index) => {
                let field = &line[..index];
                let rest = &line[index + 1..];
                let value = rest.strip_prefix(' ').unwrap_or(rest);
                (field, value)
            }
            None => (line, ""),
        };
        match field {
            "data" => self.data.push(value.to_string()),
            "id" => self.id = Some(value.to_string()),
            "retry" => {
                if let Ok(retry) = value.parse::<u64>() {
                    self.retry = retry;
                }
            }
            _ => {}
        }
    }

    fn flush(&mut self) -> SseEvent {
        let event = SseEvent {
            data: self.data.join("\n"),
            id: self.id.clone(),
            retry: self.retry,
        };
        self.data.clear();
        event
    }
}

/// Map a raw SSE event to its parsed form: `JSON.parse(data)`, or the
/// `{type: "sse.message", properties: {data, id, retry}}` fallback.
fn map_event(event: SseEvent) -> Value {
    match serde_json::from_str(&event.data) {
        Ok(value) => value,
        Err(_) => {
            let mut properties = serde_json::Map::new();
            properties.insert("data".into(), Value::String(event.data));
            if let Some(id) = event.id {
                properties.insert("id".into(), Value::String(id));
            }
            properties.insert("retry".into(), Value::Number(event.retry.into()));
            serde_json::json!({ "type": "sse.message", "properties": properties })
        }
    }
}

/// Parse a complete SSE payload (as accumulated from the wire) into parsed events.
pub fn parse_sse(input: &str) -> Vec<Value> {
    let mut state = ParseState::new();
    let mut raw = Vec::new();
    for line in input.split('\n') {
        state.handle_line(line, &mut raw);
    }
    if !state.data.is_empty() {
        raw.push(state.flush());
    }
    raw.into_iter().map(map_event).collect()
}

/// Consume a line-oriented stream, calling `on_event` for each parsed event.
/// Mirrors the streaming `Stream.runForEach(onEvent)` in the reference.
pub async fn parse_sse_stream<B, F, Fut>(reader: &mut B, mut on_event: F) -> anyhow::Result<()>
where
    B: tokio::io::AsyncBufRead + Unpin,
    F: FnMut(Value) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    use tokio::io::AsyncBufReadExt;
    let mut state = ParseState::new();
    let mut lines = reader.lines();
    loop {
        let Some(line) = lines.next_line().await? else {
            if !state.data.is_empty() {
                on_event(map_event(state.flush())).await?;
            }
            break;
        };
        let mut batch = Vec::new();
        state.handle_line(&line, &mut batch);
        for event in batch {
            on_event(map_event(event)).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_json_event() {
        let events = parse_sse("data: {\"type\":\"sync\",\"id\":\"evt_1\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            serde_json::json!({ "type": "sync", "id": "evt_1" })
        );
    }

    #[test]
    fn parses_multiple_events() {
        let input = "data: {\"a\":1}\n\nid: evt_5\ndata: {\"b\":2}\n\n";
        let events = parse_sse(input);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], serde_json::json!({ "a": 1 }));
        assert_eq!(events[1], serde_json::json!({ "b": 2 }));
    }

    #[test]
    fn id_persists_across_events_until_overridden() {
        let input = "id: evt_keep\ndata: {\"a\":1}\n\nid: evt_new\ndata: {\"b\":2}\n\n";
        let events = parse_sse(input);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn multiline_data_joins_with_newline() {
        let input = "data: first\ndata: second\n\n";
        let events = parse_sse(input);
        assert_eq!(events.len(), 1);
        assert!(events[0].is_object());
    }

    #[test]
    fn non_json_payload_falls_back_to_sse_message() {
        let events = parse_sse("data: hello world\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            serde_json::json!({
                "type": "sse.message",
                "properties": { "data": "hello world", "retry": 1000 }
            })
        );
    }

    #[test]
    fn non_json_with_id_and_custom_retry() {
        let events = parse_sse("retry: 250\nid: evt_x\ndata: plain\n\n");
        assert_eq!(
            events[0],
            serde_json::json!({
                "type": "sse.message",
                "properties": { "data": "plain", "id": "evt_x", "retry": 250 }
            })
        );
    }

    #[test]
    fn retry_defaults_to_1000_and_ignores_invalid() {
        let mut state = ParseState::new();
        let mut out = Vec::new();
        state.handle_line("retry: not-a-number", &mut out);
        state.handle_line("data: {} ", &mut out); // trailing space kept in value
        state.handle_line("", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].retry, 1000);
    }

    #[test]
    fn flush_without_trailing_newline() {
        let events = parse_sse("data: {\"x\":1}");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], serde_json::json!({ "x": 1 }));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let events = parse_sse("event: ping\ndata: {\"ok\":true}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], serde_json::json!({ "ok": true }));
    }

    #[tokio::test]
    async fn stream_parses_and_calls_on_event() {
        let input = "data: {\"type\":\"sync\"}\n\ndata: {\"type\":\"heartbeat\"}\n\n";
        let mut reader = input.as_bytes();
        let mut seen = Vec::new();
        parse_sse_stream(&mut reader, |event| {
            seen.push(event);
            async { Ok(()) }
        })
        .await
        .unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], serde_json::json!({ "type": "sync" }));
        assert_eq!(seen[1], serde_json::json!({ "type": "heartbeat" }));
    }
}
