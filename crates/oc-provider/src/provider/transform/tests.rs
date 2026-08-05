//! Golden tests for ProviderTransform, ported from
//! reference/packages/opencode/test/provider/transform.test.ts.

use serde_json::{json, Map, Value};

use crate::provider::transform::*;
use crate::provider::Model;

fn model(value: Value) -> Model {
    serde_json::from_value(value).unwrap()
}

fn messages(value: Value) -> Vec<ModelMessage> {
    serde_json::from_value(value).unwrap()
}

/// Shallow-merges a patch object over a base object (JSON spread in the
/// reference tests).
fn extend(base: Value, patch: Value) -> Value {
    let mut map = base.as_object().unwrap().clone();
    for (key, value) in patch.as_object().unwrap() {
        map.insert(key.clone(), value.clone());
    }
    Value::Object(map)
}

fn run_options(model_json: Value, provider_options: Option<Value>) -> Value {
    let m = model(model_json);
    let po = provider_options.map(|v| v.as_object().unwrap().clone());
    Value::Object(options(&m, "test-session-123", po.as_ref()))
}

fn mock_model_object() -> Value {
    json!({
        "id": "anthropic/claude-3-5-sonnet",
        "providerID": "anthropic",
        "api": { "id": "claude-3-5-sonnet-20241022", "url": "https://api.anthropic.com", "npm": "@ai-sdk/anthropic" },
        "name": "Claude 3.5 Sonnet",
        "capabilities": {
            "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
            "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": true },
            "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
            "interleaved": false,
        },
        "cost": { "input": 0.003, "output": 0.015, "cache": { "read": 0.0003, "write": 0.00375 } },
        "limit": { "context": 200000, "output": 8192 },
        "status": "active", "options": {}, "headers": {},
    })
}

fn options_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

mod set_cache_key {
    use super::*;

    fn run(model_json: Value, provider_options: Option<Value>) -> Value {
        run_options(model_json, provider_options)
    }

