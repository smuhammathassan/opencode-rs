//! From reference/packages/schema/src/v1/question.ts

use crate::define_event;
use crate::session_id::SessionID;
use crate::v1::session;
use serde::{Deserialize, Serialize};

/// `QuestionV1.ID` — starts with `que`.
pub type ID = String;

/// `Question.ID.ascending(id?)`.
pub fn ascending(id: Option<String>) -> ID {
    match id {
        Some(id) => id,
        None => format!("que_{}", crate::identifier::ascending()),
    }
}

/// `QuestionOption`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Option_ {
    pub label: String,
    pub description: String,
}

/// `QuestionInfo`.
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

/// `QuestionPrompt`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Prompt {
    pub question: String,
    pub header: String,
    pub options: Vec<Option_>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub multiple: Option<bool>,
}

/// `QuestionTool`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tool {
    #[serde(rename = "messageID")]
    pub message_id: session::MessageID,
    #[serde(rename = "callID")]
    pub call_id: String,
}

/// `QuestionRequest`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: ID,
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    pub questions: Vec<Info>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool: Option<Tool>,
}

/// `QuestionAnswer`.
pub type Answer = Vec<String>;

/// `QuestionReply`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reply {
    pub answers: Vec<Answer>,
}

/// `QuestionReplied`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Replied {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "requestID")]
    pub request_id: ID,
    pub answers: Vec<Answer>,
}

/// `QuestionRejected`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rejected {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
    #[serde(rename = "requestID")]
    pub request_id: ID,
}

define_event! {
    /// `question.asked`.
    pub struct Asked {
        tag: AskedTag,
        r#type: "question.asked",
        data: Request,
    }
}

define_event! {
    /// `question.replied`.
    pub struct RepliedEvent {
        tag: RepliedEventTag,
        r#type: "question.replied",
        data: Replied,
    }
}

define_event! {
    /// `question.rejected`.
    pub struct RejectedEvent {
        tag: RejectedEventTag,
        r#type: "question.rejected",
        data: Rejected,
    }
}

/// `QuestionV1.Event`.
#[allow(non_snake_case)]
pub mod Event {
    pub use super::{Asked, RejectedEvent, RepliedEvent};
    pub use crate::event::Definition;

    /// `QuestionV1.Event.Definitions`.
    pub const DEFINITIONS: &[Definition] = &[
        Definition {
            r#type: "question.asked",
            durable: None,
        },
        Definition {
            r#type: "question.replied",
            durable: None,
        },
        Definition {
            r#type: "question.rejected",
            durable: None,
        },
    ];
}

/// `QuestionV1.Event.Definitions` at module level (alias kept for parity).
pub use Event::DEFINITIONS as Definitions;
