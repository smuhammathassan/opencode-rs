/// From reference/packages/opencode/src/util/filesystem.ts
///
/// The reference wraps sync/async `fs/promises` calls plus the shared helpers
/// from `FSUtil` (`mimeType`, `normalizePath`, `resolve`, `windowsPath`,
/// `overlaps`, `contains`).
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use tokio::fs;

pub async fn exists(p: &str) -> bool {
    fs::try_exists(p).await.unwrap_or(false)
}

pub async fn is_dir(p: &str) -> bool {
    std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

/// Mirrors the sync `stat(p, { throwIfNoEntry: false })`.
pub fn stat(p: &str) -> Option<std::fs::Metadata> {
    std::fs::metadata(p).ok()
}

pub async fn stat_async(p: &str) -> anyhow::Result<Option<std::fs::Metadata>> {
    match fs::metadata(p).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn size(p: &str) -> u64 {
    stat(p).map(|s| s.len()).unwrap_or(0)
}

pub async fn read_text(p: &str) -> std::io::Result<String> {
    fs::read_to_string(p).await
}

pub async fn read_json(p: &str) -> anyhow::Result<Value> {
    let text = fs::read_to_string(p).await?;
    Ok(serde_json::from_str(&text)?)
}

pub async fn read_bytes(p: &str) -> std::io::Result<Vec<u8>> {
    fs::read(p).await
}

pub async fn read_array_buffer(p: &str) -> std::io::Result<Vec<u8>> {
    fs::read(p).await
}

async fn write_file(p: &str, content: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    if let Some(mode) = mode {
        fs::write(p, content).await?;
        set_mode(p, mode).await
    } else {
        fs::write(p, content).await
    }
}

async fn set_mode(p: &str, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(p).await?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(p, permissions).await
    }
    #[cfg(not(unix))]
    {
        let _ = (p, mode);
        Ok(())
    }
}

async fn write_impl(p: &str, content: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    match write_file(p, content, mode).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = Path::new(p).parent() {
                fs::create_dir_all(dir).await?;
            }
            write_file(p, content, mode).await
        }
        Err(e) => Err(e),
    }
}

pub async fn write(p: &str, content: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    write_impl(p, content, mode).await
}

pub async fn write_json(p: &str, data: &Value, mode: Option<u32>) -> anyhow::Result<()> {
    // Keep generated configuration files stable even when another workspace
    // crate enables serde_json's `preserve_order` feature.  The reference
    // writes deterministic JSON, and stable ordering also prevents needless
    // churn in config diffs.
    let sorted = sort_json_keys(data);
    let content = serde_json::to_string_pretty(&sorted)?;
    write_impl(p, content.as_bytes(), mode).await?;
    Ok(())
}

fn sort_json_keys(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object
                .iter()
                .map(|(key, value)| (key.clone(), sort_json_keys(value)))
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(entries.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.iter().map(sort_json_keys).collect()),
        other => other.clone(),
    }
}

pub async fn write_stream(
    p: &str,
    reader: impl tokio::io::AsyncRead + Unpin,
    mode: Option<u32>,
) -> std::io::Result<()> {
    if let Some(dir) = Path::new(p).parent() {
        if !dir.is_dir() {
            fs::create_dir_all(dir).await?;
        }
    }
    let mut reader = reader;
    let mut file = fs::File::create(p).await?;
    tokio::io::copy(&mut reader, &mut file).await?;
    if let Some(mode) = mode {
        set_mode(p, mode).await?;
    }
    Ok(())
}

/// From reference/packages/opencode/src/util/filesystem.ts (`mimeType`) and
/// `packages/core/src/fs-util.ts` (`FSUtil.mimeType`). The `mime-types` npm
/// package lookup is approximated by a curated extension table.
pub fn mime_type(p: &str) -> String {
    let ext = Path::new(p)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" | "svgz" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tiff" | "tif" => "image/tiff",
        "txt" | "text" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "json" => "application/json",
        "jsonc" => "application/json",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "mts" | "cts" => "video/mp2t",
        "tsx" => "text/plain",
        "xml" => "text/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => return "application/octet-stream".to_string(),
    };
    mime.to_string()
}

