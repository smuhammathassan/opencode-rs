//! MCP server configuration (local mirror of `ConfigMCPV1.Info`).
//!
//! From reference/packages/core/src/v1/config/mcp.ts. The `mcp` section of
//! opencode.json is parsed into `Info` values.
//!
//! TODO(integration): promote to oc-config once that crate models the
//! `mcp` section of the config file.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Local {
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Remote {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

impl Remote {
    pub fn oauth_enabled(&self) -> bool {
        !matches!(self.oauth, Some(OAuth::Disabled))
    }

    pub fn oauth_config(&self) -> Option<&OAuthConfig> {
        match &self.oauth {
            Some(OAuth::Config(config)) => Some(config),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthConfig {
    #[serde(rename = "clientId", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(rename = "clientSecret", skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(rename = "callbackPort", skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<u16>,
    #[serde(rename = "redirectUri", skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
}

/// `oauth` config: either an `McpOAuthConfig` object or `false` to disable
/// OAuth auto-detection.
#[derive(Debug, Clone, PartialEq)]
pub enum OAuth {
    Disabled,
    Config(OAuthConfig),
}

impl Serialize for OAuth {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            OAuth::Disabled => serializer.serialize_bool(false),
            OAuth::Config(config) => config.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for OAuth {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value == serde_json::Value::Bool(false) {
            return Ok(OAuth::Disabled);
        }
        serde_json::from_value(value)
            .map(OAuth::Config)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Info {
    #[serde(rename = "local")]
    Local(Local),
    #[serde(rename = "remote")]
    Remote(Remote),
}

impl Info {
    pub fn enabled(&self) -> bool {
        match self {
            Info::Local(local) => local.enabled != Some(false),
            Info::Remote(remote) => remote.enabled != Some(false),
        }
    }

    pub fn timeout(&self) -> Option<u64> {
        match self {
            Info::Local(local) => local.timeout,
            Info::Remote(remote) => remote.timeout,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        match self {
            Info::Local(local) => local.enabled = Some(enabled),
            Info::Remote(remote) => remote.enabled = Some(enabled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_roundtrips() {
        let raw = serde_json::json!({
            "type": "local",
            "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem"],
            "cwd": ".",
            "environment": { "FOO": "bar" },
            "enabled": true,
            "timeout": 5000
        });
        let info: Info = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&info).unwrap(), raw);
    }

    #[test]
    fn remote_with_oauth_object_roundtrips() {
        let raw = serde_json::json!({
            "type": "remote",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer x" },
            "oauth": { "clientId": "abc", "scope": "read" }
        });
        let info: Info = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&info).unwrap(), raw);
    }

    #[test]
    fn remote_with_oauth_false_roundtrips() {
        let raw = serde_json::json!({
            "type": "remote",
            "url": "https://example.com/mcp",
            "oauth": false
        });
        let info: Info = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(info, Info::Remote(ref r) if r.oauth == Some(OAuth::Disabled)));
        assert_eq!(serde_json::to_value(&info).unwrap(), raw);
        assert!(!matches!(info, Info::Remote(ref r) if r.oauth_enabled()));
    }

    #[test]
    fn enabled_flag() {
        let local: Info = serde_json::from_value(serde_json::json!({
            "type": "local",
            "command": ["x"]
        }))
        .unwrap();
        assert!(local.enabled());

        let local: Info = serde_json::from_value(serde_json::json!({
            "type": "local",
            "command": ["x"],
            "enabled": false
        }))
        .unwrap();
        assert!(!local.enabled());
    }
}
