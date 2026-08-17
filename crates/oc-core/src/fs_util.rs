//! Filesystem service.
//!
//! From reference/packages/core/src/fs-util.ts.
//!
//! NOTE: per the workspace ownership table, `core/fs-util` ultimately belongs
//! to oc-util. It is ported here because oc-util is still a stub; move it
//! during integration.
//! TODO(integration): relocate core/fs-util mirror to oc-util.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::util::glob;

/// Error type approximating `FSUtil.FileSystemError | PlatformError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "_tag")]
pub enum FsError {
    #[serde(rename = "PlatformError")]
    NotFound {
        method: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "PlatformError")]
    AlreadyExists {
        method: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "PlatformError")]
    PermissionDenied {
        method: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "FileSystemError")]
    Other {
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

impl FsError {
    pub(crate) fn from_io(method: &str, path: &str, error: std::io::Error) -> FsError {
        match error.kind() {
            std::io::ErrorKind::NotFound => FsError::NotFound {
                method: method.to_string(),
                path: path.to_string(),
                message: Some(error.to_string()),
            },
            std::io::ErrorKind::AlreadyExists => FsError::AlreadyExists {
                method: method.to_string(),
                path: path.to_string(),
                message: Some(error.to_string()),
            },
            std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied {
                method: method.to_string(),
                path: path.to_string(),
                message: Some(error.to_string()),
            },
            _ => FsError::Other {
                method: method.to_string(),
                message: Some(error.to_string()),
            },
        }
    }
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::NotFound { path, .. } => write!(f, "not found: {path}"),
            FsError::AlreadyExists { path, .. } => write!(f, "already exists: {path}"),
            FsError::PermissionDenied { path, .. } => write!(f, "permission denied: {path}"),
            FsError::Other { method, message } => {
                write!(f, "{method}: {}", message.clone().unwrap_or_default())
            }
        }
    }
}

impl std::error::Error for FsError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub kind: Kind,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, Default)]
pub struct GlobOptions {
    pub cwd: Option<String>,
    pub absolute: Option<bool>,
    pub include: Option<glob::Include>,
    pub dot: Option<bool>,
    pub symlink: Option<bool>,
}

/// The filesystem service. Stateless; callers share an `Arc`.
#[derive(Debug, Clone, Default)]
pub struct FSUtilService;

