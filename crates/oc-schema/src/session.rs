//! From reference/packages/schema/src/session.ts

use crate::agent;
use crate::location;
use crate::model;
use crate::project;
use crate::revert;
use crate::schema::{DateTimeUtc, Finite, RelativePath};
use crate::session_id::SessionID;
pub use crate::session_message::{TokenCache, TokenUsage};
use serde::{Deserialize, Serialize};

/// `Session.ID`.
pub type ID = SessionID;

/// `Session.Event` — the session event namespace.
pub use crate::session_event::{DurableEvent, Event, DEFINITIONS, DURABLE_DEFINITIONS};

/// `Session.Info.time`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Time {
    pub created: DateTimeUtc,
    pub updated: DateTimeUtc,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub archived: Option<DateTimeUtc>,
}

/// `SessionV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub id: ID,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none", default)]
    pub parent_id: Option<ID>,
    #[serde(rename = "projectID")]
    pub project_id: project::ID,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent: Option<agent::ID>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<model::Ref>,
    pub cost: Finite,
    pub tokens: TokenUsage,
    pub time: Time,
    pub title: String,
    pub location: location::Ref,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subpath: Option<RelativePath>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub revert: Option<revert::State>,
}

/// `Session.ListAnchor.direction`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    #[serde(rename = "previous")]
    Previous,
    #[serde(rename = "next")]
    Next,
}

/// `Session.ListAnchor`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ListAnchor {
    pub id: ID,
    pub time: Finite,
    pub direction: Direction,
}
