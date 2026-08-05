//! Project directory tracking.
//! From reference/packages/core/src/project/directories.ts
//!
//! TODO(integration): provide a SQLite-backed `ProjectDirectoryStore` in
//! oc-database matching `project/sql.ts` (`project_directory` table).

use std::sync::{Arc, Mutex};

use crate::ids::ProjectId;
use crate::schema::AbsolutePath;

/// `ProjectDirectories.Directory` — `{ directory, strategy? }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub directory: AbsolutePath,
    pub strategy: Option<String>,
}

/// `CreateInput.behavior`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
    Ignore,
    Replace,
}

/// `CreateInput`.
#[derive(Debug, Clone)]
pub struct CreateInput {
    pub project_id: ProjectId,
    pub directory: AbsolutePath,
    pub strategy: Option<String>,
    pub behavior: Option<Behavior>,
}

/// `RemoveInput`.
#[derive(Debug, Clone)]
pub struct RemoveInput {
    pub project_id: ProjectId,
    pub directory: AbsolutePath,
}

/// `ListInput`.
#[derive(Debug, Clone)]
pub struct ListInput {
    pub project_id: ProjectId,
}

#[derive(Debug, Clone)]
struct Row {
    project_id: String,
    directory: AbsolutePath,
    strategy: Option<String>,
    time_created: u64,
}

/// Storage seam for project directories.
pub trait ProjectDirectoryStore: Send + Sync {
    /// Mirrors `create(...)` — returns whether a row was written.
    fn create(&self, input: &CreateInput) -> bool;
    /// Mirrors `remove(...)` — returns whether a row was deleted.
    fn remove(&self, input: &RemoveInput) -> bool;
    /// `list(projectID)` — ordered by `time_created` desc then directory asc.
    fn list(&self, project_id: &ProjectId) -> Vec<Directory>;
    fn get(&self, project_id: &ProjectId, directory: &AbsolutePath) -> Option<Directory>;
    fn contains(&self, project_id: &ProjectId, directory: &AbsolutePath) -> bool;
}

/// In-memory project directory store.
#[derive(Debug, Default)]
pub struct InMemoryProjectDirectoryStore {
    rows: Mutex<Vec<Row>>,
}

impl InMemoryProjectDirectoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectDirectoryStore for InMemoryProjectDirectoryStore {
    fn create(&self, input: &CreateInput) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut rows = self.rows.lock().unwrap();
        let existing = rows
            .iter_mut()
            .find(|row| row.project_id == input.project_id.0 && row.directory == input.directory);
        match (existing, input.behavior) {
            (None, _) => {
                rows.push(Row {
                    project_id: input.project_id.0.clone(),
                    directory: input.directory.clone(),
                    strategy: input.strategy.clone(),
                    time_created: now,
                });
                true
            }
            // onConflictDoNothing for "ignore" and default.
            (Some(_), Some(Behavior::Ignore)) => false,
            (Some(_), None) => false,
            // onConflictDoUpdate with conditional strategy write.
            (Some(existing), Some(Behavior::Replace)) => match &input.strategy {
                Some(strategy) => {
                    let changed = existing.strategy.as_ref() != Some(strategy);
                    if changed {
                        existing.strategy = Some(strategy.clone());
                    }
                    changed
                }
                None => {
                    let changed = existing.strategy.is_some();
                    if changed {
                        existing.strategy = None;
                    }
                    changed
                }
            },
        }
    }

    fn remove(&self, input: &RemoveInput) -> bool {
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|row| row.project_id != input.project_id.0 || row.directory != input.directory);
        rows.len() != before
    }

    fn list(&self, project_id: &ProjectId) -> Vec<Directory> {
        let mut rows: Vec<Row> = self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.project_id == project_id.0)
            .cloned()
            .collect();
        // Ordering mirrors `orderBy(desc(time_created), asc(directory))`.
        rows.sort_by(|a, b| {
            b.time_created
                .cmp(&a.time_created)
                .then_with(|| a.directory.0.cmp(&b.directory.0))
        });
        rows.into_iter()
            .map(|row| Directory {
                directory: row.directory,
                strategy: row.strategy,
            })
            .collect()
    }

    fn get(&self, project_id: &ProjectId, directory: &AbsolutePath) -> Option<Directory> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .find(|row| row.project_id == project_id.0 && row.directory == *directory)
            .map(|row| Directory {
                directory: row.directory.clone(),
                strategy: row.strategy.clone(),
            })
    }

    fn contains(&self, project_id: &ProjectId, directory: &AbsolutePath) -> bool {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .any(|row| row.project_id == project_id.0 && row.directory == *directory)
    }
}

/// The project directories service (`@opencode/ProjectDirectories`).
#[derive(Clone)]
pub struct ProjectDirectoriesService {
    store: Arc<dyn ProjectDirectoryStore>,
}

impl ProjectDirectoriesService {
    pub fn new(store: Arc<dyn ProjectDirectoryStore>) -> Self {
        ProjectDirectoriesService { store }
    }

    pub fn with_store<S: ProjectDirectoryStore + 'static>(store: S) -> Self {
        ProjectDirectoriesService::new(Arc::new(store))
    }

    pub async fn list(&self, project_id: &ProjectId) -> Vec<Directory> {
        self.store.list(project_id)
    }

    pub async fn get(&self, project_id: &ProjectId, directory: &AbsolutePath) -> Option<Directory> {
        self.store.get(project_id, directory)
    }

    pub async fn contains(&self, project_id: &ProjectId, directory: &AbsolutePath) -> bool {
        self.store.contains(project_id, directory)
    }

    pub async fn create(&self, input: &CreateInput) -> bool {
        self.store.create(input)
    }

    pub async fn remove(&self, input: &RemoveInput) -> bool {
        self.store.remove(input)
    }
}
