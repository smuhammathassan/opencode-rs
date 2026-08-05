/// Async filesystem helpers ported from `FSUtil` (`@opencode-ai/core/fs-util`).
/// All operations swallow errors where the reference uses `Effect.catch` to
/// succeed with a fallback value.
///
/// TODO(integration): move to oc-util / oc-core once those crates expose
/// FSUtil; this is a local subset for the oc-project port.
use std::path::Path;

use tokio::io::AsyncWriteExt;

pub async fn exists(path: &str) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

pub async fn is_dir(path: &str) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

pub async fn is_file(path: &str) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

pub async fn ensure_dir(path: &str) -> std::io::Result<()> {
    tokio::fs::create_dir_all(path).await
}

pub async fn read_to_string(path: &str) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_default()
}

pub async fn read_bytes(path: &str) -> Vec<u8> {
    tokio::fs::read(path).await.unwrap_or_default()
}

pub async fn write_string(path: &str, content: &str) -> std::io::Result<()> {
    let parent = Path::new(path).parent();
    if let Some(parent) = parent {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(path).await?;
    file.write_all(content.as_bytes()).await
}

pub async fn remove(path: &str) {
    let _ = tokio::fs::remove_file(path).await;
    let _ = tokio::fs::remove_dir_all(path).await;
}

/// Like the reference's `remove` but never errors and supports recursive
/// removal.
pub async fn remove_recursive(path: &str) {
    let _ = tokio::fs::remove_dir_all(path).await;
    let _ = tokio::fs::remove_file(path).await;
}

pub async fn copy_file(from: &str, to: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(to).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::copy(from, to).await.map(|_| ())
}

/// File size in bytes; returns `None` for directories or missing files.
pub async fn file_size(path: &str) -> Option<u64> {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.is_file() => Some(meta.len()),
        _ => None,
    }
}

pub async fn realpath(path: &str) -> Option<String> {
    tokio::fs::canonicalize(path)
        .await
        .ok()
        .map(|value| value.to_string_lossy().into_owned())
}

pub async fn make_dir_recursive(path: &str) -> std::io::Result<()> {
    tokio::fs::create_dir_all(path).await
}

/// Recursive glob for `**/*.ext` style patterns with a set of extensions,
/// used by `Project.discover` for favicons.
pub fn glob_files(root: &str, extensions: &[&str]) -> Vec<String> {
    let mut matches = Vec::new();
    let mut stack = vec![Path::new(root).to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if extensions
                        .iter()
                        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                    {
                        matches.push(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    matches
}
