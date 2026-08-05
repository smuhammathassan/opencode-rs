//! Port of `reference/packages/opencode/src/tool/shell.ts`.
//!
//! The shell tool executes one command in the configured shell with timeout
//! and abort handling, streaming output through truncation limits exactly like
//! the reference `run`. Command parsing / permission reduction is ported with
//! a token-based approximation of the reference's web-tree-sitter scan.
//!
//! TODO(integration): port tree-sitter bash/powershell parsing for byte-parity
//! external-directory advisories and `always` pattern prefixes.

use std::collections::{HashMap, HashSet};
use std::process::Stdio;

use serde_json::Value as JsonValue;

use crate::model::{ExecuteResult, PermissionRequest, ToolContext, ToolError};
use crate::shell;
use crate::tool::shell_prompt::{self, Limits};
use crate::tool::tool;
use crate::truncate;

const MAX_METADATA_LENGTH: usize = 30_000;
const DEFAULT_TIMEOUT_MS: u64 = 2 * 60 * 1000;

pub const CWD: [&str; 6] = [
    "cd",
    "chdir",
    "popd",
    "pushd",
    "push-location",
    "set-location",
];
const FILES: [&str; 22] = [
    "cd",
    "chdir",
    "popd",
    "pushd",
    "push-location",
    "set-location",
    "rm",
    "cp",
    "mv",
    "mkdir",
    "touch",
    "chmod",
    "chown",
    "cat",
    "get-content",
    "set-content",
    "add-content",
    "copy-item",
    "move-item",
    "remove-item",
    "new-item",
    "rename-item",
];

/// `ShellTool` from `reference/packages/opencode/src/tool/shell.ts:338`.
pub fn def(default_timeout_ms: Option<u64>) -> tool::Def {
    let default_timeout_ms = default_timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let configured_shell = std::env::var("SHELL").ok();
    let shell = shell::acceptable(&configured_shell);
    let name = shell::name(&shell);
    let platform = platform();
    let limits = Limits {
        max_lines: truncate::MAX_LINES,
        max_bytes: truncate::MAX_BYTES,
    };
    let (description, parameters) =
        shell_prompt::render(&name, &platform, &limits, default_timeout_ms as usize);

    let shell = std::sync::Arc::new(shell);
    let shell_for_execute = shell.clone();
    let execute: tool::ExecuteFn = std::sync::Arc::new(move |args, ctx| {
        let shell = shell_for_execute.clone();
        Box::pin(async move { run_tool(&shell, default_timeout_ms, args, ctx).await })
    });
    let raw = tool::Def {
        id: shell_prompt::TOOL_ID.to_string(),
        description,
        parameters,
        json_schema: None,
        execute,
        format_validation_error: None,
    };
    tool::wrap(shell_prompt::TOOL_ID, raw)
}

