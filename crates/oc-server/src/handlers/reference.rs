//! Reference handler. From reference/packages/server/src/handlers/reference.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `reference.list()` from `reference/packages/server/src/handlers/reference.ts`.
/// TODO(integration): return oc-core project references.
pub async fn reference_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: Vec::<serde_json::Value>::new(),
    })
}
