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

/// Fork/session metadata parsed from `gh pr view --json`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PrInfo {
    is_cross_repository: bool,
    fork_owner: Option<String>,
    head_repo: Option<String>,
    head_ref: Option<String>,
    body: Option<String>,
}

/// Parse the `gh pr view <n> --json headRepository,headRepositoryOwner,
/// isCrossRepository,headRefName,body` output (F031).
fn parse_pr_info(json: &str) -> Option<PrInfo> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let is_cross_repository = value
        .get("isCrossRepository")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let fork_owner = value
        .get("headRepositoryOwner")
        .and_then(|owner| owner.get("login"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let head_repo = value
        .get("headRepository")
        .and_then(|repo| repo.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let head_ref = value
        .get("headRefName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let body = value
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Some(PrInfo {
        is_cross_repository,
        fork_owner,
        head_repo,
        head_ref,
        body,
    })
}

/// The git commands needed to wire up a cross-repository (fork) PR after a
/// `gh pr checkout`, returning `(remote_name, upstream_branch)` plus the
/// commands to run. Mirrors the fork-remote + set-upstream step in
/// `reference/packages/opencode/src/cli/cmd/pr.ts`.
///
/// `existing_remotes` is the whitespace/newline-separated `git remote` output
/// for the worktree. `local_branch` is the checkout branch (`pr/<n>`).
fn fork_metadata_commands(
    info: &PrInfo,
    existing_remotes: &str,
    local_branch: &str,
) -> (Option<String>, Vec<Vec<String>>) {
    if !info.is_cross_repository {
        return (None, Vec::new());
    }
    let Some(fork_owner) = info.fork_owner.as_deref() else {
        return (None, Vec::new());
    };
    let Some(head_repo) = info.head_repo.as_deref() else {
        return (None, Vec::new());
    };
    let Some(head_ref) = info.head_ref.as_deref() else {
        return (None, Vec::new());
    };
    let remote_name = fork_owner;
    let remotes: Vec<&str> = existing_remotes
        .split_whitespace()
        .filter(|remote| !remote.is_empty())
        .collect();
    let mut commands = Vec::new();
    if !remotes.contains(&remote_name) {
        commands.push(vec![
            "remote".to_string(),
            "add".to_string(),
            remote_name.to_string(),
            format!("https://github.com/{fork_owner}/{head_repo}.git"),
        ]);
    }
    commands.push(vec![
        "branch".to_string(),
        format!("--set-upstream-to={remote_name}/{head_ref}"),
        local_branch.to_string(),
    ]);
    (Some(remote_name.to_string()), commands)
}

/// Extract a session ID from an `https://opncd.ai/s/<id>` link in a PR body
/// (F031 session-link handoff). `None` when no link is present.
fn session_link_id(body: Option<&str>) -> Option<String> {
    let body = body?;
    // Reference regex: /https:\/\/opncd\.ai\/s\/([a-zA-Z0-9_-]+)/
    const PREFIX: &str = "https://opncd.ai/s/";
    let start = body.find(PREFIX)?;
    let rest = &body[start + PREFIX.len()..];
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
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

    // F031: read fork metadata and any session-share link from `gh pr view --json`.
    // For a cross-repository (fork) PR, add the fork remote and set the local
    // branch upstream; a session link in the PR body is handed to the client.
    let mut session_id: Option<String> = None;
    if let Ok(pr_view) = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "headRepository,headRepositoryOwner,isCrossRepository,headRefName,body",
        ])
        .output()
    {
        if pr_view.status.success() {
            if let Some(info) = parse_pr_info(&String::from_utf8_lossy(&pr_view.stdout)) {
                let remotes = std::process::Command::new("git")
                    .args(["remote"])
                    .current_dir(&ctx.worktree)
                    .output()
                    .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
                    .unwrap_or_default();
                let (remote_name, commands) =
                    fork_metadata_commands(&info, &remotes, &local_branch_name);
                if let Some(name) = &remote_name {
                    ui::println(&[&format!("Added fork remote: {name}")]);
                }
                for command in commands {
                    let _ = std::process::Command::new("git")
                        .args(&command)
                        .current_dir(&ctx.worktree)
                        .status();
                }
                session_id = session_link_id(info.body.as_deref());
                if let Some(id) = &session_id {
                    ui::println(&[&format!("Found opencode session: {id}")]);
                }
            }
        }
    }

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
        continue_session: session_id.is_some(),
        session_id,
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
    use super::{
        fork_metadata_commands, parse_github_remote, parse_pr_info, session_link_id, GithubRemote,
        PrInfo,
    };

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

    #[test]
    fn parses_pr_view_fork_metadata() {
        let info = parse_pr_info(
            r#"{
                "isCrossRepository": true,
                "headRepositoryOwner": {"login": "forker"},
                "headRepository": {"name": "head-repo", "owner": {"login": "forker"}},
                "headRefName": "feat-1",
                "body": "Fixes #2"
            }"#,
        )
        .unwrap();
        assert_eq!(info.is_cross_repository, true);
        assert_eq!(info.fork_owner.as_deref(), Some("forker"));
        assert_eq!(info.head_repo.as_deref(), Some("head-repo"));
        assert_eq!(info.head_ref.as_deref(), Some("feat-1"));
        assert_eq!(info.body.as_deref(), Some("Fixes #2"));

        // Non-fork PR: no head metadata required.
        let info = parse_pr_info(
            r#"{"isCrossRepository": false, "headRepositoryOwner": {"login":"owner"}, "headRepository": {"name":"repo"}, "headRefName": "main", "body": ""}"#,
        )
        .unwrap();
        assert!(!info.is_cross_repository);

        // Malformed JSON / missing fields degrade to None on invalid JSON.
        assert!(parse_pr_info("not-json").is_none());
        assert_eq!(
            parse_pr_info(r#"{"isCrossRepository": true}"#)
                .unwrap()
                .fork_owner,
            None
        );
    }

    #[test]
    fn fork_metadata_commands_set_upstream_and_add_remote() {
        let info = PrInfo {
            is_cross_repository: true,
            fork_owner: Some("forker".into()),
            head_repo: Some("head-repo".into()),
            head_ref: Some("feat-1".into()),
            body: None,
        };
        // Remote missing -> add it, then set upstream.
        let (remote, commands) = fork_metadata_commands(&info, "origin", "pr/5");
        assert_eq!(remote.as_deref(), Some("forker"));
        assert_eq!(
            commands,
            vec![
                vec![
                    "remote",
                    "add",
                    "forker",
                    "https://github.com/forker/head-repo.git"
                ],
                vec!["branch", "--set-upstream-to=forker/feat-1", "pr/5"],
            ]
        );

        // Remote already present -> only set upstream.
        let (_, commands) = fork_metadata_commands(&info, "origin  forker", "pr/5");
        assert_eq!(
            commands,
            vec![vec!["branch", "--set-upstream-to=forker/feat-1", "pr/5"]]
        );

        // Non-cross-repository PR -> no commands.
        let same_repo = PrInfo {
            is_cross_repository: false,
            fork_owner: Some("owner".into()),
            head_repo: Some("repo".into()),
            head_ref: Some("main".into()),
            body: None,
        };
        let (remote, commands) = fork_metadata_commands(&same_repo, "", "pr/5");
        assert_eq!(remote, None);
        assert!(commands.is_empty());
    }

    #[test]
    fn extracts_session_link_id_from_pr_body() {
        assert_eq!(
            session_link_id(Some("See https://opncd.ai/s/abc123 here")),
            Some("abc123".into())
        );
        assert_eq!(
            session_link_id(Some("https://opncd.ai/s/ses_09-abc_DEF.front")),
            Some("ses_09-abc_DEF".into())
        );
        assert_eq!(session_link_id(Some("no link present")), None);
        assert_eq!(session_link_id(None), None);
    }
}
