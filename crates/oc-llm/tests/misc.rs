//! Tool runtime, cache policy, provider-error detection, and AWS event-stream
//! framing tests.

use std::collections::BTreeMap;

use oc_llm::schema::messages::{ToolCallPart, ToolCallPartInput};
use oc_llm::schema::{CachePolicy, CachePolicyObject, LlmEvent};
use oc_llm::tool::{make, sync_execute, to_definitions, ToolConfig};

fn json_tool(name: &str, input: serde_json::Value) -> ToolCallPart {
    ToolCallPart::make(ToolCallPartInput::new(name, name, input))
}

/// `ToolRuntime.dispatch` executes a named tool and produces canonical events.
/// From reference/packages/llm/test/tool-runtime.test.ts
#[test]
fn tool_dispatch_success() {
    let tool = make(ToolConfig {
        description: "Add two numbers".to_string(),
        json_schema: Some(serde_json::json!({
            "type": "object",
            "properties": { "a": {"type": "number"}, "b": {"type": "number"} }
        })),
        output_schema: None,
        parameters: None,
        success: None,
        execute: Some(sync_execute(|params, _ctx| {
            let a = params["a"].as_i64().unwrap_or(0);
            let b = params["b"].as_i64().unwrap_or(0);
            Ok(serde_json::json!({ "sum": a + b }))
        })),
        to_model_output: None,
        to_structured_output: None,
    });
    let mut tools = BTreeMap::new();
    tools.insert("add".to_string(), tool);

    let call = json_tool("add", serde_json::json!({ "a": 1, "b": 2 }));
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(oc_llm::tool_dispatch(&tools, &call));

    assert_eq!(
        result.result,
        oc_llm::schema::ToolResultValue::Json {
            value: serde_json::json!({ "sum": 3 })
        }
    );
    assert_eq!(result.events.len(), 1);
    assert!(matches!(&result.events[0], LlmEvent::ToolResult { name, .. } if name == "add"));
}

/// Unknown tool names produce an error result, not a panic.
#[test]
fn tool_dispatch_unknown_tool() {
    let tools = BTreeMap::new();
    let call = json_tool("nope", serde_json::json!({}));
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(oc_llm::tool_dispatch(&tools, &call));
    assert!(result.result.is_error());
    assert_eq!(result.events.len(), 2);
    assert!(matches!(&result.events[0], LlmEvent::ToolError { .. }));
}

/// `ToolFailure` from a handler surfaces as a `tool-error` event.
#[test]
fn tool_dispatch_failure() {
    let tool = make(ToolConfig {
        description: "fails".to_string(),
        json_schema: Some(serde_json::json!({ "type": "object" })),
        output_schema: None,
        parameters: None,
        success: None,
        execute: Some(sync_execute(|_params, _ctx| {
            Err(oc_llm::schema::ToolFailure::new("boom"))
        })),
        to_model_output: None,
        to_structured_output: None,
    });
    let mut tools = BTreeMap::new();
    tools.insert("fails".to_string(), tool);
    let call = json_tool("fails", serde_json::json!({}));
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(oc_llm::tool_dispatch(&tools, &call));
    assert!(result.result.is_error());
    assert!(matches!(&result.events[0], LlmEvent::ToolError { message, .. } if message == "boom"));
}

/// `toDefinitions` names tools from the record keys.
#[test]
fn tool_to_definitions_names() {
    let tool = make(ToolConfig {
        description: "lookup".to_string(),
        json_schema: Some(serde_json::json!({ "type": "object" })),
        output_schema: None,
        parameters: None,
        success: None,
        execute: None,
        to_model_output: None,
        to_structured_output: None,
    });
    let mut tools = BTreeMap::new();
    tools.insert("lookup".to_string(), tool);
    let definitions = to_definitions(&tools);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "lookup");
    assert_eq!(definitions[0].description, "lookup");
}

fn request_for_route(model: oc_llm::Model) -> oc_llm::LlmRequest {
    oc_llm::llm::request(oc_llm::llm::RequestInput::new(model))
}

/// The default `"auto"` cache policy marks the last tool/system/user-message on
/// protocols that respect inline hints.
/// From reference/packages/llm/test/provider/anthropic-messages-cache.recorded.test.ts
#[test]
fn cache_policy_auto_marks_hints() {
    let model = oc_llm::providers::anthropic::configure(oc_llm::providers::anthropic::Config {
        base_url: Some("https://api.anthropic.test/v1".to_string()),
        ..Default::default()
    })
    .model("claude-sonnet-4-5");
    let mut request = request_for_route(model);
    request.system = vec![oc_llm::schema::SystemPart::make("You are concise.")];
    request.messages = vec![oc_llm::schema::Message::user("Hello")];
    request.tools = vec![oc_llm::schema::ToolDefinition::new(
        "lookup",
        "Lookup",
        serde_json::json!({ "type": "object" }),
    )];

    let resolved = oc_llm::apply_cache_policy(&request);

    assert!(resolved.system[0].cache.is_some(), "system hint injected");
    assert!(resolved.tools[0].cache.is_some(), "tool hint injected");
    let user_text = &resolved.messages[0].content[0];
    match user_text {
        oc_llm::schema::ContentPart::Text { cache, .. } => {
            assert!(cache.is_some(), "user message hint injected")
        }
        _ => unreachable!(),
    }
}

