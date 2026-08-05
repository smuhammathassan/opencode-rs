//! `opencode console` (hidden account command).
//! From reference/packages/opencode/src/cli/cmd/account.ts.

use crate::cli::args::{Cli, ConsoleArgs, ConsoleCommand};
use crate::cli::effect_cmd::not_wired;
use crate::cli::ui;

pub async fn run(_cli: &Cli, args: &ConsoleArgs) -> anyhow::Result<i32> {
    let Some(command) = &args.command else {
        ui::error("a console subcommand is required");
        return Ok(1);
    };
    match command {
        ConsoleCommand::Login { .. }
        | ConsoleCommand::Logout { .. }
        | ConsoleCommand::Switch
        | ConsoleCommand::Orgs
        | ConsoleCommand::Open => {
            // TODO(integration): device-code login against console.opencode.ai
            // via oc-sync's Account service (account.ts).
            Err(not_wired("the console account flow is not yet wired in this build (TODO(integration): oc-sync)"))
        }
    }
}
