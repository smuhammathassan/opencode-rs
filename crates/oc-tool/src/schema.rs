//! A tiny `Schema` DSL whose JSON Schema output mirrors what the reference's
//! Effect `Schema` encoders produce (`Schema.toJsonSchemaDocument`), so tool
//! parameter schemas serialize byte-identically to opencode's.
//!
//! From reference/packages/opencode/src/tool/json-schema.ts and the Effect
//! `@opencode-ai/core/schema` primitives (`reference/packages/schema/src/schema.ts`).

use serde_json::Value as JsonValue;

/// A modeled Effect schema.
#[derive(Debug, Clone)]
pub enum Schema {
    String {
        description: Option<String>,
        default: Option<JsonValue>,
    },
    Number {
        description: Option<String>,
        exclusive_minimum: Option<f64>,
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
    Integer {
        description: Option<String>,
        exclusive_minimum: Option<i64>,
        minimum: Option<i64>,
        maximum: Option<i64>,
    },
    Boolean {
        description: Option<String>,
    },
    /// `Schema.Literals([...])` → `{ type: "string", enum: [...] }`.
    Literals {
        values: Vec<String>,
        description: Option<String>,
        default: Option<JsonValue>,
    },
    Array {
        items: Box<Schema>,
        description: Option<String>,
    },
    Struct {
        props: Vec<Property>,
        description: Option<String>,
    },
    /// `Schema.NullOr(x)` → `anyOf: [x, { type: "null" }]`.
    NullOr {
        inner: Box<Schema>,
    },
    /// Raw JSON schema passthrough (plugin tools, code-mode `execute`).
    Raw(JsonValue),
}

/// A single struct property: schema plus required-ness.
#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub schema: Schema,
    pub required: bool,
}

impl Schema {
    pub fn string(description: impl Into<String>) -> Self {
        Schema::String {
            description: Some(description.into()),
            default: None,
        }
    }

    pub fn plain_string() -> Self {
        Schema::String {
            description: None,
            default: None,
        }
    }

    pub fn number() -> Self {
        Schema::Number {
            description: None,
            exclusive_minimum: None,
            minimum: None,
            maximum: None,
        }
    }

    pub fn integer() -> Self {
        Schema::Integer {
            description: None,
            exclusive_minimum: None,
            minimum: None,
            maximum: None,
        }
    }

    pub fn boolean(description: impl Into<String>) -> Self {
        Schema::Boolean {
            description: Some(description.into()),
        }
    }

    pub fn plain_boolean() -> Self {
        Schema::Boolean { description: None }
    }

    pub fn literals(values: &[&str], description: impl Into<String>) -> Self {
        Schema::Literals {
            values: values.iter().map(|v| v.to_string()).collect(),
            description: Some(description.into()),
            default: None,
        }
    }

    /// `Schema.PositiveInt` from `reference/packages/schema/src/schema.ts:3`.
    pub fn positive_int() -> Self {
        Schema::Integer {
            description: None,
            exclusive_minimum: Some(0),
            minimum: None,
            maximum: None,
        }
    }

    /// `Schema.NonNegativeInt` from `reference/packages/schema/src/schema.ts:4`.
    pub fn non_negative_int() -> Self {
        Schema::Integer {
            description: None,
            exclusive_minimum: None,
            minimum: Some(0),
            maximum: None,
        }
    }

