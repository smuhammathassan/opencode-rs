//! The HTTP route table.
//!
//! Mirrors the reference route tree assembled in
//! `reference/packages/opencode/src/server/routes/instance/httpapi/server.ts` from:
//! - the v2 `/api` surface built by `makeApi`/`makeDefaultApi`
//!   (`reference/packages/protocol/src/api.ts` + `groups/*`)
//! - the v1 instance surface (`reference/packages/opencode/src/server/routes/instance/
//!   httpapi/groups/*`)
//! - the global root routes (`groups/global.ts`)
//! - the raw `/event` SSE route, PTY WebSocket connect, `/doc` and the UI fallback.
//!
//! Paths are kept verbatim (Effect-style `:param` segments) so the golden test in
//! `tests/route_table.rs` can compare against the reference source directly.

/// HTTP methods used by the reference surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// One endpoint. `id` is the OpenAPI operation identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub method: Method,
    pub path: &'static str,
    pub id: &'static str,
}

const fn route(method: Method, path: &'static str, id: &'static str) -> Route {
    Route { method, path, id }
}

pub const GET: Method = Method::Get;
pub const POST: Method = Method::Post;
pub const PUT: Method = Method::Put;
pub const PATCH: Method = Method::Patch;
pub const DELETE: Method = Method::Delete;

