//! Tool-schema projection for provider dialects.
//! From reference/packages/llm/src/protocols/utils/tool-schema.ts

use serde_json::{Map, Value};

use crate::protocols::utils::gemini_tool_schema::convert as gemini_convert;
use crate::schema::ModelToolSchemaCompatibility;
use crate::shared::is_record;

/// `removeNullSchemas(value)`.
/// From reference/packages/llm/src/protocols/utils/tool-schema.ts
fn remove_null_schemas(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(remove_null_schemas).collect()),
        _ if !is_record(value) => value.clone(),
        _ => {
            let obj = value.as_object().unwrap();
            let mut fields = Map::new();
            for (key, field) in obj {
                if key != "anyOf" {
                    fields.insert(key.clone(), remove_null_schemas(field));
                }
            }
            let any_of = obj.get("anyOf");
            if !any_of.map(|v| v.is_array()).unwrap_or(false) {
                return Value::Object(fields);
            }
            let variants: Vec<Value> = any_of
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .filter(|variant| {
                    !(is_record(variant)
                        && variant.get("type").and_then(Value::as_str) == Some("null"))
                })
                .map(remove_null_schemas)
                .collect();
            if variants.len() == 1 && is_record(&variants[0]) {
                let mut merged = fields;
                if let Value::Object(extra) = &variants[0] {
                    for (k, v) in extra {
                        merged.insert(k.clone(), v.clone());
                    }
                }
                return Value::Object(merged);
            }
            fields.insert("anyOf".to_string(), Value::Array(variants));
            Value::Object(fields)
        }
    }
}

fn tuple_items_schema(items: &[Value]) -> Value {
    let projected: Vec<Value> = items.iter().map(moonshot_node).collect();
    match projected.len() {
        0 => Value::Object(Map::new()),
        1 => projected[0].clone(),
        _ => Value::Object(Map::from_iter([(
            "anyOf".to_string(),
            Value::Array(projected),
        )])),
    }
}

fn moonshot_node(schema: &Value) -> Value {
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(moonshot_node).collect()),
        _ if !is_record(schema) => schema.clone(),
        _ => {
            let obj = schema.as_object().unwrap();
            if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
                return Value::Object(Map::from_iter([(
                    "$ref".to_string(),
                    Value::String(reference.to_string()),
                )]));
            }
            let mut result = Map::new();
            for (key, value) in obj {
                if key == "items" && value.is_array() {
                    result.insert(
                        "items".to_string(),
                        tuple_items_schema(value.as_array().unwrap()),
                    );
                } else if key == "prefixItems" {
                    if obj.contains_key("items") {
                        continue;
                    }
                    let items: Vec<Value> = if value.is_array() {
                        value.as_array().cloned().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    result.insert("items".to_string(), tuple_items_schema(&items));
                } else if key == "unevaluatedItems" {
                    continue;
                } else {
                    result.insert(key.clone(), moonshot_node(value));
                }
            }
            Value::Object(result)
        }
    }
}

/// `moonshot(schema)`.
/// From reference/packages/llm/src/protocols/utils/tool-schema.ts
pub fn moonshot(schema: &Value) -> Value {
    let projected = moonshot_node(schema);
    if is_record(&projected) {
        projected
    } else {
        Value::Object(Map::new())
    }
}

/// `openAI(schema)`.
/// From reference/packages/llm/src/protocols/utils/tool-schema.ts
pub fn open_ai(schema: &Value) -> Value {
    let variants: Vec<&Value> = schema
        .get("anyOf")
        .and_then(Value::as_array)
        .map(|array| array.iter().filter(|v| is_record(v)).collect())
        .unwrap_or_default();
    let flattened = if variants.is_empty() {
        let mut obj = schema.as_object().cloned().unwrap_or_default();
        obj.insert("type".to_string(), Value::String("object".to_string()));
        Value::Object(obj)
    } else {
        let mut obj = Map::new();
        if let Some(schema_obj) = schema.as_object() {
            for (k, v) in schema_obj {
                if k != "anyOf" {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        obj.insert("type".to_string(), Value::String("object".to_string()));
        let mut properties = Map::new();
        for variant in &variants {
            if let Some(Value::Object(props)) = variant.get("properties") {
                for (k, v) in props {
                    properties.insert(k.clone(), v.clone());
                }
            }
        }
        obj.insert("properties".to_string(), Value::Object(properties));
        obj.insert("additionalProperties".to_string(), Value::Bool(false));
        Value::Object(obj)
    };
    let normalized = remove_null_schemas(&flattened);
    if is_record(&normalized) {
        normalized
    } else {
        Value::Object(Map::from_iter([(
            "type".to_string(),
            Value::String("object".to_string()),
        )]))
    }
}

/// `gemini(schema)`.
/// From reference/packages/llm/src/protocols/utils/tool-schema.ts
pub fn gemini(schema: &Value) -> Value {
    gemini_convert(schema).unwrap_or_else(|| Value::Object(Map::new()))
}

/// `modelCompatibility(schema, compatibility)`.
/// From reference/packages/llm/src/protocols/utils/tool-schema.ts
pub fn model_compatibility(
    schema: &Value,
    compatibility: Option<ModelToolSchemaCompatibility>,
) -> Value {
    match compatibility {
        None => schema.clone(),
        Some(ModelToolSchemaCompatibility::Gemini) => gemini(schema),
        Some(ModelToolSchemaCompatibility::Moonshot) => moonshot(schema),
    }
}

/// `ToolSchemaProjection`.
/// From reference/packages/llm/src/protocols/utils/tool-schema.ts
pub struct ToolSchemaProjection;

impl ToolSchemaProjection {
    pub fn open_ai(schema: &Value) -> Value {
        open_ai(schema)
    }
    pub fn gemini(schema: &Value) -> Value {
        gemini(schema)
    }
    pub fn moonshot(schema: &Value) -> Value {
        moonshot(schema)
    }
    pub fn model_compatibility(
        schema: &Value,
        compatibility: Option<ModelToolSchemaCompatibility>,
    ) -> Value {
        model_compatibility(schema, compatibility)
    }
}
