//! Request middleware. From reference/packages/server/src/middleware/.
//!
//! `authorization` mirrors `reference/packages/server/src/middleware/authorization.ts`;
//! `schema_error` mirrors `middleware/schema-error.ts`.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::{credentials_from_request, AUTH_TOKEN_QUERY, WWW_AUTHENTICATE};
use crate::errors::ApiError;
use crate::state::AppState;

/// True when the request is a ticketed PTY connect upgrade, which skips credential
/// checks. From reference/packages/protocol/src/groups/pty.ts (`hasPtyConnectTicketURL`).
fn has_pty_connect_ticket_url(path: &str, query: Option<&str>) -> bool {
    let is_connect = path.starts_with("/api/pty/") && path.ends_with("/connect");
    if !is_connect {
        return false;
    }
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    params.get("ticket").map_or(false, |t| !t.is_empty())
}

/// Basic-auth gate. Mirrors `authorizationLayer` from
/// reference/packages/server/src/middleware/authorization.ts. Requires the router state
/// via `axum::middleware::from_fn_with_state`.
pub async fn authorization(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let config = state.auth.clone();

    if !config.required() {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();
    let query = request.uri().query().map(|q| q.to_string());

    if has_pty_connect_ticket_url(&path, query.as_deref()) {
        return next.run(request).await;
    }

    let headers = request.headers().clone();
    let query_token = query.as_deref().and_then(|q| {
        url::form_urlencoded::parse(q.as_bytes())
            .into_owned()
            .find(|(k, _)| k == AUTH_TOKEN_QUERY)
            .map(|(_, v)| v)
    });
    let authorization = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    let credentials = credentials_from_request(query_token.as_deref(), authorization.as_deref());
    if config.authorized(&credentials) {
        return next.run(request).await;
    }

    let mut response = ApiError::Unauthorized {
        message: "Authentication required".into(),
    }
    .into_response();
    response
        .headers_mut()
        .insert("www-authenticate", WWW_AUTHENTICATE.parse().unwrap());
    response
}

/// Truncate a schema rejection reason like `reference/packages/server/src/middleware/
/// schema-error.ts`.
pub fn truncate_reason(reason: &str) -> String {
    const REASON_LIMIT: usize = 1024;
    if reason.len() <= REASON_LIMIT {
        reason.to_string()
    } else {
        format!(
            "{}... ({} more chars)",
            &reason[..REASON_LIMIT],
            reason.len() - REASON_LIMIT
        )
    }
}

/// Build an `InvalidRequestError` from a schema rejection.
pub fn schema_error(message: &str) -> ApiError {
    ApiError::InvalidRequest {
        message: message.to_string(),
        kind: None,
        field: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticketed_pty_connect_skips_auth() {
        assert!(has_pty_connect_ticket_url(
            "/api/pty/pty_1/connect",
            Some("ticket=abc")
        ));
        assert!(!has_pty_connect_ticket_url("/api/pty/pty_1/connect", None));
        assert!(!has_pty_connect_ticket_url(
            "/api/session/s",
            Some("ticket=abc")
        ));
    }

    #[test]
    fn truncates_long_reasons() {
        let long = "x".repeat(2000);
        let out = truncate_reason(&long);
        assert!(out.ends_with("more chars)"));
        assert_eq!(out.len() < 2000, true);
        assert_eq!(truncate_reason("short"), "short");
    }
}