async fn run_tool(
    shell: &str,
    default_timeout_ms: u64,
    args: JsonValue,
    ctx: &mut ToolContext,
) -> Result<ExecuteResult, ToolError> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let timeout_param = args
        .get("timeout")
        .and_then(|v| v.as_f64())
        .map(|v| v as i64);
    let workdir = args
        .get("workdir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let instance = ctx.instance.clone().ok_or_else(|| {
        ToolError::Other("InstanceState.context is required for the shell tool".to_string())
    })?;
    let cwd = match &workdir {
        Some(workdir) if std::path::Path::new(workdir).is_absolute() => workdir.clone(),
        Some(workdir) => crate::util::path_resolve(&instance.directory, workdir),
        None => instance.directory.clone(),
    };
    if let Some(timeout) = timeout_param {
        if timeout < 0 {
            return Err(ToolError::Other(format!(
                "Invalid timeout value: {timeout}. Timeout must be a positive number."
            )));
        }
    }
    let timeout = timeout_param.unwrap_or(default_timeout_ms as i64) as u64;

    let ps = shell::ps(shell);
    let scan = collect(&command, &cwd, ps, &instance.directory, shell);
    let dirs: Vec<String> = scan.dirs.into_iter().collect();
    let patterns: Vec<String> = scan.patterns.into_iter().collect();
    let always: Vec<String> = scan.always.into_iter().collect();
    let mut all_dirs = dirs;
    if !crate::util::fs_contains(&instance.directory, &cwd) {
        all_dirs.push(cwd.clone());
    }
    if !all_dirs.is_empty() {
        let globs: Vec<String> = all_dirs.iter().map(|dir| format!("{dir}/*")).collect();
        ctx.ask(PermissionRequest {
            permission: "external_directory".to_string(),
            patterns: globs.clone(),
            always: globs,
            metadata: serde_json::json!({
                "command": command,
                "directories": all_dirs,
                "patterns": patterns,
            }),
        })?;
    }
    if !patterns.is_empty() {
        ctx.ask(PermissionRequest {
            permission: shell_prompt::TOOL_ID.to_string(),
            patterns,
            always,
            metadata: serde_json::json!({
                "command": command,
            }),
        })?;
    }

    run(shell.to_string(), command.clone(), cwd, timeout, ctx).await
}

struct Scan {
    dirs: HashSet<String>,
    patterns: HashSet<String>,
    always: HashSet<String>,
}

/// `collect` from `reference/packages/opencode/src/tool/shell.ts:378` — a
/// token-based stand-in for the tree-sitter scan.
fn collect(command: &str, cwd: &str, ps: bool, instance_directory: &str, shell: &str) -> Scan {
    let mut scan = Scan {
        dirs: HashSet::new(),
        patterns: HashSet::new(),
        always: HashSet::new(),
    };
    let shell_kind = shell_prompt::to_kind(&shell::name(shell));

    for segment in split_segments(command) {
        let tokens = tokenize(&segment);
        if tokens.is_empty() {
            continue;
        }
        let cmd = if ps || shell_kind == "cmd" {
            tokens[0].to_lowercase()
        } else {
            tokens[0].clone()
        };

        let is_file_command = FILES.contains(&cmd.as_str())
            || (shell_kind == "cmd"
                && matches!(
                    cmd.as_str(),
                    "copy"
                        | "del"
                        | "dir"
                        | "erase"
                        | "md"
                        | "mkdir"
                        | "move"
                        | "rd"
                        | "ren"
                        | "rename"
                        | "rmdir"
                        | "type"
                ));
        if is_file_command {
            for arg in path_args(&tokens, ps) {
                if arg.starts_with('-') {
                    continue;
                }
                if ps {
                    let expanded = expand(&arg, cwd, shell);
                    if expanded.is_empty() {
                        continue;
                    }
                }
                let file = if ps { unquote(&arg) } else { unquote(&arg) };
                let resolved = crate::util::path_resolve(cwd, &file);
                if crate::util::fs_contains(instance_directory, &resolved) {
                    continue;
                }
                let dir = if std::path::Path::new(&resolved).is_dir() {
                    resolved
                } else {
                    std::path::Path::new(&resolved)
                        .parent()
                        .map(|parent| parent.to_string_lossy().to_string())
                        .unwrap_or_else(|| resolved.clone())
                };
                scan.dirs.insert(dir);
            }
        }

        if !tokens.is_empty() && !(tokens[0].is_empty()) && !CWD.contains(&cmd.as_str()) {
            scan.patterns.insert(segment.trim().to_string());
            let prefix = bash_arity_prefix(&tokens);
            scan.always.insert(format!("{prefix} *"));
        }
    }
    scan
}

