//! Port of `reference/packages/opencode/src/tool/tool.ts`.
//!
//! The `Def` / `Info` tool representation used by the opencode (V1) session
//! runtime. `define` builds a lazy `Info`; `init` resolves it to a `Def`
//! whose `execute` is wrapped with argument decoding and generic output
//! truncation exactly like the reference `wrap`.

use serde_json::Value as JsonValue;

use crate::jsonschema;
use crate::model::{BoxFuture, ExecuteResult, ToolContext, ToolError};
use crate::schema::Schema;
use crate::truncate;

pub type ExecuteFn = std::sync::Arc<
    dyn for<'a> Fn(
            JsonValue,
            &'a mut ToolContext,
        ) -> BoxFuture<'a, Result<ExecuteResult, ToolError>>
        + Send
        + Sync,
>;

/// Wrap a synchronous handler as an `ExecuteFn`.
pub fn sync_execute<F>(handler: F) -> ExecuteFn
where
    F: Fn(JsonValue, &mut ToolContext) -> Result<ExecuteResult, ToolError> + Send + Sync + 'static,
{
    let handler = std::sync::Arc::new(handler);
    std::sync::Arc::new(move |args, ctx| {
        let handler = handler.clone();
        Box::pin(async move { handler(args, ctx) })
    })
}

/// `Tool.Def` from `reference/packages/opencode/src/tool/tool.ts:55`.
#[derive(Clone)]
pub struct Def {
    pub id: String,
    pub description: String,
    pub parameters: Schema,
    /// Explicit JSON Schema override (e.g. task's `BaseParameters`).
    pub json_schema: Option<JsonValue>,
    pub execute: ExecuteFn,
    pub format_validation_error: Option<std::sync::Arc<dyn Fn(&str) -> String + Send + Sync>>,
}

impl Def {
    pub fn with_description(mut self, description: String) -> Self {
        self.description = description;
        self
    }

    /// Run this tool against decoded arguments.
    pub async fn execute<'a>(
        &'a self,
        args: JsonValue,
        ctx: &'a mut ToolContext,
    ) -> Result<ExecuteResult, ToolError> {
        (self.execute)(args, ctx).await
    }
}

impl Def {
    /// `ToolJsonSchema.fromTool` from `reference/packages/opencode/src/tool/json-schema.ts:24`.
    pub fn json_schema(&self) -> JsonValue {
        match &self.json_schema {
            Some(schema) => schema.clone(),
            None => jsonschema::from_schema(&self.parameters),
        }
    }
}

/// `Tool.Info` from `reference/packages/opencode/src/tool/tool.ts:71`.
pub struct Info {
    pub id: String,
    pub init: Box<dyn Fn() -> Def + Send + Sync>,
}
pub fn define<F>(id: &str, init: F) -> Info
where
    F: Fn() -> Def + Send + Sync + 'static,
{
    let id = id.to_string();
    Info {
        id: id.clone(),
        init: Box::new(move || wrap(&id, init())),
    }
}

/// `Tool.init` from `reference/packages/opencode/src/tool/tool.ts:171`.
pub fn init(info: &Info) -> Def {
    (info.init)()
}

/// `wrap` from `reference/packages/opencode/src/tool/tool.ts:99`.
///
/// Hoists argument decoding and output truncation onto a raw `Def`.
pub fn wrap(id: &str, mut tool: Def) -> Def {
    let parameters = tool.parameters.clone();
    let format_validation_error = tool.format_validation_error.take();
    let inner = std::mem::replace(
        &mut tool.execute,
        sync_execute(|_, _| Err(ToolError::Other("uninitialized execute".into()))),
    );
    let id = id.to_string();

    let execute: ExecuteFn = std::sync::Arc::new(move |args, ctx| {
        if let Err(detail) = parameters.validate(&args) {
            let message = match &format_validation_error {
                Some(format) => {
                    format!("The {id} tool was called with invalid arguments: {}.\nPlease rewrite the input so it satisfies the expected schema.", format(&detail))
                }
                None => format!(
                    "The {id} tool was called with invalid arguments: {detail}.\nPlease rewrite the input so it satisfies the expected schema."
                ),
            };
            let failure = ToolError::InvalidArguments {
                tool: id.clone(),
                detail,
                message,
            };
            return Box::pin(async move { Err(failure) });
        }
        let decoded = args;
        let inner = inner.clone();
        let _id = id.clone();
        Box::pin(async move {
            let result = inner(decoded, ctx).await?;
            if result.metadata.get("truncated").is_some() {
                return Ok(result);
            }
            let agent_permission = ctx.agent_permission().map(|rules| rules.to_vec());
            let truncated = truncate::output(
                &result.output,
                truncate::Options::default(),
                agent_permission.as_deref(),
            );
            let mut metadata = result.metadata.clone();
            metadata["truncated"] = JsonValue::Bool(truncated.truncated);
            if truncated.truncated {
                if let Some(path) = &truncated.output_path {
                    metadata["outputPath"] = JsonValue::String(path.clone());
                }
            }
            Ok(ExecuteResult {
                title: result.title,
                metadata,
                output: truncated.content,
                attachments: result.attachments,
            })
        })
    });

    tool.execute = execute;
    tool
}

/// Convenience builder used by the tool leaves.
pub fn def(
    id: &str,
    description: impl Into<String>,
    parameters: Schema,
    execute: impl Fn(JsonValue, &mut ToolContext) -> Result<ExecuteResult, ToolError>
        + Send
        + Sync
        + 'static,
) -> Def {
    Def {
        id: id.to_string(),
        description: description.into(),
        parameters,
        json_schema: None,
        execute: sync_execute(execute),
        format_validation_error: None,
    }
}

/// Convenience builder for async tool leaves.
pub fn def_async<F>(id: &str, description: impl Into<String>, parameters: Schema, execute: F) -> Def
where
    F: for<'a> Fn(
            JsonValue,
            &'a mut ToolContext,
        ) -> BoxFuture<'a, Result<ExecuteResult, ToolError>>
        + Send
        + Sync
        + 'static,
{
    Def {
        id: id.to_string(),
        description: description.into(),
        parameters,
        json_schema: None,
        execute: std::sync::Arc::new(execute),
        format_validation_error: None,
    }
}
