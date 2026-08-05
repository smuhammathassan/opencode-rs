//! Plugin resolution and loading.
//!
//! Mirrors reference/packages/opencode/src/plugin/loader.ts and shared.ts. The
//! reference dynamic-imports the resolved entrypoint via Bun; here the resolved
//! entrypoint is transpiled and evaluated in-process on QuickJS.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::js::transpile::{transpile_module, TranspileError};

/// The file names tried when a plugin target is a directory without an
/// explicit entrypoint. From reference/packages/opencode/src/plugin/shared.ts.
pub const INDEX_FILES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.js",
    "index.mjs",
    "index.cjs",
];

/// A plugin source: local file or npm package.
pub type PluginSource = &'static str;
pub const SOURCE_FILE: PluginSource = "file";
pub const SOURCE_NPM: PluginSource = "npm";

/// A plugin kind: server or tui.
pub type PluginKind = &'static str;
pub const KIND_SERVER: PluginKind = "server";
pub const KIND_TUI: PluginKind = "tui";

/// Resolves module specifiers to transpiled source code for the JS runtime.
///
/// The JS `__oc_require` bridge sends absolute paths (or bare specs) here. The
/// resolver caches transpiled modules and reads from disk on a miss, so
/// dynamic imports and un-scanned requires still work.
pub struct ModuleResolver {
    base: PathBuf,
    cache: Mutex<std::collections::HashMap<PathBuf, String>>,
}

impl ModuleResolver {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            cache: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Pre-register a transpiled module so it never needs disk access at
    /// runtime. `path` must be absolute.
    pub fn register(&self, path: impl Into<PathBuf>, code: String) {
        self.cache.lock().unwrap().insert(path.into(), code);
    }

    /// Resolve a module spec to transpiled source.
    pub fn resolve(&self, spec: &str) -> Result<Option<String>, String> {
        let path = self.resolve_path(spec);
        for candidate in self.candidates(&path) {
            if let Some(code) = self.load(&candidate)? {
                return Ok(Some(code));
            }
        }
        Ok(None)
    }

    fn resolve_path(&self, spec: &str) -> PathBuf {
        let raw = spec.strip_prefix("file://").unwrap_or(spec);
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return path;
        }
        if raw.starts_with('.') {
            return self.base.join(&path);
        }
        // Bare spec: look in node_modules under the plugin directory.
        self.base.join("node_modules").join(&path)
    }

    /// Generate the candidate paths for a module, mirroring Node's extension
    /// and directory-index resolution plus the reference's INDEX_FILES.
    fn candidates(&self, path: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let has_known_ext = path
            .extension()
            .map(|ext| {
                matches!(
                    ext.to_str(),
                    Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs")
                )
            })
            .unwrap_or(false);
        if has_known_ext {
            out.push(path.to_path_buf());
            return out;
        }
        for ext in ["ts", "tsx", "js", "mjs", "cjs"] {
            out.push(path.with_extension(ext));
        }
        for index in INDEX_FILES {
            out.push(path.join(index));
        }
        out
    }

    fn load(&self, path: &Path) -> Result<Option<String>, String> {
        if let Some(code) = self.cache.lock().unwrap().get(path) {
            return Ok(Some(code.clone()));
        }
        let source = std::fs::read_to_string(path)
            .map_err(|_| format!("failed to read {}", path.display()))?;
        let code = transpile_module(&source)
            .map_err(|e| format!("failed to transpile {}: {e}", path.display()))?;
        self.cache
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), code.clone());
        Ok(Some(code))
    }
}

/// Normalize a path plugin spec to an absolute path.
fn spec_to_path(spec: &str) -> PathBuf {
    if let Some(rest) = spec.strip_prefix("file://") {
        return PathBuf::from(rest);
    }
    PathBuf::from(spec)
}

