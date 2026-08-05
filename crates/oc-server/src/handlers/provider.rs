//! Provider handler. From reference/packages/server/src/handlers/provider.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{request_location, HandlerResult};
use crate::errors::ApiError;
use std::collections::HashMap;

/// `catalog.provider.available()` from `reference/packages/server/src/handlers/provider.ts`.
/// TODO(integration): return oc-provider catalog.
pub async fn provider_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    super::json(&crate::schema::LocationResponse {
        location: location.info(),
        data: Vec::<serde_json::Value>::new(),
    })
}

/// `catalog.provider.get(...)` from `reference/packages/server/src/handlers/provider.ts`.
/// Returns 404 until providers are registered. TODO(integration).
pub async fn provider_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let _location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let provider_id = params
        .get("providerID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let message = format!("Provider not found: {provider_id}");
    Err(ApiError::ProviderNotFound {
        provider_id,
        message,
    })
}
