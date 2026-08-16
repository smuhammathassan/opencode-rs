//! Builtin console workspace adapter.

use std::collections::BTreeMap;

use super::super::types::{
    Target, WorkspaceAdapter, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo,
};

/// Console workspaces use the same remote target contract while the account
/// lifecycle is supplied by the console service at a higher layer.
pub struct ConsoleAdapter;

#[async_trait::async_trait]
impl WorkspaceAdapter for ConsoleAdapter {
    fn name(&self) -> &'static str {
        "Console"
    }
    fn description(&self) -> &'static str {
        "Connect to a console workspace"
    }

    async fn configure(
        &self,
        info: WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<WorkspaceInfo> {
        let adapter = super::remote::RemoteAdapter;
        adapter.configure(info, context).await
    }

    async fn create(
        &self,
        info: &WorkspaceInfo,
        env: &BTreeMap<String, Option<String>>,
        from: Option<&WorkspaceInfo>,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let adapter = super::remote::RemoteAdapter;
        adapter.create(info, env, from, context).await
    }

    async fn list(
        &self,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
        let adapter = super::remote::RemoteAdapter;
        adapter.list(context).await
    }

    async fn remove(
        &self,
        info: &WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let adapter = super::remote::RemoteAdapter;
        adapter.remove(info, context).await
    }

    async fn target(
        &self,
        info: &WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Target> {
        let adapter = super::remote::RemoteAdapter;
        adapter.target(info, context).await
    }
}

pub fn console_adapter() -> std::sync::Arc<dyn WorkspaceAdapter> {
    std::sync::Arc::new(ConsoleAdapter)
}
