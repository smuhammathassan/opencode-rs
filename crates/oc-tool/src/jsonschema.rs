//! Port of `reference/packages/opencode/src/tool/json-schema.ts`.
//!
//! Converts a tool `Schema` into the JSON Schema document the LLM sees, with
//! effect's `additionalProperties: true` option, then normalizes and inlines
//! local `$defs` references for provider compatibility.

use serde_json::Value as JsonValue;

use crate::schema::{to_document, Schema};

pub const META_SCHEMA_URI_DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";
const MIN_SAFE_INTEGER: i64 = -9007199254740991;
const MAX_SAFE_INTEGER: i64 = 9007199254740991;

/// `fromSchema` from `reference/packages/opencode/src/tool/json-schema.ts:8`.
pub fn from_schema(schema: &Schema) -> JsonValue {
    let mut document = serde_json::Map::new();
    document.insert(
        "$schema".to_string(),
        JsonValue::String(META_SCHEMA_URI_DRAFT_2020_12.to_string()),
    );
    if let JsonValue::Object(base) = to_document(schema, true) {
        for (key, value) in base {
            document.insert(key, value);
        }
    }
    let result = normalize(&JsonValue::Object(document));
    let inlined = drop_definitions_if_resolved(&inline_local_references(&result));
    if !is_json_schema(&inlined) {
        panic!("tool JSON Schema helper produced a non-schema value");
    }
    inlined
}

fn is_record(value: &JsonValue) -> bool {
    value.is_object()
}

fn is_json_schema(value: &JsonValue) -> bool {
    value.is_boolean() || is_record(value)
}

fn is_non_finite_number(value: &JsonValue) -> bool {
    value == "NaN" || value == "Infinity" || value == "-Infinity"
}

fn normalize(value: &JsonValue) -> JsonValue {
    normalize_with(value, false)
}

