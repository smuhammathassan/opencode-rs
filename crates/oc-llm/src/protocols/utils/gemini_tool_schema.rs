//! Gemini tool-schema sanitization and projection.
//! From reference/packages/llm/src/protocols/utils/gemini-tool-schema.ts

use serde_json::{Map, Value};

use crate::shared::is_record;

const SCHEMA_INTENT_KEYS: &[&str] = &[
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
];

fn has_combiner(schema: &Value) -> bool {
    if !is_record(schema) {
        return false;
    }
    let obj = schema.as_object().unwrap();
    ["anyOf", "oneOf", "allOf"]
        .iter()
        .any(|key| obj.get(*key).map(|v| v.is_array()).unwrap_or(false))
}

fn has_schema_intent(schema: &Value) -> bool {
    if !is_record(schema) {
        return false;
    }
    let obj = schema.as_object().unwrap();
    has_combiner(schema) || SCHEMA_INTENT_KEYS.iter().any(|key| obj.contains_key(*key))
}

fn sanitize_node(schema: &Value) -> Value {
    if !is_record(schema) {
        if let Value::Array(items) = schema {
            return Value::Array(items.iter().map(sanitize_node).collect());
        }
        return schema.clone();
    }
    let obj = schema.as_object().unwrap();
    let mut result = Map::new();
    for (key, value) in obj {
        let value = if key == "enum" && value.is_array() {
            Value::Array(value.as_array().unwrap().iter().map(js_string).collect())
        } else {
            sanitize_node(value)
        };
        result.insert(key.clone(), value);
    }

    if result.get("enum").map(|v| v.is_array()).unwrap_or(false)
        && matches!(
            result.get("type").and_then(Value::as_str),
            Some("integer") | Some("number")
        )
    {
        result.insert("type".to_string(), Value::String("string".to_string()));
    }

    if result.get("type").and_then(Value::as_str) == Some("object") {
        if let Some(Value::Object(properties)) = result.get("properties") {
            if let Some(Value::Array(required)) = result.get("required") {
                let filtered: Vec<Value> = required
                    .iter()
                    .filter(|field| {
                        field
                            .as_str()
                            .map(|name| properties.contains_key(name))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
                result.insert("required".to_string(), Value::Array(filtered));
            }
        }
    }

    if result.get("type").and_then(Value::as_str) == Some("array")
        && !has_combiner(&Value::Object(result.clone()))
    {
        let items = result
            .get("items")
            .cloned()
            .unwrap_or(Value::Object(Map::new()));
        let mut items = items;
        if is_record(&items) && !has_schema_intent(&items) {
            if let Value::Object(mut map) = items {
                map.insert("type".to_string(), Value::String("string".to_string()));
                items = Value::Object(map);
            }
        }
        result.insert("items".to_string(), items);
    }

    if let Some(kind) = result.get("type").and_then(Value::as_str) {
        if kind != "object" && !has_combiner(&Value::Object(result.clone())) {
            result.remove("properties");
            result.remove("required");
        }
    }

    Value::Object(result)
}

fn empty_object_schema(schema: &Map<String, Value>) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
        && !schema.get("properties").map(is_record).unwrap_or(false)
        && !schema.contains_key("additionalProperties")
}

fn project_node(schema: &Value) -> Option<Value> {
    if !is_record(schema) {
        return None;
    }
    let obj = schema.as_object().unwrap();
    if empty_object_schema(obj) {
        return None;
    }

    let mut result = Map::new();

    if let Some(description) = obj.get("description") {
        result.insert("description".to_string(), description.clone());
    }
    if let Some(required) = obj.get("required") {
        result.insert("required".to_string(), required.clone());
    }
    if let Some(format) = obj.get("format") {
        result.insert("format".to_string(), format.clone());
    }
    let type_value = obj.get("type");
    if let Some(Value::Array(types)) = type_value {
        if let Some(first) = types.iter().find(|t| t.as_str() != Some("null")) {
            result.insert("type".to_string(), first.clone());
        }
        if types.iter().any(|t| t.as_str() == Some("null")) {
            result.insert("nullable".to_string(), Value::Bool(true));
        }
    } else if let Some(type_value) = type_value {
        result.insert("type".to_string(), type_value.clone());
    }
    let enum_value = if obj.contains_key("const") {
        Some(Value::Array(vec![obj.get("const").unwrap().clone()]))
    } else {
        obj.get("enum").cloned()
    };
    if let Some(enum_value) = enum_value {
        result.insert("enum".to_string(), enum_value);
    }
    if let Some(Value::Object(properties)) = obj.get("properties") {
        let mut projected = Map::new();
        for (key, value) in properties {
            if let Some(value) = project_node(value) {
                projected.insert(key.clone(), value);
            }
        }
        result.insert("properties".to_string(), Value::Object(projected));
    }
    if let Some(items) = obj.get("items") {
        if let Value::Array(items) = items {
            let projected: Vec<Value> = items.iter().filter_map(project_node).collect();
            result.insert("items".to_string(), Value::Array(projected));
        } else {
            if let Some(items) = project_node(items) {
                result.insert("items".to_string(), items);
            }
        }
    }
    for combiner in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(items)) = obj.get(combiner) {
            let projected: Vec<Value> = items.iter().filter_map(project_node).collect();
            result.insert(combiner.to_string(), Value::Array(projected));
        }
    }
    if let Some(min_length) = obj.get("minLength") {
        result.insert("minLength".to_string(), min_length.clone());
    }

    Some(Value::Object(result))
}

/// JS `String(x)` semantics for enum coercion.
fn js_string(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(s.clone()),
        Value::Number(n) => Value::String(n.to_string()),
        Value::Bool(b) => Value::String(b.to_string()),
        Value::Null => Value::String("null".to_string()),
        _ => Value::String("[object Object]".to_string()),
    }
}

/// `GeminiToolSchema.convert(schema)`.
/// From reference/packages/llm/src/protocols/utils/gemini-tool-schema.ts (`convert`)
pub fn convert(schema: &Value) -> Option<Value> {
    project_node(&sanitize_node(schema))
}