    /// `Schema.Int.check(Schema.isGreaterThanOrEqualTo(n))`.
    pub fn int_ge(minimum: i64) -> Self {
        Schema::Integer {
            description: None,
            exclusive_minimum: None,
            minimum: Some(minimum),
            maximum: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        let description = Some(description.into());
        match &mut self {
            Schema::String { description: d, .. }
            | Schema::Number { description: d, .. }
            | Schema::Integer { description: d, .. }
            | Schema::Boolean { description: d }
            | Schema::Literals { description: d, .. }
            | Schema::Array { description: d, .. }
            | Schema::Struct { description: d, .. } => *d = description,
            Schema::NullOr { .. } | Schema::Raw(_) => {}
        }
        self
    }

    pub fn with_default(mut self, default: JsonValue) -> Self {
        match &mut self {
            Schema::String { default: d, .. } | Schema::Literals { default: d, .. } => {
                *d = Some(default)
            }
            _ => {}
        }
        self
    }

    pub fn array(items: Schema, description: impl Into<String>) -> Self {
        Schema::Array {
            items: Box::new(items),
            description: Some(description.into()),
        }
    }

    pub fn struct_(props: Vec<Property>, description: impl Into<String>) -> Self {
        Schema::Struct {
            props,
            description: Some(description.into()),
        }
    }

    pub fn null_or(inner: Schema) -> Self {
        Schema::NullOr {
            inner: Box::new(inner),
        }
    }

    pub fn raw(value: JsonValue) -> Self {
        Schema::Raw(value)
    }
}

/// `Property.optional(...)` — the `Schema.optional(x)` alias.
pub fn prop(name: &str, schema: Schema) -> Property {
    Property {
        name: name.to_string(),
        schema,
        required: true,
    }
}

/// `Property.optional_key(...)` — mirrors the `optional` helper from
/// `reference/packages/schema/src/schema.ts:12` for JSON Schema purposes
/// (excluded from `required`, same underlying type).
pub fn opt_prop(name: &str, schema: Schema) -> Property {
    Property {
        name: name.to_string(),
        schema,
        required: false,
    }
}

/// Effect document generation. `additional_properties` mirrors the option the
/// reference passes to `Schema.toJsonSchemaDocument` (opencode passes `true`,
/// the core V2 `Tool.toJsonSchema` passes nothing).
pub fn to_document(schema: &Schema, additional_properties: bool) -> JsonValue {
    match schema {
        Schema::String {
            description,
            default,
        } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("string".into()));
            apply_annotations(&mut value, description.as_deref(), default.as_ref());
            JsonValue::Object(value)
        }
        Schema::Number {
            description,
            exclusive_minimum,
            minimum,
            maximum,
        } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("number".into()));
            if let Some(min) = exclusive_minimum {
                value.insert("exclusiveMinimum".into(), JsonValue::from(*min));
            }
            if let Some(min) = minimum {
                value.insert("minimum".into(), JsonValue::from(*min));
            }
            if let Some(max) = maximum {
                value.insert("maximum".into(), JsonValue::from(*max));
            }
            apply_annotations(&mut value, description.as_deref(), None);
            JsonValue::Object(value)
        }
        Schema::Integer {
            description,
            exclusive_minimum,
            minimum,
            maximum,
        } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("integer".into()));
            if let Some(min) = exclusive_minimum {
                value.insert("exclusiveMinimum".into(), JsonValue::from(*min));
            }
            if let Some(min) = minimum {
                value.insert("minimum".into(), JsonValue::from(*min));
            }
            if let Some(max) = maximum {
                value.insert("maximum".into(), JsonValue::from(*max));
            }
            apply_annotations(&mut value, description.as_deref(), None);
            JsonValue::Object(value)
        }
        Schema::Boolean { description } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("boolean".into()));
            apply_annotations(&mut value, description.as_deref(), None);
            JsonValue::Object(value)
        }
        Schema::Literals {
            values,
            description,
            default,
        } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("string".into()));
            value.insert(
                "enum".into(),
                JsonValue::Array(
                    values
                        .iter()
                        .map(|v| JsonValue::String(v.clone()))
                        .collect(),
                ),
            );
            apply_annotations(&mut value, description.as_deref(), default.as_ref());
            JsonValue::Object(value)
        }
        Schema::Array { items, description } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("array".into()));
            value.insert("items".into(), to_document(items, additional_properties));
            apply_annotations(&mut value, description.as_deref(), None);
            JsonValue::Object(value)
        }
        Schema::Struct {
            props,
            description: _,
        } => {
            let mut value = serde_json::Map::new();
            value.insert("type".into(), JsonValue::String("object".into()));
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();
            for prop in props {
                properties.insert(
                    prop.name.clone(),
                    to_document(&prop.schema, additional_properties),
                );
                if prop.required {
                    required.push(JsonValue::String(prop.name.clone()));
                }
            }
            value.insert("properties".into(), JsonValue::Object(properties));
            if !required.is_empty() {
                value.insert("required".into(), JsonValue::Array(required));
            }
            if additional_properties {
                value.insert("additionalProperties".into(), JsonValue::Bool(true));
            }
            JsonValue::Object(value)
        }
        Schema::NullOr { inner } => {
            let mut value = serde_json::Map::new();
            let mut any_of = serde_json::Map::new();
            let mut any_of_list = Vec::new();
            any_of_list.push(to_document(inner, additional_properties));
            any_of.insert("type".into(), JsonValue::String("null".into()));
            any_of_list.push(JsonValue::Object(any_of));
            value.insert("anyOf".into(), JsonValue::Array(any_of_list));
            JsonValue::Object(value)
        }
        Schema::Raw(value) => value.clone(),
    }
}

