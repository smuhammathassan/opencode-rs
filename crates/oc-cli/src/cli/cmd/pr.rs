//! `opencode pr <number>`
//! From reference/packages/opencode/src/cli/cmd/pr.ts.

use crate::cli::args::{Cli, PrArgs};
use crate::cli::context::Context;
use crate::cli::ui;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubRemote {
    owner: String,
    repository: String,
}

pub async fn run(_cli: &Cli, args: &PrArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    if ctx.project.vcs != crate::cli::context::Vcs::Git {
        return Err(anyhow::anyhow!(
            "Could not find git repository. Please run this command from a git repository."
        ));
    }

    let remote = git_remote(&ctx.worktree)?;

    let pr_number = args.number;
    if pr_number == 0 {
        return Err(anyhow::anyhow!("PR number must be greater than zero"));
    }
    let local_branch_name = format!("pr/{pr_number}");
    ui::println(&[&format!(
        "Fetching and checking out {}/{} PR #{pr_number}...",
        remote.owner, remote.repository
    )]);

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

    ui::println(&[&format!(
        "Successfully checked out PR #{pr_number} as branch '{local_branch_name}'"
    )]);

    let mut options = oc_server::server::ListenOptions::new("127.0.0.1", 0);
    options.auth = oc_server::auth::AuthConfig::from_env();
    let listener = oc_server::server::listen(options).await?;
    let cwd = std::env::current_dir()?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let state_dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("opencode");
    let result = oc_tui::run_async(oc_tui::TuiInput {
        url: listener.url.to_string(),
        directory: Some(cwd.to_string_lossy().into_owned()),
        workspace: None,
        cwd,
        home,
        state_dir,
        config: oc_tui::config::ResolvedConfig::from_environment(),
        continue_session: false,
        session_id: None,
        agent: None,
        model: None,
        prompt: None,
        initial_parts: Vec::new(),
        replay: true,
        replay_limit: None,
    })
    .await;
    listener.stop(false).await;
    result.map(|()| 0).map_err(|error| {
        anyhow::anyhow!("PR checked out, but the interactive TUI failed to start: {error}")
    })
}

fn git_remote(worktree: &std::path::Path) -> anyhow::Result<GithubRemote> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(worktree)
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!("Could not find the origin GitHub remote"));
    }
    parse_github_remote(String::from_utf8_lossy(&output.stdout).trim())
        .ok_or_else(|| anyhow::anyhow!("origin is not a GitHub remote"))
}

fn parse_github_remote(remote: &str) -> Option<GithubRemote> {
    let value = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("http://github.com/"))?
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let mut segments = value.split('/');
    let owner = segments.next()?.trim();
    let repository = segments.next()?.trim();
    if segments.next().is_some() || owner.is_empty() || repository.is_empty() {
        return None;
    }
    Some(GithubRemote {
        owner: owner.to_string(),
        repository: repository.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_github_remote, GithubRemote};

    #[test]
    fn parses_supported_github_remote_forms() {
        for remote in [
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/owner/repo",
            "https://github.com/owner/repo.git",
        ] {
            assert_eq!(
                parse_github_remote(remote),
                Some(GithubRemote {
                    owner: "owner".into(),
                    repository: "repo".into(),
                })
            );
        }
    }

    #[test]
    fn rejects_non_github_or_nested_remotes() {
        assert_eq!(parse_github_remote("https://gitlab.com/owner/repo"), None);
        assert_eq!(
            parse_github_remote("https://github.com/owner/repo/extra"),
            None
        );
    }
}
