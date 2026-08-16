//! Port of `reference/packages/opencode/src/tool/task.ts`.

use crate::jsonschema;
use crate::model::{
    ExecuteResult, PermissionRequest, SubagentRequest, SubagentResult, ToolContext, ToolError,
};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::tool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const ID: &str = "task";

const BACKGROUND_DESCRIPTION: &str = "Background mode: background=true launches the subagent asynchronously and returns immediately. Foreground is the default; use it when you need the result before continuing. Use background only for independent work that can run while you continue elsewhere. You will be notified automatically when it finishes.";

// TODO(integration): used once background subagents are wired.
#[allow(dead_code)]
const BACKGROUND_STARTED: &str = "The task is working in the background. You will be notified automatically when it finishes.\nDO NOT sleep, poll for progress, ask the task for status, or duplicate this task's work — avoid working with the same files or topics it is using.\nWork on non-overlapping tasks, or briefly tell the user what you launched and end your response.";

#[allow(dead_code)]
const BACKGROUND_UPDATED: &str = "Additional context sent to the running background task.\nThe task is still working in the background. You will be notified automatically when it finishes.\nDO NOT sleep, poll for progress, ask the task for status, or duplicate this task's work — avoid working with the same files or topics it is using.\nWork on non-overlapping tasks, or briefly tell the user what you sent and end your response.";

const SUBAGENT_ABORT_POLL: std::time::Duration = std::time::Duration::from_millis(10);

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

    let parent_depth = ctx.services.subagent_parent_depth(&ctx.session_id);
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

    let fallback_session_id = task_id.clone().unwrap_or_else(|| {
        format!(
            "ses_{}",
            crate::util::identifier::ascending("ses").trim_start_matches("ses_")
        )
    });
    let request = SubagentRequest {
        parent_session_id: ctx.session_id.clone(),
        parent_message_id: ctx.message_id.clone(),
        description: description.clone(),
        prompt,
        subagent_type,
        task_id: task_id.clone(),
        command: args
            .get("command")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        background: run_in_background,
    };
    let services = ctx.services.clone();
    let result =
        match execute_with_abort(services.clone(), request.clone(), ctx.aborted.clone()).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => SubagentResult {
                session_id: fallback_session_id.clone(),
                state: "error".to_string(),
                summary: None,
                output: error,
                metadata: serde_json::json!({}),
            },
            Err(()) => SubagentResult {
                session_id: fallback_session_id,
                state: "cancelled".to_string(),
                summary: Some("Subagent cancelled".to_string()),
                output: "The subagent task was cancelled.".to_string(),
                metadata: serde_json::json!({}),
            },
        };

    let _ = services.notify_subagent(&request, &result);
    if result.state != "running" {
        let _ = services.cleanup_subagent(&request, Some(&result.session_id));
    }

    let mut metadata = match result.metadata {
        serde_json::Value::Object(metadata) => metadata,
        other => serde_json::Map::from_iter([("result".to_string(), other)]),
    };
    metadata.insert(
        "parentSessionId".to_string(),
        serde_json::Value::String(ctx.session_id.clone()),
    );
    metadata.insert(
        "sessionId".to_string(),
        serde_json::Value::String(result.session_id.clone()),
    );
    metadata.insert(
        "model".to_string(),
        ctx.extra
            .get("model")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    );
    metadata.insert(
        "background".to_string(),
        serde_json::Value::Bool(run_in_background),
    );
    metadata.insert(
        "state".to_string(),
        serde_json::Value::String(result.state.clone()),
    );
    let metadata = serde_json::Value::Object(metadata);
    ctx.metadata(crate::model::Metadata {
        title: Some(description.clone()),
        metadata: metadata.clone(),
    })?;

    let output = if result.output.is_empty() && result.state == "running" {
        BACKGROUND_STARTED.to_string()
    } else {
        result.output
    };
    let rendered = render_output(
        &result.session_id,
        &result.state,
        result.summary.as_deref(),
        &output,
    );

    Ok(ExecuteResult {
        title: description,
        metadata,
        output: rendered,
        attachments: None,
    })
}

