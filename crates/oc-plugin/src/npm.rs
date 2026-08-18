//! npm package fetching for plugins.
//!
//! The reference installs plugins with `@npmcli/arborist` (a full npm client)
//! into `<cache>/packages/<pkg>/node_modules`. There is no JS toolchain in the
//! environment, so this module fetches the registry metadata and tarball
//! directly, resolves the package's runtime `dependencies` from the same
//! registry (version-pinned), and unpacks the tree into the same layout.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::loader::sanitize_package;
use crate::npm_config::NpmConfig;
use crate::paths::GlobalPaths;

#[derive(Debug, thiserror::Error)]
pub enum NpmError {
    #[error("failed to fetch registry metadata for {pkg}: {message}")]
    Metadata { pkg: String, message: String },
    #[error("no version {version} found for package {pkg}")]
    VersionNotFound { pkg: String, version: String },
    #[error("failed to download tarball for {pkg}: {message}")]
    Tarball { pkg: String, message: String },
    #[error("failed to unpack tarball for {pkg}: {message}")]
    Unpack { pkg: String, message: String },
    #[error("registry returned no tarball for {pkg}@{version}")]
    NoTarball { pkg: String, version: String },
    #[error("invalid package metadata for {pkg}: {message}")]
    InvalidPackage { pkg: String, message: String },
}

/// The name of the small lock marker written next to a package once its
/// runtime dependencies are installed. Mirrors npm's `package-lock.json`
/// purpose (version-pinned dependency pins) without requiring a full lockfile
/// format.
pub const DEPS_MARKER: &str = ".oc-deps.json";

fn registry_url(pkg: &str, config: &NpmConfig) -> String {
    let registry = config.registry_for(pkg);
    if pkg.starts_with('@') {
        // scoped: @scope/name → encode the slash
        let (scope, name) = pkg.split_once('/').unwrap_or((pkg, ""));
        format!("{registry}/{}/{}", scope.replace('@', "%40"), name)
    } else {
        format!("{registry}/{pkg}")
    }
}

/// A `reqwest::blocking` client with sensible plugin-install timeouts.
fn install_client() -> Result<reqwest::blocking::Client, NpmError> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| NpmError::Metadata {
            pkg: "client".to_string(),
            message: format!("failed to build registry client: {e}"),
        })
}

