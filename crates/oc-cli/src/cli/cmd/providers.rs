//! `opencode providers` (alias `auth`)
//! From reference/packages/opencode/src/cli/cmd/providers.ts.

use std::io::Write;

use crate::cli::args::{
    Cli, ProvidersArgs, ProvidersCommand, ProvidersLoginArgs, ProvidersLogoutArgs,
};
use crate::cli::auth::{Auth, AuthInfo};
use crate::cli::context::Context;
use crate::cli::effect_cmd::CliError;
use crate::cli::models_dev::ModelsDev;
use crate::cli::ui::{self, Style};

pub async fn run(_cli: &Cli, args: &ProvidersArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    match &args.command {
        ProvidersCommand::List => list(&ctx).await,
        ProvidersCommand::Login(login) => run_login(&ctx, login).await,
        ProvidersCommand::Logout(logout) => run_logout(&ctx, logout).await,
    }
}

fn clack_intro(message: &str) {
    ui::println(&["◇  ", message]);
}

fn clack_outro(message: &str) {
    ui::println(&["└  ", message]);
}

fn clack_log(message: &str) {
    ui::println(&["│  ", message]);
}

async fn list(ctx: &Context) -> anyhow::Result<i32> {
    ui::empty();
    let auth = Auth::new(&ctx.paths);
    let auth_path = auth.file.display().to_string();
    let home = ctx.paths.home();
    let display_path = if auth_path.starts_with(home.to_str().unwrap_or("")) {
        auth_path.replacen(home.to_str().unwrap_or(""), "~", 1)
    } else {
        auth_path.clone()
    };
    clack_intro(&format!(
        "Credentials {}{}{}",
        Style::TEXT_DIM,
        display_path,
        Style::TEXT_NORMAL
    ));
    let credentials = auth.all();
    let db = ModelsDev::load(&ctx.paths).unwrap_or_default();

    for (provider_id, info) in &credentials {
        let name = db
            .providers
            .get(provider_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| provider_id.clone());
        clack_log(&format!(
            "{name} {}{}{}",
            Style::TEXT_DIM,
            info.type_label(),
            Style::TEXT_NORMAL
        ));
    }
    clack_outro(&format!(
        "{} credential{}",
        credentials.len(),
        if credentials.len() == 1 { "" } else { "s" }
    ));

    let mut active_env_vars: Vec<(String, String)> = Vec::new();
    for provider in db.providers.values() {
        for env_var in &provider.env {
            if std::env::var_os(env_var).is_some() {
                active_env_vars.push((provider.name.clone(), env_var.clone()));
            }
        }
    }

    if !active_env_vars.is_empty() {
        ui::empty();
        clack_intro("Environment");
        for (provider, env_var) in &active_env_vars {
            clack_log(&format!(
                "{provider} {}{}{}",
                Style::TEXT_DIM,
                env_var,
                Style::TEXT_NORMAL
            ));
        }
        clack_outro(&format!(
            "{} environment variable{}",
            active_env_vars.len(),
            if active_env_vars.len() == 1 { "" } else { "s" }
        ));
    }
    Ok(0)
}

async fn run_logout(ctx: &Context, args: &ProvidersLogoutArgs) -> anyhow::Result<i32> {
    ui::empty();
    let auth = Auth::new(&ctx.paths);
    let credentials = auth.all();
    clack_intro("Remove credential");
    if credentials.is_empty() {
        ui::println(&[
            Style::TEXT_DANGER_BOLD,
            "✖  ",
            Style::TEXT_NORMAL,
            "No credentials found",
        ]);
        return Ok(0);
    }
    let db = ModelsDev::load(&ctx.paths).unwrap_or_default();
    let provider = if let Some(provider_arg) = &args.provider {
        credentials
            .keys()
            .find(|key| {
                *key == provider_arg
                    || db
                        .providers
                        .get(*key)
                        .map(|p| p.name.to_lowercase() == provider_arg.to_lowercase())
                        .unwrap_or(false)
            })
            .cloned()
            .ok_or_else(|| {
                CliError::new(format!("Unknown configured provider \"{provider_arg}\""))
            })?
    } else {
        // Non-interactive: refuse rather than block on a prompt.
        if !std::io::stdin().is_terminal() {
            return Err(anyhow::Error::new(CliError::new(
                "selecting a provider requires an interactive terminal; pass the provider id",
            )));
        }
        select_provider(&credentials, &db)?
    };
    auth.remove(&provider)?;
    clack_outro("Logout successful");
    Ok(0)
}

fn select_provider(
    credentials: &std::collections::BTreeMap<String, AuthInfo>,
    db: &ModelsDev,
) -> anyhow::Result<String> {
    let mut stdout = std::io::stdout();
    for (i, (key, info)) in credentials.iter().enumerate() {
        let name = db
            .providers
            .get(key)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| key.clone());
        writeln!(stdout, "  {i}) {name} ({})", info.type_label())?;
    }
    write!(stdout, "Select provider: ")?;
    stdout.flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let index: usize = input
        .trim()
        .parse()
        .map_err(|_| CliError::new("invalid selection"))?;
    credentials
        .keys()
        .nth(index)
        .cloned()
        .ok_or_else(|| anyhow::Error::new(CliError::new("invalid selection")))
}

