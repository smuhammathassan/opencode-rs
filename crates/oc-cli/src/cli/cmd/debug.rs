//! `opencode debug`
//! From reference/packages/opencode/src/cli/cmd/debug/.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::cli::args::{
    Cli, DebugAgentArgs, DebugArgs, DebugCommand, DebugFileArgs, DebugFileCommand, DebugLspArgs,
    DebugLspCommand, DebugRgArgs, DebugRgCommand, DebugSnapshotArgs, DebugSnapshotCommand,
};
use crate::cli::context::Context;
use crate::cli::models_dev::ModelsDev;
use crate::cli::paths::GlobalPaths;
use oc_config::load::{load_agent_modes, load_agents, load_instance_state, LoadOptions};
use oc_config::v1::lsp::{Entry as LspEntry, Info as LspInfo, Server as LspServer};
use serde_json::json;

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
    let ctx = Context::load(std::env::current_dir()?)?;
    let state = load_instance_state(&LoadOptions {
        directory: ctx.directory.to_string_lossy().into_owned(),
        worktree: Some(ctx.worktree.to_string_lossy().into_owned()),
        ..Default::default()
    })?;
    println!("{}", serde_json::to_string_pretty(&state.config)?);
    Ok(0)
}

async fn run_lsp(args: &DebugLspArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    let state = load_instance_state(&LoadOptions {
        directory: ctx.directory.to_string_lossy().into_owned(),
        worktree: Some(ctx.worktree.to_string_lossy().into_owned()),
        ..Default::default()
    })?;
    let servers = configured_lsp_servers(state.config.lsp.as_ref());
    if servers.is_empty() {
        anyhow::bail!("no enabled LSP server is configured");
    }

    match &args.command {
        DebugLspCommand::Diagnostics { file } => {
            let path = std::path::PathBuf::from(file);
            let server = select_lsp_server(&servers, path.extension().and_then(|v| v.to_str()));
            let adapter = start_debug_lsp(server, &ctx.worktree).await?;
            let uri = file_uri(&path)?;
            let response = adapter
                .request_method(
                    "textDocument/diagnostic",
                    json!({ "textDocument": { "uri": uri } }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            adapter.shutdown().await.ok();
        }
        DebugLspCommand::Symbols { query } => {
            let adapter = start_debug_lsp(&servers[0], &ctx.worktree).await?;
            let response = adapter
                .request_method("workspace/symbol", json!({ "query": query }))
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            adapter.shutdown().await.ok();
        }
        DebugLspCommand::DocumentSymbols { uri } => {
            let adapter = start_debug_lsp(&servers[0], &ctx.worktree).await?;
            let uri = normalize_lsp_uri(uri)?;
            let response = adapter
                .request_method(
                    "textDocument/documentSymbol",
                    json!({ "textDocument": { "uri": uri } }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            adapter.shutdown().await.ok();
        }
    }
    Ok(0)
}

fn configured_lsp_servers(lsp: Option<&LspInfo>) -> Vec<LspServer> {
    let Some(LspInfo::ByLanguage(entries)) = lsp else {
        return Vec::new();
    };
    entries
        .values()
        .filter_map(|entry| match entry {
            LspEntry::Server(server) if !server.disabled.unwrap_or(false) => Some(server.clone()),
            _ => None,
        })
        .filter(|server| !server.command.is_empty())
        .collect()
}

fn select_lsp_server<'a>(servers: &'a [LspServer], extension: Option<&str>) -> &'a LspServer {
    extension
        .and_then(|extension| {
            servers.iter().find(|server| {
                server.extensions.as_ref().is_some_and(|extensions| {
                    extensions.iter().any(|value| {
                        value.trim_start_matches('.') == extension.trim_start_matches('.')
                    })
                })
            })
        })
        .unwrap_or(&servers[0])
}

async fn start_debug_lsp(
    server: &LspServer,
    root: &std::path::Path,
) -> anyhow::Result<oc_project::lsp::LspAdapter> {
    let mut config = oc_project::lsp::LspServerConfig::new(&server.command[0]);
    config.args = server.command[1..].to_vec();
    config.cwd = Some(root.to_path_buf());
    config.initialization_options = server
        .initialization
        .as_ref()
        .map(|values| serde_json::Value::Object(values.clone().into_iter().collect()));
    Ok(oc_project::lsp::LspAdapter::start(config, root).await?)
}

fn file_uri(path: &std::path::Path) -> anyhow::Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    url::Url::from_file_path(&absolute)
        .map(|url| url.to_string())
        .map_err(|_| anyhow::anyhow!("could not convert path to a file URI: {absolute:?}"))
}

fn normalize_lsp_uri(value: &str) -> anyhow::Result<String> {
    if value.starts_with("file://") {
        return Ok(value.to_string());
    }
    file_uri(std::path::Path::new(value))
}

async fn run_rg(args: &DebugRgArgs) -> anyhow::Result<i32> {
    match &args.command {
        DebugRgCommand::Files { query, glob, limit } => {
            let pattern = glob.clone().unwrap_or_else(|| {
                query
                    .as_deref()
                    .map(|query| format!("*{query}*"))
                    .unwrap_or_else(|| "*".to_string())
            });
            let entries = oc_util::ripgrep::find(oc_util::ripgrep::FindInput {
                cwd: std::env::current_dir()?.to_string_lossy().into_owned(),
                pattern,
                limit: limit.unwrap_or(100) as usize,
                hidden: false,
                follow: false,
                signal: None,
                on_entry: None,
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&entries)?);
            Ok(0)
        }
        DebugRgCommand::Search {
            pattern,
            glob,
            limit,
        } => {
            let matches = oc_util::ripgrep::grep(oc_util::ripgrep::GrepInput {
                cwd: std::env::current_dir()?.to_string_lossy().into_owned(),
                pattern: pattern.clone(),
                file: None,
                include: (!glob.is_empty()).then(|| glob.join(",")),
                limit: limit.unwrap_or(100) as usize,
                signal: None,
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&matches)?);
            Ok(0)
        }
    }
}

async fn run_file(args: &DebugFileArgs) -> anyhow::Result<i32> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match &args.command {
        DebugFileCommand::Search { query } => {
            let entries = oc_util::ripgrep::find(oc_util::ripgrep::FindInput {
                cwd: std::env::current_dir()?.to_string_lossy().into_owned(),
                pattern: format!("*{query}*"),
                limit: 100,
                hidden: false,
                follow: false,
                signal: None,
                on_entry: None,
            })
            .await?;
            writeln!(out, "{}", serde_json::to_string_pretty(&entries)?)?;
            Ok(0)
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
    let ctx = Context::load(std::env::current_dir()?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "id": ctx.project.id,
            "directory": ctx.project.directory,
            "worktree": ctx.project.worktree,
            "vcs": match ctx.project.vcs { crate::cli::context::Vcs::Git => "git", crate::cli::context::Vcs::None => "none" },
        }))?
    );
    Ok(0)
}

