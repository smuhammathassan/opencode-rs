//! Stream-parser tests: recorded SSE chunks feed each protocol's state machine
//! and produce the provider-neutral event sequence.

mod common;

use oc_llm::llm::{request, RequestInput};
use oc_llm::schema::{FinishReason, LlmEvent, LlmResponse};

fn sse(chunks: &[&str]) -> String {
    chunks
        .iter()
        .map(|data| format!("data: {}\n\n", data))
        .collect::<Vec<_>>()
        .join("")
}

fn delta(content: &str, finish: &str) -> String {
    let finish_part = if finish.is_empty() {
        String::new()
    } else {
        format!(",\"finish_reason\":\"{}\"", finish)
    };
    format!(
        "{{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{}\"}}{}}}]}}",
        content, finish_part
    )
}

fn usage_chunk(prompt: i64, completion: i64, total: i64, cached: i64) -> String {
    format!(
        "{{\"choices\":[],\"usage\":{{\"prompt_tokens\":{},\"completion_tokens\":{},\"total_tokens\":{},\"prompt_tokens_details\":{{\"cached_tokens\":{}}},\"completion_tokens_details\":{{\"reasoning_tokens\":0}}}}}}",
        prompt, completion, total, cached
    )
}

fn request_with(model: oc_llm::Model) -> oc_llm::LlmRequest {
    request(RequestInput::new(model))
}

/// OpenAI Chat: text + usage streaming.
/// Mirrors the recorded `openai-chat/streams-text` cassette and
/// reference/packages/llm/test/provider/openai-chat.test.ts.
#[test]
fn openai_chat_streams_text() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::openai_chat::protocol().stream,
            request_with(model),
            &sse(&[
                &delta("", ""),
                &delta("Hello", ""),
                &delta("!", ""),
                &delta("", "stop"),
                &usage_chunk(5, 2, 7, 1),
            ]),
        ));

    let kinds: Vec<&str> = events.iter().map(LlmEvent::kind).collect();
    assert_eq!(
        kinds,
        [
            "step-start",
            "text-start",
            "text-delta",
            "text-delta",
            "text-end",
            "step-finish",
            "finish"
        ]
    );
    let response = common::complete(&events);
    assert_eq!(response.text(), "Hello!");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(5));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.non_cached_input_tokens, Some(4));
    assert_eq!(usage.cache_read_input_tokens, Some(1));
    assert_eq!(usage.reasoning_tokens, Some(0));
    assert_eq!(usage.total_tokens, Some(7));
}

/// OpenAI Chat: streamed tool-call input is accumulated and finalized.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
/// (`assembles streamed tool call input`)
#[test]
fn openai_chat_streams_tool_call() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let chunk1 = r#"{"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"query\""}}]}}]}"#;
    let chunk2 = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"weather\"}"}}]}}]}"#;
    let chunk3 = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#;
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::openai_chat::protocol().stream,
            request_with(model),
            &sse(&[chunk1, chunk2, chunk3]),
        ));

    let kinds: Vec<&str> = events.iter().map(LlmEvent::kind).collect();
    assert_eq!(
        kinds,
        [
            "step-start",
            "tool-input-start",
            "tool-input-delta",
            "tool-input-delta",
            "tool-input-end",
            "tool-call",
            "step-finish",
            "finish"
        ]
    );
    let call = events
        .iter()
        .find(|e| matches!(e, LlmEvent::ToolCall { .. }))
        .unwrap();
    match call {
        LlmEvent::ToolCall {
            id, name, input, ..
        } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "lookup");
            assert_eq!(*input, serde_json::json!({"query": "weather"}));
        }
        _ => unreachable!(),
    }
    let response = common::complete(&events);
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
}

/// OpenAI Chat: streamed tool calls without a finish reason are not finalized.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
#[test]
fn openai_chat_does_not_finalize_without_finish_reason() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let chunk1 = r#"{"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"query\""}}]}}]}"#;
    let chunk2 = r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"weather\"}"}}]}}]}"#;
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::openai_chat::protocol().stream,
            request_with(model),
            &sse(&[chunk1, chunk2]),
        ));
    assert!(!events
        .iter()
        .any(|e| matches!(e, LlmEvent::ToolCall { .. })));
    assert_eq!(
        oc_llm::schema::response_complete(
            &events
                .iter()
                .fold(oc_llm::schema::response_empty(), |state, event| {
                    oc_llm::schema::response_reduce(&state, event)
                })
        ),
        None
    );
}

/// OpenAI Chat: OpenAI-compatible reasoning deltas.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
#[test]
fn openai_chat_reasoning_deltas() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let chunk1 = r#"{"choices":[{"index":0,"delta":{"reasoning_content":"thinking"}}]}"#;
    let chunk2 = r#"{"choices":[{"index":0,"delta":{"content":"Hello"}}]}"#;
    let chunk3 = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::openai_chat::protocol().stream,
            request_with(model),
            &sse(&[chunk1, chunk2, chunk3]),
        ));
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.reasoning(), "thinking");
    assert_eq!(response.text(), "Hello");
    let kinds: Vec<&str> = events.iter().map(LlmEvent::kind).collect();
    assert_eq!(
        kinds,
        [
            "step-start",
            "reasoning-start",
            "reasoning-delta",
            "reasoning-end",
            "text-start",
            "text-delta",
            "text-end",
            "step-finish",
            "finish"
        ]
    );
}