async fn run_login(ctx: &Context, args: &ProvidersLoginArgs) -> anyhow::Result<i32> {
    ui::empty();
    clack_intro("Add credential");

    if let Some(url) = &args.url {
        let url = url.trim_end_matches('/');
        return login_wellknown(ctx, url).await;
    }

    let auth = Auth::new(&ctx.paths);
    let db = ModelsDev::load(&ctx.paths).unwrap_or_default();

    let provider = if let Some(provider_arg) = &args.provider {
        let lower = provider_arg.to_lowercase();
        db.providers
            .iter()
            .find(|(id, p)| *id == provider_arg || p.name.to_lowercase() == lower)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| CliError::new(format!("Unknown provider \"{provider_arg}\"")))?
    } else {
        if !std::io::stdin().is_terminal() {
            return Err(anyhow::Error::new(CliError::new(
                "selecting a provider requires an interactive terminal; pass --provider",
            )));
        }
        select_login_provider(&db)?
    };

    if !std::io::stdin().is_terminal() {
        return Err(anyhow::Error::new(CliError::new(
            "entering an API key requires an interactive terminal",
        )));
    }

    // TODO(integration): support plugin auth methods (oauth) once oc-plugin lands.
    if let Some(hint) = login_hint(&provider) {
        clack_log(hint);
    }
    let key = read_secret("Enter your API key: ")?;
    if key.is_empty() {
        return Err(anyhow::Error::new(CliError::new("API key is required")));
    }
    auth.set(
        &provider,
        AuthInfo::Api {
            key,
            metadata: None,
        },
    )?;
    clack_outro("Done");
    Ok(0)
}

fn select_login_provider(db: &ModelsDev) -> anyhow::Result<String> {
    let mut stdout = std::io::stdout();
    let ids: Vec<&String> = db.providers.keys().collect();
    for (i, id) in ids.iter().enumerate() {
        let name = db.providers[*id].name.clone();
        writeln!(stdout, "  {i}) {name}")?;
    }
    writeln!(stdout, "  {}) other", ids.len())?;
    write!(stdout, "Select provider: ")?;
    stdout.flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let index: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::Error::new(CliError::new("invalid selection")))?;
    if index < ids.len() {
        Ok(ids[index].clone())
    } else {
        write!(stdout, "Enter provider id: ")?;
        stdout.flush()?;
        let mut name = String::new();
        std::io::stdin().read_line(&mut name)?;
        Ok(name.trim().to_string())
    }
}

fn login_hint(provider: &str) -> Option<&'static str> {
    match provider {
        "opencode" => Some("Create an api key at https://opencode.ai/auth"),
        "vercel" => Some("You can create an api key at https://vercel.link/ai-gateway-token"),
        _ => None,
    }
}

fn read_secret(prompt: &str) -> anyhow::Result<String> {
    let mut stdout = std::io::stdout();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    // TODO(integration): echo-hiding (e.g. termios) for password input.
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Device-code style login against a URL, mirroring the reference's well-known
/// flow: fetch `/.well-known/opencode`, run `auth.command`, store the token.
async fn login_wellknown(ctx: &Context, url: &str) -> anyhow::Result<i32> {
    let auth = Auth::new(&ctx.paths);
    let wellknown: Value = reqwest::Client::new()
        .get(format!("{url}/.well-known/opencode"))
        .send()
        .await
        .map_err(|err| {
            CliError::new(format!(
                "Failed to load auth provider metadata from {url}: {err}"
            ))
        })?
        .json()
        .await
        .map_err(|err| {
            CliError::new(format!(
                "Failed to load auth provider metadata from {url}: {err}"
            ))
        })?;

    let command: Vec<String> = wellknown
        .pointer("/auth/command")
        .and_then(|c| serde_json::from_value(c.clone()).ok())
        .ok_or_else(|| {
            CliError::new(format!("Failed to load auth provider metadata from {url}"))
        })?;
    let env_key = wellknown
        .pointer("/auth/env")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CliError::new(format!("Failed to load auth provider metadata from {url}"))
        })?;

    clack_log(&format!("Running `{}`", command.join(" ")));
    let output = tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .output()
        .await
        .map_err(|err| CliError::new(format!("Failed to run auth provider command: {err}")))?;
    if !output.status.success() {
        clack_log("Failed");
        clack_outro("Done");
        return Ok(0);
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    auth.set(
        url,
        AuthInfo::WellKnown {
            key: env_key.to_string(),
            token,
        },
    )?;
    clack_log(&format!("Logged into {url}"));
    clack_outro("Done");
    Ok(0)
}

use serde_json::Value;
use std::io::IsTerminal;
