//! Server-Sent Events decoding.
//! Mirrors the `sse` generator in `reference/packages/client/src/generated/client.ts`:
//! blocks are split on `\n\n`, `data:` lines are collected and joined, and each
//! payload is JSON-parsed. `\r`/`\r\n` line endings are normalized and a trailing
//! `\r` is kept across chunk boundaries. Buffers larger than 1 MiB fail with
//! `MalformedResponse`.

use crate::error::{ClientError, Error};
use crate::transport::{RawRequest, RequestDescriptor, RequestOptions, Transport};
use futures::Stream;
use serde::de::DeserializeOwned;
use serde_json::Value;

const MAX_BUFFER_BYTES: usize = 1_048_576;

/// Streaming SSE decoder over a `reqwest` response body.
pub(crate) struct SseDecoder {
    response: reqwest::Response,
    buffer: String,
    done: bool,
}

impl SseDecoder {
    pub(crate) fn new(response: reqwest::Response) -> Self {
        SseDecoder {
            response,
            buffer: String::new(),
            done: false,
        }
    }

    /// Decode the next SSE event payload, or `None` once the stream ends.
    pub(crate) async fn next_value(&mut self) -> Result<Option<Value>, ClientError> {
        loop {
            if let Some(block) = self.take_block() {
                let data = extract_data(&block);
                if !data.is_empty() {
                    return Ok(Some(parse_json(&data)?));
                }
                continue;
            }
            if self.done {
                if !self.buffer.is_empty() {
                    let block = std::mem::take(&mut self.buffer);
                    let data = extract_data(&block);
                    if !data.is_empty() {
                        return Ok(Some(parse_json(&data)?));
                    }
                }
                return Ok(None);
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => {
                    self.push_chunk(&chunk);
                    if self.buffer.len() > MAX_BUFFER_BYTES {
                        return Err(ClientError::MalformedResponse(None));
                    }
                }
                Ok(None) => self.done = true,
                Err(err) => return Err(ClientError::Transport(err)),
            }
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) {
        let text = String::from_utf8_lossy(chunk);
        let trailing_carriage_return = text.ends_with('\r');
        let mut normalized = if trailing_carriage_return {
            text[..text.len() - 1].to_string()
        } else {
            text.to_string()
        };
        normalized = normalized.replace("\r\n", "\n").replace('\r', "\n");
        if trailing_carriage_return {
            normalized.push('\r');
        }
        self.buffer.push_str(&normalized);
    }

    /// Take the next complete `\n\n`-delimited block from the buffer.
    fn take_block(&mut self) -> Option<String> {
        let boundary = self.buffer.find("\n\n")?;
        let block = self.buffer[..boundary].to_string();
        self.buffer.drain(..boundary + 2);
        Some(block)
    }
}

fn extract_data(block: &str) -> String {
    block
        .split('\n')
        .filter_map(|line| {
            line.strip_prefix("data:")
                .map(|data| data.trim_start().to_string())
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn parse_json(data: &str) -> Result<Value, ClientError> {
    serde_json::from_str(data).map_err(|err| ClientError::MalformedResponse(Some(err)))
}

#[allow(clippy::large_enum_variant)]
enum SsePhase {
    Pending {
        transport: Transport,
        desc: RequestDescriptor,
        options: Option<RequestOptions>,
    },
    Decoding(SseDecoder),
    Done,
}

#[allow(clippy::large_enum_variant)]
enum RawSsePhase {
    Pending {
        transport: Transport,
        request: RawRequest,
        options: Option<RequestOptions>,
    },
    Decoding(SseDecoder),
    Done,
}

/// Build a lazy SSE stream: the request is sent on the first poll, mirroring the
/// async-iterator `sse` generator in `reference/packages/client/src/generated/client.ts`.
pub(crate) fn sse_stream<T: DeserializeOwned + 'static>(
    transport: Transport,
    desc: RequestDescriptor,
    options: Option<RequestOptions>,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<T, Error>> + Send + 'static>> {
    Box::pin(futures::stream::unfold(
        SsePhase::Pending {
            transport,
            desc,
            options,
        },
        |phase| async move {
            let mut phase = phase;
            loop {
                match phase {
                    SsePhase::Pending {
                        transport,
                        desc,
                        options,
                    } => match transport.start_sse(&desc, options.as_ref()).await {
                        Ok(decoder) => phase = SsePhase::Decoding(decoder),
                        Err(err) => return Some((Err(err), SsePhase::Done)),
                    },
                    SsePhase::Decoding(mut decoder) => match decoder.next_value().await {
                        Ok(Some(value)) => match serde_json::from_value(value) {
                            Ok(item) => return Some((Ok(item), SsePhase::Decoding(decoder))),
                            Err(err) => {
                                return Some((
                                    Err(ClientError::MalformedResponse(Some(err)).into()),
                                    SsePhase::Done,
                                ))
                            }
                        },
                        Ok(None) => return None,
                        Err(err) => return Some((Err(err.into()), SsePhase::Done)),
                    },
                    SsePhase::Done => return None,
                }
            }
        },
    ))
}

/// Build a lazy generic SSE stream for the public plugin/forward-compatible
/// client surface.
pub(crate) fn sse_stream_raw<T: DeserializeOwned + 'static>(
    transport: Transport,
    request: RawRequest,
    options: Option<RequestOptions>,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<T, Error>> + Send + 'static>> {
    Box::pin(futures::stream::unfold(
        RawSsePhase::Pending {
            transport,
            request,
            options,
        },
        |phase| async move {
            let mut phase = phase;
            loop {
                match phase {
                    RawSsePhase::Pending {
                        transport,
                        request,
                        options,
                    } => match transport.start_raw_sse(&request, options.as_ref()).await {
                        Ok(decoder) => phase = RawSsePhase::Decoding(decoder),
                        Err(err) => return Some((Err(err), RawSsePhase::Done)),
                    },
                    RawSsePhase::Decoding(mut decoder) => match decoder.next_value().await {
                        Ok(Some(value)) => match serde_json::from_value(value) {
                            Ok(item) => return Some((Ok(item), RawSsePhase::Decoding(decoder))),
                            Err(err) => {
                                return Some((
                                    Err(ClientError::MalformedResponse(Some(err)).into()),
                                    RawSsePhase::Done,
                                ))
                            }
                        },
                        Ok(None) => return None,
                        Err(err) => return Some((Err(err.into()), RawSsePhase::Done)),
                    },
                    RawSsePhase::Done => return None,
                }
            }
        },
    ))
}
