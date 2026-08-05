//! Wires the oc-project services together into one runtime, mirroring the
//! reference's `app-node` layer composition for the project/worktree/snapshot
//! graph (reference/packages/opencode/src/project/instance-store.ts `node`).
//!
//! TODO(integration): the reference bootstraps LSP, Format, ShareNext and
//! Plugin services here; those live in other crates and are skipped for now.
use std::sync::Arc;

use crate::git::Git;
use crate::project::bootstrap::Bootstrap;
use crate::project::instance_context::InstanceContext;
use crate::project::instance_store::{InstanceStore, LoadInput};
use crate::project::project_impl::Project;
use crate::project::store::ProjectStore;
use crate::project::vcs::Vcs;
use crate::snapshot::Snapshot;
use crate::util::bus::Bus;
use crate::util::config::Config;
use crate::worktree::Worktree;

#[derive(Clone)]
pub struct Runtime {
    pub bus: Arc<Bus>,
    pub config: Arc<Config>,
    pub git: Arc<Git>,
    pub project: Arc<Project>,
    pub vcs: Arc<Vcs>,
    pub snapshot: Arc<Snapshot>,
    pub bootstrap: Arc<Bootstrap>,
    pub instance_store: InstanceStore,
    pub worktree: Arc<Worktree>,
}

impl Runtime {
    pub fn new(config: Config) -> Runtime {
        let bus = Arc::new(Bus::new());
        let config = Arc::new(config);
        let git = Arc::new(Git);
        let store = Arc::new(ProjectStore::new());
        let project = Project::new(git.clone(), store, config.clone(), bus.clone());
        let vcs = Vcs::new(git.clone(), bus.clone());
        let snapshot = Snapshot::new(config.clone());
        let bootstrap = Bootstrap::new(
            config.clone(),
            project.clone(),
            vcs.clone(),
            snapshot.clone(),
        );
        let instance_store = InstanceStore::new(project.clone(), bootstrap.clone(), bus.clone());
        let worktree = Worktree::new(
            git.clone(),
            project.clone(),
            instance_store.clone(),
            bus.clone(),
        );
        Runtime {
            bus,
            config,
            git,
            project,
            vcs,
            snapshot,
            bootstrap,
            instance_store,
            worktree,
        }
    }

    /// Boots the instance for `directory` (Project.fromDirectory + bootstrap).
    pub async fn load(&self, directory: &str) -> Result<InstanceContext, String> {
        self.instance_store
            .load(LoadInput::directory(directory))
            .await
    }

    pub async fn dispose(&self, ctx: &InstanceContext) {
        self.instance_store.dispose(ctx).await;
    }

    pub async fn dispose_all(&self) {
        self.instance_store.dispose_all().await;
    }
}
