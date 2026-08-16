//! MCP client transports.
//!
//! Port of `@modelcontextprotocol/sdk@1.29.0` `client/stdio.js`,
//! `client/streamableHttp.js` and `client/sse.js` as used by
//! `reference/packages/opencode/src/mcp/index.ts`.

pub mod http;
pub mod sse;
pub mod stdio;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::jsonrpc::Message;
use crate::util::BoxFuture;

pub type MessageReceiver = mpsc::UnboundedReceiver<Message>;

/// A transport over which JSON-RPC messages flow. `start` spawns a background
/// read loop that emits incoming messages on the returned channel; when the
/// remote end closes, the channel is dropped.
pub trait Transport: Send + Sync {
    fn start(&self) -> BoxFuture<'_, crate::Result<MessageReceiver>>;
    fn send(&self, message: Message) -> BoxFuture<'_, crate::Result<()>>;
    fn close(&self) -> BoxFuture<'_, crate::Result<()>>;
    /// Child process id for stdio transports (used to kill descendants).
    fn pid(&self) -> Option<u32> {
        None
    }
    /// Set the negotiated MCP protocol version; HTTP transports must echo it in
    /// every request header. No-op for stdio.
    fn set_protocol_version(&self, _version: String) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
    /// Complete an interactive OAuth authorization with the received code.
    fn finish_auth(&self, _code: &str) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Shared open/closed flag used by HTTP transports.
#[derive(Clone)]
pub(crate) struct OpenFlag {
    closed: Arc<AtomicBool>,
}

impl OpenFlag {
    pub(crate) fn new() -> Self {
        OpenFlag {
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// A parsed Server-Sent Events event (`event:`/`data:` fields).
#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: String,
}

/// Streaming SSE parser used by the HTTP transports.
pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn new() -> Self {
        SseParser {
            buffer: String::new(),
        }
    }

    /// Feed a raw byte chunk; returns any complete events. The parser keeps
    /// `data` lines for the current event and splits on blank lines.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let text = String::from_utf8_lossy(bytes);
        self.buffer.push_str(&text);

        let mut events = Vec::new();
        loop {
            let (end, delimiter_len) = if let Some(end) = self.buffer.find("\n\n") {
                (end, 2)
            } else if let Some(end) = self.buffer.find("\r\n\r\n") {
                (end, 4)
            } else {
                break;
            };
            let raw = self.buffer[..end].to_string();
            self.buffer = self.buffer[end + delimiter_len..].to_string();
            if let Some(event) = parse_sse_event(&raw) {
                events.push(event);
            }
        }
        events
    }
}

fn parse_sse_event(raw: &str) -> Option<SseEvent> {
    let mut event: Option<String> = None;
    let mut id: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();
    for line in raw.lines() {
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (
                field.trim(),
                value.strip_prefix(' ').unwrap_or(value).to_string(),
            ),
            None => (line.trim(), String::new()),
        };
        match field {
            "event" => event = Some(value),
            "id" => id = Some(value),
            "data" => data_lines.push(value),
            _ => {}
        }
    }
    if data_lines.is_empty() && id.is_none() {
        return None;
    }
    Some(SseEvent {
        event,
        id,
        data: data_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_handles_message_and_endpoint_events() {
        let mut parser = SseParser::new();
        let events = parser.feed(
            b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\nevent: endpoint\ndata: /messages\n\n",
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].id, None);
        assert_eq!(events[0].data, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        assert_eq!(events[1].event.as_deref(), Some("endpoint"));
        assert_eq!(events[1].id, None);
        assert_eq!(events[1].data, "/messages");
    }

    #[test]
    fn sse_parser_handles_chunked_and_default_event() {
        let mut parser = SseParser::new();
        assert!(parser.feed(b"data: {\"a\":1").is_empty());
        let events = parser.feed(b"}\n\ndata: {\"b\":2}\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, None);
        assert_eq!(events[0].data, r#"{"a":1}"#);
        assert_eq!(events[1].data, r#"{"b":2}"#);
    }

    #[test]
    fn sse_parser_ignores_comments_and_empty_events() {
        let mut parser = SseParser::new();
        let events = parser.feed(b": keepalive\n\ndata: x\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "x");
    }

    #[test]
    fn sse_parser_preserves_event_ids_and_id_only_events() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"id: 42\n\ndata: {\"ok\":true}\n\nid: 43\n\n");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert!(events[0].data.is_empty());
        assert_eq!(events[1].id, None);
        assert_eq!(events[1].data, r#"{"ok":true}"#);
        assert_eq!(events[2].id.as_deref(), Some("43"));
        assert!(events[2].data.is_empty());
    }

    #[test]
    fn sse_parser_consumes_crlf_boundaries_without_stalling() {
        let mut parser = SseParser::new();
        let events = parser
            .feed(b"event: message\r\ndata: first\r\n\r\nevent: message\r\ndata: second\r\n\r\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");
    }
}
