//! Service dependencies of the workspace runtime.
//!
//! The reference (reference/packages/opencode/src/control-plane/workspace.ts)
//! yields `Session`, `SessionPrompt`, `Vcs`, and `Auth` services. Those live in
//! oc-session / oc-project / oc-provider scope; the port defines the minimal
//! surface as traits so the workspace logic is testable until those crates land.
//!
//! TODO(integration): back these traits with the real oc-session / oc-project /
//! oc-provider services.

use std::sync::Arc;

/// `Vcs.ApplyResult` from reference/packages/opencode/src/project/vcs.ts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ApplyResult {
    pub applied: bool,
}

/// `Vcs.PatchApplyError` from reference/packages/opencode/src/project/vcs.ts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatchApplyError {
    #[error("non-git")]
    NonGit,
    #[error("not-clean")]
    NotClean,
}

/// The subset of the `Vcs` service (reference/packages/opencode/src/project/vcs.ts)
/// used by session warp.
#[async_trait::async_trait]
pub trait VcsOps: Send + Sync {
    async fn diff_raw(&self) -> anyhow::Result<String>;
    async fn apply(&self, patch: &str) -> Result<ApplyResult, PatchApplyError>;
}

/// The subset of the `Session` service (reference/packages/opencode/src/session/session.ts)
/// used by the workspace runtime.
#[async_trait::async_trait]
pub trait SessionOps: Send + Sync {
    async fn set_workspace(
        &self,
        session_id: &str,
        workspace_id: Option<String>,
    ) -> anyhow::Result<()>;
    /// Returns `Ok(true)` if the session existed and was removed.
    async fn remove(&self, session_id: &str) -> anyhow::Result<bool>;
}

/// The subset of the `SessionPrompt` service used by session warp (`cancel`).
#[async_trait::async_trait]
pub trait PromptOps: Send + Sync {
    async fn cancel(&self, session_id: &str) -> anyhow::Result<()>;
}

/// The subset of the `Auth` service used by `Workspace.create` (`auth.all()`).
#[async_trait::async_trait]
pub trait AuthOps: Send + Sync {
    async fn all(&self) -> serde_json::Value;
}

/// The default stub implementations until the owning crates land.
pub struct StubDeps;

#[async_trait::async_trait]
impl VcsOps for StubDeps {
    async fn diff_raw(&self) -> anyhow::Result<String> {
        anyhow::bail!("Vcs service not available")
    }
    async fn apply(&self, _patch: &str) -> Result<ApplyResult, PatchApplyError> {
        Ok(ApplyResult { applied: false })
    }
}

#[async_trait::async_trait]
impl SessionOps for StubDeps {
    async fn set_workspace(
        &self,
        _session_id: &str,
        _workspace_id: Option<String>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn remove(&self, _session_id: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
}

#[async_trait::async_trait]
impl PromptOps for StubDeps {
    async fn cancel(&self, _session_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl AuthOps for StubDeps {
    async fn all(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// Convenience bundle of the dependency services.
pub struct WorkspaceDeps {
    pub session: Arc<dyn SessionOps>,
    pub prompt: Arc<dyn PromptOps>,
    pub vcs: Arc<dyn VcsOps>,
    pub auth: Arc<dyn AuthOps>,
}

impl Default for WorkspaceDeps {
    fn default() -> Self {
        Self {
            session: Arc::new(StubDeps),
            prompt: Arc::new(StubDeps),
            vcs: Arc::new(StubDeps),
            auth: Arc::new(StubDeps),
        }
    }
}

/// A session row: the subset of `SessionTable`
/// (reference/packages/core/src/session/sql.ts) the workspace runtime reads.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
}

/// In-memory session table shared between the workspace runtime and its
/// `SessionOps` implementation.
pub type MemorySessionStore = Arc<std::sync::Mutex<Vec<SessionRow>>>;

/// `SessionOps` over an in-memory session table.
pub struct MemorySessionOps(pub MemorySessionStore);

#[async_trait::async_trait]
impl SessionOps for MemorySessionOps {
    async fn set_workspace(
        &self,
        session_id: &str,
        workspace_id: Option<String>,
    ) -> anyhow::Result<()> {
        let mut sessions = self.0.lock().expect("session store poisoned");
        if let Some(row) = sessions.iter_mut().find(|row| row.id == session_id) {
            row.workspace_id = workspace_id;
        }
        Ok(())
    }

    async fn remove(&self, session_id: &str) -> anyhow::Result<bool> {
        let mut sessions = self.0.lock().expect("session store poisoned");
        let before = sessions.len();
        sessions.retain(|row| row.id != session_id);
        Ok(sessions.len() != before)
    }
}
