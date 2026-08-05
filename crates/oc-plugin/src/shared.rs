//! Plugin target resolution and package introspection.
//!
//! Mirrors reference/packages/opencode/src/plugin/shared.ts: spec parsing,
//! `resolvePluginTarget`, `createPluginEntry`, `readPluginPackage`,
//! `checkPluginCompatibility`, and the v1 module shape helpers.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::loader::{
    is_path_plugin_spec, parse_plugin_specifier, PluginKind, PluginPackage, PluginSource,
    INDEX_FILES, KIND_SERVER, SOURCE_FILE, SOURCE_NPM,
};

/// The entry metadata for a plugin target. Mirrors `PluginEntry` in shared.ts.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub spec: String,
    pub source: PluginSource,
    pub target: String,
    pub pkg: Option<PluginPackage>,
    pub entry: Option<String>,
}

/// Read `package.json` for a plugin target. Mirrors `readPluginPackage`.
pub fn read_plugin_package(target: &str) -> Result<PluginPackage, String> {
    let file = target.strip_prefix("file://").unwrap_or(target);
    let path = PathBuf::from(file);
    let stat =
        std::fs::metadata(&path).map_err(|e| format!("failed to stat {}: {e}", path.display()))?;
    let dir = if stat.is_dir() {
        path
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let pkg = dir.join("package.json");
    let json: Value = serde_json::from_str(
        &std::fs::read_to_string(&pkg)
            .map_err(|e| format!("failed to read {}: {e}", pkg.display()))?,
    )
    .map_err(|e| format!("invalid package.json {}: {e}", pkg.display()))?;
    Ok(PluginPackage { dir, pkg, json })
}

/// Extract the export entrypoint for a plugin kind from package metadata.
/// Mirrors `resolvePackageEntrypoint` in shared.ts.
fn package_entrypoint(_spec: &str, kind: PluginKind, pkg: &PluginPackage) -> Option<String> {
    let exports = pkg.json.get("exports").and_then(Value::as_object);
    if let Some(exports) = exports {
        let key = format!("./{kind}");
        if let Some(raw) = extract_export_value(exports.get(&key)) {
            let resolved = resolve_export_path(&raw, &pkg.dir);
            return Some(resolved);
        }
    }
    if kind != "server" {
        return None;
    }
    if let Some(main) = pkg.json.get("main").and_then(Value::as_str) {
        let main = main.trim();
        if !main.is_empty() {
            return Some(resolve_export_path(main, &pkg.dir));
        }
    }
    None
}

fn extract_export_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(obj) = value.as_object() {
        for key in ["import", "default"] {
            if let Some(s) = obj.get(key).and_then(Value::as_str) {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn resolve_export_path(raw: &str, dir: &Path) -> String {
    let raw = raw.strip_prefix("file://").unwrap_or(raw);
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    dir.join(path).to_string_lossy().into_owned()
}

/// Resolve the entrypoint for a plugin. Mirrors `resolvePluginEntrypoint` and
/// `createPluginEntry` in shared.ts.
pub fn create_plugin_entry(
    spec: &str,
    target: &str,
    kind: PluginKind,
) -> Result<PluginEntry, String> {
    let source = if is_path_plugin_spec(spec) {
        SOURCE_FILE
    } else {
        SOURCE_NPM
    };
    let pkg = if source == SOURCE_NPM {
        read_plugin_package(target).ok()
    } else {
        read_plugin_package(target).ok()
    };
    let entry = resolve_entrypoint(spec, target, kind, pkg.as_ref());
    Ok(PluginEntry {
        spec: spec.to_string(),
        source,
        target: target.to_string(),
        pkg,
        entry,
    })
}

fn resolve_entrypoint(
    spec: &str,
    target: &str,
    kind: PluginKind,
    pkg: Option<&PluginPackage>,
) -> Option<String> {
    let source = if is_path_plugin_spec(spec) {
        SOURCE_FILE
    } else {
        SOURCE_NPM
    };
    if let Some(pkg) = pkg {
        if let Some(entry) = package_entrypoint(spec, kind, pkg) {
            return Some(entry);
        }
    }
    let path = target_path(target)?;
    let is_dir = std::fs::metadata(&path)
        .map(|m| m.is_dir())
        .unwrap_or(false);
    if !is_dir {
        return Some(path.to_string_lossy().into_owned());
    }
    for name in INDEX_FILES {
        let file = path.join(name);
        if file.exists() {
            return Some(file.to_string_lossy().into_owned());
        }
    }
    if source == SOURCE_NPM && kind == KIND_SERVER {
        return None;
    }
    Some(path.to_string_lossy().into_owned())
}

fn target_path(target: &str) -> Option<PathBuf> {
    let file = target.strip_prefix("file://").unwrap_or(target);
    let path = PathBuf::from(file);
    if path.is_absolute() {
        Some(path)
    } else {
        None
    }
}

/// Resolve a plugin specifier to a target on disk, installing npm plugins on
/// demand. Mirrors `resolvePluginTarget` in shared.ts.
pub fn resolve_plugin_target(spec: &str) -> Result<String, String> {
    if is_path_plugin_spec(spec) {
        return resolve_path_plugin_target(spec);
    }
    let (pkg, version) = parse_plugin_specifier(spec);
    let full = if version.is_empty() || version == "latest" {
        format!("{pkg}@latest")
    } else {
        spec.to_string()
    };
    let dir = crate::npm::add(&full, &crate::paths::GlobalPaths::new())
        .map_err(|e| format!("failed to install plugin {spec}: {e}"))?;
    Ok(dir.to_string_lossy().into_owned())
}

/// Resolve a path-like plugin spec to a target. Mirrors
/// `resolvePathPluginTarget` in shared.ts.
pub fn resolve_path_plugin_target(spec: &str) -> Result<String, String> {
    let raw = spec.strip_prefix("file://").unwrap_or(spec);
    let path = PathBuf::from(raw);
    let file = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(&path)
    };
    let stat =
        std::fs::metadata(&file).map_err(|e| format!("failed to stat {}: {e}", file.display()))?;
    if !stat.is_dir() {
        if spec.starts_with("file://") {
            return Ok(spec.to_string());
        }
        return Ok(to_file_url(&file));
    }
    if file.join("package.json").exists() {
        return Ok(to_file_url(&file));
    }
    for name in INDEX_FILES {
        let index = file.join(name);
        if index.exists() {
            return Ok(to_file_url(&index));
        }
    }
    Err(format!(
        "Plugin directory {} is missing package.json or index file",
        file.display()
    ))
}

fn to_file_url(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap().join(path)
    };
    format!("file://{}", abs.to_string_lossy())
}

/// Check npm plugin compatibility against the running opencode version. File
/// plugins skip this gate. Mirrors `checkPluginCompatibility` in shared.ts.
pub fn check_plugin_compatibility(
    target: &str,
    opencode_version: &str,
    pkg: Option<&PluginPackage>,
) -> Result<(), String> {
    let parsed = semver::Version::parse(opencode_version).ok();
    let Some(parsed) = parsed else { return Ok(()) };
    if parsed.major == 0 {
        return Ok(());
    }
    let fallback = read_plugin_package(target).ok();
    let hit = match pkg {
        Some(pkg) => Some(pkg),
        None => fallback.as_ref(),
    };
    let Some(hit) = hit else { return Ok(()) };
    let Some(range) = hit
        .json
        .get("engines")
        .and_then(Value::as_object)
        .and_then(|e| e.get("opencode"))
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    let ok = semver::VersionReq::parse(range)
        .map(|req| req.matches(&parsed))
        .unwrap_or(true);
    if !ok {
        return Err(format!(
            "Plugin requires opencode {range} but running {opencode_version}"
        ));
    }
    Ok(())
}

/// Extract the plugin id from a module. Mirrors `readPluginId` in shared.ts.
pub fn read_plugin_id(id: &Value, spec: &str) -> Result<Option<String>, String> {
    match id {
        Value::Null => Ok(None),
        Value::String(s) => {
            let value = s.trim();
            if value.is_empty() {
                Err(format!("Plugin {spec} has an empty id"))
            } else {
                Ok(Some(value.to_string()))
            }
        }
        _ => Err(format!("Plugin {spec} has invalid id type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oc-shared-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_package_and_entrypoint() {
        let dir = pkg_dir("pkg");
        std::fs::write(
            dir.join("package.json"),
            r#"{
  "name": "my-plugin",
  "version": "1.0.0",
  "main": "dist/index.js",
  "exports": { "./server": { "import": "./dist/server.js" } }
}"#,
        )
        .unwrap();
        let pkg = read_plugin_package(&dir.to_string_lossy()).unwrap();
        assert_eq!(pkg.name().as_deref(), Some("my-plugin"));
        assert_eq!(pkg.json["version"], "1.0.0");

        let entry = package_entrypoint("my-plugin", "server", &pkg).unwrap();
        assert!(entry.ends_with("dist/server.js"));

        let main = package_entrypoint("my-plugin", "tui", &pkg);
        assert!(main.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compatibility_gate() {
        let dir = pkg_dir("compat");
        std::fs::write(
            dir.join("package.json"),
            r#"{ "name": "p", "engines": { "opencode": ">=1.18.0" } }"#,
        )
        .unwrap();
        let pkg = read_plugin_package(&dir.to_string_lossy()).unwrap();
        assert!(check_plugin_compatibility(&dir.to_string_lossy(), "1.18.13", Some(&pkg)).is_ok());
        assert!(check_plugin_compatibility(&dir.to_string_lossy(), "1.0.0", Some(&pkg)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_plugin_id() {
        assert_eq!(read_plugin_id(&Value::Null, "x").unwrap(), None);
        assert_eq!(
            read_plugin_id(&Value::String("abc".into()), "x").unwrap(),
            Some("abc".into())
        );
        assert!(read_plugin_id(&Value::String("  ".into()), "x").is_err());
        assert!(read_plugin_id(&Value::Bool(true), "x").is_err());
    }

    #[test]
    fn resolves_path_plugin_target() {
        let dir = pkg_dir("target");
        std::fs::write(dir.join("package.json"), r#"{"name":"p"}"#).unwrap();
        let target = resolve_path_plugin_target(&dir.to_string_lossy()).unwrap();
        assert!(target.starts_with("file://"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
