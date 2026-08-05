// From reference/packages/opencode/src/config/parse.ts

use crate::error::{ConfigError, Issue};
use crate::v1::Info;
use serde_json::Value;

/// Parses JSONC text (comments + trailing commas allowed). Mirrors
/// `ConfigParse.jsonc`. Note: the `json5` crate is slightly more permissive
/// than `jsonc-parser` (it also accepts single-quoted strings and unquoted
/// keys), which matches opencode's JSON5-compatible config format.
pub fn jsonc(text: &str, source: &str) -> Result<Value, ConfigError> {
    match json5::from_str::<Value>(text) {
        Ok(value) => Ok(value),
        Err(json5::Error::Message { msg, location }) => {
            let mut issues = String::new();
            if let Some(location) = location {
                let line = location.line;
                let column = location.column;
                let problem_line = text.lines().nth(line.saturating_sub(1)).unwrap_or("");
                let error_text = format!("{msg} at line {line}, column {column}");
                issues.push_str(&format!(
                    "{error_text}\n   Line {line}: {problem_line}\n{}^",
                    " ".repeat(column.saturating_sub(1) + 9)
                ));
            } else {
                issues.push_str(&msg);
            }
            let message = format!("\n--- JSONC Input ---\n{text}\n--- Errors ---\n{issues}\n--- End ---");
            Err(ConfigError::json(source, message))
        }
    }
}

/// Top-level keys allowed by `ConfigV1.Info`.
pub const KNOWN_TOP_LEVEL_KEYS: &[&str] = &[
    "$schema",
    "shell",
    "logLevel",
    "server",
    "command",
    "skills",
    "references",
    "reference",
    "watcher",
    "snapshot",
    "plugin",
    "share",
    "autoshare",
    "autoupdate",
    "disabled_providers",
    "enabled_providers",
    "model",
    "small_model",
    "default_agent",
    "subagent_depth",
    "username",
    "mode",
    "agent",
    "provider",
    "mcp",
    "formatter",
    "lsp",
    "instructions",
    "layout",
    "permission",
    "tools",
    "attachment",
    "enterprise",
    "tool_output",
    "compaction",
    "experimental",
];

/// Validates and decodes raw data against `ConfigV1.Info`. Mirrors
/// `ConfigParse.schema`: unknown top-level keys are rejected before decoding.
pub fn schema(data: Value, source: &str) -> Result<Info, ConfigError> {
    let extra = top_level_extra_keys(&data);
    if !extra.is_empty() {
        let message = format!(
            "Unrecognized key{}: {}",
            if extra.len() == 1 { "" } else { "s" },
            extra.join(", ")
        );
        let issue = Issue {
            message,
            path: Vec::new(),
            code: Some("unrecognized_keys".to_string()),
            keys: Some(extra),
        };
        return Err(ConfigError::invalid(source, vec![issue], None));
    }

    let value = serde_path_to_error::deserialize::<_, Info>(data).map_err(|error| {
        let path = error
            .path()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let issue = Issue {
            message: error.inner().to_string(),
            path,
            code: None,
            keys: None,
        };
        ConfigError::invalid(source, vec![issue], None)
    })?;

    if let Some(lsp::Info::ByLanguage(servers)) = &value.lsp {
        if let Some(message) = crate::v1::lsp::requires_extensions(servers) {
            let issue = Issue::new(message, vec!["lsp".to_string()]);
            return Err(ConfigError::invalid(source, vec![issue], None));
        }
    }

    Ok(value)
}

use crate::v1::lsp;

/// Replicates `topLevelExtraKeys` in `ConfigParse.schema`.
fn top_level_extra_keys(data: &Value) -> Vec<String> {
    let Value::Object(map) = data else {
        return Vec::new();
    };
    map.keys()
        .filter(|key| !KNOWN_TOP_LEVEL_KEYS.contains(&key.as_str()))
        .cloned()
        .collect()
}