/// Is `spec` a path-like plugin spec? Mirrors `isPathPluginSpec` in shared.ts.
pub fn is_path_plugin_spec(spec: &str) -> bool {
    spec.starts_with("file://") || spec.starts_with('.') || Path::new(spec).is_absolute()
}

/// Split an npm-style specifier into `(package, version)`. Mirrors
/// `parsePluginSpecifier` in shared.ts for the common cases.
pub fn parse_plugin_specifier(spec: &str) -> (String, String) {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    // scoped package @scope/name@version
    if let Some(rest) = trimmed.strip_prefix('@') {
        if let Some(slash) = rest.find('/') {
            let scope = &rest[..slash];
            let tail = &rest[slash + 1..];
            if let Some(at) = tail.find('@') {
                let name = tail[..at].to_string();
                let version = tail[at + 1..].to_string();
                return (format!("@{scope}/{name}"), version);
            }
            return (format!("@{scope}/{tail}"), "latest".into());
        }
        return (trimmed.to_string(), String::new());
    }
    if let Some(at) = trimmed.find('@') {
        let name = trimmed[..at].to_string();
        let version = trimmed[at + 1..].to_string();
        if name.is_empty() {
            return (trimmed.to_string(), String::new());
        }
        return (name, version);
    }
    if trimmed.contains(':') {
        // git/url specs — keep the whole thing as the package
        return (trimmed.to_string(), String::new());
    }
    (trimmed.to_string(), "latest".into())
}

/// Old npm package names for plugins that are now built-in. Mirrors
/// `DEPRECATED_PLUGIN_PACKAGES` in shared.ts.
pub const DEPRECATED_PLUGIN_PACKAGES: &[&str] =
    &["opencode-openai-codex-auth", "opencode-copilot-auth"];

pub fn is_deprecated_plugin(spec: &str) -> bool {
    DEPRECATED_PLUGIN_PACKAGES
        .iter()
        .any(|pkg| spec.contains(pkg))
}

/// A normalized plugin declaration derived from config. Mirrors
/// `PluginLoader.Plan` in loader.ts.
#[derive(Debug, Clone)]
pub struct Plan {
    pub spec: String,
    pub options: Option<serde_json::Value>,
    pub deprecated: bool,
}

/// A plugin resolved to a concrete target + entrypoint on disk. Mirrors
/// `PluginLoader.Resolved`.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub spec: String,
    pub options: Option<serde_json::Value>,
    pub source: PluginSource,
    pub target: String,
    pub entry: Option<String>,
    pub pkg: Option<PluginPackage>,
}

/// A plugin target that does not expose the requested kind of entrypoint.
/// Mirrors `PluginLoader.Missing`.
#[derive(Debug, Clone)]
pub struct Missing {
    pub spec: String,
    pub source: PluginSource,
    pub target: String,
    pub message: String,
}

/// A resolved plugin whose entry has been transpiled. Mirrors
/// `PluginLoader.Loaded`.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub resolved: Resolved,
    pub code: String,
}

/// Package metadata read from `package.json`.
#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub dir: PathBuf,
    pub pkg: PathBuf,
    pub json: serde_json::Value,
}

impl PluginPackage {
    pub fn name(&self) -> Option<String> {
        self.json
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }
}

/// Convert an `npm@range` style spec to the directory used by the npm cache.
pub fn sanitize_package(pkg: &str) -> String {
    pkg.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '-' | '.' | '_' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("plugin {spec} target is empty")]
    EmptyTarget { spec: String },
    #[error("{0}")]
    Other(String),
}

/// The intermediate result of resolving a plan to an entrypoint.
#[derive(Debug, Clone)]
pub enum Result2 {
    Resolved(Resolved),
    Missing(Missing),
}

