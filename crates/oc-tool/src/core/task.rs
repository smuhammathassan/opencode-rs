//! Core task adapter for `reference/packages/opencode/src/tool/task.ts`.
//!
//! The core engine owns the schema, permission, depth, and output contract;
//! the host owns child-session execution through `CoreContext`.

use crate::core::tool::{self, CoreContext, CoreTool};
use crate::model::{Content, SubagentRequest, ToolError};
use crate::schema::{opt_prop, prop, Schema};

pub const NAME: &str = "task";

const BACKGROUND_STARTED: &str = "The task is working in the background. You will be notified automatically when it finishes.\nDO NOT sleep, poll for progress, ask the task for status, or duplicate this task's work — avoid working with the same files or topics it is using.\nWork on non-overlapping tasks, or briefly tell the user what you launched and end your response.";

const BACKGROUND_DISABLED: &str =
    "Background subagents require OPENCODE_EXPERIMENTAL_BACKGROUND_SUBAGENTS=true";

pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "description",
                Schema::string("A short (3-5 words) description of the task"),
            ),
            prop(
                "prompt",
                Schema::string("The task for the agent to perform"),
            ),
            prop(
                "subagent_type",
                Schema::string("The type of specialized agent to use for this task"),
            ),
            opt_prop(
                "task_id",
                Schema::string("Resume a previous subagent session by id"),
            ),
            opt_prop(
                "command",
                Schema::string("The command that triggered this task"),
            ),
            opt_prop(
                "background",
                Schema::boolean("Run the agent in the background"),
            ),
        ],
        "task",
    )
}

/// Materialize the task tool. Background execution is deliberately disabled
/// until the server supplies a lifecycle-aware child-session host.
pub fn def(experimental_background_subagents: bool) -> CoreTool {
    let tool = tool::make(
        crate::prompts::TASK,
        parameters(),
        Schema::Raw(serde_json::json!({})),
        None,
        None,
        Some(std::sync::Arc::new(|_input, output| {
            let text = output
                .get("output")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            vec![Content::Text { text }]
        })),
        move |input, context| execute(input, context, experimental_background_subagents),
    );
    tool::with_permission(tool, NAME)
}