fn fetch_json(
    client: &reqwest::blocking::Client,
    url: &str,
    config: &NpmConfig,
    pkg: &str,
) -> Result<Value, NpmError> {
    let mut request = client.get(url);
    if let Some(token) = config.token_for(url) {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request
        .send()
        .and_then(|res| res.error_for_status())
        .and_then(|res| res.json::<Value>())
        .map_err(|e| NpmError::Metadata {
            pkg: pkg.to_string(),
            message: format!("{url}: {e}"),
        })
}

fn fetch_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    config: &NpmConfig,
    pkg: &str,
) -> Result<Vec<u8>, NpmError> {
    let mut request = client.get(url);
    if let Some(token) = config.token_for(url) {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request
        .send()
        .and_then(|res| res.error_for_status())
        .and_then(|res| res.bytes())
        .map(|bytes| bytes.to_vec())
        .map_err(|e| NpmError::Tarball {
            pkg: pkg.to_string(),
            message: format!("{url}: {e}"),
        })
}

fn version_tarball(metadata: &Value, version: &str) -> Option<(String, String)> {
    let versions = metadata.get("versions")?.as_object()?;
    let hit = versions.get(version)?;
    let tarball = hit.get("dist")?.get("tarball")?.as_str()?.to_string();
    Some((version.to_string(), tarball))
}

/// The resolved package version for a specifier. `version` may be `latest`, an
/// exact semver, or a range (simple prefix matching).
fn pick_version(metadata: &Value, requested: &str) -> Option<String> {
    if requested.is_empty() || requested == "latest" {
        return metadata
            .get("dist-tags")?
            .get("latest")?
            .as_str()
            .map(str::to_string);
    }
    let versions = metadata.get("versions")?.as_object()?;
    if versions.contains_key(requested) {
        return Some(requested.to_string());
    }
    // Use npm-compatible semver matching before the legacy prefix fallback.
    // The fallback keeps support for the abbreviated ranges commonly found in
    // hand-written plugin specs (for example, `1.2`).
    let requirement = semver::VersionReq::parse(requested).ok();
    let mut best: Option<(semver::Version, String)> = None;
    for version in versions.keys() {
        let Ok(parsed) = semver::Version::parse(version) else {
            continue;
        };
        if requirement.as_ref().is_some_and(|req| req.matches(&parsed)) {
            let replace = best
                .as_ref()
                .map(|(current, _)| parsed > *current)
                .unwrap_or(true);
            if replace {
                best = Some((parsed, version.clone()));
            }
        }
    }
    if let Some((_, version)) = best {
        return Some(version);
    }

    let base = requested
        .strip_prefix('^')
        .or_else(|| requested.strip_prefix('~'))
        .or_else(|| requested.strip_prefix('='))
        .unwrap_or(requested);
    versions
        .keys()
        .filter(|version| version.starts_with(base))
        .max()
        .cloned()
}

/// Extract a git URL from an npm-style git spec: `pkg@git+<url>`,
/// `pkg@github:user/repo`, or a bare `git+<url>`.
fn git_url(spec: &str) -> Option<String> {
    if let Some((_name, rest)) = spec.split_once('@') {
        if let Some(url) = rest.strip_prefix("git+") {
            return Some(url.to_string());
        }
        if let Some(url) = rest.strip_prefix("github:") {
            return Some(format!("https://github.com/{url}.git"));
        }
    }
    if let Some(url) = spec.strip_prefix("git+") {
        return Some(url.to_string());
    }
    None
}

/// Add a plugin package to the cache, returning the resolved plugin directory.
/// Mirrors `Npm.add` in reference/packages/core/src/npm.ts.
pub fn add(spec: &str, paths: &GlobalPaths) -> Result<PathBuf, NpmError> {
    let config = NpmConfig::load(None);
    add_with_config(spec, paths, &config)
}

/// [`add`] with an explicit npm configuration (registry base + auth tokens),
/// used by the mock-registry tests and any embedding application that manages
/// its own npm configuration.
pub fn add_with_config(
    spec: &str,
    paths: &GlobalPaths,
    config: &NpmConfig,
) -> Result<PathBuf, NpmError> {
    let (parsed_pkg, requested) = crate::loader::parse_plugin_specifier(spec);
    let git = git_url(spec);
    let pkg = if valid_package_name(&parsed_pkg) {
        parsed_pkg
    } else if let Some(url) = git.as_deref() {
        git_package_name(url).ok_or_else(|| NpmError::InvalidPackage {
            pkg: parsed_pkg.clone(),
            message: "could not derive a package name from git URL".to_string(),
        })?
    } else {
        return Err(NpmError::InvalidPackage {
            pkg: parsed_pkg,
            message: "invalid npm package name".to_string(),
        });
    };
    let dir = cache_dir(paths, &pkg, &requested);
    let target = dir.join("node_modules").join(pkg_name_dir(&pkg));
    let cached = if git.is_some() {
        validate_package(&target, None, None).is_ok()
    } else {
        cached_package_is_valid(&target, &pkg, &requested)
    };
    if cached {
        return Ok(target);
    }
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| NpmError::Unpack {
            pkg: pkg.clone(),
            message: format!("remove invalid cached package: {e}"),
        })?;
    }

    // Git specs (`pkg@git+...`) are cloned instead of fetched from the registry.
    if let Some(url) = git {
        return add_git(&url, &target, &pkg);
    }

    let client = install_client()?;
    let metadata_url = registry_url(&pkg, config);
    let metadata = fetch_json(&client, &metadata_url, config, &pkg)?;

    let version = pick_version(&metadata, &requested).ok_or_else(|| NpmError::VersionNotFound {
        pkg: pkg.clone(),
        version: requested.clone(),
    })?;
    let (_, tarball) = version_tarball(&metadata, &version).ok_or_else(|| NpmError::NoTarball {
        pkg: pkg.clone(),
        version: version.clone(),
    })?;

    let bytes = fetch_bytes(&client, &tarball, config, &pkg)?;

    std::fs::create_dir_all(&dir).map_err(|e| NpmError::Unpack {
        pkg: pkg.clone(),
        message: e.to_string(),
    })?;
    unpack_tarball(&bytes, &target, &pkg)?;
    validate_package(&target, Some(&pkg), Some(&version))?;
    install_dependencies(&client, &target, config, &pkg)?;

    Ok(target)
}

/// Install the runtime `dependencies` of the package at `target`, resolving
/// each from the registry, downloading version-pinned tarballs into the
/// package's `node_modules`, and recursing into nested dependencies. The
/// resolved pins are recorded in a small `.oc-deps.json` marker.
fn install_dependencies(
    client: &reqwest::blocking::Client,
    target: &Path,
    config: &NpmConfig,
    pkg: &str,
) -> Result<(), NpmError> {
    let manifest = read_package_json(target)?;
    let mut visited = HashSet::new();
    let mut resolved = BTreeMap::new();
    resolve_and_install_deps(
        client,
        config,
        &manifest,
        target,
        &mut visited,
        &mut resolved,
    )?;
    let marker = target.join(DEPS_MARKER);
    let contents = serde_json::to_string_pretty(&serde_json::json!({
        "dependencies": resolved,
    }))
    .map_err(|e| NpmError::InvalidPackage {
        pkg: pkg.to_string(),
        message: format!("serialize dependency marker: {e}"),
    })?;
    std::fs::write(&marker, format!("{contents}\n")).map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: format!("write dependency marker {}: {e}", marker.display()),
    })
}