pub fn normalize_path(p: &str) -> String {
    crate::fs_util::normalize_path(p)
}

pub fn normalize_path_pattern(p: &str) -> String {
    crate::fs_util::normalize_path_pattern(p)
}

/// From reference/packages/opencode/src/util/filesystem.ts (`resolve`).
pub fn resolve(p: &str) -> String {
    crate::fs_util::resolve(p).unwrap_or_else(|_| p.to_string())
}

pub fn resolve_file_path(root: &str, file: &str) -> String {
    let raw = if file.starts_with("file://") {
        url::Url::parse(file)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string())
    } else {
        file.to_string()
    };
    if Path::new(&raw).is_absolute() {
        raw
    } else {
        std::path::absolute(Path::new(root).join(&raw))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| raw)
    }
}

pub fn windows_path(p: &str) -> String {
    crate::fs_util::windows_path(p)
}

pub fn overlaps(a: &str, b: &str) -> bool {
    crate::fs_util::overlaps(a, b)
}

pub fn contains(parent: &str, child: &str) -> bool {
    crate::fs_util::contains(parent, child)
}

/// From reference/packages/opencode/src/util/filesystem.ts (`findUp`).
pub async fn find_up(
    targets: &[String],
    start: &str,
    stop: Option<&str>,
    root_first: bool,
) -> Vec<String> {
    let mut dirs = vec![start.to_string()];
    let mut current = Path::new(start).to_path_buf();
    loop {
        if stop == current.to_str() {
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent.to_path_buf();
                dirs.push(current.to_string_lossy().into_owned());
            }
            _ => break,
        }
    }
    if root_first {
        dirs.reverse();
    }
    let mut result = Vec::new();
    for dir in &dirs {
        for item in targets {
            let search = Path::new(dir).join(item);
            if exists(search.to_str().unwrap_or_default()).await {
                result.push(search.to_string_lossy().into_owned());
            }
        }
    }
    result
}

/// From reference/packages/opencode/src/util/filesystem.ts (`up`).
pub async fn up(targets: &[&str], start: &str, stop: Option<&str>) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Path::new(start).to_path_buf();
    loop {
        for target in targets {
            let search = current.join(target);
            if exists(search.to_str().unwrap_or_default()).await {
                result.push(search.to_string_lossy().into_owned());
            }
        }
        if stop == current.to_str() {
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent.to_path_buf();
            }
            _ => break,
        }
    }
    result
}

/// From reference/packages/opencode/src/util/filesystem.ts (`globUp`).
pub async fn glob_up(pattern: &str, start: &str, stop: Option<&str>) -> Vec<String> {
    crate::fs_util::glob_up(pattern, start, stop)
        .await
        .unwrap_or_default()
}

/// A shared streaming writer that mirrors `writeStream` for tokio readers.
pub struct StreamWriter {
    path: String,
    mode: Option<u32>,
    _marker: Arc<()>,
}

impl StreamWriter {
    pub fn new(path: &str, mode: Option<u32>) -> Self {
        StreamWriter {
            path: path.to_string(),
            mode,
            _marker: Arc::new(()),
        }
    }

