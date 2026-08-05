//! Protocol abstraction — body construction + streaming state machine.
//! From reference/packages/llm/src/route/protocol.ts

use std::any::Any;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use crate::schema::{LlmError, LlmEvent, LlmRequest};
use crate::shared;

/// One framed payload decoded from the transport byte stream.
#[derive(Debug, Clone)]
pub enum FramePayload {
    /// SSE `data:` payload — a JSON string.
    Json(String),
    /// Pre-parsed AWS event-stream payload (Bedrock).
    Aws(Value),
}

/// `ProtocolStream` — request -> provider event state machine.
/// From reference/packages/llm/src/route/protocol.ts (`ProtocolStream`)
pub trait ProtocolStream: Send + Sync {
    /// Initial parser state. Called once per response with the resolved request.
    fn initial(&self, request: &LlmRequest) -> Box<dyn Any + Send>;

    /// Translate one event into emitted `LLMEvent`s plus the next state.
    fn step(&self, state: Box<dyn Any + Send>, event: &Value) -> Result<(Box<dyn Any + Send>, Vec<LlmEvent>), LlmError>;

    /// Optional request-completion signal for transports that do not end naturally.
    fn terminal(&self, event: &Value) -> bool;

    /// Optional flush emitted when the framed stream ends.
    fn on_halt(&self, state: Box<dyn Any + Send>) -> Vec<LlmEvent>;
}

/// `Protocol.jsonEvent` — decode an SSE data payload into a JSON value.
/// From reference/packages/llm/src/route/protocol.ts (`jsonEvent`)
pub fn decode_frame(route: &str, frame: FramePayload) -> Result<Value, LlmError> {
    match frame {
        FramePayload::Json(data) => shared::decode_json(&data).map_err(|_| {
            LlmError::event_error(route, format!("Invalid {} stream event", route), Some(data))
        }),
        FramePayload::Aws(value) => Ok(value),
    }
}

/// Stream adapter that runs the protocol state machine over a framed stream.
///
/// Mirrors `Route.streamPrepared` in client.ts: decode each frame, `step` the
/// parser, stop at `terminal`, flush `on_halt` when the stream ends, and map
/// any failure through `ProviderShared.eventError`.
pub struct ProtoStream {
    inner: Pin<Box<dyn Stream<Item = Result<FramePayload, LlmError>> + Send>>,
    protocol: Arc<dyn ProtocolStream>,
    route: String,
    state: Option<Box<dyn Any + Send>>,
    pending: VecDeque<Result<LlmEvent, LlmError>>,
    done: bool,
}

impl ProtoStream {
    pub fn new(
        inner: Pin<Box<dyn Stream<Item = Result<FramePayload, LlmError>> + Send>>,
        protocol: Arc<dyn ProtocolStream>,
        request: LlmRequest,
    ) -> ProtoStream {
        let route = format!("{}/{}", request.model.provider, request.model.route.id);
        let state = protocol.initial(&request);
        ProtoStream { inner, protocol, route, state: Some(state), pending: VecDeque::new(), done: false }
    }

    fn push_events(&mut self, events: Vec<LlmEvent>) {
        for event in events {
            self.pending.push_back(Ok(event));
        }
    }

    fn flush_halt(&mut self) {
        if let Some(state) = self.state.take() {
            let protocol = self.protocol.clone();
            let events = protocol.on_halt(state);
            self.push_events(events);
        }
        self.done = true;
    }
}

impl Stream for ProtoStream {
    type Item = Result<LlmEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(event) = self.pending.pop_front() {
            return Poll::Ready(Some(event));
        }
        if self.done {
            return Poll::Ready(None);
        }
        loop {
            let inner = &mut self.inner;
            match inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    let value = match decode_frame(&self.route, frame) {
                        Ok(value) => value,
                        Err(error) => {
                            self.done = true;
                            return Poll::Ready(Some(Err(error)));
                        }
                    };
                    let protocol = self.protocol.clone();
                    let state = self.state.take().unwrap();
                    match protocol.step(state, &value) {
                        Ok((next_state, events)) => {
                            self.state = Some(next_state);
                            let terminal = protocol.terminal(&value);
                            if terminal {
                                self.done = true;
                            }
                            if !events.is_empty() {
                                self.push_events(events);
                                return Poll::Ready(self.pending.pop_front());
                            }
                            if terminal {
                                return Poll::Ready(None);
                            }
                        }
                        Err(error) => {
                            self.done = true;
                            return Poll::Ready(Some(Err(error)));
                        }
                    }
                }
                Poll::Ready(Some(Err(error))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(None) => {
                    self.flush_halt();
                    return Poll::Ready(self.pending.pop_front());
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Convenience marker mirroring `Protocol.make`.
pub fn make(protocol: Arc<dyn ProtocolStream>) -> Arc<dyn ProtocolStream> {
    protocol
}
