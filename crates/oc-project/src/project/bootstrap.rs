/// From reference/packages/opencode/src/project/bootstrap.ts
///
/// `InstanceBootstrap.run` eagerly loads config and fires each per-instance
/// service `init` in the background. In the reference this also initializes
/// LSP, Format, ShareNext and Plugin — those live in other crates.
///
/// TODO(integration): wire `lsp.init`, `format.init`, `shareNext.init`,
/// `plugin.init` once oc-lsp/oc-command/oc-plugin land.
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::project::instance_context::InstanceContext;
use crate::project::project::Project;
use crate::project::vcs::Vcs;
use crate::snapshot::Snapshot;
use crate::util::config::Config;

pub struct Bootstrap {
    pub config: Arc<Config>,
    pub project: Arc<Project>,
    pub vcs: Arc<Vcs>,
    pub snapshot: Arc<Snapshot>,
}

impl Bootstrap {
    pub fn new(
        config: Arc<Config>,
        project: Arc<Project>,
        vcs: Arc<Vcs>,
        snapshot: Arc<Snapshot>,
    ) -> Arc<Bootstrap> {
        Arc::new(Bootstrap { config, project, vcs, snapshot })
    }

    pub async fn run(&self, ctx: &InstanceContext, shutdown: broadcast::Sender<()>) -> Vec<tokio::task::JoinHandle<()>> {
        tracing::info!("bootstrapping");
        // Config is eagerly materialized for nice traces in the reference.
        let _ = self.config.clone();
        // Plugin can mutate config so it is initialized before anything else.
        // TODO(integration): `plugin.init()`

        let sender = shutdown;
        let mut handles = Vec::new();

        let project = self.project.clone();
        let project_ctx = ctx.clone();
        let shutdown = sender.subscribe();
        handles.push(tokio::spawn(async move {
            let _ = project.init(project_ctx, shutdown).await;
        }));

        let vcs = self.vcs.clone();
        let vcs_ctx = ctx.clone();
        let shutdown = sender.subscribe();
        handles.push(tokio::spawn(async move {
            let _ = vcs.init(vcs_ctx, shutdown).await;
        }));

        // Snapshot init starts its hourly cleanup loop.
        let snapshot = self.snapshot.clone();
        let snapshot_ctx = ctx.clone();
        let shutdown = sender.subscribe();
        handles.push(tokio::spawn(async move {
            let _ = snapshot.init(snapshot_ctx, shutdown).await;
        }));

        handles
    }

    /// Runs per-instance disposers on instance teardown.
    pub async fn on_dispose(&self, directory: &str) {
        self.vcs.on_dispose(directory);
        self.snapshot.on_dispose(directory);
    }
}
