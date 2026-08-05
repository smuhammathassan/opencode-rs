//! Type-safe and dynamic LLM tools.
//! From reference/packages/llm/src/tool.ts

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::schema::messages::{
    ToolCallPart, ToolContent, ToolDefinition, ToolOutput, ToolResultValue,
};

/// JSON schema encode/decode closure for a tool.
pub type ToolCodec = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

/// `ToolSchema` — parameter / success codec constraint (Effect `Schema.Codec`
/// has no Rust equivalent; the schema shape and codecs are captured directly).
/// From reference/packages/llm/src/tool.ts (`ToolSchema`)
#[derive(Clone)]
pub struct ToolSchema {
    pub json_schema: Value,
    pub decode: ToolCodec,
    pub encode: ToolCodec,
}

impl ToolSchema {
    /// Identity codec (dynamic tools and `Schema.Unknown`).
    pub fn unknown() -> ToolSchema {
        ToolSchema {
            json_schema: Value::Object(Default::default()),
            decode: Arc::new(|value| Ok(value.clone())),
            encode: Arc::new(|value| Ok(value.clone())),
        }
    }

    pub fn new(json_schema: Value) -> ToolSchema {
        ToolSchema {
            json_schema,
            decode: Arc::new(|value| Ok(value.clone())),
            encode: Arc::new(|value| Ok(value.clone())),
        }
    }
}

/// `ToolExecuteContext`.
/// From reference/packages/llm/src/tool.ts
#[derive(Debug, Clone)]
pub struct ToolExecuteContext {
    pub id: String,
    pub name: String,
}

/// `ToolModelOutputInput`.
/// From reference/packages/llm/src/tool.ts
#[derive(Debug, Clone)]
pub struct ToolModelOutputInput<'a> {
    pub call_id: &'a str,
    pub parameters: &'a Value,
    pub output: &'a Value,
}

/// `ToolExecute` — handler returning a future `Result<Value, ToolFailure>`.
pub type ToolExecute = dyn Fn(
        Value,
        ToolExecuteContext,
    )
        -> Pin<Box<dyn Future<Output = Result<Value, crate::schema::ToolFailure>> + Send + 'static>>
    + Send
    + Sync;

/// `ToolToModelOutput` — projects an executed output into `ToolContent`s.
pub type ToolToModelOutput = dyn Fn(&ToolModelOutputInput) -> Vec<ToolContent> + Send + Sync;

/// `ToolToStructuredOutput` — projects the encoded output value.
pub type ToolToStructuredOutput = dyn Fn(&Value) -> Value + Send + Sync;

/// `Tool` — a description plus parameter/success schemas and an optional
/// execute handler.
/// From reference/packages/llm/src/tool.ts (`Tool`)
#[derive(Clone)]
pub struct Tool {
    pub description: String,
    pub parameters: ToolSchema,
    pub success: ToolSchema,
    pub execute: Option<Arc<ToolExecute>>,
    pub to_model_output: Option<Arc<ToolToModelOutput>>,
    pub to_structured_output: Option<Arc<ToolToStructuredOutput>>,
    pub decode: ToolCodec,
    pub encode: ToolCodec,
    pub legacy_result: bool,
    pub definition: ToolDefinition,
}

/// `ToolConfig` — the ergonomic `Tool.make` input.
/// From reference/packages/llm/src/tool.ts (`make`)
pub struct ToolConfig {
    pub description: String,
    pub json_schema: Option<Value>,
    pub output_schema: Option<Value>,
    pub parameters: Option<ToolSchema>,
    pub success: Option<ToolSchema>,
    pub execute: Option<Arc<ToolExecute>>,
    pub to_model_output: Option<Arc<ToolToModelOutput>>,
    pub to_structured_output: Option<Arc<ToolToStructuredOutput>>,
}