impl FSUtilService {
    pub async fn exists(&self, path: &str) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }

    pub async fn is_dir(&self, path: &str) -> bool {
        tokio::fs::metadata(path)
            .await
            .map(|m| m.is_dir())
            .unwrap_or(false)
    }

    pub async fn is_file(&self, path: &str) -> bool {
        tokio::fs::metadata(path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
    }

    /// Mirrors `existsSafe`.
    pub async fn exists_safe(&self, path: &str) -> bool {
        self.exists(path).await
    }

    pub async fn read_file_string(&self, path: &str) -> Result<String, FsError> {
        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| FsError::from_io("readFileString", path, e))
    }

    /// Mirrors `readFileStringSafe` (swallows NotFound + PermissionDenied).
    pub async fn read_file_string_safe(&self, path: &str) -> Option<String> {
        match self.read_file_string(path).await {
            Ok(value) => Some(value),
            Err(FsError::NotFound { .. } | FsError::PermissionDenied { .. }) => None,
            Err(_) => None,
        }
    }

    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError> {
        tokio::fs::read(path)
            .await
            .map_err(|e| FsError::from_io("readFile", path, e))
    }

    pub async fn read_json(&self, path: &str) -> Result<serde_json::Value, FsError> {
        let text = self.read_file_string(path).await?;
        serde_json::from_str(&text).map_err(|e| FsError::Other {
            method: "readJson".to_string(),
            message: Some(e.to_string()),
        })
    }

    pub async fn write_file_string(&self, path: &str, content: &str) -> Result<(), FsError> {
        tokio::fs::write(path, content)
            .await
            .map_err(|e| FsError::from_io("writeFileString", path, e))
    }

    pub async fn write_file_string_mode(
        &self,
        path: &str,
        content: &str,
        mode: u32,
    ) -> Result<(), FsError> {
        self.write_file_string(path, content).await?;
        self.chmod(path, mode).await
    }

    pub async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FsError> {
        tokio::fs::write(path, content)
            .await
            .map_err(|e| FsError::from_io("writeFile", path, e))
    }

    pub async fn write_json(
        &self,
        path: &str,
        data: &serde_json::Value,
        mode: Option<u32>,
    ) -> Result<(), FsError> {
        // Mirrors JSON.stringify(data, null, 2).
        let content = serde_json::to_string_pretty(data).map_err(|e| FsError::Other {
            method: "writeJson".to_string(),
            message: Some(e.to_string()),
        })?;
        self.write_file_string(path, &content).await?;
        if let Some(mode) = mode {
            self.chmod(path, mode).await?;
        }
        Ok(())
    }

    pub async fn chmod(&self, path: &str, mode: u32) -> Result<(), FsError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = tokio::fs::metadata(path)
                .await
                .map_err(|e| FsError::from_io("chmod", path, e))?
                .permissions();
            permissions.set_mode(mode);
            tokio::fs::set_permissions(path, permissions)
                .await
                .map_err(|e| FsError::from_io("chmod", path, e))
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
            // No-op on Windows: `PermissionsExt::set_mode` is Unix-only.
            Ok(())
        }
    }

    pub async fn ensure_dir(&self, path: &str) -> Result<(), FsError> {
        match tokio::fs::create_dir_all(path).await {
            Ok(()) => Ok(()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::AlreadyExists && self.is_dir(path).await {
                    Ok(())
                } else {
                    Err(FsError::from_io("ensureDir", path, e))
                }
            }
        }
    }

    /// Mirrors `writeWithDirs` — creates parent directories on NotFound.
    pub async fn write_with_dirs(&self, path: &str, content: &[u8]) -> Result<(), FsError> {
        match tokio::fs::write(path, content).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = Path::new(path).parent() {
                    self.ensure_dir(&parent.display().to_string()).await?;
                }
                tokio::fs::write(path, content)
                    .await
                    .map_err(|e| FsError::from_io("writeWithDirs", path, e))
            }
            Err(e) => Err(FsError::from_io("writeWithDirs", path, e)),
        }
    }

    pub async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), FsError> {
        let result = if recursive {
            tokio::fs::remove_dir_all(path).await
        } else {
            tokio::fs::remove_file(path).await
        };
        match result {
            Ok(()) => Ok(()),
            Err(e) if force && e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(FsError::from_io("remove", path, e)),
        }
    }

    pub async fn copy_file(&self, from: &str, to: &str) -> Result<(), FsError> {
        tokio::fs::copy(from, to)
            .await
            .map_err(|e| FsError::from_io("copyFile", from, e))?;
        Ok(())
    }

    pub async fn stat(&self, path: &str) -> Result<Option<Stat>, FsError> {
        let metadata = match tokio::fs::symlink_metadata(path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(FsError::from_io("stat", path, e)),
        };
        let kind = if metadata.file_type().is_dir() {
            Kind::Directory
        } else if metadata.file_type().is_file() {
            Kind::File
        } else if metadata.file_type().is_symlink() {
            Kind::Symlink
        } else {
            Kind::Other
        };
        Ok(Some(Stat {
            kind,
            size: metadata.len(),
        }))
    }

    pub async fn read_directory_entries(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let mut read = tokio::fs::read_dir(path)
            .await
            .map_err(|e| FsError::from_io("readDirectoryEntries", path, e))?;
        let mut entries = Vec::new();
        while let Some(entry) = read
            .next_entry()
            .await
            .map_err(|e| FsError::from_io("readDirectoryEntries", path, e))?
        {
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| FsError::from_io("readDirectoryEntries", path, e))?;
            let kind = if file_type.is_dir() {
                Kind::Directory
            } else if file_type.is_symlink() {
                Kind::Symlink
            } else if file_type.is_file() {
                Kind::File
            } else {
                Kind::Other
            };
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                kind,
            });
        }
        Ok(entries)
    }

    pub async fn resolve(&self, path: &str) -> Result<String, FsError> {
        let resolved = normalize(&windows_path(path));
        match tokio::fs::canonicalize(&resolved).await {
            Ok(canonical) => Ok(canonical.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(resolved),
            Err(e) => Err(FsError::Other {
                method: "resolve".to_string(),
                message: Some(e.to_string()),
            }),
        }
    }

    pub async fn find_up(
        &self,
        target: &str,
        start: &str,
        stop: Option<&str>,
    ) -> Result<Vec<String>, FsError> {
        self.up(&[target], start, stop).await
    }

    pub async fn up(
        &self,
        targets: &[&str],
        start: &str,
        stop: Option<&str>,
    ) -> Result<Vec<String>, FsError> {
        let mut result = Vec::new();
        let mut current = PathBuf::from(start);
        loop {
            for target in targets {
                let search = current.join(target);
                if self.exists(&search.display().to_string()).await {
                    result.push(search.display().to_string());
                }
            }
            if stop == Some(current.to_str().unwrap_or_default()) {
                break;
            }
            let parent = match current.parent() {
                Some(p) if p != current => p.to_path_buf(),
                _ => break,
            };
            current = parent;
        }
        Ok(result)
    }

    pub async fn glob_up(
        &self,
        pattern: &str,
        start: &str,
        stop: Option<&str>,
    ) -> Result<Vec<String>, FsError> {
        let mut result = Vec::new();
        let mut current = PathBuf::from(start);
        loop {
            let options = GlobOptions {
                cwd: Some(current.display().to_string()),
                absolute: Some(true),
                include: Some(glob::Include::File),
                dot: Some(true),
                symlink: None,
            };
            let matches = self.glob(pattern, &options).await.unwrap_or_default();
            result.extend(matches);
            if stop == Some(current.to_str().unwrap_or_default()) {
                break;
            }
            let parent = match current.parent() {
                Some(p) if p != current => p.to_path_buf(),
                _ => break,
            };
            current = parent;
        }
        Ok(result)
    }

    pub async fn glob(&self, pattern: &str, options: &GlobOptions) -> Result<Vec<String>, FsError> {
        let opts = glob::Options {
            cwd: options.cwd.clone(),
            absolute: options.absolute,
            include: options.include,
            dot: options.dot,
            symlink: options.symlink,
        };
        glob::scan(pattern, &opts).map_err(|e| FsError::Other {
            method: "glob".to_string(),
            message: Some(e.to_string()),
        })
    }

    pub fn glob_match(&self, pattern: &str, filepath: &str) -> bool {
        glob::glob_match(pattern, filepath)
    }
}

