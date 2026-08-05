//! Agent handler. From reference/packages/server/src/handlers/agent.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `agent.all()` from `reference/packages/server/src/handlers/agent.ts`. Agents are
/// registered by oc-tool/oc-agent; this returns an empty list until integration.
pub async fn agent_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    // TODO(integration): return oc-agent registered agents.
    json(&LocationResponse {
        location: location.info(),
        data: Vec::<serde_json::Value>::new(),
    })
}
