//! From reference/packages/schema/src/model.ts

use crate::provider;
use crate::schema::{Finite, Json};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `ModelV2.ID`.
pub type ID = String;

/// `VariantID`.
pub type VariantID = String;

/// `Family`.
pub type Family = String;

/// `Model.Ref`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Ref {
    pub id: ID,
    #[serde(rename = "providerID")]
    pub provider_id: provider::ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant: Option<VariantID>,
}

/// `Model.Capabilities`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Capabilities {
    pub tools: bool,
    pub input: Vec<String>,
    pub output: Vec<String>,
}

/// The optional `tier` of `Model.Cost`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CostTier {
    #[serde(rename = "type")]
    pub r#type: CostTierType,
    pub size: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CostTierType {
    #[serde(rename = "context")]
    Value,
}

/// `Model.Cost`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cost {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tier: Option<CostTier>,
    pub input: Finite,
    pub output: Finite,
    pub cache: CostCache,
}

/// `Model.Cost.cache`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CostCache {
    pub read: Finite,
    pub write: Finite,
}

/// `Model.Api` — `{ id, ...Provider.AISDK.fields }` or `{ id, ...Provider.Native.fields }`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Api {
    Aisdk(AisdkApi),
    Native(NativeApi),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AisdkApi {
    pub id: ID,
    #[serde(rename = "type")]
    pub r#type: AisdkApiType,
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub settings: Option<IndexMap<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AisdkApiType {
    #[serde(rename = "aisdk")]
    Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NativeApi {
    pub id: ID,
    #[serde(rename = "type")]
    pub r#type: NativeApiType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    pub settings: IndexMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NativeApiType {
    #[serde(rename = "native")]
    Value,
}

/// `Model.Info.request` — `Provider.Request.fields` plus optional `variant`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub headers: IndexMap<String, String>,
    pub body: IndexMap<String, Json>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant: Option<String>,
}

/// `Model.Info.variants` element.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Variant {
    pub id: VariantID,
    pub headers: IndexMap<String, String>,
    pub body: IndexMap<String, Json>,
}

/// `Model.Info.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Time {
    pub released: Finite,
}

/// `Model.Info.status`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Status {
    #[serde(rename = "alpha")]
    Alpha,
    #[serde(rename = "beta")]
    Beta,
    #[serde(rename = "deprecated")]
    Deprecated,
    #[serde(rename = "active")]
    Active,
}

/// `Model.Info.limit`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Limit {
    pub context: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input: Option<i64>,
    pub output: i64,
}

/// `ModelV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    #[serde(rename = "providerID")]
    pub provider_id: provider::ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub family: Option<Family>,
    pub name: String,
    pub api: Api,
    pub capabilities: Capabilities,
    pub request: Request,
    pub variants: Vec<Variant>,
    pub time: Time,
    pub cost: Vec<Cost>,
    pub status: Status,
    pub enabled: bool,
    pub limit: Limit,
}

/// `Model.empty(providerID, modelID)`.
pub fn empty(provider_id: provider::ID, model_id: ID) -> Info {
    Info {
        id: model_id.clone(),
        provider_id,
        family: None,
        name: model_id.clone(),
        api: Api::Native(NativeApi {
            id: model_id,
            r#type: NativeApiType::Value,
            url: None,
            settings: IndexMap::new(),
        }),
        capabilities: Capabilities {
            tools: false,
            input: Vec::new(),
            output: Vec::new(),
        },
        request: Request {
            headers: IndexMap::new(),
            body: IndexMap::new(),
            variant: None,
        },
        variants: Vec::new(),
        time: Time {
            released: Finite(0.0),
        },
        cost: Vec::new(),
        status: Status::Active,
        enabled: true,
        limit: Limit {
            context: 0,
            input: None,
            output: 0,
        },
    }
}
