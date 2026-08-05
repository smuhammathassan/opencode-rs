//! Markdown frontmatter parsing used by the command and skill loaders.
//!
//! From reference/packages/core/src/config/markdown.ts (`parse`, `parseOption`,
//! `sanitize`) and reference/packages/opencode/src/config/markdown.ts
//! (`parse` reads a file and wraps failures in a `FrontmatterError`).
//! The delimited-split behavior mirrors `gray-matter` (the underlying parser
//! used by the reference).

use serde_json::Value;
use std::path::Path;

const OPEN: &str = "---";
const CLOSE: &str = "\n---";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markdown {
    pub data: Value,
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct FrontmatterError {
    pub path: Option<String>,
    pub message: String,
}

/// Parse frontmatter from a string.
pub fn parse_str(content: &str) -> Result<Markdown, FrontmatterError> {
    match parse_inner(content) {
        Ok(md) => Ok(md),
        Err(_) => parse_inner(&sanitize(content)).map_err(|message| FrontmatterError {
            path: None,
            message,
        }),
    }
}

/// Parse frontmatter, returning `None` when the content is invalid.
/// From reference/packages/core/src/config/markdown.ts (`parseOption`).
pub fn parse_option(content: &str) -> Option<Markdown> {
    parse_str(content).ok()
}

/// Parse a markdown file's frontmatter.
/// From reference/packages/opencode/src/config/markdown.ts (`parse`).
pub fn parse_file(path: &Path) -> Result<Markdown, FrontmatterError> {
    let content = std::fs::read_to_string(path).map_err(|error| FrontmatterError {
        path: Some(path.display().to_string()),
        message: error.to_string(),
    })?;
    parse_str(&content).map_err(|error| FrontmatterError {
        path: Some(path.display().to_string()),
        message: format!(
            "{}: Failed to parse YAML frontmatter: {}",
            path.display(),
            error.message
        ),
    })
}

fn parse_inner(content: &str) -> Result<Markdown, String> {
    if content.is_empty() {
        return Ok(Markdown {
            data: Value::Object(Default::default()),
            content: String::new(),
        });
    }
    if !content.starts_with(OPEN) {
        return Ok(Markdown {
            data: Value::Null,
            content: content.to_string(),
        });
    }
    if content.as_bytes().get(3) == Some(&b'-') {
        return Ok(Markdown {
            data: Value::Null,
            content: content.to_string(),
        });
    }

    let mut rest = &content[OPEN.len()..];
    let first_line_end = rest.find(['\r', '\n']);
    if let Some(position) = first_line_end {
        let raw = &rest[..position];
        if !raw.trim().is_empty() {
            rest = &rest[raw.len()..];
        }
    }

    let close_index = rest.find(CLOSE).unwrap_or(rest.len());
    let matter = &rest[..close_index];
    let content = if close_index == rest.len() {
        String::new()
    } else {
        let mut body = rest[close_index + CLOSE.len()..].to_string();
        if body.starts_with('\r') {
            body.drain(..1);
        }
        if body.starts_with('\n') {
            body.drain(..1);
        }
        body
    };

    let block = strip_comment_lines(matter);
    let data = if block.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_yml::from_str(matter).map_err(|error| error.to_string())?
    };

    Ok(Markdown { data, content })
}

/// Removes comment-only lines (used for the empty-block check).
/// From reference/packages/opencode/src/command/... `gray-matter`:
/// `file.matter.replace(/^\s*#[^\n]+/gm, '')`.
fn strip_comment_lines(matter: &str) -> String {
    let re = regex::Regex::new(r"(?m)^\s*#[^\n]+").expect("valid comment regex");
    re.replace_all(matter, "").into_owned()
}

/// Retry path for frontmatter whose values contain unquoted colons.
/// From reference/packages/core/src/config/markdown.ts (`sanitize`).
pub fn sanitize(content: &str) -> String {
    let after = match content.strip_prefix(OPEN) {
        Some(after) => after,
        None => return content.to_string(),
    };
    if !after.starts_with('\r') && !after.starts_with('\n') {
        return content.to_string();
    }
    let body_start = 3 + if after.starts_with("\r\n") { 2 } else { 1 };
    let Some(close_offset) = content[body_start..].find(CLOSE) else {
        return content.to_string();
    };
    let close_pos = body_start + close_offset;
    let frontmatter = &content[body_start..close_pos];
    let transformed = transform_frontmatter(frontmatter);
    let mut out = String::with_capacity(content.len() + 16);
    out.push_str(&content[..body_start]);
    out.push_str(&transformed);
    out.push_str(&content[close_pos..]);
    out
}

fn transform_frontmatter(frontmatter: &str) -> String {
    let entry =
        regex::Regex::new(r"^([a-zA-Z_][a-zA-Z0-9_]*)\s*:\s*(.*)$").expect("valid entry regex");
    let mut result: Vec<String> = Vec::new();
    for line in frontmatter.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() || line.starts_with(char::is_whitespace) {
            result.push(line.to_string());
            continue;
        }
        let Some(captures) = entry.captures(line) else {
            result.push(line.to_string());
            continue;
        };
        let value = captures.get(2).expect("value group").as_str().trim();
        if value.is_empty()
            || value == ">"
            || value == "|"
            || value.starts_with('"')
            || value.starts_with('\'')
        {
            result.push(line.to_string());
            continue;
        }
        if !value.contains(':') {
            result.push(line.to_string());
            continue;
        }
        result.push(format!(
            "{}: |-",
            captures.get(1).expect("key group").as_str()
        ));
        result.push(format!("  {value}"));
    }
    result.join("\n")
}
