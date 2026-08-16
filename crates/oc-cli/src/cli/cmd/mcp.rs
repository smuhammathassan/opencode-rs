//! `opencode mcp`
//! From reference/packages/opencode/src/cli/cmd/mcp.ts.

use crate::cli::args::{
    Cli, McpAddArgs, McpArgs, McpAuthArgs, McpAuthCommand, McpCommand, McpDebugArgs, McpLogoutArgs,
};
use crate::cli::context::{Context, Vcs};
use serde_json::{Map, Value};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub async fn run(_cli: &Cli, args: &McpArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    match &args.command {
        McpCommand::Add(add) => run_add(&ctx, add).await,
        McpCommand::List => run_list(&ctx).await,
        McpCommand::Auth(auth) => run_auth(&ctx, auth).await,
        McpCommand::Logout(logout) => run_logout(&ctx, logout).await,
        McpCommand::Debug(debug) => run_debug(&ctx, debug).await,
    }
}

async fn run_add(ctx: &Context, args: &McpAddArgs) -> anyhow::Result<i32> {
    let command = &args.command;
    if args.name.is_none()
        && (args.url.is_some()
            || !args.env.is_empty()
            || !args.header.is_empty()
            || !command.is_empty())
    {
        return Err(anyhow::anyhow!(
            "A server name is required for non-interactive MCP configuration"
        ));
    }
    if let Some(name) = &args.name {
        let has_command = !command.is_empty();
        if args.url.is_some() == has_command {
            return Err(anyhow::anyhow!(
                "Provide either --url <url> or a command after --"
            ));
        }
        let server = server_value(args)?;
        let path = global_config_path(ctx);
        let mut config = read_config(&path)?;
        let root = config
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} must contain a JSON object", path.display()))?;
        let mcp = root
            .entry("mcp")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("the mcp config must be an object"))?;

        mcp.insert(name.clone(), server);
        write_mcp_config(&path, &config, name)?;
        println!("configured MCP server `{name}` in {}", path.display());
        return Ok(0);
    }

    run_interactive_add(ctx).await
}

async fn run_list(ctx: &Context) -> anyhow::Result<i32> {
    let config = read_merged_config(ctx)?;
    let Some(mcp) = config.get("mcp").and_then(Value::as_object) else {
        println!("No MCP servers configured.");
        return Ok(0);
    };

    let service = mcp_service(ctx, &config)?;
    service.init().await;
    let statuses = service.status().await;
    println!("{:<24} {:<10} {:<22} ENDPOINT", "NAME", "TYPE", "STATUS");
    for (name, value) in mcp {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let endpoint = value
            .get("url")
            .and_then(Value::as_str)
            .or_else(|| {
                value
                    .get("command")
                    .and_then(Value::as_array)
                    .map(|_| "local")
            })
            .unwrap_or("");
        println!(
            "{:<24} {:<10} {:<22} {}",
            name,
            kind,
            status_text(statuses.get(name)),
            endpoint
        );
    }
    service.close_all().await;
    Ok(0)
}

fn project_config_path(ctx: &Context) -> PathBuf {
    for path in oc_config::paths::files("opencode", &ctx.directory, Some(&ctx.worktree)) {
        if path.exists() {
            return path;
        }
    }
    ctx.worktree.join("opencode.json")
}

fn global_config_path(ctx: &Context) -> PathBuf {
    for filename in ["opencode.json", "opencode.jsonc"] {
        let path = ctx.paths.config.join(filename);
        if path.exists() {
            return path;
        }
    }
    ctx.paths.config.join("opencode.json")
}

fn config_files(ctx: &Context) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for filename in ["opencode.json", "opencode.jsonc"] {
        let path = ctx.paths.config.join(filename);
        if path.exists() {
            files.push(path);
            break;
        }
    }
    files.extend(
        oc_config::paths::files("opencode", &ctx.directory, Some(&ctx.worktree))
            .into_iter()
            .filter(|path| path.exists()),
    );
    files
}

fn read_merged_config(ctx: &Context) -> anyhow::Result<Value> {
    let mut merged = Value::Object(Map::new());
    for path in config_files(ctx) {
        merged = oc_config::merge::merge_deep(&merged, &read_config(&path)?);
    }
    Ok(merged)
}

