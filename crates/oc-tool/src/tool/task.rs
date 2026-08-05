//! Port of `reference/packages/opencode/src/tool/task.ts`.

use crate::jsonschema;
use crate::model::{ExecuteResult, PermissionRequest, ToolContext, ToolError};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::tool;

const ID: &str = "task";

const BACKGROUND_DESCRIPTION: &str = "Background mode: background=true launches the subagent asynchronously and returns immediately. Foreground is the default; use it when you need the result before continuing. Use background only for independent work that can run while you continue elsewhere. You will be notified automatically when it finishes.";

// TODO(integration): used once background subagents are wired.
#[allow(dead_code)]
const BACKGROUND_STARTED: &str = "The task is working in the background. You will be notified automatically when it finishes.\nDO NOT sleep, poll for progress, ask the task for status, or duplicate this task's work — avoid working with the same files or topics it is using.\nWork on non-overlapping tasks, or briefly tell the user what you launched and end your response.";

#[allow(dead_code)]
const BACKGROUND_UPDATED: &str = "Additional context sent to the running background task.\nThe task is still working in the background. You will be notified automatically when it finishes.\nDO NOT sleep, poll for progress, ask the task for status, or duplicate this task's work — avoid working with the same files or topics it is using.\nWork on non-overlapping tasks, or briefly tell the user what you sent and end your response.";

/// `BaseParameterFields` from `reference/packages/opencode/src/tool/task.ts:43`.
fn base_parameter_fields() -> Vec<crate::schema::Property> {
    vec![
        prop("description", Schema::string("A short (3-5 words) description of the task")),
        prop("prompt", Schema::string("The task for the agent to perform")),
        prop("subagent_type", Schema::string("The type of specialized agent to use for this task")),
        opt_prop(
            "task_id",
            Schema::string("This should only be set if you mean to resume a previous task (you can pass a prior task_id and the task will continue the same subagent session as before instead of creating a fresh one)"),
        ),
        opt_prop("command", Schema::string("The command that triggered this task")),
    ]
}

/// `BaseParameters` from `reference/packages/opencode/src/tool/task.ts:54`.
pub fn base_parameters() -> Schema {
    Schema::struct_(base_parameter_fields(), "task")
}

/// `Parameters` from `reference/packages/opencode/src/tool/task.ts:56`.
pub fn parameters() -> Schema {
    let mut fields = base_parameter_fields();
    fields.push(opt_prop(
        "background",
        Schema::boolean("Run the agent in the background. You will be notified when it completes. DO NOT sleep, poll, or proactively check on its progress"),
    ));
    Schema::struct_(fields, "task")
}

/// `renderOutput` from `reference/packages/opencode/src/tool/task.ts:64`.
pub fn render_output(session_id: &str, state: &str, summary: Option<&str>, text: &str) -> String {
    let tag = if state == "error" {
        "task_error"
    } else {
        "task_result"
    };
    let mut lines = vec![format!("<task id=\"{session_id}\" state=\"{state}\">")];
    if let Some(summary) = summary {
        lines.push(format!("<summary>{summary}</summary>"));
    }
    lines.push(format!("<{tag}>"));
    lines.push(text.to_string());
    lines.push(format!("</{tag}>"));
    lines.push("</task>".to_string());
    lines.join("\n")
}

/// `TaskTool` from `reference/packages/opencode/src/tool/task.ts:81`.
pub fn def(experimental_background_subagents: bool) -> tool::Def {
    let description = if experimental_background_subagents {
        format!("{}\n\n{BACKGROUND_DESCRIPTION}", prompts::TASK)
    } else {
        prompts::TASK.to_string()
    };
    let json_schema = if experimental_background_subagents {
        None
    } else {
        Some(jsonschema::from_schema(&base_parameters()))
    };
    let mut raw = tool::Def {
        id: ID.to_string(),
        description,
        parameters: parameters(),
        json_schema,
        execute: std::sync::Arc::new(move |args, ctx| {
            Box::pin(run_task(args, ctx, experimental_background_subagents))
        }),
        format_validation_error: None,
    };
    raw = tool::wrap(ID, raw);
    raw
}

async fn run_task(
    args: serde_json::Value,
    ctx: &mut ToolContext,
    experimental_background_subagents: bool,
) -> Result<ExecuteResult, ToolError> {
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let subagent_type = args
        .get("subagent_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let task_id = args
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let background = args
        .get("background")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let run_in_background = background;
    if run_in_background && !experimental_background_subagents {
        return Err(ToolError::Other(
            "Background subagents require OPENCODE_EXPERIMENTAL_BACKGROUND_SUBAGENTS=true"
                .to_string(),
        ));
    }

    // TODO(integration): walk the session parent chain to compute the real
    // subagent depth; the top-level caller is depth 0.
    let parent_depth = 0usize;
    let depth = ctx.services.subagent_depth().unwrap_or(1);
    if parent_depth >= depth {
        return Err(ToolError::Other(format!(
            "Subagent depth limit reached ({depth}). Increase \"subagent_depth\" to allow nested subagents."
        )));
    }

    let bypass = ctx
        .extra
        .get("bypassAgentCheck")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !bypass {
        ctx.ask(PermissionRequest {
            permission: ID.to_string(),
            patterns: vec![subagent_type.clone()],
            always: vec!["*".to_string()],
            metadata: serde_json::json!({
                "description": description,
                "subagent_type": subagent_type,
            }),
        })?;
    }

    // TODO(integration): resolve the agent registry (`agent.get`), create or
    // resume the subagent session, run `TaskPromptOps.prompt`, and wire the
    // BackgroundJob lifecycle. The depth check above is intentionally computed
    // against the session parent chain in the reference; `subagent_depth`
    // stands in until `oc-session` provides parentIDs.
    let _ = &prompt;
    let _ = task_id;

    let metadata = serde_json::json!({
        "parentSessionId": ctx.session_id,
        "sessionId": format!("ses_subagent"),
        "model": ctx.extra.get("model").cloned().unwrap_or(serde_json::json!({})),
        "background": run_in_background,
    });

    ctx.metadata(crate::model::Metadata {
        title: Some(description.clone()),
        metadata: metadata.clone(),
    })?;

    let session_id = format!(
        "ses_{}",
        crate::util::identifier::ascending("ses").trim_start_matches("ses_")
    );
    let output = render_output(
        &session_id,
        "completed",
        None,
        "TODO(integration): subagent result text.",
    );

    Ok(ExecuteResult {
        title: description,
        metadata,
        output,
        attachments: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "background": {
                        "description": "Run the agent in the background. You will be notified when it completes. DO NOT sleep, poll, or proactively check on its progress",
                        "type": "boolean"
                    },
                    "command": { "description": "The command that triggered this task", "type": "string" },
                    "description": { "description": "A short (3-5 words) description of the task", "type": "string" },
                    "prompt": { "description": "The task for the agent to perform", "type": "string" },
                    "subagent_type": { "description": "The type of specialized agent to use for this task", "type": "string" },
                    "task_id": {
                        "description": "This should only be set if you mean to resume a previous task (you can pass a prior task_id and the task will continue the same subagent session as before instead of creating a fresh one)",
                        "type": "string"
                    }
                },
                "required": ["description", "prompt", "subagent_type"],
                "type": "object"
            })
        );
    }

    #[test]
    fn render_output_formats() {
        let output = render_output("ses_1", "completed", Some("done"), "text");
        assert!(output.contains("<task id=\"ses_1\" state=\"completed\">"));
        assert!(output.contains("<summary>done</summary>"));
        assert!(output.contains("<task_result>"));
    }
}
