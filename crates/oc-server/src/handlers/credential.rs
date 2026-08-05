//! Credential handler. From reference/packages/server/src/handlers/credential.ts.

use axum::extract::{Path, State};

use super::{no_content, HandlerResult};
use crate::errors::ApiError;
use std::collections::HashMap;

/// `connection.update(...)` from `reference/packages/server/src/handlers/credential.ts`.
/// TODO(integration): store into oc-provider credential store.
pub async fn credential_update(
    State(_state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let _credential_id = params
        .get("credentialID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let _label = body
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    no_content()
}

/// `connection.remove(...)` from `reference/packages/server/src/handlers/credential.ts`.
pub async fn credential_remove(
    State(_state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let _credential_id = params
        .get("credentialID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    no_content()
}