fn read_config(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let source = std::fs::read_to_string(path)?;
    Ok(oc_config::parse::jsonc(
        &source,
        &path.display().to_string(),
    )?)
}

fn write_config(path: &Path, config: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.extension().and_then(|ext| ext.to_str()) == Some("jsonc") && path.exists() {
        if let Some(mcp) = config.get("mcp") {
            let source = std::fs::read_to_string(path)?;
            let (_, patched) = oc_plugin::jsonc::patch_object_property(&source, None, "mcp", mcp)
                .map_err(|error| {
                anyhow::anyhow!("failed to patch {}: {error}", path.display())
            })?;
            std::fs::write(path, patched)?;
            return Ok(());
        }
    }
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(config)?))?;
    Ok(())
}

fn write_mcp_config(path: &Path, config: &Value, name: &str) -> anyhow::Result<()> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("jsonc") && path.exists() {
        if let Some(mcp) = config.get("mcp") {
            let source = std::fs::read_to_string(path)?;
            let parsed = oc_plugin::jsonc::parse(&source)
                .map_err(|error| anyhow::anyhow!("failed to parse {}: {error}", path.display()))?;
            let (object_key, key, value) = if parsed.root.member("mcp").is_some() {
                let server = mcp
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("MCP server `{name}` is missing from config"))?;
                (Some("mcp"), name, server)
            } else {
                (None, "mcp", mcp)
            };
            let (_, patched) = oc_plugin::jsonc::patch_object_property(
                &source, object_key, key, value,
            )
            .map_err(|error| anyhow::anyhow!("failed to patch {}: {error}", path.display()))?;
            std::fs::write(path, patched)?;
            return Ok(());
        }
    }
    write_config(path, config)
}

fn parse_pairs(
    values: &[String],
    flag: &str,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut result = std::collections::BTreeMap::new();
    for value in values {
        let Some((key, value)) = value.split_once('=') else {
            return Err(anyhow::anyhow!(
                "Invalid {flag}: {value}. Expected KEY=VALUE"
            ));
        };
        if key.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Invalid {flag}: {value}={}. Expected KEY=VALUE",
                value
            ));
        }
        result.insert(key.to_string(), value.to_string());
    }
    Ok(result)
}

fn server_value(args: &McpAddArgs) -> anyhow::Result<Value> {
    let has_command = !args.command.is_empty();
    if args.url.is_some() == has_command {
        return Err(anyhow::anyhow!(
            "Provide either --url <url> or a command after --"
        ));
    }
    if let Some(url) = &args.url {
        if url::Url::parse(url).is_err() {
            return Err(anyhow::anyhow!("Invalid URL: {url}"));
        }
        if !args.env.is_empty() {
            return Err(anyhow::anyhow!("--env is only valid for local MCP servers"));
        }
        let headers = parse_pairs(&args.header, "--header")?;
        let mut server = Map::new();
        server.insert("type".into(), Value::String("remote".into()));
        server.insert("url".into(), Value::String(url.clone()));
        if !headers.is_empty() {
            server.insert("headers".into(), serde_json::to_value(headers)?);
        }
        return Ok(Value::Object(server));
    }
    if !args.header.is_empty() {
        return Err(anyhow::anyhow!(
            "--header is only valid for remote MCP servers"
        ));
    }
    let environment = parse_pairs(&args.env, "--env")?;
    let mut server = Map::new();
    server.insert("type".into(), Value::String("local".into()));
    server.insert("command".into(), serde_json::to_value(&args.command)?);
    if !environment.is_empty() {
        server.insert("environment".into(), serde_json::to_value(environment)?);
    }
    Ok(Value::Object(server))
}

