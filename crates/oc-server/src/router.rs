//! Axum router construction from the route table.
//!
//! The reference composes the same route tree with Effect's HttpApi; here every entry
//! from `crate::route::all_routes()` maps to an axum handler. Handler logic mirrors the
//! matching `reference/packages/server/src/handlers/*` (v2) and
//! `reference/packages/opencode/src/server/routes/instance/httpapi/handlers/*` (v1).

use std::time::Duration;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;

use crate::cors::is_allowed_cors_origin;
use crate::state::AppState;

/// Build the full router. Layer order mirrors
/// reference/packages/opencode/src/server/routes/instance/httpapi/server.ts: CORS and
/// authorization are global, the UI fallback comes last.
pub fn build(state: AppState) -> Router {
    let cors = state.cors.clone();
    let cors_layer = tower_http::cors::CorsLayer::new()
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
        .allow_origin(tower_http::cors::AllowOrigin::predicate(
            move |origin, _parts| is_allowed_cors_origin(origin.to_str().ok(), Some(&cors)),
        ))
        .max_age(Duration::from_secs(86_400));

    let fallback_state = state.clone();
    let router = wire_v2(wire_v1(Router::new()))
        .route("/", get(crate::web::index))
        .route("/index.html", get(crate::web::index))
        .route("/assets/app.js", get(crate::web::app_js))
        .route("/assets/app.css", get(crate::web::app_css))
        .route("/doc", get(crate::instance_handlers::openapi_doc))
        .route("/openapi.json", get(crate::instance_handlers::openapi_json))
        .fallback(move |request: Request| {
            let state = fallback_state.clone();
            async move { ui_fallback(request, state).await }
        });

    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::authorization,
        ))
        .layer(cors_layer)
        .with_state(state)
}

