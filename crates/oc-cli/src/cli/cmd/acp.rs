//! `opencode acp`
//! From reference/packages/opencode/src/cli/cmd/acp.ts.

use std::net::SocketAddr;

use anyhow::Context as _;

use crate::cli::args::{AcpArgs, Cli};
use crate::cli::context::Context;
use crate::cli::network::resolve_network_options;

pub async fn run(_cli: &Cli, args: &AcpArgs) -> anyhow::Result<i32> {
    let _ctx = Context::load(std::env::current_dir()?)?;
    let opts = resolve_network_options(&args.network, None);

    // TODO(integration): start the ACP (Agent Client Protocol) bridge once
    // oc-acp + oc-server land. Today we only bind the listen socket.
    let addr: SocketAddr = format!("{}:{}", opts.hostname, opts.port)
        .parse()
        .with_context(|| format!("Invalid listen address {}:{}", opts.hostname, opts.port))?;
    let _listener = tokio::net::TcpListener::bind(addr).await?;

    let _ = args.cwd;
    std::future::pending::<()>().await;
    #[allow(unreachable_code)]
    Ok(0)
}
