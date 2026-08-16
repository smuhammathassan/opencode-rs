//! Event projectors.
//!
//! `reference/packages/opencode/src/server/projectors.ts` is a no-op today — event
//! projection lives in oc-sync's replay pipeline. This module keeps the same surface
//! (`initProjectors`) and registers lightweight in-process projections that feed the
//! SSE streams from the event bus.

use std::sync::Arc;

use crate::event::Event;
use crate::state::AppState;

/// One projection applied to each published event.
pub trait Projector: Send + Sync {
    fn name(&self) -> &'static str;
    fn apply(&self, event: &Event, state: &AppState);
}

/// Registry of active projectors.
#[derive(Default)]
pub struct ProjectorRegistry {
    projectors: Vec<Arc<dyn Projector>>,
}

impl ProjectorRegistry {
    pub fn register(&mut self, projector: Arc<dyn Projector>) {
        self.projectors.push(projector);
    }

    pub fn project(&self, event: &Event, state: &AppState) {
        for projector in &self.projectors {
            projector.apply(event, state);
        }
    }
}

/// Spawn the background projection task. Mirrors `initProjectors()` from
/// reference/packages/opencode/src/server/init-projectors.ts plus a persistent
/// subscriber that applies registered projectors to every bus event.
pub fn init_projectors(state: AppState) -> tokio::task::JoinHandle<()> {
    let mut registry = ProjectorRegistry::default();
    registry.register(Arc::new(ActiveSessionProjector));
    let mut receiver = state.events.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = receiver.recv().await {
            registry.project(&event, &state);
        }
    })
}

/// Keep session `active` state consistent with agent-loop lifecycle events.
/// TODO(integration): derive from durable session events (oc-sync).
struct ActiveSessionProjector;

impl Projector for ActiveSessionProjector {
    fn name(&self) -> &'static str {
        "active-session"
    }

    fn apply(&self, event: &Event, state: &AppState) {
        let session_id = event
            .data
            .get("sessionID")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let Some(session_id) = session_id else { return };
        let active = event.r#type.ends_with(".started") || event.r#type.ends_with(".prompted");
        if active {
            // Projectors run on the Tokio runtime. `blocking_write` would
            // panic there (and did during a real CLI prompt), so keep this
            // synchronous projection non-blocking. The runner also updates
            // the same flag on turn completion/error.
            if let Ok(mut stores) = state.stores.try_write() {
                if let Some(record) = stores.sessions.get_mut(&session_id) {
                    record.active = true;
                }
            }
        }
    }
}
