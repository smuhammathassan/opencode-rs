//! Wire types for `opencode run` event streaming.
//! Field shapes mirror `reference/packages/schema/src/v1/session.ts` and the
//! compat `GlobalEvent` envelope emitted on `/event`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PartTime {
    pub start: Option<u64>,
    pub end: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolState {
    pub status: String,
    pub input: Option<Value>,
    pub output: Option<String>,
    pub title: Option<String>,
    pub error: Option<String>,
    pub metadata: Option<Value>,
    pub time: Option<PartTime>,
}

/// A `message.part.updated` part. Only the fields the `run` loop reads are
/// surfaced; the rest are ignored.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: String,
    #[serde(rename = "messageID", default)]
    pub message_id: String,
    #[serde(rename = "type", default)]
    pub part_type: String,
    pub tool: Option<String>,
    pub call_id: Option<String>,
    pub text: Option<String>,
    pub time: Option<PartTime>,
    pub state: Option<ToolState>,
    pub metadata: Option<Value>,
}

/// A `message.updated` event info.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MessageInfo {
    pub role: String,
    pub agent: Option<String>,
    #[serde(rename = "modelID", default)]
    pub model_id: Option<String>,
}

/// A `session.status` status payload.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionStatus {
    #[serde(rename = "type")]
    pub status_type: String,
}

/// A `permission.asked` request payload.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: String,
    pub permission: String,
    pub patterns: Vec<String>,
}

/// The compat event envelope emitted on the `/event` SSE stream.
#[derive(Debug, Clone, Deserialize)]
pub struct GlobalEvent {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(rename = "properties")]
    pub properties: Value,
}

/// A session returned by the session API.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(rename = "parentID", default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub time: Option<Value>,
}

/// An agent listed by the app API.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentSummary {
    pub name: String,
    #[serde(default)]
    pub mode: String,
}

/// A model selector used when creating/prompting a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInput {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

/// A file part attached to a prompt.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PromptPart {
    Text {
        text: String,
    },
    File {
        url: String,
        filename: String,
        mime: String,
    },
}

/// Resolve a `providerID/modelID` string like the reference `pick()` helper.
pub fn pick_model(value: Option<&str>) -> Option<ModelInput> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    let (provider, rest) = value.split_once('/')?;
    Some(ModelInput {
        provider_id: provider.to_string(),
        model_id: rest.to_string(),
    })
}

/// Mirrors `resolveRunInput(value, piped)`.
pub fn resolve_run_input(value: Option<String>, piped: Option<String>) -> Option<String> {
    match (value, piped) {
        (None, piped) => piped,
        (Some(value), None) => Some(value),
        (Some(value), Some(piped)) => Some(format!("{value}\n{piped}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_model_splits_provider_and_model() {
        let model = pick_model(Some("anthropic/claude-sonnet-4")).unwrap();
        assert_eq!(model.provider_id, "anthropic");
        assert_eq!(model.model_id, "claude-sonnet-4");
        assert!(pick_model(None).is_none());
        assert!(pick_model(Some("")).is_none());
        assert!(pick_model(Some("nope")).is_none());
    }

    #[test]
    fn resolve_run_input_prefers_value_and_appends_piped() {
        assert_eq!(
            resolve_run_input(None, Some("piped".into())),
            Some("piped".into())
        );
        assert_eq!(
            resolve_run_input(Some("value".into()), None),
            Some("value".into())
        );
        assert_eq!(
            resolve_run_input(Some("value".into()), Some("piped".into())),
            Some("value\npiped".into())
        );
    }

    #[test]
    fn parses_stream_text_part() {
        let json = serde_json::json!({"id":"prt_fd0eef5090017SKAXoxJxB6QoZ","messageID":"msg_fd0eeefa9001KKSbd1x0oBOqJy","sessionID":"ses_02f1110c2ffev6vf2B7beTIIM6","type":"step-start"});
        let part: Part = serde_json::from_value(json).unwrap();
        assert_eq!(part.session_id, "ses_02f1110c2ffev6vf2B7beTIIM6");
        assert_eq!(part.part_type, "step-start");
    }

    #[test]
    fn parses_tool_part_event() {
        let json = serde_json::json!({
            "id": "evt_1",
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_1",
                "part": {
                    "id": "prt_1",
                    "sessionID": "ses_1",
                    "messageID": "msg_1",
                    "type": "tool",
                    "callID": "call_1",
                    "tool": "bash",
                    "state": {
                        "status": "completed",
                        "input": {"command": "ls"},
                        "output": "file.txt",
                        "title": "bash",
                        "time": {"start": 1, "end": 2}
                    }
                }
            }
        });
        let event: GlobalEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.event_type, "message.part.updated");
        let part: Part = serde_json::from_value(event.properties["part"].clone()).unwrap();
        assert_eq!(part.part_type, "tool");
        assert_eq!(part.tool.as_deref(), Some("bash"));
        assert_eq!(part.state.as_ref().unwrap().status, "completed");
        assert_eq!(
            part.state.as_ref().unwrap().output.as_deref(),
            Some("file.txt")
        );
    }
}