/// `Tool.make(config)` — dynamic mode when `jsonSchema` is provided, otherwise
/// typed mode using `parameters` / `success`.
/// From reference/packages/llm/src/tool.ts (`make`)
pub fn make(config: ToolConfig) -> Tool {
    if let Some(json_schema) = config.json_schema {
        return Tool {
            description: config.description.clone(),
            parameters: ToolSchema::unknown(),
            success: ToolSchema::unknown(),
            execute: config.execute.clone(),
            to_model_output: config.to_model_output.clone(),
            to_structured_output: config.to_structured_output.clone(),
            decode: Arc::new(|value| Ok(value.clone())),
            encode: Arc::new(|value| Ok(value.clone())),
            legacy_result: config.to_model_output.is_none()
                && config.to_structured_output.is_none(),
            definition: ToolDefinition {
                name: String::new(),
                description: config.description,
                input_schema: json_schema,
                output_schema: config.output_schema,
                cache: None,
                metadata: None,
                native: None,
            },
        };
    }
    let parameters = config
        .parameters
        .clone()
        .unwrap_or_else(ToolSchema::unknown);
    let success = config.success.clone().unwrap_or_else(ToolSchema::unknown);
    let decode = parameters.decode.clone();
    let encode = success.encode.clone();
    Tool {
        description: config.description.clone(),
        parameters,
        success,
        execute: config.execute.clone(),
        to_model_output: config.to_model_output.clone(),
        to_structured_output: config.to_structured_output.clone(),
        decode,
        encode,
        legacy_result: false,
        definition: ToolDefinition {
            name: String::new(),
            description: config.description,
            input_schema: config
                .parameters
                .clone()
                .map(|p| p.json_schema)
                .unwrap_or_else(|| Value::Object(Default::default())),
            output_schema: config.success.clone().map(|s| s.json_schema),
            cache: None,
            metadata: None,
            native: None,
        },
    }
}

/// `toDefinitions(tools)` — convert a named tool record into definitions.
/// From reference/packages/llm/src/tool.ts (`toDefinitions`)
pub fn to_definitions(tools: &BTreeMap<String, Tool>) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|(name, tool)| ToolDefinition {
            name: name.clone(),
            description: tool.definition.description.clone(),
            input_schema: tool.definition.input_schema.clone(),
            output_schema: tool.definition.output_schema.clone(),
            cache: tool.definition.cache.clone(),
            metadata: tool.definition.metadata.clone(),
            native: tool.definition.native.clone(),
        })
        .collect()
}

/// `project(...)` — build a `ToolOutput` from execution.
/// From reference/packages/llm/src/tool.ts (`project`)
pub fn project(
    to_model_output: Option<&Arc<ToolToModelOutput>>,
    to_structured_output: Option<&Arc<ToolToStructuredOutput>>,
    parameters: &Value,
    call_id: &str,
    output: &Value,
) -> ToolOutput {
    let structured = match to_structured_output {
        Some(f) => f(output),
        None => output.clone(),
    };
    let content = match to_model_output {
        Some(f) => f(&ToolModelOutputInput {
            call_id,
            parameters,
            output,
        }),
        None => match output.as_str() {
            Some(text) => vec![ToolContent::Text {
                text: text.to_string(),
            }],
            None => vec![],
        },
    };
    ToolOutput {
        structured,
        content,
    }
}

/// `Tool.make` helper to construct an async execute handler from a sync one.
pub fn sync_execute(
    f: impl Fn(Value, ToolExecuteContext) -> Result<Value, crate::schema::ToolFailure>
        + Send
        + Sync
        + 'static,
) -> Arc<ToolExecute> {
    Arc::new(move |params, context| Box::pin(futures::future::ready(f(params, context))))
}

/// Wrap a `ToolCallPart`-shaped call for dispatch.
pub fn call_part(id: impl Into<String>, name: impl Into<String>, input: Value) -> ToolCallPart {
    ToolCallPart {
        part_type: "tool-call".to_string(),
        id: id.into(),
        name: name.into(),
        input,
        provider_executed: None,
        metadata: None,
        provider_metadata: None,
    }
}

#[allow(unused)]
fn _result_value_marker(_: &ToolResultValue) {}
