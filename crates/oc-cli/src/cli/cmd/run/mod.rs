//! `opencode run [message..]`
//! From reference/packages/opencode/src/cli/cmd/run.ts.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::cli::args::{Cli, RunArgs};
use crate::cli::cmd::die;
use crate::cli::context::Context;
use crate::cli::ui::{self, Style};

mod client;
mod events;
mod runtime_stdin;
mod tool;
mod types;

pub use client::{AttachClient, LocalClient, RunClient};
pub use types::PromptPart;

const ATTACH_FILE_MAX_BYTES: u64 = 10 * 1024 * 1024;

fn resolve_root() -> PathBuf {
    let pwd = std::env::var_os("PWD").map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidate = pwd.unwrap_or(cwd);
    std::fs::canonicalize(&candidate).unwrap_or(candidate)
}

fn resolve_directory(args: &RunArgs, root: &Path) -> anyhow::Result<Option<PathBuf>> {
    let Some(dir) = &args.dir else {
        if args.attach.is_some() {
            return Ok(None);
        }
        return Ok(Some(root.to_path_buf()));
    };
    if args.attach.is_some() {
        return Ok(Some(PathBuf::from(dir)));
    }
    let target = if Path::new(dir).is_absolute() {
        PathBuf::from(dir)
    } else {
        root.join(dir)
    };
    if let Err(_) = std::env::set_current_dir(&target) {
        ui::error(&format!("Failed to change directory to {dir}"));
        std::process::exit(1);
    }
    std::env::set_var("PWD", &target);
    Ok(Some(std::env::current_dir().unwrap_or(target)))
}

#[derive(Clone)]
struct FilePart {
    url: String,
    filename: String,
    mime: String,
}

async fn attach_files(
    args: &RunArgs,
    root: &Path,
    directory: &Option<PathBuf>,
) -> anyhow::Result<Vec<FilePart>> {
    let mut files: Vec<FilePart> = Vec::new();
    for file_path in &args.file {
        let base = if args.attach.is_some() {
            root
        } else {
            directory.as_deref().unwrap_or(root)
        };
        let resolved = if Path::new(file_path).is_absolute() {
            PathBuf::from(file_path)
        } else {
            base.join(file_path)
        };
        if !resolved.exists() {
            die(&format!("File not found: {file_path}"));
        }
        let metadata = match std::fs::metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(_) => die(&format!("File not found: {file_path}")),
        };
        let is_directory = metadata.is_dir();
        if args.attach.is_some() && is_directory {
            die(&format!(
                "Cannot attach local directory without a shared filesystem: {file_path}"
            ));
        }

        let content = if args.attach.is_some() {
            if metadata.len() > ATTACH_FILE_MAX_BYTES {
                die(&format!(
                    "Cannot attach local file larger than 10 MiB or a special file: {file_path}"
                ));
            }
            Some(match std::fs::read(&resolved) {
                Ok(content) => content,
                Err(_) => die(&format!(
                    "Cannot attach local file larger than 10 MiB or a special file: {file_path}"
                )),
            })
        } else {
            None
        };

        let detected = mime_from_extension(&resolved);
        let text = content
            .as_ref()
            .map(|c| String::from_utf8_lossy(c).to_string());
        let is_utf8 = match (&content, &text) {
            (Some(content), Some(text)) => content.as_slice() == text.as_bytes(),
            _ => false,
        };
        let mime = if args.attach.is_none() {
            if is_directory {
                "application/x-directory".to_string()
            } else {
                "text/plain".to_string()
            }
        } else if is_utf8 {
            "text/plain".to_string()
        } else {
            detected
        };

        let url = match &content {
            Some(content) => format!("data:{mime};base64,{}", base64(content)),
            None => {
                let path = resolved.to_string_lossy();
                if path.starts_with('/') {
                    format!("file://{path}")
                } else {
                    format!("file:///{}", path.replace('\\', "/"))
                }
            }
        };
        files.push(FilePart {
            url,
            filename: resolved
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            mime,
        });
    }
    Ok(files)
}

fn base64(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input)
}

fn mime_from_extension(path: &Path) -> String {
    let mime = mime_guess::from_path(path);
    mime.first_or_octet_stream().to_string()
}

/// Non-interactive permission ruleset, mirroring the reference.
fn default_rules() -> Vec<Value> {
    vec![
        json!({"permission": "question", "action": "deny", "pattern": "*"}),
        json!({"permission": "plan_enter", "action": "deny", "pattern": "*"}),
        json!({"permission": "plan_exit", "action": "deny", "pattern": "*"}),
    ]
}

