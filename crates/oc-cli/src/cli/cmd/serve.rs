//! `opencode serve`
//! From reference/packages/opencode/src/cli/cmd/serve.ts.

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

    // Keep the command alive until the process lifecycle receives SIGINT,
    // SIGTERM, or the equivalent supported console signal on Windows.
    oc_util::util::signal::process_shutdown().wait().await;
    server.stop(false).await;
    Ok(0)
}

/// Start the real Axum server from `oc-server`.
///
/// Keeping this conversion at the CLI boundary means `serve`, `run --attach`,
/// and embedders all exercise the same router, middleware, SSE bus, and state
/// initialization instead of maintaining a second socket-only implementation.
async fn listen(opts: &ResolvedNetwork) -> anyhow::Result<oc_server::server::Listener> {
    let mut server = oc_server::server::ListenOptions::new(&opts.hostname, opts.port);
    server.auth = oc_server::auth::AuthConfig::from_env();
    server.cors = oc_server::cors::CorsOptions {
        cors: (!opts.cors.is_empty()).then(|| opts.cors.clone()),
    };
    server.mdns = opts.mdns;
    server.mdns_domain = Some(opts.mdns_domain.clone());
    Ok(oc_server::server::listen(server).await?)
}

/// Load the `server` section of the resolved config.
/// TODO(integration): load via `oc_config` once config resolution lands.
fn server_config(ctx: &Context) -> Option<crate::cli::network::ServerConfig> {
    let state = oc_config::load::load_instance_state(&oc_config::load::LoadOptions {
        directory: ctx.directory.to_string_lossy().into_owned(),
        worktree: Some(ctx.worktree.to_string_lossy().into_owned()),
        ..Default::default()
    })
    .ok()?;
    let server = state.config.server?;
    Some(crate::cli::network::ServerConfig {
        port: server.port.map(|port| port.get() as u16),
        hostname: server.hostname,
        mdns: server.mdns,
        mdns_domain: server.mdnsDomain,
        cors: server.cors.unwrap_or_default(),
    })
}
