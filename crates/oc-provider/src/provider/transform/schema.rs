//! Tool schema sanitization.
//!
//! From `transform.ts`: `schema`, `sanitizeOpenAISchema`, and the Moonshot and
//! Gemini sanitizers.

use serde_json::{json, Map, Value};

use crate::provider::Model;

fn is_plain_object(value: &Value) -> bool {
    matches!(value, Value::Object(_))
}

/// Lowers a JSON Schema to the subset OpenAI tool schemas support.
///
/// From `sanitizeOpenAISchema()` in `transform.ts`.
pub fn sanitize_openai_schema(value: &Value) -> Value {
    const TYPES: [&str; 7] = [
        "string", "number", "boolean", "integer", "object", "array", "null",
    ];
    const COMPOSITION_KEYS: [&str; 3] = ["anyOf", "oneOf", "allOf"];

    if value.is_boolean() {
        return json!({ "type": "string" });
    }
    if let Value::Array(items) = value {
        return Value::Array(items.iter().map(sanitize_openai_schema).collect());
    }
    let Value::Object(obj) = value else {
        return value.clone();
    };

    let mut result: Map<String, Value> = Map::new();
    if let Some(Value::String(r#ref)) = obj.get("$ref") {
        result.insert("$ref".to_string(), Value::from(r#ref.clone()));
    }
    if let Some(Value::String(description)) = obj.get("description") {
        result.insert("description".to_string(), Value::from(description.clone()));
    }
    if obj.contains_key("const") {
        result.insert(
            "enum".to_string(),
            Value::Array(vec![obj.get("const").cloned().unwrap_or(Value::Null)]),
        );
    } else if let Some(Value::Array(enums)) = obj.get("enum") {
        result.insert("enum".to_string(), Value::Array(enums.clone()));
    }

    if let Some(Value::Object(properties)) = obj.get("properties") {
        let mapped = properties
            .iter()
            .map(|(key, item)| (key.clone(), sanitize_openai_schema(item)))
            .collect();
        result.insert("properties".to_string(), Value::Object(mapped));
    }

    if let Some(Value::Array(required)) = obj.get("required") {
        let filtered: Vec<Value> = required
            .iter()
            .filter(|item| item.is_string())
            .cloned()
            .collect();
        result.insert("required".to_string(), Value::Array(filtered));
    }

    if let Some(items) = obj.get("items") {
        result.insert("items".to_string(), sanitize_openai_schema(items));
    }

    if let Some(additional) = obj.get("additionalProperties") {
        let value = if additional.is_boolean() {
            additional.clone()
        } else {
            sanitize_openai_schema(additional)
        };
        result.insert("additionalProperties".to_string(), value);
    }

    for key in COMPOSITION_KEYS {
        if let Some(Value::Array(values)) = obj.get(key) {
            result.insert(
                key.to_string(),
                Value::Array(values.iter().map(sanitize_openai_schema).collect()),
            );
        }
    }

    for key in ["$defs", "definitions"] {
        if let Some(Value::Object(defs)) = obj.get(key) {
            let mapped = defs
                .iter()
                .map(|(name, item)| (name.clone(), sanitize_openai_schema(item)))
                .collect();
            result.insert(key.to_string(), Value::Object(mapped));
        }
    }

    let schema_types: Vec<String> = match obj.get("type") {
        Some(Value::String(t)) => {
            if TYPES.contains(&t.as_str()) {
                vec![t.clone()]
            } else {
                Vec::new()
            }
        }
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(|t| t.as_str())
            .filter(|t| TYPES.contains(t))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };

    let has_ref = matches!(result.get("$ref"), Some(Value::String(_)));
    let has_composition = COMPOSITION_KEYS.iter().any(|key| result.contains_key(*key));
    if schema_types.is_empty() && (has_ref || has_composition) {
        return Value::Object(result);
    }

    let inferred_types: Vec<String> = if !schema_types.is_empty() {
        schema_types
    } else if ["properties", "required", "additionalProperties"]
        .iter()
        .any(|k| obj.contains_key(*k))
    {
        vec!["object".to_string()]
    } else if ["items", "prefixItems"]
        .iter()
        .any(|k| obj.contains_key(*k))
    {
        vec!["array".to_string()]
    } else if result.contains_key("enum") || obj.contains_key("format") {
        vec!["string".to_string()]
    } else if [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
    ]
    .iter()
    .any(|k| obj.contains_key(*k))
    {
        vec!["number".to_string()]
    } else {
        Vec::new()
    };

    if inferred_types.is_empty() {
        return Value::Object(Map::new());
    }

    if inferred_types.len() == 1 {
        result.insert("type".to_string(), Value::from(inferred_types[0].clone()));
    } else {
        result.insert(
            "type".to_string(),
            Value::Array(
                inferred_types
                    .iter()
                    .map(|t| Value::from(t.clone()))
                    .collect(),
            ),
        );
    }
    if inferred_types.contains(&"object".to_string()) && !result.contains_key("properties") {
        result.insert("properties".to_string(), Value::Object(Map::new()));
    }
    if inferred_types.contains(&"array".to_string()) && !result.contains_key("items") {
        result.insert("items".to_string(), json!({ "type": "string" }));
    }
    Value::Object(result)
}

fn sanitize_moonshot(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
        Value::Array(items) => Value::Array(items.iter().map(sanitize_moonshot).collect()),
        Value::Object(obj) => {
            if let Some(Value::String(r#ref)) = obj.get("$ref") {
                return json!({ "$ref": r#ref });
            }
            let mut result: Map<String, Value> = Map::new();
            for (key, value) in obj {
                let sanitized = sanitize_moonshot(value);
                if key == "items" {
                    if let Value::Array(items) = sanitized {
                        result.insert(
                            "items".to_string(),
                            items
                                .into_iter()
                                .next()
                                .unwrap_or(Value::Object(Map::new())),
                        );
                        continue;
                    }
                }
                result.insert(key.clone(), sanitized);
            }
            Value::Object(result)
        }
    }
}

fn has_combiner(node: &Value) -> bool {
    is_plain_object(node)
        && ["anyOf", "oneOf", "allOf"]
            .iter()
            .any(|key| matches!(node.as_object().unwrap().get(*key), Some(Value::Array(_))))
}

fn has_schema_intent(node: &Value) -> bool {
    if !is_plain_object(node) {
        return false;
    }
    let obj = node.as_object().unwrap();
    if has_combiner(node) {
        return true;
    }
    [
        "type",
        "properties",
        "items",
        "prefixItems",
        "enum",
        "const",
        "$ref",
        "additionalProperties",
        "patternProperties",
        "required",
        "not",
        "if",
        "then",
        "else",
    ]
    .iter()
    .any(|key| obj.contains_key(*key))
}

fn sanitize_gemini(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
        Value::Array(items) => Value::Array(items.iter().map(sanitize_gemini).collect()),
        Value::Object(obj) => {
            let mut result: Map<String, Value> = Map::new();
            for (key, value) in obj {
                if key == "enum" && value.is_array() {
                    let strings: Vec<Value> = value
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| Value::from(format!("{}", v)))
                        .collect();
                    result.insert(key.clone(), Value::Array(strings));
                    if result
                        .get("type")
                        .is_some_and(|t| t == "integer" || t == "number")
                    {
                        result.insert("type".to_string(), Value::from("string"));
                    }
                } else if value.is_object() || value.is_array() {
                    result.insert(key.clone(), sanitize_gemini(value));
                } else {
                    result.insert(key.clone(), value.clone());
                }
            }

            if let Some(Value::Array(types)) = result.get("type") {
                let has_null = types.iter().any(|t| t == "null");
                let non_null: Vec<Value> =
                    types.iter().filter(|t| **t != "null").cloned().collect();
                if non_null.is_empty() {
                    result.insert("type".to_string(), Value::from("null"));
                } else {
                    result.remove("type");
                    result.insert(
                        "anyOf".to_string(),
                        Value::Array(non_null.iter().map(|t| json!({ "type": t })).collect()),
                    );
                    if has_null {
                        result.insert("nullable".to_string(), Value::from(true));
                    }
                }
            }

            if result.get("type") == Some(&Value::from("object"))
                && result.contains_key("properties")
                && result.get("required").is_some_and(|r| r.is_array())
            {
                let properties = result.get("properties").unwrap().as_object().unwrap();
                let filtered: Vec<Value> = result
                    .get("required")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|field| {
                        field.is_string() && properties.contains_key(field.as_str().unwrap())
                    })
                    .cloned()
                    .collect();
                result.insert("required".to_string(), Value::Array(filtered));
            }

            if result.get("type") == Some(&Value::from("array"))
                && !has_combiner(&Value::Object(result.clone()))
            {
                if !result.contains_key("items") {
                    result.insert("items".to_string(), Value::Object(Map::new()));
                }
                let items = result.get("items").cloned().unwrap_or(Value::Null);
                if let Some(items) = items.as_object() {
                    if !has_schema_intent(&Value::Object(items.clone())) {
                        result
                            .get_mut("items")
                            .and_then(|items| items.as_object_mut())
                            .map(|items| {
                                items.insert("type".to_string(), Value::from("string"));
                            });
                    }
                }
            }

            let is_object = result.get("type") == Some(&Value::from("object"));
            if result.contains_key("type")
                && !is_object
                && !has_combiner(&Value::Object(result.clone()))
            {
                result.remove("properties");
                result.remove("required");
            }

            Value::Object(result)
        }
    }
}

/// Sanitizes a tool input schema for a model.
///
/// From `schema()` in `transform.ts`.
pub fn schema(model: &Model, input: Value) -> Value {
    let mut result = input;
    if model.api.npm == "@ai-sdk/openai" || model.api.npm == "@ai-sdk/azure" {
        result = sanitize_openai_schema(&result);
    }

    if model.provider_id == "moonshotai" || model.api.id.to_lowercase().contains("kimi") {
        let sanitized = sanitize_moonshot(&result);
        if is_plain_object(&sanitized) {
            result = sanitized;
        }
    }

    if model.provider_id == "google" || model.api.id.contains("gemini") {
        result = sanitize_gemini(&result);
    }

    result
}
