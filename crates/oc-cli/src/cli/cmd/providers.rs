//! `opencode providers` (alias `auth`)
//! From reference/packages/opencode/src/cli/cmd/providers.ts.

use std::io::Write;
use std::{collections::BTreeMap, io::IsTerminal};

use serde_json::Value;

use crate::cli::args::{
    Cli, ProvidersArgs, ProvidersCommand, ProvidersLoginArgs, ProvidersLogoutArgs,
};
use crate::cli::auth::{Auth, AuthInfo};
use crate::cli::context::Context;
use crate::cli::effect_cmd::CliError;
use crate::cli::models_dev::ModelsDev;
use crate::cli::ui::{self, Style};
use oc_provider::auth::{
    login::{self as provider_login, CatalogProvider, LoginOptions, LoginPrompt},
    AuthStore, FileAuthStore,
};

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
    for (_provider_id, provider) in &db.providers {
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

    if let Some(url) = &args.url {
        clack_intro("Add credential");
        let url = url.trim_end_matches('/');
        return login_wellknown(ctx, url).await;
    }

    let db = ModelsDev::load(&ctx.paths).unwrap_or_default();
    if !std::io::stdin().is_terminal() {
        return Err(anyhow::Error::new(CliError::new(
            "provider login requires an interactive terminal",
        )));
    }

    // Use oc-provider's shared login flow so the CLI and server agree on
    // provider filtering, credential shape, and API-key prompt behavior.
    // Native internal hooks are available to the CLI as well as the server;
    // `OPENCODE_DISABLE_DEFAULT_PLUGINS` disables them, while `--pure` does
    // not because the reference keeps internal defaults active in pure mode.
    let mut auth = FileAuthStore::new(&ctx.paths.data);
    run_catalog_login(&mut auth, &TerminalLoginPrompt, args, &db)
        .map_err(|error| anyhow::Error::new(CliError::new(error.to_string())))?;
    Ok(0)
}

