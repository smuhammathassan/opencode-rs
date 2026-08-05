//! `opencode mcp`
//! From reference/packages/opencode/src/cli/cmd/mcp.ts.

use crate::cli::args::{
    Cli, McpAddArgs, McpArgs, McpAuthArgs, McpAuthCommand, McpCommand, McpDebugArgs, McpLogoutArgs,
};
use crate::cli::effect_cmd::not_wired;

pub async fn run(_cli: &Cli, args: &McpArgs) -> anyhow::Result<i32> {
    match &args.command {
        McpCommand::Add(add) => run_add(add).await,
        McpCommand::List => run_list().await,
        McpCommand::Auth(auth) => run_auth(auth).await,
        McpCommand::Logout(logout) => run_logout(logout).await,
        McpCommand::Debug(debug) => run_debug(debug).await,
    }
}

async fn run_add(args: &McpAddArgs) -> anyhow::Result<i32> {
    let command = &args.command;
    if args.name.is_none()
        && (args.url.is_some()
            || !args.env.is_empty()
            || !args.header.is_empty()
            || !command.is_empty())
    {
        return Err(anyhow::anyhow!(
            "A server name is required for non-interactive MCP configuration"
        ));
    }
    if let Some(name) = &args.name {
        let has_command = !command.is_empty();
        if args.url.is_some() == has_command {
            return Err(anyhow::anyhow!(
                "Provide either --url <url> or a command after --"
            ));
        }
        if let Some(url) = &args.url {
            if url::Url::parse(url).is_err() {
                return Err(anyhow::anyhow!("Invalid URL: {url}"));
            }
            if !args.env.is_empty() {
                return Err(anyhow::anyhow!("--env is only valid for local MCP servers"));
            }
        }
        if has_command && !args.header.is_empty() {
            return Err(anyhow::anyhow!(
                "--header is only valid for remote MCP servers"
            ));
        }

        // TODO(integration): write the MCP server config into opencode.json via
        // `oc_config`, mirroring `addMcpToConfig` in mcp.ts.
        let _ = name;
        return Err(not_wired("adding MCP servers is not yet wired in this build (TODO(integration): oc-config/oc-mcp)"));
    }

    // Interactive add path.
    Err(not_wired(
        "interactive MCP server setup is not yet wired in this build (TODO(integration): oc-mcp)",
    ))
}

async fn run_list() -> anyhow::Result<i32> {
    Err(not_wired(
        "MCP server listing is not yet wired in this build (TODO(integration): oc-mcp/oc-config)",
    ))
}

async fn run_auth(args: &McpAuthArgs) -> anyhow::Result<i32> {
    if let Some(McpAuthCommand::List) = args.command {
        return Err(not_wired(
            "MCP OAuth status is not yet wired in this build (TODO(integration): oc-mcp)",
        ));
    }
    let _ = &args.name;
    Err(not_wired(
        "MCP OAuth authentication is not yet wired in this build (TODO(integration): oc-mcp)",
    ))
}

async fn run_logout(args: &McpLogoutArgs) -> anyhow::Result<i32> {
    let _ = &args.name;
    Err(not_wired(
        "MCP OAuth logout is not yet wired in this build (TODO(integration): oc-mcp)",
    ))
}

async fn run_debug(args: &McpDebugArgs) -> anyhow::Result<i32> {
    let _ = &args.name;
    Err(not_wired(
        "MCP OAuth debugging is not yet wired in this build (TODO(integration): oc-mcp)",
    ))
}
