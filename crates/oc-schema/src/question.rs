//! From reference/packages/schema/src/question.ts

use crate::define_event;
use crate::identifier::ascending;
use crate::session_id::SessionID;
use serde::{Deserialize, Serialize};

/// `QuestionV2.ID` — starts with `que`.
pub type ID = String;

/// `Question.ID.create()`.
pub fn create_id() -> ID {
    format!("que_{}", ascending())
}

/// `QuestionV2.Option`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Option_ {
    pub label: String,
    pub description: String,
}

/// `QuestionV2.Info`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Info {
    pub question: String,
    pub header: String,
    pub options: Vec<Option_>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub multiple: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom: Option<bool>,
}

/// `QuestionV2.Prompt`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prompt {
    pub question: String,
    pub header: String,
    pub options: Vec<Option_>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub multiple: Option<bool>,
}

/// `QuestionV2.Tool`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tool {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

/// `QuestionV2.Request`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: ID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub questions: Vec<Info>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool: Option<Tool>,
}

/// `QuestionV2.Answer`.
pub type Answer = Vec<String>;

/// `QuestionV2.Reply`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reply {
    pub answers: Vec<Answer>,
}

define_event! {
    /// `question.v2.asked`.
    pub struct Asked {
        tag: AskedTag,
        r#type: "question.v2.asked",
        data: Request,
    }
}

define_event! {
    /// `question.v2.replied`.
    pub struct Replied {
        tag: RepliedTag,
        r#type: "question.v2.replied",
        data: RepliedData,
    }
}

define_event! {
    /// `question.v2.rejected`.
    pub struct Rejected {
        tag: RejectedTag,
        r#type: "question.v2.rejected",
        data: RejectedData,
    }
}

/// Payload of `question.v2.replied`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct RepliedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "requestID")]
    pub request_id: ID,
    pub answers: Vec<Answer>,
}

/// Payload of `question.v2.rejected`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct RejectedData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "requestID")]
    pub request_id: ID,
}

/// `Question.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::{Asked, Rejected, Replied};
    pub use crate::event::Definition;

    /// `Question.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "question.v2.asked",
            durable: None,
        },
        Definition {
            r#type: "question.v2.replied",
            durable: None,
        },
        Definition {
            r#type: "question.v2.rejected",
            durable: None,
        },
    ];
}

/// `Question.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
