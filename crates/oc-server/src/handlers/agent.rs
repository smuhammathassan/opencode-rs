//! Agent handler. From reference/packages/server/src/handlers/agent.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `agent.all()` from `reference/packages/server/src/handlers/agent.ts`.
pub async fn agent_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let mut data = vec![serde_json::to_value(oc_core::agent::AgentInfo::empty(
        oc_core::ids::AgentId::make("build"),
    ))
    .map_err(|_| crate::errors::ApiError::V1BadRequest)?];
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    if let Some(configured) = config.get("agent").and_then(serde_json::Value::as_object) {
        for (name, value) in configured {
            if name == "build" {
                continue;
            }
            let mut agent = oc_core::agent::AgentInfo::empty(oc_core::ids::AgentId::make(name));
            agent.description = value
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            agent.mode = value
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("primary")
                .to_string();
            agent.hidden = value
                .get("hidden")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if let Some(steps) = value.get("steps").and_then(serde_json::Value::as_u64) {
                agent.steps = Some(steps);
            }
            data.push(
                serde_json::to_value(agent).map_err(|_| crate::errors::ApiError::V1BadRequest)?,
            );
        }
    }
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}
