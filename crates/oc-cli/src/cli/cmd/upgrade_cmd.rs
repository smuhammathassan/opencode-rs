//! `opencode upgrade [target]`
//! From reference/packages/opencode/src/cli/cmd/upgrade.ts.

use crate::cli::args::{Cli, UpgradeArgs};
use crate::cli::ui::{self, Style};
use crate::cli::upgrade;

pub async fn run(_cli: &Cli, args: &UpgradeArgs) -> anyhow::Result<i32> {
    ui::empty();
    ui::println(&[&ui::logo(Some("  "))]);
    ui::empty();

    let method = args.method.clone().unwrap_or_else(|| "unknown".to_string());
    ui::println(&["◇  Upgrade"]);
    ui::println(&["│  ", &format!("Using method: {method}")]);

    let target = match &args.target {
        Some(target) => target.trim_start_matches('v').to_string(),
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

    if crate::VERSION == target {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "▲  ",
            Style::TEXT_NORMAL,
            &format!("opencode upgrade skipped: {target} is already installed"),
        ]);
        ui::println(&["└  Done"]);
        return Ok(0);
    }

    ui::println(&[
        Style::TEXT_INFO_BOLD,
        "ℹ  ",
        Style::TEXT_NORMAL,
        &format!("From {} → {target}", crate::VERSION),
    ]);
    ui::println(&[
        Style::TEXT_WARNING_BOLD,
        "!  ",
        Style::TEXT_NORMAL,
        "the Rust port is installed in-process; automatic upgrades are not supported.",
    ]);
    ui::println(&["└  Done"]);
    Ok(0)
}