async fn skill() -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    let settings = oc_command::skill::Settings {
        home: ctx.paths.home.clone(),
        directory: ctx.directory.clone(),
        worktree: ctx.worktree.clone(),
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths: Vec::new(),
        pulled_dirs: Vec::new(),
        config_dirs: None,
    };
    let skills = oc_command::skill::SkillService::load_with_environment(&settings)
        .map(|service| service.all().into_iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    println!("{}", serde_json::to_string_pretty(&skills)?);
    Ok(0)
}

async fn run_snapshot(args: &DebugSnapshotArgs) -> anyhow::Result<i32> {
    let directory = std::env::current_dir()?;
    let runtime = oc_project::runtime::Runtime::new(oc_project::util::config::Config::default());
    let context = runtime
        .load(&directory.to_string_lossy())
        .await
        .map_err(|error| anyhow::anyhow!("failed to load project snapshot context: {error}"))?;
    match &args.command {
        DebugSnapshotCommand::Track => {
            let hash = runtime.snapshot.track(&context).await;
            println!("{}", serde_json::to_string_pretty(&json!({"hash": hash}))?);
        }
        DebugSnapshotCommand::Patch { hash } => {
            let patch = runtime.snapshot.patch(&context, hash).await;
            println!("{}", serde_json::to_string_pretty(&patch)?);
        }
        DebugSnapshotCommand::Diff { hash } => {
            println!("{}", runtime.snapshot.diff(&context, hash).await);
        }
    }
    runtime.dispose(&context).await;
    Ok(0)
}