/// `"none"` disables auto-placement; explicit `CacheHint`s still flow.
#[test]
fn cache_policy_none_skips() {
    let model = oc_llm::providers::anthropic::configure(oc_llm::providers::anthropic::Config {
        base_url: Some("https://api.anthropic.test/v1".to_string()),
        ..Default::default()
    })
    .model("claude-sonnet-4-5");
    let mut request = request_for_route(model);
    request.cache = Some(CachePolicy::None);
    request.system = vec![oc_llm::schema::SystemPart::make("You are concise.")];
    let resolved = oc_llm::apply_cache_policy(&request);
    assert!(resolved.system[0].cache.is_none());
}

/// OpenAI protocols skip the inline-hint pass entirely.
#[test]
fn cache_policy_skips_openai() {
    let model = oc_llm::providers::openai::configure(oc_llm::providers::openai::Config {
        base_url: Some("https://api.openai.test/v1".to_string()),
        api_key: Some("test".to_string()),
        ..Default::default()
    })
    .model("gpt-4o-mini");
    let mut request = request_for_route(model);
    request.system = vec![oc_llm::schema::SystemPart::make("You are concise.")];
    let resolved = oc_llm::apply_cache_policy(&request);
    assert!(resolved.system[0].cache.is_none());
}

/// Granular object policy with a TTL bucket.
#[test]
fn cache_policy_object_ttl() {
    let model = oc_llm::providers::anthropic::configure(oc_llm::providers::anthropic::Config {
        base_url: Some("https://api.anthropic.test/v1".to_string()),
        ..Default::default()
    })
    .model("claude-sonnet-4-5");
    let mut request = request_for_route(model);
    request.cache = Some(CachePolicy::Object(CachePolicyObject {
        tools: Some(true),
        system: Some(false),
        messages: Some(oc_llm::schema::CachePolicyMessages::LatestUserMessage),
        ttl_seconds: Some(7200),
    }));
    request.messages = vec![oc_llm::schema::Message::user("Hello")];
    request.tools = vec![oc_llm::schema::ToolDefinition::new(
        "lookup",
        "Lookup",
        serde_json::json!({ "type": "object" }),
    )];
    let resolved = oc_llm::apply_cache_policy(&request);
    assert!(resolved.tools[0].cache.as_ref().unwrap().ttl_seconds == Some(7200));
}

/// Context-overflow detection matches provider error strings.
#[test]
fn is_context_overflow_detection() {
    assert!(oc_llm::is_context_overflow(
        "This model's maximum context length is 200000 tokens"
    ));
    assert!(oc_llm::is_context_overflow("prompt is too long"));
    assert!(oc_llm::is_context_overflow("context_length_exceeded"));
    assert!(!oc_llm::is_context_overflow("rate limit exceeded"));
    assert!(!oc_llm::is_context_overflow("too many requests"));
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn aws_frame(event_type: &str, payload: &str) -> Vec<u8> {
    let mut headers = Vec::new();
    for (name, value) in [(":message-type", "event"), (":event-type", event_type)] {
        headers.push(name.len() as u8);
        headers.extend_from_slice(name.as_bytes());
        headers.push(7); // string value type
        headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
        headers.extend_from_slice(value.as_bytes());
    }
    let headers_len = headers.len() as u32;
    let total_len = 12 + headers_len as u32 + payload.len() as u32 + 4;
    let mut frame = Vec::new();
    frame.extend_from_slice(&total_len.to_be_bytes());
    frame.extend_from_slice(&headers_len.to_be_bytes());
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame.extend_from_slice(&headers);
    frame.extend_from_slice(payload.as_bytes());
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame
}

/// AWS event-stream framing decodes a Bedrock `messageStart` payload.
/// From reference/packages/llm/test/provider/bedrock-converse.test.ts
#[test]
fn aws_event_stream_framing_decodes() {
    use futures::stream::StreamExt;
    let payload = r#"{"role":"assistant"}"#;
    let frame = aws_frame("messageStart", payload);
    let bytes_stream = futures::stream::iter(vec![Ok(bytes::Bytes::from(frame))]);
    let mut stream = oc_llm::route::transport::AwsEventStream::new(Box::pin(bytes_stream));
    let decoded = stream.next();
    let item = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(decoded)
        .unwrap()
        .unwrap();
    match item {
        oc_llm::route::protocol::FramePayload::Aws(value) => {
            assert_eq!(
                value,
                serde_json::json!({ "messageStart": { "role": "assistant" } })
            );
        }
        _ => unreachable!(),
    }
}
