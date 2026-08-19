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
use crate::cli::paths::GlobalPaths;
use indexmap::IndexMap;
use oc_config::load::{load_agent_modes, load_agents, load_instance_state, LoadOptions};
use oc_config::v1::lsp::{Entry as LspEntry, Info as LspInfo, Server as LspServer};
use serde_json::{json, Value};

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
        DebugCommand::Scrap => scrap(),
        DebugCommand::Skill => skill().await,
        DebugCommand::Snapshot(snapshot) => run_snapshot(snapshot).await,
        DebugCommand::Startup => startup(),
        DebugCommand::Agent(agent) => run_agent(agent).await,
        DebugCommand::V2 => v2(),
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

/// `debug scrap` lists all known projects.
///
/// Mirrors `reference/packages/opencode/src/cli/cmd/debug/scrap.ts`, which
/// writes `JSON.stringify(await Project.list())`. `Project.list()` returns
/// every persisted project row mapped to `Project.Info`. The Rust CLI opens the
/// same durable SQLite database the server/CLI persist projects into.
fn scrap() -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    let database = oc_database::Database::open(crate::cli::cmd::db::database_path(&ctx))?;
    let projects = project_list(&database);
    println!("{}", serde_json::to_string_pretty(&projects)?);
    Ok(0)
}

/// `Project.Info` projection of the persisted `project` rows, mirroring
/// `Project.fromRow` in `reference/packages/opencode/src/project/project.ts`.
fn project_list(database: &oc_database::Database) -> Vec<serde_json::Value> {
    let Ok(rows) = database.list_projects() else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| {
            let icon = if row.icon_url.is_some()
                || row.icon_url_override.is_some()
                || row.icon_color.is_some()
            {
                let mut icon = serde_json::Map::new();
                if let Some(url) = &row.icon_url {
                    icon.insert("url".into(), json!(url));
                }
                if let Some(override_url) = &row.icon_url_override {
                    icon.insert("override".into(), json!(override_url));
                }
                if let Some(color) = &row.icon_color {
                    icon.insert("color".into(), json!(color));
                }
                Some(Value::Object(icon))
            } else {
                None
            };
            json!({
                "id": row.id,
                "worktree": row.worktree,
                "vcs": row.vcs,
                "name": row.name,
                "icon": icon,
                "time": {
                    "created": row.time_created,
                    "updated": row.time_updated,
                    "initialized": row.time_initialized,
                },
                "sandboxes": row.sandboxes,
                "commands": row.commands,
            })
        })
        .collect()
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

/// `debug v2` prints the v2 catalog alongside default/small model selection.
///
/// Mirrors `reference/packages/opencode/src/cli/cmd/debug/v2.ts`: it outputs a
/// `{ providers, default, small }` object. `providers` is the sorted catalog
/// provider set, `default` is the model the v2 catalog would select with no
/// configuration, and `small` maps each provider id to its preferred
/// small/fast model (or `null`). The port operates on the embedded models.dev
/// catalog rather than a live integration registry, matching the headless
/// scope.
fn v2() -> anyhow::Result<i32> {
    let catalog = oc_provider::models_dev::snapshot().map_err(|error| anyhow::anyhow!(error))?;
    let payload = v2_payload(&catalog);
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(0)
}

/// `SMALL_MODEL_RE` from `reference/packages/core/src/catalog.ts`.
fn small_model_re(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["nano", "flash", "lite", "mini", "haiku", "small", "fast"]
        .iter()
        .any(|word| {
            lower
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w == *word)
        })
}

/// A model is `active` in the v2 projection when the catalog does not flag it
/// as `alpha`/`beta`/`deprecated`.
fn catalog_model_active(model: &oc_provider::models_dev::Model) -> bool {
    !matches!(
        model.status,
        Some(oc_provider::provider::CatalogModelStatus::Alpha)
            | Some(oc_provider::provider::CatalogModelStatus::Beta)
            | Some(oc_provider::provider::CatalogModelStatus::Deprecated)
    )
}

fn catalog_model_text(modalities: &oc_provider::models_dev::Modalities) -> bool {
    modalities
        .input
        .contains(&oc_provider::models_dev::Modality::Text)
        && modalities
            .output
            .contains(&oc_provider::models_dev::Modality::Text)
}

/// Age in months of an ISO `release_date` relative to a fixed anchor date
/// (the 2026-08-15 audit epoch). The reference uses the live clock; an anchor
/// keeps the diagnostic output and tests deterministic.
fn release_age_months(release_date: Option<&str>) -> f64 {
    let Some(ymd) = release_date.and_then(|d| {
        let mut parts = d.split('-');
        let y = parts.next()?.parse::<f64>().ok()?;
        let m = parts.next()?.parse::<f64>().ok()?;
        Some((y, m))
    }) else {
        return -1.0;
    };
    let (y, m) = ymd;
    // Anchor: 2026-08 (the audit date in months).
    let now_months = 2026.0 * 12.0 + 8.0;
    let release_months = y * 12.0 + m;
    now_months - release_months
}

