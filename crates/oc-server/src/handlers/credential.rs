//! Credential handler. From reference/packages/server/src/handlers/credential.ts.

use axum::extract::{Path, State};

use super::{no_content, HandlerResult};
use crate::errors::ApiError;
use oc_database::tables::CredentialRow;
use oc_database::Value as SqlValue;
use std::collections::HashMap;

/// `connection.update(...)` from `reference/packages/server/src/handlers/credential.ts`.
pub async fn credential_update(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let credential_id = params
        .get("credentialID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;

    let database = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable {
            message: "credential persistence requires the server database".into(),
            service: Some("database".into()),
        })?;
    let key = SqlValue::Text(credential_id.clone());
    let existing: Option<CredentialRow> = database
        .get_by("credential", "id", &key, &[])
        .map_err(database_error)?;
    if existing.is_none() {
        return Err(ApiError::ApiNotFound {
            message: format!("Credential not found: {credential_id}"),
        });
    }

    if let Some(label) = body.get("label").and_then(|value| value.as_str()) {
        database
            .update_by(
                "credential",
                "label",
                &SqlValue::Text(label.to_string()),
                "id",
                &key,
            )
            .map_err(database_error)?;
    }
    if let Some(value) = body.get("value") {
        let serialized =
            serde_json::to_string(value).map_err(|error| ApiError::InvalidRequest {
                message: format!("invalid credential value: {error}"),
                kind: Some("credential".into()),
                field: Some("value".into()),
            })?;
        database
            .update_by(
                "credential",
                "value",
                &SqlValue::Text(serialized),
                "id",
                &key,
            )
            .map_err(database_error)?;
    }
    no_content()
}

/// `connection.remove(...)` from `reference/packages/server/src/handlers/credential.ts`.
pub async fn credential_remove(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let credential_id = params
        .get("credentialID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let database = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable {
            message: "credential persistence requires the server database".into(),
            service: Some("database".into()),
        })?;
    let removed = database
        .delete_by("credential", "id", &SqlValue::Text(credential_id.clone()))
        .map_err(database_error)?;
    if removed == 0 {
        return Err(ApiError::ApiNotFound {
            message: format!("Credential not found: {credential_id}"),
        });
    }
    no_content()
}

fn database_error(error: oc_database::Error) -> ApiError {
    tracing::error!(?error, "credential database operation failed");
    ApiError::Unknown {
        message: "credential persistence failed".into(),
        reference: None,
    }
}
