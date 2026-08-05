//! Remote sync transport.
//!
//! Client side of the remote control-plane HTTP surface. The reference uses
//! effect's `HttpClient` inside reference/packages/opencode/src/control-plane/workspace.ts;
//! this module mirrors the wire contract in
//! reference/packages/opencode/src/server/routes/instance/httpapi/groups/sync.ts
//! and the global event stream in `groups/global.ts`.
//!
//! The *server* side is oc-server scope; this crate only implements the client.

use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sync::event::SerializedEvent;

/// `SyncHttpError` / `WorkspaceSyncHttpError` from
/// reference/packages/opencode/src/control-plane/workspace.ts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SyncHttpError {
    pub message: String,
    pub status: u16,
    pub body: Option<String>,
}

impl SyncHttpError {
    pub fn new(message: impl Into<String>, status: u16, body: Option<String>) -> Self {
        Self {
            message: message.into(),
            status,
            body,
        }
    }
}

/// A decoded HTTP response.
#[derive(Debug, Clone)]
pub struct SyncHttpResponse {
    pub status: u16,
    pub text: Option<String>,
    pub json: Option<Value>,
}

/// `ReplayEvent` from reference/packages/opencode/src/server/routes/instance/httpapi/groups/sync.ts:
/// identical to `SerializedEvent`.
pub type ReplayEvent = SerializedEvent;

/// `ReplayPayload` from the reference: `{ directory, events }`.
#[derive(Debug, Clone, Serialize)]
pub struct ReplayPayload {
    pub directory: String,
    pub events: Vec<ReplayEvent>,
}

/// `ReplayResponse` from the reference: `{ sessionID }`.
#[derive(Debug, Clone, Deserialize)]
pub struct ReplayResponse {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// `SessionPayload` from the reference: `{ sessionID }`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionPayload {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// `HistoryPayload` from the reference: aggregate id -> last known seq.
pub type HistoryPayload = std::collections::HashMap<String, u64>;

/// One entry in the `/sync/history` response (see `HistoryEvent` in the
/// reference). `HistoryEvent` itself lives in `sync::event`.
///
/// A single request to the remote workspace, mirroring the `HttpClientRequest`
/// construction in the reference.
#[derive(Debug, Clone)]
pub struct SyncHttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
    /// How the response body should be interpreted.
    pub response: ResponseKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    Json,
    Text,
}

/// The remote sync surface the workspace runtime talks to.
#[async_trait::async_trait]
pub trait SyncApi: Send + Sync {
    /// Execute a request. Transport errors are returned as `SyncHttpError`.
    async fn execute(&self, request: SyncHttpRequest) -> Result<SyncHttpResponse, SyncHttpError>;
    /// Open the SSE stream for `GET /global/event`.
    async fn event_stream(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<Box<dyn tokio::io::AsyncBufRead + Send + Unpin>, SyncHttpError>;
}

/// `reqwest`-backed implementation of `SyncApi`.
#[derive(Debug, Clone)]
pub struct ReqwestSyncApi {
    client: reqwest::Client,
}

impl ReqwestSyncApi {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestSyncApi {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

fn as_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers.to_vec()
}

#[async_trait::async_trait]
impl SyncApi for ReqwestSyncApi {
    async fn execute(&self, request: SyncHttpRequest) -> Result<SyncHttpResponse, SyncHttpError> {
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in as_headers(&request.headers) {
            builder = builder.header(name, value);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| SyncHttpError::new(error.to_string(), 0, None))?;
        let status = response.status().as_u16();
        let text = response.text().await.ok();
        let json = text
            .as_deref()
            .and_then(|body| serde_json::from_str(body).ok());
        Ok(SyncHttpResponse { status, text, json })
    }

    async fn event_stream(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<Box<dyn tokio::io::AsyncBufRead + Send + Unpin>, SyncHttpError> {
        let mut builder = self.client.get(url).header("accept", "text/event-stream");
        for (name, value) in as_headers(headers) {
            builder = builder.header(name, value);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| SyncHttpError::new(error.to_string(), 0, None))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(SyncHttpError::new(
                format!("Workspace sync HTTP failure: {status}"),
                status,
                None,
            ));
        }
        let stream = response
            .bytes_stream()
            .map_err(|error| std::io::Error::other(error.to_string()));
        let reader = tokio_util::io::StreamReader::new(stream);
        Ok(Box::new(reader))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::event::EventID;

    #[test]
    fn replay_payload_serializes_with_camel_case_fields() {
        let payload = ReplayPayload {
            directory: "/tmp".into(),
            events: vec![SerializedEvent {
                id: EventID("evt_1".into()),
                r#type: "session.next.moved.1".into(),
                seq: 0,
                aggregate_id: "ses_1".into(),
                data: serde_json::json!({ "sessionID": "ses_1" }),
            }],
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            json,
            r#"{"directory":"/tmp","events":[{"id":"evt_1","type":"session.next.moved.1","seq":0,"aggregateID":"ses_1","data":{"sessionID":"ses_1"}}]}"#
        );
    }

    #[test]
    fn session_payload_serializes_with_camel_case() {
        let payload = SessionPayload {
            session_id: "ses_1".into(),
        };
        assert_eq!(
            serde_json::to_string(&payload).unwrap(),
            r#"{"sessionID":"ses_1"}"#
        );
    }

    #[test]
    fn replay_response_parses() {
        let response: ReplayResponse = serde_json::from_str(r#"{"sessionID":"ses_1"}"#).unwrap();
        assert_eq!(response.session_id, "ses_1");
    }

    #[test]
    fn history_payload_is_object_of_seq_numbers() {
        let mut state = HistoryPayload::new();
        state.insert("ses_1".into(), 3);
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, r#"{"ses_1":3}"#);
    }
}
