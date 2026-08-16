//! Skill handler. From reference/packages/server/src/handlers/skill.ts.

use axum::extract::{Query, State};
use axum::http::HeaderMap;

use super::{json, request_location, HandlerResult};
use crate::schema::LocationResponse;
use std::collections::HashMap;

/// `skill.list()` from `reference/packages/server/src/handlers/skill.ts`.
pub async fn skill_list(
    State(state): State<crate::state::AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> HandlerResult {
    let location = request_location(&state, params.get("location").map(|_| ""), &headers);
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    let settings = oc_command::skill::Settings {
        home,
        directory: std::path::PathBuf::from(&location.directory),
        worktree: std::path::PathBuf::from(&location.directory),
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths: Vec::new(),
        pulled_dirs: Vec::new(),
        config_dirs: None,
    };
    let data = oc_command::skill::SkillService::load_with_environment(&settings)
        .map(|service| {
            service
                .all()
                .into_iter()
                .filter_map(|skill| serde_json::to_value(skill).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut data = data;
    data.extend(crate::plugin_registry::plugin_skill_values(&state));
    json(&LocationResponse {
        location: location.info(),
        data,
    })
}
