//! Port of `reference/packages/opencode/src/tool/read.ts`.

use crate::mime::mime_type;
use crate::model::{ExecuteResult, FilePart, PermissionRequest, ToolContext, ToolError};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::external_directory;

pub const DEFAULT_READ_LIMIT: usize = 2000;
const MAX_LINE_LENGTH: usize = 2000;
const MAX_LINE_SUFFIX: &str = "... (line truncated to 2000 chars)";
const MAX_BYTES: usize = 50 * 1024;
const MAX_BYTES_LABEL: &str = "50 KB";
const SAMPLE_BYTES: usize = 4096;
const SUPPORTED_IMAGE_MIMES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

/// `Parameters` from `reference/packages/opencode/src/tool/read.ts:28`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "filePath",
                Schema::string("The absolute path to the file or directory to read"),
            ),
            opt_prop(
                "offset",
                Schema::non_negative_int()
                    .with_description("The line number to start reading from (1-indexed)"),
            ),
            opt_prop(
                "limit",
                Schema::non_negative_int()
                    .with_description("The maximum number of lines to read (defaults to 2000)"),
            ),
        ],
        "read",
    )
}

/// `ReadTool` from `reference/packages/opencode/src/tool/read.ts:64`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("read", prompts::READ, parameters(), |args, ctx| {
        run(args, ctx)
    })
}

fn run(args: serde_json::Value, ctx: &mut ToolContext) -> Result<ExecuteResult, ToolError> {
    let filepath = args
        .get("filePath")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if filepath.is_empty() {
        return Err(ToolError::Other("filePath is required".to_string()));
    }
    let offset = args
        .get("offset")
        .and_then(|v| v.as_i64())
        .unwrap_or(1)
        .max(1) as usize;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(DEFAULT_READ_LIMIT as i64)
        .max(1) as usize;

    let instance = ctx.instance.clone().ok_or_else(|| {
        ToolError::Other("InstanceState.context is required for the read tool".to_string())
    })?;
    let filepath = if std::path::Path::new(&filepath).is_absolute() {
        filepath
    } else {
        std::path::Path::new(&instance.directory)
            .join(&filepath)
            .to_string_lossy()
            .to_string()
    };
    let title = crate::util::path_relative(&instance.worktree, &filepath);

    let stat = std::fs::symlink_metadata(&filepath).ok();
    let kind = if stat.as_ref().map(|meta| meta.is_dir()) == Some(true) {
        external_directory::Kind::Directory
    } else {
        external_directory::Kind::File
    };
    external_directory::assert_external_directory(
        ctx,
        Some(&filepath),
        ctx.extra
            .get("bypassCwdCheck")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        kind,
    )?;

    ctx.ask(PermissionRequest {
        permission: "read".to_string(),
        patterns: vec![crate::util::path_relative(&instance.worktree, &filepath)],
        always: vec!["*".to_string()],
        metadata: serde_json::json!({}),
    })?;

    let Some(stat) = stat else {
        return miss(&filepath);
    };

    if stat.is_dir() {
        return list_directory(&filepath, offset, limit, &instance.worktree);
    }
    if !stat.is_file() {
        return miss(&filepath);
    }

    let loaded = ctx
        .services
        .resolve_instructions(&ctx.messages, &filepath)
        .unwrap_or_default();
    let size = stat.len();
    let sample = read_sample(&filepath, size).unwrap_or_default();
    let fallback = mime_type(&filepath);
    let mime = sniff_attachment_mime(&sample, &fallback);
    let is_image = SUPPORTED_IMAGE_MIMES.contains(&mime.as_str());
    let is_pdf = mime == "application/pdf";

    if is_image || is_pdf {
        let bytes = std::fs::read(&filepath)
            .map_err(|_| ToolError::Other(format!("Unable to read {filepath}")))?;
        let msg = if is_pdf {
            "PDF read successfully".to_string()
        } else {
            "Image read successfully".to_string()
        };
        return Ok(ExecuteResult {
            title,
            output: msg.clone(),
            metadata: serde_json::json!({
                "preview": msg,
                "truncated": false,
                "loaded": loaded.iter().filter_map(|item| item.get("filepath").cloned()).collect::<Vec<_>>(),
            }),
            attachments: Some(vec![FilePart {
                url: format!("data:{mime};base64,{}", base64_encode(&bytes)),
                mime,
                filename: None,
            }]),
        });
    }

    if is_binary_file(&filepath, &sample) {
        return Err(ToolError::Other(format!(
            "Cannot read binary file: {filepath}"
        )));
    }

    let file = read_lines(&filepath, limit, offset).map_err(ToolError::Other)?;
    if file.count < file.offset && !(file.count == 0 && file.offset == 1) {
        return Err(ToolError::Other(format!(
            "Offset {} is out of range for this file ({} lines)",
            file.offset, file.count
        )));
    }

    let mut output = format!("<path>{filepath}</path>\n<type>file</type>\n<content>\n");
    let numbered: Vec<String> = file
        .raw
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{}: {line}", i + file.offset))
        .collect();
    output.push_str(&numbered.join("\n"));

    let last = file.offset + file.raw.len() - 1;
    let next = last + 1;
    let truncated = file.more || file.cut;
    if file.cut {
        output.push_str(&format!(
            "\n\n(Output capped at {MAX_BYTES_LABEL}. Showing lines {}-{last}. Use offset={next} to continue.)",
            file.offset
        ));
    } else if file.more {
        output.push_str(&format!(
            "\n\n(Showing lines {}-{last} of {}. Use offset={next} to continue.)",
            file.offset, file.count
        ));
    } else {
        output.push_str(&format!("\n\n(End of file - total {} lines)", file.count));
    }
    output.push_str("\n</content>");

    let loaded_paths: Vec<serde_json::Value> = loaded
        .iter()
        .filter_map(|item| item.get("filepath").cloned())
        .collect();
    if !loaded_paths.is_empty() {
        let reminders: Vec<String> = loaded
            .iter()
            .filter_map(|item| {
                item.get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        output.push_str(&format!(
            "\n\n<system-reminder>\n{}\n</system-reminder>",
            reminders.join("\n\n")
        ));
    }

    Ok(ExecuteResult {
        title,
        output,
        metadata: serde_json::json!({
            "preview": file.raw.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
            "truncated": truncated,
            "loaded": loaded_paths,
            "display": {
                "type": "file",
                "path": filepath,
                "text": file.raw.join("\n"),
                "lineStart": file.offset,
                "lineEnd": last,
                "totalLines": file.count,
                "truncated": truncated,
            }
        }),
        attachments: None,
    })
}

fn miss(filepath: &str) -> Result<ExecuteResult, ToolError> {
    let dir = std::path::Path::new(filepath)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    let base = std::path::Path::new(filepath)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let lower_base = base.to_lowercase();
    let items = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()))
                .filter(|item| {
                    let lower = item.to_lowercase();
                    lower.contains(&lower_base) || lower_base.contains(&lower)
                })
                .map(|item| {
                    std::path::Path::new(&dir)
                        .join(item)
                        .to_string_lossy()
                        .to_string()
                })
                .take(3)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if items.is_empty() {
        Err(ToolError::Other(format!("File not found: {filepath}")))
    } else {
        Err(ToolError::Other(format!(
            "File not found: {filepath}\n\nDid you mean one of these?\n{}",
            items.join("\n")
        )))
    }
}

