//! npm package fetching for plugins.
//!
//! The reference installs plugins with `@npmcli/arborist` (a full npm client)
//! into `<cache>/packages/<pkg>/node_modules`. There is no JS toolchain in the
//! environment, so this module fetches the registry metadata and tarball
//! directly and unpacks the single package into the same layout.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::loader::sanitize_package;
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

fn registry_url(pkg: &str) -> String {
    if pkg.starts_with('@') {
        // scoped: @scope/name → encode the slash
        let (scope, name) = pkg.split_once('/').unwrap_or((pkg, ""));
        format!(
            "https://registry.npmjs.org/{}/{}",
            scope.replace('@', "%40"),
            name
        )
    } else {
        format!("https://registry.npmjs.org/{pkg}")
    }
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

    let client = reqwest::blocking::Client::new();
    let metadata = client
        .get(registry_url(&pkg))
        .send()
        .and_then(|res| res.error_for_status())
        .and_then(|res| res.json::<Value>())
        .map_err(|e| NpmError::Metadata {
            pkg: pkg.clone(),
            message: e.to_string(),
        })?;

    let version = pick_version(&metadata, &requested).ok_or_else(|| NpmError::VersionNotFound {
        pkg: pkg.clone(),
        version: requested.clone(),
    })?;
    let (_, tarball) = version_tarball(&metadata, &version).ok_or_else(|| NpmError::NoTarball {
        pkg: pkg.clone(),
        version: version.clone(),
    })?;

    let bytes = client
        .get(&tarball)
        .send()
        .and_then(|res| res.error_for_status())
        .and_then(|res| res.bytes())
        .map_err(|e| NpmError::Tarball {
            pkg: pkg.clone(),
            message: e.to_string(),
        })?;

    std::fs::create_dir_all(&dir).map_err(|e| NpmError::Unpack {
        pkg: pkg.clone(),
        message: e.to_string(),
    })?;
    unpack_tarball(&bytes, &target, &pkg)?;
    validate_package(&target, Some(&pkg), Some(&version))?;

    Ok(target)
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
    validate_package(
        target,
        Some(pkg),
        expected_version.as_ref().map(|_| requested),
    )
    .is_ok()
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
    use super::*;

    #[test]
    fn parses_registry_url() {
        assert_eq!(registry_url("foo"), "https://registry.npmjs.org/foo");
        assert_eq!(
            registry_url("@scope/name"),
            "https://registry.npmjs.org/%40scope/name"
        );
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