/// Anthropic Messages: text + usage streaming.
#[test]
fn anthropic_messages_streams_text() {
    let model = common::configured(
        "claude-sonnet-4-5",
        oc_llm::protocols::anthropic_messages::route(),
    );
    let body = sse(&[
        r#"{"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":1}}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Hello"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"!"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#,
    ]);
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::anthropic_messages::protocol().stream,
            request_with(model),
            &body,
        ));
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.text(), "Hello!");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(25));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.non_cached_input_tokens, Some(25));
    assert_eq!(usage.cache_read_input_tokens, None);
    assert_eq!(usage.cache_write_input_tokens, None);
}

/// Anthropic Messages: tool-use block streaming.
#[test]
fn anthropic_messages_streams_tool_use() {
    let model = common::configured(
        "claude-sonnet-4-5",
        oc_llm::protocols::anthropic_messages::route(),
    );
    let body = sse(&[
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":":\"weather\"}"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
    ]);
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::anthropic_messages::protocol().stream,
            request_with(model),
            &body,
        ));
    let call = events
        .iter()
        .find(|e| matches!(e, LlmEvent::ToolCall { .. }))
        .unwrap();
    match call {
        LlmEvent::ToolCall {
            id, name, input, ..
        } => {
            assert_eq!(id, "toolu_1");
            assert_eq!(name, "lookup");
            assert_eq!(*input, serde_json::json!({"query": "weather"}));
        }
        _ => unreachable!(),
    }
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
}

/// Gemini: streamed text chunks.
#[test]
fn gemini_streams_text() {
    let model = common::configured("gemini-2.5-flash", oc_llm::protocols::gemini::route());
    let body = sse(&[
        r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"Hello"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"totalTokenCount":12}}"#,
        r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"!"}]},"finishReason":"STOP"}]}"#,
    ]);
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::gemini::protocol().stream,
            request_with(model),
            &body,
        ));
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.text(), "Hello!");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(2));
}

/// Gemini: function call chunk.
#[test]
fn gemini_streams_tool_call() {
    let model = common::configured("gemini-2.5-flash", oc_llm::protocols::gemini::route());
    let body = sse(&[
        r#"{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"query":"weather"}}}]},"finishReason":"STOP"}]}"#,
    ]);
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::gemini::protocol().stream,
            request_with(model),
            &body,
        ));
    let call = events
        .iter()
        .find(|e| matches!(e, LlmEvent::ToolCall { .. }))
        .unwrap();
    match call {
        LlmEvent::ToolCall {
            id, name, input, ..
        } => {
            assert_eq!(id, "tool_0");
            assert_eq!(name, "lookup");
            assert_eq!(*input, serde_json::json!({"query": "weather"}));
        }
        _ => unreachable!(),
    }
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
}

/// OpenAI Responses: output text deltas + completion.
#[test]
fn openai_responses_streams_text() {
    let model = common::configured("gpt-4o-mini", oc_llm::protocols::openai_responses::route());
    let body = sse(&[
        r#"{"type":"response.output_text.delta","item_id":"msg_1","delta":"Hello"}"#,
        r#"{"type":"response.output_text.delta","item_id":"msg_1","delta":"!"}"#,
        r#"{"type":"response.completed","response":{"id":"resp_1","usage":{"input_tokens":5,"output_tokens":2,"total_tokens":7}}}"#,
    ]);
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::openai_responses::protocol().stream,
            request_with(model),
            &body,
        ));
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.text(), "Hello!");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(5));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(7));
}

/// OpenAI Responses: function-call arguments via `output_item` lifecycle.
#[test]
fn openai_responses_streams_tool_call() {
    let model = common::configured("gpt-4o-mini", oc_llm::protocols::openai_responses::route());
    let body = sse(&[
        r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":""}}"#,
        r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{\"query\""}"#,
        r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":":\"weather\"}"}"#,
        r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"{\"query\":\"weather\"}"}}"#,
        r#"{"type":"response.completed","response":{"id":"resp_1"}}"#,
    ]);
    let events = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(common::parse_events(
            oc_llm::protocols::openai_responses::protocol().stream,
            request_with(model),
            &body,
        ));
    let call = events
        .iter()
        .find(|e| matches!(e, LlmEvent::ToolCall { .. }))
        .unwrap();
    match call {
        LlmEvent::ToolCall {
            id, name, input, ..
        } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "lookup");
            assert_eq!(*input, serde_json::json!({"query": "weather"}));
        }
        _ => unreachable!(),
    }
    let response: LlmResponse = common::complete(&events);
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
}
