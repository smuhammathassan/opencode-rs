/// From reference/packages/tui/src/util/error.ts
///
/// Formats arbitrary errors the same way the reference does. Effect errors
/// arrive here as `serde_json::Value` objects (tagged with `_tag`/`name`); plain
/// std errors are handled directly.
use serde_json::{Map, Value};

use crate::util::record::is_record;

pub enum AnyError<'a> {
    Std(&'a dyn std::error::Error),
    Value(&'a Value),
}

fn field<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

fn tagged<'a>(input: &'a Value, tag: &str) -> Option<&'a Map<String, Value>> {
    let map = input.as_object()?;
    if map.get("_tag").and_then(Value::as_str) == Some(tag) {
        Some(map)
    } else {
        None
    }
}

fn named(input: &Value, name: &str) -> bool {
    match input {
        Value::Object(map) => {
            map.get("name").and_then(Value::as_str) == Some(name)
                || map.get("_tag").and_then(Value::as_str) == Some(name)
        }
        _ => false,
    }
}

fn config_data<'a>(input: &'a Value, tag: &str) -> Option<&'a Map<String, Value>> {
    let map = input.as_object()?;
    if map.get("name").and_then(Value::as_str) == Some(tag) {
        if let Some(Value::Object(data)) = map.get("data") {
            return Some(data);
        }
    }
    if map.get("_tag").and_then(Value::as_str) == Some(tag) {
        return Some(map);
    }
    None
}

/// Mirrors JS `String(value)` for the reference's fallback text formatting.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{f}")
                }
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// From reference/packages/tui/src/util/error.ts (`errorFormat`)
pub fn error_format(error: &AnyError<'_>) -> String {
    match error {
        AnyError::Std(e) => {
            let message = e.to_string();
            if message.is_empty() {
                "Error".to_string()
            } else {
                message
            }
        }
        AnyError::Value(value) => match value {
            Value::Object(map) => {
                if map.is_empty() {
                    "Error (no message)".to_string()
                } else {
                    serde_json::to_string_pretty(value)
                        .unwrap_or_else(|_| "Unexpected error (unserializable)".to_string())
                }
            }
            Value::Array(items) => {
                if items.is_empty() {
                    "Error (no message)".to_string()
                } else {
                    serde_json::to_string_pretty(value)
                        .unwrap_or_else(|_| "Unexpected error (unserializable)".to_string())
                }
            }
            other => js_string(other),
        },
    }
}

/// From reference/packages/tui/src/util/error.ts (`errorMessage`)
pub fn error_message(error: &AnyError<'_>) -> String {
    if let AnyError::Std(e) = error {
        let message = e.to_string();
        if !message.is_empty() {
            return message;
        }
        return "Error".to_string();
    }

    let value = match error {
        AnyError::Value(v) => v,
        AnyError::Std(_) => unreachable!(),
    };
    if let Value::Object(map) = value {
        if let Some(message) = field(map, "message").filter(|m| !m.is_empty()) {
            return message.to_string();
        }
        if let Some(Value::Object(data)) = map.get("data") {
            if let Some(message) = field(data, "message").filter(|m| !m.is_empty()) {
                return message.to_string();
            }
        }
    }

    let text = js_string(value);
    if !text.is_empty() && text != "[object Object]" {
        return text;
    }

    let formatted = error_format(error);
    if !formatted.is_empty() {
        return formatted;
    }
    "unknown error".to_string()
}