fn startup() -> anyhow::Result<i32> {
    println!("{:.3}", started_at().elapsed().as_secs_f64() * 1000.0);
    Ok(0)
}

async fn run_agent(args: &DebugAgentArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    let mut agents = load_agents(&ctx.directory)?;
    for (name, info) in load_agent_modes(&ctx.directory)? {
        agents.insert(name, info);
    }
    if args.name == "build" {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"name": "build", "mode": "all"}))?
        );
        return Ok(0);
    }
    let info = agents
        .get(&args.name)
        .ok_or_else(|| anyhow::anyhow!("agent not found: {}", args.name))?;
    if let Some(tool) = &args.tool {
        let registry = oc_tool::core::registry_with_builtins(false, false);
        let definition = registry
            .materialize(&[])
            .definitions
            .into_iter()
            .find(|definition| definition.name == *tool)
            .ok_or_else(|| anyhow::anyhow!("tool not found: {tool}"))?;
        println!("{}", serde_json::to_string_pretty(&definition)?);
    } else {
        println!("{}", serde_json::to_string_pretty(info)?);
    }
    Ok(0)
}

async fn v2() -> anyhow::Result<i32> {
    let paths = GlobalPaths::load();
    let db = ModelsDev::load(&paths).unwrap_or_default();
    let providers = db
        .providers
        .into_iter()
        .map(|(id, provider)| {
            (
                id,
                json!({
                    "id": provider.id,
                    "name": provider.name,
                    "env": provider.env,
                    "models": provider.models,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    println!("{}", serde_json::to_string_pretty(&providers)?);
    Ok(0)
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
    let origins = std::env::current_dir()
        .ok()
        .and_then(|directory| {
            let context = Context::load(directory).ok()?;
            load_instance_state(&LoadOptions {
                directory: context.directory.to_string_lossy().into_owned(),
                worktree: Some(context.worktree.to_string_lossy().into_owned()),
                ..Default::default()
            })
            .ok()
            .map(|state| state.plugin_origins)
        })
        .unwrap_or_default();
    if origins.is_empty() {
        println!("none");
    } else {
        for origin in origins {
            println!(
                "{} [{}] ({})",
                origin.specifier(),
                origin.scope.as_str(),
                origin.source
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn server(extensions: &[&str], command: &[&str]) -> LspServer {
        LspServer {
            command: command.iter().map(|value| (*value).to_string()).collect(),
            extensions: Some(
                extensions
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
            ),
            disabled: None,
            env: None,
            initialization: None,
        }
    }

    #[test]
    fn lsp_server_selection_prefers_matching_extension() {
        let rust = server(&["rs"], &["rust-analyzer"]);
        let typescript = server(&[".ts"], &["typescript-language-server"]);
        let servers = vec![rust, typescript];
        assert_eq!(
            select_lsp_server(&servers, Some("ts")).command[0],
            "typescript-language-server"
        );
        assert_eq!(
            select_lsp_server(&servers, Some("py")).command[0],
            "rust-analyzer"
        );
    }

    #[test]
    fn lsp_uri_normalization_accepts_paths_and_uris() {
        assert_eq!(
            normalize_lsp_uri("file:///tmp/example.rs").unwrap(),
            "file:///tmp/example.rs"
        );
        assert!(normalize_lsp_uri("/tmp/example.rs")
            .unwrap()
            .starts_with("file://"));
    }
}
