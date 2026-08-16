//! Integration handler. From reference/packages/server/src/handlers/integration.ts.
//!
//! Integration discovery and OAuth attempt handlers.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::ApiError;
use crate::schema::LocationResponse;
use crate::state::{now_millis, IntegrationAttempt, IntegrationAttemptStatus};
use oc_database::tables::CredentialRow;
use oc_database::Value as SqlValue;
use oc_provider::provider::auth::{AuthorizeInput, CallbackInput, CallbackMethod, MethodType};
use std::collections::{BTreeMap, HashMap};

const ATTEMPT_TTL_MS: i64 = 15 * 60 * 1000;

fn integration_values(state: &crate::state::AppState) -> Result<Vec<serde_json::Value>, ApiError> {
    let connections = state
        .database
        .as_ref()
        .map(|database| {
            database
                .list::<CredentialRow>("credential", &[])
                .map_err(database_error)
        })
        .transpose()?
        .unwrap_or_default();

    let catalog = oc_provider::models_dev::snapshot().map_err(|error| ApiError::Unknown {
        message: format!("provider catalog unavailable: {error}"),
        reference: None,
    })?;
    let mut values = catalog
        .into_values()
        .into_iter()
        .map(|provider| {
            let id = provider.id;
            let name = provider.name;
            let env = provider.env;
            let mut methods = vec![serde_json::json!({
                "type": "key",
                "label": "API key"
            })];
            if !env.is_empty() {
                methods.push(serde_json::json!({
                    "type": "env",
                    "names": env
                }));
            }
            let provider_connections = connections
                .iter()
                .filter(|connection| connection.integration_id.as_deref() == Some(&id))
                .map(|connection| {
                    serde_json::json!({
                        "type": "credential",
                        "id": connection.id,
                        "label": connection.label
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": id,
                "name": name,
                "methods": methods,
                "connections": provider_connections,
            })
        })
        .collect::<Vec<_>>();

    // Provider OAuth methods are supplied by plugin/host hooks rather than
    // models.dev. Include them in the same integration catalog and include
    // hook-only providers so the UI can discover them.
    for (provider_id, methods) in state.provider_auth.methods() {
        let oauth_methods = methods
            .into_iter()
            .enumerate()
            .filter(|(_, method)| method.r#type == MethodType::OAuth)
            .map(|(index, method)| {
                let mut value = serde_json::to_value(method).unwrap_or_default();
                if let serde_json::Value::Object(object) = &mut value {
                    object.insert(
                        "id".into(),
                        serde_json::Value::String(format!("oauth-{index}")),
                    );
                }
                value
            })
            .collect::<Vec<_>>();
        if oauth_methods.is_empty() {
            continue;
        }
        if let Some(provider) = values.iter_mut().find(|value| {
            value.get("id").and_then(serde_json::Value::as_str) == Some(provider_id.as_str())
        }) {
            if let Some(existing) = provider
                .get_mut("methods")
                .and_then(serde_json::Value::as_array_mut)
            {
                existing.extend(oauth_methods);
            }
        } else {
            values.push(serde_json::json!({
                "id": provider_id,
                "name": provider_id,
                "methods": oauth_methods,
                "connections": [],
            }));
        }
    }

    Ok(values)
}

fn oauth_method_index(
    state: &crate::state::AppState,
    provider_id: &str,
    method_id: &str,
) -> Option<usize> {
    let methods = state.provider_auth.methods();
    let methods = methods.get(provider_id)?;
    if let Some(index) = method_id
        .strip_prefix("oauth-")
        .or(Some(method_id))
        .and_then(|value| value.parse::<usize>().ok())
    {
        return methods
            .get(index)
            .filter(|method| method.r#type == MethodType::OAuth)
            .map(|_| index);
    }
    if method_id == "oauth" {
        return methods
            .iter()
            .enumerate()
            .find(|(_, method)| method.r#type == MethodType::OAuth)
            .map(|(index, _)| index);
    }
    methods
        .iter()
        .enumerate()
        .find(|(_, method)| method.r#type == MethodType::OAuth && method.label == method_id)
        .map(|(index, _)| index)
}

fn attempt_time(attempt: &IntegrationAttempt) -> serde_json::Value {
    serde_json::json!({
        "created": attempt.created,
        "expires": attempt.expires,
    })
}

fn attempt_status(attempt: &IntegrationAttempt, now: i64) -> serde_json::Value {
    let time = attempt_time(attempt);
    if matches!(attempt.status, IntegrationAttemptStatus::Pending) && now >= attempt.expires {
        return serde_json::json!({ "status": "expired", "time": time });
    }
    match &attempt.status {
        IntegrationAttemptStatus::Pending => {
            serde_json::json!({ "status": "pending", "time": time })
        }
        IntegrationAttemptStatus::Complete => {
            serde_json::json!({ "status": "complete", "time": time })
        }
        IntegrationAttemptStatus::Failed(message) => {
            serde_json::json!({ "status": "failed", "message": message, "time": time })
        }
    }
}

fn path_value(params: &HashMap<String, String>, key: &str) -> Result<String, ApiError> {
    params.get(key).cloned().ok_or(ApiError::V1BadRequest)
}

pub async fn integration_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: integration_values(&state)?,
    })
}

pub async fn integration_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    let integration_id = params
        .get("integrationID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let integration = integration_values(&state)?
        .into_iter()
        .find(|value| value.get("id").and_then(|value| value.as_str()) == Some(&integration_id));
    json(&LocationResponse {
        location: location.info(),
        data: integration.unwrap_or(serde_json::Value::Null),
    })
}

pub async fn integration_connect_key(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let integration_id = params
        .get("integrationID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    if !integration_values(&state)?
        .iter()
        .any(|value| value.get("id").and_then(|value| value.as_str()) == Some(&integration_id))
    {
        return Err(ApiError::ApiNotFound {
            message: format!("Integration not found: {integration_id}"),
        });
    }
    let key = body
        .get("key")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::InvalidRequest {
            message: "integration key is required".into(),
            kind: Some("credential".into()),
            field: Some("key".into()),
        })?;
    let database = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable {
            message: "integration credentials require the server database".into(),
            service: Some("database".into()),
        })?;
    let label = body
        .get("label")
        .and_then(|value| value.as_str())
        .unwrap_or("default")
        .to_string();
    let now = now_millis();
    let row = CredentialRow {
        id: oc_provider::credential::Id::create().0,
        integration_id: Some(integration_id.clone()),
        label,
        value: serde_json::to_string(&serde_json::json!({
            "type": "key",
            "key": key,
        }))?,
        connector_id: None,
        method_id: Some("key".into()),
        active: Some(1),
        time_created: now,
        time_updated: now,
    };
    database
        .delete_by(
            "credential",
            "integration_id",
            &SqlValue::Text(integration_id),
        )
        .map_err(database_error)?;
    database
        .insert("credential", &row, &[])
        .map_err(database_error)?;
    no_content()
}

pub async fn integration_connect_oauth(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let integration_id = path_value(&params, "integrationID")?;
    if !integration_values(&state)?
        .iter()
        .any(|value| value.get("id").and_then(serde_json::Value::as_str) == Some(&integration_id))
    {
        return Err(ApiError::ApiNotFound {
            message: format!("Integration not found: {integration_id}"),
        });
    }
    let method_id = body
        .get("methodID")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiError::InvalidRequest {
            message: "integration OAuth methodID is required".into(),
            kind: Some("integration".into()),
            field: Some("methodID".into()),
        })?;
    let method = oauth_method_index(&state, &integration_id, method_id).ok_or_else(|| {
        ApiError::InvalidRequest {
            message: format!("OAuth method not found: {method_id}"),
            kind: Some("integration".into()),
            field: Some("methodID".into()),
        }
    })?;
    let inputs = body
        .get("inputs")
        .and_then(serde_json::Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let authorization = state
        .provider_auth
        .authorize(
            &integration_id,
            &AuthorizeInput {
                method,
                inputs: Some(inputs),
            },
        )
        .map_err(|error| ApiError::ProviderAuth {
            provider_id: integration_id.clone(),
            message: error.to_string(),
        })?
        .ok_or_else(|| ApiError::InvalidRequest {
            message: format!("OAuth method not found: {method_id}"),
            kind: Some("integration".into()),
            field: Some("methodID".into()),
        })?;
    let created = now_millis();
    let attempt_id = oc_schema::integration::create_attempt_id();
    let attempt = IntegrationAttempt {
        provider_id: integration_id,
        method,
        attempt_id: attempt_id.clone(),
        url: authorization.url,
        instructions: authorization.instructions,
        mode: authorization.method,
        created,
        expires: created.saturating_add(ATTEMPT_TTL_MS),
        status: IntegrationAttemptStatus::Pending,
    };
    let response = serde_json::json!({
        "attemptID": attempt.attempt_id.clone(),
        "url": attempt.url.clone(),
        "instructions": attempt.instructions.clone(),
        "mode": match attempt.mode { CallbackMethod::Auto => "auto", CallbackMethod::Code => "code" },
        "time": attempt_time(&attempt),
    });
    state
        .integration_attempts
        .lock()
        .await
        .insert(attempt_id, attempt);
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: response,
    })
}

