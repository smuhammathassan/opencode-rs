//! `opencode upgrade [target]`
//! From reference/packages/opencode/src/cli/cmd/upgrade.ts.

use crate::cli::args::{Cli, UpgradeArgs};
use crate::cli::ui::{self, Style};
use crate::cli::upgrade;

pub async fn run(_cli: &Cli, args: &UpgradeArgs) -> anyhow::Result<i32> {
    ui::empty();
    ui::println(&[&ui::logo(Some("  "))]);
    ui::empty();

    let method = args.method.as_deref().unwrap_or("auto");
    ui::println(&["◇  Upgrade"]);
    ui::println(&["│  ", &format!("Using method: {method}")]);

    let target = match &args.target {
        Some(target) => match upgrade::normalize_target(target) {
            Some(target) => target,
            None => {
                ui::println(&[
                    Style::TEXT_DANGER_BOLD,
                    "✖  ",
                    Style::TEXT_NORMAL,
                    "target must be a release version in the form x.y.z",
                ]);
                return Ok(2);
            }
        },
        None => match upgrade::fetch_latest().await {
            Some(latest) => latest,
            None => {
                ui::println(&[
                    Style::TEXT_DANGER_BOLD,
                    "✖  ",
                    Style::TEXT_NORMAL,
                    "failed to fetch the latest version",
                ]);
                return Ok(1);
            }
        },
    };

    match upgrade::upgrade_decision(crate::VERSION, &target) {
        // Either the installed version or the (already normalized) target could
        // not be parsed as a plain `x.y.z` version.
        None => {
            ui::println(&[
                Style::TEXT_DANGER_BOLD,
                "✖  ",
                Style::TEXT_NORMAL,
                "the installed version is invalid; refusing to upgrade",
            ]);
            return Ok(1);
        }
        Some(upgrade::UpgradeDecision::AlreadyInstalled) => {
            ui::println(&[
                Style::TEXT_WARNING_BOLD,
                "▲  ",
                Style::TEXT_NORMAL,
                &format!("opencode upgrade skipped: {target} is already installed"),
            ]);
            ui::println(&["└  Done"]);
            return Ok(0);
        }
        Some(upgrade::UpgradeDecision::RefusesDowngrade) => {
            ui::println(&[
                Style::TEXT_WARNING_BOLD,
                "▲  ",
                Style::TEXT_NORMAL,
                &format!("refusing to downgrade from {} to {target}", crate::VERSION),
            ]);
            return Ok(2);
        }
        Some(upgrade::UpgradeDecision::Proceed) => {}
    }

    ui::println(&[
        Style::TEXT_INFO_BOLD,
        "ℹ  ",
        Style::TEXT_NORMAL,
        &format!("From {} → {target}", crate::VERSION),
    ]);
    if args.dry_run {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "▲  ",
            Style::TEXT_NORMAL,
            "Dry run - no changes made",
        ]);
        ui::println(&["└  Done"]);
        return Ok(0);
    }

    if method != "auto" {
        ui::println(&[
            Style::TEXT_DANGER_BOLD,
            "✖  ",
            Style::TEXT_NORMAL,
            &format!(
                "installation method `{method}` is not available for the bundled Rust installer"
            ),
        ]);
        ui::println(&["└  Done"]);
        return Ok(2);
    }

    let destination = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            ui::println(&[
                Style::TEXT_DANGER_BOLD,
                "✖  ",
                Style::TEXT_NORMAL,
                &format!("cannot locate the running executable: {error}"),
            ]);
            ui::println(&["└  Done"]);
            return Ok(1);
        }
    };
    let platform = match upgrade::current_platform() {
        Ok(platform) => platform,
        Err(error) => {
            ui::println(&[
                Style::TEXT_DANGER_BOLD,
                "✖  ",
                Style::TEXT_NORMAL,
                &error.to_string(),
            ]);
            ui::println(&["└  Done"]);
            return Ok(1);
        }
    };
    match upgrade::install_release(
        &upgrade::ReleaseClient::default(),
        &target,
        platform,
        &destination,
    )
    .await
    {
        Ok(asset) => {
            ui::println(&[
                Style::TEXT_SUCCESS_BOLD,
                "✓  ",
                Style::TEXT_NORMAL,
                &format!("installed {target} from {asset}"),
            ]);
            ui::println(&["└  Done"]);
            return Ok(0);
        }
        Err(error) => {
            ui::println(&[
                Style::TEXT_DANGER_BOLD,
                "✖  ",
                Style::TEXT_NORMAL,
                &format!("upgrade failed: {error}"),
            ]);
        }
    }
    ui::println(&["└  Done"]);
    Ok(1)
}
