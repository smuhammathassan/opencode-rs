//! Provider types.
//! From reference/packages/schema/src/provider.ts.

use crate::types::location::LocationQueryRef;
use crate::types::schema::JsonValue;
use std::collections::HashMap;

/// `Provider.Request`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequest {
    pub headers: HashMap<String, String>,
    pub body: HashMap<String, JsonValue>,
}

/// `Provider.Api` — tagged union on `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum ProviderApi {
    #[serde(rename = "aisdk")]
    Aisdk {
        package: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        settings: Option<HashMap<String, JsonValue>>,
    },
    #[serde(rename = "native")]
    Native {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        settings: HashMap<String, JsonValue>,
    },
}

/// `Provider.Info`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    #[serde(default)]
    #[serde(rename = "integrationID")]
    pub integration_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub disabled: Option<bool>,
    pub api: ProviderApi,
    pub request: ProviderRequest,
}

/// `ProvidersGetInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProvidersGetInput {
    pub provider_id: String,
    pub location: Option<LocationQueryRef>,
}
