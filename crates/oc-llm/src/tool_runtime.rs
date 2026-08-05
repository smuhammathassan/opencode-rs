//! Execute one canonical tool call without owning provider IO or continuation.
//! From reference/packages/llm/src/tool-runtime.ts

use std::collections::BTreeMap;

use serde_json::Value;

use crate::schema::messages::{ToolCallPart, ToolContent, ToolOutput, ToolResultValue};
use crate::schema::{LlmEvent, ToolFailure};
use crate::tool::{project, Tool};

/// `ToolSettlement`.
/// From reference/packages/llm/src/tool-runtime.ts
#[derive(Debug, Clone)]
pub struct ToolSettlement {
    pub result: ToolResultValue,
    pub output: Option<ToolOutput>,
}

/// `DispatchResult`.
/// From reference/packages/llm/src/tool-runtime.ts
#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub result: ToolResultValue,
    pub output: Option<ToolOutput>,
    pub events: Vec<LlmEvent>,
}

/// `ToolRuntime.dispatch(tools, call)`.
/// From reference/packages/llm/src/tool-runtime.ts (`dispatch`)
pub async fn dispatch(tools: &BTreeMap<String, Tool>, call: &ToolCallPart) -> DispatchResult {
    let Some(tool) = tools.get(&call.name) else {
        return result(
            call,
            &ToolResultValue::Error {
                value: Value::String(format!("Unknown tool: {}", call.name)),
            },
            None,
        );
    };
    if tool.execute.is_none() {
        return result(
            call,
            &ToolResultValue::Error {
                value: Value::String(format!("Tool has no execute handler: {}", call.name)),
            },
            None,
        );
    }

    match decode_and_execute(tool, call).await {
        Ok(settlement) => result(call, &settlement.result, settlement.output),
        Err(failure) => {
            let value = ToolResultValue::Error {
                value: Value::String(failure.message.clone()),
            };
            result_with_error(call, &value, Some(&failure))
        }
    }
}

async fn decode_and_execute(
    tool: &Tool,
    call: &ToolCallPart,
) -> Result<ToolSettlement, ToolFailure> {
    let decoded = (tool.decode)(&call.input)
        .map_err(|error| ToolFailure::new(format!("Invalid tool input: {}", error)))?;
    let execute = tool.execute.as_ref().unwrap();
    let value = execute(
        decoded.clone(),
        crate::tool::ToolExecuteContext {
            id: call.id.clone(),
            name: call.name.clone(),
        },
    )
    .await
    .map_err(|failure| failure)?;
    let encoded = (tool.encode)(&value).map_err(|error| {
        ToolFailure::new(format!(
            "Tool returned an invalid value for its success schema: {}",
            error
        ))
    })?;

    if tool.legacy_result && ToolResultValue::is(&encoded) {
        let result =
            serde_json::from_value(encoded).unwrap_or(ToolResultValue::Json { value: Value::Null });
        let output = ToolOutput::from_result_value(&result);
        return Ok(ToolSettlement { result, output });
    }
    let output = project(
        tool.to_model_output.as_ref(),
        tool.to_structured_output.as_ref(),
        &decoded,
        &call.id,
        &encoded,
    );
    let result = output.to_result_value();
    if result.is_error() {
        Ok(ToolSettlement {
            result,
            output: None,
        })
    } else {
        Ok(ToolSettlement {
            result,
            output: Some(output),
        })
    }
}

fn result(
    call: &ToolCallPart,
    value: &ToolResultValue,
    output: Option<ToolOutput>,
) -> DispatchResult {
    let events = if value.is_error() {
        vec![
            LlmEvent::ToolError {
                id: call.id.clone(),
                name: call.name.clone(),
                message: match &value {
                    ToolResultValue::Error { value } => match value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                    _ => String::new(),
                },
                provider_metadata: None,
            },
            LlmEvent::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                result: value.clone(),
                output: None,
                provider_executed: None,
                provider_metadata: None,
            },
        ]
    } else {
        vec![LlmEvent::ToolResult {
            id: call.id.clone(),
            name: call.name.clone(),
            result: value.clone(),
            output,
            provider_executed: None,
            provider_metadata: None,
        }]
    };
    DispatchResult {
        result: value.clone(),
        output: events_tool_output(&events),
        events,
    }
}

fn events_tool_output(events: &[LlmEvent]) -> Option<ToolOutput> {
    events.iter().find_map(|event| match event {
        LlmEvent::ToolResult { output, .. } => output.clone(),
        _ => None,
    })
}

fn result_with_error(
    call: &ToolCallPart,
    value: &ToolResultValue,
    failure: Option<&ToolFailure>,
) -> DispatchResult {
    let mut dispatch = result(call, value, None);
    if let Some(failure) = failure {
        if let Some(LlmEvent::ToolError { message, .. }) = dispatch.events.first_mut() {
            *message = failure.message.clone();
        }
    }
    dispatch
}

#[allow(unused)]
fn _marker(_: &ToolContent) {}