/// From reference/packages/tui/src/util/error.ts (`errorData`)
pub fn error_data(error: &AnyError<'_>) -> Value {
    match error {
        AnyError::Std(e) => {
            let mut map = Map::new();
            map.insert("type".into(), Value::String("Error".to_string()));
            map.insert("message".into(), Value::String(error_message(error)));
            map.insert("formatted".into(), Value::String(error_format(error)));
            if let Some(cause) = e.source() {
                map.insert("cause".into(), Value::String(cause.to_string()));
            }
            Value::Object(map)
        }
        AnyError::Value(value) => {
            if !is_record(value) {
                let mut map = Map::new();
                map.insert("type".into(), Value::String(js_type(value).to_string()));
                map.insert("message".into(), Value::String(error_message(error)));
                map.insert("formatted".into(), Value::String(error_format(error)));
                return Value::Object(map);
            }

            let mut data = Map::new();
            if let Value::Object(map) = value {
                for (key, value) in map {
                    let out = match value {
                        Value::String(_) | Value::Number(_) | Value::Bool(_) => value.clone(),
                        Value::Null => Value::String("null".to_string()),
                        _ => Value::String(js_string(value)),
                    };
                    data.insert(key.clone(), out);
                }
            }
            if !data
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|m| !m.is_empty())
            {
                data.insert("message".into(), Value::String(error_message(error)));
            }
            if !data
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| !t.is_empty())
            {
                data.insert("type".into(), Value::String("Object".to_string()));
            }
            data.insert("formatted".into(), Value::String(error_format(error)));
            Value::Object(data)
        }
    }
}

fn js_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) | Value::Object(_) => "object",
    }
}