fn split_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in command.chars() {
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            ';' | '|' => {
                if !current.trim().is_empty() {
                    segments.push(current.clone());
                }
                current.clear();
            }
            '\n' => {
                if !current.trim().is_empty() {
                    segments.push(current.clone());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        segments.push(current);
    }
    segments
}

fn tokenize(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in segment.chars() {
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            ' ' | '\t' => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn unquote(text: &str) -> String {
    if text.len() < 2 {
        return text.to_string();
    }
    let first = text.chars().next().unwrap();
    let last = text.chars().last().unwrap();
    if (first == '"' || first == '\'') && first == last {
        return text[1..text.len() - 1].to_string();
    }
    text.to_string()
}

fn expand(text: &str, cwd: &str, shell: &str) -> String {
    let expanded = unquote(text).replace("${env:", "$").replace("}", "");
    let _ = (cwd, shell);
    expanded
}

fn path_args(tokens: &[String], ps: bool) -> Vec<String> {
    if !ps {
        return tokens[1..]
            .iter()
            .filter(|item| !item.starts_with('-'))
            .cloned()
            .collect();
    }
    let mut out = Vec::new();
    let mut want = false;
    for item in &tokens[1..] {
        if want {
            out.push(item.clone());
            want = false;
            continue;
        }
        let lower = item.to_lowercase();
        if matches!(
            lower.as_str(),
            "-confirm" | "-debug" | "-force" | "-nonewline" | "-recurse" | "-verbose" | "-whatif"
        ) {
            continue;
        }
        if matches!(lower.as_str(), "-destination" | "-literalpath" | "-path") {
            want = true;
            continue;
        }
        out.push(item.clone());
    }
    out
}

/// `preview` from `reference/packages/opencode/src/tool/shell.ts:220`.
pub fn preview(text: &str) -> String {
    if text.len() <= MAX_METADATA_LENGTH {
        return text.to_string();
    }
    let start = text.len() - MAX_METADATA_LENGTH;
    format!("...\n\n{}", &text[start..])
}

/// `tail` from `reference/packages/opencode/src/tool/shell.ts:225`.
pub fn tail(text: &str, max_lines: usize, max_bytes: usize) -> (String, bool) {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() <= max_lines && text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    let mut out: Vec<&str> = Vec::new();
    let mut bytes = 0usize;
    for i in (0..lines.len()).rev() {
        if out.len() >= max_lines {
            break;
        }
        let size = lines[i].len() + if !out.is_empty() { 1 } else { 0 };
        if bytes + size > max_bytes {
            if out.is_empty() {
                let line_bytes = lines[i].as_bytes();
                let mut start = line_bytes.len().saturating_sub(max_bytes);
                while start < line_bytes.len() && line_bytes[start] & 0xc0 == 0x80 {
                    start += 1;
                }
                let tail = String::from_utf8_lossy(&line_bytes[start..]).to_string();
                out.push(Box::leak(tail.into_boxed_str()));
            }
            break;
        }
        out.insert(0, lines[i]);
        bytes += size;
    }
    (out.join("\n"), true)
}

struct Chunk {
    text: String,
    size: usize,
}

/// `run` from `reference/packages/opencode/src/tool/shell.ts:428`.
pub async fn run(
    shell: String,
    command: String,
    cwd: String,
    timeout: u64,
    ctx: &mut ToolContext,
) -> Result<ExecuteResult, ToolError> {
    let limits = truncate::limits();
    let _keep = limits.1 * 2;
    let mut list: Vec<Chunk> = Vec::new();
    let mut used = 0usize;
    let mut last = String::new();
    let mut full = String::new();
    let mut file: Option<String> = None;
    let mut sink: Option<std::fs::File> = None;
    let mut cut = false;
    let mut expired = false;
    let mut aborted = false;

    ctx.metadata(crate::model::Metadata {
        title: None,
        metadata: serde_json::json!({ "output": "" }),
    })?;

    let code: Option<i32> = {
        let mut process = tokio::process::Command::new(&shell);
        process
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if shell::ps(&shell) {
            process.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &command,
            ]);
        } else {
            process.args(["-c", &command]);
        }
        let mut child = process
            .spawn()
            .map_err(|error| ToolError::Other(format!("Unable to execute command: {error}")))?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let tx_out = tx.clone();
        let tx_err = tx;
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut buffer = vec![0u8; 8192];
            loop {
                match tokio::io::AsyncReadExt::read(&mut reader, &mut buffer[..]).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx_out.send(buffer[..n].to_vec());
                    }
                    Err(_) => break,
                }
            }
        });
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr);
            let mut buffer = vec![0u8; 8192];
            loop {
                match tokio::io::AsyncReadExt::read(&mut reader, &mut buffer[..]).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx_err.send(buffer[..n].to_vec());
                    }
                    Err(_) => break,
                }
            }
        });

        let abort = ctx.aborted.clone();
        let abort_future = async move {
            while !abort.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        };

        let timeout_duration = std::time::Duration::from_millis(timeout + 100);
        let exit = tokio::time::timeout(timeout_duration, async {
            tokio::select! {
                status = child.wait() => Ok::<_, ()>(status.map(|s| s.code())),
                _ = abort_future => Err(()),
            }
        })
        .await;

        // Drain remaining output chunks before finishing.
        while let Ok(chunk) = rx.try_recv() {
            process_chunk(
                &mut list, &mut used, &mut last, &mut full, &mut file, &mut sink, &mut cut, chunk,
                &limits, ctx,
            )?;
        }

        let status = match exit {
            Ok(Ok(Ok(status))) => status,
            Ok(Ok(Err(_))) | Ok(Err(())) => {
                aborted = true;
                let _ = child.kill().await;
                None
            }
            Err(_) => {
                expired = true;
                let _ = child.kill().await;
                None
            }
        };

        // Drain remaining output chunks after the process exits.
        while let Some(chunk) = rx.recv().await {
            process_chunk(
                &mut list, &mut used, &mut last, &mut full, &mut file, &mut sink, &mut cut, chunk,
                &limits, ctx,
            )?;
        }
        if let Some(sink) = sink.take() {
            drop(sink);
        }
        status
    };

    let mut meta: Vec<String> = Vec::new();
    if expired {
        meta.push(format!(
            "shell tool terminated command after exceeding timeout {timeout} ms. If this command is expected to take longer and is not waiting for interactive input, retry with a larger timeout value in milliseconds."
        ));
    }
    if aborted {
        meta.push("User aborted the command".to_string());
    }

    let raw: String = list.iter().map(|chunk| chunk.text.clone()).collect();
    let (end_text, end_cut) = tail(&raw, limits.0, limits.1);
    if end_cut {
        cut = true;
    }
    if file.is_none() && end_cut {
        file = truncate::write(&raw).ok();
    }

    let mut output = end_text;
    if output.is_empty() {
        output = "(no output)".to_string();
    }
    if cut {
        if let Some(file) = &file {
            output = format!("...output truncated...\n\nFull output saved to: {file}\n\n{output}");
        }
    }
    if !meta.is_empty() {
        output.push_str(&format!(
            "\n\n<shell_metadata>\n{}\n</shell_metadata>",
            meta.join("\n")
        ));
    }

    let final_preview = if last.is_empty() {
        preview(&output)
    } else {
        last
    };
    let mut metadata = serde_json::Map::new();
    metadata.insert("output".to_string(), JsonValue::String(final_preview));
    metadata.insert(
        "exit".to_string(),
        code.map(JsonValue::from).unwrap_or(JsonValue::Null),
    );
    metadata.insert("truncated".to_string(), JsonValue::Bool(cut));
    if cut {
        if let Some(path) = &file {
            metadata.insert("outputPath".to_string(), JsonValue::String(path.clone()));
        }
    }

    Ok(ExecuteResult {
        title: command,
        metadata: JsonValue::Object(metadata),
        output,
        attachments: None,
    })
}

