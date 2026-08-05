//! `opencode web`
//! From reference/packages/opencode/src/cli/cmd/web.ts.

use std::net::SocketAddr;

use anyhow::Context as _;

use crate::cli::args::{Cli, WebArgs};
use crate::cli::context::Context;
use crate::cli::network::resolve_network_options;
use crate::cli::ui::{self, Style};

fn network_ips() -> Vec<String> {
    // Best-effort: enumerate local IPv4 addresses from /proc/net or hostname.
    let mut results = Vec::new();
    if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
        if let Ok(addr) =
            std::net::ToSocketAddrs::to_socket_addrs(&(hostname.trim().to_string() + ":0"))
        {
            for addr in addr {
                if let std::net::IpAddr::V4(ip) = addr.ip() {
                    if !ip.is_loopback() && ip.octets()[0] != 172 {
                        results.push(ip.to_string());
                    }
                }
            }
        }
    }
    results
}

pub async fn run(_cli: &Cli, args: &WebArgs) -> anyhow::Result<i32> {
    if std::env::var_os("OPENCODE_SERVER_PASSWORD").is_none() {
        ui::println(&[
            Style::TEXT_WARNING_BOLD,
            "!  ",
            Style::TEXT_NORMAL,
            "OPENCODE_SERVER_PASSWORD is not set; server is unsecured.",
        ]);
    }

    let _ctx = Context::load(std::env::current_dir()?)?;
    let opts = resolve_network_options(&args.network, None);

    let addr: SocketAddr = format!("{}:{}", opts.hostname, opts.port)
        .parse()
        .with_context(|| format!("Invalid listen address {}:{}", opts.hostname, opts.port))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    drop(listener);

    // TODO(integration): serve the web interface via oc-server once wired.
    ui::empty();
    ui::println(&[&ui::logo(Some("  "))]);
    ui::empty();

    if opts.hostname == "0.0.0.0" {
        let localhost_url = format!("http://localhost:{port}");
        ui::println(&[
            Style::TEXT_INFO_BOLD,
            "  Local access:      ",
            Style::TEXT_NORMAL,
            &localhost_url,
        ]);
        for ip in network_ips() {
            ui::println(&[
                Style::TEXT_INFO_BOLD,
                "  Network access:    ",
                Style::TEXT_NORMAL,
                &format!("http://{ip}:{port}"),
            ]);
        }
    } else {
        ui::println(&[
            Style::TEXT_INFO_BOLD,
            "  Web interface:    ",
            Style::TEXT_NORMAL,
            &format!("http://{}:{port}", opts.hostname),
        ]);
    }
    ui::println(&[Style::TEXT_DIM, "  (web interface not yet wired)"]);
    Ok(0)
}
