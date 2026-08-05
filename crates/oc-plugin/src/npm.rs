//! npm package fetching for plugins.
//!
//! The reference installs plugins with `@npmcli/arborist` (a full npm client)
//! into `<cache>/packages/<pkg>/node_modules`. There is no JS toolchain in the
//! environment, so this module fetches the registry metadata and tarball
//! directly and unpacks the single package into the same layout.

use std::path::{Path, PathBuf};

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
    // Simple semver range support: prefer exact match, then any matching version.
    let range = requested
        .strip_prefix('^')
        .or_else(|| requested.strip_prefix('~'))
        .or_else(|| requested.strip_prefix('='));
    let base = range.unwrap_or(requested);
    let mut best: Option<(&String, &Value)> = None;
    for (version, entry) in versions {
        if version.starts_with(base) {
            let is_better = match best {
                Some((current, _)) => version > current,
                None => true,
            };
            if is_better {
                best = Some((version, entry));
            }
        }
    }
    best.map(|(version, _)| version.clone())
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
    let (pkg, requested) = crate::loader::parse_plugin_specifier(spec);
    let dir = paths.npm_packages().join(sanitize_package(&pkg));
    let target = dir.join("node_modules").join(pkg_name_dir(&pkg));
    if target.join("package.json").exists() {
        return Ok(target);
    }

    // Git specs (`pkg@git+...`) are cloned instead of fetched from the registry.
    if let Some(url) = git_url(spec) {
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

    Ok(target)
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
    Ok(target.to_path_buf())
}

/// Unpack a gzipped npm tarball into `target`, stripping the `package/` prefix
/// npm tarballs carry.
fn unpack_tarball(bytes: &[u8], target: &Path, pkg: &str) -> Result<(), NpmError> {
    let staging = target
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".staging-{}", std::process::id()));

    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    std::fs::create_dir_all(&staging).map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: e.to_string(),
    })?;

    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(&staging).map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: e.to_string(),
    })?;

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
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(&src, target) {
        Ok(_) => {}
        Err(_) => {
            // Cross-device fallback: copy the contents.
            move_dir_contents(&src, target);
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
}

/// Move the contents of `src` into `dest` (both directories).
fn move_dir_contents(src: &Path, dest: &Path) {
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let from = entry.path();
            let to = dest.join(entry.file_name());
            match std::fs::rename(&from, &to) {
                Ok(_) => {}
                Err(_) => {
                    let _ = std::fs::remove_dir_all(&to);
                    let _ = std::fs::rename(&from, &to);
                }
            }
        }
    }
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
    fn detects_git_specs() {
        assert_eq!(
            git_url("superpowers@git+https://github.com/obra/superpowers.git").as_deref(),
            Some("https://github.com/obra/superpowers.git")
        );
        assert_eq!(
            git_url("foo@github:user/repo").as_deref(),
            Some("https://github.com/user/repo.git")
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
        let paths = GlobalPaths::new();
        let target = add(&spec, &paths).expect("git add failed");
        assert!(target.join("package.json").exists());
        std::fs::remove_dir_all(paths.npm_packages()).ok();
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
}
