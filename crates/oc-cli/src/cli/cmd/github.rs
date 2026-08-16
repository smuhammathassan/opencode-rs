//! `opencode github`
//! From reference/packages/opencode/src/cli/cmd/github.ts.

use std::path::Path;

use crate::cli::args::{Cli, GithubArgs, GithubCommand};
use crate::cli::context::{Context, Vcs};
use crate::cli::models_dev::ModelsDev;
use crate::cli::ui;

pub async fn run(_cli: &Cli, args: &GithubArgs) -> anyhow::Result<i32> {
    match &args.command {
        GithubCommand::Install => install().await,
        GithubCommand::Run { event, token } => run_agent(event.as_deref(), token.as_deref()).await,
    }
}

async fn install() -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    if ctx.project.vcs != Vcs::Git {
        anyhow::bail!(
            "Could not find git repository. Please run this command from a git repository."
        );
    }
    let (owner, repository) = github_remote(&ctx.worktree)?;
    let model = std::env::var("OPENCODE_GITHUB_MODEL")
        .or_else(|_| std::env::var("MODEL"))
        .map_err(|_| {
            anyhow::anyhow!(
                "set OPENCODE_GITHUB_MODEL (provider/model) before installing the GitHub agent"
            )
        })?;
    let (provider_id, _) = model
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("GitHub model must use provider/model format: {model}"))?;
    let models = ModelsDev::load(&ctx.paths).unwrap_or_default();
    let env = models
        .providers
        .get(provider_id)
        .map(|provider| provider.env.as_slice())
        .unwrap_or(&[]);
    let workflow = render_workflow(&model, env);
    let path = ctx.worktree.join(".github/workflows/opencode.yml");
    if path.exists() && !env_flag("OPENCODE_GITHUB_FORCE") {
        anyhow::bail!(
            "GitHub workflow already exists at {} (set OPENCODE_GITHUB_FORCE=1 to replace it)",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, workflow)?;
    ui::println(&[
        &format!("Added GitHub workflow for {owner}/{repository}"),
        &format!("  {}", path.display()),
        "Install the opencode-agent GitHub App, then commit and push this workflow.",
    ]);
    Ok(0)
}

async fn run_agent(event: Option<&str>, token: Option<&str>) -> anyhow::Result<i32> {
    let model = std::env::var("OPENCODE_GITHUB_MODEL")
        .or_else(|_| std::env::var("MODEL"))
        .map_err(|_| anyhow::anyhow!("set OPENCODE_GITHUB_MODEL or MODEL to provider/model"))?;
    let prompt = event
        .map(parse_event_prompt)
        .transpose()?
        .or_else(|| std::env::var("PROMPT").ok())
        .unwrap_or_else(|| {
            "Review this repository and summarize the most important changes.".into()
        });
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args(["run", "--model", &model, "--auto", &prompt])
        .current_dir(std::env::current_dir()?);
    if let Some(token) = token {
        command.env("GITHUB_TOKEN", token);
    }
    let status = command.status().await?;
    Ok(status.code().unwrap_or(1))
}

fn github_remote(worktree: &Path) -> anyhow::Result<(String, String)> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(worktree)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("Could not find the origin GitHub remote");
    }
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let value = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .ok_or_else(|| anyhow::anyhow!("origin is not a GitHub remote: {remote}"))?
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let (owner, repository) = value
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("origin is not a GitHub owner/repository: {remote}"))?;
    if owner.is_empty() || repository.is_empty() || repository.contains('/') {
        anyhow::bail!("origin is not a GitHub owner/repository: {remote}");
    }
    Ok((owner.to_string(), repository.to_string()))
}

fn render_workflow(model: &str, provider_env: &[String]) -> String {
    let mut workflow = format!(
        "name: opencode\n\non:\n  issue_comment:\n    types: [created]\n  pull_request_review_comment:\n    types: [created]\n\npermissions:\n  id-token: write\n  contents: read\n  pull-requests: read\n  issues: read\n\njobs:\n  opencode:\n    if: contains(github.event.comment.body, '/oc') || startsWith(github.event.comment.body, '/opencode')\n    runs-on: ubuntu-latest\n    steps:\n      - name: Checkout repository\n        uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n      - name: Run opencode\n        uses: anomalyco/opencode/github@latest\n        with:\n          model: {model}\n",
        model = model
    );
    if !provider_env.is_empty() {
        workflow.push_str("        env:\n");
        for name in provider_env {
            workflow.push_str(&format!("          {name}: ${{{{ secrets.{name} }}}}\n"));
        }
    }
    workflow
}

fn parse_event_prompt(event: &str) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_str(event)
        .map_err(|error| anyhow::anyhow!("invalid GitHub event JSON: {error}"))?;
    if let Some(body) = value
        .pointer("/comment/body")
        .or_else(|| value.pointer("/issue/body"))
        .or_else(|| value.pointer("/pull_request/body"))
        .and_then(serde_json::Value::as_str)
        .filter(|body| !body.trim().is_empty())
    {
        return Ok(body.to_string());
    }
    let title = value
        .pointer("/issue/title")
        .or_else(|| value.pointer("/pull_request/title"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("repository event");
    Ok(format!("Review or handle this GitHub event: {title}"))
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_contains_model_and_secret_mapping() {
        let workflow = render_workflow("openai/gpt-4o", &["OPENAI_API_KEY".into()]);
        assert!(workflow.contains("model: openai/gpt-4o"));
        assert!(workflow.contains("OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}"));
        assert!(workflow.contains("anomalyco/opencode/github@latest"));
    }

    #[test]
    fn event_prompt_prefers_comment_body() {
        let prompt = parse_event_prompt(
            r#"{"comment":{"body":"/oc review this"},"issue":{"title":"ignored"}}"#,
        )
        .unwrap();
        assert_eq!(prompt, "/oc review this");
    }

    #[test]
    fn event_prompt_rejects_invalid_json() {
        assert!(parse_event_prompt("not-json").is_err());
    }
}