    #[test]
    fn prompt_cache_key_set_when_explicitly_true() {
        let result = run(mock_model_object(), Some(json!({ "setCacheKey": true })));
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn prompt_cache_key_not_set_when_explicitly_false() {
        let result = run(mock_model_object(), Some(json!({ "setCacheKey": false })));
        assert!(result.get("promptCacheKey").is_none());
    }

    #[test]
    fn prompt_cache_key_not_set_without_provider_options() {
        let result = run(mock_model_object(), None);
        assert!(result.get("promptCacheKey").is_none());
    }

    #[test]
    fn prompt_cache_key_for_openai_by_default() {
        let openai = extend(
            mock_model_object(),
            json!({
                "providerID": "openai",
                "api": { "id": "gpt-4", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            }),
        );
        let result = run(openai, Some(json!({})));
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn prompt_cache_key_for_openai_sdk_regardless_of_provider_id() {
        let custom = extend(
            mock_model_object(),
            json!({
                "providerID": "custom-openai",
                "api": { "id": "gpt-5", "url": "https://example.com", "npm": "@ai-sdk/openai" },
            }),
        );
        let result = run(custom, Some(json!({})));
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn no_prompt_cache_key_for_openai_compatible_by_provider_name() {
        let openai = extend(
            mock_model_object(),
            json!({
                "providerID": "openai",
                "api": { "id": "gpt-5", "url": "https://example.com", "npm": "@ai-sdk/openai-compatible" },
            }),
        );
        let result = run(openai, Some(json!({})));
        assert!(result.get("promptCacheKey").is_none());
    }

    #[test]
    fn openai_disabled_keeps_store_false() {
        let openai = extend(
            mock_model_object(),
            json!({
                "providerID": "openai",
                "api": { "id": "gpt-4", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            }),
        );
        let result = run(openai, Some(json!({ "setCacheKey": false })));
        assert_eq!(result["store"], false);
        assert!(result.get("promptCacheKey").is_none());
    }

    #[test]
    fn xai_sdk_by_default() {
        let xai = extend(
            mock_model_object(),
            json!({
                "providerID": "custom-xai",
                "api": { "id": "grok-4", "url": "https://api.x.ai", "npm": "@ai-sdk/xai" },
            }),
        );
        let result = run(xai, Some(json!({})));
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn xai_disabled() {
        let xai = extend(
            mock_model_object(),
            json!({
                "providerID": "xai",
                "api": { "id": "grok-4", "url": "https://api.x.ai", "npm": "@ai-sdk/xai" },
            }),
        );
        let result = run(xai, Some(json!({ "setCacheKey": false })));
        assert!(result.get("promptCacheKey").is_none());
    }

    #[test]
    fn store_false_for_openai() {
        let openai = extend(
            mock_model_object(),
            json!({
                "providerID": "openai",
                "api": { "id": "gpt-4", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            }),
        );
        let result = run(openai, Some(json!({})));
        assert_eq!(result["store"], false);
    }

    #[test]
    fn store_false_for_xai() {
        let xai = extend(
            mock_model_object(),
            json!({
                "providerID": "xai",
                "api": { "id": "grok-4", "url": "https://api.x.ai", "npm": "@ai-sdk/xai" },
            }),
        );
        let result = run(xai, Some(json!({})));
        assert_eq!(result["store"], false);
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn store_false_for_azure() {
        let azure = extend(
            mock_model_object(),
            json!({
                "providerID": "azure",
                "api": { "id": "gpt-4", "url": "https://azure.com", "npm": "@ai-sdk/azure" },
            }),
        );
        let result = run(azure, Some(json!({})));
        assert_eq!(result["store"], false);
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn azure_gpt5_5_keeps_cache_key() {
        let azure = extend(
            mock_model_object(),
            json!({
                "providerID": "azure",
                "api": { "id": "gpt-5.5", "url": "https://azure.com", "npm": "@ai-sdk/azure" },
            }),
        );
        let result = run(azure, Some(json!({})));
        assert_eq!(result["store"], false);
        assert_eq!(result["reasoningSummary"], "auto");
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn snake_case_key_for_deepinfra_and_cerebras() {
        for npm in ["@ai-sdk/deepinfra", "@ai-sdk/cerebras"] {
            let api = mock_model_object()["api"].clone();
            let custom = extend(
                mock_model_object(),
                json!({
                    "providerID": "custom",
                    "api": extend(api, json!({ "npm": npm })),
                }),
            );
            let result = run(custom, Some(json!({})));
            assert_eq!(result["prompt_cache_key"], "test-session-123");
            assert!(result.get("promptCacheKey").is_none());
        }
    }

    #[test]
    fn prompt_cache_key_for_mistral() {
        let api = mock_model_object()["api"].clone();
        let custom = extend(
            mock_model_object(),
            json!({
                "providerID": "custom",
                "api": extend(api, json!({ "npm": "@ai-sdk/mistral" })),
            }),
        );
        let result = run(custom, Some(json!({})));
        assert_eq!(result["promptCacheKey"], "test-session-123");
    }

    #[test]
    fn no_undocumented_openrouter_cache_key() {
        let api = mock_model_object()["api"].clone();
        let custom = extend(
            mock_model_object(),
            json!({
                "providerID": "openrouter",
                "api": extend(api, json!({ "npm": "@openrouter/ai-sdk-provider" })),
            }),
        );
        let result = run(custom, Some(json!({})));
        assert!(result.get("prompt_cache_key").is_none());
    }
}

mod zai_thinking {
    use super::*;

    fn create_model(provider_id: &str) -> Value {
        json!({
            "id": format!("{}/glm-4.6", provider_id),
            "providerID": provider_id,
            "api": { "id": "glm-4.6", "url": "https://open.bigmodel.cn/api/paas/v4", "npm": "@ai-sdk/openai-compatible" },
            "name": "GLM 4.6",
            "capabilities": {
                "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": true },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 128000, "output": 8192 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn sets_thinking_config() {
        for provider_id in ["zai-coding-plan", "zai", "zhipuai-coding-plan", "zhipuai"] {
            let result = run_options(create_model(provider_id), Some(json!({})));
            assert_eq!(
                result["thinking"],
                json!({ "type": "enabled", "clear_thinking": false })
            );
        }
    }
}

mod minimax_m3_thinking {
    use super::*;

    fn create_model(npm: &str) -> Value {
        json!({
            "id": "minimax/minimax-m3",
            "providerID": "minimax",
            "api": { "id": "minimax-m3", "url": "https://api.minimax.com", "npm": npm },
            "capabilities": { "reasoning": true },
            "limit": { "output": 64000 },
        })
    }

    #[test]
    fn anthropic_sdk_uses_adaptive_thinking() {
        let result = run_options(create_model("@ai-sdk/anthropic"), None);
        assert_eq!(result["thinking"], json!({ "type": "adaptive" }));
    }

    #[test]
    fn openai_compatible_uses_native_default() {
        let result = run_options(create_model("@ai-sdk/openai-compatible"), None);
        assert!(result.get("thinking").is_none());
    }
}

mod google_thinking_config {
    use super::*;

    fn create_model(reasoning: bool, npm: &str) -> Value {
        let provider_id = if npm == "@ai-sdk/google" {
            "google"
        } else {
            "google-vertex"
        };
        json!({
            "id": format!("{}/gemini-2.0-flash", provider_id),
            "providerID": provider_id,
            "api": { "id": "gemini-2.0-flash", "url": "https://generativelanguage.googleapis.com", "npm": npm },
            "name": "Gemini 2.0 Flash",
            "capabilities": {
                "temperature": true, "reasoning": reasoning, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": true },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 1000000, "output": 8192 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn no_thinking_config_without_reasoning() {
        let result = run_options(create_model(false, "@ai-sdk/google"), Some(json!({})));
        assert!(result.get("thinkingConfig").is_none());
    }

    #[test]
    fn sets_thinking_config_with_reasoning() {
        let result = run_options(create_model(true, "@ai-sdk/google"), Some(json!({})));
        assert_eq!(result["thinkingConfig"], json!({ "includeThoughts": true }));
    }

    #[test]
    fn no_thinking_config_for_vertex_without_reasoning() {
        let result = run_options(
            create_model(false, "@ai-sdk/google-vertex"),
            Some(json!({})),
        );
        assert!(result.get("thinkingConfig").is_none());
    }
}

mod gpt5_text_verbosity {
    use super::*;

    fn create_gpt5_model(api_id: &str) -> Value {
        json!({
            "id": format!("openai/{}", api_id),
            "providerID": "openai",
            "api": { "id": api_id, "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "name": api_id,
            "capabilities": {
                "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.03, "output": 0.06, "cache": { "read": 0.001, "write": 0.002 } },
            "limit": { "context": 128000, "output": 4096 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn gpt5_2_has_text_verbosity_low() {
        let result = run_options(create_gpt5_model("gpt-5.2"), Some(json!({})));
        assert_eq!(result["textVerbosity"], "low");
        assert_eq!(result["include"], json!(["reasoning.encrypted_content"]));
    }

    #[test]
    fn bedrock_mantle_gpt5_5_uses_openai_responses_defaults() {
        let model_json = extend(
            create_gpt5_model("openai.gpt-5.5"),
            json!({
                "id": "amazon-bedrock/openai.gpt-5.5",
                "providerID": "amazon-bedrock",
                "api": { "id": "openai.gpt-5.5", "url": "https://bedrock-mantle.us-east-2.api.aws/openai/v1", "npm": "@ai-sdk/amazon-bedrock/mantle" },
            }),
        );
        let result = run_options(model_json, Some(json!({})));
        assert_eq!(result["store"], false);
        assert_eq!(result["reasoningEffort"], "medium");
        assert_eq!(result["reasoningSummary"], "auto");
        assert_eq!(result["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(result["textVerbosity"], "low");
    }

    #[test]
    fn openai_compatible_omits_responses_only_reasoning_summary() {
        let model_json = extend(
            create_gpt5_model("gpt-5.4"),
            json!({
                "id": "cortecs/gpt-5.4",
                "providerID": "cortecs",
                "api": { "id": "gpt-5.4", "url": "https://api.cortecs.ai/v1", "npm": "@ai-sdk/openai-compatible" },
            }),
        );
        let result = run_options(model_json, Some(json!({})));
        assert_eq!(result["reasoningEffort"], "medium");
        assert!(result.get("reasoningSummary").is_none());
        assert!(result.get("include").is_none());
    }

    #[test]
    fn gpt5_1_has_text_verbosity_low() {
        let result = run_options(create_gpt5_model("gpt-5.1"), Some(json!({})));
        assert_eq!(result["textVerbosity"], "low");
    }

    #[test]
    fn chat_models_have_no_text_verbosity() {
        for api_id in [
            "gpt-5.2-chat-latest",
            "gpt-5.1-chat-latest",
            "gpt-5.2-chat",
            "gpt-5-chat",
            "gpt-5.2-codex",
        ] {
            let result = run_options(create_gpt5_model(api_id), Some(json!({})));
            assert!(
                result.get("textVerbosity").is_none(),
                "{} should not have textVerbosity",
                api_id
            );
        }
    }
}

mod gpt5_reasoning_effort {
    use super::*;

    fn create_model(api_id: &str) -> Value {
        json!({
            "id": format!("azure/{}", api_id),
            "providerID": "azure",
            "api": { "id": api_id, "url": "https://azure.com", "npm": "@ai-sdk/azure" },
            "name": api_id,
            "capabilities": {
                "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.03, "output": 0.06, "cache": { "read": 0.001, "write": 0.002 } },
            "limit": { "context": 128000, "output": 4096 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn gpt5_chat_no_reasoning_effort() {
        let result = run_options(create_model("gpt-5-chat"), Some(json!({})));
        assert!(result.get("reasoningEffort").is_none());
    }

    #[test]
    fn gpt5_5_no_effort_for_completions() {
        for api_id in ["gpt-5.5", "gpt-5.6"] {
            let result = run_options(
                create_model(api_id),
                Some(json!({ "useCompletionUrls": true })),
            );
            assert!(result.get("reasoningEffort").is_none(), "{}", api_id);
        }
    }

    #[test]
    fn gpt5_6_medium_for_responses() {
        let result = run_options(create_model("gpt-5.6"), Some(json!({})));
        assert_eq!(result["reasoningEffort"], "medium");
    }
}

mod gateway_options {
    use super::*;

    fn create_model(id: &str) -> Value {
        json!({
            "id": id,
            "providerID": "vercel",
            "api": { "id": id, "url": "https://ai-gateway.vercel.sh/v3/ai", "npm": "@ai-sdk/gateway" },
            "name": id,
            "capabilities": {
                "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": true },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 200000, "output": 8192 },
            "status": "active", "options": {}, "headers": {},
            "release_date": "2024-01-01",
        })
    }

    #[test]
    fn gateway_defaults_under_gateway_key() {
        let result = run_options(create_model("anthropic/claude-sonnet-4"), Some(json!({})));
        assert_eq!(result, json!({ "gateway": { "caching": "auto" } }));
    }
}

mod provider_options_tests {
    use super::*;

    fn create_model(overrides: Value) -> Value {
        extend(
            json!({
                "id": "test/test-model",
                "providerID": "test",
                "api": { "id": "test-model", "url": "https://api.test.com", "npm": "@ai-sdk/openai" },
                "name": "Test Model",
                "capabilities": {
                    "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                    "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                    "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                    "interleaved": false,
                },
                "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
                "limit": { "context": 200000, "output": 64000 },
                "status": "active", "options": {}, "headers": {},
                "release_date": "2024-01-01",
            }),
            overrides,
        )
    }

    fn run(model_json: Value, options_value: Value) -> Value {
        let m = model(model_json);
        Value::Object(provider_options(&m, options_value.as_object().unwrap()))
    }

    #[test]
    fn uses_sdk_key_for_non_gateway_models() {
        let model_json = create_model(json!({
            "providerID": "my-bedrock",
            "api": { "id": "anthropic.claude-sonnet-4", "url": "https://bedrock.aws", "npm": "@ai-sdk/amazon-bedrock" },
        }));
        assert_eq!(
            run(model_json, json!({ "cachePoint": { "type": "default" } })),
            json!({ "bedrock": { "cachePoint": { "type": "default" } } })
        );
    }

    #[test]
    fn forces_reasoning_for_custom_openai_models_with_explicit_effort() {
        let model_json = create_model(json!({
            "providerID": "meta",
            "api": { "id": "muse-spark", "url": "https://api.ai.meta.com/v1", "npm": "@ai-sdk/openai" },
        }));
        assert_eq!(
            run(
                model_json,
                json!({ "reasoningEffort": "xhigh", "reasoningSummary": "auto" })
            ),
            json!({ "openai": { "forceReasoning": true, "reasoningEffort": "xhigh", "reasoningSummary": "auto" } })
        );
    }

    #[test]
    fn forces_reasoning_for_openai_reasoning_capable() {
        assert_eq!(
            run(create_model(json!({})), json!({ "store": false })),
            json!({ "openai": { "forceReasoning": true, "store": false } })
        );
    }

    #[test]
    fn canonical_sdk_key_for_custom_xai() {
        let model_json = create_model(json!({
            "providerID": "my-xai",
            "api": { "id": "grok-4", "url": "https://api.x.ai", "npm": "@ai-sdk/xai" },
        }));
        assert_eq!(
            run(model_json, json!({ "promptCacheKey": "session" })),
            json!({ "xai": { "promptCacheKey": "session" } })
        );
    }

    #[test]
    fn forces_reasoning_for_explicit_effort_when_not_reasoning_capable() {
        let model_json = create_model(json!({
            "capabilities": {
                "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
        }));
        assert_eq!(
            run(model_json, json!({ "reasoningEffort": "xhigh" })),
            json!({ "openai": { "forceReasoning": true, "reasoningEffort": "xhigh" } })
        );
    }

    #[test]
    fn forces_reasoning_for_azure_with_explicit_effort() {
        let model_json = create_model(json!({
            "providerID": "azure",
            "api": { "id": "custom-gpt-5-deployment", "url": "https://azure.openai.example.com/openai/v1", "npm": "@ai-sdk/azure" },
        }));
        assert_eq!(
            run(model_json, json!({ "reasoningEffort": "xhigh" })),
            json!({ "openai": { "forceReasoning": true, "reasoningEffort": "xhigh" }, "azure": { "forceReasoning": true, "reasoningEffort": "xhigh" } })
        );
    }

    #[test]
    fn forces_reasoning_for_mantle_with_explicit_effort() {
        let model_json = create_model(json!({
            "providerID": "amazon-bedrock",
            "api": { "id": "openai.gpt-5-custom", "url": "https://bedrock-mantle.us-east-2.api.aws/openai/v1", "npm": "@ai-sdk/amazon-bedrock/mantle" },
        }));
        assert_eq!(
            run(model_json, json!({ "reasoningEffort": "xhigh" })),
            json!({ "openai": { "forceReasoning": true, "reasoningEffort": "xhigh" } })
        );
    }

    #[test]
    fn overrides_force_reasoning_false() {
        assert_eq!(
            run(
                create_model(json!({})),
                json!({ "forceReasoning": false, "reasoningEffort": "xhigh" })
            ),
            json!({ "openai": { "forceReasoning": true, "reasoningEffort": "xhigh" } })
        );
    }

    #[test]
    fn gateway_model_provider_slug() {
        let model_json = create_model(json!({
            "providerID": "vercel",
            "api": { "id": "anthropic/claude-sonnet-4", "url": "https://ai-gateway.vercel.sh/v3/ai", "npm": "@ai-sdk/gateway" },
        }));
        assert_eq!(
            run(
                model_json,
                json!({ "thinking": { "type": "enabled", "budgetTokens": 12000 } })
            ),
            json!({ "anthropic": { "thinking": { "type": "enabled", "budgetTokens": 12000 } } })
        );
    }

    #[test]
    fn gateway_key_fallback_when_unscoped() {
        let model_json = create_model(json!({
            "id": "anthropic/claude-sonnet-4",
            "providerID": "vercel",
            "api": { "id": "claude-sonnet-4", "url": "https://ai-gateway.vercel.sh/v3/ai", "npm": "@ai-sdk/gateway" },
        }));
        assert_eq!(
            run(
                model_json,
                json!({ "thinking": { "type": "enabled", "budgetTokens": 12000 } })
            ),
            json!({ "gateway": { "thinking": { "type": "enabled", "budgetTokens": 12000 } } })
        );
    }

    #[test]
    fn splits_gateway_routing_from_provider_options() {
        let model_json = create_model(json!({
            "providerID": "vercel",
            "api": { "id": "anthropic/claude-sonnet-4", "url": "https://ai-gateway.vercel.sh/v3/ai", "npm": "@ai-sdk/gateway" },
        }));
        assert_eq!(
            run(
                model_json,
                json!({ "gateway": { "order": ["vertex", "anthropic"] }, "thinking": { "type": "enabled", "budgetTokens": 12000 } }),
            ),
            json!({ "gateway": { "order": ["vertex", "anthropic"] }, "anthropic": { "thinking": { "type": "enabled", "budgetTokens": 12000 } } })
        );
    }

    #[test]
    fn gateway_key_fallback_when_no_provider_slug() {
        let model_json = create_model(json!({
            "id": "claude-sonnet-4",
            "providerID": "vercel",
            "api": { "id": "claude-sonnet-4", "url": "https://ai-gateway.vercel.sh/v3/ai", "npm": "@ai-sdk/gateway" },
        }));
        assert_eq!(
            run(model_json, json!({ "reasoningEffort": "high" })),
            json!({ "gateway": { "reasoningEffort": "high" } })
        );
    }

    #[test]
    fn maps_amazon_slug_to_bedrock() {
        let model_json = create_model(json!({
            "providerID": "vercel",
            "api": { "id": "amazon/nova-2-lite", "url": "https://ai-gateway.vercel.sh/v3/ai", "npm": "@ai-sdk/gateway" },
        }));
        assert_eq!(
            run(
                model_json,
                json!({ "reasoningConfig": { "type": "enabled" } })
            ),
            json!({ "bedrock": { "reasoningConfig": { "type": "enabled" } } })
        );
    }

    #[test]
    fn mantle_maps_to_openai_namespace() {
        let model_json = create_model(json!({
            "providerID": "amazon-bedrock",
            "api": { "id": "openai.gpt-5.5", "url": "https://bedrock-mantle.us-east-2.api.aws/openai/v1", "npm": "@ai-sdk/amazon-bedrock/mantle" },
        }));
        assert_eq!(
            run(model_json, json!({ "reasoningEffort": "medium" })),
            json!({ "openai": { "forceReasoning": true, "reasoningEffort": "medium" } })
        );
    }
}

mod schema_tests {
    use super::*;

    fn run(model_json: Value, schema_value: Value) -> Value {
        schema(&model(model_json), schema_value)
    }

    #[test]
    fn gemini_adds_missing_array_items() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({
            "type": "object",
            "properties": {
                "nodes": { "type": "array" },
                "edges": { "type": "array", "items": { "type": "string" } },
            },
        });
        let result = run(model_json, input);
        assert!(result["properties"]["nodes"]["items"].is_object());
        assert_eq!(result["properties"]["edges"]["items"]["type"], "string");
    }

    #[test]
    fn gemini_nested_arrays_get_default_item_type() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({
            "type": "object",
            "properties": {
                "values": { "type": "array", "items": { "type": "array", "items": {} } },
                "data": { "type": "array", "items": { "type": "array" } },
            },
        });
        let result = run(model_json, input);
        assert_eq!(
            result["properties"]["values"]["items"]["items"]["type"],
            "string"
        );
        assert_eq!(
            result["properties"]["data"]["items"]["items"]["type"],
            "string"
        );
    }

    #[test]
    fn gemini_splits_type_arrays_into_any_of() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({
            "type": "object",
            "properties": {
                "status": { "type": ["number", "string"], "description": "status filter" },
            },
        });
        let result = run(model_json, input);
        assert!(result["properties"]["status"].get("type").is_none());
        assert_eq!(
            result["properties"]["status"]["anyOf"],
            json!([{ "type": "number" }, { "type": "string" }])
        );
        assert_eq!(
            result["properties"]["status"]["description"],
            "status filter"
        );
    }

    #[test]
    fn gemini_lifts_null_into_nullable() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({
            "type": "object",
            "properties": { "maybe": { "type": ["string", "null"], "description": "nullable string" } },
        });
        let result = run(model_json, input);
        assert_eq!(
            result["properties"]["maybe"]["anyOf"],
            json!([{ "type": "string" }])
        );
        assert_eq!(result["properties"]["maybe"]["nullable"], true);
    }

    #[test]
    fn gemini_collapses_all_null_type_array() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({ "type": "object", "properties": { "nothing": { "type": ["null"] } } });
        let result = run(model_json, input);
        assert_eq!(result["properties"]["nothing"]["type"], "null");
        assert!(result["properties"]["nothing"].get("anyOf").is_none());
    }

    #[test]
    fn gemini_rewrites_type_arrays_for_copilot() {
        let model_json = json!({ "providerID": "github-copilot", "api": { "id": "gemini-3.5-flash", "npm": "@ai-sdk/github-copilot" } });
        let input = json!({
            "type": "object",
            "properties": {
                "hook_id": { "type": "number", "description": "ID of the webhook" },
                "status": { "type": ["number", "string"], "description": "Filter by response status code" },
            },
            "required": ["hook_id"],
            "additionalProperties": false,
        });
        let result = run(model_json, input);
        assert_eq!(
            result["properties"]["status"]["anyOf"],
            json!([{ "type": "number" }, { "type": "string" }])
        );
        assert!(result["properties"]["status"].get("type").is_none());
        assert_eq!(result["properties"]["hook_id"]["type"], "number");
    }

    #[test]
    fn gemini_keeps_combiner_nodes_untouched() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "items": { "anyOf": [{ "type": "string" }, { "type": "number" }] },
                },
                "value": { "oneOf": [{ "type": "string" }, { "type": "boolean" }] },
                "meta": { "allOf": [{ "type": "object", "properties": { "a": { "type": "string" } } }, { "type": "object", "properties": { "b": { "type": "string" } } }] },
            },
        });
        let result = run(model_json, input);
        assert!(result["properties"]["edits"]["items"]["anyOf"].is_array());
        assert!(result["properties"]["edits"]["items"].get("type").is_none());
    }

    #[test]
    fn gemini_removes_properties_from_non_object_types() {
        let model_json = json!({ "providerID": "google", "api": { "id": "gemini-3-pro" } });
        let input = json!({
            "type": "object",
            "properties": {
                "data": { "type": "string", "properties": { "invalid": { "type": "string" } } },
                "list": { "type": "array", "items": { "type": "string" }, "required": ["invalid"] },
            },
        });
        let result = run(model_json, input);
        assert_eq!(result["properties"]["data"]["type"], "string");
        assert!(result["properties"]["data"].get("properties").is_none());
        assert_eq!(result["properties"]["list"]["type"], "array");
        assert!(result["properties"]["list"].get("required").is_none());
    }

    #[test]
    fn openai_supported_schema_subset() {
        let model_json =
            json!({ "providerID": "openai", "api": { "id": "gpt-4.1", "npm": "@ai-sdk/openai" } });
        let input = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Search",
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query", "format": "uri", "pattern": "^https://", "minLength": 1, "maxLength": 100, "default": "https://example.com" },
                "count": { "type": "integer", "minimum": 1, "maximum": 10, "multipleOf": 1 },
                "createdAt": { "format": "date-time" },
                "mode": { "const": "fast" },
                "tags": { "type": "array", "minItems": 1, "maxItems": 3, "uniqueItems": true },
                "tuple": { "type": "array", "items": [{ "type": "number", "minimum": 0 }, { "type": "string", "pattern": "^ok$" }] },
                "metadata": { "type": "object", "patternProperties": { "^x-": { "type": "string" } }, "additionalProperties": { "type": "string", "pattern": "^safe$" } },
            },
            "patternProperties": { "^extra": { "type": "string" } },
            "required": ["query"],
            "additionalProperties": false,
        });
        let result = run(model_json, input);
        let expected = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "count": { "type": "integer" },
                "createdAt": { "type": "string" },
                "mode": { "enum": ["fast"], "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "tuple": { "type": "array", "items": [{ "type": "number" }, { "type": "string" }] },
                "metadata": { "type": "object", "properties": {}, "additionalProperties": { "type": "string" } },
            },
            "required": ["query"],
            "additionalProperties": false,
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn openai_keeps_local_refs_and_sanitizes_definitions() {
        let model_json =
            json!({ "providerID": "openai", "api": { "id": "gpt-4.1", "npm": "@ai-sdk/openai" } });
        let input = json!({
            "type": "object",
            "properties": {
                "value": { "$ref": "#/$defs/Value", "description": "Referenced value", "examples": ["ignored"] },
            },
            "$defs": {
                "Value": { "type": "string", "pattern": "^value$", "description": "Definition description" },
                "Unused": { "type": "number", "minimum": 0 },
            },
        });
        let result = run(model_json, input);
        assert_eq!(
            result["properties"]["value"],
            json!({ "$ref": "#/$defs/Value", "description": "Referenced value" })
        );
        assert_eq!(
            result["$defs"],
            json!({
                "Value": { "type": "string", "description": "Definition description" },
                "Unused": { "type": "number" },
            })
        );
    }

    #[test]
    fn does_not_sanitize_non_openai_providers() {
        let model_json = json!({ "providerID": "anthropic", "api": { "id": "claude-sonnet-4", "npm": "@ai-sdk/anthropic" } });
        let input = json!({ "type": "object", "properties": { "query": { "type": "string", "pattern": "^https://" } } });
        let result = run(model_json, input);
        assert_eq!(result["properties"]["query"]["pattern"], "^https://");
    }

    #[test]
    fn moonshot_removes_ref_siblings() {
        let model_json = json!({ "providerID": "moonshotai", "api": { "id": "kimi-k2" } });
        let input = json!({
            "type": "object",
            "properties": { "variantOptions": { "$ref": "#/$defs/VariantOptions", "description": "Required. The variant options." } },
            "$defs": { "VariantOptions": { "type": "object", "description": "config", "properties": {} } },
        });
        let result = run(model_json, input);
        assert_eq!(
            result["properties"]["variantOptions"],
            json!({ "$ref": "#/$defs/VariantOptions" })
        );
        assert_eq!(result["$defs"]["VariantOptions"]["description"], "config");
    }

    #[test]
    fn moonshot_converts_tuple_items_to_single_schema() {
        let model_json = json!({ "providerID": "moonshotai", "api": { "id": "kimi-k2" } });
        let input = json!({
            "type": "object",
            "properties": { "codeSpec": { "type": "object", "properties": { "renderedSize": { "type": "array", "items": [{ "type": "number" }, { "type": "number" }] } } } },
        });
        let result = run(model_json, input);
        assert_eq!(
            result["properties"]["codeSpec"]["properties"]["renderedSize"]["items"],
            json!({ "type": "number" })
        );
    }
}

mod message_tests {
    use super::*;

    #[test]
    fn mistral_tool_call_ids_normalized() {
        for api_id in [
            "codestral-latest",
            "pixtral-large-latest",
            "open-mixtral-8x22b",
        ] {
            let model_json = json!({
                "id": format!("custom/{}", api_id),
                "providerID": "custom",
                "api": { "id": api_id, "url": "https://example.com/v1", "npm": "@ai-sdk/openai-compatible" },
            });
            let input = messages(json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "tool-call", "toolCallId": "toolu_01CBhTTz95qkd9LJMdC9sf8t", "toolName": "read", "input": { "filePath": "/tmp/test" } },
                    ],
                },
                {
                    "role": "tool",
                    "content": [
                        { "type": "tool-result", "toolCallId": "toolu_01CBhTTz95qkd9LJMdC9sf8t", "toolName": "read", "output": { "type": "text", "value": "test" } },
                    ],
                },
            ]));
            let result = message(input, &model(model_json), &Map::new());
            let serialized = serde_json::to_value(&result).unwrap();
            assert_eq!(serialized[0]["content"][0]["toolCallId"], "toolu01CB");
            assert_eq!(serialized[1]["content"][0]["toolCallId"], "toolu01CB");
        }
    }

    #[test]
    fn deepseek_reasoning_content_in_provider_options() {
        let model_json = json!({
            "id": "deepseek/deepseek-chat",
            "providerID": "deepseek",
            "api": { "id": "deepseek-chat", "url": "https://api.deepseek.com", "npm": "@ai-sdk/openai-compatible" },
            "name": "DeepSeek Chat",
            "capabilities": {
                "temperature": true, "reasoning": true, "attachment": false, "toolcall": true,
                "input": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": { "field": "reasoning_content" },
            },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 128000, "output": 8192 },
            "status": "active", "options": {}, "headers": {},
            "release_date": "2023-04-01",
        });
        let input = messages(json!([
            {
                "role": "assistant",
                "content": [
                    { "type": "reasoning", "text": "Let me think about this..." },
                    { "type": "tool-call", "toolCallId": "test", "toolName": "bash", "input": { "command": "echo hello" } },
                ],
            },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["content"],
            json!([{ "type": "tool-call", "toolCallId": "test", "toolName": "bash", "input": { "command": "echo hello" } }])
        );
        assert_eq!(
            serialized[0]["providerOptions"]["openaiCompatible"]["reasoning_content"],
            "Let me think about this..."
        );
    }

    #[test]
    fn non_deepseek_leaves_reasoning_unchanged() {
        let model_json = json!({
            "id": "openai/gpt-4",
            "providerID": "openai",
            "api": { "id": "gpt-4", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "name": "GPT-4",
            "capabilities": {
                "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.03, "output": 0.06, "cache": { "read": 0.001, "write": 0.002 } },
            "limit": { "context": 128000, "output": 4096 },
            "status": "active", "options": {}, "headers": {},
            "release_date": "2023-04-01",
        });
        let input = messages(json!([
            { "role": "assistant", "content": [ { "type": "reasoning", "text": "Should not be processed" }, { "type": "text", "text": "Answer" } ] },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["content"][0]["text"],
            "Should not be processed"
        );
        assert!(serialized[0].get("providerOptions").is_none());
    }

    #[test]
    fn empty_image_replaced_with_error_text() {
        let model_json = extend(
            mock_model_object(),
            json!({ "id": "anthropic/claude-3-5-sonnet", "providerID": "anthropic" }),
        );
        let input = messages(json!([
            { "role": "user", "content": [ { "type": "text", "text": "What is in this image?" }, { "type": "image", "image": "data:image/png;base64," } ] },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["content"][1],
            json!({ "type": "text", "text": "ERROR: Image file is empty or corrupted. Please provide a valid image." })
        );
    }

    #[test]
    fn valid_image_kept() {
        let model_json = extend(
            mock_model_object(),
            json!({ "id": "anthropic/claude-3-5-sonnet", "providerID": "anthropic" }),
        );
        let valid = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let input = messages(json!([
            { "role": "user", "content": [ { "type": "text", "text": "What is in this image?" }, { "type": "image", "image": format!("data:image/png;base64,{}", valid) } ] },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(serialized[0]["content"][1]["type"], "image");
        assert!(serialized[0]["content"][1]["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn anthropic_filters_empty_content() {
        let model_json = extend(
            mock_model_object(),
            json!({ "id": "anthropic/claude-3-5-sonnet", "providerID": "anthropic" }),
        );
        let input = messages(json!([
            { "role": "user", "content": "Hello" },
            { "role": "assistant", "content": "" },
            { "role": "user", "content": "World" },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, MessageContent::Text("Hello".to_string()));
        assert_eq!(result[1].content, MessageContent::Text("World".to_string()));
    }

    #[test]
    fn anthropic_filters_empty_text_parts() {
        let model_json = extend(
            mock_model_object(),
            json!({ "id": "anthropic/claude-3-5-sonnet", "providerID": "anthropic" }),
        );
        let input = messages(json!([
            { "role": "assistant", "content": [ { "type": "text", "text": "" }, { "type": "text", "text": "Hello" }, { "type": "text", "text": "" } ] },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        assert_eq!(result.len(), 1);
        let parts = result[0].content.as_parts().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "Hello");
    }

    #[test]
    fn anthropic_removes_message_when_all_parts_empty() {
        let model_json = extend(
            mock_model_object(),
            json!({ "id": "anthropic/claude-3-5-sonnet", "providerID": "anthropic" }),
        );
        let input = messages(json!([
            { "role": "user", "content": "Hello" },
            { "role": "assistant", "content": [ { "type": "text", "text": "" }, { "type": "reasoning", "text": "" } ] },
            { "role": "user", "content": "World" },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, MessageContent::Text("Hello".to_string()));
        assert_eq!(result[1].content, MessageContent::Text("World".to_string()));
    }

    #[test]
    fn does_not_filter_for_non_anthropic() {
        let model_json = extend(
            mock_model_object(),
            json!({
                "providerID": "openai",
                "api": { "id": "gpt-4", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            }),
        );
        let input = messages(json!([
            { "role": "assistant", "content": "" },
            { "role": "assistant", "content": [ { "type": "text", "text": "" } ] },
        ]));
        let result = message(input, &model(model_json), &Map::new());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, MessageContent::Text("".to_string()));
    }

    fn openai_gpt5_model() -> Value {
        json!({
            "id": "openai/gpt-5",
            "providerID": "openai",
            "api": { "id": "gpt-5", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "name": "GPT-5",
            "capabilities": {
                "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.03, "output": 0.06, "cache": { "read": 0.001, "write": 0.002 } },
            "limit": { "context": 128000, "output": 4096 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn strips_openai_item_id_when_store_false() {
        let input = messages(json!([
            {
                "role": "assistant",
                "content": [
                    { "type": "reasoning", "text": "thinking...", "providerOptions": { "openai": { "itemId": "rs_123", "reasoningEncryptedContent": "encrypted" } } },
                    { "type": "text", "text": "Hello", "providerOptions": { "openai": { "itemId": "msg_456" } } },
                ],
            },
        ]));
        let result = message(
            input,
            &model(openai_gpt5_model()),
            &options_map(&[("store", json!(false))]),
        );
        let serialized = serde_json::to_value(&result).unwrap();
        assert!(serialized[0]["content"][0]["providerOptions"]["openai"]
            .get("itemId")
            .is_none());
        assert_eq!(
            serialized[0]["content"][0]["providerOptions"]["openai"]["reasoningEncryptedContent"],
            "encrypted"
        );
        assert!(serialized[0]["content"][1]["providerOptions"]["openai"]
            .get("itemId")
            .is_none());
    }

    #[test]
    fn preserves_openai_item_id_when_store_true() {
        let input = messages(json!([
            { "role": "assistant", "content": [ { "type": "text", "text": "Hello", "providerOptions": { "openai": { "itemId": "msg_123" } } } ] },
        ]));
        let result = message(
            input,
            &model(openai_gpt5_model()),
            &options_map(&[("store", json!(true))]),
        );
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["content"][0]["providerOptions"]["openai"]["itemId"],
            "msg_123"
        );
    }

    fn generic_model(provider_id: &str, npm: &str) -> Value {
        json!({
            "id": format!("{}/test-model", provider_id),
            "providerID": provider_id,
            "api": { "id": "test-model", "url": "https://api.test.com", "npm": npm },
            "name": "Test Model",
            "capabilities": {
                "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": true },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false,
            },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 128000, "output": 8192 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn azure_keeps_azure_key_and_does_not_remap_to_openai() {
        let input = messages(json!([
            { "role": "user", "content": "Hello", "providerOptions": { "azure": { "someOption": "value" } } },
        ]));
        let result = message(
            input,
            &model(generic_model("azure", "@ai-sdk/azure")),
            &Map::new(),
        );
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["providerOptions"]["azure"],
            json!({ "someOption": "value" })
        );
        assert!(serialized[0]["providerOptions"].get("openai").is_none());
    }

    #[test]
    fn copilot_remaps_to_copilot_key() {
        let input = messages(json!([
            { "role": "user", "content": "Hello", "providerOptions": { "copilot": { "someOption": "value" } } },
        ]));
        let result = message(
            input,
            &model(generic_model("github-copilot", "@ai-sdk/github-copilot")),
            &Map::new(),
        );
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["providerOptions"]["copilot"],
            json!({ "someOption": "value" })
        );
        assert!(serialized[0]["providerOptions"]
            .get("github-copilot")
            .is_none());
    }

    #[test]
    fn azure_cognitive_services_remaps_to_azure_key() {
        let input = messages(json!([
            {
                "role": "user",
                "content": [ { "type": "text", "text": "Hello", "providerOptions": { "azure-cognitive-services": { "part": true } } } ],
                "providerOptions": { "azure-cognitive-services": { "someOption": "value" } },
            },
        ]));
        let result = message(
            input,
            &model(generic_model("azure-cognitive-services", "@ai-sdk/azure")),
            &Map::new(),
        );
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["providerOptions"]["azure"],
            json!({ "someOption": "value" })
        );
        assert!(serialized[0]["providerOptions"]
            .get("azure-cognitive-services")
            .is_none());
        assert_eq!(
            serialized[0]["content"][0]["providerOptions"]["azure"],
            json!({ "part": true })
        );
    }

    #[test]
    fn bedrock_remaps_to_bedrock_key() {
        let input = messages(json!([
            { "role": "user", "content": "Hello", "providerOptions": { "my-bedrock": { "someOption": "value" } } },
        ]));
        let result = message(
            input,
            &model(generic_model("my-bedrock", "@ai-sdk/amazon-bedrock")),
            &Map::new(),
        );
        let serialized = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serialized[0]["providerOptions"]["bedrock"],
            json!({ "someOption": "value" })
        );
        assert!(serialized[0]["providerOptions"].get("my-bedrock").is_none());
    }
}

mod sampling_tests {
    use super::*;

    fn m(api_id: &str) -> Model {
        model(json!({
            "id": format!("test/{}", api_id),
            "providerID": "test",
            "api": { "id": api_id, "url": "https://api.test.com", "npm": "@ai-sdk/openai-compatible" },
            "name": api_id,
            "capabilities": { "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 128000, "output": 64000 },
            "status": "active", "options": {}, "headers": {},
        }))
    }

    #[test]
    fn temperature_defaults() {
        assert_eq!(temperature(&m("qwen-plus")), Some(0.55));
        assert_eq!(temperature(&m("north-mini-code-1-0")), Some(1.0));
        assert_eq!(temperature(&m("claude-sonnet-4")), None);
        assert_eq!(temperature(&m("gemini-2.5-pro")), Some(1.0));
        assert_eq!(temperature(&m("gemini-3-flash")), Some(1.0));
        assert_eq!(temperature(&m("gemini-2.0-flash")), None);
        assert_eq!(temperature(&m("glm-4.6")), Some(1.0));
        assert_eq!(temperature(&m("kimi-k2-thinking")), Some(1.0));
        assert_eq!(temperature(&m("kimi-k2")), Some(0.6));
        assert_eq!(temperature(&m("gpt-4o")), None);
    }

    #[test]
    fn top_p_defaults() {
        assert_eq!(top_p(&m("qwen-plus")), Some(1.0));
        assert_eq!(top_p(&m("gemini-2.5-pro")), Some(0.95));
        assert_eq!(top_p(&m("gemini-3-flash")), Some(0.95));
        assert_eq!(top_p(&m("gemini-2.0-flash")), None);
        assert_eq!(top_p(&m("minimax-m2")), Some(0.95));
        assert_eq!(top_p(&m("kimi-k2.5")), Some(0.95));
        assert_eq!(top_p(&m("gpt-4o")), None);
    }

    #[test]
    fn top_k_defaults() {
        assert_eq!(top_k(&m("minimax-m2")), Some(20));
        assert_eq!(top_k(&m("minimax-m2.5")), Some(40));
        assert_eq!(top_k(&m("minimax-m25")), Some(40));
        assert_eq!(top_k(&m("gemini-2.5-pro")), Some(64));
        assert_eq!(top_k(&m("gemini-2.0-flash")), None);
        assert_eq!(top_k(&m("gpt-4o")), None);
    }

    fn limit_model(output: f64) -> Model {
        model(json!({
            "id": "test/m", "providerID": "test",
            "api": { "id": "m", "url": "", "npm": "@ai-sdk/openai-compatible" },
            "name": "m",
            "capabilities": { "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false },
            "cost": { "input": 0.0, "output": 0.0, "cache": { "read": 0.0, "write": 0.0 } },
            "limit": { "context": 128000, "output": output },
            "status": "active", "options": {}, "headers": {},
        }))
    }

    #[test]
    fn max_output_tokens_caps() {
        assert_eq!(max_output_tokens(&limit_model(8192.0), 32_000.0), 8192.0);
        assert_eq!(max_output_tokens(&limit_model(0.0), 32_000.0), 32_000.0);
    }

    #[test]
    fn sanitize_surrogates_passes_valid_utf8() {
        assert_eq!(sanitize_surrogates("hello world"), "hello world");
        assert_eq!(
            sanitize_surrogates("emoji \u{1F680} ok"),
            "emoji \u{1F680} ok"
        );
    }
}

mod small_options_tests {
    use super::*;

    fn base(provider_id: &str, api_id: &str, npm: &str) -> Value {
        json!({
            "id": format!("{}/{}", provider_id, api_id),
            "providerID": provider_id,
            "api": { "id": api_id, "url": "https://api.test.com", "npm": npm },
            "name": api_id,
            "capabilities": { "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false },
            "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
            "limit": { "context": 128000, "output": 64000 },
            "status": "active", "options": {}, "headers": {},
        })
    }

    #[test]
    fn openai_merges_store_false_with_first_variant() {
        let m = extend(
            base("openai", "gpt-5.2", "@ai-sdk/openai"),
            json!({
                "variants": { "high": { "reasoningEffort": "high" } },
            }),
        );
        let result = small_options(&model(m));
        assert_eq!(
            result,
            json!({ "store": false, "reasoningEffort": "high" })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn openrouter_google_reasoning_disabled() {
        let m = base(
            "openrouter",
            "google/gemini-3-pro",
            "@openrouter/ai-sdk-provider",
        );
        let result = small_options(&model(m));
        assert_eq!(
            result,
            json!({ "reasoning": { "enabled": false } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn venice_uses_first_variant_or_disable_thinking() {
        let m = base("venice", "llama-4", "venice-ai-sdk-provider");
        let result = small_options(&model(m));
        assert_eq!(
            result,
            json!({ "veniceParameters": { "disableThinking": true } })
                .as_object()
                .unwrap()
                .clone()
        );
    }
}

mod variants_tests {
    use super::*;

    fn create_model(overrides: Value) -> Value {
        extend(
            json!({
                "id": "test/test-model",
                "providerID": "test",
                "api": { "id": "test-model", "url": "https://api.test.com", "npm": "@ai-sdk/openai" },
                "name": "Test Model",
                "capabilities": {
                    "temperature": true, "reasoning": true, "attachment": true, "toolcall": true,
                    "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                    "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                    "interleaved": false,
                },
                "cost": { "input": 0.001, "output": 0.002, "cache": { "read": 0.0001, "write": 0.0002 } },
                "limit": { "context": 128000, "output": 64000 },
                "status": "active", "options": {}, "headers": {},
            }),
            overrides,
        )
    }

    fn keys(variants: &VariantMap) -> Vec<&str> {
        variants.keys().map(|k| k.as_str()).collect()
    }

    #[test]
    fn empty_when_not_reasoning() {
        let m = create_model(json!({
            "capabilities": { "temperature": true, "reasoning": false, "attachment": true, "toolcall": true,
                "input": { "text": true, "audio": false, "image": true, "video": false, "pdf": false },
                "output": { "text": true, "audio": false, "image": false, "video": false, "pdf": false },
                "interleaved": false },
        }));
        assert!(variants(&model(m)).is_empty());
    }

    #[test]
    fn openai_gpt5_standard_efforts() {
        let m = create_model(json!({
            "id": "openai/gpt-5",
            "providerID": "openai",
            "api": { "id": "gpt-5", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "release_date": "2024-01-01",
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["minimal", "low", "medium", "high"]);
        assert_eq!(v["high"], json!({ "reasoningEffort": "high", "reasoningSummary": "auto", "include": ["reasoning.encrypted_content"] }).as_object().unwrap().clone());
    }

    #[test]
    fn openai_none_effort_after_release_date() {
        let m = create_model(json!({
            "id": "openai/gpt-5",
            "providerID": "openai",
            "api": { "id": "gpt-5", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "release_date": "2025-12-01",
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["none", "minimal", "low", "medium", "high"]);
    }

    #[test]
    fn openai_xhigh_effort_after_release_date() {
        let m = create_model(json!({
            "id": "openai/gpt-5",
            "providerID": "openai",
            "api": { "id": "gpt-5", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "release_date": "2025-12-10",
        }));
        let v = variants(&model(m));
        assert_eq!(
            keys(&v),
            vec!["none", "minimal", "low", "medium", "high", "xhigh"]
        );
    }

    #[test]
    fn openai_gpt5_pro_only_high() {
        let m = create_model(json!({
            "id": "openai/gpt-5-pro",
            "providerID": "openai",
            "api": { "id": "gpt-5-pro", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "release_date": "2024-01-01",
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["high"]);
    }

    #[test]
    fn openai_gpt50_lookalike_untreated() {
        let m = create_model(json!({
            "id": "openai/gpt-50",
            "providerID": "openai",
            "api": { "id": "gpt-50", "url": "https://api.openai.com", "npm": "@ai-sdk/openai" },
            "release_date": "2025-12-10",
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["none", "low", "medium", "high", "xhigh"]);
    }

    #[test]
    fn groq_efforts() {
        let m = create_model(json!({
            "providerID": "groq",
            "api": { "id": "llama-4", "url": "https://api.groq.com", "npm": "@ai-sdk/groq" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["none", "low", "medium", "high"]);
        assert_eq!(
            v["low"],
            json!({ "reasoningEffort": "low" })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn cerebras_efforts() {
        let m = create_model(json!({
            "providerID": "cerebras",
            "api": { "id": "llama-4-sc", "url": "https://api.cerebras.ai", "npm": "@ai-sdk/cerebras" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high"]);
        assert_eq!(
            v["low"],
            json!({ "reasoningEffort": "low" })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn anthropic_high_max_with_thinking() {
        let m = create_model(json!({
            "providerID": "anthropic",
            "api": { "id": "claude-sonnet-4", "url": "https://api.anthropic.com", "npm": "@ai-sdk/anthropic" },
            "limit": { "context": 200000, "output": 64000 },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["high", "max"]);
        assert_eq!(
            v["high"],
            json!({ "thinking": { "type": "enabled", "budgetTokens": 16000 } })
                .as_object()
                .unwrap()
                .clone()
        );
        assert_eq!(
            v["max"],
            json!({ "thinking": { "type": "enabled", "budgetTokens": 31999 } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn anthropic_adaptive_efforts_for_modern_models() {
        let m = create_model(json!({
            "providerID": "anthropic",
            "api": { "id": "claude-opus-4-7", "url": "https://api.anthropic.com", "npm": "@ai-sdk/anthropic" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(v["high"], json!({ "thinking": { "type": "adaptive", "display": "summarized" }, "effort": "high" }).as_object().unwrap().clone());
    }

    #[test]
    fn anthropic_copilot_opus_4_7_only_medium() {
        let m = create_model(json!({
            "id": "claude-opus-4.7",
            "providerID": "github-copilot",
            "api": { "id": "claude-opus-4.7", "url": "https://api.githubcopilot.com", "npm": "@ai-sdk/anthropic" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["medium"]);
    }

    #[test]
    fn copilot_standard_efforts() {
        let m = create_model(json!({
            "providerID": "github-copilot",
            "api": { "id": "gpt-4.5", "url": "https://api.githubcopilot.com", "npm": "@ai-sdk/github-copilot" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high"]);
        assert_eq!(v["low"], json!({ "reasoningEffort": "low", "reasoningSummary": "auto", "include": ["reasoning.encrypted_content"] }).as_object().unwrap().clone());
    }

    #[test]
    fn copilot_gpt5_2_includes_xhigh() {
        let m = create_model(json!({
            "id": "gpt-5.2",
            "providerID": "github-copilot",
            "api": { "id": "gpt-5.2", "url": "https://api.githubcopilot.com", "npm": "@ai-sdk/github-copilot" },
            "release_date": "2024-01-01",
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high", "xhigh"]);
    }

    #[test]
    fn copilot_gemini_empty() {
        let m = create_model(json!({
            "id": "gemini-3-pro",
            "providerID": "github-copilot",
            "api": { "id": "gemini-3-pro", "url": "https://api.githubcopilot.com", "npm": "@ai-sdk/github-copilot" },
        }));
        assert!(variants(&model(m)).is_empty());
    }

    #[test]
    fn openai_compatible_deepseek_v4_has_max() {
        let m = create_model(json!({
            "providerID": "deepseek",
            "api": { "id": "deepseek-v4", "url": "https://api.deepseek.com", "npm": "@ai-sdk/openai-compatible" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high", "max"]);
    }

    #[test]
    fn openai_compatible_north_mini_code() {
        let m = create_model(json!({
            "providerID": "north",
            "api": { "id": "north-mini-code-1-0", "url": "https://api.test.com", "npm": "@ai-sdk/openai-compatible" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["none", "high"]);
    }

    #[test]
    fn google_thinking_variants_25() {
        let m = create_model(json!({
            "providerID": "google",
            "api": { "id": "gemini-2.5-pro", "url": "https://generativelanguage.googleapis.com", "npm": "@ai-sdk/google" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["high", "max"]);
        assert_eq!(
            v["high"],
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": 16000 } })
                .as_object()
                .unwrap()
                .clone()
        );
        assert_eq!(
            v["max"],
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingBudget": 32768 } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn google_thinking_level_variants() {
        let m = create_model(json!({
            "providerID": "google",
            "api": { "id": "gemini-3-flash", "url": "https://generativelanguage.googleapis.com", "npm": "@ai-sdk/google" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["minimal", "low", "medium", "high"]);
        assert_eq!(
            v["high"],
            json!({ "thinkingConfig": { "includeThoughts": true, "thinkingLevel": "high" } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn mistral_reasoning_ids_only() {
        let m = create_model(json!({
            "providerID": "mistral",
            "api": { "id": "mistral-large-latest", "url": "https://api.mistral.ai", "npm": "@ai-sdk/mistral" },
        }));
        assert!(variants(&model(m)).is_empty());

        let m = create_model(json!({
            "providerID": "mistral",
            "api": { "id": "mistral-medium-3.5", "url": "https://api.mistral.ai", "npm": "@ai-sdk/mistral" },
        }));
        let v = variants(&model(m));
        assert_eq!(
            v["high"],
            json!({ "reasoningEffort": "high" })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn deepseek_chat_returns_empty() {
        let m = create_model(json!({
            "id": "deepseek/deepseek-chat",
            "providerID": "deepseek",
            "api": { "id": "deepseek-chat", "url": "https://api.deepseek.com", "npm": "@ai-sdk/openai-compatible" },
        }));
        assert!(variants(&model(m)).is_empty());
    }

    #[test]
    fn gateway_anthropic_adaptive() {
        let m = create_model(json!({
            "id": "anthropic/claude-sonnet-5",
            "providerID": "gateway",
            "api": { "id": "anthropic/claude-sonnet-5", "url": "https://gateway.ai", "npm": "@ai-sdk/gateway" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(v["high"], json!({ "thinking": { "type": "adaptive", "display": "summarized" }, "effort": "high" }).as_object().unwrap().clone());
    }

    #[test]
    fn gateway_openai_efforts() {
        let m = create_model(json!({
            "id": "gateway/gateway-model",
            "providerID": "gateway",
            "api": { "id": "gateway-model", "url": "https://gateway.ai", "npm": "@ai-sdk/gateway" },
        }));
        let v = variants(&model(m));
        assert_eq!(
            keys(&v),
            vec!["none", "minimal", "low", "medium", "high", "xhigh"]
        );
    }

    #[test]
    fn openrouter_uses_openai_efforts_for_gpt() {
        let m = create_model(json!({
            "id": "openai/gpt-5.2",
            "providerID": "openrouter",
            "api": { "id": "openai/gpt-5.2", "url": "https://openrouter.ai", "npm": "@openrouter/ai-sdk-provider" },
            "release_date": "2024-01-01",
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["none", "low", "medium", "high", "xhigh"]);
        assert_eq!(
            v["low"],
            json!({ "reasoning": { "effort": "low" } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn bedrock_anthropic_reasoning_config() {
        let m = create_model(json!({
            "providerID": "amazon-bedrock",
            "api": { "id": "anthropic.claude-sonnet-4", "url": "https://bedrock-runtime.us-east-1.amazonaws.com", "npm": "@ai-sdk/amazon-bedrock" },
        }));
        let v = variants(&model(m));
        assert_eq!(
            v["high"],
            json!({ "reasoningConfig": { "type": "enabled", "budgetTokens": 16000 } })
                .as_object()
                .unwrap()
                .clone()
        );
        assert_eq!(
            v["max"],
            json!({ "reasoningConfig": { "type": "enabled", "budgetTokens": 31999 } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn bedrock_nova_reasoning_config() {
        let m = create_model(json!({
            "providerID": "amazon-bedrock",
            "api": { "id": "amazon.nova-pro-v1", "url": "https://bedrock-runtime.us-east-1.amazonaws.com", "npm": "@ai-sdk/amazon-bedrock" },
        }));
        let v = variants(&model(m));
        assert_eq!(keys(&v), vec!["low", "medium", "high"]);
        assert_eq!(
            v["low"],
            json!({ "reasoningConfig": { "type": "enabled", "maxReasoningEffort": "low" } })
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn azure_o1_mini_empty() {
        let m = create_model(json!({
            "id": "o1-mini",
            "providerID": "azure",
            "api": { "id": "o1-mini", "url": "https://azure.com", "npm": "@ai-sdk/azure" },
        }));
        assert!(variants(&model(m)).is_empty());
    }

    #[test]
    fn cohere_and_perplexity_empty() {
        for (provider_id, npm) in [
            ("cohere", "@ai-sdk/cohere"),
            ("perplexity", "@ai-sdk/perplexity"),
        ] {
            let m = create_model(json!({
                "providerID": provider_id,
                "api": { "id": "model", "url": "https://api.test.com", "npm": npm },
            }));
            assert!(variants(&model(m)).is_empty(), "{}", provider_id);
        }
    }
}
