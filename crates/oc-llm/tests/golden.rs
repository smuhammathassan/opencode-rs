//! Golden tests: exact request-body serialization for each wire protocol.
//! Bodies are compared as JSON strings against expectations derived from the
//! reference cassettes under `reference/packages/llm/test/fixtures/recordings`.

mod common;

use oc_llm::llm::{request, RequestInput};
use oc_llm::route::LlmClient;
use oc_llm::schema::messages::{Message, ToolCallPart};

fn client() -> LlmClient {
    LlmClient::new()
}

fn base(model: oc_llm::Model) -> RequestInput {
    RequestInput::new(model)
}

/// OpenAI Chat: body matches the recorded `openai-chat/streams-text` request.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
/// (`prepares OpenAI Chat payload`)
#[test]
fn openai_chat_body() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let mut input = base(model);
    input.system = Some("You are concise.".to_string().into());
    input.prompt = Some("Say hello.".into());
    input.generation = Some(oc_llm::schema::GenerationOptions {
        max_tokens: Some(20),
        temperature: Some(0.0),
        ..Default::default()
    });
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let body = serde_json::to_string(&prepared.body).unwrap();
    assert_eq!(
        body,
        r#"{"model":"gpt-4o-mini","messages":[{"role":"system","content":"You are concise."},{"role":"user","content":"Say hello."}],"stream":true,"stream_options":{"include_usage":true},"max_tokens":20,"temperature":0}"#
    );
}

/// OpenAI Chat: chronological system updates lower to escaped user wrappers.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
#[test]
fn openai_chat_system_update_wrapper() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let mut input = base(model);
    input.messages = Some(vec![
        Message::user("Before."),
        Message::system("Treat <admin> & data literally."),
        Message::assistant("After."),
    ]);
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let messages = &prepared.body["messages"];
    let messages = serde_json::to_string(messages).unwrap();
    assert_eq!(
        messages,
        r#"[{"role":"user","content":"Before.\n<system-update>\nTreat &lt;admin&gt; &amp; data literally.\n</system-update>"},{"role":"assistant","content":"After."}]"#
    );
}

/// OpenAI Chat: assistant tool-call + tool-result messages.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
#[test]
fn openai_chat_tool_messages() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let mut input = base(model);
    input.messages = Some(vec![
        Message::user("What is the weather?"),
        Message::assistant(vec![oc_llm::schema::ContentPart::from_tool_call(
            ToolCallPart::make(oc_llm::schema::messages::ToolCallPartInput::new(
                "call_1",
                "lookup",
                serde_json::json!({"query": "weather"}),
            )),
        )]),
        Message::tool(oc_llm::schema::messages::ToolResultPart::make(
            oc_llm::schema::messages::ToolResultPartInput {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                result: serde_json::json!({"forecast": "sunny"}),
                result_type: None,
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            },
        )),
    ]);
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let body = serde_json::to_string(&prepared.body).unwrap();
    assert_eq!(
        body,
        r#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"What is the weather?"},{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"query\":\"weather\"}"}}]},{"role":"tool","tool_call_id":"call_1","content":"{\"forecast\":\"sunny\"}"}],"stream":true,"stream_options":{"include_usage":true}}"#
    );
}

/// OpenAI Chat: reasoning-only assistant history replays as reasoning_content.
/// From reference/packages/llm/test/provider/openai-chat.test.ts
#[test]
fn openai_chat_reasoning_content() {
    let model = common::openai_chat_model("gpt-4o-mini");
    let mut input = base(model);
    input.messages = Some(vec![Message::assistant(vec![
        oc_llm::schema::ContentPart::Reasoning {
            text: "hidden".to_string(),
            encrypted: None,
            metadata: None,
            provider_metadata: None,
        },
    ])]);
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let messages = &prepared.body["messages"];
    assert_eq!(
        serde_json::to_string(messages).unwrap(),
        r#"[{"role":"assistant","content":null,"reasoning_content":"hidden"}]"#
    );
}

/// OpenAI Responses: body shape with tools and tool choice.
#[test]
fn openai_responses_body() {
    let model = common::configured("gpt-4o-mini", oc_llm::protocols::openai_responses::route());
    let mut input = base(model);
    input.system = Some("You are concise.".to_string().into());
    input.prompt = Some("Say hello.".into());
    input.tools = Some(vec![oc_llm::schema::ToolDefinition::new(
        "lookup",
        "Lookup data",
        serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    )]);
    input.tool_choice = Some(oc_llm::schema::ToolChoiceInput::String(
        "lookup".to_string(),
    ));
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let body = serde_json::to_string(&prepared.body).unwrap();
    assert_eq!(
        body,
        r#"{"model":"gpt-4o-mini","input":[{"role":"system","content":"You are concise."},{"role":"user","content":[{"type":"input_text","text":"Say hello."}]}],"tools":[{"type":"function","name":"lookup","description":"Lookup data","parameters":{"type":"object","properties":{"query":{"type":"string"}}},"strict":false}],"tool_choice":{"type":"function","name":"lookup"},"stream":true,"store":false}"#
    );
}

/// Anthropic Messages: body shape with system, tools, and generation.
#[test]
fn anthropic_messages_body() {
    let model = common::configured(
        "claude-sonnet-4-5",
        oc_llm::protocols::anthropic_messages::route(),
    );
    let mut input = base(model);
    input.system = Some("You are concise.".to_string().into());
    input.prompt = Some("Say hello.".into());
    input.generation = Some(oc_llm::schema::GenerationOptions {
        max_tokens: Some(512),
        ..Default::default()
    });
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let body = serde_json::to_string(&prepared.body).unwrap();
    assert_eq!(
        body,
        r#"{"model":"claude-sonnet-4-5","system":[{"type":"text","text":"You are concise.","cache_control":{"type":"ephemeral"}}],"messages":[{"role":"user","content":[{"type":"text","text":"Say hello.","cache_control":{"type":"ephemeral"}}]}],"stream":true,"max_tokens":512}"#
    );
}

/// Gemini: body shape with systemInstruction and generationConfig.
#[test]
fn gemini_body() {
    let model = common::configured("gemini-2.5-flash", oc_llm::protocols::gemini::route());
    let mut input = base(model);
    input.system = Some("You are concise.".to_string().into());
    input.prompt = Some("Say hello.".into());
    input.generation = Some(oc_llm::schema::GenerationOptions {
        max_tokens: Some(128),
        temperature: Some(0.7),
        top_p: Some(1.0),
        ..Default::default()
    });
    let request = request(input);
    let prepared = client().prepare(&request).unwrap();
    let body = serde_json::to_string(&prepared.body).unwrap();
    assert_eq!(
        body,
        r#"{"contents":[{"role":"user","parts":[{"text":"Say hello."}]}],"systemInstruction":{"parts":[{"text":"You are concise."}]},"generationConfig":{"maxOutputTokens":128,"temperature":0.7,"topP":1}}"#
    );
}

/// Routes without a canonical base URL require configuration before
/// `route.model(...)`.
#[test]
fn route_model_requires_base_url() {
    let route = oc_llm::protocols::openai_compatible_chat::route();
    let err = route
        .model(oc_llm::RouteModelInput {
            id: "gpt-4o-mini".to_string(),
            provider: Some("test".to_string()),
            defaults: None,
            compatibility: None,
        })
        .unwrap_err();
    assert!(
        err.contains("baseURL"),
        "expected baseURL error, got: {}",
        err
    );
}