/// v2 `/api` surface. From reference/packages/protocol/src/groups/.
pub const V2_ROUTES: &[Route] = &[
    // health.ts
    route(GET, "/api/health", "v2.health.get"),
    // location.ts
    route(GET, "/api/location", "v2.location.get"),
    // agent.ts
    route(GET, "/api/agent", "v2.agent.list"),
    // session.ts
    route(GET, "/api/session", "v2.session.list"),
    route(POST, "/api/session", "v2.session.create"),
    route(GET, "/api/session/active", "v2.session.active"),
    route(GET, "/api/session/:sessionID", "v2.session.get"),
    route(
        POST,
        "/api/session/:sessionID/agent",
        "v2.session.switchAgent",
    ),
    route(
        POST,
        "/api/session/:sessionID/model",
        "v2.session.switchModel",
    ),
    route(POST, "/api/session/:sessionID/prompt", "v2.session.prompt"),
    route(
        POST,
        "/api/session/:sessionID/compact",
        "v2.session.compact",
    ),
    route(POST, "/api/session/:sessionID/wait", "v2.session.wait"),
    route(
        POST,
        "/api/session/:sessionID/revert/stage",
        "v2.session.revert.stage",
    ),
    route(
        POST,
        "/api/session/:sessionID/revert/clear",
        "v2.session.revert.clear",
    ),
    route(
        POST,
        "/api/session/:sessionID/revert/commit",
        "v2.session.revert.commit",
    ),
    route(GET, "/api/session/:sessionID/context", "v2.session.context"),
    route(GET, "/api/session/:sessionID/history", "v2.session.history"),
    route(GET, "/api/session/:sessionID/event", "v2.session.events"),
    route(
        POST,
        "/api/session/:sessionID/interrupt",
        "v2.session.interrupt",
    ),
    route(
        GET,
        "/api/session/:sessionID/message/:messageID",
        "v2.session.message",
    ),
    // message.ts
    route(
        GET,
        "/api/session/:sessionID/message",
        "v2.session.messages",
    ),
    // model.ts
    route(GET, "/api/model", "v2.model.list"),
    // provider.ts
    route(GET, "/api/provider", "v2.provider.list"),
    route(GET, "/api/provider/:providerID", "v2.provider.get"),
    // integration.ts
    route(GET, "/api/integration", "v2.integration.list"),
    route(GET, "/api/integration/:integrationID", "v2.integration.get"),
    route(
        POST,
        "/api/integration/:integrationID/connect/key",
        "v2.integration.connect.key",
    ),
    route(
        POST,
        "/api/integration/:integrationID/connect/oauth",
        "v2.integration.connect.oauth",
    ),
    route(
        GET,
        "/api/integration/attempt/:attemptID",
        "v2.integration.attempt.status",
    ),
    route(
        POST,
        "/api/integration/attempt/:attemptID/complete",
        "v2.integration.attempt.complete",
    ),
    route(
        DELETE,
        "/api/integration/attempt/:attemptID",
        "v2.integration.attempt.cancel",
    ),
    // credential.ts
    route(
        PATCH,
        "/api/credential/:credentialID",
        "v2.credential.update",
    ),
    route(
        DELETE,
        "/api/credential/:credentialID",
        "v2.credential.remove",
    ),
    // permission.ts
    route(GET, "/api/permission/request", "v2.permission.request.list"),
    route(GET, "/api/permission/saved", "v2.permission.saved.list"),
    route(
        DELETE,
        "/api/permission/saved/:id",
        "v2.permission.saved.remove",
    ),
    route(
        POST,
        "/api/session/:sessionID/permission",
        "v2.session.permission.create",
    ),
    route(
        GET,
        "/api/session/:sessionID/permission",
        "v2.session.permission.list",
    ),
    route(
        GET,
        "/api/session/:sessionID/permission/:requestID",
        "v2.session.permission.get",
    ),
    route(
        POST,
        "/api/session/:sessionID/permission/:requestID/reply",
        "v2.session.permission.reply",
    ),
    // fs.ts
    route(GET, "/api/fs/read/*", "v2.fs.read"),
    route(GET, "/api/fs/list", "v2.fs.list"),
    route(GET, "/api/fs/find", "v2.fs.find"),
    // command.ts
    route(GET, "/api/command", "v2.command.list"),
    // skill.ts
    route(GET, "/api/skill", "v2.skill.list"),
    // event.ts
    route(GET, "/api/event", "v2.event.subscribe"),
    // pty.ts
    route(GET, "/api/pty", "v2.pty.list"),
    route(POST, "/api/pty", "v2.pty.create"),
    route(GET, "/api/pty/:ptyID", "v2.pty.get"),
    route(PUT, "/api/pty/:ptyID", "v2.pty.update"),
    route(DELETE, "/api/pty/:ptyID", "v2.pty.remove"),
    route(POST, "/api/pty/:ptyID/connect-token", "v2.pty.connectToken"),
    route(GET, "/api/pty/:ptyID/connect", "v2.pty.connect"),
    // question.ts
    route(GET, "/api/question/request", "v2.question.request.list"),
    route(
        GET,
        "/api/session/:sessionID/question",
        "v2.session.question.list",
    ),
    route(
        POST,
        "/api/session/:sessionID/question/:requestID/reply",
        "v2.session.question.reply",
    ),
    route(
        POST,
        "/api/session/:sessionID/question/:requestID/reject",
        "v2.session.question.reject",
    ),
    // reference.ts
    route(GET, "/api/reference", "v2.reference.list"),
    // project-copy.ts
    route(
        POST,
        "/experimental/project/:projectID/copy",
        "v2.projectCopy.create",
    ),
    route(
        DELETE,
        "/experimental/project/:projectID/copy",
        "v2.projectCopy.remove",
    ),
    route(
        POST,
        "/experimental/project/:projectID/copy/refresh",
        "v2.projectCopy.refresh",
    ),
];

/// v1 config routes. From reference/packages/opencode/src/server/routes/instance/httpapi/
/// groups/config.ts.
pub const CONFIG_ROUTES: &[Route] = &[
    route(GET, "/config", "config.get"),
    route(PATCH, "/config", "config.update"),
    route(GET, "/config/providers", "config.providers"),
];

/// v1 instance routes. From groups/instance.ts.
pub const INSTANCE_ROUTES: &[Route] = &[
    route(POST, "/instance/dispose", "instance.dispose"),
    route(GET, "/path", "path.get"),
    route(GET, "/vcs", "vcs.get"),
    route(GET, "/vcs/status", "vcs.status"),
    route(GET, "/vcs/diff", "vcs.diff"),
    route(GET, "/vcs/diff/raw", "vcs.diff.raw"),
    route(POST, "/vcs/apply", "vcs.apply"),
    route(GET, "/command", "command.list"),
    route(GET, "/agent", "app.agents"),
    route(GET, "/skill", "app.skills"),
    route(GET, "/lsp", "lsp.status"),
    route(GET, "/formatter", "formatter.status"),
];

