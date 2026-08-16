//! Shared server helpers. From reference/packages/opencode/src/server/shared/.

/// PTY connect ticket constants. From reference/.../shared/pty-ticket.ts.
pub mod pty_ticket {
    pub const PTY_CONNECT_TICKET_QUERY: &str = "ticket";
    pub const PTY_CONNECT_TOKEN_HEADER: &str = "x-opencode-ticket";
    pub const PTY_CONNECT_TOKEN_HEADER_VALUE: &str = "1";

    /// `^\/pty\/[^/]+\/connect$`
    fn is_pty_connect_path(pathname: &str) -> bool {
        let pathname = pathname.strip_prefix("/api").unwrap_or(pathname);
        let mut segments = pathname.split('/').filter(|s| !s.is_empty());
        let first = segments.next();
        let second = segments.next();
        let third = segments.next();
        matches!(
            (first, second, third),
            (Some("pty"), Some(_), Some("connect"))
        ) && segments.next().is_none()
    }

    pub fn has_pty_connect_ticket_url(path: &str, query: Option<&str>) -> bool {
        if !is_pty_connect_path(path) {
            return false;
        }
        let params: std::collections::HashMap<String, String> =
            url::form_urlencoded::parse(query.unwrap_or("").as_bytes())
                .into_owned()
                .collect();
        params
            .get(PTY_CONNECT_TICKET_QUERY)
            .map_or(false, |t| !t.is_empty())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pty_connect_ticket_matching() {
            assert!(has_pty_connect_ticket_url(
                "/pty/pty_1/connect",
                Some("ticket=abc")
            ));
            assert!(has_pty_connect_ticket_url(
                "/api/pty/pty_1/connect",
                Some("ticket=abc")
            ));
            assert!(!has_pty_connect_ticket_url("/pty/pty_1/connect", None));
            assert!(!has_pty_connect_ticket_url(
                "/session/status",
                Some("ticket=abc")
            ));
        }
    }
}

/// Workspace routing rules. From reference/.../shared/workspace-routing.ts.
pub mod workspace_routing {
    /// Whether the route is handled locally rather than forwarded to the workspace.
    pub fn is_local_workspace_route(method: &str, path: &str) -> bool {
        const RULES: &[(&str, &str, bool)] = &[
            // path, method, action_is_local
            ("/experimental/workspace", "*", true),
            ("/session/status", "*", false),
            ("/session", "GET", true),
        ];
        for (rule_path, rule_method, local) in RULES {
            if *rule_method != "*" && *rule_method != method {
                continue;
            }
            let matches = path == *rule_path || path.starts_with(&format!("{rule_path}/"));
            if matches {
                return *local;
            }
        }
        false
    }

    /// Extract the session ID from a workspace-routed path.
    pub fn get_workspace_route_session_id(path: &str) -> Option<String> {
        if path == "/session/status" {
            return None;
        }
        if let Some(captured) = path
            .strip_prefix("/session/")
            .and_then(|rest| rest.split('/').next())
            .map(|s| s.to_string())
        {
            return Some(captured);
        }
        let background = path
            .strip_prefix("/experimental/session/")?
            .strip_suffix("/background")?;
        Some(background.split('/').next().unwrap_or("").to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn route_classification() {
            assert!(is_local_workspace_route("GET", "/session"));
            assert!(!is_local_workspace_route("POST", "/session"));
            assert!(!is_local_workspace_route("GET", "/session/status"));
            assert!(is_local_workspace_route(
                "GET",
                "/experimental/workspace/warp"
            ));
            assert!(is_local_workspace_route("POST", "/experimental/workspace"));
        }

        #[test]
        fn session_id_extraction() {
            assert_eq!(
                get_workspace_route_session_id("/session/ses_1/message"),
                Some("ses_1".into())
            );
            assert_eq!(
                get_workspace_route_session_id("/experimental/session/ses_1/background"),
                Some("ses_1".into())
            );
            assert_eq!(get_workspace_route_session_id("/session/status"), None);
        }
    }
}

/// TUI control queues. From reference/.../shared/tui-control.ts.
pub mod tui_control {
    use tokio::sync::mpsc;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct TuiRequest {
        pub path: String,
        pub body: serde_json::Value,
    }

