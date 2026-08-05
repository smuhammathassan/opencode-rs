//! Shared test helpers for protocol golden and stream-parsing tests.
#![allow(dead_code)]

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::{BoxStream, StreamExt};
use oc_llm::route::protocol::{FramePayload, ProtoStream, ProtocolStream};
use oc_llm::route::{Auth, EndpointPatch, RoutePatch};
use oc_llm::schema::{
    response_complete, response_empty, response_reduce, LlmError, LlmEvent, LlmRequest,
    LlmResponse, Model,
};

/// Configure a route's base URL + bearer auth so `route.model(...)` works.
pub fn configured(model_id: &str, route: oc_llm::Route) -> Model {
    let mut patch = RoutePatch::empty();
    patch.auth = Some(Auth::bearer(Auth::value("test")));
    patch.endpoint = Some(EndpointPatch::base_url("https://api.openai.test/v1/"));
    route
        .with(patch)
        .model(oc_llm::RouteModelInput {
            id: model_id.to_string(),
            provider: None,
            defaults: None,
            compatibility: None,
        })
        .unwrap()
}

/// Parse an SSE document into its `data:` payloads.
pub fn sse_events(body: &str) -> Vec<String> {
    body.split("\n\n")
        .filter_map(|event| {
            let mut data = Vec::new();
            for line in event.lines() {
                if let Some(rest) = line.strip_prefix("data:") {
                    data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
                }
            }
            if data.is_empty() {
                None
            } else {
                Some(data.join("\n"))
            }
        })
        .filter(|data| !data.is_empty() && data != "[DONE]")
        .collect()
}

/// Feed recorded SSE payloads through a protocol's parser.
pub async fn parse_events(
    protocol: Arc<dyn ProtocolStream>,
    request: LlmRequest,
    sse: &str,
) -> Vec<LlmEvent> {
    let frames: Vec<Result<FramePayload, LlmError>> = sse_events(sse)
        .into_iter()
        .map(FramePayload::Json)
        .map(Ok)
        .collect();
    let stream: BoxStream<'static, Result<FramePayload, LlmError>> =
        futures::stream::iter(frames).boxed();
    let mut proto = ProtoStream::new(Pin::from(stream), protocol, request);
    let mut events = Vec::new();
    while let Some(event) = proto.next().await {
        events.push(event.unwrap());
    }
    events
}

/// Fold collected events into a completed response.
pub fn complete(events: &[LlmEvent]) -> LlmResponse {
    let state = events.iter().fold(response_empty(), |state, event| {
        response_reduce(&state, event)
    });
    response_complete(&state).expect("stream should terminate")
}

/// Build an OpenAI Chat model bound to the test URL.
pub fn openai_chat_model(model_id: &str) -> Model {
    configured(model_id, oc_llm::protocols::openai_chat::route())
}