fn execute(
    input: serde_json::Value,
    context: &mut CoreContext,
    experimental_background_subagents: bool,
) -> Result<serde_json::Value, ToolError> {
    let description = input
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let prompt = input
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let subagent_type = input
        .get("subagent_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let task_id = input
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let background = input
        .get("background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    if background && !experimental_background_subagents {
        return Err(ToolError::failure(BACKGROUND_DISABLED));
    }

    let depth = context.subagent_depth.unwrap_or(1);
    let parent_depth = (context.subagent_parent_depth)(&context.session_id);
    if parent_depth >= depth {
        return Err(ToolError::failure(format!(
            "Subagent depth limit reached ({depth}). Increase \"subagent_depth\" to allow nested subagents."
        )));
    }

    context.assert(crate::core::tool::CorePermissionRequest {
        action: NAME.to_string(),
        resources: vec![subagent_type.clone()],
        save: Some(vec!["*".to_string()]),
        metadata: Some(serde_json::json!({
            "description": description,
            "subagent_type": subagent_type,
        })),
        source: crate::core::tool::CorePermissionSource {
            message_id: context.assistant_message_id.clone(),
            call_id: context.tool_call_id.clone(),
        },
    })?;

    let Some(execute_subagent) = context.execute_subagent.clone() else {
        return Err(ToolError::failure(
            "Subagent execution is not configured for this server tool runtime",
        ));
    };

    let request = SubagentRequest {
        parent_session_id: context.session_id.clone(),
        parent_message_id: context.assistant_message_id.clone(),
        description: description.clone(),
        prompt,
        subagent_type,
        task_id,
        command: input
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        background,
    };
    let result = tool::run_future((execute_subagent)(request)).map_err(ToolError::failure)?;
    let output = if result.output.is_empty() && result.state == "running" {
        BACKGROUND_STARTED.to_string()
    } else {
        result.output.clone()
    };

    Ok(serde_json::json!({
        "title": description,
        "sessionId": result.session_id,
        "state": result.state,
        "summary": result.summary,
        "output": output,
        "metadata": result.metadata,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::{CoreToolRegistry, ExecuteInput, Settlement};
    use crate::core::tool::CoreSubagentExecute;
    use crate::model::{SubagentResult, ToolCall};
    use serde_json::json;
    use std::sync::Arc;

    fn context(execute_subagent: Option<CoreSubagentExecute>) -> CoreContext {
        CoreContext {
            session_id: "ses_parent".into(),
            agent: "build".into(),
            assistant_message_id: "msg_parent".into(),
            tool_call_id: "call_task".into(),
            location_directory: "/tmp".into(),
            asks: Vec::new(),
            subagent_depth: Some(2),
            subagent_parent_depth: Arc::new(|_| 0),
            execute_subagent,
            lsp_request: None,
        }
    }

    #[test]
    fn configured_callback_is_materialized_and_rendered() {
        let execute: CoreSubagentExecute = Arc::new(|request| {
            assert_eq!(request.parent_session_id, "ses_parent");
            Box::pin(async {
                Ok(SubagentResult {
                    session_id: "ses_child".into(),
                    state: "completed".into(),
                    summary: Some("done".into()),
                    output: "child output".into(),
                    metadata: json!({ "agent": "explore" }),
                })
            })
        });
        let mut registry = CoreToolRegistry::with_applications();
        let _ = registry
            .register(vec![(NAME.into(), def(false))])
            .expect("valid task registration");
        let materialization = registry.materialize(&[]);
        assert!(materialization
            .definitions
            .iter()
            .any(|definition| definition.name == NAME));
        let mut input = ExecuteInput {
            session_id: "ses_parent".into(),
            agent: "build".into(),
            assistant_message_id: "msg_parent".into(),
            call: ToolCall {
                id: "call_task".into(),
                name: NAME.into(),
                input: json!({
                    "description": "inspect files",
                    "prompt": "Read and summarize",
                    "subagent_type": "explore"
                }),
            },
        };
        let mut context = context(Some(execute));
        let settlement = (materialization.settle)(&mut input, &mut context);
        let Settlement::Ok {
            output: Some(output),
            ..
        } = settlement
        else {
            panic!("expected completed task settlement")
        };
        assert_eq!(output.structured["sessionId"], "ses_child");
        assert_eq!(output.structured["state"], "completed");
        assert_eq!(output.content.len(), 1);
        assert!(matches!(
            &output.content[0],
            crate::model::ToolContent::Text { text } if text == "child output"
        ));
        assert_eq!(context.asks[0].action, NAME);
    }

    #[test]
    fn missing_callback_is_a_tool_error_not_success() {
        let mut registry = CoreToolRegistry::with_applications();
        let _ = registry
            .register(vec![(NAME.into(), def(false))])
            .expect("valid task registration");
        let materialization = registry.materialize(&[]);
        let mut input = ExecuteInput {
            session_id: "ses_parent".into(),
            agent: "build".into(),
            assistant_message_id: "msg_parent".into(),
            call: ToolCall {
                id: "call_task".into(),
                name: NAME.into(),
                input: json!({
                    "description": "inspect files",
                    "prompt": "Read and summarize",
                    "subagent_type": "explore"
                }),
            },
        };
        let mut context = context(None);
        let settlement = (materialization.settle)(&mut input, &mut context);
        let Settlement::Error { value } = settlement else {
            panic!("missing callback must fail closed")
        };
        assert!(value.contains("Subagent execution is not configured"));
    }

    #[test]
    fn depth_limit_is_checked_before_callback() {
        let execute: CoreSubagentExecute = Arc::new(|_| {
            Box::pin(async {
                Ok(SubagentResult {
                    session_id: "ses_child".into(),
                    state: "completed".into(),
                    summary: None,
                    output: "should not run".into(),
                    metadata: json!({}),
                })
            })
        });
        let mut registry = CoreToolRegistry::with_applications();
        let _ = registry
            .register(vec![(NAME.into(), def(false))])
            .expect("valid task registration");
        let materialization = registry.materialize(&[]);
        let mut input = ExecuteInput {
            session_id: "ses_parent".into(),
            agent: "build".into(),
            assistant_message_id: "msg_parent".into(),
            call: ToolCall {
                id: "call_task".into(),
                name: NAME.into(),
                input: json!({
                    "description": "inspect files",
                    "prompt": "Read and summarize",
                    "subagent_type": "explore"
                }),
            },
        };
        let mut context = context(Some(execute));
        context.subagent_parent_depth = Arc::new(|_| 2);
        let settlement = (materialization.settle)(&mut input, &mut context);
        let Settlement::Error { value } = settlement else {
            panic!("depth limit must fail")
        };
        assert!(value.contains("Subagent depth limit reached (2)"));
        assert!(context.asks.is_empty());
    }
}