/// Resolve a plugin plan into a concrete entrypoint, installing npm plugins on
/// demand. Mirrors `PluginLoader.resolve` in loader.ts.
pub fn resolve(plan: &Plan, kind: PluginKind) -> Result<Result2, ResolveError> {
    let target = crate::shared::resolve_plugin_target(&plan.spec)
        .map_err(|e| ResolveError::Other(e.to_string()))?;
    if target.is_empty() {
        return Err(ResolveError::EmptyTarget {
            spec: plan.spec.clone(),
        });
    }
    let entry = crate::shared::create_plugin_entry(&plan.spec, &target, kind)
        .map_err(|e| ResolveError::Other(e.to_string()))?;
    let resolved = Resolved {
        spec: plan.spec.clone(),
        options: plan.options.clone(),
        source: entry.source,
        target: entry.target.clone(),
        entry: entry.entry.clone(),
        pkg: entry.pkg.clone(),
    };
    if entry.entry.is_none() {
        return Ok(Result2::Missing(Missing {
            spec: plan.spec.clone(),
            source: entry.source,
            target: entry.target,
            message: format!("Plugin {} does not expose a {kind} entrypoint", plan.spec),
        }));
    }
    Ok(Result2::Resolved(resolved))
}

/// The outcome of resolving and loading a plan. Mirrors the staged results of
/// `PluginLoader.resolve` in loader.ts (with the import step folded in).
/// The large `Loaded` payload is boxed to keep the enum size small (clippy
/// `large_enum_variant`).
#[derive(Debug, Clone)]
pub enum ResolveOutcome {
    Resolved(Box<Loaded>),
    Missing(Missing),
    Failed { stage: &'static str, error: String },
}

/// Transpile the resolved entrypoint for in-process evaluation. Mirrors
/// `PluginLoader.load` (the module import step).
pub fn load(resolved: &Resolved) -> Result<Loaded, TranspileError> {
    let entry = resolved
        .entry
        .as_deref()
        .ok_or_else(|| TranspileError::new("plugin has no entrypoint"))?;
    let path = spec_to_path(entry);
    let source = std::fs::read_to_string(&path)
        .map_err(|e| TranspileError::new(format!("failed to read {}: {e}", path.display())))?;
    let code = transpile_module(&source)?;
    Ok(Loaded {
        resolved: resolved.clone(),
        code,
    })
}

/// Resolve and transpile a single plugin plan. Mirrors `PluginLoader.resolve`
/// plus the module import step.
pub fn resolve_and_load(plan: &Plan, kind: PluginKind) -> ResolveOutcome {
    let resolved = match resolve(plan, kind) {
        Ok(Result2::Resolved(resolved)) => resolved,
        Ok(Result2::Missing(missing)) => return ResolveOutcome::Missing(missing),
        Err(error) => {
            return ResolveOutcome::Failed {
                stage: "install",
                error: error.to_string(),
            };
        }
    };
    // npm plugins can declare which opencode versions they support; file
    // plugins are treated as local development code and skip this gate.
    if resolved.source == SOURCE_NPM {
        if let Err(error) = crate::shared::check_plugin_compatibility(
            &resolved.target,
            crate::OPENCODE_VERSION,
            resolved.pkg.as_ref(),
        ) {
            return ResolveOutcome::Failed {
                stage: "compatibility",
                error,
            };
        }
    }
    match load(&resolved) {
        Ok(loaded) => ResolveOutcome::Resolved(Box::new(loaded)),
        Err(error) => ResolveOutcome::Failed {
            stage: "load",
            error: error.to_string(),
        },
    }
}

/// Progress reporting hooks for [`load_external`]. Mirrors `PluginLoader.Report`
/// in loader.ts.
pub trait Report {
    /// Called before each load attempt.
    fn start(&mut self, _candidate: &crate::config::PluginOrigin) {}
    /// Called when the package exists but lacks the requested entrypoint.
    fn missing(&mut self, _candidate: &crate::config::PluginOrigin, _message: &str) {}
    /// Called for operational failures (install, entry, compatibility, load).
    fn error(&mut self, _candidate: &crate::config::PluginOrigin, _stage: &str, _error: &str) {}
}

/// A no-op reporter.
pub struct NoopReport;

impl Report for NoopReport {}

/// The resolved plugin id for a loaded plugin. Mirrors `resolvePluginId` in
/// shared.ts: file plugins must export an id, npm plugins fall back to the
/// package name.
pub fn resolve_plugin_id(loaded: &Loaded) -> Result<String, String> {
    let source = loaded.resolved.source;
    let id = loaded.resolved.pkg.as_ref().and_then(|pkg| pkg.name());
    if source == SOURCE_FILE {
        return id.ok_or_else(|| format!("Path plugin {} must export id", loaded.resolved.spec));
    }
    Ok(id.unwrap_or_else(|| loaded.resolved.spec.clone()))
}

/// Load all configured plugins, dropping skipped/failed entries while
/// preserving order. Mirrors `PluginLoader.loadExternal`.
pub fn load_external(
    items: &[crate::config::PluginOrigin],
    kind: PluginKind,
    resolver: &ModuleResolver,
) -> Vec<Loaded> {
    load_external_reported(items, kind, resolver, &mut NoopReport)
}

/// Like [`load_external`] but reports progress/errors through `report`.
pub fn load_external_reported(
    items: &[crate::config::PluginOrigin],
    kind: PluginKind,
    resolver: &ModuleResolver,
    report: &mut dyn Report,
) -> Vec<Loaded> {
    let mut out = Vec::new();
    for origin in items {
        let spec = crate::config::plugin_specifier(&origin.spec);
        let plan = Plan {
            spec: spec.clone(),
            options: crate::config::plugin_options(&origin.spec),
            deprecated: is_deprecated_plugin(&spec),
        };
        if plan.deprecated {
            continue;
        }
        report.start(origin);
        match resolve_and_load(&plan, kind) {
            ResolveOutcome::Resolved(loaded) => {
                register_with_resolver(&loaded, resolver);
                out.push(*loaded);
            }
            ResolveOutcome::Missing(missing) => report.missing(origin, &missing.message),
            ResolveOutcome::Failed { stage, error } => report.error(origin, stage, &error),
        }
    }
    out
}

/// Register a loaded module (and its static local imports) in the resolver so
/// `__oc_require` hits the cache at runtime.
pub fn register_with_resolver(loaded: &Loaded, resolver: &ModuleResolver) {
    let entry = loaded.resolved.entry.as_deref().unwrap_or("");
    let path = spec_to_path(entry);
    resolver.register(&path, loaded.code.clone());
    if let Some(dir) = path.parent() {
        let base = dir.to_path_buf();
        preload_imports(&loaded.code, &base, resolver);
    }
}

/// Pre-register the transitive local imports of a transpiled module so
/// `__oc_require` hits the cache at runtime. Mirrors Bun's static import
/// resolution; dynamic imports fall back to on-disk resolution.
fn preload_imports(code: &str, base: &Path, resolver: &ModuleResolver) {
    for spec in static_import_specs(code) {
        let path = {
            let p = std::path::PathBuf::from(&spec);
            if p.is_absolute() {
                p
            } else if spec.starts_with('.') {
                base.join(&p)
            } else {
                base.join("node_modules").join(&p)
            }
        };
        if let Ok(Some(resolved)) = resolver.resolve(&path.to_string_lossy()) {
            resolver.register(&path, resolved.clone());
            if let Some(parent) = path.parent() {
                preload_imports(&resolved, parent, resolver);
            }
        }
    }
}

/// Extract static import specifiers from a transpiled module. The transpiler
/// leaves them as `__oc_require("...")` calls.
fn static_import_specs(code: &str) -> Vec<String> {
    let mut specs = Vec::new();
    for line in code.lines() {
        let mut rest = line;
        while let Some(start) = rest.find("__oc_require(") {
            rest = &rest[start + "__oc_require(".len()..];
            let Some(end) = rest.find(')') else { break };
            let inner = rest[..end].trim();
            if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
                specs.push(inner[1..inner.len() - 1].to_string());
            }
            rest = &rest[end..];
        }
    }
    specs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_specifiers() {
        assert_eq!(
            parse_plugin_specifier("foo"),
            ("foo".into(), "latest".into())
        );
        assert_eq!(
            parse_plugin_specifier("foo@1.2.3"),
            ("foo".into(), "1.2.3".into())
        );
        assert_eq!(
            parse_plugin_specifier("foo@^1"),
            ("foo".into(), "^1".into())
        );
        assert_eq!(
            parse_plugin_specifier("@scope/name"),
            ("@scope/name".into(), "latest".into())
        );
        assert_eq!(
            parse_plugin_specifier("@scope/name@2"),
            ("@scope/name".into(), "2".into())
        );
    }