pub async fn integration_attempt_status(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let attempt_id = path_value(&params, "attemptID")?;
    let attempt = state
        .integration_attempts
        .lock()
        .await
        .get(&attempt_id)
        .cloned()
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Integration attempt not found: {attempt_id}"),
        })?;
    let location = request_location(&state, query.get("location").map(|_| ""), &headers);
    json(&LocationResponse {
        location: location.info(),
        data: attempt_status(&attempt, now_millis()),
    })
}

pub async fn integration_attempt_complete(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let attempt_id = path_value(&params, "attemptID")?;
    let attempt = state
        .integration_attempts
        .lock()
        .await
        .get(&attempt_id)
        .cloned()
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Integration attempt not found: {attempt_id}"),
        })?;
    if now_millis() >= attempt.expires {
        return Err(ApiError::InvalidRequest {
            message: "integration OAuth attempt expired".into(),
            kind: Some("integration".into()),
            field: Some("attemptID".into()),
        });
    }
    if !matches!(attempt.status, IntegrationAttemptStatus::Pending) {
        return Err(ApiError::Conflict {
            message: "integration OAuth attempt is no longer pending".into(),
            resource: Some(attempt_id),
        });
    }
    let input = CallbackInput {
        method: attempt.method,
        code: body
            .get("code")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    };
    let mut auth = oc_provider::auth::FileAuthStore::new(oc_mcp::auth::default_data_dir());
    state
        .provider_auth
        .callback(&attempt.provider_id, &input, &mut auth)
        .map_err(|error| ApiError::ProviderAuth {
            provider_id: attempt.provider_id.clone(),
            message: error.to_string(),
        })?;
    if let Some(current) = state.integration_attempts.lock().await.get_mut(&attempt_id) {
        current.status = IntegrationAttemptStatus::Complete;
    }
    state.emit_event(crate::event::Event {
        id: crate::event::event_id(),
        metadata: None,
        r#type: "integration.connection.updated".into(),
        durable: None,
        location: None,
        data: serde_json::json!({ "integrationID": attempt.provider_id }),
    });
    no_content()
}

pub async fn integration_attempt_cancel(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let attempt_id = path_value(&params, "attemptID")?;
    let attempt = state
        .integration_attempts
        .lock()
        .await
        .remove(&attempt_id)
        .ok_or_else(|| ApiError::ApiNotFound {
            message: format!("Integration attempt not found: {attempt_id}"),
        })?;
    state.provider_auth.cancel(&attempt.provider_id);
    no_content()
}

fn database_error(error: oc_database::Error) -> ApiError {
    tracing::error!(?error, "integration database operation failed");
    ApiError::Unknown {
        message: "integration persistence failed".into(),
        reference: None,
    }
}
