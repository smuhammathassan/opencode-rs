//! Request-shaped convenience API.
//! From reference/packages/llm/src/llm.ts

use serde_json::Value;
use std::collections::BTreeMap;

use crate::route::LlmClient;
use crate::schema::messages::{
    ContentInput, ContentPart, LlmRequest, LlmRequestInput, LlmRequestPatch, Message, MessageInput,
    ResponseFormat, SystemPart, SystemPartRef, ToolChoice, ToolChoiceInput, ToolDefinition,
    ToolResultPart,
};
use crate::schema::options::{GenerationOptions, HttpOptions, Model, ProviderOptions};
use crate::schema::{LlmError, LlmErrorReason, LlmResponse, MessageRole};
use crate::tool::{make as make_tool, to_definitions, Tool, ToolConfig};

pub const GENERATE_OBJECT_TOOL_NAME: &str = "generate_object";
pub const GENERATE_OBJECT_TOOL_DESCRIPTION: &str =
    "Return the structured result by calling this tool.";

/// `RequestInput` — the `LLM.request` input shape.
/// From reference/packages/llm/src/llm.ts (`RequestInput`)
#[derive(Debug, Clone)]
pub struct RequestInput {
    pub id: Option<String>,
    pub model: Model,
    pub system: Option<SystemPartRef>,
    pub prompt: Option<ContentInput>,
    pub messages: Option<Vec<Message>>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub tool_choice: Option<ToolChoiceInput>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
    pub response_format: Option<ResponseFormat>,
    pub cache: Option<crate::schema::CachePolicy>,
    pub metadata: Option<serde_json::Map<String, Value>>,
}

impl RequestInput {
    pub fn new(model: Model) -> RequestInput {
        RequestInput {
            id: None,
            model,
            system: None,
            prompt: None,
            messages: None,
            tools: None,
            tool_choice: None,
            generation: None,
            provider_options: None,
            http: None,
            response_format: None,
            cache: None,
            metadata: None,
        }
    }
}

/// `LLM.request(input)`.
/// From reference/packages/llm/src/llm.ts (`request`)
pub fn request(input: RequestInput) -> LlmRequest {
    let mut messages: Vec<Message> = Vec::new();
    if let Some(messages_input) = &input.messages {
        messages.extend(messages_input.iter().cloned());
    }
    if let Some(prompt) = &input.prompt {
        messages.push(Message::user(prompt.clone()));
    }
    LlmRequest::new(LlmRequestInput {
        id: input.id,
        model: input.model,
        system: SystemPart::content(input.system.as_ref()),
        messages,
        tools: input.tools.unwrap_or_default(),
        tool_choice: input.tool_choice.map(ToolChoice::make),
        generation: input.generation,
        provider_options: input.provider_options,
        http: input.http,
        response_format: input.response_format,
        cache: input.cache,
        metadata: input.metadata,
    })
}

/// `LLM.requestInput(request)`.
/// From reference/packages/llm/src/llm.ts (`requestInput`)
pub fn request_input(request: &LlmRequest) -> RequestInput {
    let mut input = RequestInput::new(request.model.clone());
    input.id = request.id.clone();
    input.system = Some(SystemPartRef::Many(request.system.clone()));
    input.messages = Some(request.messages.clone());
    input.tools = Some(request.tools.clone());
    input.tool_choice = request.tool_choice.clone().map(ToolChoiceInput::Choice);
    input.generation = request.generation.clone();
    input.provider_options = request.provider_options.clone();
    input.http = request.http.clone();
    input.response_format = request.response_format.clone();
    input.cache = request.cache.clone();
    input.metadata = request.metadata.clone();
    input
}

/// `LLM.updateRequest(request, patch)`.
/// From reference/packages/llm/src/llm.ts (`updateRequest`)
pub fn update_request(input: &LlmRequest, patch: RequestInput) -> LlmRequest {
    let mut base = request_input(input);
    if let Some(id) = patch.id {
        base.id = Some(id);
    }
    if let Some(system) = patch.system {
        base.system = Some(system);
    }
    if let Some(prompt) = patch.prompt {
        base.prompt = Some(prompt);
    }
    if let Some(messages) = patch.messages {
        base.messages = Some(messages);
    }
    if let Some(tools) = patch.tools {
        base.tools = Some(tools);
    }
    if let Some(tool_choice) = patch.tool_choice {
        base.tool_choice = Some(tool_choice);
    }
    if let Some(generation) = patch.generation {
        base.generation = Some(generation);
    }
    if let Some(provider_options) = patch.provider_options {
        base.provider_options = Some(provider_options);
    }
    if let Some(http) = patch.http {
        base.http = Some(http);
    }
    if let Some(response_format) = patch.response_format {
        base.response_format = Some(response_format);
    }
    if let Some(cache) = patch.cache {
        base.cache = Some(cache);
    }
    if let Some(metadata) = patch.metadata {
        base.metadata = Some(metadata);
    }
    request(base)
}