/// v1 session routes. From groups/session.ts.
pub const SESSION_ROUTES: &[Route] = &[
    route(GET, "/session", "session.list"),
    route(GET, "/session/status", "session.status"),
    route(GET, "/session/:sessionID", "session.get"),
    route(GET, "/session/:sessionID/children", "session.children"),
    route(GET, "/session/:sessionID/todo", "session.todo"),
    route(GET, "/session/:sessionID/diff", "session.diff"),
    route(GET, "/session/:sessionID/message", "session.messages"),
    route(
        GET,
        "/session/:sessionID/message/:messageID",
        "session.message",
    ),
    route(POST, "/session", "session.create"),
    route(DELETE, "/session/:sessionID", "session.delete"),
    route(PATCH, "/session/:sessionID", "session.update"),
    route(POST, "/session/:sessionID/fork", "session.fork"),
    route(POST, "/session/:sessionID/abort", "session.abort"),
    route(POST, "/session/:sessionID/share", "session.share"),
    route(DELETE, "/session/:sessionID/share", "session.unshare"),
    route(POST, "/session/:sessionID/init", "session.init"),
    route(POST, "/session/:sessionID/summarize", "session.summarize"),
    route(POST, "/session/:sessionID/message", "session.prompt"),
    route(
        POST,
        "/session/:sessionID/prompt_async",
        "session.prompt_async",
    ),
    route(POST, "/session/:sessionID/command", "session.command"),
    route(POST, "/session/:sessionID/shell", "session.shell"),
    route(POST, "/session/:sessionID/revert", "session.revert"),
    route(POST, "/session/:sessionID/unrevert", "session.unrevert"),
    route(
        POST,
        "/session/:sessionID/permissions/:permissionID",
        "permission.respond",
    ),
    route(
        DELETE,
        "/session/:sessionID/message/:messageID",
        "session.deleteMessage",
    ),
    route(
        DELETE,
        "/session/:sessionID/message/:messageID/part/:partID",
        "part.delete",
    ),
    route(
        PATCH,
        "/session/:sessionID/message/:messageID/part/:partID",
        "part.update",
    ),
];

/// v1 instance event SSE route. From groups/event.ts.
pub const EVENT_ROUTES: &[Route] = &[route(GET, "/event", "event.subscribe")];

/// v1 PTY routes. From groups/pty.ts.
pub const PTY_ROUTES: &[Route] = &[
    route(GET, "/pty/shells", "pty.shells"),
    route(GET, "/pty", "pty.list"),
    route(POST, "/pty", "pty.create"),
    route(GET, "/pty/:ptyID", "pty.get"),
    route(PUT, "/pty/:ptyID", "pty.update"),
    route(DELETE, "/pty/:ptyID", "pty.remove"),
    route(POST, "/pty/:ptyID/connect-token", "pty.connectToken"),
    route(GET, "/pty/:ptyID/connect", "pty.connect"),
];

/// v1 question routes. From groups/question.ts.
pub const QUESTION_ROUTES: &[Route] = &[
    route(GET, "/question", "question.list"),
    route(POST, "/question/:requestID/reply", "question.reply"),
    route(POST, "/question/:requestID/reject", "question.reject"),
];

/// v1 permission routes. From groups/permission.ts.
pub const PERMISSION_ROUTES: &[Route] = &[
    route(GET, "/permission", "permission.list"),
    route(POST, "/permission/:requestID/reply", "permission.reply"),
];

/// v1 project routes. From groups/project.ts.
pub const PROJECT_ROUTES: &[Route] = &[
    route(GET, "/project", "project.list"),
    route(GET, "/project/current", "project.current"),
    route(POST, "/project/git/init", "project.initGit"),
    route(PATCH, "/project/:projectID", "project.update"),
    route(
        GET,
        "/project/:projectID/directories",
        "project.directories",
    ),
];

/// v1 provider routes. From groups/provider.ts.
pub const PROVIDER_ROUTES: &[Route] = &[
    route(GET, "/provider", "provider.list"),
    route(GET, "/provider/auth", "provider.auth"),
    route(
        POST,
        "/provider/:providerID/oauth/authorize",
        "provider.oauth.authorize",
    ),
    route(
        POST,
        "/provider/:providerID/oauth/callback",
        "provider.oauth.callback",
    ),
];

