// Config loading: opencode.json/.jsonc parse, deep merge, and discovery.
//
// From reference/packages/opencode/src/config/config.ts,
// reference/packages/opencode/src/config/command.ts,
// reference/packages/opencode/src/config/agent.ts and
// reference/packages/opencode/src/config/plugin.ts.

use crate::entry_name::config_entry_name_from_path;
use crate::error::{ConfigError, Issue, Result};
use crate::glob;
use crate::managed;
use crate::merge::{concat_unique, dedupe_keep_last, merge_deep};
use crate::parse;
use crate::paths;
use crate::v1::agent;
use crate::v1::command;
use crate::v1::config::Info;
use crate::v1::permission::{Action, Rule};
use crate::v1::plugin::Spec;
use crate::variable::{self, Missing, Source};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const REMOTE_CONFIG_MAX_BODY_BYTES: usize = 1024 * 1024;
const REMOTE_CONFIG_TIMEOUT: Duration = Duration::from_secs(10);

/// Environment-derived flags mirroring `@opencode-ai/core/flag/flag`.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    /// `OPENCODE_CONFIG` — explicit config file path.
    pub config: Option<String>,
    /// `OPENCODE_CONFIG_DIR` — explicit config directory.
    pub config_dir: Option<String>,
    /// `OPENCODE_CONFIG_CONTENT` — inline config content.
    pub config_content: Option<String>,
    /// `OPENCODE_DISABLE_PROJECT_CONFIG`.
    pub disable_project_config: bool,
    /// `OPENCODE_PERMISSION` — JSON permission overrides.
    pub permission: Option<String>,
    /// `OPENCODE_DISABLE_AUTOCOMPACT`.
    pub disable_autocompact: bool,
    /// `OPENCODE_DISABLE_PRUNE`.
    pub disable_prune: bool,
}

fn truthy(value: Option<String>) -> bool {
    matches!(value.as_deref(), Some("true") | Some("1"))
}

impl Flags {
    pub fn from_env() -> Self {
        Self {
            config: std::env::var("OPENCODE_CONFIG").ok(),
            config_dir: std::env::var("OPENCODE_CONFIG_DIR").ok(),
            config_content: std::env::var("OPENCODE_CONFIG_CONTENT").ok(),
            disable_project_config: truthy(std::env::var("OPENCODE_DISABLE_PROJECT_CONFIG").ok()),
            permission: std::env::var("OPENCODE_PERMISSION").ok(),
            disable_autocompact: truthy(std::env::var("OPENCODE_DISABLE_AUTOCOMPACT").ok()),
            disable_prune: truthy(std::env::var("OPENCODE_DISABLE_PRUNE").ok()),
        }
    }
}

/// A plugin spec together with the file and scope that declared it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginOrigin {
    pub spec: Spec,
    pub source: String,
    pub scope: Scope,
}

impl PluginOrigin {
    pub fn specifier(&self) -> &str {
        plugin_specifier(&self.spec)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Options for `load_instance_state`.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// The working/project directory.
    pub directory: String,
    /// The git worktree root; project configs are only discovered up to here.
    pub worktree: Option<String>,
    /// Extra env substitutions (e.g. auth tokens).
    pub env: IndexMap<String, String>,
    /// Explicit username override (falls back to the OS user).
    pub username: Option<String>,
}

/// A credential entry that opts a caller into OpenCode's well-known remote
/// config discovery. This intentionally mirrors the `wellknown` auth record
/// without making `oc-config` depend on the provider/auth crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfigCredential {
    /// The enterprise/account base URL, without a trailing slash.
    pub origin: String,
    /// Environment variable name made available to remote and local config
    /// substitutions while this instance is loaded.
    pub key: String,
    /// Token associated with `key`.
    pub token: String,
}

impl RemoteConfigCredential {
    pub fn new(
        origin: impl Into<String>,
        key: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            origin: origin.into(),
            key: key.into(),
            token: token.into(),
        }
    }
}

/// Inputs for remote config discovery. The reference obtains these entries
/// from its auth service; callers of this crate provide the already-resolved
/// credentials at the config boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteConfigOptions {
    pub credentials: Vec<RemoteConfigCredential>,
}

impl RemoteConfigOptions {
    pub fn new(credentials: Vec<RemoteConfigCredential>) -> Self {
        Self { credentials }
    }
}

/// The result of loading an instance's config.
#[derive(Debug, Clone)]
pub struct InstanceState {
    pub config: Info,
    pub directories: Vec<String>,
    pub plugin_origins: Vec<PluginOrigin>,
}

#[derive(Debug, Deserialize)]
struct WellKnownResponse {
    #[serde(default)]
    config: Value,
    #[serde(default)]
    remote_config: Value,
}

