//! Plugin installation and config patching.
//!
//! Mirrors reference/packages/opencode/src/plugin/install.ts: install an npm
//! plugin and patch its spec into the plugin list of the opencode config.

use std::path::PathBuf;

use serde_json::Value;

use crate::jsonc;
use crate::loader::{self, PluginPackage};
use crate::paths::GlobalPaths;

/// A config file target kind.
pub type Kind = &'static str;
pub const KIND_SERVER: Kind = "server";
pub const KIND_TUI: Kind = "tui";

/// A target discovered from package metadata.
#[derive(Debug, Clone)]
pub struct Target {
    pub kind: Kind,
    pub opts: Option<Value>,
}

/// The result of installing a plugin.
#[derive(Debug)]
pub enum InstallResult {
    Ok { target: String },
    Failed { error: String },
}

/// The result of reading a plugin manifest.
#[derive(Debug)]
pub enum ManifestResult {
    Ok { targets: Vec<Target> },
    ReadFailed { file: String, error: String },
    NoTargets { file: String },
}

/// The mode of a config patch.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchMode {
    Noop,
    Add,
    Replace,
}

/// One patched config file.
#[derive(Debug, Clone)]
pub struct PatchItem {
    pub kind: Kind,
    pub mode: PatchMode,
    pub file: String,
}

/// The result of patching plugin config.
#[derive(Debug)]
pub enum PatchResult {
    Ok {
        dir: String,
        items: Vec<PatchItem>,
    },
    Failed {
        dir: String,
        kind: Kind,
        message: String,
    },
}

/// Input for patching plugin config. Mirrors `PatchInput` in install.ts.
#[derive(Debug, Clone)]
pub struct PatchInput {
    pub spec: String,
    pub targets: Vec<Target>,
    pub force: bool,
    pub global: bool,
    pub vcs: Option<String>,
    pub worktree: String,
    pub directory: String,
    pub config: Option<String>,
}

/// Install a plugin, returning the resolved target directory.
pub fn install_plugin(spec: &str) -> InstallResult {
    match crate::shared::resolve_plugin_target(spec) {
        Ok(target) => InstallResult::Ok { target },
        Err(error) => InstallResult::Failed { error },
    }
}

/// Read the targets (server/tui) a plugin package exposes. Mirrors
/// `packageTargets` + `readPluginManifest` in install.ts.
pub fn read_plugin_manifest(target: &str) -> ManifestResult {
    let pkg = match crate::shared::read_plugin_package(target) {
        Ok(pkg) => pkg,
        Err(error) => {
            return ManifestResult::ReadFailed {
                file: target.to_string(),
                error,
            };
        }
    };
    let targets = package_targets(&pkg);
    if targets.is_empty() {
        return ManifestResult::NoTargets {
            file: pkg.pkg.to_string_lossy().into_owned(),
        };
    }
    ManifestResult::Ok { targets }
}

fn export_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(s) = value.as_str() {
        let next = s.trim();
        if !next.is_empty() {
            return Some(next.to_string());
        }
        return None;
    }
    if let Some(obj) = value.as_object() {
        for key in ["import", "default"] {
            if let Some(s) = obj.get(key).and_then(Value::as_str) {
                let hit = s.trim();
                if !hit.is_empty() {
                    return Some(hit.to_string());
                }
            }
        }
    }
    None
}

fn export_target(pkg: &PluginPackage, kind: Kind) -> Option<Target> {
    let exports = pkg.json.get("exports").and_then(Value::as_object)?;
    let key = format!("./{kind}");
    let value = exports.get(&key)?;
    let entry = export_value(Some(value))?;
    let opts = value
        .as_object()
        .and_then(|obj| obj.get("config"))
        .cloned()
        .filter(|v| v.is_object());
    let _ = entry;
    Some(Target { kind, opts })
}

