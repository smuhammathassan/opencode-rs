//! `SessionExecution` — process-local execution control keyed by Session ID.
//!
//! Ports `packages/core/src/session/execution.ts`.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use crate::runner::RunError;
use crate::session::SessionID;

/// Execution control surface. `resume` starts execution while idle or joins
/// the active execution; `wake` registers newly recorded work (repeated
/// wakeups may coalesce); `interrupt` stops active work owned by this process
/// (idle interruption is a no-op).
/// /// From reference/packages/core/src/session/execution.ts
pub trait SessionExecution: Send + Sync {
    fn active(&self) -> Pin<Box<dyn Future<Output = HashSet<SessionID>> + Send + '_>>;
    fn resume(
        &self,
        session_id: SessionID,
    ) -> Pin<Box<dyn Future<Output = Result<(), RunError>> + Send + '_>>;
    fn wake(&self, session_id: SessionID) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
    fn interrupt(&self, session_id: SessionID) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Low-level compatibility layer for callers that only need durable Session
/// recording.
/// /// From reference/packages/core/src/session/execution.ts (`noopLayer`)
#[derive(Debug, Default, Clone)]
pub struct NoopExecution;

impl SessionExecution for NoopExecution {
    fn active(&self) -> Pin<Box<dyn Future<Output = HashSet<SessionID>> + Send + '_>> {
        Box::pin(async { HashSet::new() })
    }

    fn resume(
        &self,
        _session_id: SessionID,
    ) -> Pin<Box<dyn Future<Output = Result<(), RunError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn wake(&self, _session_id: SessionID) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    fn interrupt(&self, _session_id: SessionID) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }
}