/// Loads the normal instance state with authenticated well-known remote
/// config sources applied before local/global config.
///
/// The remote protocol is the reference OpenCode protocol:
/// `GET {origin}/.well-known/opencode` returns optional `config` and
/// `remote_config` JSON values. When `remote_config.url` is present, that URL
/// is fetched and its object (or its object-valued `config` member) is merged
/// over the well-known inline config. Remote failures are returned rather than
/// silently producing a partially configured instance.
///
/// This function is async because the reference performs network I/O during
/// instance loading. The existing synchronous [`load_instance_state`] remains
/// unchanged for embedders that do not provide auth-backed remotes.
pub async fn load_instance_state_with_remotes(
    options: &LoadOptions,
    remotes: &RemoteConfigOptions,
) -> Result<InstanceState> {
    if remotes.credentials.is_empty() {
        return load_instance_state(options);
    }

    let client = reqwest::Client::builder()
        .timeout(REMOTE_CONFIG_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| ConfigError::Remote {
            url: String::new(),
            message: format!("failed to create HTTP client: {error}"),
        })?;

    let mut env = options.env.clone();
    let mut remote = Info::default();
    let mut remote_origins = Vec::new();

    for credential in &remotes.credentials {
        let origin = normalize_remote_origin(&credential.origin)?;
        if credential.key.is_empty() {
            return Err(ConfigError::Remote {
                url: origin,
                message: "well-known credential has an empty environment key".to_string(),
            });
        }
        env.insert(credential.key.clone(), credential.token.clone());

        let wellknown_url = format!("{origin}/.well-known/opencode");
        let wellknown_value = fetch_remote_json(&client, &wellknown_url, None, &origin).await?;
        let wellknown: WellKnownResponse =
            serde_json::from_value(wellknown_value).map_err(|error| ConfigError::Remote {
                url: wellknown_url.clone(),
                message: format!("expected a JSON object with config fields: {error}"),
            })?;

        let inline = object_or_empty(wellknown.config);
        let fetched = if let Value::Object(remote_config) = wellknown.remote_config {
            // The reference treats a non-object or an object without a string
            // `url` as absent; preserve that forward-compatible behavior.
            if let Some(raw_url) = remote_config.get("url").and_then(Value::as_str) {
                let endpoint_url = substitute_remote_value(raw_url, &wellknown_url, &env)?;
                let headers = remote_config
                    .get("headers")
                    .and_then(Value::as_object)
                    .map(|headers| {
                        headers
                            .iter()
                            .filter_map(|(key, value)| value.as_str().map(|value| (key, value)))
                            .map(|(key, value)| {
                                Ok::<_, ConfigError>((
                                    key.clone(),
                                    substitute_remote_value(value, &wellknown_url, &env)?,
                                ))
                            })
                            .collect::<Result<BTreeMap<_, _>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let endpoint_value =
                    fetch_remote_json(&client, &endpoint_url, Some(&headers), &origin).await?;
                match endpoint_value {
                    Value::Object(mut object) => match object.shift_remove("config") {
                        Some(Value::Object(config)) => Value::Object(config),
                        Some(other) => {
                            object.insert("config".to_string(), other);
                            Value::Object(object)
                        }
                        None => Value::Object(object),
                    },
                    _ => {
                        return Err(ConfigError::Remote {
                            url: endpoint_url,
                            message: "expected a JSON object".to_string(),
                        })
                    }
                }
            } else {
                Value::Object(Default::default())
            }
        } else {
            Value::Object(Default::default())
        };

        let mut combined = crate::merge::merge_deep(&inline, &fetched);
        if let Value::Object(ref mut object) = combined {
            object
                .entry("$schema")
                .or_insert_with(|| Value::String("https://opencode.ai/config.json".to_string()));
        }
        let text = serde_json::to_string(&combined).map_err(|error| ConfigError::Remote {
            url: wellknown_url.clone(),
            message: format!("failed to encode merged remote config: {error}"),
        })?;
        let next = load_config(
            &text,
            &Source::Virtual {
                source: wellknown_url.clone(),
                dir: origin.clone(),
            },
            Some(&env),
        )?;
        let mut next_for_merge = next.clone();
        next_for_merge.plugin = None;
        merge_plugins(
            &mut remote,
            &mut remote_origins,
            next.plugin.clone().unwrap_or_default(),
            &wellknown_url,
            Scope::Global,
        );
        remote = merge_config(&remote, &next_for_merge);
    }

    let local_options = LoadOptions {
        env,
        ..options.clone()
    };
    let local = load_instance_state(&local_options)?;
    let mut local_config = local.config.clone();
    local_config.plugin = None;
    let mut config = merge_config(&remote, &local_config);

    // Keep remote plugin provenance while allowing later local declarations
    // to win exactly as they do in the normal loader.
    let mut origins = remote_origins;
    origins.extend(local.plugin_origins);
    origins = dedupe_keep_last(origins, |origin| plugin_identity(&origin.spec));
    config.plugin = Some(origins.iter().map(|origin| origin.spec.clone()).collect());

    // `load_instance_state` supplies the final username default before this
    // wrapper can merge the remote base. Preserve an explicit remote username
    // when the local value is only that default.
    let default_username = options.username.clone().unwrap_or_else(current_username);
    if remote.username.is_some() && local.config.username.as_deref() == Some(&default_username) {
        config.username = remote.username.clone();
    }

    Ok(InstanceState {
        config,
        directories: local.directories,
        plugin_origins: origins,
    })
}

fn normalize_remote_origin(origin: &str) -> Result<String> {
    let normalized = origin.trim_end_matches('/');
    let parsed = url::Url::parse(normalized).map_err(|error| ConfigError::Remote {
        url: origin.to_string(),
        message: format!("invalid URL: {error}"),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ConfigError::Remote {
            url: origin.to_string(),
            message: "only http and https URLs are supported".to_string(),
        });
    }
    if normalized.is_empty() {
        return Err(ConfigError::Remote {
            url: origin.to_string(),
            message: "URL is empty".to_string(),
        });
    }
    Ok(normalized.to_string())
}

fn object_or_empty(value: Value) -> Value {
    match value {
        Value::Object(_) => value,
        _ => Value::Object(Default::default()),
    }
}

fn substitute_remote_value(
    value: &str,
    source: &str,
    env: &IndexMap<String, String>,
) -> Result<String> {
    variable::substitute(
        value,
        &Source::Virtual {
            source: source.to_string(),
            dir: source.to_string(),
        },
        Some(env),
        Missing::Empty,
    )
}

async fn fetch_remote_json(
    client: &reqwest::Client,
    url: &str,
    headers: Option<&BTreeMap<String, String>>,
    login_origin: &str,
) -> Result<Value> {
    let parsed = url::Url::parse(url).map_err(|error| ConfigError::Remote {
        url: url.to_string(),
        message: format!("invalid URL: {error}"),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ConfigError::Remote {
            url: url.to_string(),
            message: "only http and https URLs are supported".to_string(),
        });
    }

    let mut request = client.get(url);
    if let Some(headers) = headers {
        for (name, value) in headers {
            request = request.header(name, value);
        }
    }
    let response = request.send().await.map_err(|error| ConfigError::Remote {
        url: url.to_string(),
        message: error.to_string(),
    })?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let body = response
        .bytes()
        .await
        .map_err(|error| ConfigError::Remote {
            url: url.to_string(),
            message: error.to_string(),
        })?;
    if body.len() > REMOTE_CONFIG_MAX_BODY_BYTES {
        return Err(ConfigError::Remote {
            url: url.to_string(),
            message: format!("response exceeds {REMOTE_CONFIG_MAX_BODY_BYTES} bytes"),
        });
    }
    let text = String::from_utf8_lossy(&body);
    if content_type.contains("html")
        || text.trim_start().starts_with("<!doctype")
        || text.trim_start().starts_with("<html")
    {
        return Err(ConfigError::RemoteAuth {
            url: login_origin.to_string(),
            remote: url.to_string(),
        });
    }
    if !status.is_success() {
        return Err(ConfigError::Remote {
            url: url.to_string(),
            message: format!("server returned HTTP {status}"),
        });
    }
    serde_json::from_slice(&body).map_err(|error| ConfigError::Remote {
        url: url.to_string(),
        message: format!("invalid JSON: {error}"),
    })
}

/// Parses config text into a `Config` struct.
///
/// Mirrors `Config.loadConfig` with a virtual source: variable substitution,
/// JSONC parse, legacy-key normalization, then schema validation.
pub fn load_config(
    text: &str,
    source: &Source,
    env: Option<&IndexMap<String, String>>,
) -> Result<Info> {
    let expanded = variable::substitute(text, source, env, Missing::Error)?;
    let parsed = parse::jsonc(&expanded, source.display())?;
    let data = normalize_loaded_config(parsed);
    parse::schema(data, source.display())
}

/// Loads a config file from disk (`Config.loadFile`).
pub fn load_file(path: &str, env: Option<&IndexMap<String, String>>) -> Result<Info> {
    tracing::info!(path, "loading config");
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Ok(Info::default()),
    };
    if text.trim().is_empty() {
        return Ok(Info::default());
    }
    let mut config = load_config(
        &text,
        &Source::Path {
            path: path.to_string(),
        },
        env,
    )?;
    resolve_loaded_plugins(&mut config, path);
    if config.schema.is_none() {
        config.schema = Some("https://opencode.ai/config.json".to_string());
        let updated = insert_schema_header(&text, "https://opencode.ai/config.json");
        let _ = std::fs::write(path, updated);
    }
    Ok(config)
}

/// `normalizeLoadedConfig` — drop legacy `theme`/`keybinds`/`tui` keys.
fn normalize_loaded_config(data: Value) -> Value {
    let Value::Object(mut map) = data else {
        return data;
    };
    let had_legacy =
        map.contains_key("theme") || map.contains_key("keybinds") || map.contains_key("tui");
    if !had_legacy {
        return Value::Object(map);
    }
    map.remove("theme");
    map.remove("keybinds");
    map.remove("tui");
    Value::Object(map)
}

fn insert_schema_header(text: &str, schema: &str) -> String {
    match text.find('{') {
        Some(index) => {
            let mut out = String::with_capacity(text.len() + 48);
            out.push_str(&text[..index]);
            out.push_str("{\n  \"$schema\": \"");
            out.push_str(schema);
            out.push_str("\",");
            out.push_str(&text[index + 1..]);
            out
        }
        None => text.to_string(),
    }
}

/// Merges `source` into `target` with `mergeConfigConcatArrays` semantics:
/// deep merge with `instructions` concatenated and de-duplicated.
pub fn merge_config(target: &Info, source: &Info) -> Info {
    let target_value = serde_json::to_value(target).expect("config serializes");
    let source_value = serde_json::to_value(source).expect("config serializes");
    let mut merged = merge_deep(&target_value, &source_value);
    if let (Some(target), Some(source)) = (&target.instructions, &source.instructions) {
        merged["instructions"] = Value::Array(
            concat_unique(target, source)
                .into_iter()
                .map(Value::String)
                .collect(),
        );
    }
    serde_json::from_value(merged).expect("merged config is valid")
}

/// The full global+project+env config pipeline (`Config.loadInstanceState`),
/// minus remote well-known/account configs which need HTTP/auth.
pub fn load_instance_state(options: &LoadOptions) -> Result<InstanceState> {
    let flags = Flags::from_env();
    let directory = std::path::PathBuf::from(&options.directory);
    let worktree = options.worktree.as_deref().map(std::path::PathBuf::from);
    let env = if options.env.is_empty() {
        None
    } else {
        Some(&options.env)
    };

    struct Acc {
        config: Info,
        origins: Vec<PluginOrigin>,
    }

    impl Acc {
        fn merge(&mut self, next: Info, source: &str, scope: Scope) {
            self.config = merge_config(&self.config, &next);
            merge_plugins(
                &mut self.config,
                &mut self.origins,
                next.plugin.clone().unwrap_or_default(),
                source,
                scope,
            );
        }
    }

    let mut acc = Acc {
        config: Info::default(),
        origins: Vec::new(),
    };

    let global = load_global(env)?;
    acc.merge(
        global,
        &paths::config_dir().to_string_lossy(),
        Scope::Global,
    );

    if let Some(custom) = &flags.config {
        acc.merge(load_file(custom, env)?, custom, Scope::Global);
    }

    if !flags.disable_project_config {
        for file in paths::files("opencode", &directory, worktree.as_deref()) {
            let path = file.to_string_lossy().into_owned();
            let loaded = load_file(&path, env)?;
            acc.merge(loaded, &path, Scope::Local);
        }
    }

    acc.config.agent.get_or_insert_with(IndexMap::new);
    acc.config.mode.get_or_insert_with(IndexMap::new);
    acc.config.plugin.get_or_insert_with(Vec::new);

    let directories = paths::directories(&directory, worktree.as_deref());

    for dir in &directories {
        let dir_str = dir.to_string_lossy().into_owned();
        let is_opencode = dir.ends_with(".opencode") || Some(&dir_str) == flags.config_dir.as_ref();
        if is_opencode {
            for file in ["opencode.json", "opencode.jsonc"] {
                let source = dir.join(file);
                let path = source.to_string_lossy().into_owned();
                let loaded = load_file(&path, env)?;
                acc.merge(loaded, &path, Scope::Local);
            }
            acc.config.agent.get_or_insert_with(IndexMap::new);
            acc.config.mode.get_or_insert_with(IndexMap::new);
            acc.config.plugin.get_or_insert_with(Vec::new);
        }

        let _ = ensure_gitignore(dir);

        // TODO(integration): background npm dependency install per directory
        // (`npmSvc.install(dir, { add: [{ name: "@opencode-ai/plugin", ... }] })`).

        let commands = load_commands(dir)?;
        acc.config.command = Some(merge_maps(acc.config.command.take(), commands));

        let agents = load_agents(dir)?;
        acc.config.agent = Some(merge_maps(acc.config.agent.take(), agents));

        let modes = load_agent_modes(dir)?;
        acc.config.agent = Some(merge_maps(acc.config.agent.take(), modes));

        let discovered = load_plugins(dir);
        merge_plugins(
            &mut acc.config,
            &mut acc.origins,
            discovered,
            &dir_str,
            Scope::Local,
        );
    }

    if let Some(content) = &flags.config_content {
        let source = Source::Virtual {
            source: "OPENCODE_CONFIG_CONTENT".to_string(),
            dir: options.directory.clone(),
        };
        let next = load_config(content, &source, env)?;
        acc.merge(next, "OPENCODE_CONFIG_CONTENT", Scope::Local);
    }

    // Managed system config (e.g. MDM), with plist normalization on macOS.
    if let Some(managed_dir) = managed_config_dir() {
        for source in managed::config_files(&managed_dir) {
            if source.exists() {
                let path = source.to_string_lossy().into_owned();
                acc.merge(load_managed_file(&source, env)?, &path, Scope::Global);
            }
        }
    }

    let mut result = acc.config;

    if let Some(mode_map) = result.mode.clone() {
        for (name, mode) in mode_map {
            let mut value = serde_json::to_value(&mode).expect("agent serializes");
            value["mode"] = Value::String("primary".to_string());
            let agents = result.agent.get_or_insert_with(IndexMap::new);
            let existing = agents
                .get(&name)
                .map(|agent| serde_json::to_value(agent).expect("agent serializes"))
                .unwrap_or(Value::Object(Default::default()));
            let merged = merge_deep(&existing, &value);
            if let Ok(info) = serde_json::from_value(merged) {
                agents.insert(name, info);
            }
        }
    }

    if let Some(permission) = &flags.permission {
        if let Ok(value) = serde_json::from_str::<Value>(permission) {
            let current = result
                .permission
                .as_ref()
                .map(|p| serde_json::to_value(p).expect("permission serializes"))
                .unwrap_or(Value::Object(Default::default()));
            let merged = merge_deep(&current, &value);
            if let Ok(info) = serde_json::from_value(merged) {
                result.permission = Some(info);
            }
        } else {
            tracing::warn!("OPENCODE_PERMISSION contains invalid JSON, skipping");
        }
    }

    if result.tools.is_some() {
        let tools = result.tools.take().unwrap_or_default();
        let mut perms = crate::v1::PermissionInfo::default();
        for (tool, enabled) in tools {
            let action = if enabled { Action::Allow } else { Action::Deny };
            let key = if tool == "write" || tool == "edit" || tool == "patch" {
                "edit".to_string()
            } else {
                tool.clone()
            };
            perms.insert(key, Rule::Action(action));
        }
        let merged = merge_permissions(&perms, result.permission.as_ref());
        result.permission = Some(merged);
    }

    if result.username.is_none() {
        result.username = Some(options.username.clone().unwrap_or_else(current_username));
    }

    if result.autoshare == Some(true) && result.share.is_none() {
        result.share = Some(crate::v1::Share::Auto);
    }

    if flags.disable_autocompact {
        let compaction = result
            .compaction
            .get_or_insert_with(crate::v1::Compaction::default);
        compaction.auto = Some(false);
    }
    if flags.disable_prune {
        let compaction = result
            .compaction
            .get_or_insert_with(crate::v1::Compaction::default);
        compaction.prune = Some(false);
    }

    Ok(InstanceState {
        config: result,
        directories: directories
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect(),
        plugin_origins: acc.origins,
    })
}

fn merge_permissions(
    source: &crate::v1::PermissionInfo,
    target: Option<&crate::v1::PermissionInfo>,
) -> crate::v1::PermissionInfo {
    let mut out = source.clone();
    if let Some(target) = target {
        out.assign(target);
    }
    out
}

fn merge_maps<K, V>(target: Option<IndexMap<K, V>>, source: IndexMap<K, V>) -> IndexMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: serde::Serialize + serde::de::DeserializeOwned + Clone,
{
    let mut out = target.unwrap_or_default();
    for (key, value) in source {
        let value = match out.get(&key) {
            Some(existing) => {
                let t = serde_json::to_value(existing).expect("serializes");
                let s = serde_json::to_value(&value).expect("serializes");
                serde_json::from_value(merge_deep(&t, &s)).unwrap_or(value)
            }
            None => value,
        };
        out.insert(key, value);
    }
    out
}

/// Loads `~/.config/opencode` configs (`Config.loadGlobal`): seeds a schema
/// file, merges `config.json` → `opencode.json` → `opencode.jsonc`, and
/// migrates the legacy `config` TOML file.
pub fn load_global(env: Option<&IndexMap<String, String>>) -> Result<Info> {
    let flags = Flags::from_env();
    let dir = paths::config_dir();

    if flags.config.is_none() && flags.config_dir.is_none() && flags.config_content.is_none() {
        let file = global_config_file();
        if !file.exists() {
            let content = serde_json::json!({ "$schema": "https://opencode.ai/config.json" });
            let _ = std::fs::create_dir_all(dir.clone());
            let _ = std::fs::write(
                &file,
                serde_json::to_string_pretty(&content).expect("serializes"),
            );
        }
    }

    let mut result = Info::default();
    for name in ["config.json", "opencode.json", "opencode.jsonc"] {
        let path = dir.join(name);
        let path_str = path.to_string_lossy().into_owned();
        let loaded = load_file(&path_str, env)?;
        result = merge_config(&result, &loaded);
    }

    let legacy = dir.join("config");
    if legacy.exists() {
        if let Ok(next) = migrate_legacy_toml(&legacy) {
            result = next;
        }
    }

    Ok(result)
}

fn migrate_legacy_toml(legacy: &std::path::Path) -> Result<Info> {
    let text = std::fs::read_to_string(legacy)?;
    let value: toml::Value = toml::from_str(&text).map_err(|e| {
        ConfigError::invalid(legacy.to_string_lossy(), Vec::new(), Some(e.to_string()))
    })?;
    let mut map = match serde_json::to_value(value).expect("toml serializes") {
        Value::Object(map) => map,
        _ => return Ok(Info::default()),
    };
    let provider = map
        .shift_remove("provider")
        .and_then(|v| v.as_str().map(String::from));
    let model = map
        .shift_remove("model")
        .and_then(|v| v.as_str().map(String::from));
    if let (Some(provider), Some(model)) = (provider, model) {
        map.insert(
            "model".to_string(),
            Value::String(format!("{provider}/{model}")),
        );
    }
    map.insert(
        "$schema".to_string(),
        Value::String("https://opencode.ai/config.json".to_string()),
    );
    let next = parse::schema(Value::Object(map), &legacy.to_string_lossy())?;
    let _ = std::fs::write(
        legacy.with_file_name("config.json"),
        serde_json::to_string_pretty(&next).expect("serializes"),
    );
    let _ = std::fs::remove_file(legacy);
    Ok(next)
}

/// First existing global config file, or `opencode.jsonc` when none exist.
pub fn global_config_file() -> std::path::PathBuf {
    let dir = paths::config_dir();
    for name in ["opencode.jsonc", "opencode.json", "config.json"] {
        let candidate = dir.join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    dir.join("opencode.jsonc")
}

/// `ensureGitignore` — creates the directory and a `.gitignore` covering npm
/// artifacts.
fn ensure_gitignore(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(
            gitignore,
            "node_modules\npackage.json\npackage-lock.json\nbun.lock\n.gitignore\n",
        )?;
    }
    Ok(())
}

/// `ConfigManaged.managedConfigDir()` — system-managed config directory.
pub fn managed_config_dir() -> Option<std::path::PathBuf> {
    managed::managed_config_dir()
}

fn load_managed_file(
    path: &std::path::Path,
    env: Option<&IndexMap<String, String>>,
) -> Result<Info> {
    let display = path.to_string_lossy().into_owned();
    let text = managed::read_config(path).map_err(|error| ConfigError::Io {
        path: display.clone(),
        error: error.to_string(),
    })?;
    if text.trim().is_empty() {
        return Ok(Info::default());
    }
    let mut config = load_config(
        &text,
        &Source::Path {
            path: display.clone(),
        },
        env,
    )?;
    resolve_loaded_plugins(&mut config, &display);
    Ok(config)
}

/// `ConfigCommand.load(dir)` — discovers `{command,commands}/**/*.md`.
pub fn load_commands(dir: &std::path::Path) -> Result<IndexMap<String, command::Info>> {
    let mut result = IndexMap::new();
    for item in glob::scan("{command,commands}/**/*.md", dir) {
        let content = match std::fs::read_to_string(&item) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let Some((data, body)) = crate::v2::markdown::parse(&content) else {
            continue;
        };
        let relative = item
            .strip_prefix(dir)
            .unwrap_or(&item)
            .to_string_lossy()
            .into_owned();
        let name = config_entry_name_from_path(&relative, &["command/", "commands/"]);
        let mut map = data;
        map.insert("template".to_string(), Value::String(body));
        let parsed =
            match serde_json::from_value::<command::Info>(Value::Object(map.into_iter().collect()))
            {
                Ok(info) => info,
                Err(error) => {
                    return Err(ConfigError::invalid(
                        item.to_string_lossy(),
                        vec![Issue::new(error.to_string(), Vec::new())],
                        None,
                    ));
                }
            };
        result.insert(name, parsed);
    }
    Ok(result)
}

/// `ConfigAgent.load(dir)` — discovers `{agent,agents}/**/*.md`.
pub fn load_agents(dir: &std::path::Path) -> Result<IndexMap<String, agent::Info>> {
    let mut result = IndexMap::new();
    for item in glob::scan("{agent,agents}/**/*.md", dir) {
        let content = match std::fs::read_to_string(&item) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let Some((data, body)) = crate::v2::markdown::parse(&content) else {
            continue;
        };
        let relative = item
            .strip_prefix(dir)
            .unwrap_or(&item)
            .to_string_lossy()
            .into_owned();
        let name = config_entry_name_from_path(&relative, &["agent/", "agents/"]);
        let info = agent::Info::from_parts(name.clone(), data, body).map_err(|error| {
            ConfigError::invalid(
                item.to_string_lossy(),
                vec![Issue::new(error.to_string(), Vec::new())],
                None,
            )
        })?;
        result.insert(name, info);
    }
    Ok(result)
}

/// `ConfigAgent.loadMode(dir)` — discovers `{mode,modes}/*.md`, forcing
/// `mode: "primary"`. Invalid files are skipped silently.
pub fn load_agent_modes(dir: &std::path::Path) -> Result<IndexMap<String, agent::Info>> {
    let mut result = IndexMap::new();
    for item in glob::scan("{mode,modes}/*.md", dir) {
        let content = match std::fs::read_to_string(&item) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let Some((data, body)) = crate::v2::markdown::parse(&content) else {
            continue;
        };
        let relative = item
            .strip_prefix(dir)
            .unwrap_or(&item)
            .to_string_lossy()
            .into_owned();
        let name = config_entry_name_from_path(&relative, &["mode/", "modes/"]);
        let Ok(mut info) = agent::Info::from_parts(name.clone(), data, body) else {
            continue;
        };
        info.mode = Some(agent::Mode::Primary);
        result.insert(name, info);
    }
    Ok(result)
}

/// `ConfigPlugin.load(dir)` — discovers `{plugin,plugins}/*.{ts,js}` as file
/// URLs.
pub fn load_plugins(dir: &std::path::Path) -> Vec<Spec> {
    glob::scan("{plugin,plugins}/*.{ts,js}", dir)
        .into_iter()
        .map(|path| Spec::Package(path_to_file_url(&path)))
        .collect()
}

fn path_to_file_url(path: &std::path::Path) -> String {
    let path = path.to_string_lossy();
    format!("file://{}", path.replace('\\', "/"))
}

/// `ConfigPlugin.resolveLoadedPlugins` — resolves path-like plugin specs
/// relative to the config file that declared them.
fn resolve_loaded_plugins(config: &mut Info, filepath: &str) {
    let Some(plugins) = config.plugin.take() else {
        return;
    };
    let resolved = plugins
        .into_iter()
        .map(|plugin| resolve_plugin_spec(plugin, filepath))
        .collect();
    config.plugin = Some(resolved);
}

/// `ConfigPlugin.resolvePluginSpec` — keeps package specs, resolves path-like
/// specs to `file://` URLs.
pub fn resolve_plugin_spec(plugin: Spec, config_filepath: &str) -> Spec {
    let specifier = plugin_specifier(&plugin);
    if !is_path_plugin_spec(specifier) {
        return plugin;
    }
    let base = std::path::Path::new(config_filepath)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let file = if specifier.starts_with("file://") {
        specifier.to_string()
    } else if std::path::Path::new(specifier).is_absolute() {
        path_to_file_url(std::path::Path::new(specifier))
    } else {
        path_to_file_url(&base.join(specifier))
    };
    let resolved = resolve_path_plugin_target(&file).unwrap_or(file);
    match plugin {
        Spec::Package(_) => Spec::Package(resolved),
        Spec::Entry((_, options)) => Spec::Entry((resolved, options)),
        Spec::Object { options, .. } => Spec::Object {
            package: resolved,
            options,
        },
    }
}

pub fn plugin_specifier(plugin: &Spec) -> &str {
    match plugin {
        Spec::Package(package) => package,
        Spec::Entry((package, _)) => package,
        Spec::Object { package, .. } => package,
    }
}

pub fn plugin_options(plugin: &Spec) -> Option<&IndexMap<String, Value>> {
    match plugin {
        Spec::Package(_) => None,
        Spec::Entry((_, options)) => Some(options),
        Spec::Object { options, .. } if options.is_empty() => None,
        Spec::Object { options, .. } => Some(options),
    }
}

/// `isPathPluginSpec`.
fn is_path_plugin_spec(spec: &str) -> bool {
    spec.starts_with("file://")
        || spec.starts_with('.')
        || std::path::Path::new(spec).is_absolute()
        || is_windows_absolute(spec)
}

fn is_windows_absolute(spec: &str) -> bool {
    let bytes = spec.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && bytes[0].is_ascii_alphabetic()
}

/// `resolvePathPluginTarget` — directories resolve to their own URL (with
/// `package.json`) or their index file.
fn resolve_path_plugin_target(spec: &str) -> std::io::Result<String> {
    let raw = spec
        .strip_prefix("file://")
        .map(|s| s.replace('/', std::path::MAIN_SEPARATOR_STR))
        .unwrap_or_else(|| spec.to_string());
    let path = std::path::PathBuf::from(&raw);
    if !path.is_dir() {
        if spec.starts_with("file://") {
            return Ok(spec.to_string());
        }
        return Ok(path_to_file_url(&path));
    }
    if path.join("package.json").exists() {
        return Ok(path_to_file_url(&path));
    }
    const INDEX_FILES: [&str; 5] = [
        "index.ts",
        "index.tsx",
        "index.js",
        "index.mjs",
        "index.cjs",
    ];
    for name in INDEX_FILES {
        let index = path.join(name);
        if index.exists() {
            return Ok(path_to_file_url(&index));
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "Plugin directory {} is missing package.json or index file",
            path.display()
        ),
    ))
}