fn process_chunk(
    list: &mut Vec<Chunk>,
    used: &mut usize,
    last: &mut String,
    full: &mut String,
    file: &mut Option<String>,
    sink: &mut Option<std::fs::File>,
    cut: &mut bool,
    bytes: Vec<u8>,
    limits: &(usize, usize),
    ctx: &mut ToolContext,
) -> Result<(), ToolError> {
    let text = String::from_utf8_lossy(&bytes).to_string();
    let size = text.len();
    list.push(Chunk {
        text: text.clone(),
        size,
    });
    *used += size;
    while *used > limits.1 * 2 && list.len() > 1 {
        let item = list.remove(0);
        *used = used.saturating_sub(item.size);
        *cut = true;
    }
    *last = preview(&format!("{last}{text}"));

    if file.is_some() {
        if let Some(sink) = sink.as_mut() {
            use std::io::Write;
            let _ = sink.write_all(bytes.as_slice());
        }
    } else {
        full.push_str(&text);
        if full.len() > limits.1 {
            if let Ok(path) = truncate::write(full) {
                *file = Some(path.clone());
                *cut = true;
                let handle = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path)
                    .ok();
                *sink = handle;
                full.clear();
            }
        }
    }
    ctx.metadata(crate::model::Metadata {
        title: None,
        metadata: serde_json::json!({ "output": last.clone() }),
    })
}

