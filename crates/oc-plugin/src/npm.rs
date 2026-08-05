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

/// Add a plugin package to the cache, returning the resolved plugin directory.
/// Mirrors `Npm.add` in reference/packages/core/src/npm.ts.
pub fn add(spec: &str, paths: &GlobalPaths) -> Result<PathBuf, NpmError> {
    let (pkg, requested) = crate::loader::parse_plugin_specifier(spec);
    let dir = paths.npm_packages().join(sanitize_package(&pkg));
    let target = dir.join("node_modules").join(pkg_name_dir(&pkg));
    if target.join("package.json").exists() {
        return Ok(target);
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

/// Unpack a gzipped npm tarball into `target`, stripping the `package/` prefix
/// npm tarballs carry.
fn unpack_tarball(bytes: &[u8], target: &Path, pkg: &str) -> Result<(), NpmError> {
    if target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
    std::fs::create_dir_all(target).map_err(|e| NpmError::Unpack {
        pkg: pkg.to_string(),
        message: e.to_string(),
    })?;

    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    let count = archive
        .entries()
        .map_err(|e| NpmError::Unpack {
            pkg: pkg.to_string(),
            message: e.to_string(),
        })?
        .filter_map(|entry| entry.ok())
        .map(|mut entry| {
            let path = entry.path().map(PathBuf::from).ok()?;
            let rel = if path.starts_with("package") {
                path.strip_prefix("package").ok()?.to_path_buf()
            } else {
                path
            };
            if rel.as_os_str().is_empty() {
                return None;
            }
            // Ensure the parent exists (avoids unpack_in following symlinks
            // outside the target).
            if let Some(parent) = rel.parent() {
                let dest = target.join(parent);
                let _ = std::fs::create_dir_all(&dest);
            }
            entry.unpack_in(target).ok()?;
            Some(())
        })
        .collect::<Option<Vec<_>>>()
        .map(|items| items.len())
        .unwrap_or(0);

    if count == 0 {
        return Err(NpmError::Unpack {
            pkg: pkg.to_string(),
            message: "tarball contained no files".into(),
        });
    }
    Ok(())
}
