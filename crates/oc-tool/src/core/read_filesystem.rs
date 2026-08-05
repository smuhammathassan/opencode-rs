//! Port of `reference/packages/core/src/tool/read-filesystem.ts` — the
//! file/directory reading primitives used by the V2 `read` tool.

use std::path::Path;

use serde::Serialize;

use crate::mime::mime_type;
use crate::model::ToolError;

pub const MAX_READ_LINES: usize = 2_000;
pub const MAX_READ_BYTES: usize = 50 * 1024;
pub const MAX_MEDIA_INGEST_BYTES: usize = 20 * 1024 * 1024;
const MAX_LINE_LENGTH: usize = 2_000;
const MAX_LINE_SUFFIX: &str = "... (line truncated to 2000 chars)";

const BINARY_EXTENSIONS: [&str; 28] = [
    ".zip", ".tar", ".gz", ".exe", ".dll", ".so", ".class", ".jar", ".war", ".7z", ".doc", ".docx",
    ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".bin", ".dat", ".obj", ".o", ".a",
    ".lib", ".wasm", ".pyc", ".pyo",
];

/// `FileSystem.Content` from `reference/packages/core/src/filesystem.ts:18`.
#[derive(Debug, Clone, Serialize)]
pub struct FileContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub content: String,
    pub encoding: String,
    pub mime: String,
}

/// `ReadTool.TextPage` from `reference/packages/core/src/tool/read-filesystem.ts:78`.
#[derive(Debug, Clone, Serialize)]
pub struct TextPage {
    #[serde(rename = "type")]
    pub kind: String,
    pub content: String,
    pub mime: String,
    pub offset: i64,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
}