fn title(args: &RunArgs, message: &str) -> Option<String> {
    let title = args.title.as_deref()?;
    if !title.is_empty() {
        return Some(title.to_string());
    }
    if message.chars().count() > 50 {
        let truncated: String = message.chars().take(50).collect();
        Some(format!("{truncated}..."))
    } else {
        Some(message.to_string())
    }
}

struct ExecuteOpts<'a> {
    args: &'a RunArgs,
    message: String,
    directory: Option<PathBuf>,
    files: Vec<FilePart>,
    auto: bool,
    thinking: bool,
    interactive: bool,
    rules: Vec<Value>,
    /// Present when connecting to a running server (`--attach`).
    attach_url: Option<String>,
    attach_password: Option<String>,
    attach_username: Option<String>,
}

async fn execute(sdk: Box<dyn RunClient>, opts: &ExecuteOpts<'_>) -> anyhow::Result<i32> {
    let Some(sess) = resolve_session(sdk.as_ref(), opts).await? else {
        die("Session not found");
    };
    let session_id = sess.id.clone();
    let format_json = opts.args.format == "json";

    let mut cwd = opts
        .directory
        .clone()
        .or_else(|| sess.directory.clone().map(PathBuf::from));
    if cwd.is_none() && opts.attach_url.is_some() {
        // Mirrors `current(sdk)`: resolve the remote directory.
        cwd = sdk.path_get().await.unwrap_or(None).map(PathBuf::from);
        if cwd.is_none() {
            die("Failed to resolve remote directory");
        }
    }

    // Mirrors `const client = args.attach ? attachSDK(cwd) : sdk`.
    let client: Box<dyn RunClient> = match &opts.attach_url {
        Some(url) => Box::new(AttachClient::new(
            url,
            cwd.map(|d| d.to_string_lossy().to_string()),
            opts.attach_password.as_deref(),
            opts.attach_username.as_deref(),
        )),
        None => sdk,
    };

    let agent = pick_agent(client.as_ref(), opts).await;
    share(client.as_ref(), &session_id, opts).await;

    if !opts.interactive {
        let loop_opts = events::LoopOptions {
            format_json,
            thinking: opts.thinking,
            auto: opts.auto,
            session_id: session_id.clone(),
        };
        let events = client.subscribe().await?;
        // Start the event loop before sending the prompt/command so no events
        // are missed, mirroring the reference (`completed = loop(...)` started
        // before the prompt send). The loop is polled concurrently with the
        // action, but the action result is what gates the exit: the reference
        // awaits `session.command`/`session.prompt` first and only awaits the
        // loop (session idle) in the success path. A failed action must return
        // immediately — it never schedules a run, so the session would never
        // go idle and `join!` would hang forever (F013).
        let loop_fut = events::event_loop(client.as_ref(), events, &loop_opts);
        futures::pin_mut!(loop_fut);

        let prompt_fut = async {
            if let Some(command) = &opts.args.command {
                client
                    .session_command(
                        session_id.clone(),
                        agent.clone(),
                        opts.args.model.clone(),
                        command.clone(),
                        opts.message.clone(),
                        opts.args.variant.clone(),
                    )
                    .await
            } else {
                let model = types::pick_model(opts.args.model.as_deref());
                let parts = build_prompt_parts(opts);
                client
                    .session_prompt(
                        session_id.clone(),
                        agent.clone(),
                        model,
                        opts.args.variant.clone(),
                        parts,
                    )
                    .await
            }
        };
        futures::pin_mut!(prompt_fut);

        let mut loop_result: Option<anyhow::Result<Option<String>>> = None;
        let prompt_result: anyhow::Result<()> = loop {
            tokio::select! {
                biased;
                result = &mut prompt_fut => break result,
                result = &mut loop_fut => {
                    // The session went idle before the action response arrived;
                    // stop driving the loop and wait for the action alone.
                    loop_result = Some(result);
                    break prompt_fut.await;
                }
            }
        };

        if let Err(err) = prompt_result {
            // The session will never go idle; return immediately (the pinned
            // loop future is dropped with this scope, closing the event
            // stream), mirroring the reference's early return.
            if !emit_error(format_json, &err, &session_id) {
                ui::error(&format_run_error(&err));
            }
            return Ok(1);
        }

        let loop_result = match loop_result {
            Some(result) => result,
            None => loop_fut.await,
        };
        if let Err(err) = loop_result {
            return Err(err);
        }
        let session_error = loop_result?;
        if session_error.is_some() {
            return Ok(1);
        }
        return Ok(0);
    }

    unreachable!("interactive mode is handled by run_mini");
}