fn run_catalog_login(
    auth: &mut impl AuthStore,
    prompt: &dyn LoginPrompt,
    args: &ProvidersLoginArgs,
    db: &ModelsDev,
) -> Result<(), provider_login::LoginError> {
    let catalog = db
        .providers
        .iter()
        .map(|(id, provider)| {
            (
                id.clone(),
                CatalogProvider {
                    name: provider.name.clone(),
                    env: provider.env.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let hooks = if matches!(
        std::env::var("OPENCODE_DISABLE_DEFAULT_PLUGINS")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("True")
    ) {
        BTreeMap::new()
    } else {
        oc_server::builtin_auth::default_auth_hooks()
    };

    provider_login::login(
        auth,
        prompt,
        &LoginOptions {
            provider: args.provider.clone(),
            method: args.method.clone(),
        },
        &catalog,
        &hooks,
        &Default::default(),
        &None,
    )
}

struct TerminalLoginPrompt;

impl TerminalLoginPrompt {
    fn choose(&self, message: &str, options: &[(String, String)]) -> Option<usize> {
        let mut stdout = std::io::stdout();
        for (index, (_, label)) in options.iter().enumerate() {
            if writeln!(stdout, "  {index}) {label}").is_err() {
                return None;
            }
        }
        if write!(stdout, "{message}: ").is_err() || stdout.flush().is_err() {
            return None;
        }
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok()?;
        let index = input.trim().parse::<usize>().ok()?;
        (index < options.len()).then_some(index)
    }
}

impl LoginPrompt for TerminalLoginPrompt {
    fn intro(&self, title: &str) {
        clack_intro(title);
    }

    fn log_info(&self, message: &str) {
        clack_log(message);
    }

    fn log_warn(&self, message: &str) {
        clack_log(message);
    }

    fn log_error(&self, message: &str) {
        clack_log(message);
    }

    fn log_success(&self, message: &str) {
        clack_log(message);
    }

    fn outro(&self, message: &str) {
        clack_outro(message);
    }

    fn text(
        &self,
        message: &str,
        placeholder: Option<&str>,
        validate: Option<&dyn Fn(&str) -> Option<String>>,
    ) -> Option<String> {
        let mut stdout = std::io::stdout();
        if write!(stdout, "{message}").is_err() {
            return None;
        }
        if let Some(placeholder) = placeholder {
            if write!(stdout, " ({placeholder})").is_err() {
                return None;
            }
        }
        if write!(stdout, ": ").is_err() || stdout.flush().is_err() {
            return None;
        }
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok()?;
        let input = input.trim().to_string();
        if validate.and_then(|validate| validate(&input)).is_some() {
            return None;
        }
        Some(input)
    }

    fn password(&self, message: &str) -> Option<String> {
        read_secret(&format!("{message}: ")).ok()
    }

    fn select(&self, message: &str, options: &[(String, String)]) -> Option<usize> {
        self.choose(message, options)
    }

    fn autocomplete(&self, message: &str, options: &[(String, String)]) -> Option<String> {
        self.choose(message, options)
            .and_then(|index| options.get(index).map(|(value, _)| value.clone()))
    }

    fn spinner_start(&self, message: &str) {
        clack_log(message);
    }

    fn spinner_stop(&self, message: &str, _failed: bool) {
        clack_log(message);
    }
}

fn read_secret(prompt: &str) -> anyhow::Result<String> {
    let mut stdout = std::io::stdout();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut input = String::new();
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let stdin = std::io::stdin();
        let fd = stdin.as_raw_fd();
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        let mut saved = None;
        let configured = unsafe {
            if libc::tcgetattr(fd, original.as_mut_ptr()) == 0 {
                let original = original.assume_init();
                let mut hidden = original;
                hidden.c_lflag &= !libc::ECHO;
                if libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) == 0 {
                    saved = Some(original);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if configured {
            let result = std::io::stdin().read_line(&mut input);
            if let Some(original) = saved {
                let _ = unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &original) };
            }
            writeln!(stdout)?;
            result?;
            return Ok(input.trim().to_string());
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use oc_provider::auth::{Api, AuthError, Info};

    #[derive(Default)]
    struct MemoryAuth(BTreeMap<String, Info>);

    impl AuthStore for MemoryAuth {
        fn get(&self, provider_id: &str) -> Result<Option<Info>, AuthError> {
            Ok(self.0.get(provider_id).cloned())
        }

        fn all(&self) -> Result<BTreeMap<String, Info>, AuthError> {
            Ok(self.0.clone())
        }

        fn set(&mut self, key: &str, info: Info) -> Result<(), AuthError> {
            self.0.insert(key.trim_end_matches('/').to_string(), info);
            Ok(())
        }

        fn remove(&mut self, key: &str) -> Result<(), AuthError> {
            self.0.remove(key.trim_end_matches('/'));
            Ok(())
        }
    }

    struct ScriptedPrompt;

    impl LoginPrompt for ScriptedPrompt {
        fn intro(&self, _: &str) {}
        fn log_info(&self, _: &str) {}
        fn log_warn(&self, _: &str) {}
        fn log_error(&self, _: &str) {}
        fn log_success(&self, _: &str) {}
        fn outro(&self, _: &str) {}

        fn text(
            &self,
            _: &str,
            _: Option<&str>,
            _: Option<&dyn Fn(&str) -> Option<String>>,
        ) -> Option<String> {
            None
        }

        fn password(&self, _: &str) -> Option<String> {
            Some("test-key".to_string())
        }

        fn select(&self, _: &str, _: &[(String, String)]) -> Option<usize> {
            None
        }

        fn autocomplete(&self, _: &str, _: &[(String, String)]) -> Option<String> {
            None
        }

        fn spinner_start(&self, _: &str) {}
        fn spinner_stop(&self, _: &str, _: bool) {}
    }

    fn catalog() -> ModelsDev {
        ModelsDev {
            providers: BTreeMap::from([(
                "acme".to_string(),
                crate::cli::models_dev::Provider {
                    id: "acme".to_string(),
                    name: "Acme AI".to_string(),
                    env: vec!["ACME_API_KEY".to_string()],
                    models: BTreeMap::new(),
                },
            )]),
        }
    }

    fn catalog_with_native_provider() -> ModelsDev {
        ModelsDev {
            providers: BTreeMap::from([(
                "openai".to_string(),
                crate::cli::models_dev::Provider {
                    id: "openai".to_string(),
                    name: "OpenAI".to_string(),
                    env: vec!["OPENAI_API_KEY".to_string()],
                    models: BTreeMap::new(),
                },
            )]),
        }
    }

    #[test]
    fn catalog_login_uses_shared_provider_flow_and_persists_api_credentials() {
        let mut auth = MemoryAuth::default();
        let args = ProvidersLoginArgs {
            url: None,
            provider: Some("Acme AI".to_string()),
            method: None,
        };

        run_catalog_login(&mut auth, &ScriptedPrompt, &args, &catalog()).unwrap();

        assert_eq!(
            auth.0.get("acme"),
            Some(&Info::Api(Api {
                key: "test-key".to_string(),
                metadata: None,
            }))
        );
    }

    #[test]
    fn catalog_login_reports_unknown_provider_before_prompting() {
        let mut auth = MemoryAuth::default();
        let args = ProvidersLoginArgs {
            url: None,
            provider: Some("missing".to_string()),
            method: None,
        };

        let error = run_catalog_login(&mut auth, &ScriptedPrompt, &args, &catalog()).unwrap_err();

        assert!(error.to_string().contains("Unknown provider \"missing\""));
        assert!(auth.0.is_empty());
    }

    #[test]
    fn catalog_login_uses_native_internal_api_hook() {
        let mut auth = MemoryAuth::default();
        let args = ProvidersLoginArgs {
            url: None,
            provider: Some("openai".to_string()),
            method: Some("Manually enter API Key".to_string()),
        };

        run_catalog_login(
            &mut auth,
            &ScriptedPrompt,
            &args,
            &catalog_with_native_provider(),
        )
        .unwrap();

        assert_eq!(
            auth.0.get("openai"),
            Some(&Info::Api(Api {
                key: "test-key".to_string(),
                metadata: None,
            }))
        );
    }
}
