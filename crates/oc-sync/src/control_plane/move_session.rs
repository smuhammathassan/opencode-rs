//! Move session: relocate a session between directories / projects.
//!
//! From reference/packages/core/src/control-plane/move-session.ts. The reference
//! depends on `Git`, `EventV2`, `ProjectV2`, and `SessionStore`; the port keeps
//! the orchestration over trait dependencies so it stays testable until those
//! crates land.
//!
//! TODO(integration): back `GitOps`/`ProjectOps`/`MoveSessionStore` with
//! oc-project / oc-session, and `MoveSession`'s store with `sync::store`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::sync::event::{Definition, LocationRef};
use crate::sync::store::{Store, StoreError};

/// `MoveSession.Destination` from the reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Destination {
    pub directory: String,
}

/// `MoveSession.Input` from the reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub session_id: String,
    pub destination: Destination,
    pub move_changes: Option<bool>,
}

/// A Git repository handle (`Git.Repository`).
#[derive(Debug, Clone, PartialEq)]
pub struct GitRepository {
    pub path: String,
}

/// The subset of the `Git` service used by move-session
/// (reference/packages/core/src/git.ts).
#[async_trait::async_trait]
pub trait GitOps: Send + Sync {
    async fn discover(&self, directory: &str) -> Option<GitRepository>;
    /// `git.change.capture` — returns the ChangeSet patch.
    async fn capture(&self, repository: &GitRepository, path: &str) -> Result<String, String>;
    /// `git.change.apply`.
    async fn apply(
        &self,
        repository: &GitRepository,
        path: &str,
        patch: &str,
    ) -> Result<(), String>;
    /// `git.change.discard` with `index`/`untracked` options.
    async fn discard(&self, repository: &GitRepository, path: &str) -> Result<(), String>;
}

/// A session as read by move-session (`SessionV2`).
#[derive(Debug, Clone, PartialEq)]
pub struct MovedSession {
    pub session_id: String,
    pub location_directory: String,
    pub project_id: String,
}

/// The `SessionStore.get` surface used here.
#[async_trait::async_trait]
pub trait MoveSessionStore: Send + Sync {
    async fn get(&self, session_id: &str) -> Option<MovedSession>;
}

/// `ProjectV2.resolve` output.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProject {
    pub id: String,
    pub directory: String,
}

/// The `ProjectV2.resolve` surface used here.
#[async_trait::async_trait]
pub trait ProjectOps: Send + Sync {
    async fn resolve(&self, directory: &str) -> ResolvedProject;
}

/// `MoveSession.DestinationProjectMismatchError` from the reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("destination project mismatch: expected {expected}, got {actual}")]
pub struct DestinationProjectMismatchError {
    pub expected: String,
    pub actual: String,
}

/// `MoveSession.ApplyChangesError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ApplyChangesError {
    pub message: String,
}

/// `MoveSession.CaptureChangesError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct CaptureChangesError {
    pub message: String,
}

/// `MoveSession.ResetSourceChangesError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ResetSourceChangesError {
    pub directory: String,
    pub message: String,
}

