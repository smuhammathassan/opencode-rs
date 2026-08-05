//! Control-plane utilities: `waitEvent` and the `route` URL helper.
//!
//! From reference/packages/opencode/src/control-plane/util.ts and the `route`
//! helper in reference/packages/opencode/src/control-plane/workspace.ts.

use std::time::Duration;

use thiserror::Error;
use url::Url;

use super::global_bus::{GlobalBus, GlobalEvent};

/// Error mirroring the failure paths of `waitEvent` in
/// reference/packages/opencode/src/control-plane/util.ts.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WaitEventError {
    #[error("Timed out waiting for global event")]
    TimedOut,
    #[error("Request aborted")]
    Aborted,
}

/// `waitEvent` from reference/packages/opencode/src/control-plane/util.ts: wait
/// (up to `timeout`) for a global event matching `predicate`.
pub async fn wait_event(
    bus: &GlobalBus,
    timeout: Duration,
    predicate: impl Fn(&GlobalEvent) -> bool,
) -> Result<(), WaitEventError> {
    let mut rx = bus.subscribe();
    tokio::time::timeout(timeout, async {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let matched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        predicate(&event)
                    }));
                    match matched {
                        Ok(true) => return Ok(()),
                        Ok(false) => continue,
                        Err(_) => return Err(WaitEventError::Aborted),
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err(WaitEventError::Aborted)
                }
            }
        }
    })
    .await
    .map_err(|_| WaitEventError::TimedOut)?
}

/// `route(url, path)` from reference/packages/opencode/src/control-plane/workspace.ts:
/// join `path` onto the URL's pathname (stripping trailing `/`), clearing query
/// and fragment.
pub fn route(url: &str, path: &str) -> Result<Url, url::ParseError> {
    let mut next = Url::parse(url)?;
    let base = next.path().trim_end_matches('/');
    next.set_path(&format!("{base}{path}"));
    next.set_query(None);
    next.set_fragment(None);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_appends_path_and_clears_query() {
        let url = route("http://localhost:5000/", "/sync/history").unwrap();
        assert_eq!(url.to_string(), "http://localhost:5000/sync/history");
    }

    #[test]
    fn route_strips_trailing_slash() {
        let url = route("http://localhost:5000/base/", "/sync/history").unwrap();
        assert_eq!(url.to_string(), "http://localhost:5000/base/sync/history");
    }

    #[test]
    fn route_preserves_base_path() {
        let url = route("http://localhost:5000/instance?q=1#frag", "/global/event").unwrap();
        assert_eq!(
            url.to_string(),
            "http://localhost:5000/instance/global/event"
        );
    }

    #[tokio::test]
    async fn wait_event_returns_on_match() {
        let bus = GlobalBus::new();
        let bus_for_emit = bus.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            bus_for_emit.emit(GlobalEvent {
                directory: None,
                project: None,
                workspace: Some("wrk_1".into()),
                payload: serde_json::json!({ "type": "workspace.status", "properties": { "workspaceID": "wrk_1", "status": "connected" } }),
            });
        });
        let result = wait_event(&bus, Duration::from_secs(1), |event| {
            event.workspace.as_deref() == Some("wrk_1")
                && event.payload.get("type").and_then(|t| t.as_str()) == Some("workspace.status")
        })
        .await;
        handle.await.unwrap();
        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    async fn wait_event_times_out() {
        let bus = GlobalBus::new();
        let result = wait_event(&bus, Duration::from_millis(20), |_| false).await;
        assert_eq!(result, Err(WaitEventError::TimedOut));
    }
}