fn resolve_and_install_deps(
    client: &reqwest::blocking::Client,
    config: &NpmConfig,
    manifest: &Value,
    target: &Path,
    visited: &mut HashSet<String>,
    resolved: &mut BTreeMap<String, String>,
) -> Result<(), NpmError> {
    let Some(dependencies) = manifest.get("dependencies").and_then(Value::as_object) else {
        return Ok(());
    };
    for (dep_name, spec) in dependencies {
        // Guard against dependency cycles (npm allows `a` -> `b` -> `a`).
        if !visited.insert(dep_name.clone()) {
            continue;
        }
        let Some(range) = spec.as_str() else {
            continue; // object specs (npm aliases) are not resolvable here
        };
        if !is_registry_spec(range) {
            continue; // git/file/workspace/link/http specs are out of scope
        }
        let dep_version = resolve_dependency_version(client, config, dep_name, range)?;
        let dep_dir = target.join("node_modules").join(pkg_name_dir(dep_name));
        if !cached_package_is_valid(&dep_dir, dep_name, &dep_version) {
            if dep_dir.exists() {
                std::fs::remove_dir_all(&dep_dir).map_err(|e| NpmError::Unpack {
                    pkg: dep_name.clone(),
                    message: format!("remove stale dependency: {e}"),
                })?;
            }
            let metadata_url = registry_url(dep_name, config);
            let metadata = fetch_json(client, &metadata_url, config, dep_name)?;
            let (_, tarball) =
                version_tarball(&metadata, &dep_version).ok_or_else(|| NpmError::NoTarball {
                    pkg: dep_name.clone(),
                    version: dep_version.clone(),
                })?;
            let bytes = fetch_bytes(client, &tarball, config, dep_name)?;
            unpack_tarball(&bytes, &dep_dir, dep_name)?;
            validate_package(&dep_dir, Some(dep_name), Some(&dep_version))?;
        }
        resolved.insert(dep_name.clone(), dep_version);
        // Recursively resolve the dependency's own runtime dependencies.
        let nested = read_package_json(&dep_dir)?;
        resolve_and_install_deps(client, config, &nested, &dep_dir, visited, resolved)?;
    }
    Ok(())
}

/// Resolve the exact, version-pinned version for a dependency range without
/// downloading it (used both for the download step and the cache check).
fn resolve_dependency_version(
    client: &reqwest::blocking::Client,
    config: &NpmConfig,
    dep_name: &str,
    range: &str,
) -> Result<String, NpmError> {
    let metadata_url = registry_url(dep_name, config);
    let metadata = fetch_json(client, &metadata_url, config, dep_name)?;
    pick_version(&metadata, range).ok_or_else(|| NpmError::VersionNotFound {
        pkg: dep_name.to_string(),
        version: range.to_string(),
    })
}

/// Whether `spec` is a registry-resolvable version range. Git, file,
/// workspace, link, alias (`npm:`) and http(s) specs are not resolvable
/// through the metadata endpoint.
fn is_registry_spec(spec: &str) -> bool {
    !(spec.starts_with("git+")
        || spec.starts_with("github:")
        || spec.starts_with("git:")
        || spec.starts_with("file:")
        || spec.starts_with("workspace:")
        || spec.starts_with("link:")
        || spec.starts_with("npm:")
        || spec.contains("://")
        || spec.starts_with('.'))
}

fn read_package_json(dir: &Path) -> Result<Value, NpmError> {
    let path = dir.join("package.json");
    let text = std::fs::read_to_string(&path).map_err(|e| NpmError::InvalidPackage {
        pkg: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "plugin".into()),
        message: format!("failed to read {}: {e}", path.display()),
    })?;
    serde_json::from_str(&text).map_err(|e| NpmError::InvalidPackage {
        pkg: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "plugin".into()),
        message: format!("invalid JSON in {}: {e}", path.display()),
    })
}

fn valid_package_name(pkg: &str) -> bool {
    if pkg.is_empty() || pkg.contains('\\') || pkg.contains("..") {
        return false;
    }
    if let Some(rest) = pkg.strip_prefix('@') {
        let Some((scope, name)) = rest.split_once('/') else {
            return false;
        };
        !scope.is_empty() && !name.is_empty() && !name.contains('/')
    } else {
        !pkg.contains('/') && !pkg.starts_with('.')
    }
}

fn git_package_name(url: &str) -> Option<String> {
    let name = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()?
        .strip_suffix(".git")
        .unwrap_or_else(|| url.trim_end_matches('/').rsplit('/').next().unwrap());
    if valid_package_name(name) {
        Some(name.to_string())
    } else {
        None
    }
}

fn cache_dir(paths: &GlobalPaths, pkg: &str, requested: &str) -> PathBuf {
    let base = paths.npm_packages().join(sanitize_package(pkg));
    if requested.is_empty() || requested == "latest" {
        return base;
    }
    let suffix: String = requested
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect();
    base.with_file_name(format!(
        "{}--{suffix}",
        base.file_name().unwrap().to_string_lossy()
    ))
}

fn cached_package_is_valid(target: &Path, pkg: &str, requested: &str) -> bool {
    let expected_version = semver::Version::parse(requested).ok();
    let valid = validate_package(
        target,
        Some(pkg),
        expected_version.as_ref().map(|_| requested),
    )
    .is_ok();
    // Dependency installation is part of the cache contract: a package is
    // cached only once its `.oc-deps.json` marker exists.
    valid && target.join(DEPS_MARKER).exists()
}

