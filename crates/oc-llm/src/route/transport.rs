//! HTTP transport: URL/auth preparation and stream framing.
//! From reference/packages/llm/src/route/transport/http.ts

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;
use url::Url;

use super::auth::{Auth, AuthInput};
use super::endpoint::{self, Endpoint};
use super::framing::Framing;
use super::protocol::FramePayload;
use crate::schema::{LlmError, LlmRequest};
use crate::shared;

/// `JsonRequestParts`.
/// From reference/packages/llm/src/route/transport/http.ts (`JsonRequestParts`)
#[derive(Debug, Clone)]
pub struct JsonRequestParts {
    pub url: String,
    pub body_text: String,
    pub headers: BTreeMap<String, String>,
}

/// `HttpPrepared`.
/// From reference/packages/llm/src/route/transport/http.ts (`HttpPrepared`)
#[derive(Debug, Clone)]
pub struct HttpPrepared {
    pub request: HttpRequestValue,
    pub framing: Framing,
}

#[derive(Debug, Clone)]
pub struct HttpRequestValue {
    pub url: String,
    pub body: String,
    pub headers: BTreeMap<String, String>,
}

/// `PROTOCOL_BODY_OVERLAY_DENYLIST` — fields `http.body` may not overlay.
/// From reference/packages/llm/src/route/transport/http.ts
const PROTOCOL_BODY_OVERLAY_DENYLIST: &[&str] = &[
    "content",
    "contents",
    "frequencyPenalty",
    "frequency_penalty",
    "generationConfig",
    "inferenceConfig",
    "input",
    "maxTokens",
    "max_tokens",
    "messages",
    "model",
    "presencePenalty",
    "presence_penalty",
    "responseFormat",
    "response_format",
    "seed",
    "stop",
    "stopSequences",
    "stop_sequences",
    "stream",
    "streamOptions",
    "stream_options",
    "system",
    "systemInstruction",
    "system_instruction",
    "temperature",
    "thinking",
    "toolChoice",
    "toolConfig",
    "tool_choice",
    "tool_config",
    "tools",
    "topK",
    "topP",
    "top_k",
    "top_p",
];

fn apply_query(url: &Url, query: Option<&BTreeMap<String, String>>) -> Url {
    let mut url = url.clone();
    if let Some(query) = query {
        for (key, value) in query {
            url.query_pairs_mut().append_pair(key, value);
        }
    }
    url
}

fn forbidden_body_overlay_keys(body: &Value) -> Vec<String> {
    let Some(obj) = body.as_object() else {
        return vec![];
    };
    obj.keys()
        .filter(|key| PROTOCOL_BODY_OVERLAY_DENYLIST.contains(&key.as_str()))
        .cloned()
        .collect()
}

/// `jsonRequestParts` — render the endpoint, overlay http options, apply auth.
/// From reference/packages/llm/src/route/transport/http.ts (`jsonRequestParts`)
pub fn json_request_parts(
    body: &Value,
    request: &LlmRequest,
    endpoint: &Endpoint,
    auth: &Auth,
    headers: Option<&Arc<dyn Fn(&LlmRequest) -> BTreeMap<String, String> + Send + Sync>>,
) -> Result<JsonRequestParts, LlmError> {
    let endpoint_input = endpoint::EndpointInput { request, body };
    let rendered = endpoint::render(endpoint, &endpoint_input)?;
    let url = apply_query(
        &rendered,
        request.http.as_ref().and_then(|http| http.query.as_ref()),
    );
    let url = url.to_string();

    let (json_body, body_text) = match &request.http.as_ref().and_then(|http| http.body.as_ref()) {
        None => (body.clone(), encode_json(body)),
        Some(overlay) => {
            let forbidden = forbidden_body_overlay_keys(overlay);
            if !forbidden.is_empty() {
                return Err(shared::invalid_request(format!(
                    "http.body cannot overlay protocol-owned field(s): {}",
                    forbidden.join(", ")
                )));
            }
            if shared::is_record(body) {
                let merged = crate::schema::merge_json_records(
                    body.as_object()
                        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
                    &overlay
                        .as_object()
                        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default(),
                )
                .map(|map| Value::Object(map.into_iter().collect()))
                .unwrap_or(Value::Null);
                (merged.clone(), encode_json(&merged))
            } else {
                return Err(shared::invalid_request(
                    "http.body can only overlay JSON object request bodies",
                ));
            }
        }
    };

    let mut headers_map = BTreeMap::new();
    if let Some(headers) = headers {
        headers_map.extend(headers(request));
    }
    if let Some(http) = &request.http {
        if let Some(request_headers) = &http.headers {
            headers_map.extend(request_headers.clone());
        }
    }

    let auth_input = AuthInput {
        request: request.clone(),
        method: "POST".to_string(),
        url: url.clone(),
        body: body_text.clone(),
        headers: headers_map,
    };
    let mut headers = auth.apply(&auth_input)?;

    // `ProviderShared.jsonPost` always sets `content-type: application/json`
    // after caller-supplied headers so routes never send JSON with a stale
    // content type. The body is passed pre-encoded; force the same framing
    // here so OpenAI-compatible endpoints do not reject the request.
    headers.insert("content-type".to_string(), "application/json".to_string());

    let _ = json_body;
    Ok(JsonRequestParts {
        url,
        body_text,
        headers,
    })
}

