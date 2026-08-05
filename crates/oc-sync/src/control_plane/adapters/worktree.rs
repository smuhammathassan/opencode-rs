//! The builtin worktree adapter.
//!
//! From reference/packages/opencode/src/control-plane/adapters/worktree.ts. The
//! reference calls the `Worktree` service (reference/packages/opencode/src/worktree/,
//! oc-project scope); the port injects `WorktreeOps` so the adapter logic is
//! testable until oc-project lands.
//!
//! TODO(integration): wire `WorktreeOps` to the oc-project worktree service.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::types::{
    Target, WorkspaceAdapter, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo,
};

/// A worktree entry as produced by the worktree service.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeInfo {
    pub name: String,
    pub branch: Option<String>,
    pub directory: String,
}

/// The `Worktree` service surface used by the adapter
/// (reference/packages/opencode/src/worktree/).
#[async_trait::async_trait]
pub trait WorktreeOps: Send + Sync {
    async fn make_worktree_info(&self, detached: bool) -> anyhow::Result<WorktreeInfo>;
    async fn create_from_info(
        &self,
        name: &str,
        directory: &str,
        branch: Option<&str>,
    ) -> anyhow::Result<()>;
    async fn list(&self) -> anyhow::Result<Vec<WorktreeInfo>>;
    async fn remove(&self, directory: &str) -> anyhow::Result<()>;
}

/// `decodeWorktreeConfig` from the reference: requires `name` and `directory`,
/// `branch` is optional/nullable.
fn decode_worktree_config(info: &WorkspaceInfo) -> anyhow::Result<WorktreeConfig> {
    let name = if info.name.is_empty() {
        anyhow::bail!("Worktree config requires a name")
    } else {
        info.name.clone()
    };
    let directory = match &info.directory {
        Some(Some(directory)) if !directory.is_empty() => directory.clone(),
        _ => anyhow::bail!("Worktree config requires a directory"),
    };
    Ok(WorktreeConfig {
        name,
        branch: info.branch.as_ref().and_then(|b| b.clone()),
        directory,
    })
}

struct WorktreeConfig {
    name: String,
    branch: Option<String>,
    directory: String,
}

/// `WorktreeAdapter` from reference/packages/opencode/src/control-plane/adapters/worktree.ts.
pub struct WorktreeAdapter {
    worktree: Arc<dyn WorktreeOps>,
}

impl WorktreeAdapter {
    pub fn new(worktree: Arc<dyn WorktreeOps>) -> Self {
        Self { worktree }
    }
}

#[async_trait::async_trait]
impl WorkspaceAdapter for WorktreeAdapter {
    fn name(&self) -> &'static str {
        "Worktree"
    }

    fn description(&self) -> &'static str {
        "Create a git worktree"
    }

    async fn configure(
        &self,
        info: WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<WorkspaceInfo> {
        let next = self.worktree.make_worktree_info(true).await?;
        let mut info = info;
        info.name = next.name;
        info.directory = Some(Some(next.directory));
        Ok(info)
    }

    async fn create(
        &self,
        info: &WorkspaceInfo,
        _env: &BTreeMap<String, Option<String>>,
        _from: Option<&WorkspaceInfo>,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let config = decode_worktree_config(info)?;
        self.worktree
            .create_from_info(&config.name, &config.directory, config.branch.as_deref())
            .await
    }

    async fn list(
        &self,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
        let project_id = context
            .project_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Worktree adapter requires an instance context"))?;
        let list = self.worktree.list().await?;
        Ok(list
            .into_iter()
            .map(|info| WorkspaceListedInfo {
                ty: "worktree".into(),
                name: info.name,
                branch: Some(info.branch),
                directory: Some(Some(info.directory)),
                extra: Some(None),
                project_id: project_id.clone(),
            })
            .collect())
    }

    async fn remove(
        &self,
        info: &WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let config = decode_worktree_config(info)?;
        self.worktree.remove(&config.directory).await
    }

    async fn target(
        &self,
        info: &WorkspaceInfo,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Target> {
        let config = decode_worktree_config(info)?;
        Ok(Target::Local {
            directory: config.directory,
        })
    }
}

/// The default worktree adapter, erroring on worktree service calls until wired.
/// TODO(integration): remove once oc-project provides the service.
pub fn worktree_adapter() -> Arc<dyn WorkspaceAdapter> {
    struct Unwired;
    #[async_trait::async_trait]
    impl WorktreeOps for Unwired {
        async fn make_worktree_info(&self, _detached: bool) -> anyhow::Result<WorktreeInfo> {
            anyhow::bail!("Worktree adapter requires a worktree service")
        }
        async fn create_from_info(
            &self,
            _name: &str,
            _directory: &str,
            _branch: Option<&str>,
        ) -> anyhow::Result<()> {
            anyhow::bail!("Worktree adapter requires a worktree service")
        }
        async fn list(&self) -> anyhow::Result<Vec<WorktreeInfo>> {
            anyhow::bail!("Worktree adapter requires a worktree service")
        }
        async fn remove(&self, _directory: &str) -> anyhow::Result<()> {
            anyhow::bail!("Worktree adapter requires a worktree service")
        }
    }
    Arc::new(WorktreeAdapter::new(Arc::new(Unwired)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWorktree {
        next: WorktreeInfo,
        list: Vec<WorktreeInfo>,
    }

    #[async_trait::async_trait]
    impl WorktreeOps for FakeWorktree {
        async fn make_worktree_info(&self, _detached: bool) -> anyhow::Result<WorktreeInfo> {
            Ok(self.next.clone())
        }
        async fn create_from_info(
            &self,
            _name: &str,
            _directory: &str,
            _branch: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list(&self) -> anyhow::Result<Vec<WorktreeInfo>> {
            Ok(self.list.clone())
        }
        async fn remove(&self, _directory: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn adapter() -> WorktreeAdapter {
        WorktreeAdapter::new(Arc::new(FakeWorktree {
            next: WorktreeInfo {
                name: "crisp-planet".into(),
                branch: Some("feat".into()),
                directory: "/tmp/wt/crisp-planet".into(),
            },
            list: vec![WorktreeInfo {
                name: "existing".into(),
                branch: None,
                directory: "/tmp/wt/existing".into(),
            }],
        }))
    }

    fn info() -> WorkspaceInfo {
        WorkspaceInfo {
            id: "wrk_1".into(),
            ty: "worktree".into(),
            name: "ignored".into(),
            branch: Some(None),
            directory: Some(None),
            extra: None,
            project_id: "global".into(),
        }
    }

    #[tokio::test]
    async fn configure_overrides_name_and_directory() {
        let configured = adapter()
            .configure(info(), &WorkspaceAdapterContext::default())
            .await
            .unwrap();
        assert_eq!(configured.name, "crisp-planet");
        assert_eq!(
            configured.directory,
            Some(Some("/tmp/wt/crisp-planet".into()))
        );
    }

    #[tokio::test]
    async fn target_resolves_local_directory() {
        let mut configured = info();
        configured.directory = Some(Some("/tmp/wt/crisp-planet".into()));
        let target = adapter()
            .target(&configured, &WorkspaceAdapterContext::default())
            .await
            .unwrap();
        assert_eq!(
            target,
            Target::Local {
                directory: "/tmp/wt/crisp-planet".into()
            }
        );
    }

    #[tokio::test]
    async fn list_requires_instance_context() {
        let result = adapter().list(&WorkspaceAdapterContext::default()).await;
        assert!(result.is_err());
    }
}
