//! npm configuration loading: `.npmrc` files + `NPM_CONFIG_*` / `NPM_TOKEN`
//! environment variables.
//!
//! Mirrors `NpmConfig.load`/`NpmConfig.registry` in
//! reference/packages/core/src/npm-config.ts. The reference uses `@npmcli/config`
//! (which reads the project and user `.npmrc`, then folds in `NPM_CONFIG_*`
//! environment variables); this module implements the same precedence without
//! pulling in a full npm client:
//!
//! 1. project `.npmrc` (the directory passed to [`load`], or the process cwd)
//! 2. user `~/.npmrc`
//! 3. `NPM_CONFIG_REGISTRY`, `NPM_CONFIG_//<host>/:_authToken`,
//!    `NPM_CONFIG__authToken` and `NPM_TOKEN` environment variables.
//!
//! Registry auth tokens are matched against the request URL's origin
//! (`//registry.host/:_authToken=` or the env spelling `NPM_CONFIG_//HOST/:_AUTHTOKEN`),
//! exactly like npm's `nerfDart` scoping.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One scoped auth token: `(origin-with-slashes, token)`. The origin is the
/// `.npmrc` key minus the leading `//` and trailing `:_authToken`, normalized
/// to a lowercase host (for example `registry.npmjs.org/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthToken {
    pub host: String,
    pub token: String,
}

/// The resolved npm configuration for a plugin install.
#[derive(Debug, Clone, Default)]
pub struct NpmConfig {
    /// Base registry URL without a trailing slash (defaults to
    /// `https://registry.npmjs.org`).
    pub registry: String,
    /// Registry tokens keyed by origin. An empty origin (`""`) matches every
    /// request and represents `_authToken=...` / `NPM_CONFIG__authToken`.
    pub auth_tokens: Vec<AuthToken>,
    /// Scoped registries: `@scope` -> base registry URL.
    pub scoped_registries: HashMap<String, String>,
}

impl NpmConfig {
    /// Load the configuration for `dir` (defaults to the process cwd).
    pub fn load(dir: Option<&Path>) -> Self {
        let mut config = Self::default();
        config.registry = "https://registry.npmjs.org".to_string();
        config.apply_env();
        if let Some(dir) = dir {
            config.apply_npmrc(&dir.join(".npmrc"));
        } else if let Ok(cwd) = std::env::current_dir() {
            config.apply_npmrc(&cwd.join(".npmrc"));
        }
        if let Some(home) = home_dir() {
            config.apply_npmrc(&home.join(".npmrc"));
        }
        config
    }

    /// Build a config from a pre-selected registry URL and token, used by the
    /// mock-registry tests.
    pub fn from_registry(registry: impl Into<String>, token: Option<impl Into<String>>) -> Self {
        let mut config = Self::default();
        config.registry = normalize_registry(registry.into());
        if let Some(token) = token {
            config.auth_tokens.push(AuthToken {
                host: "".into(),
                token: token.into(),
            });
        }
        config
    }

    /// The registry base URL for `pkg`, honoring `@scope:registry=...` scopes.
    pub fn registry_for(&self, pkg: &str) -> String {
        if let Some((scope, _)) = pkg.split_once('/') {
            if let Some(registry) = self.scoped_registries.get(scope) {
                return registry.clone();
            }
        }
        self.registry.clone()
    }

    /// Resolve the bearer token to attach to a request for `url`, if any.
    /// npm matches the request origin against `//host/:_authToken` scopes,
    /// with a bare `_authToken`/`NPM_TOKEN` as the fallback.
    pub fn token_for(&self, url: &str) -> Option<String> {
        let host = format!("{}/", extract_host(url).unwrap_or_default());
        let fallback = self.auth_tokens.iter().find(|token| token.host.is_empty());
        self.auth_tokens
            .iter()
            .find(|token| !token.host.is_empty() && host.starts_with(&token.host))
            .or(fallback)
            .map(|token| token.token.clone())
    }

    /// Whether a request for `url` must carry an auth token at all (npm's
    /// `always-auth` flag; tokens are attached whenever one is configured).
    pub fn has_token_for(&self, url: &str) -> bool {
        self.token_for(url).is_some()
    }

