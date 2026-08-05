//! From reference/packages/schema/src/session-status-event.ts

use crate::define_event;
use crate::event::Definition;
use crate::schema::NonNegativeInt;
use crate::session_id::SessionID;
use serde::{Deserialize, Serialize};

/// `SessionStatusEvent.Info` — tagged union on `type`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Info {
    Idle(IdleInfo),
    Retry(RetryInfo),
    Busy(BusyInfo),
}

/// `SessionStatusEvent.Info` (`idle`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct IdleInfo {
    #[serde(rename = "type")]
    pub r#type: IdleInfoType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum IdleInfoType {
    #[serde(rename = "idle")]
    Value,
}

/// `SessionStatusEvent.Info.action`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Action {
    pub reason: String,
    pub provider: String,
    pub title: String,
    pub message: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub link: Option<String>,
}

/// `SessionStatusEvent.Info` (`retry`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RetryInfo {
    #[serde(rename = "type")]
    pub r#type: RetryInfoType,
    pub attempt: NonNegativeInt,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub action: Option<Action>,
    pub next: NonNegativeInt,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum RetryInfoType {
    #[serde(rename = "retry")]
    Value,
}

/// `SessionStatusEvent.Info` (`busy`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BusyInfo {
    #[serde(rename = "type")]
    pub r#type: BusyInfoType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum BusyInfoType {
    #[serde(rename = "busy")]
    Value,
}

define_event! {
    /// `session.status`.
    pub struct Status {
        tag: StatusTag,
        r#type: "session.status",
        data: StatusData,
    }
}

define_event! {
    /// `session.idle` (deprecated).
    pub struct Idle {
        tag: IdleTag,
        r#type: "session.idle",
        data: IdleData,
    }
}

/// Payload of `session.status`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct StatusData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub status: Info,
}

/// Payload of `session.idle`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct IdleData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
}

/// `SessionStatusEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "session.status",
        durable: None,
    },
    Definition {
        r#type: "session.idle",
        durable: None,
    },
];
