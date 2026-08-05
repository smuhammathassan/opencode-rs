//! Current-process routing for implicit-local Locations.
//!
//! Ports `packages/core/src/session/execution/local.ts`. A `RunCoordinator`
//! serializes drains per Session while letting different Sessions run
//! concurrently. Future remote placement belongs here.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::execution::SessionExecution;
use crate::run_coordinator::RunCoordinator;
use crate::runner::RunError;
use crate::session::services::SessionStore;
use crate::session::SessionID;

/// Process-local `SessionExecution` backed by a per-session coordinator.
/// The runner is Location-scoped; callers must supply the runner bound to the
/// same Location as the sessions it drains.
/// /// From reference/packages/core/src/session/execution/local.ts
pub struct LocalExecution {
    coordinator: RunCoordinator<SessionID, RunError>,
}

impl LocalExecution {
    pub fn new(
        store: Arc<dyn SessionStore>,
        runner: Arc<dyn crate::runner::SessionRunner>,
    ) -> Self {
        let drain = move |session_id: SessionID, force: bool, token: CancellationToken| {
            let store = store.clone();
            let runner = runner.clone();
            async move {
                let session = store.get(&session_id).await.ok_or_else(|| {
                    tracing::error!(session_id = %session_id, "drain failed: session not found");
                    RunError::SessionNotFound {
                        session_id: session_id.clone(),
                    }
                })?;
                let _ = session;
                runner.run(&session_id, force, token).await.map_err(|error| {
                    // The reference logs drain failures that are not
                    // interrupts-only.
                    tracing::error!(session_id = %session_id, error = %error, "failed to drain session");
                    error
                })
            }
        };
        Self {
            coordinator: RunCoordinator::new(drain),
        }
    }
}

impl SessionExecution for LocalExecution {
    fn active(&self) -> Pin<Box<dyn Future<Output = HashSet<SessionID>> + Send + '_>> {
        Box::pin(async move { self.coordinator.active().await })
    }

    fn resume(
        &self,
        session_id: SessionID,
    ) -> Pin<Box<dyn Future<Output = Result<(), RunError>> + Send + '_>> {
        Box::pin(async move { self.coordinator.run(session_id).await })
    }

    fn wake(&self, session_id: SessionID) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move { self.coordinator.wake(session_id).await })
    }

    fn interrupt(&self, session_id: SessionID) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move { self.coordinator.interrupt(session_id).await })
    }
}