    fn apply_env(&mut self) {
        for (key, value) in std::env::vars() {
            let key = key.to_ascii_lowercase();
            match key.as_str() {
                "npm_config_registry" => {
                    if !value.trim().is_empty() {
                        self.registry = normalize_registry(value);
                    }
                }
                "npm_config__authtoken" => {
                    if !value.is_empty() {
                        self.auth_tokens.push(AuthToken {
                            host: "".into(),
                            token: value,
                        });
                    }
                }
                "npm_token" => {
                    // NPM_TOKEN is the CI convention for the default registry.
                    if !value.is_empty() && !self.has_token_for_registry(&self.registry) {
                        self.auth_tokens.push(AuthToken {
                            host: "".into(),
                            token: value,
                        });
                    }
                }
                _ => {
                    // NPM_CONFIG_//HOST/:_AUTHTOKEN (npm lower-cases the key).
                    if let Some(rest) = key.strip_prefix("npm_config_") {
                        if let Some(host) = rest.strip_prefix("//") {
                            if let Some((host, _)) = host.split_once("/:_authtoken") {
                                if !value.is_empty() {
                                    self.auth_tokens.push(AuthToken {
                                        host: format!("{host}/"),
                                        token: value,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn apply_npmrc(&mut self, path: &Path) {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return;
        };
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            if key == "registry" {
                if !value.is_empty() {
                    self.registry = normalize_registry(value.to_string());
                }
            } else if let Some(scope) = key.strip_suffix(":registry") {
                if !value.is_empty() {
                    self.scoped_registries
                        .insert(scope.to_string(), normalize_registry(value.to_string()));
                }
            } else if key == "_authToken" || key == "_auth" {
                if !value.is_empty() {
                    self.auth_tokens.push(AuthToken {
                        host: "".into(),
                        token: value.to_string(),
                    });
                }
            } else if let Some(host) = key.strip_prefix("//") {
                if let Some((host, _)) = host.split_once("/:_authToken") {
                    if !value.is_empty() {
                        self.auth_tokens.push(AuthToken {
                            host: format!("{host}/"),
                            token: value.to_string(),
                        });
                    }
                }
            }
        }
    }

    fn has_token_for_registry(&self, registry: &str) -> bool {
        self.auth_tokens.iter().any(|token| {
            if token.host.is_empty() {
                return true;
            }
            let host = format!("{}/", extract_host(registry).unwrap_or_default());
            host.starts_with(&token.host)
        })
    }
}

/// Extract the lowercase host from a URL string without pulling in a URL
/// parser dependency (the inputs here are always registry/tarball URLs).
fn extract_host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

fn normalize_registry(mut registry: String) -> String {
    if registry.ends_with('/') {
        registry.pop();
    }
    registry
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npmrc_file(name: &str, contents: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-npmrc-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".npmrc");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn reads_registry_and_scopes_from_project_npmrc() {
        let path = npmrc_file(
            "reg",
            "registry=https://registry.example.test/\n@acme:registry=https://npm.acme.test/\n",
        );
        let config = NpmConfig::load(Some(path.parent().unwrap()));
        assert_eq!(config.registry, "https://registry.example.test");
        assert_eq!(config.registry_for("@acme/pkg"), "https://npm.acme.test");
        assert_eq!(
            config.registry_for("plain"),
            "https://registry.example.test"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn reads_auth_token_for_host() {
        let path = npmrc_file(
            "token",
            "//registry.example.test/:_authToken=secret\nregistry=https://registry.example.test/\n",
        );
        let config = NpmConfig::load(Some(path.parent().unwrap()));
        assert_eq!(
            config
                .token_for("https://registry.example.test/pkg")
                .as_deref(),
            Some("secret")
        );
        // A different host does not inherit the token.
        assert_eq!(config.token_for("https://other.test/pkg"), None);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn falls_back_to_bare_authtoken() {
        let path = npmrc_file("bare", "_authToken=fallback\n");
        let config = NpmConfig::load(Some(path.parent().unwrap()));
        assert_eq!(
            config
                .token_for("https://registry.npmjs.org/pkg")
                .as_deref(),
            Some("fallback")
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn from_registry_builds_test_config() {
        let config = NpmConfig::from_registry("http://127.0.0.1:9999/", Some("tok"));
        assert_eq!(config.registry, "http://127.0.0.1:9999");
        assert_eq!(
            config.token_for("http://127.0.0.1:9999/foo").as_deref(),
            Some("tok")
        );
    }
}
