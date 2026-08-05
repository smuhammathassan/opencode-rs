//! `opencode import <file>`
//! From reference/packages/opencode/src/cli/cmd/import.ts.

use std::path::Path;

use crate::cli::args::{Cli, ImportArgs};
use crate::cli::effect_cmd::not_wired;

/// Extract a share id from a share URL like `https://opncd.ai/share/abc123`.
/// Mirrors `parseShareUrl` in import.ts.
pub fn parse_share_url(url: &str) -> Option<&str> {
    let prefix = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let slug = prefix.split_once('/')?.1;
    slug.strip_prefix("share/").filter(|s| {
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Mirrors `formatImportFileError` in import.ts.
pub fn format_import_file_error(file: &str, err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => format!("File not found: {file}"),
        std::io::ErrorKind::PermissionDenied => {
            "Failed to read file: Permission denied".to_string()
        }
        _ => format!("Failed to read file: {err}"),
    }
}

pub async fn run(_cli: &Cli, args: &ImportArgs) -> anyhow::Result<i32> {
    let file = &args.file;
    if file.starts_with("http://") || file.starts_with("https://") {
        if parse_share_url(file).is_none() {
            println!("Invalid URL format. Expected: <baseUrl>/share/<slug>");
            return Ok(0);
        }
    } else if !Path::new(file).exists() {
        return Err(anyhow::anyhow!(
            "{}",
            format_import_file_error(
                file,
                &std::io::Error::new(std::io::ErrorKind::NotFound, "not found")
            )
        ));
    }

    // TODO(integration): decode the export JSON and persist session/message/part
    // rows via `oc_database`, mirroring import.ts's `runImport`.
    Err(not_wired(
        "session import is not yet wired in this build (TODO(integration): oc-database/oc-session)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_share_urls() {
        assert_eq!(
            parse_share_url("https://opncd.ai/share/abc123"),
            Some("abc123")
        );
        assert_eq!(
            parse_share_url("https://opncd.ai/share/a_b-9"),
            Some("a_b-9")
        );
        assert_eq!(parse_share_url("https://example.com/other"), None);
        assert_eq!(parse_share_url("not-a-url"), None);
    }
}
