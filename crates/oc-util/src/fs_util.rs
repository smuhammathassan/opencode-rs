/// From reference/packages/core/src/fs-util.ts
///
/// Pure path helpers (`mimeType`, `normalizePath`, `resolve`, `windowsPath`,
/// `overlaps`, `contains`) and the filesystem operations exposed by the
/// reference's `FSUtil` service, as plain async functions.
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use tokio::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub r#type: &'static str,
}

pub fn mime_type(p: &str) -> String {
    crate::util::filesystem::mime_type(p)
}

pub fn normalize_path(p: &str) -> String {
    if cfg!(windows) {
        normalize_path_windows(p)
    } else {
        p.to_string()
    }
}

fn normalize_path_windows(p: &str) -> String {
    let resolved = std::path::absolute(Path::new(&windows_path(p)))
        .unwrap_or_else(|_| PathBuf::from(&windows_path(p)));
    match std::fs::canonicalize(&resolved) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => resolved.to_string_lossy().into_owned(),
    }
}

pub fn normalize_path_pattern(p: &str) -> String {
    if cfg!(windows) {
        if p == "*" {
            return p.to_string();
        }
        if let Some(rest) = p.strip_suffix("/*").or_else(|| p.strip_suffix("\\*")) {
            let dir = if rest.len() == 2
                && rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && rest.ends_with(':')
            {
                format!("{rest}\\")
            } else {
                rest.to_string()
            };
            return std::path::absolute(Path::new(&dir))
                .map(|d| format!("{}\\*", d.to_string_lossy()))
                .unwrap_or_else(|_| format!("{dir}/*"));
        }
        normalize_path(p)
    } else {
        p.to_string()
    }
}

pub fn resolve(p: &str) -> Result<String, std::io::Error> {
    let resolved = std::path::absolute(Path::new(p))?;
    match std::fs::canonicalize(&resolved) {
        Ok(canonical) => Ok(canonical.to_string_lossy().into_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(resolved.to_string_lossy().into_owned())
        }
        Err(e) => Err(e),
    }
}

pub fn windows_path(p: &str) -> String {
    if cfg!(windows) {
        let mut out = p.to_string();
        if let Some(rest) = p.strip_prefix('/') {
            let bytes = rest.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                out = format!("{}:{}", rest[..1].to_uppercase(), &rest[1..]);
            }
        }
        out
    } else {
        p.to_string()
    }
}

pub fn contains(parent: &str, child: &str) -> bool {
    let parent = Path::new(parent);
    let child = Path::new(child);
    match child.strip_prefix(parent) {
        Ok(rest) => !matches!(rest.components().next(), Some(Component::ParentDir)),
        Err(_) => false,
    }
}

pub fn overlaps(a: &str, b: &str) -> bool {
    contains(a, b) || contains(b, a)
}

pub async fn exists_safe(path: &str) -> bool {
    fs::try_exists(path).await.unwrap_or(false)
}

pub async fn is_dir(path: &str) -> bool {
    fs::metadata(path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

pub async fn is_file(path: &str) -> bool {
    fs::metadata(path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
}

pub async fn read_file_string_safe(path: &str) -> Option<String> {
    fs::read_to_string(path).await.ok()
}

pub async fn read_file_bytes(path: &str) -> std::io::Result<Vec<u8>> {
    fs::read(path).await
}

pub async fn read_json(path: &str) -> anyhow::Result<Value> {
    let text = fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&text)?)
}

pub async fn write_json(path: &str, data: &Value, mode: Option<u32>) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(data)?;
    write_with_dirs(path, content.into_bytes(), mode).await
}

pub async fn ensure_dir(path: &str) -> anyhow::Result<()> {
    fs::create_dir_all(path).await?;
    Ok(())
}

async fn write_file_with_mode(
    path: &str,
    content: &[u8],
    mode: Option<u32>,
) -> std::io::Result<()> {
    if let Some(mode) = mode {
        tokio::fs::write(path, content).await?;
        set_mode(path, mode).await
    } else {
        tokio::fs::write(path, content).await
    }
}

pub async fn write_with_dirs(
    path: &str,
    content: Vec<u8>,
    mode: Option<u32>,
) -> anyhow::Result<()> {
    match write_file_with_mode(path, &content, mode).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = Path::new(path).parent() {
                fs::create_dir_all(dir).await?;
            }
            write_file_with_mode(path, &content, mode).await?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

async fn set_mode(path: &str, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).await?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).await
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

pub async fn read_directory_entries(dir: &str) -> anyhow::Result<Vec<DirEntry>> {
    let mut entries = fs::read_dir(dir).await?;
    let mut result = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let r#type = if file_type.is_dir() {
            "directory"
        } else if file_type.is_symlink() {
            "symlink"
        } else if file_type.is_file() {
            "file"
        } else {
            "other"
        };
        result.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            r#type,
        });
    }
    Ok(result)
}

pub async fn find_up(target: &str, start: &str, stop: Option<&str>) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    let mut current = PathBuf::from(start);
    loop {
        let search = current.join(target);
        if exists_safe(search.to_str().unwrap_or_default()).await {
            result.push(search.to_string_lossy().into_owned());
        }
        if stop == current.to_str() {
            break;
        }
        let parent = current.parent().map(Path::to_path_buf);
        match parent {
            Some(parent) if parent != current => current = parent,
            _ => break,
        }
    }
    Ok(result)
}

