//! Streaming tool-call accumulator.
//! From reference/packages/llm/src/protocols/utils/tool-stream.ts

use std::collections::BTreeMap;

use crate::schema::{LlmError, LlmEvent, ProviderMetadata};
use crate::shared::{event_error, parse_tool_input};

/// `PendingTool`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts
#[derive(Debug, Clone)]
pub struct PendingTool {
    pub id: String,
    pub name: String,
    pub input: String,
    pub provider_executed: Option<bool>,
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Sparse parser state keyed by the provider's stream-local tool identifier.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`State`)
pub type State<K> = BTreeMap<K, PendingTool>;

/// `ToolStream.empty()`.
pub fn empty<K>() -> State<K>
where
    K: Ord,
{
    BTreeMap::new()
}

/// Namespace-style access mirroring the reference's `ToolStream.*` API.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts
pub struct ToolStream;

impl ToolStream {
    pub fn empty<K>() -> State<K>
    where
        K: Ord,
    {
        empty()
    }

    pub fn start<K: Ord + Clone>(tools: &State<K>, key: K, tool: PendingToolInput) -> State<K> {
        start(tools, key, tool)
    }

    pub fn append_or_start<K: Ord + Clone>(
        route: &str,
        tools: &State<K>,
        key: K,
        delta: ToolDelta,
        missing_tool_message: &str,
    ) -> Result<AppendOutcome<K>, LlmError> {
        append_or_start(route, tools, key, delta, missing_tool_message)
    }

    pub fn append_existing<K: Ord + Clone>(
        route: &str,
        tools: &State<K>,
        key: &K,
        text: &str,
        missing_tool_message: &str,
    ) -> Result<AppendOutcome<K>, LlmError> {
        append_existing(route, tools, key, text, missing_tool_message)
    }

    pub fn finish<K: Ord + Clone>(route: &str, tools: &State<K>, key: &K) -> Result<FinishOutcome<K>, LlmError> {
        finish(route, tools, key)
    }

    pub fn finish_with_input<K: Ord + Clone>(
        route: &str,
        tools: &State<K>,
        key: &K,
        input: &str,
    ) -> Result<FinishOutcome<K>, LlmError> {
        finish_with_input(route, tools, key, input)
    }

    pub fn finish_all<K: Ord + Clone>(route: &str, tools: &State<K>) -> Result<FinishOutcome<K>, LlmError> {
        finish_all(route, tools)
    }

    pub fn is_error<K: Ord + Clone>(_result: &Result<AppendOutcome<K>, LlmError>) -> bool {
        _result.is_err()
    }
}

/// `AppendOutcome`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`AppendOutcome`)
#[derive(Debug, Clone)]
pub struct AppendOutcome<K> {
    pub tools: State<K>,
    pub tool: PendingTool,
    pub events: Vec<LlmEvent>,
}

fn input_start(tool: &PendingTool) -> LlmEvent {
    LlmEvent::ToolInputStart {
        id: tool.id.clone(),
        name: tool.name.clone(),
        provider_metadata: tool.provider_metadata.clone(),
    }
}

fn input_delta(tool: &PendingTool, text: &str) -> LlmEvent {
    LlmEvent::ToolInputDelta { id: tool.id.clone(), name: tool.name.clone(), text: text.to_string() }
}

fn tool_call(route: &str, tool: &PendingTool, input_override: Option<&str>) -> Result<LlmEvent, LlmError> {
    let input = parse_tool_input(route, &tool.name, input_override.unwrap_or(&tool.input))?;
    Ok(LlmEvent::ToolCall {
        id: tool.id.clone(),
        name: tool.name.clone(),
        input,
        provider_executed: if tool.provider_executed == Some(true) { Some(true) } else { None },
        provider_metadata: tool.provider_metadata.clone(),
    })
}

fn append_tool<K: Ord + Clone>(tools: &State<K>, key: &K, tool: &PendingTool, text: &str) -> AppendOutcome<K> {
    let mut events = Vec::new();
    if !tools.contains_key(key) {
        events.push(input_start(tool));
    }
    if !text.is_empty() {
        events.push(input_delta(tool, text));
    }
    let mut next = tools.clone();
    next.insert(key.clone(), tool.clone());
    AppendOutcome { tools: next, tool: tool.clone(), events }
}

/// `ToolStream.start(tools, key, tool)`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`start`)
pub fn start<K: Ord + Clone>(
    tools: &State<K>,
    key: K,
    tool: PendingToolInput,
) -> State<K> {
    let mut next = tools.clone();
    let mut input = tool.input.unwrap_or_default();
    if input.is_empty() {
        input = String::new();
    }
    next.insert(
        key,
        PendingTool {
            id: tool.id,
            name: tool.name,
            input,
            provider_executed: tool.provider_executed,
            provider_metadata: tool.provider_metadata,
        },
    );
    next
}

pub struct PendingToolInput {
    pub id: String,
    pub name: String,
    pub input: Option<String>,
    pub provider_executed: Option<bool>,
    pub provider_metadata: Option<ProviderMetadata>,
}

/// `ToolStream.appendOrStart(...)`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`appendOrStart`)
pub fn append_or_start<K: Ord + Clone>(
    route: &str,
    tools: &State<K>,
    key: K,
    delta: ToolDelta,
    missing_tool_message: &str,
) -> Result<AppendOutcome<K>, LlmError> {
    let current = tools.get(&key);
    let id = delta.id.clone().or_else(|| current.map(|c| c.id.clone()));
    let name = delta.name.clone().or_else(|| current.map(|c| c.name.clone()));
    let (Some(id), Some(name)) = (id, name) else {
        return Err(event_error(route, missing_tool_message, None));
    };
    let input = format!("{}{}", current.map(|c| c.input.as_str()).unwrap_or(""), delta.text);
    let tool = PendingTool {
        id,
        name,
        input,
        provider_executed: current.and_then(|c| c.provider_executed),
        provider_metadata: current.and_then(|c| c.provider_metadata.clone()),
    };
    if let Some(current) = current {
        if delta.text.is_empty() && current.id == tool.id && current.name == tool.name {
            return Ok(AppendOutcome { tools: tools.clone(), tool: current.clone(), events: vec![] });
        }
    }
    Ok(append_tool(tools, &key, &tool, &delta.text))
}

/// `ToolDelta` — identity + argument delta for one streamed tool call.
#[derive(Debug, Clone, Default)]
pub struct ToolDelta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub text: String,
}

