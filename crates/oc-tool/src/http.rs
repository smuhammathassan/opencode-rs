//! Shared HTTP client for the network tools.

use std::sync::OnceLock;

/// A process-wide `reqwest` client (mirrors the Effect `HttpClient` service).
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("opencode")
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client")
    })
}