/// `BashArity.prefix` from `reference/packages/opencode/src/permission/arity.ts`.
pub fn bash_arity_prefix(tokens: &[String]) -> String {
    for len in (1..=tokens.len()).rev() {
        let prefix = tokens[..len].join(" ");
        if let Some(arity) = arity_table().get(prefix.as_str()) {
            let arity = *arity;
            return tokens[..arity.min(tokens.len())].join(" ");
        }
    }
    if tokens.is_empty() {
        return String::new();
    }
    tokens[..1].join(" ")
}

fn platform() -> String {
    if cfg!(target_os = "windows") {
        "win32".to_string()
    } else if cfg!(target_os = "macos") {
        "darwin".to_string()
    } else {
        "linux".to_string()
    }
}

static ARITY: std::sync::OnceLock<HashMap<&'static str, usize>> = std::sync::OnceLock::new();

fn arity_table() -> &'static HashMap<&'static str, usize> {
    ARITY.get_or_init(|| {
        [
            ("cat", 1),
            ("cd", 1),
            ("chmod", 1),
            ("chown", 1),
            ("cp", 1),
            ("echo", 1),
            ("env", 1),
            ("export", 1),
            ("grep", 1),
            ("kill", 1),
            ("killall", 1),
            ("ln", 1),
            ("ls", 1),
            ("mkdir", 1),
            ("mv", 1),
            ("ps", 1),
            ("pwd", 1),
            ("rm", 1),
            ("rmdir", 1),
            ("sleep", 1),
            ("source", 1),
            ("tail", 1),
            ("touch", 1),
            ("unset", 1),
            ("which", 1),
            ("aws", 3),
            ("az", 3),
            ("bazel", 2),
            ("brew", 2),
            ("bun", 2),
            ("bun run", 3),
            ("bun x", 3),
            ("cargo", 2),
            ("cargo add", 3),
            ("cargo run", 3),
            ("cdk", 2),
            ("cf", 2),
            ("cmake", 2),
            ("composer", 2),
            ("consul", 2),
            ("consul kv", 3),
            ("crictl", 2),
            ("deno", 2),
            ("deno task", 3),
            ("doctl", 3),
            ("docker", 2),
            ("docker builder", 3),
            ("docker compose", 3),
            ("docker container", 3),
            ("docker image", 3),
            ("docker network", 3),
            ("docker volume", 3),
            ("eksctl", 2),
            ("eksctl create", 3),
            ("firebase", 2),
            ("flyctl", 2),
            ("gcloud", 3),
            ("gh", 3),
            ("git", 2),
            ("git config", 3),
            ("git remote", 3),
            ("git stash", 3),
            ("go", 2),
            ("gradle", 2),
            ("helm", 2),
            ("heroku", 2),
            ("hugo", 2),
            ("ip", 2),
            ("ip addr", 3),
            ("ip link", 3),
            ("ip netns", 3),
            ("ip route", 3),
            ("kind", 2),
            ("kind create", 3),
            ("kubectl", 2),
            ("kubectl kustomize", 3),
            ("kubectl rollout", 3),
            ("kustomize", 2),
            ("make", 2),
            ("mc", 2),
            ("mc admin", 3),
            ("minikube", 2),
            ("mongosh", 2),
            ("mysql", 2),
            ("mvn", 2),
            ("ng", 2),
            ("npm", 2),
            ("npm exec", 3),
            ("npm init", 3),
            ("npm run", 3),
            ("npm view", 3),
            ("nvm", 2),
            ("nx", 2),
            ("openssl", 2),
            ("openssl req", 3),
            ("openssl x509", 3),
            ("pip", 2),
            ("pipenv", 2),
            ("pnpm", 2),
            ("pnpm dlx", 3),
            ("pnpm exec", 3),
            ("pnpm run", 3),
            ("poetry", 2),
            ("podman", 2),
            ("podman container", 3),
            ("podman image", 3),
            ("psql", 2),
            ("pulumi", 2),
            ("pulumi stack", 3),
            ("pyenv", 2),
            ("python", 2),
            ("rake", 2),
            ("rbenv", 2),
            ("redis-cli", 2),
            ("rustup", 2),
            ("serverless", 2),
            ("sfdx", 3),
            ("skaffold", 2),
            ("sls", 2),
            ("sst", 2),
            ("swift", 2),
            ("systemctl", 2),
            ("terraform", 2),
            ("terraform workspace", 3),
            ("tmux", 2),
            ("turbo", 2),
            ("ufw", 2),
            ("vault", 2),
            ("vault auth", 3),
            ("vault kv", 3),
            ("vercel", 2),
            ("volta", 2),
            ("wp", 2),
            ("yarn", 2),
            ("yarn dlx", 3),
            ("yarn run", 3),
        ]
        .into_iter()
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_description() {
        let tool = def(None);
        assert!(tool
            .description
            .starts_with("Executes a given bash command"));
        assert!(tool
            .description
            .contains("commands will time out after 120000ms."));
    }

    #[test]
    fn arity_prefix_picks_subcommand() {
        assert_eq!(
            bash_arity_prefix(&["git".into(), "checkout".into(), "main".into()]),
            "git checkout"
        );
        assert_eq!(
            bash_arity_prefix(&["npm".into(), "run".into(), "dev".into()]),
            "npm run dev"
        );
        assert_eq!(bash_arity_prefix(&["ls".into(), "-la".into()]), "ls");
    }

    #[test]
    fn tail_truncates() {
        let text = (0..5000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (out, cut) = tail(&text, 2000, 50 * 1024);
        assert!(cut);
        assert!(out.ends_with("line 4999"));
    }

    #[tokio::test]
    async fn executes_and_returns_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let def = crate::tool::tool::wrap("bash", def(None));
        let mut ctx = crate::model::ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let result = def
            .execute(serde_json::json!({ "command": "echo hello" }), &mut ctx)
            .await
            .unwrap();
        assert_eq!(result.output.trim(), "hello");
        assert_eq!(result.metadata["exit"], 0);
        assert_eq!(result.metadata["truncated"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn reports_timeout_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let def = crate::tool::tool::wrap("bash", def(None));
        let mut ctx = crate::model::ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let result = def
            .execute(
                serde_json::json!({ "command": "sleep 5", "timeout": 100 }),
                &mut ctx,
            )
            .await
            .unwrap();
        assert!(result.output.contains("<shell_metadata>"));
        assert!(result.output.contains("exceeding timeout 100 ms"));
    }
}
