//! `opencode agent`
//! From reference/packages/opencode/src/cli/cmd/agent.ts.

use crate::cli::args::{AgentArgs, AgentCommand, AgentCreateArgs, Cli};
use crate::cli::context::Context;
use oc_config::load::{load_agent_modes, load_agents};
use std::path::PathBuf;

pub async fn run(_cli: &Cli, args: &AgentArgs) -> anyhow::Result<i32> {
    match &args.command {
        AgentCommand::Create(create) => create_agent(create).await,
        AgentCommand::List => list_agents().await,
    }
}

async fn create_agent(args: &AgentCreateArgs) -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    let description = args
        .description
        .as_deref()
        .unwrap_or("General-purpose coding agent")
        .trim();
    if description.is_empty() {
        return Err(anyhow::anyhow!("--description must not be empty"));
    }
    let permissions = parse_permissions(args.permissions.as_deref())?;
    let name = slugify(description);
    let directory = args
        .path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.worktree.join(".opencode").join("agents"));
    std::fs::create_dir_all(&directory)?;
    let file = unique_agent_path(&directory, &name);
    let mut frontmatter = vec![format!("description: {}", yaml_quote(description))];
    if let Some(mode) = &args.mode {
        frontmatter.push(format!("mode: {mode}"));
    }
    if let Some(model) = &args.model {
        frontmatter.push(format!("model: {}", yaml_quote(model)));
    }
    frontmatter.push("permission:".to_string());
    for permission in permissions {
        frontmatter.push(format!("  {permission}: allow"));
    }
    let content = format!("---\n{}\n---\n\n{}\n", frontmatter.join("\n"), description);
    std::fs::write(&file, content)?;
    println!(
        "created agent `{}` at {}",
        file.file_stem().unwrap().to_string_lossy(),
        file.display()
    );
    Ok(0)
}

fn parse_permissions(value: Option<&str>) -> anyhow::Result<Vec<String>> {
    const AVAILABLE: [&str; 11] = [
        "bash",
        "read",
        "edit",
        "glob",
        "grep",
        "webfetch",
        "task",
        "todowrite",
        "websearch",
        "lsp",
        "skill",
    ];
    let values = value
        .map(|value| value.split(',').map(str::trim).collect::<Vec<_>>())
        .unwrap_or_else(|| AVAILABLE.iter().copied().collect());
    let mut result = Vec::new();
    for permission in values {
        if permission.is_empty() {
            continue;
        }
        if !AVAILABLE.contains(&permission) {
            return Err(anyhow::anyhow!(
                "unknown permission `{permission}`; available: {}",
                AVAILABLE.join(", ")
            ));
        }
        if !result.iter().any(|existing| existing == permission) {
            result.push(permission.to_string());
        }
    }
    Ok(result)
}

fn slugify(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
        } else if !result.ends_with('-') && !result.is_empty() {
            result.push('-');
        }
        if result.len() >= 48 {
            break;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        "custom-agent".to_string()
    } else {
        result
    }
}

fn unique_agent_path(directory: &std::path::Path, name: &str) -> PathBuf {
    let first = directory.join(format!("{name}.md"));
    if !first.exists() {
        return first;
    }
    for index in 2..1000 {
        let candidate = directory.join(format!("{name}-{index}.md"));
        if !candidate.exists() {
            return candidate;
        }
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    directory.join(format!("{name}-{millis}.md"))
}

fn yaml_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " ./_-".contains(c))
        && !value.starts_with('-')
        && !value.starts_with(':')
        && !value.starts_with('#')
    {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

async fn list_agents() -> anyhow::Result<i32> {
    let ctx = Context::load(std::env::current_dir()?)?;
    let mut agents = load_agents(&ctx.directory)?;
    for (name, info) in load_agent_modes(&ctx.directory)? {
        agents.insert(name, info);
    }

    println!("{:<24} {:<10} DESCRIPTION", "NAME", "MODE");
    println!("{:<24} {:<10} built-in build agent", "build", "all");
    for (name, info) in agents {
        let mode = info
            .mode
            .map(|mode| match mode {
                oc_config::v1::agent::Mode::Subagent => "subagent",
                oc_config::v1::agent::Mode::Primary => "primary",
                oc_config::v1::agent::Mode::All => "all",
            })
            .unwrap_or("all");
        println!(
            "{:<24} {:<10} {}",
            name,
            mode,
            info.description.as_deref().unwrap_or("")
        );
    }
    Ok(0)
}
