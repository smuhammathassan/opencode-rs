//! `opencode attach <url>` and the default TUI command (`opencode [project]`).
//! From reference/packages/opencode/src/cli/cmd/attach.ts and cmd/tui.ts.

use std::io::IsTerminal;
use std::path::PathBuf;

use crate::cli::args::{AttachArgs, Cli};
use crate::cli::effect_cmd::not_wired;
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

    // TODO(integration): validate the session via `oc_client` and launch the TUI
    // via `oc_tui` (`run()` from cli/tui/layer.ts) once those crates land.
    Err(not_wired("attaching the TUI to a running server is not yet wired in this build (TODO(integration): oc-tui/oc-client)"))
}

async fn run_mini_attach(
    args: &AttachArgs,
    directory: Option<PathBuf>,
    no_replay: bool,
) -> anyhow::Result<i32> {
    // TODO(integration): `runMini` from run.ts drives the split-footer
    // interactive mode over an attached server.
    let _ = (args, directory, no_replay);
    Err(not_wired(
        "mini interactive mode is not yet wired in this build (TODO(integration): oc-tui)",
    ))
}

/// Run the default `opencode [project]` command (the TUI thread).
pub async fn run_default_tui(cli: &Cli) -> anyhow::Result<i32> {
    let args = &cli.tui;

    if args.replay == Some(true) {
        ui::error("--replay is not supported; replay is enabled by default");
        return Ok(1);
    }
    let no_replay = args.replay == Some(false) || args.no_replay;

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
        // TODO(integration): `runMini` over the in-process server.
        let _ = no_replay;
        return Err(not_wired(
            "mini interactive mode is not yet wired in this build (TODO(integration): oc-tui)",
        ));
    }

    if no_replay {
        ui::error("--no-replay requires --mini");
        return Ok(1);
    }
    if args.replay_limit.is_some() {
        ui::error("--replay-limit requires --mini");
        return Ok(1);
    }
    if args.demo == Some(true) {
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

    // Mirror the external-server detection in tui.ts: explicit --port/--hostname
    // or mDNS turn the default TUI into a client of an external server.
    let network = resolve_network_options(&args.network, None);
    let external = network.port != 0 || !network.hostname.is_empty() || network.mdns;

    // TODO(integration): launch the TUI via `oc_tui` (`run()` from
    // cli/tui/layer.ts), with an in-process or external server + worker.
    let _ = (external, directory);
    if !std::io::stdout().is_terminal() {
        ui::println(&[Style::TEXT_DIM, "opencode: starting TUI (requires a TTY)"]);
        return Ok(0);
    }
    Err(not_wired(
        "the TUI is not yet wired in this build (TODO(integration): oc-tui)",
    ))
}
