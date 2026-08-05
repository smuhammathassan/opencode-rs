//! Provider schema mirror.
//!
//! From reference/packages/schema/src/provider.ts and
//! reference/packages/core/src/provider.ts.
//!
//! TODO(integration): promote to oc-schema (Provider types) — oc-provider
//! will own the provider runtime.

use serde::{Deserialize, Serialize};

use crate::ids::{IntegrationId, ProviderId};

/// `Provider.AISDK` — `{ type: "aisdk", package, url?, settings? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAisdk {
    #[serde(rename = "type")]
    pub kind: String,
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
}

/// `Provider.Native` — `{ type: "native", url?, settings }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderNative {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub settings: serde_json::Map<String, serde_json::Value>,
}

/// `Provider.Api` — tagged union on `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProviderApi {
    #[serde(rename = "aisdk")]
    Aisdk(ProviderAisdk),
    #[serde(rename = "native")]
    Native(ProviderNative),
}

/// `Provider.Request` — `{ headers, body }`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub headers: serde_json::Map<String, serde_json::Value>,
    pub body: serde_json::Map<String, serde_json::Value>,
}

/// `Provider.Info` — `{ id, integrationID?, name, disabled?, api, request }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrationID: Option<IntegrationId>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    pub api: ProviderApi,
    pub request: ProviderRequest,
}

impl ProviderInfo {
    /// Mirrors `Provider.Info.empty(id)`.
    pub fn empty(id: ProviderId) -> Self {
        ProviderInfo {
            id: id.clone(),
            integrationID: None,
            name: id.0.clone(),
            disabled: None,
            api: ProviderApi::Native(ProviderNative {
                kind: "native".to_string(),
                url: None,
                settings: serde_json::Map::new(),
            }),
            request: ProviderRequest::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_info_json() {
        let info = ProviderInfo::empty(ProviderId::make("anthropic"));
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({
                "id": "anthropic",
                "name": "anthropic",
                "api": { "type": "native", "settings": {} },
                "request": { "headers": {}, "body": {} }
            })
        );
    }
}
