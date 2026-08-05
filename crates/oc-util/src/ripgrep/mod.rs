/// From reference/packages/core/src/ripgrep.ts
///
/// Small core-owned ripgrep execution adapter. It deliberately exposes raw
/// process-oriented rows, not model text or permission behavior.
pub mod binary;

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::util::signal::Signal;

const ERROR_BYTES: usize = 8 * 1024;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_SUBMATCHES: usize = 100;

/// Mirrors `FileSystem.Entry` from `@opencode-ai/schema/filesystem`.
/// `TODO(integration): promote to oc-schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub r#type: String,
}

impl Entry {
    pub fn file(path: String) -> Self {
        Entry {
            path,
            r#type: "file".to_string(),
        }
    }
}

/// Mirrors `FileSystem.Submatch` from `@opencode-ai/schema/filesystem`.
/// `TODO(integration): promote to oc-schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submatch {
    pub text: String,
    pub start: u64,
    pub end: u64,
}

/// Mirrors `FileSystem.Match` from `@opencode-ai/schema/filesystem`.
/// `TODO(integration): promote to oc-schema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub entry: Entry,
    pub line: u64,
    pub offset: u64,
    pub text: String,
    pub submatches: Vec<Submatch>,
}

#[derive(Debug)]
pub struct Error {
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub struct InvalidPatternError {
    pub pattern: String,
    pub message: String,
}

impl fmt::Display for InvalidPatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid ripgrep pattern {:?}: {}",
            self.pattern, self.message
        )
    }
}

impl std::error::Error for InvalidPatternError {}

#[derive(Debug)]
pub enum RipgrepError {
    Error(Error),
    InvalidPattern(InvalidPatternError),
    Aborted,
}

impl fmt::Display for RipgrepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RipgrepError::Error(e) => write!(f, "{e}"),
            RipgrepError::InvalidPattern(e) => write!(f, "{e}"),
            RipgrepError::Aborted => write!(f, "aborted"),
        }
    }
}

impl std::error::Error for RipgrepError {}

impl From<Error> for RipgrepError {
    fn from(e: Error) -> Self {
        RipgrepError::Error(e)
    }
}

impl From<InvalidPatternError> for RipgrepError {
    fn from(e: InvalidPatternError) -> Self {
        RipgrepError::InvalidPattern(e)
    }
}

pub struct FindInput {
    pub cwd: String,
    pub pattern: String,
    pub limit: usize,
    pub hidden: bool,
    pub follow: bool,
    pub signal: Option<Arc<Signal>>,
    pub on_entry: Option<Arc<dyn Fn(&Entry) + Send + Sync>>,
}

pub struct GlobInput {
    pub cwd: String,
    pub pattern: String,
    pub limit: usize,
    pub hidden: bool,
    pub follow: bool,
    pub signal: Option<Arc<Signal>>,
}

pub struct GrepInput {
    pub cwd: String,
    pub pattern: String,
    pub file: Option<String>,
    pub include: Option<String>,
    pub limit: usize,
    pub signal: Option<Arc<Signal>>,
}

#[allow(dead_code)]
struct Outcome<A> {
    items: Vec<A>,
    truncated: bool,
    partial: bool,
}

struct RunInput<A> {
    cwd: String,
    args: Vec<String>,
    limit: usize,
    signal: Option<Arc<Signal>>,
    parse: fn(&str) -> Result<Option<A>, RipgrepError>,
    pattern: Option<String>,
    on_item: Option<Arc<dyn Fn(&A) + Send + Sync>>,
    _marker: std::marker::PhantomData<A>,
}

fn is_invalid_pattern(stderr: &str) -> bool {
    stderr.contains("regex parse error") || stderr.contains("error parsing regex")
}