fn validate_package(
    target: &Path,
    expected_name: Option<&str>,
    expected_version: Option<&str>,
) -> Result<Value, NpmError> {
    let error_name = expected_name.unwrap_or("plugin");
    let path = target.join("package.json");
    let text = std::fs::read_to_string(&path).map_err(|e| NpmError::InvalidPackage {
        pkg: error_name.to_string(),
        message: format!("failed to read {}: {e}", path.display()),
    })?;
    let json: Value = serde_json::from_str(&text).map_err(|e| NpmError::InvalidPackage {
        pkg: error_name.to_string(),
        message: format!("invalid JSON in {}: {e}", path.display()),
    })?;
    let name = json.get("name").and_then(Value::as_str).unwrap_or_default();
    if name.is_empty() || expected_name.is_some_and(|expected| name != expected) {
        return Err(NpmError::InvalidPackage {
            pkg: error_name.to_string(),
            message: format!("package.json name is {name:?}"),
        });
    }
    if let Some(expected_version) = expected_version {
        let actual = json
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if actual != expected_version {
            return Err(NpmError::InvalidPackage {
                pkg: error_name.to_string(),
                message: format!("expected version {expected_version}, found {actual:?}"),
            });
        }
    }
    Ok(json)
}

fn pkg_name_dir(pkg: &str) -> PathBuf {
    // scoped names unpack as @scope/name
    PathBuf::from(pkg)
}