fn encode_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// `HttpJsonTransport.frames` — execute the request and cut the byte stream
/// into protocol frames.
/// From reference/packages/llm/src/route/transport/http.ts (`frames`)
pub fn frames(
    prepared: &HttpPrepared,
    request: &LlmRequest,
    response_body: Pin<Box<dyn Stream<Item = Result<Bytes, LlmError>> + Send>>,
) -> Pin<Box<dyn Stream<Item = Result<FramePayload, LlmError>> + Send>> {
    let route = format!("{}/{}", request.model.provider, request.model.route.id);
    match prepared.framing {
        Framing::Sse => Box::pin(SseStream::new(response_body).map(move |data| {
            data.map_err(|_| {
                LlmError::event_error(&route, format!("Failed to read {} stream", route), None)
            })
            .map(FramePayload::Json)
        })),
        Framing::AwsEventStream => {
            Box::pin(AwsEventStream::new(response_body).map(move |value| value))
        }
    }
}

/// SSE framing over a byte stream. Drops empty and `[DONE]` keep-alives.
/// From reference/packages/llm/src/protocols/shared.ts (`sseFraming`)
pub struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, LlmError>> + Send>>,
    buf: Vec<u8>,
    pending: std::collections::VecDeque<String>,
}

impl SseStream {
    pub fn new(inner: Pin<Box<dyn Stream<Item = Result<Bytes, LlmError>> + Send>>) -> SseStream {
        SseStream {
            inner,
            buf: Vec::new(),
            pending: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self) {
        loop {
            let Some(delim) = find_delim(&self.buf) else {
                break;
            };
            let (start, len) = delim;
            let event_bytes = &self.buf[..start];
            let event = String::from_utf8_lossy(event_bytes).replace('\r', "");
            self.buf.drain(..start + len);
            if let Some(data) = parse_sse_event(&event) {
                if !data.is_empty() && data != "[DONE]" {
                    self.pending.push_back(data);
                }
            }
        }
    }
}

impl Stream for SseStream {
    type Item = Result<String, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(data) = self.pending.pop_front() {
                return Poll::Ready(Some(Ok(data)));
            }
            match self.inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.buf.extend_from_slice(&bytes);
                    self.process();
                    if !self.pending.is_empty() {
                        continue;
                    }
                }
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Find the next SSE event delimiter. Returns `(start_index, delimiter_len)`.
fn find_delim(buf: &[u8]) -> Option<(usize, usize)> {
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < buf.len()
            && buf[i] == b'\r'
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, 4));
        }
    }
    None
}

fn parse_sse_event(event: &str) -> Option<String> {
    let mut data = Vec::new();
    for line in event.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data.push(rest.to_string());
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

/// AWS event-stream framing for Bedrock Converse.
/// From reference/packages/llm/src/protocols/bedrock-event-stream.ts
pub struct AwsEventStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, LlmError>> + Send>>,
    buf: Vec<u8>,
    pending: std::collections::VecDeque<Value>,
    route: String,
}

impl AwsEventStream {
    pub fn new(
        inner: Pin<Box<dyn Stream<Item = Result<Bytes, LlmError>> + Send>>,
    ) -> AwsEventStream {
        AwsEventStream {
            inner,
            buf: Vec::new(),
            pending: std::collections::VecDeque::new(),
            route: "bedrock-converse".to_string(),
        }
    }

