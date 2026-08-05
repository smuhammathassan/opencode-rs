//! Network (server listen) options shared by `serve`, `web`, `acp` and the TUI.
//! From reference/packages/opencode/src/cli/network.ts.

/// The `server` section of the resolved config, as consumed by
/// `resolveNetworkOptionsNoConfig`.
/// TODO(integration): reuse `oc_config`'s resolved server config instead of this
/// local mirror.
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    pub port: Option<u16>,
    pub hostname: Option<String>,
    pub mdns: Option<bool>,
    pub mdns_domain: Option<String>,
    pub cors: Vec<String>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct NetworkArgs {
    /// port to listen on
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// hostname to listen on
    #[arg(long, default_value = "127.0.0.1")]
    pub hostname: String,
    /// enable mDNS service discovery (defaults hostname to 0.0.0.0)
    #[arg(long, default_value_t = false)]
    pub mdns: bool,
    /// custom domain name for mDNS service (default: opencode.local)
    #[arg(long, default_value = "opencode.local")]
    pub mdns_domain: String,
    /// additional domains to allow for CORS
    #[arg(long, num_args = 0..)]
    pub cors: Vec<String>,
}

impl Default for NetworkArgs {
    fn default() -> Self {
        NetworkArgs {
            port: 0,
            hostname: "127.0.0.1".to_string(),
            mdns: false,
            mdns_domain: "opencode.local".to_string(),
            cors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedNetwork {
    pub hostname: String,
    pub port: u16,
    pub mdns: bool,
    pub mdns_domain: String,
    pub cors: Vec<String>,
}

/// Whether a CLI flag (e.g. `--port` or `--port=1234`) appears in the process
/// args before a `--` separator. Mirrors `hasArg(name)` in network.ts.
pub fn has_arg(name: &str) -> bool {
    args_before_dashes()
        .iter()
        .any(|arg| arg == name || arg.strip_prefix(name).map_or(false, |s| s.starts_with('=')))
}

/// Mirrors `hasBooleanArg(name)`.
pub fn has_boolean_arg(name: &str) -> bool {
    args_before_dashes().iter().any(|arg| {
        arg == name || arg == &format!("{name}=true") || arg == &format!("{name}=false") || {
            let no = arg.strip_prefix("--no-").unwrap_or("");
            !no.is_empty() && format!("--{no}") == name
        }
    })
}

fn args_before_dashes() -> Vec<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let separator = args.iter().position(|a| a == "--");
    args[..separator.unwrap_or(args.len())].to_vec()
}

/// Mirrors `resolveNetworkOptionsNoConfig(args, config)`.
pub fn resolve_network_options(
    args: &NetworkArgs,
    config: Option<&ServerConfig>,
) -> ResolvedNetwork {
    let config = config.cloned().unwrap_or_default();
    let port_explicit = has_arg("--port");
    let hostname_explicit = has_arg("--hostname");
    let mdns_explicit = has_boolean_arg("--mdns");
    let mdns_domain_explicit = has_arg("--mdns-domain");

    let mdns = if mdns_explicit {
        args.mdns
    } else {
        config.mdns.unwrap_or(args.mdns)
    };
    let mdns_domain = if mdns_domain_explicit {
        args.mdns_domain.clone()
    } else {
        config
            .mdns_domain
            .clone()
            .unwrap_or_else(|| args.mdns_domain.clone())
    };
    let port = if port_explicit {
        args.port
    } else {
        config.port.unwrap_or(args.port)
    };
    let hostname = if hostname_explicit {
        args.hostname.clone()
    } else if mdns && config.hostname.is_none() {
        "0.0.0.0".to_string()
    } else {
        config
            .hostname
            .clone()
            .unwrap_or_else(|| args.hostname.clone())
    };
    let mut cors = config.cors.clone();
    cors.extend(args.cors.iter().cloned());

    ResolvedNetwork {
        hostname,
        port,
        mdns,
        mdns_domain,
        cors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_defaults_without_config() {
        let args = NetworkArgs::default();
        let resolved = resolve_network_options(&args, None);
        assert_eq!(resolved.port, 0);
        assert_eq!(resolved.hostname, "127.0.0.1");
        assert!(!resolved.mdns);
        assert_eq!(resolved.mdns_domain, "opencode.local");
    }

    #[test]
    fn mdns_defaults_hostname_to_0_0_0_0() {
        let args = NetworkArgs {
            mdns: true,
            ..Default::default()
        };
        let resolved = resolve_network_options(&args, None);
        assert_eq!(resolved.hostname, "0.0.0.0");
    }

    #[test]
    fn config_values_win_over_defaults() {
        let args = NetworkArgs::default();
        let config = ServerConfig {
            port: Some(4096),
            hostname: Some("0.0.0.0".into()),
            cors: vec!["https://example.com".into()],
            ..Default::default()
        };
        let resolved = resolve_network_options(&args, Some(&config));
        assert_eq!(resolved.port, 4096);
        assert_eq!(resolved.hostname, "0.0.0.0");
        assert_eq!(resolved.cors, vec!["https://example.com"]);
    }
}
