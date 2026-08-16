//! Command handler. From reference/packages/server/src/handlers/command.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `command.list()` from `reference/packages/server/src/handlers/command.ts`.
pub async fn command_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let mut registry = oc_command::command::Registry::new(&location.directory);
    if let Ok(entries) =
        oc_command::command::load_from_dir(std::path::Path::new(&location.directory))
    {
        registry.add_config_entries(entries);
    }
    let config =
        crate::plugin_registry::merged_config(&state, state.stores.read().await.config.clone());
    if let Some(commands) = config.get("command") {
        let _ = registry.add_config_commands(commands);
    }
    crate::instance_handlers::add_mcp_prompt_commands(&state, &mut registry).await;
    let skill_settings = oc_command::skill::Settings {
        home: oc_command::global::Global::detect().home,
        directory: std::path::PathBuf::from(&location.directory),
        worktree: std::path::PathBuf::from(&location.directory),
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths: config
            .get("skills")
            .and_then(serde_json::Value::as_object)
            .and_then(|skills| skills.get("paths"))
            .and_then(serde_json::Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        pulled_dirs: Vec::new(),
        config_dirs: None,
    };
    if let Ok(service) = oc_command::skill::SkillService::load_with_environment(&skill_settings) {
        let skills = service
            .available(None)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        registry.add_skills(&skills);
    }
    let plugin_skills = crate::plugin_registry::plugin_skill_infos(&state);
    registry.add_skills(&plugin_skills);
    let data = registry
        .list()
        .filter_map(|command| serde_json::to_value(command).ok())
        .collect::<Vec<_>>();
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}