fn build_prompt_parts(opts: &ExecuteOpts<'_>) -> Vec<PromptPart> {
    let mut parts: Vec<PromptPart> = Vec::new();
    for file in &opts.files {
        parts.push(PromptPart::File {
            url: file.url.clone(),
            filename: file.filename.clone(),
            mime: file.mime.clone(),
        });
    }
    parts.push(PromptPart::Text {
        text: opts.message.clone(),
    });
    parts
}

fn emit_error(format_json: bool, err: &anyhow::Error, session_id: &str) -> bool {
    if !format_json {
        return false;
    }
    let mut extra = serde_json::Map::new();
    extra.insert("error".into(), json!({ "message": err.to_string() }));
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), "error".into());
    obj.insert(
        "timestamp".into(),
        chrono::Utc::now().timestamp_millis().into(),
    );
    obj.insert("sessionID".into(), session_id.into());
    for (k, v) in extra {
        obj.insert(k, v);
    }
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{}", serde_json::Value::Object(obj));
    true
}

fn format_run_error(err: &anyhow::Error) -> String {
    crate::cli::error::format_error(err).unwrap_or_else(|| format!("{err:#}"))
}

async fn resolve_session(
    sdk: &dyn RunClient,
    opts: &ExecuteOpts<'_>,
) -> anyhow::Result<Option<types::SessionInfo>> {
    if let Some(session) = &opts.args.session {
        let current = sdk.session_get(session.clone()).await.ok().flatten();
        let Some(current) = current else {
            die("Session not found");
        };
        if opts.args.fork {
            let forked = sdk.session_fork(current.id.clone()).await.ok().flatten();
            let Some(forked) = forked else {
                return Ok(None);
            };
            return Ok(Some(types::SessionInfo {
                id: forked.id,
                title: forked.title.or(current.title),
                directory: forked.directory.or(current.directory),
                parent_id: None,
                time: None,
            }));
        }
        return Ok(Some(current));
    }

    let base = if opts.args.continue_ {
        sdk.session_list()
            .await
            .ok()
            .and_then(|items| items.into_iter().find(|item| item.parent_id.is_none()))
    } else {
        None
    };

    if let Some(base) = base {
        if opts.args.fork {
            let forked = sdk.session_fork(base.id.clone()).await.ok().flatten();
            let Some(forked) = forked else {
                return Ok(None);
            };
            return Ok(Some(types::SessionInfo {
                id: forked.id,
                title: forked.title.or(base.title),
                directory: forked.directory.or(base.directory),
                parent_id: None,
                time: None,
            }));
        }
        return Ok(Some(base));
    }

    let name = title(opts.args, &opts.message);
    let created = sdk
        .session_create(name, None, None, None, opts.rules.clone())
        .await?;
    Ok(created)
}

/// Whether sharing should be attempted for this run, mirroring the reference
/// `share()` guard in run.ts: `cfg.data.share !== "auto" && !flags.autoShare
/// && !args.share` skips sharing.
fn should_share(config: &Value, auto_share: bool, share_flag: bool) -> bool {
    let share_mode = config.get("share").and_then(Value::as_str).unwrap_or("");
    share_mode == "auto" || auto_share || share_flag
}

async fn share(sdk: &dyn RunClient, session_id: &str, opts: &ExecuteOpts<'_>) {
    let config = sdk.config_get().await.unwrap_or_default();
    // `flags.autoShare` reads `OPENCODE_AUTO_SHARE` (reference runtime-flags).
    let auto_share = matches!(
        std::env::var("OPENCODE_AUTO_SHARE").ok().as_deref(),
        Some("1") | Some("true")
    );
    if !should_share(&config, auto_share, opts.args.share) {
        return;
    }
    match sdk.session_share(session_id.to_string()).await {
        Ok(Some(url)) => {
            ui::println(&[Style::TEXT_INFO_BOLD, "~  ", Style::TEXT_NORMAL, &url]);
        }
        // The reference prints a "disabled" rejection and swallows other
        // errors (`share().catch(...)`).
        Err(err) if err.to_string().contains("disabled") => {
            ui::println(&[
                Style::TEXT_DANGER_BOLD,
                "!  ",
                Style::TEXT_NORMAL,
                &err.to_string(),
            ]);
        }
        _ => {}
    }
}

