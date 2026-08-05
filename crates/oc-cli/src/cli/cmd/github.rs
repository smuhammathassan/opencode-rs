//! `opencode github`
//! From reference/packages/opencode/src/cli/cmd/github.ts.

use crate::cli::args::{Cli, GithubArgs, GithubCommand};
use crate::cli::effect_cmd::not_wired;

pub async fn run(_cli: &Cli, args: &GithubArgs) -> anyhow::Result<i32> {
    match &args.command {
        GithubCommand::Install => install().await,
        GithubCommand::Run { event, token } => {
            let _ = (event, token);
            run_agent().await
        }
    }
}

async fn install() -> anyhow::Result<i32> {
    // TODO(integration): `githubInstall` from github.handler.ts.
    Err(not_wired(
        "the GitHub agent installer is not yet wired in this build (TODO(integration): oc-sync)",
    ))
}

async fn run_agent() -> anyhow::Result<i32> {
    // TODO(integration): `githubRun` from github.handler.ts.
    Err(not_wired(
        "the GitHub agent runner is not yet wired in this build (TODO(integration): oc-sync)",
    ))
}
