//! Port of `reference/packages/opencode/src/tool/invalid.ts`.

use crate::model::ExecuteResult;
use crate::schema::{prop, Schema};
use crate::tool::tool;

/// `Parameters` from `reference/packages/opencode/src/tool/invalid.ts:4`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop("tool", Schema::plain_string()),
            prop("error", Schema::plain_string()),
        ],
        "invalid",
    )
}

/// `InvalidTool` from `reference/packages/opencode/src/tool/invalid.ts:9`.
pub fn def() -> tool::Def {
    tool::def("invalid", "Do not use", parameters(), |args, _ctx| {
        let error = args
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        Ok(ExecuteResult {
            title: "Invalid Tool".to_string(),
            output: format!("The arguments provided to the tool are invalid: {error}"),
            metadata: serde_json::json!({}),
            attachments: None,
        })
    })
}
