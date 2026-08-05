//! Port of `reference/packages/opencode/src/tool/plan.ts` (`plan_exit`).
//!
//! The session/agent switch operations are stubbed behind `ToolServices`.
//! TODO(integration): resolve the plan file, switch to the build agent, and
//! update the session message/part like the reference `Session.plan` flow.

use crate::model::{ExecuteResult, ToolError};
use crate::prompts;
use crate::schema::Schema;

/// `Parameters` from `reference/packages/opencode/src/tool/plan.ts:13`.
pub fn parameters() -> Schema {
    Schema::struct_(Vec::new(), "plan_exit")
}

/// `PlanExitTool` from `reference/packages/opencode/src/tool/plan.ts:15`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def(
        "plan_exit",
        prompts::PLAN_EXIT,
        parameters(),
        |_args, ctx| {
            let questions = serde_json::json!([
                {
                    "question": "Plan at PLAN.md is complete. Would you like to switch to the build agent and start implementing?",
                    "header": "Build Agent",
                    "custom": false,
                    "options": [
                        { "label": "Yes", "description": "Switch to build agent and start implementing the plan" },
                        { "label": "No", "description": "Stay with plan agent to continue refining the plan" }
                    ]
                }
            ]);
            let answers = ctx.services.question_ask(
                &ctx.session_id,
                &questions,
                ctx.call_id
                    .clone()
                    .map(|call_id| (ctx.message_id.clone(), call_id)),
            )?;
            let first = answers
                .as_array()
                .and_then(|items| items.first())
                .and_then(|item| item.as_array())
                .and_then(|labels| labels.first())
                .and_then(|label| label.as_str());
            if first == Some("No") {
                return Err(ToolError::Other("Question rejected".to_string()));
            }

            // TODO(integration): switch the session agent to `build` and append
            // the synthetic approval text part.
            Ok(ExecuteResult {
                title: "Switching to build agent".to_string(),
                output: "User approved switching to build agent. Wait for further instructions."
                    .to_string(),
                metadata: serde_json::json!({}),
                attachments: None,
            })
        },
    )
}