/// From reference/packages/tui/src/util/error.ts (`cliErrorMessage`)
pub fn cli_error_message(input: &AnyError<'_>) -> Option<String> {
    let value = match input {
        AnyError::Value(v) => v,
        AnyError::Std(e) => {
            let message = e.to_string();
            return if message.is_empty() {
                None
            } else {
                Some(message)
            };
        }
    };

    if let Value::Object(map) = value {
        if let Some(Value::Object(body)) = map.get("body") {
            let nested = cli_error_message(&AnyError::Value(&Value::Object(body.clone())));
            if let Some(nested) = nested {
                return Some(nested);
            }
        }
    }

    if let Some(map) = tagged(value, "CliError") {
        return Some(field(map, "message").unwrap_or_default().to_string());
    }
    if tagged(value, "AccountServiceError").is_some()
        || tagged(value, "AccountTransportError").is_some()
    {
        let map = value.as_object().unwrap();
        return Some(field(map, "message").unwrap_or_default().to_string());
    }

    if let Some(model) = config_data(value, "ProviderModelNotFoundError") {
        let suggestions: Vec<&str> = model
            .get("suggestions")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut lines = vec![format!(
            "Model not found: {}/{}",
            field(model, "providerID").unwrap_or(""),
            field(model, "modelID").unwrap_or("")
        )];
        if !suggestions.is_empty() {
            lines.push(format!("Did you mean: {}", suggestions.join(", ")));
        }
        lines.push("Try: `opencode models` to list available models".to_string());
        lines.push("Or check your config (opencode.json) provider/model names".to_string());
        return Some(lines.join("\n"));
    }

    if let Some(provider) = config_data(value, "ProviderInitError") {
        return Some(format!(
            "Failed to initialize provider \"{}\". Check credentials and configuration.",
            field(provider, "providerID").unwrap_or("")
        ));
    }

    if let Some(json) = config_data(value, "ConfigJsonError") {
        let message = field(json, "message").unwrap_or("");
        let path = field(json, "path").unwrap_or("");
        return Some(format!(
            "Config file at {path} is not valid JSON(C){}",
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        ));
    }

    if let Some(directory) = config_data(value, "ConfigDirectoryTypoError") {
        return Some(format!(
            "Directory \"{}\" in {} is not valid. Rename the directory to \"{}\" or remove it. This is a common typo.",
            field(directory, "dir").unwrap_or(""),
            field(directory, "path").unwrap_or(""),
            field(directory, "suggestion").unwrap_or("")
        ));
    }

    if let Some(frontmatter) = config_data(value, "ConfigFrontmatterError") {
        return Some(
            field(frontmatter, "message")
                .unwrap_or_default()
                .to_string(),
        );
    }

    if let Some(invalid) = config_data(value, "ConfigInvalidError") {
        let path = field(invalid, "path").unwrap_or("");
        let message = field(invalid, "message").unwrap_or("");
        let mut lines = vec![format!(
            "Configuration is invalid{}{}",
            if path.is_empty() || path == "config" {
                String::new()
            } else {
                format!(" at {path}")
            },
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        )];
        if let Some(issues) = invalid.get("issues").and_then(Value::as_array) {
            for issue in issues {
                if let Value::Object(map) = issue {
                    if let Some(issue_message) = field(map, "message") {
                        let path = map
                            .get("path")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .collect::<Vec<_>>()
                                    .join(".")
                            })
                            .unwrap_or_default();
                        lines.push(format!("↳ {issue_message} {path}"));
                    }
                }
            }
        }
        return Some(lines.join("\n"));
    }

    if tagged(value, "UICancelledError").is_some() || named(value, "UICancelledError") {
        return Some(String::new());
    }
    if is_record(value) && named(value, "MCPFailed") {
        let name = value
            .as_object()
            .and_then(|map| map.get("data"))
            .and_then(Value::as_object)
            .and_then(|data| field(data, "name"))
            .unwrap_or("");
        return Some(format!(
            "MCP server \"{name}\" failed. Note, opencode does not support MCP authentication yet."
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_prefers_std_error_display() {
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        assert_eq!(error_message(&AnyError::Std(&error)), "file not found");
    }

    #[test]
    fn message_from_tagged_object() {
        let value = json!({ "_tag": "CliError", "message": "boom" });
        assert_eq!(error_message(&AnyError::Value(&value)), "boom");
    }

    #[test]
    fn message_from_data_object() {
        let value = json!({ "name": "ProviderInitError", "data": { "message": "no creds" } });
        assert_eq!(error_message(&AnyError::Value(&value)), "no creds");
    }

    #[test]
    fn message_falls_back_to_string() {
        assert_eq!(
            error_message(&AnyError::Value(&json!("plain string"))),
            "plain string"
        );
        assert_eq!(error_message(&AnyError::Value(&json!(42))), "42");
        assert_eq!(
            error_message(&AnyError::Value(&json!({}))),
            "Error (no message)"
        );
    }

    #[test]
    fn format_prints_pretty_json_for_objects() {
        let value = json!({ "_tag": "Foo", "message": "bar" });
        let formatted = error_format(&AnyError::Value(&value));
        assert!(formatted.contains("\"message\": \"bar\""));
    }

    #[test]
    fn format_falls_back_for_empty_object() {
        assert_eq!(
            error_format(&AnyError::Value(&json!({}))),
            "Error (no message)"
        );
    }

    #[test]
    fn data_mirrors_own_properties() {
        let value = json!({ "_tag": "Foo", "code": 7, "ok": true, "detail": { "x": 1 } });
        let data = error_data(&AnyError::Value(&value));
        assert_eq!(data["code"], json!(7));
        assert_eq!(data["ok"], json!(true));
        assert_eq!(data["detail"], json!("[object Object]"));
        assert_eq!(data["type"], json!("Object"));
        assert!(data["formatted"].as_str().is_some());
    }

    #[test]
    fn data_for_std_error() {
        let error = std::io::Error::other("boom");
        let data = error_data(&AnyError::Std(&error));
        assert_eq!(data["message"], json!("boom"));
        assert!(data["formatted"].as_str().is_some());
    }

    #[test]
    fn cli_error_for_model_not_found() {
        let value = json!({
            "name": "ProviderModelNotFoundError",
            "data": {
                "providerID": "openai",
                "modelID": "gpt-nope",
                "suggestions": ["gpt-4o", "gpt-4o-mini"]
            }
        });
        let message = cli_error_message(&AnyError::Value(&value)).unwrap();
        assert!(message.starts_with("Model not found: openai/gpt-nope"));
        assert!(message.contains("Did you mean: gpt-4o, gpt-4o-mini"));
        assert!(message.contains("Try: `opencode models`"));
    }

    #[test]
    fn cli_error_for_config_invalid() {
        let value = json!({
            "_tag": "ConfigInvalidError",
            "path": "opencode.json",
            "message": "bad schema",
            "issues": [{ "message": "must be a string", "path": ["model"] }]
        });
        let message = cli_error_message(&AnyError::Value(&value)).unwrap();
        assert!(message.contains("Configuration is invalid at opencode.json: bad schema"));
        assert!(message.contains("↳ must be a string model"));
    }

    #[test]
    fn cli_error_undefined_for_unknown() {
        assert_eq!(
            cli_error_message(&AnyError::Value(&json!({ "foo": 1 }))),
            None
        );
    }
}