// Pure helpers ---------------------------------------------------------------

/// Mirrors `FSUtil.windowsPath(p)`.
pub fn windows_path(p: &str) -> String {
    if cfg!(windows) {
        windows_path_impl(p)
    } else {
        p.to_string()
    }
}

fn windows_path_impl(p: &str) -> String {
    let mut out = p.to_string();
    if let Some(rest) = out.strip_prefix('/') {
        if let Some((drive, tail)) = rest.split_once(['/', '\\']) {
            if drive.len() == 1 && drive.chars().next().unwrap().is_ascii_alphabetic() {
                out = format!("{}:/{tail}", drive.to_uppercase());
            }
        }
    }
    for marker in ["/cygdrive/", "/mnt/"] {
        if let Some(rest) = out.strip_prefix(marker) {
            if let Some((drive, tail)) = rest.split_once(['/', '\\']) {
                if drive.len() == 1 {
                    out = format!("{}:/{tail}", drive.to_uppercase());
                }
            }
        }
    }
    out
}

fn normalize(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    Path::new(p).display().to_string()
}

/// Mirrors `FSUtil.contains(parent, child)`.
pub fn contains(parent: &str, child: &str) -> bool {
    let result = path_relative(parent, child);
    let sep = std::path::MAIN_SEPARATOR;
    result.is_empty()
        || (!Path::new(&result).is_absolute()
            && result != ".."
            && !result.starts_with(&format!("..{sep}")))
}

/// Mirrors `FSUtil.overlaps(a, b)`.
pub fn overlaps(a: &str, b: &str) -> bool {
    contains(a, b) || contains(b, a)
}

/// Approximation of `path.relative(parent, child)`.
fn path_relative(parent: &str, child: &str) -> String {
    use std::path::Component;
    let parent: Vec<Component> = Path::new(parent).components().collect();
    let child: Vec<Component> = Path::new(child).components().collect();
    let common = parent
        .iter()
        .zip(child.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = (0..parent.len().saturating_sub(common))
        .map(|_| "..".to_string())
        .collect();
    for component in child.iter().skip(common) {
        parts.push(component.as_os_str().to_string_lossy().to_string());
    }
    parts.join("/")
}

/// Mirrors `FSUtil.mimeType(p)`; a minimal extension-based lookup.
pub fn mime_type(p: &str) -> String {
    let ext = Path::new(p)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("js") => "text/javascript".to_string(),
        Some("mjs") => "text/javascript".to_string(),
        Some("json") => "application/json".to_string(),
        Some("md") => "text/markdown".to_string(),
        Some("txt") => "text/plain".to_string(),
        Some("html") => "text/html".to_string(),
        Some("css") => "text/css".to_string(),
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        Some("wasm") => "application/wasm".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_checks_relative_cross_platform() {
        let root = std::path::Path::new("a").join("b");
        let inside = root.join("c");
        assert!(contains(&root, &inside));
    }

    #[cfg(unix)]
    #[test]
    fn contains_checks_relative() {
        assert!(contains("/a/b", "/a/b/c"));
        assert!(contains("/a/b", "/a/b"));
        assert!(!contains("/a/b", "/a/c"));
        assert!(!contains("/a/b", "/a/bc"));
    }
}
