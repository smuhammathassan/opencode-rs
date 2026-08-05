//! Shared test harness: a local axum HTTP server that records requests and
//! serves canned responses, plus small assertion helpers.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::Request;
use axum::response::Response as AxumResponse;
use axum::Router;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: axum::http::HeaderMap,
    pub body: String,
}

impl RecordedRequest {
    pub fn json_body(&self) -> Option<serde_json::Value> {
        serde_json::from_str(&self.body).ok()
    }
}

pub type Responder = Arc<dyn Fn(&RecordedRequest) -> AxumResponse + Send + Sync>;

type MockServerState = Arc<(Arc<Mutex<Vec<RecordedRequest>>>, Responder)>;

pub struct MockServer {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<RecordedRequest>>>,
    handle: tokio::task::JoinHandle<()>,
    _addr: SocketAddr,
}

impl MockServer {
    /// Start a server whose fallback handler records every request and defers
    /// the response to `responder`.
    pub async fn spawn(responder: Responder) -> Self {
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new((requests.clone(), responder));
        let app = Router::new().fallback(handler).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        MockServer {
            base_url: format!("http://{addr}"),
            requests,
            handle,
            _addr: addr,
        }
    }

    pub fn recorded(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn handler(
    state: axum::extract::State<MockServerState>,
    request: Request<Body>,
) -> AxumResponse {
    let method = request.method().to_string();
    let path = request.uri().to_string();
    let headers = request.headers().clone();
    let body = axum::body::to_bytes(request.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let recorded = RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    };
    let (requests, responder) = &*state.0;
    requests.lock().unwrap().push(recorded.clone());
    (responder)(&recorded)
}

/// Build an `application/json` response.
pub fn json_response(status: u16, value: &serde_json::Value) -> AxumResponse {
    let body = serde_json::to_string(value).expect("json body");
    AxumResponse::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("response")
}

/// Build a `text/event-stream` response.
pub fn sse_response(status: u16, body: &str) -> AxumResponse {
    AxumResponse::builder()
        .status(status)
        .header("content-type", "text/event-stream")
        .body(Body::from(body.to_string()))
        .expect("response")
}

/// Build a `204 No Content` response.
pub fn no_content() -> AxumResponse {
    AxumResponse::builder()
        .status(204)
        .body(Body::empty())
        .expect("response")
}

/// An error response with a `_tag`-discriminated body.
pub fn error_response(
    status: u16,
    tag: &str,
    fields: &[(&str, &serde_json::Value)],
) -> AxumResponse {
    let mut object = serde_json::Map::new();
    object.insert("_tag".into(), serde_json::Value::String(tag.to_string()));
    for (key, value) in fields {
        object.insert((*key).to_string(), (*value).clone());
    }
    json_response(status, &serde_json::Value::Object(object))
}

/// Assert a single recorded request matches the expected method, path, and body.
pub fn assert_request<'a>(
    requests: &'a [RecordedRequest],
    index: usize,
    method: &str,
    path: &str,
) -> &'a RecordedRequest {
    let recorded = &requests[index];
    assert_eq!(recorded.method, method, "method for request {index}");
    assert_eq!(recorded.path, path, "path for request {index}");
    recorded
}

/// Assert a recorded request has the given JSON body (after normalizing
/// whitespace via serde).
pub fn assert_body(recorded: &RecordedRequest, expected: &serde_json::Value) {
    let actual = recorded
        .json_body()
        .unwrap_or_else(|| panic!("body is not JSON: {}", recorded.body));
    assert_eq!(&actual, expected, "body for {}", recorded.path);
}

pub fn counter() -> std::sync::atomic::AtomicUsize {
    std::sync::atomic::AtomicUsize::new(0)
}
