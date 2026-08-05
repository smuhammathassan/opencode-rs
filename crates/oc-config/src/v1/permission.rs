// From reference/packages/core/src/v1/config/permission.ts

use indexmap::IndexMap;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `Action` = `Schema.Literals(["ask", "allow", "deny"])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Ask,
    Allow,
    Deny,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Ask => "ask",
            Action::Allow => "allow",
            Action::Deny => "deny",
        }
    }
}

impl Serialize for Action {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "ask" => Ok(Action::Ask),
            "allow" => Ok(Action::Allow),
            "deny" => Ok(Action::Deny),
            _ => Err(D::Error::custom(format!(
                "Expected \"ask\", \"allow\", or \"deny\" but got \"{value}\""
            ))),
        }
    }
}

/// `Rule` = `Schema.Union([Action, Object])` where `Object` is a map of
/// action-to-`Action`. A scalar action applies to `*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Rule {
    Action(Action),
    Object(IndexMap<String, Action>),
}

impl Rule {
    fn from_value(value: &serde_json::Value) -> Result<Rule, String> {
        match value {
            serde_json::Value::String(s) => parse_action(s).map(Rule::Action),
            serde_json::Value::Object(map) => {
                let mut out = IndexMap::new();
                for (key, value) in map {
                    let action = match value {
                        serde_json::Value::String(s) => parse_action(s)?,
                        other => {
                            return Err(format!(
                                "Expected a permission action for \"{key}\" but got {other}"
                            ))
                        }
                    };
                    out.insert(key.clone(), action);
                }
                Ok(Rule::Object(out))
            }
            other => Err(format!("Expected a permission action or object but got {other}")),
        }
    }
}

fn parse_action(value: &str) -> Result<Action, String> {
    match value {
        "ask" => Ok(Action::Ask),
        "allow" => Ok(Action::Allow),
        "deny" => Ok(Action::Deny),
        other => Err(format!(
            "Expected \"ask\", \"allow\", or \"deny\" but got \"{other}\""
        )),
    }
}

/// Keys that accept `Action` only (not a pattern object).
const ACTION_ONLY_KEYS: [&str; 5] = ["todowrite", "question", "webfetch", "websearch", "doom_loop"];

/// `Info` — the normalized permission object. A scalar input like `"deny"` is
/// normalized to `{ "*": "deny" }`, matching the `decodeTo` transform in the
/// reference.
///
/// Entry order is significant: downstream permission evaluation iterates rules
/// in order with `findLast` winning, so this is an ordered map rather than a
/// typed struct with fixed field order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info(pub IndexMap<String, Rule>);

impl Info {
    pub fn get(&self, key: &str) -> Option<&Rule> {
        self.0.get(key)
    }

    pub fn insert(&mut self, key: String, rule: Rule) {
        self.0.insert(key, rule);
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &Rule)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Shallow `Object.assign` used when merging `tools`-derived permission
    /// over an explicit `permission` in agent normalization.
    pub fn assign(&mut self, other: &Info) {
        for (key, rule) in &other.0 {
            self.0.insert(key.clone(), rule.clone());
        }
    }
}

impl Serialize for Info {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_map(self.0.iter())
    }
}

impl<'de> Deserialize<'de> for Info {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(action) => {
                let action = parse_action(&action).map_err(D::Error::custom)?;
                let mut map = IndexMap::new();
                map.insert("*".to_string(), Rule::Action(action));
                Ok(Info(map))
            }
            serde_json::Value::Object(map) => {
                let mut out = IndexMap::new();
                for (key, value) in map {
                    let rule = Rule::from_value(&value).map_err(D::Error::custom)?;
                    if ACTION_ONLY_KEYS.contains(&key.as_str()) {
                        if !matches!(rule, Rule::Action(_)) {
                            return Err(D::Error::custom(format!(
                                "Expected a permission action for \"{key}\" but got an object"
                            )));
                        }
                    }
                    out.insert(key, rule);
                }
                Ok(Info(out))
            }
            other => Err(D::Error::custom(format!(
                "Expected a permission action or object but got {other}"
            ))),
        }
    }
}
