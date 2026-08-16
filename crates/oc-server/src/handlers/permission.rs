//! Permission handler. From reference/packages/server/src/handlers/permission.ts.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use super::{json, no_content, request_location, HandlerResult};
use crate::errors::{session_not_found, ApiError};
use crate::event::permission_id;
use crate::schema::{
    LocationResponse, PermissionCreateData, PermissionEffect, PermissionSavedData,
};
use crate::state::timestamp;
use oc_session::v1::{PermissionRule, Ruleset};
use std::collections::HashMap;

/// `PermissionV2.list()` from `reference/packages/server/src/handlers/permission.ts`.
pub async fn permission_request_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let stores = state.stores.read().await;
    let data = stores.permissions.values().cloned().collect::<Vec<_>>();
    drop(stores);
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}

/// `permission.ask(...)` from `reference/packages/server/src/handlers/permission.ts`.
pub async fn session_permission_create(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let (agent, config) = {
        let stores = state.stores.read().await;
        let session = stores
            .sessions
            .get(&session_id)
            .ok_or_else(|| session_not_found(&session_id))?;
        (
            session.info.agent.as_deref().unwrap_or("build").to_string(),
            stores.config.clone(),
        )
    };

    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(permission_id);
    let action = body
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("*");
    let resources = body
        .get("resources")
        .and_then(serde_json::Value::as_array)
        .map(|resources| {
            resources
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|resources| !resources.is_empty())
        .unwrap_or_else(|| vec!["*".to_string()]);
    let rules = configured_permission_rules(&config, &agent);
    let effect = permission_effect(action, &resources, &rules);

    if effect == "ask" {
        let mut request = body.0.clone();
        if let Some(object) = request.as_object_mut() {
            object.insert("id".into(), serde_json::Value::String(id.clone()));
            object.insert("sessionID".into(), serde_json::Value::String(session_id));
        }
        state
            .stores
            .write()
            .await
            .permissions
            .insert(id.clone(), request);
    }
    json(&PermissionCreateData {
        data: PermissionEffect {
            id,
            effect: effect.to_string(),
        },
    })
}

fn permission_effect(action: &str, resources: &[String], rules: &Ruleset) -> &'static str {
    let mut asked = false;
    for resource in resources {
        match oc_session::permission::evaluate(action, resource, &[rules])
            .action
            .as_str()
        {
            "deny" => return "deny",
            "ask" => asked = true,
            "allow" => {}
            _ => asked = true,
        }
    }
    if asked {
        "ask"
    } else {
        "allow"
    }
}

fn configured_permission_rules(config: &serde_json::Value, agent: &str) -> Ruleset {
    let mut rules = Vec::new();
    append_tool_permission_rules(config.get("tools"), &mut rules);
    append_permission_rules(config.get("permission"), &mut rules);
    if let Some(agent_config) = config
        .get("agent")
        .and_then(serde_json::Value::as_object)
        .and_then(|agents| agents.get(agent))
    {
        append_tool_permission_rules(agent_config.get("tools"), &mut rules);
        append_permission_rules(agent_config.get("permission"), &mut rules);
    }
    rules
}

