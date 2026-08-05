//! File mutation service.
//!
//! From reference/packages/core/src/file-mutation.ts.
//! Serializes file changes by canonical target; conditional writes compare and
//! write under the same lock so cooperating mutations do not overwrite each
//! other.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::fs_util::{FSUtilService, FsError};
use crate::keyed_mutex::KeyedMutex;

/// `FileMutation.Target`.
pub struct Target {
    pub canonical: String,
    pub resource: String,
}

/// `FileMutation.StaleContentError` — `_tag: "FileMutation.StaleContentError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleContentError {
    pub _tag: String,
    pub path: String,
}

/// `FileMutation.TargetExistsError` — `_tag: "FileMutation.TargetExistsError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetExistsError {
    pub _tag: String,
    pub path: String,
}

impl std::fmt::Display for StaleContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stale content for {}", self.path)
    }
}

impl std::error::Error for StaleContentError {}

impl std::fmt::Display for TargetExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "target exists: {}", self.path)
    }
}

impl std::error::Error for TargetExistsError {}

/// `FileMutation.WriteResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteResult {
    pub operation: String,
    pub target: String,
    pub resource: String,
    pub existed: bool,
}

/// `FileMutation.RemoveResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveResult {
    pub operation: String,
    pub target: String,
    pub resource: String,
    pub existed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FileMutationError {
    #[error(transparent)]
    Fs(#[from] FsError),
    #[error(transparent)]
    Stale(#[from] StaleContentError),
    #[error(transparent)]
    Exists(#[from] TargetExistsError),
}

/// The file mutation service (`@opencode/v2/FileMutation`).
#[derive(Clone)]
pub struct FileMutationService {
    fs: Arc<FSUtilService>,
    locks: Arc<KeyedMutex<String>>,
}

impl FileMutationService {
    pub fn new(fs: Arc<FSUtilService>) -> Self {
        FileMutationService {
            fs,
            locks: Arc::new(KeyedMutex::make()),
        }
    }

    fn write_result(target: &Target, existed: bool) -> WriteResult {
        WriteResult {
            operation: "write".to_string(),
            target: target.canonical.clone(),
            resource: target.resource.clone(),
            existed,
        }
    }

    fn remove_result(target: &Target, existed: bool) -> RemoveResult {
        RemoveResult {
            operation: "remove".to_string(),
            target: target.canonical.clone(),
            resource: target.resource.clone(),
            existed,
        }
    }

    /// `write(input)` — overwrite without BOM handling.
    pub async fn write(&self, target: &Target, content: &[u8]) -> Result<WriteResult, FsError> {
        self.locks
            .with_lock(target.canonical.clone(), async {
                let existed = self.fs.exists(&target.canonical).await;
                self.fs.write_with_dirs(&target.canonical, content).await?;
                Ok(Self::write_result(target, existed))
            })
            .await
    }

    /// `create(input)` — create without replacing an existing target.
    pub async fn create(
        &self,
        target: &Target,
        content: &[u8],
    ) -> Result<WriteResult, FileMutationError> {
        self.locks
            .with_lock(target.canonical.clone(), async {
                match tokio::fs::write(&target.canonical, content).await {
                    Ok(()) => Ok(Self::write_result(target, false)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if let Some(parent) = Path::new(&target.canonical).parent() {
                            self.fs.ensure_dir(&parent.display().to_string()).await?;
                        }
                        tokio::fs::write(&target.canonical, content)
                            .await
                            .map_err(|e| FsError::from_io("create", &target.canonical, e))?;
                        Ok(Self::write_result(target, false))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        Err(FileMutationError::Exists(TargetExistsError {
                            _tag: "FileMutation.TargetExistsError".to_string(),
                            path: target.canonical.clone(),
                        }))
                    }
                    Err(error) => Err(FileMutationError::Fs(FsError::from_io(
                        "create",
                        &target.canonical,
                        error,
                    ))),
                }
            })
            .await
    }

    /// `writeTextPreservingBom(input)` — write text retaining an existing
    /// UTF-8 BOM and emitting at most one BOM.
    pub async fn write_text_preserving_bom(
        &self,
        target: &Target,
        content: &str,
    ) -> Result<WriteResult, FsError> {
        self.locks
            .with_lock(target.canonical.clone(), async {
                let (next_bom, next_text) = split_bom(content);
                let current = match self.fs.read_file(&target.canonical).await {
                    Ok(bytes) => Some(bytes),
                    Err(FsError::NotFound { .. }) => None,
                    Err(error) => return Err(error),
                };
                let bom = next_bom
                    || current
                        .as_ref()
                        .map(|bytes| has_utf8_bom(bytes))
                        .unwrap_or(false);
                let joined = join_bom(&next_text, bom);
                self.fs
                    .write_with_dirs(&target.canonical, joined.as_bytes())
                    .await?;
                Ok(Self::write_result(target, current.is_some()))
            })
            .await
    }

    /// `writeIfUnchanged(input)` — commit only if the target still holds the
    /// expected bytes.
    pub async fn write_if_unchanged(
        &self,
        target: &Target,
        content: &[u8],
        expected: &[u8],
    ) -> Result<WriteResult, FileMutationError> {
        self.locks
            .with_lock(target.canonical.clone(), async {
                let current = self.fs.read_file(&target.canonical).await;
                let current = match current {
                    Ok(bytes) => bytes,
                    Err(FsError::NotFound { .. }) => Vec::new(),
                    Err(error) => return Err(FileMutationError::Fs(error)),
                };
                if !same_bytes(&current, expected) {
                    return Err(FileMutationError::Stale(StaleContentError {
                        _tag: "FileMutation.StaleContentError".to_string(),
                        path: target.canonical.clone(),
                    }));
                }
                self.fs.write_file(&target.canonical, content).await?;
                Ok(Self::write_result(target, true))
            })
            .await
    }

    /// `remove(input)`.
    pub async fn remove(&self, target: &Target) -> Result<RemoveResult, FsError> {
        self.locks
            .with_lock(target.canonical.clone(), async {
                let existed = match self.fs.remove(&target.canonical, false, false).await {
                    Ok(()) => true,
                    Err(FsError::NotFound { .. }) => false,
                    Err(error) => return Err(error),
                };
                Ok(Self::remove_result(target, existed))
            })
            .await
    }
}

fn split_bom(text: &str) -> (bool, String) {
    let stripped = text.trim_start_matches('\u{FEFF}').to_string();
    (stripped.len() != text.len(), stripped)
}

fn join_bom(text: &str, bom: bool) -> String {
    let (_, stripped) = split_bom(text);
    if bom {
        format!("\u{FEFF}{stripped}")
    } else {
        stripped
    }
}

fn has_utf8_bom(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xef && bytes[1] == 0xbb && bytes[2] == 0xbf
}

fn same_bytes(left: &[u8], right: &[u8]) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_split_and_join() {
        let (bom, text) = split_bom("\u{FEFF}\u{FEFF}hello");
        assert!(bom);
        assert_eq!(text, "hello");
        assert_eq!(join_bom("hello", true), "\u{FEFF}hello");
        assert_eq!(join_bom("\u{FEFF}hi", true), "\u{FEFF}hi");
        assert_eq!(join_bom("\u{FEFF}hi", false), "hi");
    }

    #[test]
    fn utf8_bom_detection() {
        assert!(has_utf8_bom(&[0xef, 0xbb, 0xbf, b'a']));
        assert!(!has_utf8_bom(b"abc"));
    }
}
