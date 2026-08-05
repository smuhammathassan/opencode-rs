//! Port of `reference/packages/core/src/ripgrep.ts`.
//!
//! Executes the `rg` binary with the reference's argument vectors and parses
//! its `--json` output into `Entry` / `Match` rows.

use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::model::ToolError;

const ERROR_BYTES: usize = 8 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_SUBMATCHES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
}

impl Entry {
    /// `Entry.make` for the reference `@opencode-ai/schema/filesystem` contract.
    pub fn make(path: impl Into<String>, kind: impl Into<String>) -> Self {
        Entry {
            path: path.into(),
            kind: kind.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Match {
    pub entry: Entry,
    pub line: i64,
    pub offset: i64,
    pub text: String,
    pub submatches: Vec<Submatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Submatch {
    pub text: String,
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone)]
pub struct GlobInput {
    pub cwd: String,
    pub pattern: String,
    pub limit: usize,
    pub hidden: bool,
    pub follow: bool,
}

#[derive(Debug, Clone)]
pub struct GrepInput {
    pub cwd: String,
    pub pattern: String,
    pub file: Option<String>,
    pub include: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct FindInput {
    pub cwd: String,
    pub pattern: String,
    pub limit: usize,
    pub hidden: bool,
    pub follow: bool,
}

/// Mirror of the reference parse function:
/// `.replace(/^(?:\.[\\/])+/u, "").replace(/^[\\/]+/u, "").replaceAll("\\", "/")`.
fn clean_relative(path: &str) -> String {
    let trimmed = path.trim_start_matches(['.', '/', '\\']);
    trimmed.replace('\\', "/")
}

#[derive(Debug, Deserialize)]
struct RawMatch {
    #[serde(rename = "type")]
    kind: String,
    data: RawMatchData,
}

#[derive(Debug, Deserialize)]
struct RawMatchData {
    path: RawText,
    lines: RawText,
    line_number: i64,
    absolute_offset: i64,
    submatches: Vec<RawSubmatch>,
}

#[derive(Debug, Deserialize)]
struct RawText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RawSubmatch {
    #[serde(rename = "match")]
    value: RawText,
    start: i64,
    end: i64,
}

struct RgOutput {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_rg(args: &[&str], cwd: &str) -> Result<RgOutput, ToolError> {
    let output = Command::new("rg")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| ToolError::Other(format!("failed to spawn ripgrep: {error}")))?;
    let stderr =
        String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(ERROR_BYTES)]).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(RgOutput {
        stdout,
        stderr,
        code: output.status.code().unwrap_or(-1),
    })
}

fn invalid_pattern(stderr: &str) -> bool {
    stderr.contains("regex parse error") || stderr.contains("error parsing regex")
}

fn parse_files(out: &RgOutput, limit: usize) -> Result<Vec<Entry>, ToolError> {
    if out.code == 2 && invalid_pattern(&out.stderr) {
        return Err(ToolError::Other(format!(
            "invalid pattern: {}",
            out.stderr.trim()
        )));
    }
    if out.code != 0 && out.code != 1 {
        return Err(ToolError::Other(out.stderr.trim().to_string()));
    }
    Ok(out
        .stdout
        .lines()
        .take(limit)
        .map(|line| Entry::make(clean_relative(line), "file"))
        .collect())
}

/// `Ripgrep.glob` from `reference/packages/core/src/ripgrep.ts:155`.
pub fn glob(input: &GlobInput) -> Result<Vec<Entry>, ToolError> {
    let mut args: Vec<String> = vec!["--no-config".to_string(), "--files".to_string()];
    if input.hidden {
        args.push("--hidden".to_string());
    }
    if input.follow {
        args.push("--follow".to_string());
    }
    args.push(format!("--glob={}", input.pattern));
    args.push("--glob=!**/.git/**".to_string());
    args.push(".".to_string());
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_rg(&arg_refs, &input.cwd)?;
    parse_files(&out, input.limit)
}

/// `Ripgrep.find` from `reference/packages/core/src/ripgrep.ts:187`.
pub fn find(input: &FindInput) -> Result<Vec<Entry>, ToolError> {
    let mut args: Vec<String> = vec!["--no-config".to_string(), "--files".to_string()];
    if input.hidden {
        args.push("--hidden".to_string());
    }
    if input.follow {
        args.push("--follow".to_string());
    }
    if input.pattern != "*" {
        args.push(format!("--glob={}", input.pattern));
    }
    args.push("--glob=!**/.git/**".to_string());
    args.push(".".to_string());
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_rg(&arg_refs, &input.cwd)?;
    parse_files(&out, input.limit)
}

/// `Ripgrep.grep` from `reference/packages/core/src/ripgrep.ts:218`.
pub fn grep(input: &GrepInput) -> Result<Vec<Match>, ToolError> {
    let mut args: Vec<String> = vec![
        "--no-config".to_string(),
        "--json".to_string(),
        "--hidden".to_string(),
        "--no-messages".to_string(),
    ];
    if let Some(include) = &input.include {
        args.push(format!("--glob={include}"));
    }
    args.push("--glob=!**/.git/**".to_string());
    args.push("--".to_string());
    args.push(input.pattern.clone());
    args.push(input.file.clone().unwrap_or_else(|| ".".to_string()));
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run_rg(&arg_refs, &input.cwd)?;
    if out.code == 2 && invalid_pattern(&out.stderr) {
        return Err(ToolError::Other(format!(
            "invalid pattern: {}",
            out.stderr.trim()
        )));
    }
    if out.code != 0 && out.code != 1 {
        return Err(ToolError::Other(out.stderr.trim().to_string()));
    }

    let mut matches = Vec::new();
    for line in out.stdout.lines() {
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_RECORD_BYTES {
            return Err(ToolError::Other(format!(
                "Ripgrep JSON record exceeded {MAX_RECORD_BYTES} bytes"
            )));
        }
        let raw: RawMatch = match serde_json::from_str(line) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        if raw.kind != "match" {
            continue;
        }
        let relative = clean_relative(&raw.data.path.text);
        let mut text = raw.data.lines.text;
        if text.len() > 2_000 {
            text.truncate(2_000);
            text.push_str("...");
        }
        matches.push(Match {
            entry: Entry::make(relative, "file"),
            line: raw.data.line_number,
            offset: raw.data.absolute_offset,
            text,
            submatches: raw
                .data
                .submatches
                .into_iter()
                .take(MAX_SUBMATCHES)
                .map(|sub| Submatch {
                    text: sub.value.text,
                    start: sub.start,
                    end: sub.end,
                })
                .collect(),
        });
        if matches.len() >= input.limit {
            break;
        }
    }
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_resolves_relative_entries() {
        let dir = std::env::temp_dir();
        let results = glob(&GlobInput {
            cwd: dir.to_str().unwrap().to_string(),
            pattern: "*.rs".to_string(),
            limit: 100,
            hidden: false,
            follow: false,
        })
        .expect("glob should run");
        for entry in results {
            assert_eq!(entry.kind, "file");
        }
    }

    #[test]
    fn grep_matches_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "alpha\nbeta\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "fn alpha() {}\n").unwrap();
        let results = grep(&GrepInput {
            cwd: dir.path().to_string_lossy().to_string(),
            pattern: "alpha".to_string(),
            limit: 10,
            file: None,
            include: None,
        })
        .expect("grep should run");
        assert_eq!(results.len(), 2);
        for item in &results {
            assert!(item.line >= 1);
            assert!(item.text.contains("alpha"));
        }
    }
}
