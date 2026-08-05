//! Question wire contracts.
//!
//! From reference/packages/schema/src/v1/question.ts (the `QuestionV1`
//! namespace used by `reference/packages/opencode/src/question/index.ts`).

use crate::id;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct QuestionId(String);

impl QuestionId {
    /// `id ?? "que_" + ascending()` from `QuestionV1.ID`.
    pub fn ascending() -> Self {
        QuestionId(format!("que_{}", id::ascending()))
    }

    pub fn new(value: impl Into<String>) -> Self {
        QuestionId(value.into())
    }
}

impl std::fmt::Display for QuestionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<bool>,
}

/// The `QuestionV1.Prompt` base (used by the question tool input).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Prompt {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tool {
    pub message_id: String,
    pub call_id: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Request {
    pub id: QuestionId,
    pub session_id: String,
    pub questions: Vec<Info>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<Tool>,
}

pub type Answer = Vec<String>;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reply {
    pub answers: Vec<Answer>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Replied {
    pub session_id: String,
    pub request_id: QuestionId,
    pub answers: Vec<Answer>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rejected {
    pub session_id: String,
    pub request_id: QuestionId,
}
