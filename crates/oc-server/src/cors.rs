//! CORS policy. From reference/packages/server/src/cors.ts.

use std::sync::Arc;

use regex::Regex;

/// `^https://([a-z0-9-]+\.)*opencode\.ai$`
static OPENCODE_ORIGIN: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"^https://([a-z0-9-]+\.)*opencode\.ai$").unwrap());

/// Server CORS options. From reference/packages/server/src/cors.ts (`CorsOptions`).
#[derive(Debug, Clone, Default)]
pub struct CorsOptions {
    pub cors: Option<Vec<String>>,
}

/// Whether an origin may reach the API. Empty/missing origins are allowed.
/// From reference/packages/server/src/cors.ts (`isAllowedCorsOrigin`).
pub fn is_allowed_cors_origin(input: Option<&str>, opts: Option<&CorsOptions>) -> bool {
    let Some(input) = input else { return true };
    if input.starts_with("http://localhost:") {
        return true;
    }
    if input.starts_with("http://127.0.0.1:") {
        return true;
    }
    if input.starts_with("oc://renderer") {
        return true;
    }
    if input == "tauri://localhost"
        || input == "http://tauri.localhost"
        || input == "https://tauri.localhost"
    {
        return true;
    }
    if OPENCODE_ORIGIN.is_match(input) {
        return true;
    }
    opts.and_then(|o| o.cors.as_ref())
        .map_or(false, |cors| cors.iter().any(|o| o == input))
}

/// Origin check that also accepts same-host requests. From
/// reference/packages/server/src/cors.ts (`isAllowedRequestOrigin`).
pub fn is_allowed_request_origin(
    input: Option<&str>,
    host: Option<&str>,
    opts: Option<&CorsOptions>,
) -> bool {
    let Some(input) = input else { return true };
    if let Some(host) = host {
        if same_host(input, host) {
            return true;
        }
    }
    is_allowed_cors_origin(Some(input), opts)
}

fn same_host(origin: &str, host: &str) -> bool {
    // Node's `URL.host` includes the port; the `Host` request header matches that.
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let Some(name) = url.host_str() else {
        return false;
    };
    match url.port() {
        Some(port) => format!("{name}:{port}") == host,
        None => name == host,
    }
}

/// `Arc` alias kept so handlers can hold the config next to other shared state.
pub type SharedCorsOptions = Arc<CorsOptions>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_opencode_origin() {
        assert!(is_allowed_cors_origin(
            Some("https://app.opencode.ai"),
            None
        ));
        assert!(is_allowed_cors_origin(
            Some("https://sub.opencode.ai"),
            None
        ));
        assert!(!is_allowed_cors_origin(
            Some("https://evil.opencode.ai.com"),
            None
        ));
        assert!(!is_allowed_cors_origin(Some("https://example.com"), None));
    }

    #[test]
    fn allows_loopback_and_native_schemes() {
        assert!(is_allowed_cors_origin(Some("http://localhost:5173"), None));
        assert!(is_allowed_cors_origin(Some("http://127.0.0.1:3000"), None));
        assert!(is_allowed_cors_origin(Some("oc://renderer"), None));
        assert!(is_allowed_cors_origin(Some("tauri://localhost"), None));
    }

    #[test]
    fn allows_configured_origins() {
        let opts = CorsOptions {
            cors: Some(vec!["https://example.com".into()]),
        };
        assert!(is_allowed_cors_origin(
            Some("https://example.com"),
            Some(&opts)
        ));
        assert!(!is_allowed_cors_origin(
            Some("https://not-configured.com"),
            Some(&opts)
        ));
    }

    #[test]
    fn allows_same_host() {
        assert!(is_allowed_request_origin(
            Some("http://localhost:4096"),
            Some("localhost:4096"),
            None
        ));
        assert!(is_allowed_request_origin(
            Some("http://127.0.0.1:4096"),
            Some("127.0.0.1:4096"),
            None
        ));
    }
}