    pub async fn write(&self, reader: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<()> {
        write_stream(&self.path, reader, self.mode).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oc-util-filesystem-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn read_write_round_trip() {
        let dir = tmp_dir("rw");
        let file = dir.join("sub/a.txt");
        write(file.to_str().unwrap(), b"hello", None).await.unwrap();
        assert_eq!(read_text(file.to_str().unwrap()).await.unwrap(), "hello");
        assert_eq!(read_bytes(file.to_str().unwrap()).await.unwrap(), b"hello");
        assert_eq!(size(file.to_str().unwrap()).await, 5);
    }

    #[tokio::test]
    async fn write_creates_parent_dirs() {
        let dir = tmp_dir("mkdirs");
        let file = dir.join("x/y/z.txt");
        write(file.to_str().unwrap(), b"deep", None).await.unwrap();
        assert!(exists(file.to_str().unwrap()).await);
        assert!(is_dir(dir.join("x").to_str().unwrap()).await);
    }

    #[tokio::test]
    async fn stat_missing_returns_none() {
        assert!(stat("/definitely/missing").is_none());
        assert!(stat_async("/definitely/missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn write_stream_copies() {
        let dir = tmp_dir("stream");
        let file = dir.join("out.bin");
        let bytes = b"streamed content";
        write_stream(file.to_str().unwrap(), &bytes[..], None)
            .await
            .unwrap();
        assert_eq!(read_bytes(file.to_str().unwrap()).await.unwrap(), bytes);
    }

    #[tokio::test]
    async fn write_json_indents() {
        let dir = tmp_dir("json");
        let file = dir.join("cfg.json");
        write_json(
            file.to_str().unwrap(),
            &serde_json::json!({ "b": 2, "a": 1 }),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            read_text(file.to_str().unwrap()).await.unwrap(),
            "{\n  \"a\": 1,\n  \"b\": 2\n}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_file_path_handles_file_urls() {
        assert!(resolve_file_path("/root", "file:///tmp/x.txt").ends_with("x.txt"));
        assert_eq!(resolve_file_path("/root", "/abs/path"), "/abs/path");
        assert!(resolve_file_path("/root", "rel/path").contains("/root/rel/path"));
    }

    #[test]
    fn mime_types_by_extension() {
        assert_eq!(mime_type("a.png"), "image/png");
        assert_eq!(mime_type("b.JPG"), "image/jpeg");
        assert_eq!(mime_type("c.pdf"), "application/pdf");
        assert_eq!(mime_type("d.js"), "text/javascript");
        assert_eq!(mime_type("e.unknown_ext"), "application/octet-stream");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn find_up_with_stop_and_root_first() {
        let dir = tmp_dir("findup");
        std::fs::create_dir_all(dir.join("a/b/c")).unwrap();
        std::fs::write(dir.join("m"), "").unwrap();
        std::fs::write(dir.join("a/b/m"), "").unwrap();
        let start = dir.join("a/b/c").to_str().unwrap().to_string();
        let targets = vec!["m".to_string()];
        let found = find_up(&targets, &start, None, false).await;
        assert_eq!(found.len(), 2);
        let reversed = find_up(&targets, &start, None, true).await;
        assert_eq!(reversed[0], dir.join("m").to_string_lossy().into_owned());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn up_collects_all_targets_per_directory() {
        let dir = tmp_dir("up");
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/m1"), "").unwrap();
        std::fs::write(dir.join("a/b/m2"), "").unwrap();
        std::fs::write(dir.join("m2"), "").unwrap();
        let start = dir.join("a/b").to_str().unwrap().to_string();
        let found = up(&["m1", "m2"], &start, None).await;
        assert_eq!(found.len(), 3);
        assert!(found.iter().any(|f| f.ends_with("a/b/m2")));
        assert!(found.iter().any(|f| f.ends_with("a/m1")));
        assert!(found.iter().any(|f| f.ends_with("m2")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn glob_up_collects_matches() {
        let dir = tmp_dir("globup");
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x.txt"), "").unwrap();
        std::fs::write(dir.join("x.txt"), "").unwrap();
        let start = dir.join("a/b").to_str().unwrap().to_string();
        let stop = dir.to_str().unwrap().to_string();
        let found = glob_up("*.txt", &start, Some(&stop)).await;
        assert!(found.contains(&dir.join("x.txt").to_string_lossy().into_owned()));
        assert!(found.contains(&dir.join("a/b/x.txt").to_string_lossy().into_owned()));
    }
}