/// Strips the leading `./` (or `.\`) prefixes, then leading separators, and
/// normalizes separators to `/` — mirroring the reference's parse transforms.
fn normalize_relative(line: &str) -> String {
    let mut line = line.to_string();
    loop {
        if let Some(rest) = line.strip_prefix("./") {
            line = rest.to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix(".\\") {
            line = rest.to_string();
            continue;
        }
        break;
    }
    while line.starts_with('/') || line.starts_with('\\') {
        line = line[1..].to_string();
    }
    line.replace('\\', "/")
}

async fn run<A>(input: RunInput<A>) -> Result<Outcome<A>, RipgrepError>
where
    A: Clone + Send + 'static,
{
    if input.signal.as_ref().is_some_and(|signal| signal.aborted()) {
        return Err(RipgrepError::Aborted);
    }
    let binary = binary::filepath().await.map_err(|e| {
        RipgrepError::Error(Error {
            message: e.to_string(),
        })
    })?;
    let signal = input.signal.clone();
    let program = run_inner(binary, input);
    match signal {
        Some(signal) => {
            tokio::select! {
                result = program => result,
                _ = signal.wait() => Err(RipgrepError::Aborted),
            }
        }
        None => program.await,
    }
}

async fn run_inner<A>(binary: PathBuf, input: RunInput<A>) -> Result<Outcome<A>, RipgrepError>
where
    A: Clone + Send + 'static,
{
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    use tokio::process::Command;

    struct KillOnDrop(tokio::process::Child);

    impl std::ops::Deref for KillOnDrop {
        type Target = tokio::process::Child;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl std::ops::DerefMut for KillOnDrop {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.start_kill();
        }
    }

    let mut child = KillOnDrop(
        Command::new(&binary)
            .args(&input.args)
            .current_dir(&input.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                RipgrepError::Error(Error {
                    message: e.to_string(),
                })
            })?,
    );

    let mut stderr = child.stderr.take().expect("stderr pipe");
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buf).await;
        buf.truncate(ERROR_BYTES);
        buf
    });

    let stdout = child.stdout.take().expect("stdout pipe");
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut observed = 0usize;
    let mut rows = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    continue;
                }
                match (input.parse)(trimmed) {
                    Ok(Some(row)) => {
                        if observed < input.limit {
                            if let Some(on_item) = &input.on_item {
                                on_item(&row);
                            }
                        }
                        observed += 1;
                        rows.push(row);
                        if rows.len() >= input.limit + 1 {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = child.kill().await;
                        return Err(e);
                    }
                }
            }
            Err(e) => {
                let _ = child.kill().await;
                return Err(RipgrepError::Error(Error {
                    message: e.to_string(),
                }));
            }
        }
    }
    drop(reader);

    if rows.len() > input.limit {
        let _ = child.kill().await;
        return Ok(Outcome {
            items: rows[..input.limit].to_vec(),
            truncated: true,
            partial: false,
        });
    }

    let code = match child.wait().await {
        Ok(status) => status
            .code()
            .unwrap_or(if status.signal().is_some() { 1 } else { 0 }),
        Err(e) => {
            return Err(RipgrepError::Error(Error {
                message: e.to_string(),
            }))
        }
    };
    let stderr = stderr_task.await.unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr);

    if let Some(pattern) = &input.pattern {
        if code == 2 && is_invalid_pattern(&stderr_text) {
            return Err(RipgrepError::InvalidPattern(InvalidPatternError {
                pattern: pattern.clone(),
                message: stderr_text.trim().to_string(),
            }));
        }
    }
    if code != 0 && code != 1 && code != 2 {
        let text = stderr_text.trim();
        let message = if text.is_empty() {
            format!("ripgrep failed with code {code}")
        } else {
            text.to_string()
        };
        return Err(RipgrepError::Error(Error { message }));
    }
    Ok(Outcome {
        items: if code == 1 { Vec::new() } else { rows },
        truncated: false,
        partial: code == 2,
    })
}

/// From reference/packages/core/src/ripgrep.ts (`Service.glob`).
pub async fn glob(input: GlobInput) -> Result<Vec<Entry>, RipgrepError> {
    let mut args = vec!["--no-config".to_string(), "--files".to_string()];
    if input.hidden {
        args.push("--hidden".to_string());
    }
    if input.follow {
        args.push("--follow".to_string());
    }
    args.push(format!("--glob={}", input.pattern));
    args.push("--glob=!**/.git/**".to_string());
    args.push(".".to_string());

    let result = run(RunInput {
        cwd: input.cwd,
        args,
        limit: input.limit,
        signal: input.signal,
        parse: |line| Ok(Some(Entry::file(normalize_relative(line)))),
        pattern: Some(input.pattern),
        on_item: None,
        _marker: std::marker::PhantomData,
    })
    .await
    .map_err(|e| match e {
        RipgrepError::InvalidPattern(e) => RipgrepError::Error(Error { message: e.message }),
        other => other,
    })?;
    Ok(result.items)
}