/// `ReadTool.ListPage` from `reference/packages/core/src/tool/read-filesystem.ts:87`.
#[derive(Debug, Clone, Serialize)]
pub struct ListPage {
    pub entries: Vec<crate::ripgrep::Entry>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct PageInput {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

impl Default for PageInput {
    fn default() -> Self {
        PageInput {
            offset: None,
            limit: None,
        }
    }
}

fn starts_with(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if starts_with(bytes, &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if starts_with(bytes, &[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if starts_with(bytes, &[0x47, 0x49, 0x46, 0x38]) {
        return Some("image/gif");
    }
    if starts_with(bytes, &[0x52, 0x49, 0x46, 0x46])
        && starts_with(&bytes[8.min(bytes.len())..], &[0x57, 0x45, 0x42, 0x50])
    {
        return Some("image/webp");
    }
    None
}

/// `binary` heuristic from `reference/packages/core/src/tool/read-filesystem.ts:143`.
fn is_binary(resource: &str, bytes: &[u8]) -> bool {
    let extension = Path::new(resource)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
        .unwrap_or_default();
    if BINARY_EXTENSIONS.contains(&extension.as_str()) {
        return true;
    }
    if bytes.is_empty() {
        return false;
    }
    let mut non_printable = 0usize;
    for byte in bytes {
        if *byte == 0 {
            return true;
        }
        if *byte < 9 || (*byte > 13 && *byte < 32) {
            non_printable += 1;
        }
    }
    non_printable as f64 / bytes.len() as f64 > 0.3
}

/// `ReadTool.inspect` from `reference/packages/core/src/tool/read-filesystem.ts:164`.
pub fn inspect(input: &str) -> Result<&'static str, ToolError> {
    let info = std::fs::symlink_metadata(input)
        .map_err(|error| ToolError::Other(format!("Unable to inspect {input}: {error}")))?;
    if info.is_file() {
        Ok("file")
    } else if info.is_dir() {
        Ok("directory")
    } else {
        Err(ToolError::failure(format!(
            "Path is not a file or directory: {input}"
        )))
    }
}

fn file_url(path: &str) -> String {
    format!("file://{}", path)
}

/// `ReadTool.read` from `reference/packages/core/src/tool/read-filesystem.ts:171`.
pub fn read(input: &str, resource: &str, page: &PageInput) -> Result<serde_json::Value, ToolError> {
    let real = std::path::absolute(input)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| input.to_string());
    let info = std::fs::symlink_metadata(&real)
        .map_err(|error| ToolError::Other(format!("Unable to read {real}: {error}")))?;
    if !info.is_file() {
        return Err(ToolError::failure(format!(
            "Path is not a file: {resource}"
        )));
    }
    let bytes = std::fs::read(&real)
        .map_err(|error| ToolError::Other(format!("Unable to read {real}: {error}")))?;
    let first_len = (64 * 1024).min(bytes.len().max(0));
    let first = &bytes[..first_len.min(bytes.len())];

    if let Some(mime) = image_mime(first) {
        if bytes.len() > MAX_MEDIA_INGEST_BYTES {
            return Err(ToolError::failure(format!(
                "Media exceeds {MAX_MEDIA_INGEST_BYTES} byte ingestion limit: {resource}"
            )));
        }
        return Ok(serde_json::to_value(FileContent {
            uri: file_url(&real),
            name: Some(
                Path::new(&real)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            content: crate::base64::encode(&bytes),
            encoding: "base64".to_string(),
            mime: mime.to_string(),
        })
        .map_err(|error| ToolError::Other(error.to_string()))?);
    }

    let extension = Path::new(&resource)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
        .unwrap_or_default();
    if starts_with(first, &[0x25, 0x50, 0x44, 0x46])
        || BINARY_EXTENSIONS.contains(&extension.as_str())
    {
        return Err(ToolError::failure(format!(
            "Cannot read binary file: {resource}"
        )));
    }

    let paged = bytes.len() > MAX_READ_BYTES || page.offset.is_some() || page.limit.is_some();
    if !paged {
        if is_binary(resource, first) {
            return Err(ToolError::failure(format!(
                "Cannot read binary file: {resource}"
            )));
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        return Ok(serde_json::to_value(FileContent {
            uri: file_url(&real),
            name: Some(
                Path::new(&real)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            content: text,
            encoding: "utf8".to_string(),
            mime: mime_type(&real),
        })
        .map_err(|error| ToolError::Other(error.to_string()))?);
    }

    let offset = page.offset.unwrap_or(1) as usize;
    let limit = page
        .limit
        .unwrap_or(MAX_READ_LINES as i64)
        .min(MAX_READ_LINES as i64) as usize;

    let text = String::from_utf8_lossy(&bytes).to_string();
    if is_binary(resource, &bytes) {
        return Err(ToolError::failure(format!(
            "Cannot read binary file: {resource}"
        )));
    }
    let (content, truncated, next) = read_text_page(&text, offset, limit);
    if content.is_empty() && offset != 1 {
        return Err(ToolError::failure(format!(
            "Offset {offset} is out of range"
        )));
    }

    Ok(serde_json::to_value(TextPage {
        kind: "text-page".to_string(),
        content,
        mime: mime_type(&real),
        offset: offset as i64,
        truncated,
        next: next.map(|value| value as i64),
    })
    .map_err(|error| ToolError::Other(error.to_string()))?)
}

fn read_text_page(text: &str, offset: usize, limit: usize) -> (String, bool, Option<usize>) {
    let physical: Vec<&str> = text.split('\n').collect();
    let ends_with_newline = text.ends_with('\n');
    let mut lines: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    let mut line = 1usize;
    let mut next: Option<usize> = None;

    for (index, item) in physical.iter().enumerate() {
        if index == physical.len() - 1 && ends_with_newline && item.is_empty() {
            continue;
        }
        if line < offset {
            line += 1;
            continue;
        }
        if lines.len() >= limit || bytes >= MAX_READ_BYTES {
            next = Some(line);
            break;
        }
        let shown = if item.len() > MAX_LINE_LENGTH {
            format!("{}{MAX_LINE_SUFFIX}", &item[..MAX_LINE_LENGTH])
        } else {
            (*item).to_string()
        };
        let size = shown.len() + if !lines.is_empty() { 1 } else { 0 };
        if bytes + size > MAX_READ_BYTES {
            next = Some(line);
            break;
        }
        lines.push(shown);
        bytes += size;
        line += 1;
    }

    (lines.join("\n"), next.is_some(), next)
}

/// `ReadTool.list` from `reference/packages/core/src/tool/read-filesystem.ts:324`.
pub fn list(input: &str, page: &PageInput) -> Result<ListPage, ToolError> {
    let real = std::path::absolute(input)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| input.to_string());
    let mut items = Vec::new();
    for entry in std::fs::read_dir(&real)
        .map_err(|error| ToolError::Other(format!("Unable to list {real}: {error}")))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let target = std::path::absolute(entry.path())
            .unwrap_or_else(|_| entry.path())
            .to_string_lossy()
            .to_string();
        if !crate::util::fs_contains(&real, &target) {
            continue;
        }
        let stat = std::fs::symlink_metadata(&target).ok();
        let kind = if stat.as_ref().map(|meta| meta.is_dir()) == Some(true) {
            "directory"
        } else if stat.as_ref().map(|meta| meta.is_file()) == Some(true) {
            "file"
        } else {
            continue;
        };
        let display = if kind == "directory" {
            format!("{name}{}", std::path::MAIN_SEPARATOR)
        } else {
            name
        };
        items.push(crate::ripgrep::Entry::make(display, kind));
    }
    items.sort_by(|a, b| {
        if a.kind == b.kind {
            a.path.cmp(&b.path)
        } else if a.kind == "directory" {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    let offset = page.offset.unwrap_or(1) as usize;
    let limit = page
        .limit
        .unwrap_or(MAX_READ_LINES as i64)
        .min(MAX_READ_LINES as i64) as usize;
    let start = offset.saturating_sub(1);
    let selected: Vec<crate::ripgrep::Entry> = if start >= items.len() {
        Vec::new()
    } else {
        items[start..(start + limit).min(items.len())].to_vec()
    };
    let truncated = start + selected.len() < items.len();
    let next = if truncated {
        Some((offset + selected.len()) as i64)
    } else {
        None
    };
    Ok(ListPage {
        entries: selected,
        truncated,
        next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_small_text_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "hello\nworld\n").unwrap();
        let value = read(&file.to_string_lossy(), "a.txt", &PageInput::default()).unwrap();
        assert_eq!(value["encoding"], "utf8");
        assert_eq!(value["content"], "hello\nworld\n");
        assert_eq!(value["mime"], "text/plain");
    }

    #[test]
    fn reads_pages() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        let content: String = (0..10).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&file, &content).unwrap();
        let page = PageInput {
            offset: Some(3),
            limit: Some(2),
        };
        let value = read(&file.to_string_lossy(), "a.txt", &page).unwrap();
        assert_eq!(value["type"], "text-page");
        assert_eq!(value["content"], "line 2\nline 3");
        assert_eq!(value["offset"], 3);
        assert!(value["truncated"] == serde_json::json!(true));
        assert_eq!(value["next"], 5);
    }

    #[test]
    fn offsets_out_of_range_fail() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "one\n").unwrap();
        let error = read(
            &file.to_string_lossy(),
            "a.txt",
            &PageInput {
                offset: Some(9),
                limit: None,
            },
        )
        .unwrap_err();
        assert!(error.message().contains("out of range"));
    }

    #[test]
    fn lists_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let list_page = list(&dir.path().to_string_lossy(), &PageInput::default()).unwrap();
        let names: Vec<&str> = list_page
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(names, vec!["sub/", "a.txt"]);
    }
}
