//! Stream framing: bytes -> frames.
//! From reference/packages/llm/src/route/framing.ts

use crate::schema::LlmError;

/// `Framing<Frame>` — how the byte stream is cut into protocol frames.
/// From reference/packages/llm/src/route/framing.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// Server-Sent Events: one JSON `data:` payload per frame.
    Sse,
    /// AWS event-stream binary frames (Bedrock Converse).
    AwsEventStream,
}

impl Framing {
    pub fn id(&self) -> &'static str {
        match self {
            Framing::Sse => "sse",
            Framing::AwsEventStream => "aws-event-stream",
        }
    }
}

/// `Framing.sse`.
/// From reference/packages/llm/src/route/framing.ts (`sse`)
pub const SSE: Framing = Framing::Sse;

/// A framed unit decoded from the wire.
#[derive(Debug, Clone)]
pub enum Frame {
    /// One SSE `data:` payload (a JSON string).
    Json(String),
    /// One decoded AWS event-stream payload wrapped by `:event-type`.
    AwsEvent(serde_json::Value),
}

/// `Framing.sseFraming` — drop empty / `[DONE]` keep-alives.
/// From reference/packages/llm/src/protocols/shared.ts (`sseFraming`)
pub fn is_keep_alive(data: &str) -> bool {
    data.is_empty() || data == "[DONE]"
}

#[allow(unused)]
pub(crate) fn _llm_error(_: &LlmError) {}