/// From reference/packages/core/src/ripgrep.ts (`Service.find`).
pub async fn find(input: FindInput) -> Result<Vec<Entry>, RipgrepError> {
    let mut args = vec!["--no-config".to_string(), "--files".to_string()];
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

    let result = run(RunInput {
        cwd: input.cwd,
        args,
        limit: input.limit,
        signal: input.signal,
        parse: |line| Ok(Some(Entry::file(normalize_relative(line)))),
        pattern: Some(input.pattern),
        on_item: input.on_entry,
        _marker: std::marker::PhantomData,
    })
    .await
    .map_err(|e| match e {
        RipgrepError::InvalidPattern(e) => RipgrepError::Error(Error { message: e.message }),
        other => other,
    })?;
    Ok(result.items)
}

#[derive(Deserialize)]
struct RawLine {
    #[serde(rename = "type")]
    kind: String,
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct RawData {
    path: RawPath,
    lines: RawLines,
    line_number: u64,
    absolute_offset: u64,
    submatches: Vec<RawSubmatch>,
}

#[derive(Deserialize)]
struct RawPath {
    text: String,
}

#[derive(Deserialize)]
struct RawLines {
    text: String,
}

#[derive(Deserialize)]
struct RawSubmatch {
    #[serde(rename = "match")]
    match_: RawMatchText,
    start: u64,
    end: u64,
}

#[derive(Deserialize)]
struct RawMatchText {
    text: String,
}

#[derive(Clone)]
struct RawMatchData {
    path: String,
    lines: String,
    line_number: u64,
    absolute_offset: u64,
    submatches: Vec<Submatch>,
}

fn parse_raw_match(line: &str) -> Result<Option<RawMatchData>, RipgrepError> {
    if line.len() > MAX_RECORD_BYTES {
        return Err(RipgrepError::Error(Error {
            message: format!("Ripgrep JSON record exceeded {MAX_RECORD_BYTES} bytes"),
        }));
    }
    let raw: RawLine = serde_json::from_str(line).map_err(|_| {
        RipgrepError::Error(Error {
            message: "Invalid ripgrep JSON output".to_string(),
        })
    })?;
    if raw.kind != "match" {
        return Ok(None);
    }
    let data: RawData = serde_json::from_value(raw.data).map_err(|_| {
        RipgrepError::Error(Error {
            message: "Invalid ripgrep match output".to_string(),
        })
    })?;
    let path = data
        .path
        .text
        .strip_prefix("./")
        .or_else(|| data.path.text.strip_prefix(".\\"))
        .unwrap_or(&data.path.text)
        .to_string();
    Ok(Some(RawMatchData {
        path,
        lines: data.lines.text,
        line_number: data.line_number,
        absolute_offset: data.absolute_offset,
        submatches: data
            .submatches
            .into_iter()
            .take(MAX_SUBMATCHES)
            .map(|s| Submatch {
                text: s.match_.text,
                start: s.start,
                end: s.end,
            })
            .collect(),
    }))
}

/// From reference/packages/core/src/ripgrep.ts (`Service.grep`).
pub async fn grep(input: GrepInput) -> Result<Vec<Match>, RipgrepError> {
    let mut args = vec![
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

    let result = run(RunInput {
        cwd: input.cwd,
        args,
        limit: input.limit,
        signal: input.signal,
        parse: parse_raw_match,
        pattern: Some(input.pattern),
        on_item: None,
        _marker: std::marker::PhantomData,
    })
    .await?;

    Ok(result
        .items
        .into_iter()
        .map(|raw| {
            let relative = normalize_relative(&raw.path);
            let text = if raw.lines.chars().count() > 2_000 {
                let head: String = raw.lines.chars().take(2_000).collect();
                format!("{head}...")
            } else {
                raw.lines
            };
            Match {
                entry: Entry::file(relative),
                line: raw.line_number,
                offset: raw.absolute_offset,
                text,
                submatches: raw.submatches,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_tree(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-util-rg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "hello world\nsecond line\n").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "goodbye\nhello again\n").unwrap();
        std::fs::write(dir.join("c.md"), "nothing here\n").unwrap();
        std::fs::write(dir.join(".hidden.txt"), "hello hidden\n").unwrap();
        dir
    }

    #[test]
    fn normalizes_relative_paths() {
        assert_eq!(normalize_relative("./a.txt"), "a.txt");
        assert_eq!(normalize_relative("./sub/./a.txt"), "sub/./a.txt");
        assert_eq!(normalize_relative("/abs/path"), "abs/path");
        assert_eq!(normalize_relative("a\\b\\c"), "a/b/c");
        assert_eq!(normalize_relative(".\\a.txt"), "a.txt");
    }

    #[test]
    fn detects_invalid_pattern_messages() {
        assert!(is_invalid_pattern("regex parse error: ..."));
        assert!(is_invalid_pattern("error parsing regex: ..."));
        assert!(!is_invalid_pattern("no such file"));
    }

    #[tokio::test]
    async fn find_lists_files_respecting_glob() {
        let dir = tmp_tree("find");
        let found = find(FindInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "**/*.txt".to_string(),
            limit: 100,
            hidden: false,
            follow: false,
            signal: None,
            on_entry: None,
        })
        .await
        .unwrap();
        assert!(found.iter().any(|e| e.path == "a.txt"));
        assert!(found.iter().any(|e| e.path == "sub/b.txt"));
        assert!(!found.iter().any(|e| e.path == "c.md"));
    }

    #[tokio::test]
    async fn find_includes_hidden_with_flag() {
        let dir = tmp_tree("hidden");
        let found = find(FindInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "*".to_string(),
            limit: 100,
            hidden: true,
            follow: false,
            signal: None,
            on_entry: None,
        })
        .await
        .unwrap();
        assert!(found.iter().any(|e| e.path == ".hidden.txt"));
    }

    #[tokio::test]
    async fn find_applies_limit_and_calls_on_entry() {
        let dir = tmp_tree("limit");
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let found = find(FindInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "*".to_string(),
            limit: 1,
            hidden: false,
            follow: false,
            signal: None,
            on_entry: Some(Arc::new({
                let seen = Arc::clone(&seen);
                move |entry: &Entry| seen.lock().unwrap().push(entry.path.clone())
            })),
        })
        .await
        .unwrap();
        assert!(found.len() <= 1);
        assert!(seen.lock().unwrap().len() <= 1);
    }

    fn grep_tree(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oc-util-rg-grep-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "hello world\nsecond line\n").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "goodbye\nhello again\n").unwrap();
        dir
    }

    #[tokio::test]
    async fn grep_returns_matches() {
        let dir = grep_tree("basic");
        let matches = grep(GrepInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "hello".to_string(),
            file: None,
            include: None,
            limit: 100,
            signal: None,
        })
        .await
        .unwrap();
        assert_eq!(matches.len(), 2);
        let a = matches.iter().find(|m| m.entry.path == "a.txt").unwrap();
        assert_eq!(a.line, 1);
        assert_eq!(a.submatches[0].text, "hello");
        assert_eq!(a.submatches[0].start, 0);
        let b = matches
            .iter()
            .find(|m| m.entry.path == "sub/b.txt")
            .unwrap();
        assert_eq!(b.line, 2);
        assert_eq!(b.entry.r#type, "file");
    }

    #[tokio::test]
    async fn grep_honors_file_and_include() {
        let dir = grep_tree("file");
        let matches = grep(GrepInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "hello".to_string(),
            file: Some("sub/b.txt".to_string()),
            include: None,
            limit: 100,
            signal: None,
        })
        .await
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].entry.path, "sub/b.txt");
    }

    #[tokio::test]
    async fn grep_invalid_pattern_errors() {
        let dir = grep_tree("badpat");
        let err = grep(GrepInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "[".to_string(),
            file: None,
            include: None,
            limit: 100,
            signal: None,
        })
        .await
        .unwrap_err();
        assert!(
            matches!(err, RipgrepError::InvalidPattern(_)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn grep_no_matches_returns_empty() {
        let dir = grep_tree("nomatch");
        let matches = grep(GrepInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "zzz-nonexistent".to_string(),
            file: None,
            include: None,
            limit: 100,
            signal: None,
        })
        .await
        .unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn glob_returns_entries() {
        let dir = tmp_tree("glob");
        let found = glob(GlobInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "**/*.md".to_string(),
            limit: 100,
            hidden: false,
            follow: false,
            signal: None,
        })
        .await
        .unwrap();
        assert_eq!(found, vec![Entry::file("c.md".to_string())]);
    }

    #[tokio::test]
    async fn abort_stops_search() {
        let dir = tmp_tree("abort");
        let signal = Signal::new();
        signal.trigger();
        let result = find(FindInput {
            cwd: dir.to_string_lossy().into_owned(),
            pattern: "**".to_string(),
            limit: 1000,
            hidden: false,
            follow: false,
            signal: Some(signal),
            on_entry: None,
        })
        .await;
        assert!(
            matches!(result, Err(RipgrepError::Aborted)),
            "got {result:?}"
        );
    }
}