fn normalize_with(value: &JsonValue, strip_null: bool) -> JsonValue {
    if value.is_array() {
        return JsonValue::Array(value.as_array().unwrap().iter().map(normalize).collect());
    }
    if !is_record(value) {
        return value.clone();
    }

    let object = value.as_object().unwrap();
    let required = object
        .get("required")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect::<std::collections::HashSet<_>>()
        });

    let mut schema = serde_json::Map::new();
    for (key, item) in object {
        let normalized = if key == "properties" && is_record(item) {
            let props = item.as_object().unwrap();
            let mut next = serde_json::Map::new();
            for (name, property) in props {
                let strip = required
                    .as_ref()
                    .is_none_or(|required| !required.contains(name));
                next.insert(name.clone(), normalize_with(property, strip));
            }
            JsonValue::Object(next)
        } else {
            normalize(item)
        };
        schema.insert(key.clone(), normalized);
    }

    if schema.get("additionalProperties") == Some(&JsonValue::Bool(true)) {
        schema.remove("additionalProperties");
    }

    if strip_null && schema.get("anyOf").and_then(|v| v.as_array()).is_some() {
        let any_of = schema.get("anyOf").unwrap().as_array().unwrap();
        let without_null: Vec<JsonValue> = any_of
            .iter()
            .filter(|item| {
                !(is_record(item) && item.get("type") == Some(&JsonValue::String("null".into())))
            })
            .cloned()
            .collect();
        if without_null.len() != any_of.len() {
            let mut next = schema.clone();
            next.insert("anyOf".to_string(), JsonValue::Array(without_null));
            return normalize_with(&JsonValue::Object(next), strip_null);
        }
    }

    if schema.get("anyOf").and_then(|v| v.as_array()).is_some() {
        let any_of = schema.get("anyOf").unwrap().as_array().unwrap();
        let number = any_of.iter().find(|item| {
            is_record(item) && item.get("type") == Some(&JsonValue::String("number".into()))
        });
        let non_finite: Vec<&JsonValue> = any_of
            .iter()
            .filter(|item| {
                is_record(item)
                    && item
                        .get("enum")
                        .and_then(|e| e.as_array())
                        .map(|entries| entries.iter().all(is_non_finite_number))
                        .unwrap_or(false)
            })
            .collect();
        if let Some(number) = number {
            if non_finite.len() == any_of.len().saturating_sub(1) {
                let mut rest = schema.clone();
                rest.remove("anyOf");
                if let JsonValue::Object(mut merged) = number.clone() {
                    for (key, value) in rest {
                        merged.insert(key, value);
                    }
                    return normalize_with(&JsonValue::Object(merged), strip_null);
                }
            }
        }

        if is_empty_struct_union(any_of) {
            let mut rest = schema.clone();
            rest.remove("anyOf");
            rest.insert("type".to_string(), JsonValue::String("object".into()));
            rest.insert(
                "properties".to_string(),
                JsonValue::Object(Default::default()),
            );
            return normalize_with(&JsonValue::Object(rest), strip_null);
        }

        if any_of.len() == 1 && is_record(&any_of[0]) {
            let mut rest = schema.clone();
            rest.remove("anyOf");
            if let JsonValue::Object(mut merged) = any_of[0].clone() {
                for (key, value) in rest {
                    merged.insert(key, value);
                }
                return normalize_with(&JsonValue::Object(merged), strip_null);
            }
        }
    }

    if schema.get("allOf").and_then(|v| v.as_array()).is_some() {
        let all_of = schema.get("allOf").unwrap().as_array().unwrap();
        let all_records = all_of.iter().all(is_record);
        if all_records && can_flatten_all_of(all_of, &schema) {
            let mut rest = schema.clone();
            rest.remove("allOf");
            let mut merged = serde_json::Map::new();
            for item in all_of {
                if let JsonValue::Object(obj) = item {
                    for (key, value) in obj {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
            for (key, value) in rest {
                merged.insert(key, value);
            }
            return normalize_with(&JsonValue::Object(merged), strip_null);
        }
    }

    if schema.get("type") == Some(&JsonValue::String("integer".into()))
        && schema.get("maximum").is_none()
    {
        let mut next = schema.clone();
        if !next.contains_key("minimum") {
            next.insert("minimum".to_string(), JsonValue::from(MIN_SAFE_INTEGER));
        }
        next.insert("maximum".to_string(), JsonValue::from(MAX_SAFE_INTEGER));
        return JsonValue::Object(next);
    }

    JsonValue::Object(schema)
}

fn is_empty_struct_union(items: &[JsonValue]) -> bool {
    items.len() == 2
        && items.iter().any(|item| {
            is_record(item)
                && item.get("type") == Some(&JsonValue::String("object".into()))
                && item.get("properties").is_none()
        })
        && items.iter().any(|item| {
            is_record(item)
                && item.get("type") == Some(&JsonValue::String("array".into()))
                && item.get("items").is_none()
        })
}

fn can_flatten_all_of(all_of: &[JsonValue], parent: &serde_json::Map<String, JsonValue>) -> bool {
    let mut keys: std::collections::HashSet<String> = parent
        .keys()
        .filter(|key| key.as_str() != "allOf")
        .cloned()
        .collect();
    all_of.iter().all(|item| {
        if let JsonValue::Object(obj) = item {
            obj.keys().all(|key| keys.insert(key.clone()))
        } else {
            false
        }
    })
}

fn inline_local_references(value: &JsonValue) -> JsonValue {
    inline_local_references_with(value, None, &mut std::collections::HashSet::new())
}

fn inline_local_references_with(
    value: &JsonValue,
    definitions: Option<&serde_json::Map<String, JsonValue>>,
    seen: &mut std::collections::HashSet<String>,
) -> JsonValue {
    if value.is_array() {
        return JsonValue::Array(
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|item| inline_local_references_with(item, definitions, seen))
                .collect(),
        );
    }
    if !is_record(value) {
        return value.clone();
    }

    let object = value.as_object().unwrap();
    let local_definitions = match definitions {
        Some(defs) => Some(defs),
        None => object.get("$defs").and_then(|defs| defs.as_object()),
    };

    if let Some(JsonValue::String(reference)) = object.get("$ref") {
        if let Some(definitions) = local_definitions {
            let name = reference
                .strip_prefix("#/$defs/")
                .or_else(|| reference.strip_prefix("#/definitions/"));
            if let Some(name) = name {
                if !seen.contains(name) {
                    if let Some(target) = definitions.get(name) {
                        let mut merged = serde_json::Map::new();
                        if let JsonValue::Object(target) = target {
                            for (key, value) in target {
                                merged.insert(key.clone(), value.clone());
                            }
                        }
                        for (key, value) in object {
                            if key != "$ref" {
                                merged.insert(key.clone(), value.clone());
                            }
                        }
                        let mut next_seen = seen.clone();
                        next_seen.insert(name.to_string());
                        return inline_local_references_with(
                            &JsonValue::Object(merged),
                            Some(definitions),
                            &mut next_seen,
                        );
                    }
                }
            }
        }
    }

    let mut next = serde_json::Map::new();
    for (key, item) in object {
        next.insert(
            key.clone(),
            inline_local_references_with(item, local_definitions, seen),
        );
    }
    JsonValue::Object(next)
}

fn drop_definitions_if_resolved(value: &JsonValue) -> JsonValue {
    if !is_record(value) || has_local_reference(value) {
        return value.clone();
    }
    let object = value.as_object().unwrap();
    let mut next = serde_json::Map::new();
    for (key, item) in object {
        if key == "$defs" || key == "definitions" {
            continue;
        }
        next.insert(key.clone(), item.clone());
    }
    JsonValue::Object(next)
}

fn has_local_reference(value: &JsonValue) -> bool {
    if value.is_array() {
        return value.as_array().unwrap().iter().any(has_local_reference);
    }
    if !is_record(value) {
        return false;
    }
    if let Some(JsonValue::String(reference)) = value.get("$ref") {
        if reference.starts_with("#/$defs/") || reference.starts_with("#/definitions/") {
            return true;
        }
    }
    value.as_object().unwrap().values().any(has_local_reference)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{opt_prop, prop, Schema};

    #[test]
    fn applies_integer_safe_bounds_to_bare_integers() {
        let schema = Schema::struct_(vec![prop("value", Schema::integer())], "test struct");
        let result = from_schema(&schema);
        assert_eq!(
            result.pointer("/properties/value"),
            Some(&serde_json::json!({
                "minimum": MIN_SAFE_INTEGER,
                "maximum": MAX_SAFE_INTEGER,
                "type": "integer"
            }))
        );
    }

    #[test]
    fn preserves_required_nullable_fields() {
        let schema = Schema::struct_(
            vec![prop("value", Schema::null_or(Schema::plain_string()))],
            "test struct",
        );
        let result = from_schema(&schema);
        let any_of = result.pointer("/properties/value/anyOf").unwrap();
        assert!(any_of
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.get("type") == Some(&JsonValue::String("null".into()))));
    }

    #[test]
    fn strips_null_from_optional_properties() {
        let schema = Schema::struct_(
            vec![opt_prop("value", Schema::null_or(Schema::plain_string()))],
            "test struct",
        );
        let result = from_schema(&schema);
        let value = result.pointer("/properties/value").unwrap();
        assert!(value.get("anyOf").is_none());
        assert_eq!(value, &serde_json::json!({ "type": "string" }));
    }

    #[test]
    fn output_keys_are_sorted() {
        let schema = Schema::struct_(
            vec![prop("query", Schema::string("Websearch query"))],
            "websearch",
        );
        let result = from_schema(&schema);
        let pretty = serde_json::to_string_pretty(&result).unwrap();
        let first = pretty.lines().nth(1).unwrap().trim().trim_end_matches(',');
        assert!(first.starts_with("\"$schema\""));
    }
}