fn list_directory(
    filepath: &str,
    offset: usize,
    limit: usize,
    worktree: &str,
) -> Result<ExecuteResult, ToolError> {
    let items = list_entries(filepath)?;
    let start = offset - 1;
    let sliced = if start >= items.len() {
        Vec::new()
    } else {
        items[start..(start + limit).min(items.len())].to_vec()
    };
    let truncated = start + sliced.len() < items.len();
    let note = if truncated {
        format!(
            "\n(Showing {} of {} entries. Use 'offset' parameter to read beyond entry {})",
            sliced.len(),
            items.len(),
            offset + sliced.len()
        )
    } else {
        format!("\n({} entries)", items.len())
    };

    let output = [
        format!("<path>{filepath}</path>"),
        "<type>directory</type>".to_string(),
        "<entries>".to_string(),
        sliced.join("\n"),
        note,
        "</entries>".to_string(),
    ]
    .join("\n");

    Ok(ExecuteResult {
        title: crate::util::path_relative(worktree, filepath),
        output,
        metadata: serde_json::json!({
            "preview": sliced.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
            "truncated": truncated,
            "loaded": [],
            "display": {
                "type": "directory",
                "path": filepath,
                "entries": sliced,
                "offset": offset,
                "totalEntries": items.len(),
                "truncated": truncated,
            }
        }),
        attachments: None,
    })
}

fn list_entries(filepath: &str) -> Result<Vec<String>, ToolError> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(filepath)
        .map_err(|error| ToolError::Other(format!("Unable to read {filepath}: {error}")))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let kind = entry
            .file_type()
            .map_err(|error| ToolError::Other(error.to_string()))?;
        if kind.is_dir() {
            entries.push(format!("{name}/"));
        } else if kind.is_file() {
            entries.push(name);
        } else {
            let target = entry.path().join("");
            let target = target.to_string_lossy().to_string();
            let stat = std::fs::symlink_metadata(&target).ok();
            if stat.as_ref().map(|meta| meta.is_dir()) == Some(true) {
                entries.push(format!("{name}/"));
            } else {
                entries.push(name);
            }
        }
    }
    entries.sort();
    Ok(entries)
}

