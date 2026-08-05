//! Port of `reference/packages/opencode/src/tool/todo.ts`.

use crate::model::{ExecuteResult, PermissionRequest};
use crate::prompts;
use crate::schema::{prop, Schema};

/// `Todo.Info` item schema (`reference/packages/schema/src/session-todo.ts:7`).
pub fn todo_item_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("content", Schema::string("Brief description of the task")),
            prop(
                "status",
                Schema::string(
                    "Current status of the task: pending, in_progress, completed, cancelled",
                ),
            ),
            prop(
                "priority",
                Schema::string("Priority level of the task: high, medium, low"),
            ),
        ],
        "todo",
    )
}

/// `Parameters` from `reference/packages/opencode/src/tool/todo.ts:6`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![prop(
            "todos",
            Schema::array(todo_item_schema(), "The updated todo list"),
        )],
        "todowrite",
    )
}

/// `TodoWriteTool` from `reference/packages/opencode/src/tool/todo.ts:14`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def(
        "todowrite",
        prompts::TODOWRITE,
        parameters(),
        |args, ctx| {
            let todos = args
                .get("todos")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            ctx.ask(PermissionRequest {
                permission: "todowrite".to_string(),
                patterns: vec!["*".to_string()],
                always: vec!["*".to_string()],
                metadata: serde_json::json!({}),
            })?;
            ctx.services.todo_update(&ctx.session_id, &todos)?;
            let pending = todos
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| item.get("status") != Some(&serde_json::json!("completed")))
                        .count()
                })
                .unwrap_or(0);
            Ok(ExecuteResult {
                title: format!("{pending} todos"),
                output: serde_json::to_string_pretty(&todos).unwrap_or_else(|_| todos.to_string()),
                metadata: serde_json::json!({ "todos": todos }),
                attachments: None,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;
    use crate::tool::tool;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "todos": {
                        "description": "The updated todo list",
                        "items": {
                            "properties": {
                                "content": { "description": "Brief description of the task", "type": "string" },
                                "priority": { "description": "Priority level of the task: high, medium, low", "type": "string" },
                                "status": { "description": "Current status of the task: pending, in_progress, completed, cancelled", "type": "string" }
                            },
                            "required": ["content", "status", "priority"],
                            "type": "object"
                        },
                        "type": "array"
                    }
                },
                "required": ["todos"],
                "type": "object"
            })
        );
    }

    #[tokio::test]
    async fn executes_and_counts_pending() {
        let def = tool::wrap("todowrite", def());
        let mut ctx = ToolContext {
            session_id: "ses_1".to_string(),
            ..Default::default()
        };
        let result = def
            .execute(
                serde_json::json!({
                    "todos": [
                        { "content": "a", "status": "in_progress", "priority": "high" },
                        { "content": "b", "status": "completed", "priority": "low" }
                    ]
                }),
                &mut ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.title, "1 todos");
        assert_eq!(ctx.asks.len(), 1);
    }
}