/// v1 file routes. From groups/file.ts.
pub const FILE_ROUTES: &[Route] = &[
    route(GET, "/find", "find.text"),
    route(GET, "/find/file", "find.files"),
    route(GET, "/find/symbol", "find.symbols"),
    route(GET, "/file", "file.list"),
    route(GET, "/file/content", "file.read"),
    route(GET, "/file/status", "file.status"),
];

/// v1 sync routes. From groups/sync.ts.
pub const SYNC_ROUTES: &[Route] = &[
    route(POST, "/sync/start", "sync.start"),
    route(POST, "/sync/replay", "sync.replay"),
    route(POST, "/sync/steal", "sync.steal"),
    route(POST, "/sync/history", "sync.history.list"),
];

/// v1 TUI routes. From groups/tui.ts.
pub const TUI_ROUTES: &[Route] = &[
    route(POST, "/tui/append-prompt", "tui.appendPrompt"),
    route(POST, "/tui/open-help", "tui.openHelp"),
    route(POST, "/tui/open-sessions", "tui.openSessions"),
    route(POST, "/tui/open-themes", "tui.openThemes"),
    route(POST, "/tui/open-models", "tui.openModels"),
    route(POST, "/tui/submit-prompt", "tui.submitPrompt"),
    route(POST, "/tui/clear-prompt", "tui.clearPrompt"),
    route(POST, "/tui/execute-command", "tui.executeCommand"),
    route(POST, "/tui/show-toast", "tui.showToast"),
    route(POST, "/tui/publish", "tui.publish"),
    route(POST, "/tui/select-session", "tui.selectSession"),
    route(GET, "/tui/control/next", "tui.control.next"),
    route(POST, "/tui/control/response", "tui.control.response"),
];

/// v1 experimental routes. From groups/experimental.ts.
pub const EXPERIMENTAL_ROUTES: &[Route] = &[
    route(
        GET,
        "/experimental/capabilities",
        "experimental.capabilities.get",
    ),
    route(GET, "/experimental/console", "experimental.console.get"),
    route(
        GET,
        "/experimental/console/orgs",
        "experimental.console.listOrgs",
    ),
    route(
        POST,
        "/experimental/console/switch",
        "experimental.console.switchOrg",
    ),
    route(GET, "/experimental/tool", "tool.list"),
    route(GET, "/experimental/tool/ids", "tool.ids"),
    route(GET, "/experimental/worktree", "worktree.list"),
    route(POST, "/experimental/worktree", "worktree.create"),
    route(DELETE, "/experimental/worktree", "worktree.remove"),
    route(POST, "/experimental/worktree/reset", "worktree.reset"),
    route(GET, "/experimental/session", "experimental.session.list"),
    route(
        POST,
        "/experimental/session/:sessionID/background",
        "experimental.session.background",
    ),
    route(GET, "/experimental/resource", "experimental.resource.list"),
];

/// v1 workspace routes. From groups/workspace.ts.
pub const WORKSPACE_ROUTES: &[Route] = &[
    route(
        GET,
        "/experimental/workspace/adapter",
        "experimental.workspace.adapter.list",
    ),
    route(
        GET,
        "/experimental/workspace",
        "experimental.workspace.list",
    ),
    route(
        POST,
        "/experimental/workspace",
        "experimental.workspace.create",
    ),
    route(
        POST,
        "/experimental/workspace/sync-list",
        "experimental.workspace.syncList",
    ),
    route(
        GET,
        "/experimental/workspace/status",
        "experimental.workspace.status",
    ),
    route(
        DELETE,
        "/experimental/workspace/:id",
        "experimental.workspace.remove",
    ),
    route(
        POST,
        "/experimental/workspace/warp",
        "experimental.workspace.warp",
    ),
];

/// v1 control-plane routes. From groups/control-plane.ts.
pub const CONTROL_PLANE_ROUTES: &[Route] = &[route(
    POST,
    "/experimental/control-plane/move-session",
    "experimental.controlPlane.moveSession",
)];

/// v1 project-copy naming route. From groups/project-copy.ts.
pub const PROJECT_COPY_ROUTES: &[Route] = &[route(
    POST,
    "/experimental/project/:projectID/copy/generate-name",
    "experimental.projectCopy.generateName",
)];