async fn execute_with_abort(
    services: Arc<dyn crate::model::ToolServices>,
    request: SubagentRequest,
    aborted: Arc<AtomicBool>,
) -> Result<Result<SubagentResult, String>, ()> {
    if aborted.load(Ordering::Relaxed) {
        let _ = services.cancel_subagent(&request);
        return Err(());
    }

    let future = services.execute_subagent(request.clone());
    tokio::pin!(future);
    tokio::select! {
        result = &mut future => Ok(result),
        _ = wait_for_abort(aborted) => {
            let _ = services.cancel_subagent(&request);
            Err(())
        }
    }
}

async fn wait_for_abort(aborted: Arc<AtomicBool>) {
    while !aborted.load(Ordering::Relaxed) {
        tokio::time::sleep(SUBAGENT_ABORT_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::{BoxFuture, ToolServices};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct CallbackServices {
        result: Result<SubagentResult, String>,
        parent_depth: usize,
        depth: Option<usize>,
        lifecycle: Arc<Mutex<Vec<String>>>,
        started: Option<Arc<tokio::sync::Notify>>,
        pending: bool,
    }

    impl ToolServices for CallbackServices {
        fn subagent_depth(&self) -> Option<usize> {
            self.depth
        }

        fn subagent_parent_depth(&self, _session_id: &str) -> usize {
            self.parent_depth
        }

        fn execute_subagent(
            &self,
            _request: SubagentRequest,
        ) -> BoxFuture<'static, Result<SubagentResult, String>> {
            let result = self.result.clone();
            let lifecycle = self.lifecycle.clone();
            let started = self.started.clone();
            let pending = self.pending;
            Box::pin(async move {
                if let Some(started) = started {
                    started.notify_one();
                }
                if pending {
                    std::future::pending().await
                } else {
                    let _ = lifecycle;
                    result
                }
            })
        }

        fn notify_subagent(
            &self,
            _request: &SubagentRequest,
            result: &SubagentResult,
        ) -> Result<(), String> {
            self.lifecycle
                .lock()
                .unwrap()
                .push(format!("notify:{}", result.state));
            Ok(())
        }

        fn cancel_subagent(&self, _request: &SubagentRequest) -> Result<(), String> {
            self.lifecycle.lock().unwrap().push("cancel".into());
            Ok(())
        }

        fn cleanup_subagent(
            &self,
            _request: &SubagentRequest,
            session_id: Option<&str>,
        ) -> Result<(), String> {
            self.lifecycle
                .lock()
                .unwrap()
                .push(format!("cleanup:{}", session_id.unwrap_or("unknown")));
            Ok(())
        }
    }

    fn callback_services(result: Result<SubagentResult, String>) -> CallbackServices {
        CallbackServices {
            result,
            parent_depth: 0,
            depth: Some(1),
            lifecycle: Arc::new(Mutex::new(Vec::new())),
            started: None,
            pending: false,
        }
    }

    fn task_args() -> serde_json::Value {
        serde_json::json!({
            "description": "inspect files",
            "prompt": "Read the files and summarize them",
            "subagent_type": "explore"
        })
    }

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

    #[tokio::test]
    async fn callback_result_is_rendered_without_placeholder_text() {
        let services = CallbackServices {
            result: Ok(SubagentResult {
                session_id: "ses_child".into(),
                state: "completed".into(),
                summary: Some("finished reading".into()),
                output: "The files contain three functions.".into(),
                metadata: serde_json::json!({ "agent": "explore" }),
            }),
            parent_depth: 0,
            depth: Some(1),
            lifecycle: Arc::new(Mutex::new(Vec::new())),
            started: None,
            pending: false,
        };
        let mut context = ToolContext {
            session_id: "ses_parent".into(),
            message_id: "msg_parent".into(),
            services: Arc::new(services),
            ..Default::default()
        };
        let result = def(false)
            .execute(task_args(), &mut context)
            .await
            .expect("callback task result");

        assert!(result.output.contains("The files contain three functions."));
        assert!(result
            .output
            .contains("<summary>finished reading</summary>"));
        assert!(!result.output.contains("TODO(integration)"));
        assert_eq!(result.metadata["state"], "completed");
    }

    #[tokio::test]
    async fn callback_error_is_rendered_as_task_error() {
        let services = CallbackServices {
            result: Err("child runner failed".into()),
            parent_depth: 0,
            depth: Some(1),
            lifecycle: Arc::new(Mutex::new(Vec::new())),
            started: None,
            pending: false,
        };
        let mut context = ToolContext {
            session_id: "ses_parent".into(),
            services: Arc::new(services),
            ..Default::default()
        };
        let result = def(false)
            .execute(task_args(), &mut context)
            .await
            .expect("rendered callback error");

        assert!(result.output.contains("state=\"error\""));
        assert!(result.output.contains("<task_error>"));
        assert!(result.output.contains("child runner failed"));
    }

    #[tokio::test]
    async fn parent_depth_is_checked_before_callback() {
        let services = CallbackServices {
            result: Ok(SubagentResult {
                session_id: "ses_never".into(),
                state: "completed".into(),
                summary: None,
                output: "should not run".into(),
                metadata: serde_json::json!({}),
            }),
            parent_depth: 1,
            depth: Some(1),
            lifecycle: Arc::new(Mutex::new(Vec::new())),
            started: None,
            pending: false,
        };
        let mut context = ToolContext {
            session_id: "ses_nested".into(),
            services: Arc::new(services),
            ..Default::default()
        };
        let error = def(false)
            .execute(task_args(), &mut context)
            .await
            .expect_err("depth limit");
        assert!(error.to_string().contains("Subagent depth limit reached"));
    }

    #[tokio::test]
    async fn terminal_child_notifies_then_cleans_up() {
        let services = callback_services(Ok(SubagentResult {
            session_id: "ses_child".into(),
            state: "completed".into(),
            summary: None,
            output: "done".into(),
            metadata: serde_json::json!({}),
        }));
        let lifecycle = services.lifecycle.clone();
        let mut context = ToolContext {
            session_id: "ses_parent".into(),
            services: Arc::new(services),
            ..Default::default()
        };

        def(false)
            .execute(task_args(), &mut context)
            .await
            .expect("terminal child");

        assert_eq!(
            *lifecycle.lock().unwrap(),
            vec!["notify:completed", "cleanup:ses_child"]
        );
    }

    #[tokio::test]
    async fn background_child_notifies_without_premature_cleanup() {
        let mut args = task_args();
        args["background"] = serde_json::Value::Bool(true);
        let services = callback_services(Ok(SubagentResult {
            session_id: "ses_background".into(),
            state: "running".into(),
            summary: None,
            output: String::new(),
            metadata: serde_json::json!({}),
        }));
        let lifecycle = services.lifecycle.clone();
        let mut context = ToolContext {
            session_id: "ses_parent".into(),
            services: Arc::new(services),
            ..Default::default()
        };

        let result = def(true)
            .execute(args, &mut context)
            .await
            .expect("background child");

        assert!(result.output.contains("working in the background"));
        assert_eq!(*lifecycle.lock().unwrap(), vec!["notify:running"]);
    }

    #[tokio::test]
    async fn abort_cancels_and_cleans_up_pending_child() {
        let started = Arc::new(tokio::sync::Notify::new());
        let services = CallbackServices {
            result: Ok(SubagentResult {
                session_id: "ses_never".into(),
                state: "completed".into(),
                summary: None,
                output: "should not finish".into(),
                metadata: serde_json::json!({}),
            }),
            parent_depth: 0,
            depth: Some(1),
            lifecycle: Arc::new(Mutex::new(Vec::new())),
            started: Some(started.clone()),
            pending: true,
        };
        let lifecycle = services.lifecycle.clone();
        let mut context = ToolContext {
            session_id: "ses_parent".into(),
            services: Arc::new(services),
            ..Default::default()
        };
        let aborted = context.aborted.clone();
        let mut args = task_args();
        args["task_id"] = serde_json::Value::String("ses_child".into());

        let task = tokio::spawn(async move { def(false).execute(args, &mut context).await });
        started.notified().await;
        aborted.store(true, Ordering::Relaxed);
        let result = task.await.unwrap().expect("cancelled child result");

        assert!(result.output.contains("state=\"cancelled\""));
        assert_eq!(
            *lifecycle.lock().unwrap(),
            vec!["cancel", "notify:cancelled", "cleanup:ses_child"]
        );
    }
}