/// Clone a git plugin into the cache.
fn add_git(url: &str, target: &std::path::Path, pkg: &str) -> Result<PathBuf, NpmError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: e.to_string(),
        })?;
    }
    if target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url])
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| NpmError::Tarball {
            pkg: pkg.to_string(),
            message: format!("git is not available: {e}"),
        })?;
    if !status.success() {
        return Err(NpmError::Tarball {
            pkg: pkg.to_string(),
            message: format!("git clone failed for {url}"),
        });
    }
    validate_package(target, None, None).map_err(|error| match error {
        NpmError::InvalidPackage { message, .. } => NpmError::InvalidPackage {
            pkg: pkg.to_string(),
            message,
        },
        other => other,
    })?;
    Ok(target.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    use super::*;

    fn test_config() -> NpmConfig {
        NpmConfig::from_registry("https://registry.npmjs.org", None::<String>)
    }

    #[test]
    fn parses_registry_url() {
        let config = test_config();
        assert_eq!(
            registry_url("foo", &config),
            "https://registry.npmjs.org/foo"
        );
        assert_eq!(
            registry_url("@scope/name", &config),
            "https://registry.npmjs.org/%40scope/name"
        );
    }

    #[test]
    fn parses_registry_url_from_configured_registry() {
        let config = NpmConfig::from_registry("http://127.0.0.1:4321", None::<String>);
        assert_eq!(registry_url("foo", &config), "http://127.0.0.1:4321/foo");
    }

    #[test]
    fn parses_scoped_registry_url() {
        let config = NpmConfig::from_registry("http://127.0.0.1:4321", None::<String>);
        assert_eq!(
            registry_url("@scope/name", &config),
            "http://127.0.0.1:4321/%40scope/name"
        );
    }

    #[test]
    fn skips_non_registry_dependency_specs() {
        assert!(is_registry_spec("^1.2.3"));
        assert!(is_registry_spec("latest"));
        assert!(is_registry_spec("1.2.x"));
        assert!(!is_registry_spec("git+https://github.com/x/y.git"));
        assert!(!is_registry_spec("github:user/repo"));
        assert!(!is_registry_spec("file:../local"));
        assert!(!is_registry_spec("workspace:*"));
        assert!(!is_registry_spec("npm:alias@1.0.0"));
        assert!(!is_registry_spec("https://example.test/pkg.tgz"));
    }

    #[test]
    fn picks_versions() {
        let metadata = serde_json::json!({
            "dist-tags": { "latest": "2.0.0" },
            "versions": {
                "1.0.0": { "dist": { "tarball": "https://x/1.tgz" } },
                "2.0.0": { "dist": { "tarball": "https://x/2.tgz" } }
            }
        });
        assert_eq!(pick_version(&metadata, "latest").as_deref(), Some("2.0.0"));
        assert_eq!(pick_version(&metadata, "").as_deref(), Some("2.0.0"));
        assert_eq!(pick_version(&metadata, "1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(
            version_tarball(&metadata, "1.0.0").unwrap().1,
            "https://x/1.tgz"
        );
    }

    #[test]
    fn picks_highest_version_in_a_semver_range() {
        let metadata = serde_json::json!({
            "versions": {
                "1.2.0": {},
                "1.9.0": {},
                "2.0.0": {}
            }
        });
        assert_eq!(pick_version(&metadata, "^1.0.0").as_deref(), Some("1.9.0"));
        assert_eq!(pick_version(&metadata, "~1.2.0").as_deref(), Some("1.2.0"));
    }

    #[test]
    fn detects_git_specs() {
        assert_eq!(
            git_url("superpowers@git+https://github.com/obra/superpowers.git").as_deref(),
            Some("https://github.com/obra/superpowers.git")
        );
        assert_eq!(
            git_url("foo@github:user/repo").as_deref(),
            Some("https://github.com/user/repo.git")
        );
        assert_eq!(
            git_package_name("https://github.com/user/repo.git").as_deref(),
            Some("repo")
        );
        assert_eq!(
            git_package_name("file:///tmp/local-plugin").as_deref(),
            Some("local-plugin")
        );
        assert!(git_url("foo@1.0.0").is_none());
    }

    #[test]
    fn clones_git_plugin_from_local_repo() {
        // Skip unless git is available.
        let probe = std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !probe {
            eprintln!("skipping: git not available");
            return;
        }
        let repo = std::env::temp_dir().join(format!("oc-git-plugin-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&repo);
        std::fs::write(repo.join("package.json"), r#"{"name": "local-plugin"}"#).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        let url = format!("file://{}", repo.to_string_lossy());
        let spec = format!("local-plugin@git+{url}");
        // Use an isolated cache rather than the process user's global
        // directories. This keeps the test hermetic and works in restricted
        // sandboxes where the default XDG cache is not writable.
        let cache_root =
            std::env::temp_dir().join(format!("oc-git-plugin-cache-{}", std::process::id()));
        let paths = GlobalPaths {
            home: cache_root.clone(),
            data: cache_root.join("data"),
            cache: cache_root.join("cache"),
            config: cache_root.join("config"),
            state: cache_root.join("state"),
            tmp: cache_root.join("tmp"),
        };
        let target = add(&spec, &paths).expect("git add failed");
        assert!(target.join("package.json").exists());
        std::fs::remove_dir_all(&paths.npm_packages()).ok();
        std::fs::remove_dir_all(&cache_root).ok();
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn unpacks_tarball() {
        // Build a gzipped tarball with a `package/` prefix, as npm publishes.
        let mut tar_buf = Vec::new();
        {
            let encoder =
                flate2::write::GzEncoder::new(&mut tar_buf, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            let mut file = tar::Header::new_ustar();
            file.set_path("package/package.json").unwrap();
            file.set_size(2);
            file.set_mode(0o644);
            file.set_cksum();
            builder.append(&file, "{}".as_bytes()).unwrap();
            let mut dir = tar::Header::new_ustar();
            dir.set_path("package/src").unwrap();
            dir.set_entry_type(tar::EntryType::Directory);
            dir.set_size(0);
            dir.set_mode(0o755);
            dir.set_cksum();
            builder.append(&dir, std::io::empty()).unwrap();
            let mut file2 = tar::Header::new_ustar();
            file2.set_path("package/src/index.js").unwrap();
            file2.set_size(3);
            file2.set_mode(0o644);
            file2.set_cksum();
            builder.append(&file2, "abc".as_bytes()).unwrap();
            builder.finish().unwrap();
            let encoder = builder.into_inner().unwrap();
            encoder.finish().unwrap();
        }

        let target = std::env::temp_dir().join(format!("oc-npm-unpack-{}", std::process::id()));
        unpack_tarball(&tar_buf, &target, "test").unwrap();
        assert!(target.join("package.json").exists());
        assert_eq!(
            std::fs::read_to_string(target.join("src/index.js")).unwrap(),
            "abc"
        );
        std::fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn rejects_archive_path_traversal() {
        let staging = std::env::temp_dir().join(format!("oc-npm-traversal-{}", std::process::id()));
        let result = safe_archive_path(Path::new("package/../../escaped.js"), &staging, "plugin");
        assert!(matches!(result, Err(NpmError::Unpack { .. })));
        assert!(safe_archive_path(Path::new("/tmp/escaped.js"), &staging, "plugin").is_err());
    }

    #[test]
    fn rejects_symlink_entries() {
        let mut tar_buf = Vec::new();
        {
            let encoder =
                flate2::write::GzEncoder::new(&mut tar_buf, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            let mut link = tar::Header::new_ustar();
            link.set_path("package/link.js").unwrap();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_link_name("../../outside.js").unwrap();
            link.set_size(0);
            link.set_cksum();
            builder.append(&link, std::io::empty()).unwrap();
            builder.finish().unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }

        let root = std::env::temp_dir().join(format!("oc-npm-symlink-{}", std::process::id()));
        let target = root.join("node_modules/plugin");
        let result = unpack_tarball(&tar_buf, &target, "plugin");
        assert!(matches!(result, Err(NpmError::Unpack { .. })));
        assert!(!target.exists());
        std::fs::remove_dir_all(root).ok();
    }

    // -----------------------------------------------------------------------
    // Mock npm registry (in-process blocking HTTP server)
    // -----------------------------------------------------------------------

    /// A tiny in-process HTTP server serving npm-style metadata + tarballs.
    struct MockRegistry {
        addr: std::net::SocketAddr,
        authorizations: Arc<Mutex<Vec<String>>>,
        handle: Option<std::thread::JoinHandle<()>>,
        stop: Arc<std::sync::atomic::AtomicBool>,
    }

    /// Build a gzipped npm tarball with a `package/` prefix.
    fn make_tarball(package_json: &str, files: &[(&str, &str)]) -> Vec<u8> {
        let mut tar_buf = Vec::new();
        {
            let encoder =
                flate2::write::GzEncoder::new(&mut tar_buf, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            let mut manifest = tar::Header::new_ustar();
            manifest.set_path("package/package.json").unwrap();
            manifest.set_size(package_json.len() as u64);
            manifest.set_mode(0o644);
            manifest.set_cksum();
            builder.append(&manifest, package_json.as_bytes()).unwrap();
            for (path, contents) in files {
                let mut file = tar::Header::new_ustar();
                file.set_path(format!("package/{path}")).unwrap();
                file.set_size(contents.len() as u64);
                file.set_mode(0o644);
                file.set_cksum();
                builder.append(&file, contents.as_bytes()).unwrap();
            }
            builder.finish().unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        tar_buf
    }

    impl MockRegistry {
        /// A package published at `name`, with `versions` → `(package_json, files)`.
        fn new(packages: &[(&str, &[(&str, &str, &[(&str, &str)])])]) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock registry");
            listener.set_nonblocking(false).expect("blocking listener");
            let addr = listener.local_addr().expect("mock registry address");
            let authorizations: std::sync::Arc<Mutex<Vec<String>>> =
                std::sync::Arc::new(Mutex::new(Vec::new()));
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

            // Pre-build the tarballs and metadata so request handling is cheap.
            let mut tarballs: std::collections::HashMap<String, Vec<u8>> =
                std::collections::HashMap::new();
            let mut metadata: serde_json::Map<String, Value> = serde_json::Map::new();
            for (name, versions) in packages {
                let mut versions_map = serde_json::Map::new();
                let mut latest_version: Option<String> = None;
                for (version, package_json, files) in *versions {
                    let tarball_name = format!(
                        "{}-{}.tgz",
                        name.rsplit('/').next().unwrap_or(name),
                        version
                    );
                    let tarball_path = format!("/{name}/-/{tarball_name}");
                    let bytes = make_tarball(package_json, files);
                    tarballs.insert(tarball_path.clone(), bytes);
                    versions_map.insert(
                        version.to_string(),
                        serde_json::json!({ "dist": { "tarball": format!("http://{addr}{tarball_path}") } }),
                    );
                    latest_version = Some(version.to_string());
                }
                let mut dist_tags = serde_json::Map::new();
                if let Some(version) = latest_version {
                    dist_tags.insert("latest".into(), Value::String(version));
                }
                let entry = serde_json::json!({
                    "name": name,
                    "dist-tags": dist_tags,
                    "versions": versions_map,
                });
                metadata.insert(name.to_string(), entry);
            }

            let auths = authorizations.clone();
            let stop_flag = stop.clone();
            let handle = std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let Ok(mut stream) = stream else { continue };
                    let mut buffer = [0u8; 8192];
                    let Ok(read) = stream.read(&mut buffer) else {
                        continue;
                    };
                    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let mut lines = request.lines();
                    let request_line = lines.next().unwrap_or_default().to_string();
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or_default();
                    let path = parts.next().unwrap_or_default();
                    for line in lines {
                        if line.to_ascii_lowercase().starts_with("authorization:") {
                            let value = line
                                .split_once(':')
                                .map(|(_, v)| v.trim().to_string())
                                .unwrap_or_default();
                            auths.lock().unwrap().push(value);
                        }
                        if line.is_empty() {
                            break;
                        }
                    }
                    let decoded = percent_decode(path);
                    let body: Vec<u8>;
                    let status;
                    let content_type;
                    let metadata_key = decoded.trim_start_matches('/').to_string();
                    if method == "GET" && metadata.contains_key(&metadata_key) {
                        body = serde_json::to_vec(&metadata[&metadata_key]).unwrap();
                        status = "200 OK";
                        content_type = "application/json";
                    } else if method == "GET" && tarballs.contains_key(&decoded) {
                        body = tarballs[&decoded].clone();
                        status = "200 OK";
                        content_type = "application/octet-stream";
                    } else {
                        body = b"not found".to_vec();
                        status = "404 Not Found";
                        content_type = "text/plain";
                    }
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                }
            });

            Self {
                addr,
                authorizations,
                handle: Some(handle),
                stop,
            }
        }

        fn config(&self, token: Option<&str>) -> NpmConfig {
            NpmConfig::from_registry(format!("http://{}", self.addr), token)
        }

        fn auth_headers(&self) -> Vec<String> {
            self.authorizations.lock().unwrap().clone()
        }
    }

    impl Drop for MockRegistry {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            // A final connection wakes the accept loop so it can observe the flag.
            let _ = std::net::TcpStream::connect(self.addr);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// Decode the percent-encoded package path (`%40scope/name` -> `@scope/name`).
    fn percent_decode(path: &str) -> String {
        let bytes = path.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(hex) = u8::from_str_radix(&path[i + 1..i + 3], 16) {
                    out.push(hex);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn test_paths(name: &str) -> (GlobalPaths, std::path::PathBuf) {
        let cache_root =
            std::env::temp_dir().join(format!("oc-npm-mock-{}-{name}", std::process::id()));
        let paths = GlobalPaths {
            home: cache_root.clone(),
            data: cache_root.join("data"),
            cache: cache_root.join("cache"),
            config: cache_root.join("config"),
            state: cache_root.join("state"),
            tmp: cache_root.join("tmp"),
        };
        (paths, cache_root)
    }

    #[test]
    fn installs_nested_dependencies_from_mock_registry() {
        // app depends on helper-a (^1.0.0) which depends on helper-b (2.x).
        let app_json =
            r#"{"name":"oc-app","version":"1.0.0","dependencies":{"helper-a":"^1.0.0"}}"#;
        let helper_a_json =
            r#"{"name":"helper-a","version":"1.2.0","dependencies":{"helper-b":"2.x"}}"#;
        let helper_b_json = r#"{"name":"helper-b","version":"2.1.0","main":"index.js"}"#;
        let registry = MockRegistry::new(&[
            (
                "oc-app",
                &[("1.0.0", app_json, &[("index.js", "module.exports = 1")])],
            ),
            (
                "helper-a",
                &[
                    (
                        "1.1.0",
                        r#"{"name":"helper-a","version":"1.1.0","dependencies":{"helper-b":"2.x"}}"#,
                        &[("index.js", "old")],
                    ),
                    ("1.2.0", helper_a_json, &[("index.js", "new")]),
                ],
            ),
            (
                "helper-b",
                &[("2.1.0", helper_b_json, &[("index.js", "b")])],
            ),
        ]);
        let (paths, cache_root) = test_paths("nested");
        let config = registry.config(None);
        let target = add_with_config("oc-app@1.0.0", &paths, &config).expect("install root");

        // Root package is installed.
        assert!(target.join("package.json").exists());
        // Dependency tree is installed and version-pinned to the highest match.
        let helper_a = target.join("node_modules/helper-a");
        assert!(helper_a.join("package.json").exists());
        assert_eq!(
            std::fs::read_to_string(helper_a.join("index.js")).unwrap(),
            "new"
        );
        let helper_b = helper_a.join("node_modules/helper-b");
        assert!(helper_b.join("package.json").exists());
        // The marker records the pinned versions.
        let marker: Value =
            serde_json::from_str(&std::fs::read_to_string(target.join(DEPS_MARKER)).unwrap())
                .unwrap();
        assert_eq!(marker["dependencies"]["helper-a"], "1.2.0");

        // A second add hits the cache (no additional metadata requests needed to
        // re-download; it returns the same directory).
        let again = add_with_config("oc-app@1.0.0", &paths, &config).expect("cached add");
        assert_eq!(again, target);

        std::fs::remove_dir_all(&cache_root).ok();
    }

    #[test]
    fn sends_registry_auth_token_on_metadata_and_tarball_requests() {
        let app_json =
            r#"{"name":"private-app","version":"1.0.0","dependencies":{"private-dep":"1.0.0"}}"#;
        let dep_json = r#"{"name":"private-dep","version":"1.0.0"}"#;
        let registry = MockRegistry::new(&[
            ("private-app", &[("1.0.0", app_json, &[("index.js", "x")])]),
            ("private-dep", &[("1.0.0", dep_json, &[("index.js", "y")])]),
        ]);
        let (paths, cache_root) = test_paths("auth");
        let config = registry.config(Some("secret-token"));
        let target = add_with_config("private-app", &paths, &config).expect("install private");

        assert!(target
            .join("node_modules/private-dep/package.json")
            .exists());
        let headers = registry.auth_headers();
        // Metadata for both packages + both tarballs carry the token.
        assert!(
            headers.len() >= 4,
            "expected authed requests, got {headers:?}"
        );
        assert!(
            headers.iter().all(|h| h == "Bearer secret-token"),
            "unexpected auth headers: {headers:?}"
        );
        std::fs::remove_dir_all(&cache_root).ok();
    }

    #[test]
    fn installs_scoped_packages_from_mock_registry() {
        let app_json = r#"{"name":"@acme/plugin","version":"0.3.0","dependencies":{}}"#;
        let registry = MockRegistry::new(&[(
            "@acme/plugin",
            &[("0.3.0", app_json, &[("index.js", "scoped")])],
        )]);
        let (paths, cache_root) = test_paths("scoped");
        let config = registry.config(None);
        let target =
            add_with_config("@acme/plugin@0.3.0", &paths, &config).expect("install scoped");
        assert_eq!(
            std::fs::read_to_string(target.join("index.js")).unwrap(),
            "scoped"
        );
        std::fs::remove_dir_all(&cache_root).ok();
    }

    #[test]
    fn resolves_highest_version_in_range_from_mock_registry() {
        let app_json = r#"{"name":"ranged","version":"1.0.0","dependencies":{"lib":"^1.0.0"}}"#;
        let registry = MockRegistry::new(&[
            ("ranged", &[("1.0.0", app_json, &[("index.js", "r")])]),
            (
                "lib",
                &[
                    (
                        "1.0.0",
                        r#"{"name":"lib","version":"1.0.0"}"#,
                        &[("index.js", "old")],
                    ),
                    (
                        "1.5.0",
                        r#"{"name":"lib","version":"1.5.0"}"#,
                        &[("index.js", "new")],
                    ),
                ],
            ),
        ]);
        let (paths, cache_root) = test_paths("range");
        let config = registry.config(None);
        let target = add_with_config("ranged", &paths, &config).expect("install ranged");
        let lib = target.join("node_modules/lib");
        assert_eq!(
            std::fs::read_to_string(lib.join("index.js")).unwrap(),
            "new"
        );
        let marker: Value =
            serde_json::from_str(&std::fs::read_to_string(target.join(DEPS_MARKER)).unwrap())
                .unwrap();
        assert_eq!(marker["dependencies"]["lib"], "1.5.0");
        std::fs::remove_dir_all(&cache_root).ok();
    }
}

/// Unpack a gzipped npm tarball into `target`, stripping the `package/` prefix
/// npm tarballs carry.
fn unpack_tarball(bytes: &[u8], target: &Path, pkg: &str) -> Result<(), NpmError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let staging = target
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".staging-{}-{nonce}", std::process::id()));

    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: format!("remove stale staging directory: {e}"),
        })?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: e.to_string(),
    })?;

    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    if let Err(error) = extract_archive(&mut archive, &staging, pkg) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    // npm tarballs wrap everything under `package/`; move that directory (or
    // the whole staging dir) into the target.
    let src = if staging.join("package").exists() {
        staging.join("package")
    } else {
        staging.clone()
    };
    if target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: e.to_string(),
        })?;
    }
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: format!("replace existing package: {e}"),
        })?;
    }
    if let Err(error) = std::fs::rename(&src, target) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(NpmError::Unpack {
            pkg: pkg.to_string(),
            message: format!("atomically install package: {error}"),
        });
    }
    std::fs::remove_dir_all(&staging).map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: format!("clean staging directory: {e}"),
    })?;
    Ok(())
}