/// v1 MCP routes. From groups/mcp.ts.
pub const MCP_ROUTES: &[Route] = &[
    route(GET, "/mcp", "mcp.status"),
    route(POST, "/mcp", "mcp.add"),
    route(POST, "/mcp/:name/auth", "mcp.auth.start"),
    route(POST, "/mcp/:name/auth/callback", "mcp.auth.callback"),
    route(
        POST,
        "/mcp/:name/auth/authenticate",
        "mcp.auth.authenticate",
    ),
    route(DELETE, "/mcp/:name/auth", "mcp.auth.remove"),
    route(POST, "/mcp/:name/connect", "mcp.connect"),
    route(POST, "/mcp/:name/disconnect", "mcp.disconnect"),
];

/// v1 control routes. From groups/control.ts.
pub const CONTROL_ROUTES: &[Route] = &[
    route(PUT, "/auth/:providerID", "auth.set"),
    route(DELETE, "/auth/:providerID", "auth.remove"),
    route(POST, "/log", "app.log"),
];

/// v1 global routes. From groups/global.ts.
pub const GLOBAL_ROUTES: &[Route] = &[
    route(GET, "/global/health", "global.health"),
    route(GET, "/global/event", "global.event"),
    route(GET, "/global/config", "global.config.get"),
    route(PATCH, "/global/config", "global.config.update"),
    route(POST, "/global/dispose", "global.dispose"),
    route(POST, "/global/upgrade", "global.upgrade"),
];

/// Raw router routes: `/doc` OpenAPI spec and the UI catch-all fallback.
pub const RAW_ROUTES: &[Route] = &[
    route(GET, "/doc", "doc"),
    route(GET, "/openapi.json", "openapi.json"),
];

/// Every declared endpoint. The order follows the route tree in
/// reference/packages/opencode/src/server/routes/instance/httpapi/server.ts
/// (`rootApiRoutes`, `eventApiRoutes`, `ptyConnectApiRoutes`, `instanceRoutes`,
/// `serverRoutes`, `docRoute`).
pub fn all_routes() -> Vec<Route> {
    let mut out = Vec::new();
    out.extend_from_slice(CONTROL_ROUTES);
    out.extend_from_slice(CONTROL_PLANE_ROUTES);
    out.extend_from_slice(GLOBAL_ROUTES);
    out.extend_from_slice(EVENT_ROUTES);
    out.extend_from_slice(PTY_ROUTES);
    out.extend_from_slice(CONFIG_ROUTES);
    out.extend_from_slice(EXPERIMENTAL_ROUTES);
    out.extend_from_slice(FILE_ROUTES);
    out.extend_from_slice(INSTANCE_ROUTES);
    out.extend_from_slice(MCP_ROUTES);
    out.extend_from_slice(PROJECT_ROUTES);
    out.extend_from_slice(PROJECT_COPY_ROUTES);
    out.extend_from_slice(PERMISSION_ROUTES);
    out.extend_from_slice(PROVIDER_ROUTES);
    out.extend_from_slice(QUESTION_ROUTES);
    out.extend_from_slice(SESSION_ROUTES);
    out.extend_from_slice(SYNC_ROUTES);
    out.extend_from_slice(TUI_ROUTES);
    out.extend_from_slice(WORKSPACE_ROUTES);
    out.extend_from_slice(V2_ROUTES);
    out.extend_from_slice(RAW_ROUTES);
    out
}

/// Convert an Effect-style `:param` path to axum's `{param}` syntax.
pub fn axum_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' {
            let mut param = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_alphanumeric() || next == '_' {
                    param.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push('{');
            out.push_str(&param);
            out.push('}');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_ids_are_unique() {
        let routes = all_routes();
        let mut seen = std::collections::HashSet::new();
        for route in &routes {
            assert!(
                seen.insert((route.method, route.path)),
                "duplicate {} {}",
                route.method.as_str(),
                route.path
            );
        }
    }

    #[test]
    fn axum_path_conversion() {
        assert_eq!(
            axum_path("/api/session/:sessionID"),
            "/api/session/{sessionID}"
        );
        assert_eq!(
            axum_path("/session/:sessionID/message/:messageID/part/:partID"),
            "/session/{sessionID}/message/{messageID}/part/{partID}"
        );
        assert_eq!(axum_path("/api/fs/read/*"), "/api/fs/read/*");
    }
}
