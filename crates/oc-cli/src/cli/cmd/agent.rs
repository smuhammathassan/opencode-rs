//! `opencode agent`
//! From reference/packages/opencode/src/cli/cmd/agent.ts.

use crate::cli::args::{AgentArgs, AgentCommand, AgentCreateArgs, Cli};
use crate::cli::effect_cmd::not_wired;

const AVAILABLE_PERMISSIONS: [&str; 11] = [
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

pub async fn run(_cli: &Cli, args: &AgentArgs) -> anyhow::Result<i32> {
    match &args.command {
        AgentCommand::Create(create) => create_agent(create).await,
        AgentCommand::List => list_agents().await,
    }
}

async fn create_agent(args: &AgentCreateArgs) -> anyhow::Result<i32> {
    let _ = args;
    // TODO(integration): generate the agent via `oc_llm` + write the markdown
    // agent file, mirroring `AgentCreateCommand` in agent.ts.
    Err(not_wired(
        "agent creation is not yet wired in this build (TODO(integration): oc-llm/oc-command)",
    ))
}

async fn list_agents() -> anyhow::Result<i32> {
    let _ = AVAILABLE_PERMISSIONS;
    // TODO(integration): list agents via `oc_command`.
    Err(not_wired(
        "agent listing is not yet wired in this build (TODO(integration): oc-command)",
    ))
}
