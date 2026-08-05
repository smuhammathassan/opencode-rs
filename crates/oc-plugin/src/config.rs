//! Plugin declarations from opencode config.
//!
//! Mirrors reference/packages/opencode/src/config/plugin.ts (`ConfigPlugin`)
//! and reference/packages/core/src/v1/config/plugin.ts (`ConfigPluginV1`).

/// A plugin entry from `opencode.json`: either a bare specifier string or a
/// `[specifier, options]` tuple. Mirrors `ConfigPluginV1.Spec`.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginSpec {
    Plain(String),
    WithOptions(String, serde_json::Value),
}

impl PluginSpec {
    /// The specifier string. Mirrors `ConfigPlugin.pluginSpecifier`.
    pub fn specifier(&self) -> String {
        match self {
            PluginSpec::Plain(s) => s.clone(),
            PluginSpec::WithOptions(s, _) => s.clone(),
        }
    }

    /// The options object, if declared. Mirrors `ConfigPlugin.pluginOptions`.
    pub fn options(&self) -> Option<serde_json::Value> {
        match self {
            PluginSpec::Plain(_) => None,
            PluginSpec::WithOptions(_, options) => Some(options.clone()),
        }
    }
}

/// A config scope a plugin origin belongs to. Mirrors `ConfigPlugin.Scope`.
#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    Global,
    Local,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Local => "local",
        }
    }
}

/// Keeps the original config provenance attached to a spec. Mirrors
/// `ConfigPlugin.Origin`.
#[derive(Debug, Clone)]
pub struct PluginOrigin {
    pub spec: PluginSpec,
    pub source: String,
    pub scope: Scope,
}

pub fn plugin_specifier(spec: &PluginSpec) -> String {
    spec.specifier()
}

pub fn plugin_options(spec: &PluginSpec) -> Option<serde_json::Value> {
    spec.options()
}

/// Deduplicate on the load identity (package name for npm specs, exact file URL
/// for local specs), keeping the last origin. Mirrors
/// `ConfigPlugin.deduplicatePluginOrigins`.
pub fn deduplicate_plugin_origins(plugins: &[PluginOrigin]) -> Vec<PluginOrigin> {
    let mut seen = std::collections::HashSet::new();
    let mut list: Vec<PluginOrigin> = Vec::new();
    for plugin in plugins.iter().rev() {
        let spec = plugin_specifier(&plugin.spec);
        let name = if spec.starts_with("file://") {
            spec.clone()
        } else {
            crate::loader::parse_plugin_specifier(&spec).0
        };
        if seen.contains(&name) {
            continue;
        }
        seen.insert(name);
        list.push(plugin.clone());
    }
    list.reverse();
    list
}
