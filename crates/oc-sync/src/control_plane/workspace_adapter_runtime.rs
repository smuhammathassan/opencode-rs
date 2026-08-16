//! Workspace adapter runtime: resolves adapters and builds the adapter context.
//!
//! From reference/packages/opencode/src/control-plane/workspace-adapter-runtime.ts.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::adapters::{self, AdapterRef};
use super::sync_api::SyncApi;
use super::types::{Target, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo};
use super::workspace_context::{InstanceContext, WorkspaceContext};

/// Builds the `WorkspaceAdapterContext` from ambient instance/workspace context.
fn context(api: Option<Arc<dyn SyncApi>>, target: Option<Target>) -> WorkspaceAdapterContext {
    WorkspaceAdapterContext {
        workspace_id: WorkspaceContext::workspace_id(),
        project_id: InstanceContext::project_id(),
        sync_api: api,
        target,
    }
}

/// `target` from the reference: resolve the adapter for the workspace type and
/// ask it for the target.
pub async fn target(info: &WorkspaceInfo) -> anyhow::Result<Target> {
    let adapter = adapters::get_adapter(&info.project_id, &info.ty)?;
    let ctx = context(None, None);
    adapter.target(info, &ctx).await
}

/// `configure` from the reference.
pub async fn configure(adapter: &AdapterRef, info: WorkspaceInfo) -> anyhow::Result<WorkspaceInfo> {
    let ctx = context(None, None);
    adapter.configure(info, &ctx).await
}

/// `create` from the reference.
pub async fn create(
    adapter: &AdapterRef,
    info: &WorkspaceInfo,
    env: &BTreeMap<String, Option<String>>,
    from: Option<&WorkspaceInfo>,
) -> anyhow::Result<()> {
    let ctx = context(None, None);
    adapter.create(info, env, from, &ctx).await
}

/// `create` with the owning workspace runtime's injectable transport.
pub async fn create_with_api(
    adapter: &AdapterRef,
    info: &WorkspaceInfo,
    env: &BTreeMap<String, Option<String>>,
    from: Option<&WorkspaceInfo>,
    api: Arc<dyn SyncApi>,
) -> anyhow::Result<()> {
    let ctx = context(Some(api), None);
    adapter.create(info, env, from, &ctx).await
}

/// `list` from the reference (`adapter.list?.(ctx) ?? []`).
pub async fn list(adapter: &AdapterRef) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
    let ctx = context(None, None);
    adapter.list(&ctx).await
}

/// `list` with an injectable transport and the target resolved from a known
/// workspace of the same adapter type.
pub async fn list_with_api(
    adapter: &AdapterRef,
    api: Arc<dyn SyncApi>,
    target: Option<Target>,
) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
    let ctx = context(Some(api), target);
    adapter.list(&ctx).await
}

/// `remove` from the reference.
pub async fn remove(info: &WorkspaceInfo) -> anyhow::Result<()> {
    let adapter = adapters::get_adapter(&info.project_id, &info.ty)?;
    let ctx = context(None, None);
    adapter.remove(info, &ctx).await
}

/// `remove` with the owning workspace runtime's injectable transport.
pub async fn remove_with_api(info: &WorkspaceInfo, api: Arc<dyn SyncApi>) -> anyhow::Result<()> {
    let adapter = adapters::get_adapter(&info.project_id, &info.ty)?;
    let ctx = context(Some(api), None);
    adapter.remove(info, &ctx).await
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::types::*;
    use std::sync::Arc;

    struct FakeAdapter;

    #[async_trait::async_trait]
    impl WorkspaceAdapter for FakeAdapter {
        fn name(&self) -> &'static str {
            "Fake"
        }
        fn description(&self) -> &'static str {
            "Fake adapter"
        }
        async fn configure(
            &self,
            info: WorkspaceInfo,
            context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<WorkspaceInfo> {
            assert_eq!(context.project_id.as_deref(), Some("prj_1"));
            Ok(info)
        }
        async fn create(
            &self,
            _info: &WorkspaceInfo,
            _env: &BTreeMap<String, Option<String>>,
            _from: Option<&WorkspaceInfo>,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list(
            &self,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
            Ok(vec![])
        }
        async fn remove(
            &self,
            _info: &WorkspaceInfo,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn target(
            &self,
            _info: &WorkspaceInfo,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<Target> {
            Ok(Target::Local {
                directory: "/tmp".into(),
            })
        }
    }

    #[tokio::test]
    async fn context_carries_ambient_project_and_workspace() {
        let adapter: AdapterRef = Arc::new(FakeAdapter);
        let info = WorkspaceInfo::from_row(
            "wrk_1".into(),
            "fake".into(),
            "name".into(),
            None,
            None,
            None,
            "prj_1".into(),
        );
        InstanceContext::provide(Some("prj_1".into()), async {
            WorkspaceContext::provide(Some("wrk_1".into()), async {
                configure(&adapter, info).await.unwrap();
            })
            .await;
        })
        .await;
    }
}