pub async fn up(targets: &[&str], start: &str, stop: Option<&str>) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    let mut current = PathBuf::from(start);
    loop {
        for target in targets {
            let search = current.join(target);
            if exists_safe(search.to_str().unwrap_or_default()).await {
                result.push(search.to_string_lossy().into_owned());
            }
        }
        if stop == current.to_str() {
            break;
        }
        let parent = current.parent().map(Path::to_path_buf);
        match parent {
            Some(parent) if parent != current => current = parent,
            _ => break,
        }
    }
    Ok(result)
}

pub async fn glob_up(
    pattern: &str,
    start: &str,
    stop: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    let mut current = PathBuf::from(start);
    loop {
        let matches = crate::glob::scan(
            pattern,
            &crate::glob::Options {
                cwd: Some(current.clone()),
                absolute: true,
                include: crate::glob::Include::File,
                dot: true,
                symlink: false,
            },
        )
        .await
        .unwrap_or_default();
        result.extend(matches);
        if stop == current.to_str() {
            break;
        }
        let parent = current.parent().map(Path::to_path_buf);
        match parent {
            Some(parent) if parent != current => current = parent,
            _ => break,
        }
    }
    Ok(result)
}

pub async fn glob(pattern: &str, options: &crate::glob::Options) -> anyhow::Result<Vec<String>> {
    crate::glob::scan(pattern, options).await
}

pub fn glob_match(pattern: &str, filepath: &str) -> bool {
    crate::glob::glob_match(pattern, filepath)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_detects_ancestry() {
        assert!(contains("/a/b", "/a/b"));
        assert!(contains("/a/b", "/a/b/c"));
        assert!(!contains("/a/b", "/a"));
        assert!(!contains("/a/b", "/a/c"));
        assert!(!contains("/a/b", "/a/bc"));
        assert!(contains("/a", "/a/b"));
    }

    #[test]
    fn overlaps_detects_either_direction() {
        assert!(overlaps("/a/b", "/a/b/c"));
        assert!(overlaps("/a/b/c", "/a/b"));
        assert!(!overlaps("/a/x", "/a/y"));
    }

    #[tokio::test]
    async fn write_with_dirs_creates_parents() {
        let dir = std::env::temp_dir().join(format!("oc-util-fs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        let file = dir.join("nested/deep/file.txt");
        write_with_dirs(file.to_str().unwrap(), b"hello".to_vec(), None)
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&file).await.unwrap(), "hello");
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn write_json_is_pretty() {
        let dir = std::env::temp_dir().join(format!("oc-util-fs-json-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        fs::create_dir_all(&dir).await.unwrap();
        let file = dir.join("data.json");
        write_json(file.to_str().unwrap(), &serde_json::json!({ "a": 1 }), None)
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&file).await.unwrap(), "{\n  \"a\": 1\n}");
        let parsed = read_json(file.to_str().unwrap()).await.unwrap();
        assert_eq!(parsed["a"], 1);
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn read_file_string_safe_returns_none_when_missing() {
        assert_eq!(read_file_string_safe("/definitely/missing").await, None);
        assert_eq!(exists_safe("/definitely/missing").await, false);
    }

    #[tokio::test]
    async fn read_directory_entries_classifies_types() {
        let dir = std::env::temp_dir().join(format!("oc-util-fs-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join("f.txt"), "x").await.unwrap();
        fs::create_dir(dir.join("sub")).await.unwrap();
        let entries = read_directory_entries(dir.to_str().unwrap()).await.unwrap();
        assert_eq!(entries.len(), 2);
        let f = entries.iter().find(|e| e.name == "f.txt").unwrap();
        assert_eq!(f.r#type, "file");
        let d = entries.iter().find(|e| e.name == "sub").unwrap();
        assert_eq!(d.r#type, "directory");
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn find_up_walks_to_root() {
        let dir = std::env::temp_dir().join(format!("oc-util-findup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        fs::create_dir_all(dir.join("a/b/c")).await.unwrap();
        fs::write(dir.join("marker"), "").await.unwrap();
        fs::write(dir.join("a/b/marker"), "").await.unwrap();
        let found = find_up("marker", dir.join("a/b/c").to_str().unwrap(), None)
            .await
            .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0],
            dir.join("a/b/marker").to_string_lossy().into_owned()
        );
        assert_eq!(found[1], dir.join("marker").to_string_lossy().into_owned());
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_up_collects_across_directories() {
        let dir = std::env::temp_dir().join(format!("oc-util-globup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        fs::create_dir_all(dir.join("a/b")).await.unwrap();
        fs::write(dir.join("a/b/x.txt"), "").await.unwrap();
        fs::write(dir.join("x.txt"), "").await.unwrap();
        let stop = dir.to_str().unwrap().to_string();
        let found = glob_up("*.txt", dir.join("a/b").to_str().unwrap(), Some(&stop))
            .await
            .unwrap();
        assert!(found.contains(&dir.join("x.txt").to_string_lossy().into_owned()));
        assert!(found.contains(&dir.join("a/b/x.txt").to_string_lossy().into_owned()));
        let _ = fs::remove_dir_all(&dir).await;
    }
}
