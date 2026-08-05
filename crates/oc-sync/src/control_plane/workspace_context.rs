//! Workspace context — ambient workspace ID for a fiber/task.
//!
//! From reference/packages/opencode/src/control-plane/workspace-context.ts. The
//! reference uses an AsyncLocal (`LocalContext`); the port uses a tokio task-local
//! so concurrent tasks keep independent values.
//!
//! TODO(integration): revisit once oc-core lands its own context/instance service.

tokio::task_local! {
    static WORKSPACE_ID: Option<String>;
    static PROJECT_ID: Option<String>;
}

/// Mirrors `WorkspaceContext` in reference/packages/opencode/src/control-plane/workspace-context.ts.
pub struct WorkspaceContext;

impl WorkspaceContext {
    /// `provide`: run `fut` with a workspace ID in context.
    pub async fn provide<Fut, T>(workspace_id: Option<String>, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        WORKSPACE_ID.scope(workspace_id, fut).await
    }

    /// `restore`: run `fut` with a workspace ID in context.
    pub async fn restore<Fut, T>(workspace_id: &str, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        WORKSPACE_ID
            .scope(Some(workspace_id.to_string()), fut)
            .await
    }

    /// `get workspaceID` — the ambient workspace ID, if any.
    pub fn workspace_id() -> Option<String> {
        WORKSPACE_ID.try_with(|id| id.clone()).unwrap_or(None)
    }
}

/// The subset of the reference's `InstanceRef` the control plane needs: the
/// current project id (`ctx.project.id`). The full `InstanceContext` lives in
/// oc-core.
///
/// TODO(integration): fold into the oc-core instance service.
pub struct InstanceContext;

impl InstanceContext {
    /// Run `fut` with a project ID in context.
    pub async fn provide<Fut, T>(project_id: Option<String>, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        PROJECT_ID.scope(project_id, fut).await
    }

    /// The ambient project id, if any.
    pub fn project_id() -> Option<String> {
        PROJECT_ID.try_with(|id| id.clone()).unwrap_or(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provide_scopes_workspace_id() {
        let mut seen = None;
        WorkspaceContext::provide(Some("wrk_1".into()), async {
            seen = WorkspaceContext::workspace_id();
        })
        .await;
        assert_eq!(seen.as_deref(), Some("wrk_1"));
        assert_eq!(WorkspaceContext::workspace_id(), None);
    }

    #[tokio::test]
    async fn restore_sets_workspace_id() {
        let mut seen = None;
        WorkspaceContext::restore("wrk_2", async {
            seen = WorkspaceContext::workspace_id();
        })
        .await;
        assert_eq!(seen.as_deref(), Some("wrk_2"));
    }

    #[tokio::test]
    async fn default_is_none_outside_context() {
        assert_eq!(WorkspaceContext::workspace_id(), None);
    }

    #[tokio::test]
    async fn instance_context_scopes_project_id() {
        let mut seen = None;
        InstanceContext::provide(Some("prj_1".into()), async {
            seen = InstanceContext::project_id();
        })
        .await;
        assert_eq!(seen.as_deref(), Some("prj_1"));
        assert_eq!(InstanceContext::project_id(), None);
    }
}