/// `ToolStream.appendExisting(...)`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`appendExisting`)
pub fn append_existing<K: Ord + Clone>(
    route: &str,
    tools: &State<K>,
    key: &K,
    text: &str,
    missing_tool_message: &str,
) -> Result<AppendOutcome<K>, LlmError> {
    let Some(current) = tools.get(key) else {
        return Err(event_error(route, missing_tool_message, None));
    };
    if text.is_empty() {
        return Ok(AppendOutcome { tools: tools.clone(), tool: current.clone(), events: vec![] });
    }
    let mut next_tool = current.clone();
    next_tool.input.push_str(text);
    Ok(append_tool(tools, key, &next_tool, text))
}

/// `ToolStream.finish(route, tools, key)`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`finish`)
pub fn finish<K: Ord + Clone>(route: &str, tools: &State<K>, key: &K) -> Result<FinishOutcome<K>, LlmError> {
    let Some(tool) = tools.get(key) else {
        return Ok(FinishOutcome { tools: tools.clone(), events: vec![] });
    };
    let mut next = tools.clone();
    next.remove(key);
    let mut events = vec![LlmEvent::ToolInputEnd {
        id: tool.id.clone(),
        name: tool.name.clone(),
        provider_metadata: tool.provider_metadata.clone(),
    }];
    events.push(tool_call(route, tool, None)?);
    Ok(FinishOutcome { tools: next, events })
}

/// `ToolStream.finishWithInput(...)`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`finishWithInput`)
pub fn finish_with_input<K: Ord + Clone>(
    route: &str,
    tools: &State<K>,
    key: &K,
    input: &str,
) -> Result<FinishOutcome<K>, LlmError> {
    let Some(tool) = tools.get(key) else {
        return Ok(FinishOutcome { tools: tools.clone(), events: vec![] });
    };
    let mut next = tools.clone();
    next.remove(key);
    let mut events = vec![LlmEvent::ToolInputEnd {
        id: tool.id.clone(),
        name: tool.name.clone(),
        provider_metadata: tool.provider_metadata.clone(),
    }];
    events.push(tool_call(route, tool, Some(input))?);
    Ok(FinishOutcome { tools: next, events })
}

/// `ToolStream.finishAll(route, tools)`.
/// From reference/packages/llm/src/protocols/utils/tool-stream.ts (`finishAll`)
pub fn finish_all<K: Ord + Clone>(route: &str, tools: &State<K>) -> Result<FinishOutcome<K>, LlmError> {
    let pending: Vec<&PendingTool> = tools.values().collect();
    let mut events = Vec::new();
    for tool in pending {
        events.push(LlmEvent::ToolInputEnd {
            id: tool.id.clone(),
            name: tool.name.clone(),
            provider_metadata: tool.provider_metadata.clone(),
        });
        events.push(tool_call(route, tool, None)?);
    }
    Ok(FinishOutcome { tools: empty(), events })
}

/// Result of finalizing one or more pending tool calls.
pub struct FinishOutcome<K> {
    pub tools: State<K>,
    pub events: Vec<LlmEvent>,
}

#[allow(unused)]
fn _marker(_: &LlmError) {}
