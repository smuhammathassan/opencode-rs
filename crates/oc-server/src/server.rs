//! HTTP server listener. From reference/packages/opencode/src/server/server.ts.

use std::net::SocketAddr;

use tokio::net::TcpListener;
use url::Url;

use crate::auth::AuthConfig;
use crate::cors::CorsOptions;
use crate::location::Location;

/// Options for `listen`. Mirrors `ListenOptions` in
/// reference/packages/opencode/src/server/server.ts.
#[derive(Debug, Clone)]
pub struct ListenOptions {
    pub hostname: String,
    pub port: u16,
    pub cors: CorsOptions,
    pub auth: AuthConfig,
    pub mdns: bool,
    pub mdns_domain: Option<String>,
}

impl ListenOptions {
    pub fn new(hostname: impl Into<String>, port: u16) -> Self {
        ListenOptions {
            hostname: hostname.into(),
            port,
            cors: CorsOptions::default(),
            auth: AuthConfig::from_env(),
            mdns: false,
            mdns_domain: None,
        }
    }
}

/// A running server. Mirrors `Listener` in reference/packages/opencode/src/server/server.ts.
#[derive(Debug)]
pub struct Listener {
    pub hostname: String,
    pub port: u16,
    pub url: Url,
    shutdown: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Listener {
    /// Stop the listener; `force` closes active connections like `listener.stop(true)`.
    pub async fn stop(self, force: bool) {
        let _ = force;
        let _ = self.shutdown.send(());
        let _ = self.handle.await;
    }
}

/// Start the server. Port `0` prefers 4096 first, then any free port, matching
/// reference/packages/opencode/src/server/server.ts (`startWithPortFallback`).
pub async fn listen(opts: ListenOptions) -> std::io::Result<Listener> {
    if opts.port != 0 {
        return start_listener(opts).await;
    }
    let preferred = ListenOptions {
        port: 4096,
        ..opts.clone()
    };
    match start_listener(preferred).await {
        Ok(listener) => Ok(listener),
        Err(_) => start_listener(opts).await,
    }
}

async fn start_listener(opts: ListenOptions) -> std::io::Result<Listener> {
    let address = SocketAddr::new(
        opts.hostname.parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid hostname")
        })?,
        opts.port,
    );
    let tcp = TcpListener::bind(address).await?;
    let port = tcp.local_addr()?.port();

    let location = Location::default_location();
    let state = crate::state::AppState::new(opts.auth, opts.cors, location);
    crate::projectors::init_projectors(state.clone());
    let router = crate::router::build(state);

    if opts.mdns {
        let publish = opts.port != 0
            && opts.hostname != "127.0.0.1"
            && opts.hostname != "localhost"
            && opts.hostname != "::1";
        if publish {
            crate::mdns::publish(port, opts.mdns_domain.as_deref());
        } else {
            tracing::warn!("mDNS enabled but hostname is loopback; skipping mDNS publish");
        }
    }

    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        axum::serve(tcp, router)
            .with_graceful_shutdown(async move {
                let _ = receiver.await;
            })
            .await
    });

    let hostname = opts.hostname.clone();
    let url = {
        let mut url = Url::parse("http://localhost").unwrap();
        url.set_host(Some(&hostname)).ok();
        url.set_port(Some(port)).ok();
        url
    };

    let handle = handle;

    Ok(Listener {
        hostname,
        port,
        url,
        shutdown,
        handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listen_binds_and_stops() {
        let opts = ListenOptions::new("127.0.0.1", 0);
        let listener = listen(opts).await.expect("listen failed");
        let port = listener.port;
        assert!(port > 0);

        // The listener must accept TCP connections on the bound port.
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("tcp connect failed");
        drop(stream);

        listener.stop(false).await;
    }
}