    struct RequestBus {
        tx: mpsc::UnboundedSender<TuiRequest>,
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<TuiRequest>>,
    }

    struct ResponseBus {
        tx: mpsc::UnboundedSender<serde_json::Value>,
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<serde_json::Value>>,
    }

    static REQUEST_BUS: std::sync::OnceLock<RequestBus> = std::sync::OnceLock::new();
    static RESPONSE_BUS: std::sync::OnceLock<ResponseBus> = std::sync::OnceLock::new();

    fn request_bus() -> &'static RequestBus {
        REQUEST_BUS.get_or_init(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            RequestBus {
                tx,
                rx: tokio::sync::Mutex::new(rx),
            }
        })
    }

    fn response_bus() -> &'static ResponseBus {
        RESPONSE_BUS.get_or_init(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            ResponseBus {
                tx,
                rx: tokio::sync::Mutex::new(rx),
            }
        })
    }

    pub fn submit_tui_request(body: TuiRequest) {
        let _ = request_bus().tx.send(body);
    }

    pub async fn next_tui_request() -> Option<TuiRequest> {
        request_bus().rx.lock().await.recv().await
    }

    pub fn submit_tui_response(body: serde_json::Value) {
        let _ = response_bus().tx.send(body);
    }

    pub async fn next_tui_response() -> Option<serde_json::Value> {
        response_bus().rx.lock().await.recv().await
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn request_and_response_round_trip_through_live_queues() {
            submit_tui_request(TuiRequest {
                path: "/tui/open-help".into(),
                body: serde_json::json!({}),
            });
            assert_eq!(
                next_tui_request()
                    .await
                    .map(|request| (request.path, request.body)),
                Some(("/tui/open-help".into(), serde_json::json!({})))
            );

            submit_tui_response(serde_json::json!({ "accepted": true }));
            assert_eq!(
                next_tui_response().await,
                Some(serde_json::json!({ "accepted": true }))
            );
        }
    }
}

/// Sync fence helpers. From reference/.../shared/fence.ts.
pub mod fence {
    use std::collections::HashMap;

    pub const HEADER: &str = "x-opencode-sync";
    pub type State = HashMap<String, i64>;

    /// Parse the `x-opencode-sync` header into aggregate sequence state.
    pub fn parse(value: Option<&str>) -> Option<State> {
        let raw = value?;
        let data: serde_json::Value = serde_json::from_str(raw).ok()?;
        let object = data.as_object()?;
        let mut state = HashMap::new();
        for (key, value) in object {
            if let Some(seq) = value.as_i64() {
                state.insert(key.clone(), seq);
            }
        }
        Some(state)
    }

    /// Compute aggregates whose sequence differs from the previous state.
    pub fn diff(prev: &State, next: &State) -> State {
        let mut ids: std::collections::BTreeSet<&String> = std::collections::BTreeSet::new();
        ids.extend(prev.keys());
        ids.extend(next.keys());
        let mut result = HashMap::new();
        for id in ids {
            let prev_seq = prev.get(id).copied().unwrap_or(-1);
            let next_seq = next.get(id).copied().unwrap_or(-1);
            if prev_seq != next_seq {
                result.insert(id.clone(), next_seq);
            }
        }
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_fence_header() {
            let parsed = parse(Some(r#"{"ses_1":3,"ses_2":0}"#)).unwrap();
            assert_eq!(parsed.get("ses_1"), Some(&3));
            assert!(parse(Some("not json")).is_none());
        }

        #[test]
        fn diffs_sequences() {
            let mut prev = HashMap::new();
            prev.insert("a".to_string(), 1);
            let mut next = HashMap::new();
            next.insert("a".to_string(), 2);
            next.insert("b".to_string(), 5);
            let diff = diff(&prev, &next);
            assert_eq!(diff.get("a"), Some(&2));
            assert_eq!(diff.get("b"), Some(&5));
        }
    }
}
