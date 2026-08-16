//! `opencode web`
//! From reference/packages/opencode/src/cli/cmd/web.ts.

use anyhow::Context as _;

use crate::cli::args::{Cli, WebArgs};
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

    let opts = resolve_network_options(&args.network, None);
    let mut options = oc_server::server::ListenOptions::new(&opts.hostname, opts.port);
    options.auth = oc_server::auth::AuthConfig::from_env();
    options.cors = oc_server::cors::CorsOptions {
        cors: (!opts.cors.is_empty()).then_some(opts.cors.clone()),
    };
    options.mdns = opts.mdns;
    options.mdns_domain = Some(opts.mdns_domain.clone());
    let listener = oc_server::server::listen(options)
        .await
        .with_context(|| format!("failed to listen on {}:{}", opts.hostname, opts.port))?;
    let port = listener.port;
    let web_url = browser_url(&opts.hostname, port);

    ui::empty();
    ui::println(&[&ui::logo(Some("  "))]);
    ui::empty();

    if opts.hostname == "0.0.0.0" {
        ui::println(&[
            Style::TEXT_INFO_BOLD,
            "  Local access:      ",
            Style::TEXT_NORMAL,
            &web_url,
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
            &web_url,
        ]);
    }
    if open_browser(&web_url).await {
        ui::println(&[
            Style::TEXT_DIM,
            "  Opened the embedded web interface in your browser.",
        ]);
    } else {
        ui::println(&[
            Style::TEXT_DIM,
            "  Open the URL above in a browser to use the embedded web interface.",
        ]);
    }

    // Keep the process alive until the shared lifecycle signal fires, then
    // let the listener drain before returning from command dispatch.
    oc_util::util::signal::process_shutdown().wait().await;
    listener.stop(false).await;
    Ok(0)
}

fn browser_url(hostname: &str, port: u16) -> String {
    let host = match hostname {
        "0.0.0.0" | "::" => "localhost",
        other => other,
    };
    if host.contains(':') && !host.starts_with('[') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

async fn open_browser(url: &str) -> bool {
    let status = if let Ok(browser) = std::env::var("BROWSER") {
        tokio::process::Command::new(browser)
            .arg(url)
            .status()
            .await
    } else if cfg!(target_os = "macos") {
        tokio::process::Command::new("open").arg(url).status().await
    } else if cfg!(target_os = "windows") {
        tokio::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .await
    } else {
        tokio::process::Command::new("xdg-open")
            .arg(url)
            .status()
            .await
    };
    status.map(|status| status.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::browser_url;

    #[test]
    fn browser_url_uses_localhost_for_wildcard_bind() {
        assert_eq!(browser_url("0.0.0.0", 4096), "http://localhost:4096");
        assert_eq!(browser_url("::", 4096), "http://localhost:4096");
        assert_eq!(browser_url("127.0.0.1", 4096), "http://127.0.0.1:4096");
        assert_eq!(browser_url("::1", 4096), "http://[::1]:4096");
    }
}