    fn process(&mut self) -> Result<(), LlmError> {
        loop {
            if self.buf.len() < 4 {
                break;
            }
            let total_length =
                u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
            if self.buf.len() < total_length {
                break;
            }
            let frame_bytes = self.buf.drain(..total_length).collect::<Vec<_>>();
            if let Some(value) = decode_frame(&frame_bytes, &self.route)? {
                self.pending.push_back(value);
            }
        }
        Ok(())
    }
}

impl Stream for AwsEventStream {
    type Item = Result<FramePayload, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(value) = self.pending.pop_front() {
                return Poll::Ready(Some(Ok(FramePayload::Aws(value))));
            }
            match self.inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.buf.extend_from_slice(&bytes);
                    if let Err(error) = self.process() {
                        return Poll::Ready(Some(Err(error)));
                    }
                }
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Decode one length-prefixed AWS event-stream frame. Returns `None` for
/// non-`event` message types.
///
/// Frame layout: `[totalLen:4][headersLen:4][preludeCrc:4][headers][payload][crc:4]`.
/// The payload JSON is rewrapped under its `:event-type` header and any `p`
/// padding field is dropped.
fn decode_frame(frame: &[u8], route: &str) -> Result<Option<Value>, LlmError> {
    if frame.len() < 12 {
        return Err(crate::shared::event_error(
            route,
            "Failed to decode Bedrock Converse event-stream frame: frame too short",
            None,
        ));
    }
    let headers_length = u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    let total_length = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if 12 + headers_length > total_length {
        return Err(crate::shared::event_error(
            route,
            "Failed to decode Bedrock Converse event-stream frame: invalid header length",
            None,
        ));
    }
    let prelude_crc = u32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]);
    let computed = crc32(&frame[..8]);
    if computed != prelude_crc {
        return Err(crate::shared::event_error(
            route,
            "Failed to decode Bedrock Converse event-stream frame: prelude CRC mismatch",
            None,
        ));
    }
    let message_crc = u32::from_be_bytes([
        frame[total_length - 4],
        frame[total_length - 3],
        frame[total_length - 2],
        frame[total_length - 1],
    ]);
    let computed = crc32(&frame[..total_length - 4]);
    if computed != message_crc {
        return Err(crate::shared::event_error(
            route,
            "Failed to decode Bedrock Converse event-stream frame: message CRC mismatch",
            None,
        ));
    }

    let headers_end = 12 + headers_length;
    let headers = &frame[12..headers_end];
    let payload = &frame[headers_end..total_length - 4];

    let mut message_type: Option<String> = None;
    let mut event_type: Option<String> = None;
    let mut cursor = 0usize;
    while cursor < headers.len() {
        let name_len = headers[cursor] as usize;
        cursor += 1;
        if cursor + name_len + 2 > headers.len() {
            break;
        }
        let name = String::from_utf8_lossy(&headers[cursor..cursor + name_len]).to_string();
        cursor += name_len;
        let value_type = headers[cursor];
        cursor += 1;
        if cursor + 2 > headers.len() {
            break;
        }
        let value_len = u16::from_be_bytes([headers[cursor], headers[cursor + 1]]) as usize;
        cursor += 2;
        if cursor + value_len > headers.len() {
            break;
        }
        let value_bytes = &headers[cursor..cursor + value_len];
        cursor += value_len;
        if value_type == 7 {
            let value = String::from_utf8_lossy(value_bytes).to_string();
            if name == ":message-type" {
                message_type = Some(value);
            } else if name == ":event-type" {
                event_type = Some(value);
            }
        }
    }

    if message_type.as_deref() != Some("event") {
        return Ok(None);
    }
    let Some(event_type) = event_type else {
        return Ok(None);
    };
    if payload.is_empty() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(payload);
    let mut parsed = crate::shared::parse_json(
        route,
        &text,
        "Failed to parse Bedrock Converse event-stream payload",
    )?;
    if let Value::Object(obj) = &mut parsed {
        obj.remove("p");
    }
    let mut wrapped = serde_json::Map::new();
    wrapped.insert(event_type, parsed);
    Ok(Some(Value::Object(wrapped)))
}

/// CRC-32 (IEEE 802.3) used by AWS event-stream framing.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// `HttpTransport` namespace marker.
/// From reference/packages/llm/src/route/transport/http.ts
pub const _ID: &str = "http-json";

#[allow(unused)]
pub(crate) fn _arc_marker(_: Arc<()>) {}
