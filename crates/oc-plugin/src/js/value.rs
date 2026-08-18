use std::collections::HashMap;

/// Errors produced by the QuickJS runtime bridge.
#[derive(Debug, Clone, PartialEq)]
pub enum JsError {
    /// A string contained a zero byte and could not cross the FFI boundary.
    StringWithZeroBytes,
    /// A value conversion failed because of an unexpected type.
    UnexpectedType,
    /// A string was not valid UTF-8.
    InvalidString(std::str::Utf8Error),
    /// A JS exception was thrown; the payload is the exception string.
    Exception(String),
    /// Execution was aborted by the runtime limits (instruction/time budget or
    /// memory guard).
    Limit(String),
    /// An internal runtime error.
    Internal(String),
}

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsError::StringWithZeroBytes => write!(f, "string contains \\0 bytes"),
            JsError::UnexpectedType => write!(f, "unexpected value type"),
            JsError::InvalidString(error) => write!(f, "invalid utf-8: {error}"),
            JsError::Exception(message) => write!(f, "{message}"),
            JsError::Limit(message) => write!(f, "{message}"),
            JsError::Internal(message) => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for JsError {}

impl From<std::str::Utf8Error> for JsError {
    fn from(error: std::str::Utf8Error) -> Self {
        JsError::Internal(format!("invalid utf-8: {error}"))
    }
}

/// A value that can cross the QuickJS boundary.
///
/// This mirrors `quick_js::JsValue` (value.rs) but adds `Undefined` so we can
/// distinguish an absent property from an explicit `null`. Objects are
/// serializable in both directions as long as they only contain these types;
/// the runtime rejects arbitrary JS objects (functions, classes, ...).
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Int(i32),
    Float(f64),
    String(String),
    Array(Vec<JsValue>),
    Object(HashMap<String, JsValue>),
}

impl JsValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn into_string(self) -> Option<String> {
        match self {
            JsValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl From<JsValue> for serde_json::Value {
    fn from(value: JsValue) -> Self {
        match value {
            JsValue::Undefined | JsValue::Null => serde_json::Value::Null,
            JsValue::Bool(b) => serde_json::Value::Bool(b),
            JsValue::Int(i) => serde_json::Value::from(i),
            JsValue::Float(f) => serde_json::Value::from(f),
            JsValue::String(s) => serde_json::Value::String(s),
            JsValue::Array(items) => {
                serde_json::Value::Array(items.into_iter().map(Into::into).collect())
            }
            JsValue::Object(map) => {
                let obj = map.into_iter().map(|(k, v)| (k, v.into())).collect();
                serde_json::Value::Object(obj)
            }
        }
    }
}

impl From<&serde_json::Value> for JsValue {
    fn from(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsValue::Null,
            serde_json::Value::Bool(b) => JsValue::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if let Ok(i) = i32::try_from(i) {
                        return JsValue::Int(i);
                    }
                    return JsValue::Float(i as f64);
                }
                if let Some(f) = n.as_f64() {
                    return JsValue::Float(f);
                }
                JsValue::Null
            }
            serde_json::Value::String(s) => JsValue::String(s.clone()),
            serde_json::Value::Array(items) => {
                JsValue::Array(items.iter().map(JsValue::from).collect())
            }
            serde_json::Value::Object(map) => {
                let fields = map
                    .iter()
                    .map(|(k, v)| (k.clone(), JsValue::from(v)))
                    .collect();
                JsValue::Object(fields)
            }
        }
    }
}

impl From<serde_json::Value> for JsValue {
    fn from(value: serde_json::Value) -> Self {
        JsValue::from(&value)
    }
}

impl From<bool> for JsValue {
    fn from(value: bool) -> Self {
        JsValue::Bool(value)
    }
}

macro_rules! try_from_value {
    ($t:ty => $variant:ident) => {
        impl std::convert::TryFrom<JsValue> for $t {
            type Error = JsError;

            fn try_from(value: JsValue) -> Result<Self, Self::Error> {
                match value {
                    JsValue::$variant(inner) => Ok(inner),
                    _ => Err(JsError::UnexpectedType),
                }
            }
        }
    };
}

try_from_value!(bool => Bool);
try_from_value!(i32 => Int);
try_from_value!(f64 => Float);
try_from_value!(String => String);

impl From<i32> for JsValue {
    fn from(value: i32) -> Self {
        JsValue::Int(value)
    }
}

impl From<f64> for JsValue {
    fn from(value: f64) -> Self {
        JsValue::Float(value)
    }
}

impl From<String> for JsValue {
    fn from(value: String) -> Self {
        JsValue::String(value)
    }
}

impl<'a> From<&'a str> for JsValue {
    fn from(value: &'a str) -> Self {
        JsValue::String(value.into())
    }
}

impl<T: Into<JsValue>> From<Vec<T>> for JsValue {
    fn from(values: Vec<T>) -> Self {
        JsValue::Array(values.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<JsValue>> From<Option<T>> for JsValue {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => value.into(),
            None => JsValue::Null,
        }
    }
}

impl<K: Into<String>, V: Into<JsValue>> From<HashMap<K, V>> for JsValue {
    fn from(map: HashMap<K, V>) -> Self {
        let fields = map.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        JsValue::Object(fields)
    }
}
