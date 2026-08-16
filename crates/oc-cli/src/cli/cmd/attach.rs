//! `opencode attach <url>` and the default TUI command (`opencode [project]`).
//! From reference/packages/opencode/src/cli/cmd/attach.ts and cmd/tui.ts.

use std::io::IsTerminal;
use std::path::PathBuf;

use crate::cli::args::{AttachArgs, Cli};
use crate::cli::network::resolve_network_options;
use crate::cli::ui::{self, Style};

/// Mirrors `resolveThreadDirectory(project, envPWD, cwd)` in tui.ts.
pub fn resolve_thread_directory(project: Option<&str>, env_pwd: Option<PathBuf>) -> PathBuf {
    let root = env_pwd
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .unwrap_or_else(resolve_root);
    let cwd = std::env::current_dir().unwrap_or_else(|_| root.clone());
    match project {
        Some(project) => {
            let path = if PathBuf::from(project).is_absolute() {
                PathBuf::from(project)
            } else {
                root.join(project)
            };
            std::fs::canonicalize(&path).unwrap_or(path)
        }
        None => cwd,
    }
}

fn resolve_root() -> PathBuf {
    let pwd = std::env::var_os("PWD").map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_default();
    let candidate = pwd.unwrap_or(cwd);
    std::fs::canonicalize(&candidate).unwrap_or(candidate)
}

/// Run the `opencode attach <url>` command.
pub async fn run(_cli: &Cli, args: &AttachArgs) -> anyhow::Result<i32> {
    if args.replay == Some(true) {
        ui::error("--replay is not supported; replay is enabled by default");
        return Ok(1);
    }
    let no_replay = args.replay == Some(false) || args.no_replay;
    if args.replay_limit == Some(0) {
        ui::error("--replay-limit must be a positive integer");
        return Ok(1);
    }

    let directory = if let Some(dir) = &args.dir {
        match std::env::set_current_dir(dir) {
            Ok(()) => Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(dir))),
            Err(_) => Some(PathBuf::from(dir)),
        }
    } else {
        None
    };

    if args.mini {
        return run_mini_attach(args, directory, no_replay).await;
    }

    if no_replay {
        ui::error("--no-replay requires --mini");
        return Ok(1);
    }
    if args.replay_limit.is_some() {
        ui::error("--replay-limit requires --mini");
        return Ok(1);
    }
    if args.fork && !args.continue_ && args.session.is_none() {
        ui::error("--fork requires --continue or --session");
        return Ok(1);
    }

    run_tui(
        args.url.clone(),
        directory,
        args.continue_,
        args.session.clone(),
        None,
        None,
        None,
        true,
        None,
    )
    .await
}

async fn run_mini_attach(
    args: &AttachArgs,
    directory: Option<PathBuf>,
    no_replay: bool,
) -> anyhow::Result<i32> {
    run_tui(
        args.url.clone(),
        directory,
        args.continue_,
        args.session.clone(),
        None,
        None,
        None,
        !no_replay,
        args.replay_limit.map(|limit| limit as usize),
    )
    .await
}

/// Run the default `opencode [project]` command (the TUI thread).
pub async fn run_default_tui(cli: &Cli) -> anyhow::Result<i32> {
    let args = &cli.tui;

    if args.replay == Some(true) {
        ui::error("--replay is not supported; replay is enabled by default");
        return Ok(1);
    }
    let no_replay = args.replay == Some(false) || args.no_replay;

    if args.replay_limit == Some(0) {
        ui::error("--replay-limit must be a positive integer");
        return Ok(1);
    }

    if args.mini {
        let network = [
            "--port",
            "--hostname",
            "--mdns",
            "--no-mdns",
            "--mdns-domain",
            "--cors",
        ]
        .iter()
        .find(|option| {
            std::env::args()
                .skip(1)
                .any(|arg| arg == **option || arg.starts_with(&format!("{option}=")))
        })
        .copied();
        if let Some(network) = network {
            ui::error(&format!("{network} cannot be used with --mini"));
            return Ok(1);
        }
    }

    if no_replay && !args.mini {
        ui::error("--no-replay requires --mini");
        return Ok(1);
    }
    if args.replay_limit.is_some() && !args.mini {
        ui::error("--replay-limit requires --mini");
        return Ok(1);
    }
    if args.demo == Some(true) && !args.mini {
        ui::error("--demo requires --mini");
        return Ok(1);
    }
    if args.fork && !args.continue_ && args.session.is_none() {
        ui::error("--fork requires --continue or --session");
        return Ok(1);
    }

    let directory = resolve_thread_directory(
        args.project.as_deref(),
        std::env::var_os("PWD").map(PathBuf::from),
    );
    if let Err(err) = std::env::set_current_dir(&directory) {
        ui::error(&format!(
            "Failed to change directory to {}",
            directory.display()
        ));
        return Err(err.into());
    }

    // Start the same HTTP server used by `serve` and have the TUI speak its
    // public contract. This keeps local and attached sessions on one path.
    let network = resolve_network_options(&args.network, None);
    if !std::io::stdout().is_terminal() {
        ui::println(&[Style::TEXT_DIM, "opencode: starting TUI (requires a TTY)"]);
        return Ok(0);
    }
    let mut options = oc_server::server::ListenOptions::new(&network.hostname, network.port);
    options.auth = oc_server::auth::AuthConfig::from_env();
    options.cors = oc_server::cors::CorsOptions {
        cors: (!network.cors.is_empty()).then_some(network.cors),
    };
    options.mdns = network.mdns;
    options.mdns_domain = Some(network.mdns_domain);
    let listener = oc_server::server::listen(options).await?;
    let result = run_tui(
        listener.url.to_string(),
        Some(directory.clone()),
        args.continue_,
        args.session.clone(),
        args.agent.clone(),
        args.model.clone(),
        args.prompt.clone(),
        !no_replay,
        args.replay_limit.map(|limit| limit as usize),
    )
    .await;
    listener.stop(false).await;
    result
}

async fn run_tui(
    url: String,
    directory: Option<PathBuf>,
    continue_session: bool,
    session_id: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    prompt: Option<String>,
    replay: bool,
    replay_limit: Option<usize>,
) -> anyhow::Result<i32> {
    if !std::io::stdout().is_terminal() {
        ui::error("interactive TUI requires a terminal");
        return Ok(1);
    }
    oc_tui::run_async(tui_input(
        url,
        directory,
        continue_session,
        session_id,
        agent,
        model,
        prompt,
        replay,
        replay_limit,
    ))
    .await?;
    Ok(0)
}

fn tui_input(
    url: String,
    directory: Option<PathBuf>,
    continue_session: bool,
    session_id: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    prompt: Option<String>,
    replay: bool,
    replay_limit: Option<usize>,
) -> oc_tui::TuiInput {
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
    oc_tui::TuiInput {
        url,
        directory: directory.map(|path| path.to_string_lossy().into_owned()),
        workspace: None,
        cwd,
        home,
        state_dir,
        config: oc_tui::config::ResolvedConfig::from_environment(),
        continue_session,
        session_id,
        agent,
        model,
        prompt,
        initial_parts: Vec::new(),
        replay,
        replay_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::tui_input;
    use std::path::PathBuf;

    #[test]
    fn default_tui_launch_preserves_initial_prompt() {
        let input = tui_input(
            "http://127.0.0.1:0".into(),
            Some(PathBuf::from("/workspace")),
            false,
            None,
            None,
            None,
            Some("summarize this project".into()),
            true,
            None,
        );

        assert_eq!(input.prompt.as_deref(), Some("summarize this project"));
    }
}
