/// From reference/packages/opencode/src/util/proxy-env.ts
///
/// Adapted from proxy-from-env (MIT). Computes the proxy URL to use for a
/// given target URL based on `{protocol}_proxy` / `all_proxy` / `no_proxy`
/// environment variables.
use std::collections::HashMap;

const DEFAULT_PORTS: &[(&str, u16)] = &[
    ("ftp", 21),
    ("gopher", 70),
    ("http", 80),
    ("https", 443),
    ("ws", 80),
    ("wss", 443),
];

pub fn get_proxy_for_url(input: &str) -> Option<String> {
    get_proxy_for_url_with_env(input, &std::env::vars().collect())
}

/// Test-friendly variant that takes an explicit environment map instead of
/// reading the process environment.
pub fn get_proxy_for_url_with_env(input: &str, vars: &HashMap<String, String>) -> Option<String> {
    let url = url::Url::parse(input).ok()?;
    let protocol = url.scheme().to_string();
    let hostname = url.host_str()?.to_string();
    let port = url.port().or_else(|| default_port(&protocol)).unwrap_or(0);
    if !should_proxy(&hostname, port, vars) {
        return None;
    }
    let proxy = env(vars, &format!("{protocol}_proxy")).or_else(|| env(vars, "all_proxy"))?;
    if proxy.is_empty() {
        return None;
    }
    if proxy.contains("://") {
        Some(proxy)
    } else {
        Some(format!("{protocol}://{proxy}"))
    }
}

fn default_port(protocol: &str) -> Option<u16> {
    DEFAULT_PORTS
        .iter()
        .find(|(p, _)| *p == protocol)
        .map(|(_, port)| *port)
}

fn env(vars: &HashMap<String, String>, key: &str) -> Option<String> {
    vars.get(&key.to_lowercase())
        .or_else(|| vars.get(&key.to_uppercase()))
        .cloned()
        .filter(|value| !value.is_empty())
}

fn should_proxy(hostname: &str, port: u16, vars: &HashMap<String, String>) -> bool {
    let no_proxy = env(vars, "no_proxy").unwrap_or_default().to_lowercase();
    if no_proxy.is_empty() {
        return true;
    }
    if no_proxy == "*" {
        return false;
    }
    no_proxy.split([',', ' ', '\t']).all(|raw| {
        let proxy = raw.trim();
        if proxy.is_empty() {
            return true;
        }
        let (proxy_hostname, proxy_port) = match proxy.split_once(':') {
            Some((host, port_str))
                if !port_str.is_empty() && port_str.chars().all(|c| c.is_ascii_digit()) =>
            {
                (host, port_str.parse::<u16>().unwrap_or(0))
            }
            _ => (proxy, 0),
        };
        if proxy_port != 0 && proxy_port != port {
            return true;
        }
        if !proxy_hostname.starts_with('.') && !proxy_hostname.starts_with('*') {
            hostname != proxy_hostname
        } else {
            let suffix = proxy_hostname.strip_prefix('*').unwrap_or(proxy_hostname);
            !hostname.ends_with(suffix)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn invalid_url_returns_none() {
        assert_eq!(get_proxy_for_url_with_env("not a url", &vars(&[])), None);
    }

    #[test]
    fn uses_protocol_specific_proxy() {
        assert_eq!(
            get_proxy_for_url_with_env(
                "http://example.com",
                &vars(&[("http_proxy", "http://localhost:8080")])
            ),
            Some("http://localhost:8080".to_string())
        );
    }

    #[test]
    fn prepends_protocol_when_missing() {
        assert_eq!(
            get_proxy_for_url_with_env(
                "https://example.com",
                &vars(&[("https_proxy", "proxy.example.com:8443")])
            ),
            Some("https://proxy.example.com:8443".to_string())
        );
    }

    #[test]
    fn all_proxy_fallback() {
        assert_eq!(
            get_proxy_for_url_with_env(
                "http://example.com",
                &vars(&[("all_proxy", "http://squid:3128")])
            ),
            Some("http://squid:3128".to_string())
        );
        // protocol-specific wins
        assert_eq!(
            get_proxy_for_url_with_env(
                "http://example.com",
                &vars(&[("http_proxy", "http://h:1"), ("all_proxy", "http://a:2")])
            ),
            Some("http://h:1".to_string())
        );
    }

    #[test]
    fn no_proxy_blocks() {
        let proxied = vars(&[
            ("http_proxy", "http://localhost:8080"),
            ("no_proxy", "example.com"),
        ]);
        assert_eq!(
            get_proxy_for_url_with_env("http://example.com", &proxied),
            None
        );
        assert_eq!(
            get_proxy_for_url_with_env("http://other.com", &proxied),
            Some("http://localhost:8080".to_string())
        );
    }

    #[test]
    fn no_proxy_star_blocks_everything() {
        let proxied = vars(&[("http_proxy", "http://localhost:8080"), ("no_proxy", "*")]);
        assert_eq!(
            get_proxy_for_url_with_env("http://example.com", &proxied),
            None
        );
    }

    #[test]
    fn no_proxy_uppercase_and_suffix_matching() {
        let proxied = vars(&[
            ("http_proxy", "http://localhost:8080"),
            ("NO_PROXY", ".example.com"),
        ]);
        assert_eq!(
            get_proxy_for_url_with_env("http://sub.example.com", &proxied),
            None
        );
        assert_eq!(
            get_proxy_for_url_with_env("http://sub.example.org", &proxied),
            Some("http://localhost:8080".to_string())
        );
        assert!(!should_proxy(
            "example.com",
            80,
            &vars(&[("no_proxy", "example.com")])
        ));
        assert!(should_proxy(
            "sub.example.com",
            80,
            &vars(&[("no_proxy", "example.com")])
        ));
        assert!(!should_proxy(
            "sub.example.com",
            80,
            &vars(&[("no_proxy", ".example.com")])
        ));
        assert!(!should_proxy(
            "sub.example.com",
            80,
            &vars(&[("no_proxy", "*.example.com")])
        ));
        assert!(should_proxy(
            "sub.example.org",
            80,
            &vars(&[("no_proxy", ".example.com")])
        ));
        assert!(should_proxy(
            "example.com",
            8080,
            &vars(&[("no_proxy", "example.com:80")])
        ));
    }

    #[test]
    fn default_ports_used_when_absent() {
        let proxied = vars(&[("http_proxy", "http://localhost:8080")]);
        // no explicit port -> default 80; a no_proxy entry on 8080 does not apply
        assert_eq!(
            get_proxy_for_url_with_env("http://example.com", &proxied),
            Some("http://localhost:8080".to_string())
        );
    }
}