fn has_main_target(pkg: &PluginPackage) -> bool {
    pkg.json
        .get("main")
        .and_then(Value::as_str)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn package_targets(pkg: &PluginPackage) -> Vec<Target> {
    let mut targets = Vec::new();
    let server = export_target(pkg, KIND_SERVER);
    if let Some(server) = server {
        targets.push(server);
    } else if has_main_target(pkg) {
        targets.push(Target {
            kind: KIND_SERVER,
            opts: None,
        });
    }
    let tui = export_target(pkg, KIND_TUI);
    if let Some(tui) = tui {
        targets.push(tui);
    } else if !targets.iter().any(|t| t.kind == KIND_TUI) && package_has_themes(pkg) {
        targets.push(Target {
            kind: KIND_TUI,
            opts: None,
        });
    }
    targets
}

fn package_has_themes(pkg: &PluginPackage) -> bool {
    pkg.json
        .get("oc-themes")
        .map(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
        .unwrap_or(false)
}

fn patch_dir(input: &PatchInput, paths: &GlobalPaths) -> PathBuf {
    if input.global {
        return input
            .config
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.config.clone());
    }
    let git = input.vcs.as_deref() == Some("git") && input.worktree != "/";
    let root = if git {
        PathBuf::from(&input.worktree)
    } else {
        PathBuf::from(&input.directory)
    };
    root.join(".opencode")
}

fn patch_name(kind: Kind) -> &'static str {
    if kind == KIND_SERVER {
        "opencode"
    } else {
        "tui"
    }
}

/// The config file candidates in a directory for a given name.
fn config_files(dir: &std::path::Path, name: &str) -> Vec<PathBuf> {
    for ext in ["json", "jsonc"] {
        let file = dir.join(format!("{name}.{ext}"));
        if file.exists() {
            return vec![file];
        }
    }
    vec![dir.join(format!("{name}.json"))]
}

fn patch_one(
    dir: &std::path::Path,
    target: &Target,
    spec: &str,
    force: bool,
    pkg: &str,
) -> Result<PatchItem, (Kind, String)> {
    let name = patch_name(target.kind);
    let files = config_files(dir, name);
    let mut cfg = files[0].clone();
    for file in files {
        if file.exists() {
            cfg = file;
            break;
        }
    }
    let src = std::fs::read_to_string(&cfg).unwrap_or_else(|_| "{}".to_string());
    let text = if src.trim().is_empty() {
        "{}".to_string()
    } else {
        src
    };

    let (spec_str, options) = (spec.to_string(), target.opts.clone());
    let item: Value = match options {
        Some(opts) => Value::Array(vec![Value::String(spec_str), opts]),
        None => Value::String(spec_str),
    };

    let (mode, out) = jsonc::patch_plugin_list(&text, spec, &item, pkg, force).map_err(|e| {
        (
            target.kind,
            format!("invalid json in {}: {e}", cfg.display()),
        )
    })?;
    if mode != jsonc::PatchMode::Noop {
        std::fs::write(&cfg, out).map_err(|e| (target.kind, e.to_string()))?;
    }
    let mode = match mode {
        jsonc::PatchMode::Noop => PatchMode::Noop,
        jsonc::PatchMode::Add => PatchMode::Add,
        jsonc::PatchMode::Replace => PatchMode::Replace,
    };
    Ok(PatchItem {
        kind: target.kind,
        mode,
        file: cfg.to_string_lossy().into_owned(),
    })
}

/// Patch the plugin list of the opencode config. Mirrors `patchPluginConfig`.
pub fn patch_plugin_config(input: &PatchInput) -> PatchResult {
    let paths = GlobalPaths::new();
    let dir = patch_dir(input, &paths);
    let (pkg, _) = loader::parse_plugin_specifier(&input.spec);
    let mut items = Vec::new();
    for target in &input.targets {
        match patch_one(&dir, target, &input.spec, input.force, &pkg) {
            Ok(item) => items.push(item),
            Err((kind, message)) => {
                return PatchResult::Failed {
                    dir: dir.to_string_lossy().into_owned(),
                    kind,
                    message,
                };
            }
        }
    }
    PatchResult::Ok {
        dir: dir.to_string_lossy().into_owned(),
        items,
    }
}
