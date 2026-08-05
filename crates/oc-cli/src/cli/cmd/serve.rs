//! `opencode serve`
//! From reference/packages/opencode/src/cli/cmd/serve.ts.

use anyhow::Context as _;
use std::net::SocketAddr;

use crate::cli::args::{Cli, ServeArgs};
use crate::cli::context::Context;
use crate::cli::network::{resolve_network_options, ResolvedNetwork};

/// Mirrors the `serve` command handler. The reference delegates to
/// `Server.listen(opts)` and blocks forever (`Effect.never`).
pub async fn run(_cli: &Cli, args: &ServeArgs) -> anyhow::Result<i32> {
    if std::env::var_os("OPENCODE_SERVER_PASSWORD").is_none() {
        println!("Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.");
    }

    let ctx = Context::load(std::env::current_dir()?)?;
    let opts = resolve_network_options(&args.network, server_config(&ctx).as_ref());
    let server = listen(&opts).await?;
    println!(
        "opencode server listening on http://{}:{}",
        server.hostname, server.port
    );

    // Block forever, mirroring `Effect.never`.
    std::future::pending::<()>().await;
    #[allow(unreachable_code)]
    Ok(0)
}

struct ListeningServer {
    hostname: String,
    port: u16,
}

/// Bind a real TCP socket so `serve` genuinely listens.
/// TODO(integration): delegate to `oc_server::Server::listen(opts)` once
/// oc-server lands its HTTP server, instead of binding a bare socket here.
async fn listen(opts: &ResolvedNetwork) -> anyhow::Result<ListeningServer> {
    let addr: SocketAddr = format!("{}:{}", opts.hostname, opts.port)
        .parse()
        .with_context(|| format!("Invalid listen address {}:{}", opts.hostname, opts.port))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut buf = [0u8; 1024];
                loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                }
            });
        }
    });
    Ok(ListeningServer {
        hostname: opts.hostname.clone(),
        port,
    })
}

/// Load the `server` section of the resolved config.
/// TODO(integration): load via `oc_config` once config resolution lands.
fn server_config(_ctx: &Context) -> Option<crate::cli::network::ServerConfig> {
    None
}
