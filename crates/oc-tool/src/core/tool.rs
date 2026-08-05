//! Port of `reference/packages/core/src/tool/tool.ts` — the V2 core tool
//! representation (`Tool.make`), plus the registration primitives
//! (`validateName`, `withPermission`, `definition`, `settle`).

use serde_json::Value as JsonValue;

use crate::model::{BoxFuture, Content, ToolCall, ToolContent, ToolError, ToolFailure, ToolOutput};
use crate::schema::Schema;

/// `Core.Tool.Context` — the V2 invocation context.
#[derive(Debug, Clone)]
pub struct CoreContext {
    pub session_id: String,
    pub agent: String,
    pub assistant_message_id: String,
    pub tool_call_id: String,
    /// The active Location directory (relative paths resolve from here).
    pub location_directory: String,
    /// Recorded V2 permission requests.
    pub asks: Vec<CorePermissionRequest>,
}

#[derive(Debug, Clone)]
pub struct CorePermissionRequest {
    pub action: String,
    pub resources: Vec<String>,
    pub save: Option<Vec<String>>,
    pub metadata: Option<JsonValue>,
    pub source: CorePermissionSource,
}

#[derive(Debug, Clone)]
pub struct CorePermissionSource {
    #[allow(dead_code)]
    pub message_id: String,
    #[allow(dead_code)]
    pub call_id: String,
}

impl CoreContext {
    /// `PermissionV2.assert` recording hook; the session layer enforces it.
    pub fn assert(&mut self, request: CorePermissionRequest) -> Result<(), ToolError> {
        self.asks.push(request);
        Ok(())
    }
}

pub type CoreExecute = std::sync::Arc<
    dyn for<'a> Fn(JsonValue, &'a mut CoreContext) -> BoxFuture<'a, Result<JsonValue, ToolError>>
        + Send
        + Sync,
>;

/// Structured-output projection for a core tool.
pub type ToStructuredOutput =
    std::sync::Arc<dyn Fn(&JsonValue, &JsonValue) -> JsonValue + Send + Sync>;

/// Model-output projection for a core tool.
pub type ToModelOutput =
    std::sync::Arc<dyn Fn(&JsonValue, &JsonValue) -> Vec<Content> + Send + Sync>;

/// `Core.Tool.Definition` from `reference/packages/core/src/tool/tool.ts:20`.
#[derive(Clone)]
pub struct CoreTool {
    pub description: String,
    pub input: Schema,
    pub output: Schema,
    pub structured: Option<Schema>,
    pub to_structured_output: Option<ToStructuredOutput>,
    pub to_model_output: Option<ToModelOutput>,
    pub execute: CoreExecute,
    pub permission: Option<String>,
}

/// Result of `settle`: the structured value plus the model-facing content.
#[derive(Debug, Clone)]
pub struct Settled {
    pub structured: JsonValue,
    pub content: Vec<ToolContent>,
}

/// `Tool.make` from `reference/packages/core/src/tool/tool.ts:71`.
#[allow(clippy::too_many_arguments)]
pub fn make(
    description: impl Into<String>,
    input: Schema,
    output: Schema,
    structured: Option<Schema>,
    to_structured_output: Option<ToStructuredOutput>,
    to_model_output: Option<ToModelOutput>,
    execute: impl Fn(JsonValue, &mut CoreContext) -> Result<JsonValue, ToolError>
        + Send
        + Sync
        + 'static,
) -> CoreTool {
    let execute = std::sync::Arc::new(execute);
    CoreTool {
        description: description.into(),
        input,
        output,
        structured,
        to_structured_output,
        to_model_output,
        execute: std::sync::Arc::new(move |args, ctx| {
            let execute = execute.clone();
            Box::pin(async move { execute(args, ctx) })
        }),
        permission: None,
    }
}