async fn run_interactive_add(ctx: &Context) -> anyhow::Result<i32> {
    require_terminal("interactive MCP setup")?;
    let path = if ctx.project.vcs == Vcs::Git {
        println!("Where should this MCP server be configured?");
        println!(
            "  1) Current project ({})",
            project_config_path(ctx).display()
        );
        println!("  2) Global ({})", global_config_path(ctx).display());
        match prompt_required("Location [1]: ")?.as_str() {
            "" | "1" => project_config_path(ctx),
            "2" => global_config_path(ctx),
            other => return Err(anyhow::anyhow!("invalid location: {other}")),
        }
    } else {
        global_config_path(ctx)
    };
    let name = prompt_required("MCP server name: ")?;
    let kind = prompt_required("Server type [local/remote]: ")?.to_lowercase();
    let server = match kind.as_str() {
        "local" => {
            let command = prompt_required("Command: ")?;
            let command = command
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if command.is_empty() {
                return Err(anyhow::anyhow!("a local MCP command is required"));
            }
            let mut server = Map::new();
            server.insert("type".into(), Value::String("local".into()));
            server.insert("command".into(), serde_json::to_value(command)?);
            Value::Object(server)
        }
        "remote" => {
            let url = prompt_required("MCP server URL: ")?;
            if url::Url::parse(&url).is_err() {
                return Err(anyhow::anyhow!("Invalid URL: {url}"));
            }
            let mut server = Map::new();
            server.insert("type".into(), Value::String("remote".into()));
            server.insert("url".into(), Value::String(url));
            if prompt_yes_no("Does this server require OAuth? [y/N]: ")? {
                let mut oauth = Map::new();
                if prompt_yes_no("Do you have a pre-registered client ID? [y/N]: ")? {
                    oauth.insert(
                        "clientId".into(),
                        Value::String(prompt_required("Client ID: ")?),
                    );
                    if prompt_yes_no("Do you have a client secret? [y/N]: ")? {
                        oauth.insert(
                            "clientSecret".into(),
                            Value::String(prompt_required("Client secret: ")?),
                        );
                    }
                }
                server.insert("oauth".into(), Value::Object(oauth));
            }
            Value::Object(server)
        }
        other => {
            return Err(anyhow::anyhow!(
                "server type must be `local` or `remote`, got `{other}`"
            ))
        }
    };

    let mut config = read_config(&path)?;
    let root = config
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} must contain a JSON object", path.display()))?;
    let mcp = root
        .entry("mcp")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("the mcp config must be an object"))?;
    mcp.insert(name.clone(), server);
    write_mcp_config(&path, &config, &name)?;
    println!("configured MCP server `{name}` in {}", path.display());
    Ok(0)
}

fn require_terminal(operation: &str) -> anyhow::Result<()> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{operation} requires an interactive terminal; pass a server name and non-interactive options"
        ))
    }
}

fn prompt_required(prompt: &str) -> anyhow::Result<String> {
    let mut stdout = io::stdout();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut input = String::new();
    let read = io::stdin().read_line(&mut input)?;
    if read == 0 {
        return Err(anyhow::anyhow!(
            "input ended while reading MCP configuration"
        ));
    }
    let input = input.trim().to_string();
    if input.is_empty() {
        return Err(anyhow::anyhow!("a value is required"));
    }
    Ok(input)
}

fn prompt_yes_no(prompt: &str) -> anyhow::Result<bool> {
    let answer = prompt_required(prompt)?.to_lowercase();
    match answer.as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Err(anyhow::anyhow!("please answer yes or no")),
    }
}

fn configured_server(config: &Value, name: &str) -> anyhow::Result<oc_mcp::config::Info> {
    let value = config
        .get("mcp")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(name))
        .ok_or_else(|| anyhow::anyhow!("MCP server `{name}` is not configured"))?;
    if value.get("type").is_none() {
        return Err(anyhow::anyhow!(
            "MCP server `{name}` does not contain a supported `type`"
        ));
    }
    serde_json::from_value(value.clone())
        .map_err(|error| anyhow::anyhow!("invalid MCP server `{name}` configuration: {error}"))
}

