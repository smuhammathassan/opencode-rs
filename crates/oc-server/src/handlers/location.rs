//! Location handler. From reference/packages/server/src/handlers/location.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use std::collections::HashMap;

pub async fn location_get(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    json(&location.info())
}
