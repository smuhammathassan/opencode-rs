//! `opencode debug`
//! From reference/packages/opencode/src/cli/cmd/debug/.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::cli::args::{
    Cli, DebugAgentArgs, DebugArgs, DebugCommand, DebugFileArgs, DebugFileCommand, DebugLspArgs,
    DebugLspCommand, DebugRgArgs, DebugRgCommand, DebugSnapshotArgs, DebugSnapshotCommand,
};
use crate::cli::effect_cmd::not_wired;
use crate::cli::paths::GlobalPaths;

pub async fn run(cli: &Cli, args: &DebugArgs) -> anyhow::Result<i32> {
    let Some(command) = &args.command else {
        // Bare `opencode debug` shows help.
        return Err(anyhow::anyhow!("a debug subcommand is required"));
    };
    match command {
        DebugCommand::Config => config(cli).await,
        DebugCommand::Lsp(lsp) => run_lsp(lsp).await,
        DebugCommand::Rg(rg) => run_rg(rg).await,
        DebugCommand::File(file) => run_file(file).await,
        DebugCommand::Scrap => scrap().await,
        DebugCommand::Skill => skill().await,
        DebugCommand::Snapshot(snapshot) => run_snapshot(snapshot).await,
        DebugCommand::Startup => startup(),
        DebugCommand::Agent(agent) => run_agent(agent).await,
        DebugCommand::V2 => v2().await,
        DebugCommand::Info => info(cli).await,
        DebugCommand::Paths => paths(),
        DebugCommand::Wait => wait().await,
    }
}

/// `debug config`: print the resolved config as JSON.
async fn config(_cli: &Cli) -> anyhow::Result<i32> {
    // TODO(integration): resolve via `oc_config` (deep-merged global + project
    // config). Today config resolution is not wired.
    Err(not_wired(
        "config resolution is not yet wired in this build (TODO(integration): oc-config)",
    ))
}

async fn run_lsp(args: &DebugLspArgs) -> anyhow::Result<i32> {
    match &args.command {
        DebugLspCommand::Diagnostics { .. }
        | DebugLspCommand::Symbols { .. }
        | DebugLspCommand::DocumentSymbols { .. } => Err(not_wired(
            "LSP debugging is not yet wired in this build (TODO(integration): oc-llm/oc-core)",
        )),
    }
}

async fn run_rg(args: &DebugRgArgs) -> anyhow::Result<i32> {
    match &args.command {
        DebugRgCommand::Files { .. } | DebugRgCommand::Search { .. } => Err(not_wired(
            "ripgrep debugging is not yet wired in this build (TODO(integration): oc-util/ripgrep)",
        )),
    }
}

async fn run_file(args: &DebugFileArgs) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match &args.command {
        DebugFileCommand::Search { query } => {
            // TODO(integration): fuzzy file search over the worktree.
            let _ = query;
            Err(not_wired("file search is not yet wired in this build (TODO(integration): oc-core/filesystem)"))
        }
        DebugFileCommand::Read { path } => {
            let content =
                std::fs::read(path).map_err(|err| anyhow::anyhow!("Failed to read file: {err}"))?;
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            let output = serde_json::json!({
                "content": encoded,
                "encoding": "base64",
                "mime": mime,
            });
            writeln!(out, "{}", serde_json::to_string_pretty(&output)?)?;
            Ok(0)
        }
        DebugFileCommand::List { path } => {
            let entries: Vec<String> = std::fs::read_dir(path)
                .map_err(|err| anyhow::anyhow!("Failed to list directory: {err}"))?
                .flatten()
                .map(|entry| entry.path().to_string_lossy().to_string())
                .collect();
            writeln!(out, "{}", serde_json::to_string_pretty(&entries)?)?;
            Ok(0)
        }
    }
}

async fn scrap() -> anyhow::Result<i32> {
    Err(not_wired(
        "project listing is not yet wired in this build (TODO(integration): oc-project)",
    ))
}

async fn skill() -> anyhow::Result<i32> {
    Err(not_wired(
        "skill listing is not yet wired in this build (TODO(integration): oc-command)",
    ))
}

async fn run_snapshot(args: &DebugSnapshotArgs) -> anyhow::Result<i32> {
    match &args.command {
        DebugSnapshotCommand::Track
        | DebugSnapshotCommand::Patch { .. }
        | DebugSnapshotCommand::Diff { .. } => Err(not_wired(
            "snapshot debugging is not yet wired in this build (TODO(integration): oc-project)",
        )),
    }
}

fn startup() -> anyhow::Result<i32> {
    println!("{:.3}", started_at().elapsed().as_secs_f64() * 1000.0);
    Ok(0)
}

async fn run_agent(args: &DebugAgentArgs) -> anyhow::Result<i32> {
    let _ = args;
    Err(not_wired(
        "agent debugging is not yet wired in this build (TODO(integration): oc-command)",
    ))
}

async fn v2() -> anyhow::Result<i32> {
    Err(not_wired(
        "catalog debugging is not yet wired in this build (TODO(integration): oc-provider)",
    ))
}

/// `debug info`: version/os/terminal/plugins.
async fn info(cli: &Cli) -> anyhow::Result<i32> {
    let paths = GlobalPaths::load();
    let _ = paths.ensure();
    println!("opencode version: {}", crate::VERSION);
    println!(
        "os: {} {} {}",
        std::env::consts::OS,
        os_release(),
        std::env::consts::ARCH
    );
    let term_program = std::env::var("TERM_PROGRAM").ok().map(|program| {
        if let Ok(version) = std::env::var("TERM_PROGRAM_VERSION") {
            format!("{program} {version}")
        } else {
            program
        }
    });
    let terminal = [term_program, std::env::var("TERM").ok()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" / ");
    println!(
        "terminal: {}",
        if terminal.is_empty() {
            "unknown".to_string()
        } else {
            terminal
        }
    );
    println!("plugins:");
    if cli.global.pure
        || std::env::var("OPENCODE_PURE")
            .map(|v| v == "1")
            .unwrap_or(false)
    {
        println!("external plugins disabled (--pure)");
        return Ok(0);
    }
    // TODO(integration): list `config.plugin_origins` via oc-config/oc-plugin.
    println!("none");
    Ok(0)
}

fn os_release() -> String {
    let data = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    data.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

/// `debug paths`: print the global paths, mirroring `Global.Path`.
fn paths() -> anyhow::Result<i32> {
    let paths = GlobalPaths::load();
    let _ = paths.ensure();
    let entries: Vec<(&str, PathBuf)> = vec![
        ("data", paths.data.clone()),
        ("bin", paths.bin.clone()),
        ("log", paths.log.clone()),
        ("repos", paths.repos.clone()),
        ("cache", paths.cache.clone()),
        ("config", paths.config.clone()),
        ("state", paths.state.clone()),
        ("tmp", paths.tmp.clone()),
        ("home", paths.home.clone()),
    ];
    for (key, value) in entries {
        println!("{key:<10} {}", value.display());
    }
    Ok(0)
}

async fn wait() -> anyhow::Result<i32> {
    // Mirrors `Effect.sleep(Duration.days(1))`.
    tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
    Ok(0)
}

fn started_at() -> &'static Instant {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}
