//! `opencode pr <number>`
//! From reference/packages/opencode/src/cli/cmd/pr.ts.

use crate::cli::args::{Cli, PrArgs};
use crate::cli::context::Context;
use crate::cli::ui::{self, Style};

pub async fn run(_cli: &Cli, args: &PrArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    if ctx.project.vcs != crate::cli::context::Vcs::Git {
        return Err(anyhow::anyhow!(
            "Could not find git repository. Please run this command from a git repository."
        ));
    }

    let pr_number = args.number;
    let local_branch_name = format!("pr/{pr_number}");
    ui::println(&[&format!("Fetching and checking out PR #{pr_number}...")]);

    let checkout = std::process::Command::new("gh")
        .args([
            "pr",
            "checkout",
            &pr_number.to_string(),
            "--branch",
            &local_branch_name,
            "--force",
        ])
        .output();
    match checkout {
        Ok(output) if output.status.success() => {}
        _ => {
            return Err(anyhow::anyhow!(
                "Failed to checkout PR #{pr_number}. Make sure you have gh CLI installed and authenticated."
            ));
        }
    }

    // TODO(integration): parse `gh pr view` JSON for fork remotes + session
    // links, and re-exec the TUI with `-s <session>` like pr.ts does.
    ui::println(&[&format!(
        "Successfully checked out PR #{pr_number} as branch '{local_branch_name}'"
    )]);
    ui::println(&[Style::TEXT_DIM, "  (opening the TUI is not yet wired)"]);
    Ok(0)
}