/// `ConfigPlugin.deduplicatePluginOrigins` — keep the last occurrence of each
/// plugin identity (package name for npm specs, exact URL for local files).
fn merge_plugins(
    config: &mut Info,
    origins: &mut Vec<PluginOrigin>,
    list: Vec<Spec>,
    source: &str,
    scope: Scope,
) {
    if list.is_empty() {
        return;
    }
    origins.extend(list.into_iter().map(|spec| PluginOrigin {
        spec,
        source: source.to_string(),
        scope,
    }));
    *origins = dedupe_keep_last(std::mem::take(origins), |origin| {
        plugin_identity(&origin.spec)
    });
    config.plugin = Some(origins.iter().map(|origin| origin.spec.clone()).collect());
}

/// Plugin identity: exact URL for local specs, package name for npm specs.
fn plugin_identity(spec: &Spec) -> String {
    let specifier = plugin_specifier(spec);
    if specifier.starts_with("file://") {
        return specifier.to_string();
    }
    package_name(specifier)
}

fn package_name(spec: &str) -> String {
    let spec = spec.strip_prefix("npm:").unwrap_or(spec);
    if let Some(rest) = spec.strip_prefix('@') {
        let Some(slash) = rest.find('/') else {
            return spec.to_string();
        };
        let name = &rest[slash + 1..];
        let name = name.split('@').next().unwrap_or(name);
        return format!("@{}/{}", &rest[..slash], name);
    }
    spec.split('@').next().unwrap_or(spec).to_string()
}