    #[test]
    fn detects_path_specs() {
        assert!(is_path_plugin_spec("./plugin.ts"));
        assert!(is_path_plugin_spec("../plugin.ts"));
        assert!(is_path_plugin_spec("file:///tmp/x.ts"));
        assert!(is_path_plugin_spec("/abs/path.ts"));
        assert!(!is_path_plugin_spec("my-plugin"));
        assert!(!is_path_plugin_spec("my-plugin@1.0.0"));
    }

    #[test]
    fn detects_deprecated() {
        assert!(is_deprecated_plugin("opencode-openai-codex-auth"));
        assert!(is_deprecated_plugin("opencode-copilot-auth"));
        assert!(!is_deprecated_plugin("my-plugin"));
    }

    #[test]
    fn sanitizes_packages() {
        assert_eq!(sanitize_package("my-plugin"), "my-plugin");
        assert_eq!(sanitize_package("@scope/name"), "@scope/name");
    }

    #[test]
    fn extracts_import_specs() {
        let code = r#"
const a = __oc_require("./a.js").a;
const b = __oc_require("node:fs/promises").b;
"#;
        let specs = static_import_specs(code);
        assert_eq!(
            specs,
            vec!["./a.js".to_string(), "node:fs/promises".to_string()]
        );
    }

    #[test]
    fn resolves_file_plugin() {
        let dir = std::env::temp_dir().join(format!("oc-plugin-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"name": "test-plugin", "main": "index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("index.js"),
            "export default { id: 'x', server: async () => ({}) }",
        )
        .unwrap();

        let target = crate::shared::resolve_plugin_target(&dir.to_string_lossy()).unwrap();
        let entry =
            crate::shared::create_plugin_entry(&dir.to_string_lossy(), &target, KIND_SERVER)
                .unwrap();
        assert!(entry.entry.is_some());
        assert!(entry.entry.unwrap().contains("index.js"));

        let plan = Plan {
            spec: dir.to_string_lossy().into_owned(),
            options: None,
            deprecated: false,
        };
        match resolve_and_load(&plan, KIND_SERVER) {
            ResolveOutcome::Resolved(loaded) => {
                assert!(loaded.code.contains("__oc_define(\"default\""));
            }
            other => panic!("expected resolved, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reports_failed_plugins() {
        let origin = crate::config::PluginOrigin {
            spec: crate::config::PluginSpec::Plain("/does/not/exist.ts".into()),
            source: "opencode.json".into(),
            scope: crate::config::Scope::Local,
        };
        struct R {
            errors: Vec<String>,
        }
        impl Report for R {
            fn error(&mut self, _c: &crate::config::PluginOrigin, _s: &str, e: &str) {
                self.errors.push(e.to_string());
            }
        }
        let mut report = R { errors: Vec::new() };
        let resolver = ModuleResolver::new("/tmp");
        let loaded = load_external_reported(&[origin], KIND_SERVER, &resolver, &mut report);
        assert!(loaded.is_empty());
        assert!(!report.errors.is_empty());
    }
}
