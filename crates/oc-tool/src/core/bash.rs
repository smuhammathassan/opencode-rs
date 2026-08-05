//! Port of `reference/packages/core/src/tool/bash.ts` (V2 core `bash` leaf).

use std::process::Stdio;

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, ToolError};
use crate::schema::{opt_prop, prop, Schema};

pub const NAME: &str = "bash";
pub const DEFAULT_TIMEOUT_MS: u64 = 2 * 60 * 1_000;
pub const MAX_TIMEOUT_MS: u64 = 10 * 60 * 1_000;
pub const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

/// `Input` from `reference/packages/core/src/tool/bash.ts:23`.
pub fn input() -> Schema {
    Schema::struct_(
        vec![
            prop("command", Schema::string("Shell command string to execute")),
            opt_prop(
                "workdir",
                Schema::string("Working directory. Defaults to the active Location; relative paths resolve from that Location."),
            ),
            opt_prop(
                "timeout",
                Schema::positive_int().with_description(
                    format!("Timeout in milliseconds. Defaults to {DEFAULT_TIMEOUT_MS} and may not exceed {MAX_TIMEOUT_MS}."),
                ),
            ),
        ],
        "bash",
    )
}

/// `Output` from `reference/packages/core/src/tool/bash.ts:41`.
pub fn output_schema() -> Schema {
    Schema::struct_(
        vec![
            opt_prop("exit", Schema::integer()),
            prop("output", Schema::plain_string()),
            prop("truncated", Schema::plain_boolean()),
            opt_prop("timeout", Schema::plain_boolean()),
            opt_prop(
                "warnings",
                Schema::array(Schema::plain_string(), "warnings"),
            ),
        ],
        "bash",
    )
}

/// `modelOutput` from `reference/packages/core/src/tool/bash.ts:51`.
pub fn model_output(output: &serde_json::Value) -> String {
    let warnings = output
        .get("warnings")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|warning| format!("- {warning}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|joined| !joined.is_empty());
    let warnings_text = warnings
        .as_ref()
        .map(|warnings| format!("\n\nWarnings:\n{warnings}"))
        .unwrap_or_default();
    if output
        .get("timeout")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return format!(
            "{}Command timed out before completion.",
            warnings_text.trim_start()
        );
    }
    let exit = output.get("exit").and_then(|v| v.as_i64()).unwrap_or(0);
    format!(
        "{}Command exited with code {exit}.",
        warnings_text.trim_start()
    )
}

/// `BashTool` from `reference/packages/core/src/tool/bash.ts:97`.
pub fn def() -> CoreTool {
    tool::make(
        format!("Execute one shell command string with the host user's filesystem, process, and network authority. The active Location is the default working directory. Relative workdir values resolve from that Location. External workdir values require external_directory approval; best-effort command-argument path warnings are advisory only. Timeout values are milliseconds (default: {DEFAULT_TIMEOUT_MS}; maximum: {MAX_TIMEOUT_MS}). Uses the configured shell when set; otherwise uses /bin/sh on POSIX and COMSPEC or cmd.exe on Windows."),
        input(),
        output_schema(),
        Some(bash_structured_schema()),
        Some(std::sync::Arc::new(|_input, output| {
            let mut value = serde_json::Map::new();
            value.insert("truncated".to_string(), output.get("truncated").cloned().unwrap_or(serde_json::json!(false)));
            if let Some(exit) = output.get("exit") {
                value.insert("exit".to_string(), exit.clone());
            }
            if let Some(timeout) = output.get("timeout") {
                value.insert("timeout".to_string(), timeout.clone());
            }
            serde_json::Value::Object(value)
        })),
        Some(std::sync::Arc::new(|_input, output| {
            let text = output.get("output").and_then(|v| v.as_str()).unwrap_or("").to_string();
            vec![
                Content::Text { text },
                Content::Text { text: model_output(output) },
            ]
        })),
        execute,
    )
}

fn bash_structured_schema() -> Schema {
    Schema::struct_(
        vec![
            opt_prop("exit", Schema::integer()),
            prop("truncated", Schema::plain_boolean()),
            opt_prop("timeout", Schema::plain_boolean()),
        ],
        "bash-structured",
    )
}

