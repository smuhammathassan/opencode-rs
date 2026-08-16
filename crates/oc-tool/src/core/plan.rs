//! V2 plan-mode exit tool.
//!
//! The host owns the approval question and the session-agent transition. This
//! core definition supplies the stable tool contract and a deterministic
//! fallback for embedders that do not provide the host hook.

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, ToolError};
use crate::schema::{prop, Schema};

pub const NAME: &str = "plan_exit";

pub fn input() -> Schema {
    Schema::struct_(Vec::new(), NAME)
}

fn output() -> Schema {
    Schema::struct_(
        vec![
            prop("status", Schema::plain_string()),
            prop("question", Schema::plain_string()),
        ],
        NAME,
    )
}

/// `PlanExitTool` from `reference/packages/opencode/src/tool/plan.ts`.
pub fn def() -> CoreTool {
    tool::make(
        "Exit plan mode and ask whether to switch to the build agent.",
        input(),
        output(),
        None,
        None,
        Some(std::sync::Arc::new(|_input, result| {
            vec![Content::Text {
                text: result
                    .get("question")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Plan approval is required.")
                    .to_string(),
            }]
        })),
        execute,
    )
}

fn execute(
    _input: serde_json::Value,
    context: &mut CoreContext,
) -> Result<serde_json::Value, ToolError> {
    context.assert(crate::core::tool::CorePermissionRequest {
        action: NAME.to_string(),
        resources: vec!["*".to_string()],
        save: None,
        metadata: None,
        source: crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        },
    })?;
    Ok(serde_json::json!({
        "status": "approval_required",
        "question": "Plan at PLAN.md is complete. Would you like to switch to the build agent and start implementing?"
    }))
}