fn read_sample(filepath: &str, file_size: u64) -> std::io::Result<Vec<u8>> {
    if file_size == 0 {
        return Ok(Vec::new());
    }
    use std::io::Read;
    let mut file = std::fs::File::open(filepath)?;
    let amount = (SAMPLE_BYTES as u64).min(file_size) as usize;
    let mut buffer = vec![0u8; amount];
    let _ = file.read(&mut buffer)?;
    Ok(buffer)
}

struct LineResult {
    raw: Vec<String>,
    count: usize,
    cut: bool,
    more: bool,
    offset: usize,
}

fn read_lines(filepath: &str, limit: usize, offset: usize) -> Result<LineResult, String> {
    let content =
        std::fs::read(filepath).map_err(|error| format!("Unable to read {filepath}: {error}"))?;
    let text = String::from_utf8_lossy(&content);
    let start = offset.saturating_sub(1);
    let mut raw: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    let mut count = 0usize;
    let mut cut = false;
    let mut more = false;

    for line in text.split_inclusive('\n') {
        count += 1;
        if count <= start {
            continue;
        }
        if raw.len() >= limit {
            more = true;
            break;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let shown = if trimmed.len() > MAX_LINE_LENGTH {
            format!("{}{MAX_LINE_SUFFIX}", &trimmed[..MAX_LINE_LENGTH])
        } else {
            trimmed.to_string()
        };
        let size = shown.len() + if !raw.is_empty() { 1 } else { 0 };
        if bytes + size <= MAX_BYTES {
            raw.push(shown);
            bytes += size;
        } else {
            cut = true;
            more = true;
            break;
        }
    }

    Ok(LineResult {
        raw,
        count,
        cut,
        more,
        offset,
    })
}

fn is_binary_file(filepath: &str, bytes: &[u8]) -> bool {
    let extension = std::path::Path::new(filepath)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let binary_extensions = [
        "zip", "tar", "gz", "exe", "dll", "so", "class", "jar", "war", "7z", "doc", "docx", "xls",
        "xlsx", "ppt", "pptx", "odt", "ods", "odp", "bin", "dat", "obj", "o", "a", "lib", "wasm",
        "pyc", "pyo",
    ];
    if binary_extensions.contains(&extension.as_str()) {
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

fn starts_with(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix
}

/// `sniffAttachmentMime` from `reference/packages/opencode/src/util/media.ts`.
pub fn sniff_attachment_mime(bytes: &[u8], fallback: &str) -> String {
    if starts_with(bytes, &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return "image/png".to_string();
    }
    if starts_with(bytes, &[0xff, 0xd8, 0xff]) {
        return "image/jpeg".to_string();
    }
    if starts_with(bytes, &[0x47, 0x49, 0x46, 0x38]) {
        return "image/gif".to_string();
    }
    if starts_with(bytes, &[0x42, 0x4d]) {
        return "image/bmp".to_string();
    }
    if starts_with(bytes, &[0x25, 0x50, 0x44, 0x46, 0x2d]) {
        return "application/pdf".to_string();
    }
    if starts_with(bytes, &[0x52, 0x49, 0x46, 0x46])
        && starts_with(&bytes[8.min(bytes.len())..], &[0x57, 0x45, 0x42, 0x50])
    {
        return "image/webp".to_string();
    }
    fallback.to_string()
}

fn base64_encode(bytes: &[u8]) -> String {
    crate::base64::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "filePath": { "description": "The absolute path to the file or directory to read", "type": "string" },
                    "limit": { "description": "The maximum number of lines to read (defaults to 2000)", "maximum": 9007199254740991i64, "minimum": 0, "type": "integer" },
                    "offset": { "description": "The line number to start reading from (1-indexed)", "maximum": 9007199254740991i64, "minimum": 0, "type": "integer" }
                },
                "required": ["filePath"],
                "type": "object"
            })
        );
    }

    #[test]
    fn formats_file_read_output() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("example.txt");
        std::fs::write(&file, "foo\nbar\nbaz\n").unwrap();
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let result = run(
            serde_json::json!({ "filePath": file.to_string_lossy() }),
            &mut ctx,
        )
        .unwrap();
        assert!(result.output.contains("<type>file</type>"));
        assert!(result.output.contains("1: foo\n2: bar\n3: baz"));
        assert!(result.output.contains("(End of file - total 3 lines)"));
        assert_eq!(result.output.lines().last().unwrap(), "</content>");
    }

    #[test]
    fn formats_directory_read_output() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let result = run(
            serde_json::json!({ "filePath": dir.path().to_string_lossy() }),
            &mut ctx,
        )
        .unwrap();
        assert!(result.output.contains("<type>directory</type>"));
        assert!(result.output.contains("a.txt\nsub/"));
    }

    #[test]
    fn miss_suggests_similar_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.ts"), "x").unwrap();
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let error = run(
            serde_json::json!({ "filePath": dir.path().join("broken.t").to_string_lossy() }),
            &mut ctx,
        )
        .unwrap_err();
        assert!(error.message().contains("Did you mean one of these?"));
    }
}