async fn pick_agent(sdk: &dyn RunClient, opts: &ExecuteOpts<'_>) -> Option<String> {
    let Some(name) = opts.args.agent.clone() else {
        return None;
    };

    let modes = sdk.app_agents().await.ok();
    let Some(modes) = modes else {
        let source = opts.attach_url.as_deref().unwrap_or("the local server");
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!",
            Style::TEXT_NORMAL,
            &format!("failed to list agents from {source}. Falling back to default agent"),
        ]);
        return None;
    };
    let agent = modes.into_iter().find(|a| a.name == name);
    let Some(agent) = agent else {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!",
            Style::TEXT_NORMAL,
            &format!("agent \"{name}\" not found. Falling back to default agent"),
        ]);
        return None;
    };
    if agent.mode == "subagent" {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!",
            Style::TEXT_NORMAL,
            &format!("agent \"{name}\" is a subagent, not a primary agent. Falling back to default agent"),
        ]);
        return None;
    }
    Some(name)
}

pub async fn run(_cli: &Cli, args: &RunArgs) -> anyhow::Result<i32> {
    let raw_message = args
        .message
        .iter()
        .chain(args.dashes.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let interactive = args.mini || args.interactive;
    let auto = args.auto || args.yolo || args.dangerously_skip_permissions;
    let thinking = if interactive {
        args.thinking.unwrap_or(true)
    } else {
        args.thinking.unwrap_or(false)
    };

    let message = args
        .message
        .iter()
        .chain(args.dashes.iter())
        .map(|arg| {
            if arg.contains(' ') {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    if interactive && args.command.is_some() {
        die("--mini cannot be used with --command");
    }
    if args.demo && !interactive {
        die("--demo requires --mini");
    }
    if interactive && args.format == "json" {
        die("--mini cannot be used with --format json");
    }
    if args.replay_limit.is_some() && !interactive {
        die("--replay-limit requires --mini");
    }
    if let Some(limit) = args.replay_limit {
        if limit == 0 {
            die("--replay-limit must be a positive integer");
        }
    }
    if interactive && !std::io::stdout().is_terminal() {
        die("--mini requires a TTY stdout");
    }
    if interactive {
        let stdin = runtime_stdin::resolve_interactive_stdin().map_err(|err| {
            if err.to_string() == runtime_stdin::INTERACTIVE_INPUT_ERROR {
                die(&err.to_string());
            }
            err
        })?;
        stdin.cleanup();
    }

    let root = resolve_root();
    let directory = resolve_directory(args, &root)?;
    let files = attach_files(args, &root, &directory).await?;

    let piped = if std::io::stdin().is_terminal() {
        None
    } else {
        let mut buf = String::new();
        let _ = std::io::stdin().lock().read_to_string(&mut buf);
        Some(buf)
    };
    let message = types::resolve_run_input(Some(message), piped.clone()).unwrap_or_default();
    let _initial_input = types::resolve_run_input(Some(raw_message), piped);

    if message.trim().is_empty() && args.command.is_none() && !interactive {
        die("You must provide a message or a command");
    }
    if args.fork && !args.continue_ && args.session.is_none() {
        die("--fork requires --continue or --session");
    }

    if interactive {
        return run_mini(args, directory, message, files).await;
    }

    let rules = if interactive {
        Vec::new()
    } else {
        default_rules()
    };

    let sdk: Box<dyn RunClient> = if let Some(attach) = &args.attach {
        Box::new(AttachClient::new(
            attach,
            directory.clone().map(|d| d.to_string_lossy().to_string()),
            args.password.as_deref(),
            args.username.as_deref(),
        ))
    } else {
        let ctx = Context::load(std::env::current_dir()?)?;
        match LocalClient::create(&ctx).await {
            Ok(client) => client,
            Err(err) => {
                let message = format!(
                    "{err}\nTry `opencode run --attach <url>` to connect to a running opencode server."
                );
                return Err(anyhow::Error::new(crate::cli::effect_cmd::CliError::new(
                    message,
                )));
            }
        }
    };

    let opts = ExecuteOpts {
        args,
        message,
        directory,
        files,
        auto,
        thinking,
        interactive,
        rules,
        attach_url: args.attach.clone(),
        attach_password: args.password.clone(),
        attach_username: args.username.clone(),
    };
    execute(sdk, &opts).await
}

async fn run_mini(
    args: &RunArgs,
    directory: Option<PathBuf>,
    message: String,
    files: Vec<FilePart>,
) -> anyhow::Result<i32> {
    let (message, initial_parts) = tui_initial_prompt(message, &files);
    let cwd = directory
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let state_dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("opencode");

    let (url, listener) = if let Some(url) = &args.attach {
        (url.clone(), None)
    } else {
        let mut options =
            oc_server::server::ListenOptions::new("127.0.0.1", args.port.unwrap_or(0));
        options.auth = oc_server::auth::AuthConfig::from_env();
        let listener = oc_server::server::listen(options).await?;
        (listener.url.to_string(), Some(listener))
    };

    let result = oc_tui::run_async(oc_tui::TuiInput {
        url,
        directory: directory.map(|path| path.to_string_lossy().into_owned()),
        workspace: None,
        cwd,
        home,
        state_dir,
        config: oc_tui::config::ResolvedConfig::from_environment(),
        continue_session: args.continue_,
        session_id: args.session.clone(),
        agent: args.agent.clone(),
        model: args.model.clone(),
        prompt: (!message.trim().is_empty()).then_some(message),
        initial_parts,
        replay: args.replay.unwrap_or(true),
        replay_limit: args.replay_limit.map(|limit| limit as usize),
    })
    .await;

    if let Some(listener) = listener {
        listener.stop(false).await;
    }
    result.map(|()| 0)
}

/// Build the visible bootstrap prompt and preserve every `--file` argument as
/// a structured part. The marker is only the TUI-side representation; the
/// submit path strips it from text and sends the file URL/mime/name separately.
fn tui_initial_prompt(message: String, files: &[FilePart]) -> (String, Vec<Value>) {
    let mut prompt = message;
    let mut parts = Vec::with_capacity(files.len());

    for (index, file) in files.iter().enumerate() {
        if !prompt.is_empty() || index > 0 {
            prompt.push_str(if index == 0 { "\n\n" } else { "\n" });
        }
        let marker = format!("[file:{}]", file.filename);
        let start = prompt.chars().count();
        prompt.push_str(&marker);
        let end = prompt.chars().count();
        parts.push(json!({
            "type": "file",
            "url": file.url,
            "filename": file.filename,
            "mime": file.mime,
            "source": {
                "type": "file",
                "path": file.filename,
                "text": { "value": marker, "start": start, "end": end }
            }
        }));
    }

    (prompt, parts)
}

#[cfg(test)]
mod tui_initial_prompt_tests {
    use super::{should_share, tui_initial_prompt, FilePart};

    #[test]
    fn share_policy_honors_config_flag_and_auto_share() {
        // `share: "auto"` shares without any flag.
        assert!(should_share(
            &serde_json::json!({ "share": "auto" }),
            false,
            false
        ));
        // Manual/disabled/absent config does not share by default.
        assert!(!should_share(
            &serde_json::json!({ "share": "manual" }),
            false,
            false
        ));
        assert!(!should_share(
            &serde_json::json!({ "share": "disabled" }),
            false,
            false
        ));
        assert!(!should_share(&serde_json::json!({}), false, false));
        // `--share` and `OPENCODE_AUTO_SHARE` (flags.autoShare) both opt in.
        assert!(should_share(
            &serde_json::json!({ "share": "manual" }),
            false,
            true
        ));
        assert!(should_share(
            &serde_json::json!({ "share": "disabled" }),
            true,
            false
        ));
    }

    #[test]
    fn preserves_file_parts_while_showing_markers() {
        let (prompt, parts) = tui_initial_prompt(
            "Review these".to_string(),
            &[FilePart {
                url: "file:///tmp/notes.txt".to_string(),
                filename: "notes.txt".to_string(),
                mime: "text/plain".to_string(),
            }],
        );

        assert_eq!(prompt, "Review these\n\n[file:notes.txt]");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["type"], "file");
        assert_eq!(parts[0]["url"], "file:///tmp/notes.txt");
        assert_eq!(parts[0]["source"]["text"]["value"], "[file:notes.txt]");
        assert_eq!(parts[0]["source"]["text"]["start"], 14);
        assert_eq!(parts[0]["source"]["text"]["end"], 30);
    }

    #[test]
    fn file_only_input_still_has_a_submitable_prompt() {
        let (prompt, parts) = tui_initial_prompt(
            String::new(),
            &[FilePart {
                url: "data:text/plain;base64,YQ==".to_string(),
                filename: "a.txt".to_string(),
                mime: "text/plain".to_string(),
            }],
        );

        assert_eq!(prompt, "[file:a.txt]");
        assert_eq!(parts.len(), 1);
    }
}
