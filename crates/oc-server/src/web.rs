//! Embedded web client for the server root.
//!
//! The full OpenCode app is a separate frontend package. This small, dependency-free
//! client keeps the Rust distribution useful when that package is not available: it
//! probes server health, lists sessions, shows session messages, and submits prompts
//! through the existing HTTP API.

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;

pub const INDEX_HTML: &str = include_str!("../web/index.html");
pub const APP_JS: &str = include_str!("../web/app.js");
pub const APP_CSS: &str = include_str!("../web/app.css");

fn asset(content_type: &'static str, body: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .expect("embedded web asset response is valid")
}

/// GET `/` — the embedded browser entrypoint.
pub async fn index() -> Response {
    asset("text/html; charset=utf-8", INDEX_HTML)
}

/// GET `/assets/app.js` — the browser client used by [`index`].
pub async fn app_js() -> Response {
    asset("text/javascript; charset=utf-8", APP_JS)
}

/// GET `/assets/app.css` — the browser client stylesheet.
pub async fn app_css() -> Response {
    asset("text/css; charset=utf-8", APP_CSS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_client_references_live_api_routes() {
        assert!(INDEX_HTML.contains("/assets/app.js"));
        assert!(APP_JS.contains("/global/health"));
        assert!(APP_JS.contains("/session"));
        assert!(APP_JS.contains("/message"));
    }
}