fn execute(
    input: serde_json::Value,
    context: &mut CoreContext,
) -> Result<serde_json::Value, ToolError> {
    let command = input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let workdir = input
        .get("workdir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let timeout = input
        .get("timeout")
        .and_then(|v| v.as_i64())
        .map(|value| value as u64)
        .unwrap_or(DEFAULT_TIMEOUT_MS);

    let target = match &workdir {
        Some(workdir) => crate::util::path_resolve(&context.location_directory, workdir),
        None => context.location_directory.clone(),
    };
    if !crate::util::fs_contains(&context.location_directory, &target) {
        context.assert(crate::core::tool::CorePermissionRequest {
            action: "external_directory".to_string(),
            resources: vec![format!("{target}/*")],
            save: None,
            metadata: Some(serde_json::json!({ "filepath": target })),
            source: source(context),
        })?;
    }
    context.assert(crate::core::tool::CorePermissionRequest {
        action: NAME.to_string(),
        resources: vec![command.clone()],
        save: Some(vec![command.clone()]),
        metadata: None,
        source: source(context),
    })?;

    if !std::path::Path::new(&target).is_dir() {
        return Err(ToolError::failure(format!(
            "Working directory is not a directory: {target}"
        )));
    }

    let warnings = external_command_directories(&command, &target);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    let result =
        crate::core::tool::run_future(Box::pin(run_command(&shell, &command, &target, timeout)))?;
    let mut output = serde_json::Map::new();
    if result.3 {
        output.insert(
            "output".to_string(),
            serde_json::Value::String(format!(
                "Command exceeded timeout of {timeout} ms. Retry with a larger timeout if the command is expected to take longer."
            )),
        );
        output.insert("truncated".to_string(), serde_json::Value::Bool(false));
        output.insert("timeout".to_string(), serde_json::Value::Bool(true));
    } else {
        if let Some(exit) = result.0 {
            output.insert("exit".to_string(), serde_json::Value::from(exit));
        }
        output.insert(
            "output".to_string(),
            serde_json::Value::String(result.1.clone()),
        );
        output.insert("truncated".to_string(), serde_json::Value::Bool(result.2));
    }
    if !warnings.is_empty() {
        output.insert(
            "warnings".to_string(),
            serde_json::Value::Array(
                warnings
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    Ok(serde_json::Value::Object(output))
}

async fn run_command(
    shell: &str,
    command: &str,
    cwd: &str,
    timeout: u64,
) -> Result<(Option<i32>, String, bool, bool), ToolError> {
    let mut child = tokio::process::Command::new(shell)
        .args(["-c", command])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ToolError::Other(format!("Unable to execute command: {error}")))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_reader = tokio::io::BufReader::new(stdout);
    let mut err_reader = tokio::io::BufReader::new(stderr);

    let output = tokio::time::timeout(std::time::Duration::from_millis(timeout), async {
        tokio::join!(
            tokio::io::AsyncReadExt::read_to_end(&mut out_reader, &mut out),
            tokio::io::AsyncReadExt::read_to_end(&mut err_reader, &mut err),
            child.wait(),
        )
    })
    .await;

    match output {
        Err(_) => {
            let _ = child.kill().await;
            Ok((None, String::new(), false, true))
        }
        Ok((_, _, Ok(status))) => {
            let exit = status.code();
            let text = String::from_utf8_lossy(&out).to_string();
            let truncated = text.len() > MAX_CAPTURE_BYTES;
            let capped = if truncated {
                format!(
                    "{}\n\n[output capture truncated at the in-memory safety limit]",
                    truncate_utf8(&text, MAX_CAPTURE_BYTES)
                )
            } else {
                text
            };
            Ok((exit, capped, truncated, false))
        }
        Ok((_, _, Err(error))) => Err(ToolError::Other(format!(
            "Unable to execute command: {error}"
        ))),
    }
}

fn truncate_utf8(text: &str, maximum: usize) -> String {
    let mut size = 0usize;
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if size + ch.len_utf8() > maximum {
            break;
        }
        size += ch.len_utf8();
        end = index + ch.len_utf8();
    }
    text[..end].to_string()
}

/// Best-effort advisory external-directory scan
/// (`BashTool.externalCommandDirectories`). TODO(integration): parser-based.
fn external_command_directories(command: &str, cwd: &str) -> Vec<String> {
    let mut directories = std::collections::BTreeSet::new();
    for token in command.split_whitespace() {
        let unquoted = token.trim_matches(['\'', '"']);
        if !std::path::Path::new(unquoted).is_absolute() {
            continue;
        }
        if crate::util::fs_contains(cwd, unquoted) {
            continue;
        }
        if let Some(parent) = std::path::Path::new(unquoted).parent() {
            directories.insert(parent.to_string_lossy().to_string());
        }
    }
    directories
        .into_iter()
        .map(|directory| {
            format!(
                "Command argument references external directory {}/. Bash runs with host-user filesystem, process, and network authority; this scan is advisory only.",
                std::path::Path::new(&directory)
                    .join("*")
                    .to_string_lossy()
                    .replace('\\', "/")
            )
        })
        .collect()
}

fn source(context: &CoreContext) -> crate::core::tool::CorePermissionSource {
    crate::core::tool::CorePermissionSource {
        message_id: context.assistant_message_id.clone(),
        call_id: context.tool_call_id.clone(),
    }
}