fn mcp_service(ctx: &Context, config: &Value) -> anyhow::Result<Arc<oc_mcp::index::Mcp>> {
    let entries = config
        .get("mcp")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|servers| servers.iter())
        .filter(|(_, value)| value.get("type").is_some())
        .map(|(name, value)| {
            let info = serde_json::from_value(value.clone()).map_err(|error| {
                anyhow::anyhow!("invalid MCP server `{name}` configuration: {error}")
            })?;
            Ok((name.clone(), info))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(oc_mcp::index::Mcp::new(
        entries.into_iter().collect(),
        ctx.directory.clone(),
    ))
}

async fn run_auth(ctx: &Context, args: &McpAuthArgs) -> anyhow::Result<i32> {
    if matches!(args.command.as_ref(), Some(McpAuthCommand::List)) {
        return run_auth_list(ctx).await;
    }
    let config = read_merged_config(ctx)?;
    let name = match &args.name {
        Some(name) => name.clone(),
        None => select_oauth_server(&config)?,
    };
    let server = configured_server(&config, &name)?;
    let service = mcp_service(ctx, &config)?;
    if !service.supports_oauth(&name).await? {
        service.close_all().await;
        return match &server {
            oc_mcp::config::Info::Remote(_) => Err(anyhow::anyhow!(
                "MCP server `{name}` has OAuth explicitly disabled"
            )),
            oc_mcp::config::Info::Local(_) => Err(anyhow::anyhow!(
                "MCP server `{name}` is not an OAuth-capable remote server"
            )),
        };
    }

    match service.get_auth_status(&name).await? {
        oc_mcp::index::AuthStatus::Authenticated => {
            println!("MCP server `{name}` is already authenticated.");
            service.close_all().await;
            return Ok(0);
        }
        oc_mcp::index::AuthStatus::Expired => {
            println!("Stored credentials for `{name}` are expired; starting re-authentication.");
        }
        oc_mcp::index::AuthStatus::NotAuthenticated => {}
    }

    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        let started = service.start_auth(&name).await?;
        if started.authorization_url.is_empty() {
            service.close_all().await;
            println!("MCP server `{name}` authenticated without browser interaction.");
            return Ok(0);
        }
        let auth = oc_mcp::auth::McpAuth::default();
        let _ = auth.clear_code_verifier(&name).await;
        let _ = auth.clear_oauth_state(&name).await;
        service.close_all().await;
        return Err(anyhow::anyhow!(
            "MCP server `{name}` requires external browser authorization. Open this URL in a browser, then rerun `opencode mcp auth {name}` from an interactive terminal:\n{}",
            started.authorization_url
        ));
    }

    let callback = Arc::new(|url: &str| {
        println!("Authorize MCP access in your browser:\n{url}");
    });
    let result = service.authenticate(&name, Some(callback)).await;
    service.close_all().await;
    let status = result?;
    match status {
        oc_mcp::index::Status::Connected => {
            println!("MCP server `{name}` authenticated successfully.");
            Ok(0)
        }
        other => Err(anyhow::anyhow!(
            "MCP authentication for `{name}` did not connect: {}",
            status_text(Some(&other))
        )),
    }
}

async fn run_logout(ctx: &Context, args: &McpLogoutArgs) -> anyhow::Result<i32> {
    let auth = oc_mcp::auth::McpAuth::default();
    let entries = auth.all().await?;
    if entries.is_empty() {
        println!("No MCP OAuth credentials stored.");
        return Ok(0);
    }
    let name = match &args.name {
        Some(name) => name.clone(),
        None => select_name(
            "Select MCP server to logout",
            entries.keys().cloned().collect(),
        )?,
    };
    if !entries.contains_key(&name) {
        return Err(anyhow::anyhow!("No OAuth credentials found for `{name}`"));
    }
    let service = mcp_service(ctx, &Value::Object(Map::new()))?;
    service.remove_auth(&name).await?;
    service.close_all().await;
    println!("Removed OAuth credentials for MCP server `{name}`");
    Ok(0)
}

async fn run_debug(ctx: &Context, args: &McpDebugArgs) -> anyhow::Result<i32> {
    let config = read_merged_config(ctx)?;
    let server = configured_server(&config, &args.name)?;
    let oc_mcp::config::Info::Remote(remote) = &server else {
        return Err(anyhow::anyhow!(
            "MCP server `{}` is not a remote server",
            args.name
        ));
    };
    let service = mcp_service(ctx, &config)?;
    if !service.supports_oauth(&args.name).await? {
        service.close_all().await;
        return Err(anyhow::anyhow!(
            "MCP server `{}` has OAuth explicitly disabled",
            args.name
        ));
    }
    let auth = oc_mcp::auth::McpAuth::default();
    let entry = auth.get_for_url(&args.name, &remote.url).await?;
    let auth_status = service.get_auth_status(&args.name).await?;
    let mut result = serde_json::json!({
        "name": args.name,
        "config": debug_config_value(&server)?,
        "authFile": auth.path(),
        "oauthSupported": true,
        "hasCredentials": entry.is_some(),
        "authStatus": auth_status_label(auth_status),
    });
    if let Some(entry) = entry {
        if let Some(object) = result.as_object_mut() {
            if let Some(server_url) = entry.server_url {
                object.insert("serverURL".into(), server_url.into());
            }
            object.insert("hasTokens".into(), entry.tokens.is_some().into());
            object.insert("hasClientInfo".into(), entry.client_info.is_some().into());
            object.insert(
                "hasCodeVerifier".into(),
                entry.code_verifier.is_some().into(),
            );
            if let Some(tokens) = entry.tokens {
                object.insert(
                    "tokenPreview".into(),
                    mask_token(&tokens.access_token).into(),
                );
                object.insert(
                    "hasRefreshToken".into(),
                    tokens.refresh_token.is_some().into(),
                );
                if let Some(expires_at) = tokens.expires_at {
                    object.insert("expiresAt".into(), expires_at.into());
                }
            }
        }
    }
    service.close_all().await;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(0)
}

