//! Console account service used by the hidden `opencode console` command.
//!
//! Mirrors the reference device-code account flow: authenticate through
//! `/auth/device/code` and `/auth/device/token`, then persist the account and
//! active organization in the existing SQLite `account`/`account_state`
//! tables.

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context as _};
use serde::Deserialize;

use crate::cli::args::{Cli, ConsoleArgs, ConsoleCommand};
use crate::cli::ui;

pub const DEFAULT_CONSOLE_URL: &str = "https://console.opencode.ai";
const CLIENT_ID: &str = "opencode-cli";

#[derive(Debug, Deserialize)]
struct DeviceAuth {
    device_code: String,
    user_code: String,
    verification_uri_complete: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenSuccess {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    token_type: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenError {
    error: String,
    #[serde(default)]
    error_description: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DeviceToken {
    Success(DeviceTokenSuccess),
    Error(DeviceTokenError),
}

#[derive(Debug, Deserialize)]
struct User {
    id: String,
    email: String,
}

#[derive(Debug, Deserialize)]
struct Org {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct LoginConfig {
    code: String,
    server: String,
    expires_in: u64,
    interval: u64,
}

pub async fn run(_cli: &Cli, args: &ConsoleArgs) -> anyhow::Result<i32> {
    let Some(command) = &args.command else {
        ui::error("a console subcommand is required");
        return Ok(1);
    };
    match command {
        ConsoleCommand::Login { url } => login(url.as_deref()).await,
        ConsoleCommand::Logout { email } => logout(email.as_deref()),
        ConsoleCommand::Switch => switch_org().await,
        ConsoleCommand::Orgs => orgs().await,
        ConsoleCommand::Open => open().await,
    }
}

async fn login(url: Option<&str>) -> anyhow::Result<i32> {
    let server = normalize_url(url.unwrap_or(DEFAULT_CONSOLE_URL));
    let client = reqwest::Client::new();
    let device: DeviceAuth = client
        .post(format!("{server}/auth/device/code"))
        .json(&serde_json::json!({ "client_id": CLIENT_ID }))
        .send()
        .await
        .with_context(|| format!("failed to reach console server {server}"))?
        .error_for_status()
        .context("console device-code request failed")?
        .json()
        .await
        .context("console returned an invalid device-code response")?;

    ui::println(&["Log in to console"]);
    ui::println(&[&format!("Go to: {}", device.verification_uri_complete)]);
    ui::println(&[&format!("Enter code: {}", device.user_code)]);
    open_browser(&device.verification_uri_complete).await;

    let login = LoginConfig {
        code: device.device_code,
        server,
        expires_in: device.expires_in,
        interval: device.interval,
    };
    let started = SystemTime::now();
    let mut interval = login.interval.max(1);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if started.elapsed().unwrap_or_default().as_secs() >= login.expires_in {
            bail!("console device code expired")
        }

        let response = client
            .post(format!("{}/auth/device/token", login.server))
            .json(&serde_json::json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                "device_code": login.code,
                "client_id": CLIENT_ID,
            }))
            .send()
            .await
            .context("failed to poll console authorization")?;
        let payload: DeviceToken = response
            .json()
            .await
            .context("console returned an invalid token response")?;
        match payload {
            DeviceToken::Success(token) => {
                if token.token_type.to_lowercase() != "bearer" {
                    bail!(
                        "console returned unsupported token type {:?}",
                        token.token_type
                    )
                }
                let (user, remote_orgs) = tokio::try_join!(
                    fetch_user(&client, &login.server, &token.access_token),
                    fetch_orgs(&client, &login.server, &token.access_token),
                )?;
                let now = now_ms();
                let database = open_database()?;
                let refresh_token = token
                    .refresh_token
                    .ok_or_else(|| anyhow!("console did not return a refresh token"))?;
                let account = oc_database::tables::AccountRow {
                    id: user.id.clone(),
                    email: user.email.clone(),
                    url: login.server.clone(),
                    access_token: token.access_token,
                    refresh_token,
                    token_expiry: Some(now + token.expires_in as i64 * 1000),
                    time_created: now,
                    time_updated: now,
                };
                database.upsert(
                    "account",
                    &account,
                    oc_database::tables::json_columns("account"),
                    "id",
                    &oc_database::Value::Text(account.id.clone()),
                )?;
                persist_active(
                    &database,
                    Some(account.id),
                    remote_orgs.first().map(|org| org.id.clone()),
                )?;
                ui::println(&[&format!("Logged in as {}", user.email)]);
                ui::println(&["Done"]);
                return Ok(0);
            }
            DeviceToken::Error(error) if error.error == "authorization_pending" => {}
            DeviceToken::Error(error) if error.error == "slow_down" => interval += 5,
            DeviceToken::Error(error) if error.error == "expired_token" => {
                bail!("console device code expired")
            }
            DeviceToken::Error(error) if error.error == "access_denied" => {
                bail!("console authorization denied")
            }
            DeviceToken::Error(error) => {
                let detail = if error.error_description.is_empty() {
                    error.error
                } else {
                    format!("{}: {}", error.error, error.error_description)
                };
                bail!("console authorization failed: {detail}")
            }
        }
    }
}

async fn fetch_user(
    client: &reqwest::Client,
    server: &str,
    access_token: &str,
) -> anyhow::Result<User> {
    Ok(client
        .get(format!("{server}/api/user"))
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn fetch_orgs(
    client: &reqwest::Client,
    server: &str,
    access_token: &str,
) -> anyhow::Result<Vec<Org>> {
    Ok(client
        .get(format!("{server}/api/orgs"))
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn refresh_account_if_needed(
    database: &oc_database::Database,
    account: oc_database::tables::AccountRow,
) -> anyhow::Result<oc_database::tables::AccountRow> {
    const REFRESH_SKEW_MS: i64 = 30_000;
    if account
        .token_expiry
        .map(|expiry| expiry > now_ms() + REFRESH_SKEW_MS)
        .unwrap_or(true)
    {
        return Ok(account);
    }

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/auth/device/token", account.url))
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": account.refresh_token,
            "client_id": CLIENT_ID,
        }))
        .send()
        .await
        .context("failed to refresh console authorization")?;
    let payload: DeviceToken = response
        .json()
        .await
        .context("console returned an invalid refresh response")?;
    let DeviceToken::Success(token) = payload else {
        bail!("console refresh authorization failed")
    };
    if token.token_type.to_lowercase() != "bearer" {
        bail!(
            "console returned unsupported token type {:?}",
            token.token_type
        )
    }

    let now = now_ms();
    let refreshed = oc_database::tables::AccountRow {
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .unwrap_or_else(|| account.refresh_token.clone()),
        token_expiry: Some(now + token.expires_in as i64 * 1000),
        time_updated: now,
        ..account
    };
    database.upsert(
        "account",
        &refreshed,
        oc_database::tables::json_columns("account"),
        "id",
        &oc_database::Value::Text(refreshed.id.clone()),
    )?;
    Ok(refreshed)
}

fn logout(email: Option<&str>) -> anyhow::Result<i32> {
    let database = open_database()?;
    let accounts = list_accounts(&database)?;
    if accounts.is_empty() {
        ui::println(&["Not logged in"]);
        return Ok(0);
    }
    let selected = match email {
        Some(email) => accounts
            .iter()
            .find(|account| account.email == email)
            .ok_or_else(|| anyhow!("Account not found: {email}"))?,
        None if !io::stdin().is_terminal() => {
            bail!("select an account by email when console logout is non-interactive")
        }
        None => {
            for (index, account) in accounts.iter().enumerate() {
                println!("  {index}) {} ({})", account.email, account.url);
            }
            print!("Select account: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let index = input.trim().parse::<usize>()?;
            accounts
                .get(index)
                .ok_or_else(|| anyhow!("invalid account selection"))?
        }
    };
    let active = active_state(&database)?;
    database.delete_by(
        "account",
        "id",
        &oc_database::Value::Text(selected.id.clone()),
    )?;
    if active
        .as_ref()
        .and_then(|state| state.active_account_id.as_deref())
        == Some(selected.id.as_str())
    {
        let next = accounts.iter().find(|account| account.id != selected.id);
        persist_active(&database, next.map(|account| account.id.clone()), None)?;
    }
    ui::println(&[&format!("Logged out from {}", selected.email)]);
    Ok(0)
}

async fn orgs() -> anyhow::Result<i32> {
    let database = open_database()?;
    let accounts = list_accounts(&database)?;
    if accounts.is_empty() {
        ui::println(&["No accounts found"]);
        return Ok(0);
    }
    let active = active_state(&database)?;
    let client = reqwest::Client::new();
    let mut found = false;
    for account in accounts {
        let account = refresh_account_if_needed(&database, account).await?;
        let remote_orgs = fetch_orgs(&client, &account.url, &account.access_token)
            .await
            .with_context(|| format!("failed to list orgs for {}", account.email))?;
        for org in remote_orgs {
            found = true;
            let marker = if active
                .as_ref()
                .and_then(|state| state.active_org_id.as_deref())
                == Some(org.id.as_str())
            {
                "*"
            } else {
                " "
            };
            ui::println(&[&format!(
                " {marker} {} ({}) {}",
                org.name, account.email, org.id
            )]);
        }
    }
    if !found {
        ui::println(&["No orgs found"]);
    }
    Ok(0)
}

async fn switch_org() -> anyhow::Result<i32> {
    let database = open_database()?;
    let accounts = list_accounts(&database)?;
    if accounts.is_empty() {
        ui::println(&["Not logged in"]);
        return Ok(0);
    }
    if !io::stdin().is_terminal() {
        bail!("switching orgs requires an interactive terminal")
    }
    let client = reqwest::Client::new();
    let mut choices = Vec::new();
    for account in accounts {
        let account = refresh_account_if_needed(&database, account).await?;
        for org in fetch_orgs(&client, &account.url, &account.access_token).await? {
            choices.push((account.id.clone(), account.email.clone(), org));
        }
    }
    if choices.is_empty() {
        ui::println(&["No orgs found"]);
        return Ok(0);
    }
    for (index, (_, email, org)) in choices.iter().enumerate() {
        println!("  {index}) {} ({email})", org.name);
    }
    print!("Select org: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let index = input.trim().parse::<usize>()?;
    let (account_id, _, org) = choices
        .get(index)
        .ok_or_else(|| anyhow!("invalid org selection"))?;
    persist_active(&database, Some(account_id.clone()), Some(org.id.clone()))?;
    ui::println(&[&format!("Switched to {}", org.name)]);
    Ok(0)
}

async fn open() -> anyhow::Result<i32> {
    let database = open_database()?;
    let Some(state) = active_state(&database)? else {
        ui::println(&["No active account"]);
        return Ok(0);
    };
    let Some(account_id) = state.active_account_id else {
        ui::println(&["No active account"]);
        return Ok(0);
    };
    let Some(account) = database.get_by::<oc_database::tables::AccountRow>(
        "account",
        "id",
        &oc_database::Value::Text(account_id),
        oc_database::tables::json_columns("account"),
    )?
    else {
        ui::println(&["No active account"]);
        return Ok(0);
    };
    open_browser(&account.url).await;
    ui::println(&[&format!("Opened {}", account.url)]);
    Ok(0)
}

async fn open_browser(url: &str) {
    let command = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = tokio::process::Command::new(command.0)
        .args(command.1)
        .status()
        .await;
}

fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn open_database() -> anyhow::Result<oc_database::Database> {
    Ok(oc_database::Database::open(oc_database::database::path())?)
}

fn list_accounts(
    database: &oc_database::Database,
) -> anyhow::Result<Vec<oc_database::tables::AccountRow>> {
    Ok(database.list("account", oc_database::tables::json_columns("account"))?)
}

fn active_state(
    database: &oc_database::Database,
) -> anyhow::Result<Option<oc_database::tables::AccountStateRow>> {
    Ok(database.get_by(
        "account_state",
        "id",
        &oc_database::Value::Integer(0),
        oc_database::tables::json_columns("account_state"),
    )?)
}

fn persist_active(
    database: &oc_database::Database,
    account_id: Option<String>,
    org_id: Option<String>,
) -> anyhow::Result<()> {
    let row = oc_database::tables::AccountStateRow {
        id: 0,
        active_account_id: account_id,
        active_org_id: org_id,
    };
    database.upsert(
        "account_state",
        &row,
        oc_database::tables::json_columns("account_state"),
        "id",
        &oc_database::Value::Integer(0),
    )?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_server_url_strips_trailing_slashes() {
        assert_eq!(
            normalize_url(" https://console.example/// "),
            "https://console.example"
        );
    }

    #[test]
    fn active_account_state_round_trips() {
        let database = oc_database::Database::open_memory().unwrap();
        let now = now_ms();
        let row = oc_database::tables::AccountRow {
            id: "acct_test".into(),
            email: "user@example.test".into(),
            url: "https://console.example".into(),
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            token_expiry: Some(now + 60_000),
            time_created: now,
            time_updated: now,
        };
        database
            .insert(
                "account",
                &row,
                oc_database::tables::json_columns("account"),
            )
            .unwrap();
        persist_active(&database, Some("acct_test".into()), Some("org_test".into())).unwrap();
        let state = active_state(&database).unwrap().unwrap();
        assert_eq!(state.active_account_id.as_deref(), Some("acct_test"));
        assert_eq!(state.active_org_id.as_deref(), Some("org_test"));
    }
}