fn apply_annotations(
    value: &mut serde_json::Map<String, JsonValue>,
    description: Option<&str>,
    default: Option<&JsonValue>,
) {
    if let Some(description) = description {
        value.insert(
            "description".into(),
            JsonValue::String(description.to_string()),
        );
    }
    if let Some(default) = default {
        value.insert("default".into(), default.clone());
    }
}

impl Schema {
    /// Decode validation mirroring the Effect `Schema.decodeUnknownEffect`
    /// gate the reference runs before executing a tool
    /// (`reference/packages/opencode/src/tool/tool.ts:111`). Returns a
    /// human-readable error message on failure.
    pub fn validate(&self, value: &JsonValue) -> Result<(), String> {
        match self {
            Schema::String { .. } => {
                if !value.is_string() {
                    return Err(format!("Expected string, received {}", value));
                }
                Ok(())
            }
            Schema::Number {
                minimum,
                maximum,
                exclusive_minimum,
                ..
            } => {
                let Some(number) = value.as_f64() else {
                    return Err(format!("Expected number, received {}", value));
                };
                if let Some(min) = exclusive_minimum {
                    if number <= *min {
                        return Err(format!(
                            "Expected a number greater than {min}, received {number}"
                        ));
                    }
                }
                if let Some(min) = minimum {
                    if number < *min {
                        return Err(format!(
                            "Expected a number greater than or equal to {min}, received {number}"
                        ));
                    }
                }
                if let Some(max) = maximum {
                    if number > *max {
                        return Err(format!(
                            "Expected a number less than or equal to {max}, received {number}"
                        ));
                    }
                }
                Ok(())
            }
            Schema::Integer {
                minimum,
                maximum,
                exclusive_minimum,
                ..
            } => {
                let Some(integer) = value.as_i64() else {
                    return Err(format!("Expected integer, received {}", value));
                };
                if let Some(min) = exclusive_minimum {
                    if integer <= *min {
                        return Err(format!(
                            "Expected an integer greater than {min}, received {integer}"
                        ));
                    }
                }
                if let Some(min) = minimum {
                    if integer < *min {
                        return Err(format!("Expected an integer greater than or equal to {min}, received {integer}"));
                    }
                }
                if let Some(max) = maximum {
                    if integer > *max {
                        return Err(format!(
                            "Expected an integer less than or equal to {max}, received {integer}"
                        ));
                    }
                }
                Ok(())
            }
            Schema::Boolean { .. } => {
                if !value.is_boolean() {
                    return Err(format!("Expected boolean, received {}", value));
                }
                Ok(())
            }
            Schema::Literals { values, .. } => {
                if !value.is_string()
                    || !values
                        .iter()
                        .any(|v| JsonValue::String(v.clone()) == *value)
                {
                    return Err(format!(
                        "Expected one of [{}], received {}",
                        values.join(", "),
                        value
                    ));
                }
                Ok(())
            }
            Schema::Array { items, .. } => {
                let Some(array) = value.as_array() else {
                    return Err(format!("Expected array, received {}", value));
                };
                for item in array {
                    items.validate(item)?;
                }
                Ok(())
            }
            Schema::Struct { props, .. } => {
                let Some(object) = value.as_object() else {
                    return Err(format!("Expected object, received {}", value));
                };
                for prop in props {
                    if prop.required {
                        if !object.contains_key(&prop.name) {
                            return Err(format!("Missing required field {}", prop.name));
                        }
                        prop.schema.validate(&object[&prop.name])?;
                    } else if let Some(item) = object.get(&prop.name) {
                        if !item.is_null() {
                            prop.schema.validate(item)?;
                        }
                    }
                }
                Ok(())
            }
            Schema::NullOr { inner } => {
                if value.is_null() {
                    return Ok(());
                }
                inner.validate(value)
            }
            Schema::Raw(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_int_encodes_exclusive_minimum() {
        let value = to_document(&Schema::positive_int(), false);
        assert_eq!(
            value,
            serde_json::json!({ "type": "integer", "exclusiveMinimum": 0 })
        );
    }

    #[test]
    fn validates_required_and_constraints() {
        let schema = Schema::struct_(
            vec![
                prop("filePath", Schema::string("The absolute path")),
                opt_prop("line", Schema::int_ge(1)),
            ],
            "read",
        );
        assert!(schema
            .validate(&serde_json::json!({ "filePath": "/a" }))
            .is_ok());
        assert!(schema.validate(&serde_json::json!({ "line": 1 })).is_err());
        assert!(schema
            .validate(&serde_json::json!({ "filePath": "/a", "line": 0 }))
            .is_err());
        assert!(schema
            .validate(&serde_json::json!({ "filePath": "/a", "line": 3 }))
            .is_ok());
    }
}