async fn run_auth_list(ctx: &Context) -> anyhow::Result<i32> {
    let config = read_merged_config(ctx)?;
    let Some(servers) = config.get("mcp").and_then(Value::as_object) else {
        println!("No MCP servers configured.");
        return Ok(0);
    };
    let service = mcp_service(ctx, &config)?;
    println!("{:<24} {:<12} STATUS", "NAME", "TYPE");
    for (name, server) in servers {
        let Ok(info) = serde_json::from_value::<oc_mcp::config::Info>(server.clone()) else {
            continue;
        };
        let oc_mcp::config::Info::Remote(remote) = info else {
            continue;
        };
        if !remote.oauth_enabled() {
            continue;
        }
        let status = service.get_auth_status(name).await?;
        println!("{name:<24} {:<12} {}", "remote", auth_status_label(status));
    }
    service.close_all().await;
    Ok(0)
}

fn select_oauth_server(config: &Value) -> anyhow::Result<String> {
    let names = config
        .get("mcp")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|servers| servers.iter())
        .filter_map(|(name, value)| {
            let info = serde_json::from_value::<oc_mcp::config::Info>(value.clone()).ok()?;
            match info {
                oc_mcp::config::Info::Remote(remote) if remote.oauth_enabled() => {
                    Some(name.clone())
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(anyhow::anyhow!("No OAuth-capable MCP servers configured"));
    }
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return Err(anyhow::anyhow!(
            "provide an MCP server name; selecting one requires an interactive terminal (use `opencode mcp auth list` to see options)"
        ));
    }
    select_name("Select MCP server to authenticate", names)
}

fn select_name(prompt: &str, mut names: Vec<String>) -> anyhow::Result<String> {
    names.sort();
    println!("{prompt}");
    for (index, name) in names.iter().enumerate() {
        println!("  {}) {}", index + 1, name);
    }
    let selected = prompt_required("Selection: ")?;
    let index = selected
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("invalid selection: {selected}"))?;
    names
        .get(
            index
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("invalid selection"))?,
        )
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("selection is out of range"))
}

fn status_text(status: Option<&oc_mcp::index::Status>) -> String {
    match status {
        None => "not initialized".into(),
        Some(oc_mcp::index::Status::Connected) => "connected".into(),
        Some(oc_mcp::index::Status::Disabled) => "disabled".into(),
        Some(oc_mcp::index::Status::NeedsAuth) => "needs authentication".into(),
        Some(oc_mcp::index::Status::NeedsClientRegistration { error }) => {
            format!("needs client registration: {error}")
        }
        Some(oc_mcp::index::Status::Failed { error }) => format!("failed: {error}"),
    }
}

fn auth_status_label(status: oc_mcp::index::AuthStatus) -> &'static str {
    match status {
        oc_mcp::index::AuthStatus::Authenticated => "authenticated",
        oc_mcp::index::AuthStatus::Expired => "expired",
        oc_mcp::index::AuthStatus::NotAuthenticated => "not authenticated",
    }
}

fn mask_token(token: &str) -> String {
    if token.chars().count() <= 8 {
        return "***".into();
    }
    let prefix = token.chars().take(4).collect::<String>();
    let suffix = token
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}***{suffix}")
}

fn debug_config_value(server: &oc_mcp::config::Info) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(server)?;
    redact_debug_secrets(&mut value, None);
    Ok(value)
}

