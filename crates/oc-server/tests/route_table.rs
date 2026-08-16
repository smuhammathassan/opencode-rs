//! Golden test: the route table must match the reference route tree.
//!
//! The expected table below is transcribed from the reference HttpApi group
//! definitions (`reference/packages/protocol/src/groups/*` and
//! `reference/packages/opencode/src/server/routes/instance/httpapi/groups/*`), composed
//! the same way `reference/packages/opencode/src/server/routes/instance/httpapi/server.ts`
//! assembles its route tree. Any deviation here is a wire-parity regression.

use oc_server::route::{all_routes, Method, Route};

fn route(method: &str, path: &'static str) -> (String, &'static str) {
    (method.to_string(), path)
}

fn expected() -> Vec<(String, &'static str)> {
    vec![
        // control.ts
        route("PUT", "/auth/:providerID"),
        route("DELETE", "/auth/:providerID"),
        route("POST", "/log"),
        // control-plane.ts
        route("POST", "/experimental/control-plane/move-session"),
        // global.ts
        route("GET", "/global/health"),
        route("GET", "/global/event"),
        route("GET", "/global/config"),
        route("PATCH", "/global/config"),
        route("POST", "/global/dispose"),
        route("POST", "/global/upgrade"),
        // event.ts
        route("GET", "/event"),
        // pty.ts
        route("GET", "/pty/shells"),
        route("GET", "/pty"),
        route("POST", "/pty"),
        route("GET", "/pty/:ptyID"),
        route("PUT", "/pty/:ptyID"),
        route("DELETE", "/pty/:ptyID"),
        route("POST", "/pty/:ptyID/connect-token"),
        route("GET", "/pty/:ptyID/connect"),
        // config.ts
        route("GET", "/config"),
        route("PATCH", "/config"),
        route("GET", "/config/providers"),
        // experimental.ts
        route("GET", "/experimental/capabilities"),
        route("GET", "/experimental/console"),
        route("GET", "/experimental/console/orgs"),
        route("POST", "/experimental/console/switch"),
        route("GET", "/experimental/tool"),
        route("GET", "/experimental/tool/ids"),
        route("GET", "/experimental/worktree"),
        route("POST", "/experimental/worktree"),
        route("DELETE", "/experimental/worktree"),
        route("POST", "/experimental/worktree/reset"),
        route("GET", "/experimental/session"),
        route("GET", "/experimental/session/background"),
        route("GET", "/experimental/session/:sessionID/background"),
        route("POST", "/experimental/session/:sessionID/background"),
        route("DELETE", "/experimental/session/:sessionID/background"),
        route("GET", "/experimental/resource"),
        // file.ts
        route("GET", "/find"),
        route("GET", "/find/file"),
        route("GET", "/find/symbol"),
        route("GET", "/file"),
        route("GET", "/file/content"),
        route("GET", "/file/status"),
        // instance.ts
        route("POST", "/instance/dispose"),
        route("GET", "/path"),
        route("GET", "/vcs"),
        route("GET", "/vcs/status"),
        route("GET", "/vcs/diff"),
        route("GET", "/vcs/diff/raw"),
        route("POST", "/vcs/apply"),
        route("GET", "/command"),
        route("GET", "/agent"),
        route("GET", "/skill"),
        route("GET", "/lsp"),
        route("GET", "/formatter"),
        // mcp.ts
        route("GET", "/mcp"),
        route("POST", "/mcp"),
        route("POST", "/mcp/:name/auth"),
        route("POST", "/mcp/:name/auth/callback"),
        route("POST", "/mcp/:name/auth/authenticate"),
        route("DELETE", "/mcp/:name/auth"),
        route("POST", "/mcp/:name/connect"),
        route("POST", "/mcp/:name/disconnect"),
        // project.ts
        route("GET", "/project"),
        route("GET", "/project/current"),
        route("POST", "/project/git/init"),
        route("PATCH", "/project/:projectID"),
        route("GET", "/project/:projectID/directories"),
        // project-copy.ts
        route(
            "POST",
            "/experimental/project/:projectID/copy/generate-name",
        ),
        // permission.ts
        route("GET", "/permission"),
        route("POST", "/permission/:requestID/reply"),
        // provider.ts
        route("GET", "/provider"),
        route("GET", "/provider/auth"),
        route("POST", "/provider/:providerID/oauth/authorize"),
        route("POST", "/provider/:providerID/oauth/callback"),
        // question.ts
        route("GET", "/question"),
        route("POST", "/question/:requestID/reply"),
        route("POST", "/question/:requestID/reject"),
        // session.ts
        route("GET", "/session"),
        route("GET", "/session/status"),
        route("GET", "/session/:sessionID"),
        route("GET", "/session/:sessionID/children"),
        route("GET", "/session/:sessionID/todo"),
        route("GET", "/session/:sessionID/diff"),
        route("GET", "/session/:sessionID/message"),
        route("GET", "/session/:sessionID/message/:messageID"),
        route("POST", "/session"),
        route("DELETE", "/session/:sessionID"),
        route("PATCH", "/session/:sessionID"),
        route("POST", "/session/:sessionID/fork"),
        route("POST", "/session/:sessionID/abort"),
        route("POST", "/session/:sessionID/share"),
        route("DELETE", "/session/:sessionID/share"),
        route("POST", "/session/:sessionID/init"),
        route("POST", "/session/:sessionID/summarize"),
        route("POST", "/session/:sessionID/message"),
        route("POST", "/session/:sessionID/prompt_async"),
        route("POST", "/session/:sessionID/command"),
        route("POST", "/session/:sessionID/shell"),
        route("POST", "/session/:sessionID/revert"),
        route("POST", "/session/:sessionID/unrevert"),
        route("POST", "/session/:sessionID/permissions/:permissionID"),
        route("DELETE", "/session/:sessionID/message/:messageID"),
        route(
            "DELETE",
            "/session/:sessionID/message/:messageID/part/:partID",
        ),
        route(
            "PATCH",
            "/session/:sessionID/message/:messageID/part/:partID",
        ),
        // sync.ts
        route("POST", "/sync/start"),
        route("POST", "/sync/replay"),
        route("POST", "/sync/steal"),
        route("POST", "/sync/history"),
        // tui.ts
        route("POST", "/tui/append-prompt"),
        route("POST", "/tui/open-help"),
        route("POST", "/tui/open-sessions"),
        route("POST", "/tui/open-themes"),
        route("POST", "/tui/open-models"),
        route("POST", "/tui/submit-prompt"),
        route("POST", "/tui/clear-prompt"),
        route("POST", "/tui/execute-command"),
        route("POST", "/tui/show-toast"),
        route("POST", "/tui/publish"),
        route("POST", "/tui/select-session"),
        route("GET", "/tui/control/next"),
        route("POST", "/tui/control/response"),
        // workspace.ts
        route("GET", "/experimental/workspace/adapter"),
        route("GET", "/experimental/workspace"),
        route("POST", "/experimental/workspace"),
        route("POST", "/experimental/workspace/sync-list"),
        route("GET", "/experimental/workspace/status"),
        route("DELETE", "/experimental/workspace/:id"),
        route("POST", "/experimental/workspace/warp"),
        // v2 protocol groups
        route("GET", "/api/health"),
        route("GET", "/api/location"),
        route("GET", "/api/agent"),
        route("GET", "/api/session"),
        route("POST", "/api/session"),
        route("GET", "/api/session/active"),
        route("GET", "/api/session/:sessionID"),
        route("POST", "/api/session/:sessionID/agent"),
        route("POST", "/api/session/:sessionID/model"),
        route("POST", "/api/session/:sessionID/prompt"),
        route("POST", "/api/session/:sessionID/compact"),
        route("POST", "/api/session/:sessionID/wait"),
        route("POST", "/api/session/:sessionID/revert/stage"),
        route("POST", "/api/session/:sessionID/revert/clear"),
        route("POST", "/api/session/:sessionID/revert/commit"),
        route("GET", "/api/session/:sessionID/context"),
        route("GET", "/api/session/:sessionID/history"),
        route("GET", "/api/session/:sessionID/event"),
        route("POST", "/api/session/:sessionID/interrupt"),
        route("GET", "/api/session/:sessionID/message/:messageID"),
        route("GET", "/api/session/:sessionID/message"),
        route("GET", "/api/model"),
        route("GET", "/api/provider"),
        route("GET", "/api/provider/:providerID"),
        route("GET", "/api/integration"),
        route("GET", "/api/integration/:integrationID"),
        route("POST", "/api/integration/:integrationID/connect/key"),
        route("POST", "/api/integration/:integrationID/connect/oauth"),
        route("GET", "/api/integration/attempt/:attemptID"),
        route("POST", "/api/integration/attempt/:attemptID/complete"),
        route("DELETE", "/api/integration/attempt/:attemptID"),
        route("PATCH", "/api/credential/:credentialID"),
        route("DELETE", "/api/credential/:credentialID"),
        route("GET", "/api/permission/request"),
        route("GET", "/api/permission/saved"),
        route("DELETE", "/api/permission/saved/:id"),
        route("POST", "/api/session/:sessionID/permission"),
        route("GET", "/api/session/:sessionID/permission"),
        route("GET", "/api/session/:sessionID/permission/:requestID"),
        route(
            "POST",
            "/api/session/:sessionID/permission/:requestID/reply",
        ),
        route("GET", "/api/fs/read/*"),
        route("GET", "/api/fs/list"),
        route("GET", "/api/fs/find"),
        route("GET", "/api/command"),
        route("GET", "/api/skill"),
        route("GET", "/api/event"),
        route("GET", "/api/pty"),
        route("POST", "/api/pty"),
        route("GET", "/api/pty/:ptyID"),
        route("PUT", "/api/pty/:ptyID"),
        route("DELETE", "/api/pty/:ptyID"),
        route("POST", "/api/pty/:ptyID/connect-token"),
        route("GET", "/api/pty/:ptyID/connect"),
        route("GET", "/api/question/request"),
        route("GET", "/api/session/:sessionID/question"),
        route("POST", "/api/session/:sessionID/question/:requestID/reply"),
        route("POST", "/api/session/:sessionID/question/:requestID/reject"),
        route("GET", "/api/reference"),
        route("POST", "/experimental/project/:projectID/copy"),
        route("DELETE", "/experimental/project/:projectID/copy"),
        route("POST", "/experimental/project/:projectID/copy/refresh"),
        // raw router routes
        route("GET", "/doc"),
        route("GET", "/openapi.json"),
    ]
}