fn append_permission_rules(value: Option<&serde_json::Value>, rules: &mut Ruleset) {
    let Some(value) = value else { return };
    match value {
        serde_json::Value::String(action) => rules.push(PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: action.clone(),
        }),
        serde_json::Value::Object(entries) => {
            for (permission, action) in entries {
                match action {
                    serde_json::Value::String(action) => rules.push(PermissionRule {
                        permission: permission.clone(),
                        pattern: "*".into(),
                        action: action.clone(),
                    }),
                    serde_json::Value::Object(patterns) => {
                        for (pattern, action) in patterns {
                            if let Some(action) = action.as_str() {
                                rules.push(PermissionRule {
                                    permission: permission.clone(),
                                    pattern: pattern.clone(),
                                    action: action.to_string(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn append_tool_permission_rules(value: Option<&serde_json::Value>, rules: &mut Ruleset) {
    let Some(serde_json::Value::Object(tools)) = value else {
        return;
    };
    for (tool, enabled) in tools {
        let Some(enabled) = enabled.as_bool() else {
            continue;
        };
        let permission = match tool.as_str() {
            "write" | "edit" | "patch" => "edit",
            other => other,
        };
        rules.push(PermissionRule {
            permission: permission.to_string(),
            pattern: "*".into(),
            action: if enabled { "allow" } else { "deny" }.into(),
        });
    }
}

pub async fn session_permission_list(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    if !stores.sessions.contains_key(&session_id) {
        return Err(session_not_found(&session_id));
    }
    let data = stores
        .permissions
        .values()
        .filter(|request| request_belongs_to_session(request, &session_id))
        .cloned()
        .collect::<Vec<_>>();
    drop(stores);
    json(&serde_json::json!({ "data": data }))
}

pub async fn session_permission_get(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let stores = state.stores.read().await;
    let request = stores.permissions.get(&request_id).cloned();
    drop(stores);
    let Some(request) = request.filter(|request| request_belongs_to_session(request, &session_id))
    else {
        return Err(missing_request(&request_id));
    };
    json(&serde_json::json!({ "data": request }))
}

pub async fn session_permission_reply(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
    body: axum::extract::Json<serde_json::Value>,
) -> HandlerResult {
    let session_id = params
        .get("sessionID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    let request_id = params
        .get("requestID")
        .cloned()
        .ok_or(ApiError::V1BadRequest)?;
    {
        let stores = state.stores.read().await;
        let belongs = stores
            .permissions
            .get(&request_id)
            .is_some_and(|request| request_belongs_to_session(request, &session_id));
        if !belongs {
            return Err(missing_request(&request_id));
        }
    }
    let reply = body
        .get("reply")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let resolved = state.resolve_permission(&request_id, &reply).await;
    let _ = timestamp();
    let mut stores = state.stores.write().await;
    if !resolved && stores.permissions.remove(&request_id).is_none() {
        return Err(missing_request(&request_id));
    }
    drop(stores);
    no_content()
}

pub async fn permission_saved_list(
    State(state): State<crate::state::AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> HandlerResult {
    let project_id = query.get("projectID").map(String::as_str);
    let stores = state.stores.read().await;
    let data = stores
        .saved_permissions
        .values()
        .filter(|saved| project_id.is_none_or(|project| saved.project_id == project))
        .cloned()
        .collect();
    drop(stores);
    json(&PermissionSavedData { data })
}

pub async fn permission_saved_remove(
    State(state): State<crate::state::AppState>,
    Path(params): Path<HashMap<String, String>>,
) -> HandlerResult {
    let id = params.get("id").cloned().ok_or(ApiError::V1BadRequest)?;
    state.stores.write().await.saved_permissions.remove(&id);
    state.delete_saved_permission(&id);
    no_content()
}

fn missing_request(id: &str) -> ApiError {
    ApiError::PermissionNotFound {
        request_id: id.to_string(),
        message: format!("Permission request not found: {id}"),
    }
}

fn request_belongs_to_session(request: &serde_json::Value, session_id: &str) -> bool {
    request.get("sessionID").and_then(serde_json::Value::as_str) == Some(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(value: &str) -> String {
        value.to_string()
    }

    #[test]
    fn configured_rules_preserve_allow_ask_and_deny() {
        let config = serde_json::json!({
            "permission": {
                "bash": { "rm *": "deny", "echo *": "allow" },
                "read": "allow"
            }
        });
        let rules = configured_permission_rules(&config, "build");

        assert_eq!(
            permission_effect("bash", &[resource("rm -rf build")], &rules),
            "deny"
        );
        assert_eq!(
            permission_effect("bash", &[resource("echo safe")], &rules),
            "allow"
        );
        assert_eq!(
            permission_effect("bash", &[resource("git status")], &rules),
            "ask"
        );
    }

    #[test]
    fn permission_requests_are_session_scoped() {
        let request = serde_json::json!({ "sessionID": "ses_a", "action": "bash" });
        assert!(request_belongs_to_session(&request, "ses_a"));
        assert!(!request_belongs_to_session(&request, "ses_b"));
        assert!(!request_belongs_to_session(&serde_json::json!({}), "ses_a"));
    }

    #[test]
    fn deny_wins_when_multiple_resources_are_requested() {
        let config = serde_json::json!({
            "permission": { "read": "allow", "bash": { "rm *": "deny" } }
        });
        let rules = configured_permission_rules(&config, "build");
        assert_eq!(
            permission_effect(
                "bash",
                &[resource("echo safe"), resource("rm -rf build")],
                &rules,
            ),
            "deny"
        );
    }

    #[test]
    fn agent_rules_are_appended_after_global_rules() {
        let config = serde_json::json!({
            "permission": { "bash": "deny" },
            "agent": { "build": { "permission": { "bash": "allow" } } }
        });
        let rules = configured_permission_rules(&config, "build");
        assert_eq!(
            permission_effect("bash", &[resource("echo safe")], &rules),
            "allow"
        );
    }

    #[test]
    fn permission_decision_keeps_response_wire_shape() {
        let value = serde_json::to_value(PermissionCreateData {
            data: PermissionEffect {
                id: "per_test".into(),
                effect: "ask".into(),
            },
        })
        .expect("permission response serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "data": { "id": "per_test", "effect": "ask" }
            })
        );
    }
}