/// `validateName` from `reference/packages/core/src/tool/tool.ts:134`.
pub fn validate_name(name: &str) -> Result<(), String> {
    let re = regex::Regex::new(r"^[A-Za-z][A-Za-z0-9_-]{0,63}$").unwrap();
    if re.is_match(name) {
        Ok(())
    } else {
        Err(format!("Invalid tool name: {name}"))
    }
}

/// `withPermission` from `reference/packages/core/src/tool/tool.ts:139`.
pub fn with_permission(mut tool: CoreTool, permission: &str) -> CoreTool {
    tool.permission = Some(permission.to_string());
    tool
}

/// `Tool.permission` from `reference/packages/core/src/tool/tool.ts:148`.
pub fn permission(tool: &CoreTool, name: &str) -> String {
    tool.permission.clone().unwrap_or_else(|| name.to_string())
}

/// `toJsonSchema` from `reference/packages/core/src/tool/tool.ts:158`.
pub fn to_json_schema(schema: &Schema) -> JsonValue {
    crate::schema::to_document(schema, false)
}

/// `Tool.definition` from `reference/packages/core/src/tool/tool.ts:149`.
pub fn definition(name: &str, tool: &CoreTool) -> crate::model::ToolDefinition {
    crate::model::ToolDefinition {
        name: name.to_string(),
        description: tool.description.clone(),
        input_schema: to_json_schema(&tool.input),
        output_schema: Some(to_json_schema(
            tool.structured.as_ref().unwrap_or(&tool.output),
        )),
    }
}

/// `Tool.settle` from `reference/packages/core/src/tool/tool.ts:150`.
pub fn settle(
    tool: &CoreTool,
    call: &ToolCall,
    context: &mut CoreContext,
) -> Result<Settled, ToolError> {
    tool.input
        .validate(&call.input)
        .map_err(|error| ToolError::failure(format!("Invalid tool input: {error}")))?;
    let output = poll(tool, call.input.clone(), call, context)?;
    tool.output.validate(&output).map_err(|error| {
        ToolError::failure(format!(
            "Tool returned an invalid value for its output schema: {error}"
        ))
    })?;

    let structured = match (&tool.structured, &tool.to_structured_output) {
        (Some(schema), Some(map)) => {
            let value = map(&call.input, &output);
            schema.validate(&value).map_err(|error| {
                ToolError::failure(format!(
                    "Tool returned an invalid value for its output schema: {error}"
                ))
            })?;
            value
        }
        _ => output.clone(),
    };

    let content = match &tool.to_model_output {
        Some(to_model_output) => to_model_output(&call.input, &output)
            .into_iter()
            .map(|part| match part {
                Content::Text { text } => ToolContent::Text { text },
                Content::File { data, mime, name } => ToolContent::File {
                    uri: format!("data:{mime};base64,{data}"),
                    mime,
                    name,
                },
            })
            .collect(),
        None => match &output {
            JsonValue::String(text) => vec![ToolContent::Text { text: text.clone() }],
            _ => Vec::new(),
        },
    };

    Ok(Settled {
        structured,
        content,
    })
}

fn poll(
    tool: &CoreTool,
    input: JsonValue,
    _call: &ToolCall,
    context: &mut CoreContext,
) -> Result<JsonValue, ToolError> {
    // BoxFuture is a single-use future; run it with a mini executor. The
    // session runner will host its own runtime; here we use a one-shot tokio
    // runtime when needed.
    let future = (tool.execute)(input, context);
    run_future(future)
}

pub fn run_future<T>(future: BoxFuture<'_, Result<T, ToolError>>) -> Result<T, ToolError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(future),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("failed to start tokio runtime")
            .block_on(future),
    }
}

/// `ToolOutput` projection for a settled result, mirroring `ToolOutput.make`.
pub fn project_output(settled: Settled) -> ToolOutput {
    ToolOutput::make(settled.structured, settled.content)
}

/// `Core.Tool.Failure` — recoverable tool failure wrapper.
pub fn failure(message: impl Into<String>) -> ToolError {
    ToolError::Failure(ToolFailure::new(message))
}