#[test]
fn route_table_matches_reference() {
    let expected = expected();
    let mut actual: Vec<(String, String)> = all_routes()
        .iter()
        .map(|r| (r.method.as_str().to_string(), r.path.to_string()))
        .collect();
    actual.sort();
    let mut expected = expected
        .into_iter()
        .map(|(m, p)| (m, p.to_string()))
        .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(actual.len(), expected.len(), "route count mismatch");
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        assert_eq!(actual, expected, "route mismatch");
    }
}

#[test]
fn route_ids_are_stable_and_unique() {
    let mut seen = std::collections::HashSet::new();
    for Route { method, path, id } in all_routes() {
        assert!(seen.insert(id), "duplicate route id {id}");
        let _ = (method, path);
    }
}

#[test]
fn key_route_ids_are_expected() {
    let routes = all_routes();
    let find =
        |method: Method, path: &str| routes.iter().find(|r| r.method == method && r.path == path);
    assert_eq!(
        find(Method::Get, "/api/health").unwrap().id,
        "v2.health.get"
    );
    assert_eq!(
        find(Method::Get, "/api/session").unwrap().id,
        "v2.session.list"
    );
    assert_eq!(
        find(Method::Post, "/api/session/:sessionID/prompt")
            .unwrap()
            .id,
        "v2.session.prompt"
    );
    assert_eq!(
        find(Method::Get, "/api/event").unwrap().id,
        "v2.event.subscribe"
    );
    assert_eq!(find(Method::Get, "/config").unwrap().id, "config.get");
    assert_eq!(
        find(Method::Get, "/global/health").unwrap().id,
        "global.health"
    );
    assert_eq!(
        find(Method::Get, "/session/:sessionID/message").unwrap().id,
        "session.messages"
    );
}
