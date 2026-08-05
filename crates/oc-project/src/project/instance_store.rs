/// From reference/packages/opencode/src/project/instance-store.ts
///
/// Boots project instances (one per directory), memoizing in-flight boots so
/// concurrent `load` calls share a single initialization. `dispose` tears the
/// instance down and runs its disposers.
use std::sync::Arc;
use std::sync::OnceLock;

use tokio::sync::{broadcast, Notify};

use crate::project::bootstrap::Bootstrap;
use crate::project::instance_context::InstanceContext;
use crate::project::project::Project;
use crate::schema::ProjectInfo;
use crate::util::bus::{Bus, BusEvent, EventPayload};
use crate::util::pathutil;

pub const INSTANCE_DISPOSED: &str = "server.instance.disposed";

#[derive(Debug, Clone)]
pub struct LoadInput {
    pub directory: String,
    pub worktree: Option<String>,
    pub project: Option<ProjectInfo>,
}

impl LoadInput {
    pub fn directory(directory: impl Into<String>) -> LoadInput {
        LoadInput {
            directory: directory.into(),
            worktree: None,
            project: None,
        }
    }
}

#[derive(Clone)]
struct Entry {
    result: Arc<OnceLock<Result<InstanceContext, String>>>,
    notify: Arc<Notify>,
    shutdown: broadcast::Sender<()>,
}

impl Entry {
    fn new() -> Entry {
        let (shutdown, _) = broadcast::channel(1);
        Entry {
            result: Arc::new(OnceLock::new()),
            notify: Arc::new(Notify::new()),
            shutdown,
        }
    }

    async fn await_result(&self) -> Result<InstanceContext, String> {
        loop {
            if let Some(result) = self.result.get() {
                return result.clone();
            }
            self.notify.notified().await;
        }
    }

    fn complete(&self, result: Result<InstanceContext, String>) {
        let _ = self.result.set(result);
        self.notify.notify_waiters();
    }
}

#[derive(Clone)]
pub struct InstanceStore {
    pub project: Arc<Project>,
    pub bootstrap: Arc<Bootstrap>,
    pub bus: Arc<Bus>,
    cache: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Entry>>>,
}