/// Catch-all UI fallback. From reference/.../httpapi/server.ts (`uiRoute`):
/// serves the embedded web UI or proxies `app.opencode.ai`.
async fn ui_fallback(request: Request, _state: AppState) -> Response {
    let path = request.uri().path().to_string();
    if path == "/site.webmanifest" || path.ends_with(".png") {
        return StatusCode::NOT_FOUND.into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

/// v1 instance + global + root routes.
fn wire_v1(app: Router<AppState>) -> Router<AppState> {
    use crate::instance_handlers as h;

    app
        // control.ts
        .route(
            "/auth/:providerID",
            put(h::control_auth_set).delete(h::control_auth_remove),
        )
        .route("/log", post(h::control_log))
        // control-plane.ts
        .route(
            "/experimental/control-plane/move-session",
            post(h::control_plane_move_session),
        )
        // global.ts
        .route("/global/health", get(h::global_health))
        .route("/global/event", get(h::global_event))
        .route(
            "/global/config",
            get(h::global_config_get).patch(h::global_config_update),
        )
        .route("/global/dispose", post(h::global_dispose))
        .route("/global/upgrade", post(h::global_upgrade))
        // event.ts
        .route("/event", get(h::event_subscribe))
        // pty.ts
        .route("/pty/shells", get(h::pty_shells))
        .route(
            "/pty",
            get(crate::handlers::pty::pty_list).post(crate::handlers::pty::pty_create),
        )
        .route(
            "/pty/:ptyID",
            get(crate::handlers::pty::pty_get)
                .put(crate::handlers::pty::pty_update)
                .delete(crate::handlers::pty::pty_remove),
        )
        .route(
            "/pty/:ptyID/connect-token",
            post(crate::handlers::pty::pty_connect_token),
        )
        .route(
            "/pty/:ptyID/connect",
            get(crate::handlers::pty::pty_connect),
        )
        // config.ts
        .route("/config", get(h::config_get).patch(h::config_update))
        .route("/config/providers", get(h::config_providers))
        // experimental.ts
        .route(
            "/experimental/capabilities",
            get(h::experimental_capabilities),
        )
        .route("/experimental/console", get(h::experimental_console))
        .route(
            "/experimental/console/orgs",
            get(h::experimental_console_orgs),
        )
        .route(
            "/experimental/console/switch",
            post(h::experimental_console_switch),
        )
        .route("/experimental/tool", get(h::experimental_tool))
        .route("/experimental/tool/ids", get(h::experimental_tool_ids))
        .route(
            "/experimental/worktree",
            get(h::experimental_worktree_list)
                .post(h::experimental_worktree_create)
                .delete(h::experimental_worktree_remove),
        )
        .route(
            "/experimental/worktree/reset",
            post(h::experimental_worktree_reset),
        )
        .route("/experimental/session", get(h::experimental_session_list))
        .route(
            "/experimental/session/background",
            get(h::experimental_session_background_list),
        )
        .route(
            "/experimental/session/:sessionID/background",
            get(h::experimental_session_background_status)
                .post(h::experimental_session_background)
                .delete(h::experimental_session_background_cancel),
        )
        .route("/experimental/resource", get(h::experimental_resource))
        // file.ts
        .route("/find", get(h::find_text))
        .route("/find/file", get(h::find_file))
        .route("/find/symbol", get(h::find_symbol))
        .route("/file", get(h::file_list))
        .route("/file/content", get(h::file_content))
        .route("/file/status", get(h::file_status))
        // instance.ts
        .route("/instance/dispose", post(h::instance_dispose))
        .route("/path", get(h::instance_path))
        .route("/vcs", get(h::vcs_get))
        .route("/vcs/status", get(h::vcs_status))
        .route("/vcs/diff", get(h::vcs_diff))
        .route("/vcs/diff/raw", get(h::vcs_diff_raw))
        .route("/vcs/apply", post(h::vcs_apply))
        .route("/command", get(h::command_list))
        .route("/agent", get(h::agent_list))
        .route("/skill", get(h::skill_list))
        .route("/lsp", get(h::lsp_status))
        .route("/formatter", get(h::formatter_status))
        // mcp.ts
        .route("/mcp", get(h::mcp_status).post(h::mcp_add))
        .route(
            "/mcp/:name/auth",
            post(h::mcp_auth_start).delete(h::mcp_auth_remove),
        )
        .route("/mcp/:name/auth/callback", post(h::mcp_auth_callback))
        .route(
            "/mcp/:name/auth/authenticate",
            post(h::mcp_auth_authenticate),
        )
        .route("/mcp/:name/connect", post(h::mcp_connect))
        .route("/mcp/:name/disconnect", post(h::mcp_disconnect))
        // project.ts
        .route("/project", get(h::project_list))
        .route("/project/current", get(h::project_current))
        .route("/project/git/init", post(h::project_git_init))
        .route("/project/:projectID", patch(h::project_update))
        .route(
            "/project/:projectID/directories",
            get(h::project_directories),
        )
        // project-copy.ts
        .route(
            "/experimental/project/:projectID/copy/generate-name",
            post(h::project_copy_generate_name),
        )
        // permission.ts
        .route("/permission", get(h::permission_list))
        .route("/permission/:requestID/reply", post(h::permission_reply))
        // provider.ts
        .route("/provider", get(h::provider_list))
        .route("/provider/auth", get(h::provider_auth))
        .route(
            "/provider/:providerID/oauth/authorize",
            post(h::provider_oauth_authorize),
        )
        .route(
            "/provider/:providerID/oauth/callback",
            post(h::provider_oauth_callback),
        )
        // question.ts
        .route("/question", get(h::question_list))
        .route("/question/:requestID/reply", post(h::question_reply))
        .route("/question/:requestID/reject", post(h::question_reject))
        // session.ts
        .route("/session", get(h::session_list).post(h::session_create))
        .route("/session/status", get(h::session_status))
        .route(
            "/session/:sessionID",
            get(h::session_get)
                .patch(h::session_update)
                .delete(h::session_delete),
        )
        .route("/session/:sessionID/children", get(h::session_children))
        .route("/session/:sessionID/todo", get(h::session_todo))
        .route("/session/:sessionID/diff", get(h::session_diff))
        .route(
            "/session/:sessionID/message",
            get(h::session_messages).post(h::session_prompt),
        )
        .route(
            "/session/:sessionID/message/:messageID",
            get(h::session_message).delete(h::session_delete_message),
        )
        .route(
            "/session/:sessionID/message/:messageID/part/:partID",
            delete(h::session_delete_part).patch(h::session_update_part),
        )
        .route("/session/:sessionID/fork", post(h::session_fork))
        .route("/session/:sessionID/abort", post(h::session_abort))
        .route(
            "/session/:sessionID/share",
            post(h::session_share).delete(h::session_unshare),
        )
        .route("/session/:sessionID/init", post(h::session_init))
        .route("/session/:sessionID/compact", post(h::session_compact))
        .route("/session/:sessionID/summarize", post(h::session_summarize))
        .route(
            "/session/:sessionID/prompt_async",
            post(h::session_prompt_async),
        )
        .route("/session/:sessionID/command", post(h::session_command))
        .route("/session/:sessionID/shell", post(h::session_shell))
        .route("/session/:sessionID/revert", post(h::session_revert))
        .route("/session/:sessionID/unrevert", post(h::session_unrevert))
        .route(
            "/session/:sessionID/permissions/:permissionID",
            post(h::session_permission_respond),
        )
        // sync.ts
        .route("/sync/start", post(h::sync_start))
        .route("/sync/replay", post(h::sync_replay))
        .route("/sync/steal", post(h::sync_steal))
        .route("/sync/history", post(h::sync_history))
        // tui.ts
        .route("/tui/append-prompt", post(h::tui_append_prompt))
        .route("/tui/open-help", post(h::tui_open_help))
        .route("/tui/open-sessions", post(h::tui_open_sessions))
        .route("/tui/open-themes", post(h::tui_open_themes))
        .route("/tui/open-models", post(h::tui_open_models))
        .route("/tui/submit-prompt", post(h::tui_submit_prompt))
        .route("/tui/clear-prompt", post(h::tui_clear_prompt))
        .route("/tui/execute-command", post(h::tui_execute_command))
        .route("/tui/show-toast", post(h::tui_show_toast))
        .route("/tui/publish", post(h::tui_publish))
        .route("/tui/select-session", post(h::tui_select_session))
        .route("/tui/control/next", get(h::tui_control_next))
        .route("/tui/control/response", post(h::tui_control_response))
        // workspace.ts
        .route(
            "/experimental/workspace/adapter",
            get(h::workspace_adapters),
        )
        .route(
            "/experimental/workspace",
            get(h::workspace_list).post(h::workspace_create),
        )
        .route(
            "/experimental/workspace/sync-list",
            post(h::workspace_sync_list),
        )
        .route("/experimental/workspace/status", get(h::workspace_status))
        .route("/experimental/workspace/:id", delete(h::workspace_remove))
        .route("/experimental/workspace/warp", post(h::workspace_warp))
}

/// v2 `/api` surface routes.
fn wire_v2(app: Router<AppState>) -> Router<AppState> {
    use crate::handlers as h;

    app
        // health.ts
        .route("/api/health", get(h::health::health_get))
        // location.ts
        .route("/api/location", get(h::location::location_get))
        // agent.ts
        .route("/api/agent", get(h::agent::agent_list))
        // session.ts
        .route(
            "/api/session",
            get(h::session::session_list).post(h::session::session_create),
        )
        .route("/api/session/active", get(h::session::session_active))
        .route("/api/session/:sessionID", get(h::session::session_get))
        .route(
            "/api/session/:sessionID/agent",
            post(h::session::session_switch_agent),
        )
        .route(
            "/api/session/:sessionID/model",
            post(h::session::session_switch_model),
        )
        .route(
            "/api/session/:sessionID/fork",
            post(h::session::session_fork),
        )
        .route(
            "/api/session/:sessionID/prompt",
            post(h::session::session_prompt),
        )
        .route(
            "/api/session/:sessionID/compact",
            post(h::session::session_compact),
        )
        .route(
            "/api/session/:sessionID/wait",
            post(h::session::session_wait),
        )
        .route(
            "/api/session/:sessionID/revert/stage",
            post(h::session::session_revert_stage),
        )
        .route(
            "/api/session/:sessionID/revert/clear",
            post(h::session::session_revert_clear),
        )
        .route(
            "/api/session/:sessionID/revert/commit",
            post(h::session::session_revert_commit),
        )
        .route(
            "/api/session/:sessionID/context",
            get(h::session::session_context),
        )
        .route(
            "/api/session/:sessionID/history",
            get(h::session::session_history),
        )
        .route(
            "/api/session/:sessionID/event",
            get(h::session::session_events),
        )
        .route(
            "/api/session/:sessionID/interrupt",
            post(h::session::session_interrupt),
        )
        .route(
            "/api/session/:sessionID/message",
            get(h::message::session_messages),
        )
        .route(
            "/api/session/:sessionID/message/:messageID",
            get(h::session::session_message),
        )
        // model.ts
        .route("/api/model", get(h::model::model_list))
        // provider.ts
        .route("/api/provider", get(h::provider::provider_list))
        .route("/api/provider/:providerID", get(h::provider::provider_get))
        // integration.ts
        .route("/api/integration", get(h::integration::integration_list))
        .route(
            "/api/integration/:integrationID",
            get(h::integration::integration_get),
        )
        .route(
            "/api/integration/:integrationID/connect/key",
            post(h::integration::integration_connect_key),
        )
        .route(
            "/api/integration/:integrationID/connect/oauth",
            post(h::integration::integration_connect_oauth),
        )
        .route(
            "/api/integration/attempt/:attemptID",
            get(h::integration::integration_attempt_status)
                .delete(h::integration::integration_attempt_cancel),
        )
        .route(
            "/api/integration/attempt/:attemptID/complete",
            post(h::integration::integration_attempt_complete),
        )
        // credential.ts
        .route(
            "/api/credential/:credentialID",
            patch(h::credential::credential_update).delete(h::credential::credential_remove),
        )
        // permission.ts
        .route(
            "/api/permission/request",
            get(h::permission::permission_request_list),
        )
        .route(
            "/api/permission/saved",
            get(h::permission::permission_saved_list),
        )
        .route(
            "/api/permission/saved/:id",
            delete(h::permission::permission_saved_remove),
        )
        .route(
            "/api/session/:sessionID/permission",
            post(h::permission::session_permission_create)
                .get(h::permission::session_permission_list),
        )
        .route(
            "/api/session/:sessionID/permission/:requestID",
            get(h::permission::session_permission_get),
        )
        .route(
            "/api/session/:sessionID/permission/:requestID/reply",
            post(h::permission::session_permission_reply),
        )
        // fs.ts
        .route("/api/fs/read/*rest", get(h::fs::fs_read))
        .route("/api/fs/list", get(h::fs::fs_list))
        .route("/api/fs/find", get(h::fs::fs_find))
        // command.ts
        .route("/api/command", get(h::command::command_list))
        // skill.ts
        .route("/api/skill", get(h::skill::skill_list))
        // event.ts
        .route("/api/event", get(h::event::event_subscribe))
        // pty.ts
        .route("/api/pty", get(h::pty::pty_list).post(h::pty::pty_create))
        .route(
            "/api/pty/:ptyID",
            get(h::pty::pty_get)
                .put(h::pty::pty_update)
                .delete(h::pty::pty_remove),
        )
        .route(
            "/api/pty/:ptyID/connect-token",
            post(h::pty::pty_connect_token),
        )
        .route("/api/pty/:ptyID/connect", get(h::pty::pty_connect))
        // question.ts
        .route(
            "/api/question/request",
            get(h::question::question_request_list),
        )
        .route(
            "/api/session/:sessionID/question",
            get(h::question::session_question_list),
        )
        .route(
            "/api/session/:sessionID/question/:requestID/reply",
            post(h::question::session_question_reply),
        )
        .route(
            "/api/session/:sessionID/question/:requestID/reject",
            post(h::question::session_question_reject),
        )
        // reference.ts
        .route("/api/reference", get(h::reference::reference_list))
        // project-copy.ts
        .route(
            "/experimental/project/:projectID/copy",
            post(h::project_copy::project_copy_create).delete(h::project_copy::project_copy_remove),
        )
        .route(
            "/experimental/project/:projectID/copy/refresh",
            post(h::project_copy::project_copy_refresh),
        )
}