/// The `MoveSession.Error` union from the reference.
#[derive(Debug, thiserror::Error)]
pub enum MoveSessionError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    DestinationProjectMismatch(#[from] DestinationProjectMismatchError),
    #[error(transparent)]
    ApplyChanges(#[from] ApplyChangesError),
    #[error(transparent)]
    CaptureChanges(#[from] CaptureChangesError),
    #[error(transparent)]
    ResetSourceChanges(#[from] ResetSourceChangesError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The `session.next.moved` durable event definition
/// (reference/packages/schema/src/session-event.ts).
pub fn moved_definition() -> Definition {
    Definition::durable("session.next.moved", "sessionID", 1)
}

#[derive(Clone)]
pub struct MoveSession {
    store: Store,
    git: Arc<dyn GitOps>,
    sessions: Arc<dyn MoveSessionStore>,
    project: Arc<dyn ProjectOps>,
}

impl MoveSession {
    pub fn new(
        store: Store,
        git: Arc<dyn GitOps>,
        sessions: Arc<dyn MoveSessionStore>,
        project: Arc<dyn ProjectOps>,
    ) -> Self {
        Self {
            store,
            git,
            sessions,
            project,
        }
    }

    /// `moveSession` from the reference.
    pub async fn move_session(&self, input: Input) -> Result<(), MoveSessionError> {
        let Some(current) = self.sessions.get(&input.session_id).await else {
            return Err(MoveSessionError::NotFound(input.session_id));
        };
        let directory = input.destination.directory.clone();
        if current.location_directory == directory {
            return Ok(());
        }

        let source = self.project.resolve(&current.location_directory).await;
        let destination = self.project.resolve(&directory).await;
        if current.project_id != destination.id {
            return Err(DestinationProjectMismatchError {
                expected: current.project_id.clone(),
                actual: destination.id.clone(),
            }
            .into());
        }

        let move_changes =
            input.move_changes.unwrap_or(false) && source.directory != destination.directory;
        let source_repository = if move_changes {
            self.git.discover(&current.location_directory).await
        } else {
            None
        };
        if move_changes && source_repository.is_none() {
            return Err(CaptureChangesError {
                message: "Source is not a Git repository".to_string(),
            }
            .into());
        }

        let patch = match &source_repository {
            Some(repository) => self
                .git
                .capture(repository, &current.location_directory)
                .await
                .map_err(|message| CaptureChangesError { message })?,
            None => String::new(),
        };

        if !patch.is_empty() {
            let repository =
                self.git
                    .discover(&directory)
                    .await
                    .ok_or_else(|| ApplyChangesError {
                        message: "Destination is not a Git repository".to_string(),
                    })?;
            self.git
                .apply(&repository, &directory, &patch)
                .await
                .map_err(|message| ApplyChangesError { message })?;
        }

        let timestamp = now_ms();
        self.store
            .publish(
                &moved_definition(),
                serde_json::json!({
                    "timestamp": timestamp,
                    "sessionID": input.session_id,
                    "location": LocationRef {
                        directory: directory.clone(),
                        workspace_id: None,
                    },
                    "subdirectory": relative_path(&destination.directory, &directory),
                }),
                Default::default(),
            )
            .map_err(MoveSessionError::Store)?;

        if !patch.is_empty() {
            let repository = self
                .git
                .discover(&current.location_directory)
                .await
                .ok_or_else(|| ResetSourceChangesError {
                    directory: current.location_directory.clone(),
                    message: "Source is not a Git repository".to_string(),
                })?;
            self.git
                .discard(&repository, &current.location_directory)
                .await
                .map_err(|message| ResetSourceChangesError {
                    directory: current.location_directory.clone(),
                    message,
                })?;
        }

        Ok(())
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

/// `path.relative(destination.directory, directory).replaceAll("\\", "/")` from
/// the reference.
fn relative_path(from: &str, to: &str) -> String {
    let from: Vec<&str> = from
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let to: Vec<&str> = to
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }
    let mut parts = Vec::new();
    for _ in common..from.len() {
        parts.push("..");
    }
    for part in &to[common..] {
        parts.push(*part);
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemStore(Arc<std::sync::Mutex<Vec<MovedSession>>>);

    #[async_trait::async_trait]
    impl MoveSessionStore for MemStore {
        async fn get(&self, session_id: &str) -> Option<MovedSession> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.session_id == session_id)
                .cloned()
        }
    }

    struct FakeGit;

    #[async_trait::async_trait]
    impl GitOps for FakeGit {
        async fn discover(&self, directory: &str) -> Option<GitRepository> {
            if directory.contains("nogit") {
                None
            } else {
                Some(GitRepository {
                    path: format!("{directory}/.git"),
                })
            }
        }
        async fn capture(
            &self,
            _repository: &GitRepository,
            _path: &str,
        ) -> Result<String, String> {
            Ok("PATCH".to_string())
        }
        async fn apply(
            &self,
            _repository: &GitRepository,
            _path: &str,
            _patch: &str,
        ) -> Result<(), String> {
            Ok(())
        }
        async fn discard(&self, _repository: &GitRepository, _path: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeProject;

    #[async_trait::async_trait]
    impl ProjectOps for FakeProject {
        async fn resolve(&self, directory: &str) -> ResolvedProject {
            ResolvedProject {
                id: "prj_1".to_string(),
                directory: directory.to_string(),
            }
        }
    }

    #[allow(dead_code)]
    fn service(sessions: Vec<MovedSession>) -> MoveSession {
        MoveSession::new(
            Store::new(),
            Arc::new(FakeGit),
            Arc::new(MemStore(Arc::new(std::sync::Mutex::new(sessions)))),
            Arc::new(FakeProject),
        )
    }

    fn session() -> MovedSession {
        MovedSession {
            session_id: "ses_1".into(),
            location_directory: "/src/old".into(),
            project_id: "prj_1".into(),
        }
    }

    #[tokio::test]
    async fn publishes_moved_event_on_success() {
        let store = Store::new();
        let moved = MoveSession::new(
            store.clone(),
            Arc::new(FakeGit),
            Arc::new(MemStore(Arc::new(std::sync::Mutex::new(vec![session()])))),
            Arc::new(FakeProject),
        );
        moved
            .move_session(Input {
                session_id: "ses_1".into(),
                destination: Destination {
                    directory: "/src/new".into(),
                },
                move_changes: Some(true),
            })
            .await
            .unwrap();

        let history = store.history("ses_1");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].r#type, "session.next.moved.1");
        assert_eq!(history[0].data["sessionID"], serde_json::json!("ses_1"));
        assert_eq!(
            history[0].data["location"]["directory"],
            serde_json::json!("/src/new")
        );
        assert_eq!(history[0].data["subdirectory"], serde_json::json!(""));
    }

    #[tokio::test]
    async fn noop_when_directory_unchanged() {
        let store = Store::new();
        let moved = MoveSession::new(
            store.clone(),
            Arc::new(FakeGit),
            Arc::new(MemStore(Arc::new(std::sync::Mutex::new(vec![session()])))),
            Arc::new(FakeProject),
        );
        moved
            .move_session(Input {
                session_id: "ses_1".into(),
                destination: Destination {
                    directory: "/src/old".into(),
                },
                move_changes: None,
            })
            .await
            .unwrap();
        assert!(store.history("ses_1").is_empty());
    }

    #[tokio::test]
    async fn missing_session_is_not_found() {
        let store = Store::new();
        let moved = MoveSession::new(
            store.clone(),
            Arc::new(FakeGit),
            Arc::new(MemStore(Arc::new(std::sync::Mutex::new(vec![])))),
            Arc::new(FakeProject),
        );
        let err = moved
            .move_session(Input {
                session_id: "ses_9".into(),
                destination: Destination {
                    directory: "/src/new".into(),
                },
                move_changes: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, MoveSessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn project_mismatch_is_rejected() {
        #[derive(Clone)]
        struct OtherProject;
        #[async_trait::async_trait]
        impl ProjectOps for OtherProject {
            async fn resolve(&self, directory: &str) -> ResolvedProject {
                ResolvedProject {
                    id: if directory == "/src/new" {
                        "prj_2".into()
                    } else {
                        "prj_1".into()
                    },
                    directory: directory.to_string(),
                }
            }
        }
        let moved = MoveSession::new(
            Store::new(),
            Arc::new(FakeGit),
            Arc::new(MemStore(Arc::new(std::sync::Mutex::new(vec![session()])))),
            Arc::new(OtherProject),
        );
        let err = moved
            .move_session(Input {
                session_id: "ses_1".into(),
                destination: Destination {
                    directory: "/src/new".into(),
                },
                move_changes: None,
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, MoveSessionError::DestinationProjectMismatch(e) if e.expected == "prj_1" && e.actual == "prj_2")
        );
    }

    #[test]
    fn relative_path_computation() {
        assert_eq!(relative_path("/a/b", "/a/b"), "");
        assert_eq!(relative_path("/a/b", "/a/b/c"), "c");
        assert_eq!(relative_path("/a/b", "/a/c"), "../c");
        assert_eq!(relative_path("/a/b/c", "/a/b"), "..");
    }
}