/// `os.userInfo().username || "user"`.
pub fn current_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names() {
        assert_eq!(package_name("opencode-ai/plugin"), "opencode-ai/plugin");
        assert_eq!(package_name("oh-my-opencode@2.4.3"), "oh-my-opencode");
        assert_eq!(package_name("@scope/pkg"), "@scope/pkg");
        assert_eq!(package_name("@scope/pkg@1.0.0"), "@scope/pkg");
        assert_eq!(package_name("npm:foo@1.0.0"), "foo");
    }

    #[test]
    fn plugin_dedupe_keeps_last() {
        let plugins = [
            "global-plugin@1.0.0",
            "shared-plugin@1.0.0",
            "local-plugin@2.0.0",
            "shared-plugin@2.0.0",
        ]
        .into_iter()
        .map(|p| Spec::Package(p.to_string()))
        .collect::<Vec<_>>();
        let mut origins = plugins
            .iter()
            .map(|spec| PluginOrigin {
                spec: spec.clone(),
                source: "".to_string(),
                scope: Scope::Global,
            })
            .collect::<Vec<_>>();
        origins = dedupe_keep_last(std::mem::take(&mut origins), |o| plugin_identity(&o.spec));
        let names = origins
            .iter()
            .map(|o| plugin_specifier(&o.spec).to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "global-plugin@1.0.0",
                "local-plugin@2.0.0",
                "shared-plugin@2.0.0"
            ]
        );
    }

    #[test]
    fn schema_insertion() {
        let text = "{\n  \"model\": \"x\"\n}";
        let updated = insert_schema_header(text, "https://opencode.ai/config.json");
        assert!(updated.starts_with("{\n  \"$schema\": \"https://opencode.ai/config.json\","));
        assert!(updated.contains("\"model\": \"x\""));
    }
}