impl InstanceStore {
    pub fn new(project: Arc<Project>, bootstrap: Arc<Bootstrap>, bus: Arc<Bus>) -> InstanceStore {
        InstanceStore {
            project,
            bootstrap,
            bus,
            cache: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub async fn load(&self, input: LoadInput) -> Result<InstanceContext, String> {
        let directory = pathutil::resolve(&input.directory);
        let existing = { self.cache.lock().await.get(&directory).cloned() };
        if let Some(entry) = existing {
            return entry.await_result().await;
        }

        let entry = Entry::new();
        {
            let mut cache = self.cache.lock().await;
            if let Some(existing) = cache.get(&directory).cloned() {
                drop(cache);
                return existing.await_result().await;
            }
            cache.insert(directory.clone(), entry.clone());
        }

        let store = self.clone();
        let input = LoadInput {
            directory: directory.clone(),
            ..input
        };
        let shutdown = entry.shutdown.clone();
        let task_entry = entry.clone();
        tokio::spawn(async move {
            let result = store.boot(&input, shutdown).await;
            if result.is_err() {
                store.remove_if_current(&directory, &task_entry).await;
            }
            task_entry.complete(result);
        });
        entry.await_result().await
    }

    pub async fn reload(&self, input: LoadInput) -> Result<InstanceContext, String> {
        let directory = pathutil::resolve(&input.directory);
        let previous = self.cache.lock().await.get(&directory).cloned();
        let entry = Entry::new();
        self.cache
            .lock()
            .await
            .insert(directory.clone(), entry.clone());

        let store = self.clone();
        let input = LoadInput {
            directory: directory.clone(),
            ..input
        };
        let shutdown = entry.shutdown.clone();
        let task_entry = entry.clone();
        tokio::spawn(async move {
            if let Some(previous) = previous {
                let _ = previous.await_result().await;
                let _ = previous.shutdown.send(());
                store.bootstrap.on_dispose(&directory).await;
                let project = input.project.as_ref().map(|project| project.id.0.as_str());
                store.emit_disposed(&directory, project);
            }
            let result = store.boot(&input, shutdown).await;
            if result.is_err() {
                store.remove_if_current(&directory, &task_entry).await;
            }
            task_entry.complete(result);
        });
        entry.await_result().await
    }

    pub async fn dispose(&self, ctx: &InstanceContext) {
        let entry = self.cache.lock().await.get(&ctx.directory).cloned();
        let Some(entry) = entry else {
            self.dispose_context(ctx).await;
            return;
        };
        let exit = entry.await_result().await;
        match exit {
            Err(_) => {
                self.remove_if_current(&ctx.directory, &entry).await;
            }
            Ok(value) => {
                if &value != ctx {
                    return;
                }
                self.dispose_entry(&ctx.directory, &entry, &ctx).await;
            }
        }
    }

    pub async fn dispose_directory(&self, input: &str) {
        let directory = pathutil::resolve(input);
        let entry = self.cache.lock().await.get(&directory).cloned();
        let Some(entry) = entry else { return };
        let exit = entry.await_result().await;
        match exit {
            Err(_) => {
                self.remove_if_current(&directory, &entry).await;
            }
            Ok(value) => {
                self.dispose_entry(&directory, &entry, &value).await;
            }
        }
    }

    pub async fn dispose_all(&self) {
        tracing::info!("disposing all instances");
        let entries: Vec<(String, Entry)> = self
            .cache
            .lock()
            .await
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for (directory, entry) in entries {
            let exit = entry.await_result().await;
            match exit {
                Err(error) => {
                    tracing::warn!("instance dispose failed: {error}");
                    self.remove_if_current(&directory, &entry).await;
                }
                Ok(value) => {
                    self.dispose_entry(&directory, &entry, &value).await;
                }
            }
        }
    }

    pub async fn provide<T>(
        &self,
        input: LoadInput,
        effect: impl FnOnce(&InstanceContext) -> T,
    ) -> Result<T, String> {
        let ctx = self.load(input).await?;
        Ok(effect(&ctx))
    }

    async fn boot(
        &self,
        input: &LoadInput,
        shutdown: broadcast::Sender<()>,
    ) -> Result<InstanceContext, String> {
        let ctx = if let (Some(project), Some(worktree)) = (&input.project, &input.worktree) {
            InstanceContext {
                directory: input.directory.clone(),
                worktree: worktree.clone(),
                project: project.clone(),
            }
        } else {
            let result = self
                .project
                .from_directory(&input.directory)
                .await
                .map_err(|error| error.to_string())?;
            InstanceContext {
                directory: input.directory.clone(),
                worktree: result.sandbox,
                project: result.project,
            }
        };
        self.bootstrap.run(&ctx, shutdown).await;
        Ok(ctx)
    }

    async fn dispose_context(&self, ctx: &InstanceContext) {
        tracing::info!("disposing instance: {}", ctx.directory);
        self.bootstrap.on_dispose(&ctx.directory).await;
        self.emit_disposed(&ctx.directory, Some(&ctx.project.id.0));
    }

    async fn dispose_entry(&self, directory: &str, entry: &Entry, ctx: &InstanceContext) -> bool {
        if !self.is_current(directory, entry).await {
            return false;
        }
        let _ = entry.shutdown.send(());
        self.dispose_context(ctx).await;
        if !self.is_current(directory, entry).await {
            return false;
        }
        self.cache.lock().await.remove(directory);
        true
    }

    async fn remove_if_current(&self, directory: &str, entry: &Entry) {
        let mut cache = self.cache.lock().await;
        if let Some(existing) = cache.get(directory) {
            if Arc::ptr_eq(&existing.result, &entry.result) {
                cache.remove(directory);
            }
        }
    }

    async fn is_current(&self, directory: &str, entry: &Entry) -> bool {
        let cache = self.cache.lock().await;
        cache
            .get(directory)
            .map(|existing| Arc::ptr_eq(&existing.result, &entry.result))
            .unwrap_or(false)
    }

    fn emit_disposed(&self, directory: &str, project: Option<&str>) {
        let workspace = std::env::var("OPENCODE_WORKSPACE_ID").ok();
        self.bus.emit(BusEvent {
            directory: directory.to_string(),
            project: project.map(String::from),
            workspace,
            payload: EventPayload {
                r#type: INSTANCE_DISPOSED.to_string(),
                properties: Some(serde_json::json!({ "directory": directory })),
                data: None,
                location: None,
            },
        });
    }
}