fn extract_archive(
    archive: &mut tar::Archive<flate2::read::GzDecoder<&[u8]>>,
    staging: &Path,
    pkg: &str,
) -> Result<(), NpmError> {
    let entries = archive.entries().map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: e.to_string(),
    })?;
    for entry in entries {
        let mut entry = entry.map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: e.to_string(),
        })?;
        let raw_path = entry.path().map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: e.to_string(),
        })?;
        let relative = safe_archive_path(&raw_path, staging, pkg)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = staging.join(&relative);
        match entry.header().entry_type() {
            tar::EntryType::Directory => {
                std::fs::create_dir_all(&destination).map_err(|e| NpmError::Unpack {
                    pkg: pkg.to_string(),
                    message: format!("create {}: {e}", destination.display()),
                })?;
            }
            tar::EntryType::Regular => {
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| NpmError::Unpack {
                        pkg: pkg.to_string(),
                        message: format!("create {}: {e}", parent.display()),
                    })?;
                }
                let mut output =
                    std::fs::File::create(&destination).map_err(|e| NpmError::Unpack {
                        pkg: pkg.to_string(),
                        message: format!("create {}: {e}", destination.display()),
                    })?;
                io::copy(&mut entry, &mut output).map_err(|e| NpmError::Unpack {
                    pkg: pkg.to_string(),
                    message: format!("write {}: {e}", destination.display()),
                })?;
            }
            entry_type => {
                return Err(NpmError::Unpack {
                    pkg: pkg.to_string(),
                    message: format!("unsupported archive entry type {entry_type:?}"),
                });
            }
        }
    }
    Ok(())
}

/// Validate a tar path before writing it. npm archives normally have a single
/// `package/` prefix; accepting an unprefixed archive is useful for local test
/// fixtures, but traversal, absolute paths, and links are never accepted.
fn safe_archive_path(path: &Path, staging: &Path, pkg: &str) -> Result<PathBuf, NpmError> {
    let components: Vec<Component<'_>> = path.components().collect();
    if components.iter().any(|component| {
        matches!(
            component,
            Component::RootDir | Component::Prefix(_) | Component::ParentDir
        )
    }) {
        return Err(NpmError::Unpack {
            pkg: pkg.to_string(),
            message: format!("archive path escapes staging directory: {}", path.display()),
        });
    }
    let mut relative = PathBuf::new();
    for component in components {
        if let Component::Normal(name) = component {
            relative.push(name);
        }
    }
    let candidate = staging.join(&relative);
    if candidate.strip_prefix(staging).is_err() {
        return Err(NpmError::Unpack {
            pkg: pkg.to_string(),
            message: format!("archive path escapes staging directory: {}", path.display()),
        });
    }
    Ok(relative)
}
