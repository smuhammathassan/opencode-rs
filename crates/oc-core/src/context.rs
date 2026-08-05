//! Service graph construction.
//!
//! From reference/packages/core/src/effect/app-node.ts +
//! effect/layer-node.ts + effect/app-node-builder.ts.
//!
//! The reference builds a typed dependency graph of `LayerNode`s and compiles
//! it into an Effect `Layer`. This module constructs the same set of services
//! in dependency order and exposes them for shared use, with seams to inject
//! alternative stores (durable events, credentials, project directories).

use std::sync::Arc;

use crate::agent::AgentService;
use crate::background_job::BackgroundJob;
use crate::bus::EventBus;
use crate::catalog::CatalogService;
use crate::command::CommandService;
use crate::credential::{CredentialService, CredentialStore};
use crate::durable::DurableStore;
use crate::event::DurableRegistry;
use crate::file_mutation::FileMutationService;
use crate::fs_util::FSUtilService;
use crate::git::GitService;
use crate::integration::IntegrationService;
use crate::policy::PolicyService;
use crate::project::directories::{
    InMemoryProjectDirectoryStore, ProjectDirectoriesService, ProjectDirectoryStore,
};
use crate::project::ProjectService;

/// The full service graph, mirroring `AppNodeBuilder.build(...)` with all core
/// nodes.
pub struct Services {
    pub fs: Arc<FSUtilService>,
    pub bus: Arc<EventBus>,
    pub registry: Arc<DurableRegistry>,
    pub jobs: Arc<BackgroundJob>,
    pub git: Arc<GitService>,
    pub project_directories: Arc<ProjectDirectoriesService>,
    pub project: Arc<ProjectService>,
    pub credential: Arc<CredentialService>,
    pub file_mutation: Arc<FileMutationService>,
    pub agent: Arc<AgentService>,
    pub command: Arc<CommandService>,
    pub policy: Arc<PolicyService>,
    pub integrations: Arc<IntegrationService>,
    pub catalog: Arc<CatalogService>,
}

impl Services {
    /// Construct the graph with in-memory stores.
    pub fn build() -> Self {
        Self::build_with(
            Arc::new(crate::durable::InMemoryDurableStore::new()),
            None,
            None,
        )
    }

    /// Construct the graph, optionally replacing the durable store and the
    /// credential / project-directory stores.
    pub fn build_with(
        durable_store: Arc<dyn DurableStore>,
        credential_store: Option<Arc<dyn CredentialStore>>,
        directory_store: Option<Arc<dyn ProjectDirectoryStore>>,
    ) -> Self {
        let fs = Arc::new(FSUtilService::default());
        let registry = Arc::new(DurableRegistry::default());
        let bus = Arc::new(EventBus::new(durable_store, registry.clone()));
        let jobs = Arc::new(BackgroundJob::new());
        let git = Arc::new(GitService::new(fs.clone()));
        let project_directories = Arc::new(ProjectDirectoriesService::new(
            directory_store.unwrap_or_else(|| Arc::new(InMemoryProjectDirectoryStore::new())),
        ));
        let project = Arc::new(ProjectService::new(
            fs.clone(),
            git.clone(),
            project_directories.clone(),
        ));
        let credential = Arc::new(CredentialService::new(
            credential_store
                .unwrap_or_else(|| Arc::new(crate::credential::InMemoryCredentialStore::new())),
        ));
        let file_mutation = Arc::new(FileMutationService::new(fs.clone()));
        let agent = Arc::new(AgentService::new());
        let command = Arc::new(CommandService::new());
        let policy = Arc::new(PolicyService::new());
        let integrations = Arc::new(IntegrationService::new());
        let catalog = Arc::new(CatalogService::new(
            bus.clone(),
            policy.clone(),
            integrations.clone(),
        ));

        Services {
            fs,
            bus,
            registry,
            jobs,
            git,
            project_directories,
            project,
            credential,
            file_mutation,
            agent,
            command,
            policy,
            integrations,
            catalog,
        }
    }
}

impl Default for Services {
    fn default() -> Self {
        Self::build()
    }
}