fn redact_debug_secrets(value: &mut Value, key: Option<&str>) {
    if key.is_some_and(is_sensitive_debug_key) {
        *value = Value::String("***".into());
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                redact_debug_secrets(value, Some(key));
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_debug_secrets(value, None);
            }
        }
        _ => {}
    }
}

fn is_sensitive_debug_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "apikey",
        "api-key",
        "clientsecret",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_server_value_preserves_equals_in_header_values() {
        let args = McpAddArgs {
            name: Some("demo".into()),
            url: Some("https://example.com/mcp".into()),
            header: vec!["Authorization=Bearer=a=b".into()],
            ..Default::default()
        };
        let value = server_value(&args).unwrap();
        assert_eq!(
            value["headers"]["Authorization"],
            Value::String("Bearer=a=b".into())
        );
    }

    #[test]
    fn server_value_rejects_wrong_transport_flags() {
        let args = McpAddArgs {
            name: Some("demo".into()),
            command: vec!["server".into()],
            header: vec!["X-Test=value".into()],
            ..Default::default()
        };
        let error = server_value(&args).unwrap_err().to_string();
        assert!(error.contains("--header is only valid for remote MCP servers"));
    }

    #[test]
    fn token_debug_output_is_redacted() {
        assert_eq!(mask_token("short"), "***");
        assert_eq!(mask_token("abcdefghijkl"), "abcd***ijkl");
    }

    #[test]
    fn debug_config_redacts_credentials_but_keeps_connection_details() {
        let server = serde_json::from_value::<oc_mcp::config::Info>(serde_json::json!({
            "type": "remote",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer secret" },
            "oauth": { "clientId": "client", "clientSecret": "secret" }
        }))
        .unwrap();
        let value = debug_config_value(&server).unwrap();

        assert_eq!(value["url"], "https://example.com/mcp");
        assert_eq!(value["headers"]["Authorization"], "***");
        assert_eq!(value["oauth"]["clientSecret"], "***");
        assert_eq!(value["oauth"]["clientId"], "client");
    }

    #[test]
    fn status_text_is_explicit_for_auth_and_registration_failures() {
        assert_eq!(
            status_text(Some(&oc_mcp::index::Status::NeedsAuth)),
            "needs authentication"
        );
        assert_eq!(
            status_text(Some(&oc_mcp::index::Status::NeedsClientRegistration {
                error: "missing client".into(),
            })),
            "needs client registration: missing client"
        );
    }

    #[test]
    fn jsonc_config_write_preserves_unrelated_comments() {
        let path = std::env::temp_dir().join(format!(
            "opencode-mcp-jsonc-{}-{}.jsonc",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "{\n  // keep this comment\n  \"theme\": \"dark\",\n}\n",
        )
        .unwrap();
        write_config(
            &path,
            &serde_json::json!({
                "theme": "dark",
                "mcp": {"demo": {"type": "remote", "url": "https://example.test/mcp"}}
            }),
        )
        .unwrap();
        let output = std::fs::read_to_string(&path).unwrap();
        assert!(output.contains("// keep this comment"));
        assert!(output.contains("\"demo\""));
        assert_eq!(
            oc_config::parse::jsonc(&output, "test").unwrap()["theme"],
            "dark"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn jsonc_mcp_write_preserves_comments_inside_mcp_object() {
        let path = std::env::temp_dir().join(format!(
            "opencode-mcp-jsonc-nested-{}-{}.jsonc",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "{\n  \"mcp\": {\n    // keep this server comment\n    \"old\": {\"type\": \"local\"},\n  },\n}\n",
        )
        .unwrap();
        write_mcp_config(
            &path,
            &serde_json::json!({
                "mcp": {
                    "old": {"type": "local"},
                    "demo": {"type": "remote", "url": "https://example.test/mcp"}
                }
            }),
            "demo",
        )
        .unwrap();
        let output = std::fs::read_to_string(&path).unwrap();
        assert!(output.contains("// keep this server comment"));
        assert!(output.contains("\"demo\""));
        assert_eq!(
            oc_config::parse::jsonc(&output, "test").unwrap()["mcp"]["old"]["type"],
            "local"
        );
        std::fs::remove_file(path).unwrap();
    }
}