/// `GenerateObjectResponse<T>`.
/// From reference/packages/llm/src/llm.ts (`GenerateObjectResponse`)
#[derive(Debug, Clone)]
pub struct GenerateObjectResponse {
    pub object: Value,
    pub response: LlmResponse,
}

impl GenerateObjectResponse {
    pub fn events(&self) -> &[crate::schema::LlmEvent] {
        &self.response.events
    }

    pub fn usage(&self) -> Option<&crate::schema::Usage> {
        self.response.usage.as_ref()
    }
}

/// `GenerateObjectOptions` — run a model and decode its output against a schema
/// by forcing a synthetic `generate_object` tool call.
/// From reference/packages/llm/src/llm.ts (`generateObject`)
pub struct GenerateObjectOptions {
    pub base: RequestInput,
    /// Tool used for the synthetic call. For typed mode the tool carries its
    /// own decode; for dynamic mode `json_schema` is used.
    pub tool: Tool,
}

/// `LLM.generateObject(client, options)`.
/// From reference/packages/llm/src/llm.ts (`generateObject`)
pub async fn generate_object(
    client: &LlmClient,
    options: GenerateObjectOptions,
) -> Result<GenerateObjectResponse, LlmError> {
    let base_request = request(options.base);
    let mut tools = BTreeMap::new();
    tools.insert(GENERATE_OBJECT_TOOL_NAME.to_string(), options.tool.clone());
    let mut patch = LlmRequestPatch::empty();
    patch.tools = Some(to_definitions(&tools));
    patch.tool_choice = Some(Some(ToolChoice::named(GENERATE_OBJECT_TOOL_NAME)));
    let generate_request = LlmRequest::update(&base_request, patch);

    let response = client.generate(generate_request).await?;
    let call = response
        .events
        .iter()
        .find(|event| {
            matches!(event, crate::schema::LlmEvent::ToolCall { name, .. } if name == GENERATE_OBJECT_TOOL_NAME)
        })
        .cloned();

    let Some(call) = call else {
        return Err(LlmError::new(
            "LLM",
            "generateObject",
            LlmErrorReason::InvalidProviderOutput {
                message: format!(
                    "generateObject: model did not call the forced `{}` tool",
                    GENERATE_OBJECT_TOOL_NAME
                ),
                route: None,
                raw: None,
                provider_metadata: None,
            },
        ));
    };
    let crate::schema::LlmEvent::ToolCall { input, .. } = call else {
        unreachable!()
    };
    let object = (options.tool.decode)(&input).map_err(|error| {
        LlmError::new(
            "LLM",
            "generateObject",
            LlmErrorReason::InvalidProviderOutput {
                message: format!("generateObject: tool input failed schema decode: {}", error),
                route: None,
                raw: None,
                provider_metadata: None,
            },
        )
    })?;
    Ok(GenerateObjectResponse { object, response })
}

/// `LLM.generateObject` dynamic mode — schema available only at runtime.
/// From reference/packages/llm/src/llm.ts (`GenerateObjectDynamicOptions`)
pub fn generate_object_dynamic(base: RequestInput, json_schema: Value) -> GenerateObjectOptions {
    GenerateObjectOptions {
        base,
        tool: make_tool(ToolConfig {
            description: GENERATE_OBJECT_TOOL_DESCRIPTION.to_string(),
            json_schema: Some(json_schema),
            output_schema: None,
            parameters: None,
            success: None,
            execute: None,
            to_model_output: None,
            to_structured_output: None,
        }),
    }
}

#[allow(unused)]
fn _markers(
    _: &MessageInput,
    _: &MessageRole,
    _: &ContentPart,
    _: &ToolResultPart,
    _: &HttpOptions,
) {
}
