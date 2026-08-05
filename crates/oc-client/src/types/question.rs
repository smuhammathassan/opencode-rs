//! Question types.
//! From reference/packages/schema/src/question.ts.

// TODO(integration): promote to oc-schema.
/// `Question.Option`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// `Question.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionInfo {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<bool>,
}

/// `Question.Tool`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionTool {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

/// `Question.Request`.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub questions: Vec<QuestionInfo>,
    #[serde(default)]
    pub tool: Option<QuestionTool>,
}

/// `Question.Reply` — `{ answers }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReply {
    pub answers: Vec<Vec<String>>,
}

/// `QuestionsListInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionsListInput {
    pub session_id: String,
}

/// `QuestionsReplyInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionsReplyInput {
    pub session_id: String,
    pub request_id: String,
    pub answers: Vec<Vec<String>>,
}

/// `QuestionsRejectInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionsRejectInput {
    pub session_id: String,
    pub request_id: String,
}