#[derive(Clone)]
struct SmallCandidate {
    id: String,
    cost: f64,
    age: f64,
    small: bool,
}

/// `catalog.model.small(providerID)` from `reference/packages/core/src/catalog.ts`
/// for a single provider in the embedded catalog.
fn small_model_for(
    catalog: &IndexMap<String, oc_provider::models_dev::Provider>,
    provider_id: &str,
) -> Option<String> {
    let provider = catalog.get(provider_id)?;
    // Reference excludes Azure-managed providers from the small-model pick.
    if provider_id == "azure" || provider_id == "azure-cognitive-services" {
        return None;
    }
    // The reference opencode provider prefers `gpt-5-nano` when active.
    if provider_id == "opencode" {
        if let Some(model) = provider.models.get("gpt-5-nano") {
            if catalog_model_active(model) {
                return Some("gpt-5-nano".to_string());
            }
        }
    }

    let mut candidates: Vec<SmallCandidate> = Vec::new();
    for (model_id, model) in &provider.models {
        if !catalog_model_active(model) {
            continue;
        }
        let Some(modalities) = &model.modalities else {
            continue;
        };
        if !catalog_model_text(modalities) {
            continue;
        }
        let cost = model.cost.as_ref().and_then(|c| c.input).unwrap_or(0.0)
            + model.cost.as_ref().and_then(|c| c.output).unwrap_or(0.0);
        let age = release_age_months(model.release_date.as_deref());
        if cost <= 0.0 || age < 0.0 || age > 18.0 {
            continue;
        }
        let name = format!(
            "{} {} {}",
            model.id,
            model.family.clone().unwrap_or_default(),
            model.name
        );
        candidates.push(SmallCandidate {
            id: model_id.clone(),
            cost,
            age,
            small: small_model_re(&name),
        });
    }
    if candidates.is_empty() {
        return None;
    }

    let pick = |items: &mut Vec<SmallCandidate>| -> Option<String> {
        if items.is_empty() {
            return None;
        }
        let max_cost = items.iter().map(|c| c.cost).fold(0.0, f64::max).max(0.01);
        let max_age = items.iter().map(|c| c.age).fold(0.0, f64::max).max(0.01);
        items.sort_by(|a, b| {
            let a_score = (a.cost / max_cost) * 0.8 + (a.age / max_age) * 0.2;
            let b_score = (b.cost / max_cost) * 0.8 + (b.age / max_age) * 0.2;
            a_score
                .partial_cmp(&b_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Some(items[0].id.clone())
    };

    let mut small: Vec<SmallCandidate> = candidates.iter().filter(|c| c.small).cloned().collect();
    if !small.is_empty() {
        return pick(&mut small);
    }
    pick(&mut candidates)
}

/// Computes the default and per-provider small model selection for the v2
/// catalog, mirroring `catalog.model.default()` and `catalog.model.small()`
/// in `reference/packages/core/src/catalog.ts` (no integrations configured).
fn v2_payload(catalog: &IndexMap<String, oc_provider::models_dev::Provider>) -> serde_json::Value {
    // Reference sorts providers by id for the output.
    let mut provider_ids: Vec<&String> = catalog.keys().collect();
    provider_ids.sort();

    let providers = provider_ids
        .iter()
        .map(|id| {
            let provider = &catalog[*id];
            json!({
                "id": provider.id,
                "name": provider.name,
                "env": provider.env,
            })
        })
        .collect::<Vec<_>>();

    // `model.default()` with no config: pick the newest-released active model.
    // Treat all catalog models as enabled (embedded catalog, no allowlists).
    let mut available: Vec<(String, String, &str)> = Vec::new();
    for (provider_id, provider) in catalog {
        for (model_id, model) in &provider.models {
            if catalog_model_active(model) {
                available.push((
                    provider_id.clone(),
                    model_id.clone(),
                    model.release_date.as_deref().unwrap_or(""),
                ));
            }
        }
    }
    available.sort_by(|a, b| b.2.cmp(a.2).then_with(|| a.1.cmp(&b.1)));
    let default = available
        .first()
        .map(|(provider_id, model_id, _)| format!("{provider_id}/{model_id}"))
        .unwrap_or_default();

    // `model.small(providerID)` per provider.
    let small: IndexMap<String, serde_json::Value> = provider_ids
        .iter()
        .map(|id| {
            let model_id = small_model_for(catalog, id);
            (
                (*id).clone(),
                model_id
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            )
        })
        .collect();

    json!({
        "providers": providers,
        "default": default,
        "small": small,
    })
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

    fn catalog_model(
        id: &str,
        name: &str,
        family: Option<&str>,
        release: Option<&str>,
        cost_input: f64,
        cost_output: f64,
    ) -> oc_provider::models_dev::Model {
        oc_provider::models_dev::Model {
            id: id.to_string(),
            name: name.to_string(),
            family: family.map(str::to_string),
            release_date: release.map(str::to_string),
            cost: Some(oc_provider::models_dev::Cost {
                input: Some(cost_input),
                output: Some(cost_output),
                cache_read: None,
                cache_write: None,
                tiers: None,
                context_over_200k: None,
            }),
            modalities: Some(oc_provider::models_dev::Modalities {
                input: vec![oc_provider::models_dev::Modality::Text],
                output: vec![oc_provider::models_dev::Modality::Text],
            }),
            status: None,
            ..Default::default()
        }
    }

    fn provider_with(
        id: &str,
        models: Vec<oc_provider::models_dev::Model>,
    ) -> oc_provider::models_dev::Provider {
        oc_provider::models_dev::Provider {
            id: id.to_string(),
            name: id.to_string(),
            env: Vec::new(),
            api: None,
            npm: None,
            models: models.into_iter().map(|m| (m.id.clone(), m)).collect(),
        }
    }

    #[test]
    fn small_model_prefers_fast_family_within_age_window() {
        let catalog = IndexMap::from([(
            "testco".to_string(),
            provider_with(
                "testco",
                vec![
                    catalog_model(
                        "gpt-7-flash",
                        "GPT-7 Flash",
                        Some("flash"),
                        Some("2026-01-01"),
                        1.0,
                        2.0,
                    ),
                    catalog_model(
                        "gpt-7",
                        "GPT-7",
                        Some("pro"),
                        Some("2026-01-01"),
                        10.0,
                        20.0,
                    ),
                ],
            ),
        )]);
        // The `flash` model matches SMALL_MODEL_RE and is cheaper, so it wins.
        assert_eq!(
            small_model_for(&catalog, "testco").as_deref(),
            Some("gpt-7-flash")
        );
        assert!(small_model_re("gpt-7 flash"));
        assert!(!small_model_re("gpt-7-pro"));
    }

    #[test]
    fn small_model_excludes_azure_and_overage_models() {
        let catalog = IndexMap::from([(
            "azure".to_string(),
            provider_with(
                "azure",
                vec![catalog_model(
                    "gpt-4o-mini",
                    "GPT-4o mini",
                    Some("mini"),
                    Some("2022-01-01"),
                    1.0,
                    2.0,
                )],
            ),
        )]);
        assert_eq!(small_model_for(&catalog, "azure"), None);
        // A model older than 18 months is excluded.
        let catalog = IndexMap::from([(
            "legacy".to_string(),
            provider_with(
                "legacy",
                vec![catalog_model(
                    "old-mini",
                    "Old mini",
                    Some("mini"),
                    Some("2022-01-01"),
                    1.0,
                    2.0,
                )],
            ),
        )]);
        assert_eq!(small_model_for(&catalog, "legacy"), None);
    }

    #[test]
    fn v2_payload_reports_providers_default_and_small() {
        let catalog = IndexMap::from([
            (
                "beta".to_string(),
                provider_with(
                    "beta",
                    vec![catalog_model(
                        "new",
                        "Newest",
                        Some("pro"),
                        Some("2026-07-01"),
                        5.0,
                        5.0,
                    )],
                ),
            ),
            (
                "alpha".to_string(),
                provider_with(
                    "alpha",
                    vec![catalog_model(
                        "mini",
                        "Mini",
                        Some("mini"),
                        Some("2026-01-01"),
                        1.0,
                        1.0,
                    )],
                ),
            ),
        ]);
        let payload = v2_payload(&catalog);
        // Providers are sorted by id.
        assert_eq!(payload["providers"][0]["id"], "alpha");
        assert_eq!(payload["providers"][1]["id"], "beta");
        // Default is the newest-released active model.
        assert_eq!(payload["default"], "beta/new");
        // Small maps each provider to its preferred fast model.
        assert_eq!(payload["small"]["alpha"], "mini");
        assert_eq!(payload["small"]["beta"], "new");
    }

    #[test]
    fn project_list_projects_rows_like_reference_from_row() {
        let db = oc_database::Database::open_memory().unwrap();
        let row = oc_database::tables::ProjectRow {
            id: "proj-1".to_string(),
            worktree: "/work".to_string(),
            vcs: Some("git".to_string()),
            name: Some("my-proj".to_string()),
            icon_url: None,
            icon_url_override: None,
            icon_color: None,
            time_created: 100,
            time_updated: 200,
            time_initialized: Some(300),
            sandboxes: serde_json::json!({}),
            commands: None,
        };
        db.upsert_project(&row).unwrap();
        let projects = project_list(&db);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["id"], "proj-1");
        assert_eq!(projects[0]["worktree"], "/work");
        assert_eq!(projects[0]["vcs"], "git");
        assert_eq!(projects[0]["time"]["created"], 100);
        assert_eq!(projects[0]["time"]["initialized"], 300);
        assert_eq!(projects[0]["icon"], serde_json::Value::Null);
    }
}
