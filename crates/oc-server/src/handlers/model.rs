//! Model handler. From reference/packages/server/src/handlers/model.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `catalog.model.available()` from `reference/packages/server/src/handlers/model.ts`.
pub async fn model_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    let hooks = crate::plugin_registry::plugin_model_hooks(&state);
    let models = super::provider::provider_catalog_from_config_with_model_hooks(&config, &hooks)?
        .into_iter()
        .flat_map(|provider| provider.models.into_values())
        .collect::<Vec<_>>();
    json(&LocationResponse {
        location: location.info(),
        data: models,
    })
}
