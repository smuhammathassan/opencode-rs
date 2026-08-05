//! OpenAI-compatible provider profiles.
//! From reference/packages/llm/src/providers/openai-compatible-profile.ts

#[derive(Debug, Clone, Copy)]
pub struct OpenAICompatibleProfile {
    pub provider: &'static str,
    pub base_url: &'static str,
}

pub const PROFILES: [OpenAICompatibleProfile; 8] = [
    OpenAICompatibleProfile { provider: "baseten", base_url: "https://inference.baseten.co/v1" },
    OpenAICompatibleProfile { provider: "cerebras", base_url: "https://api.cerebras.ai/v1" },
    OpenAICompatibleProfile { provider: "deepinfra", base_url: "https://api.deepinfra.com/v1/openai" },
    OpenAICompatibleProfile { provider: "deepseek", base_url: "https://api.deepseek.com/v1" },
    OpenAICompatibleProfile { provider: "fireworks", base_url: "https://api.fireworks.ai/inference/v1" },
    OpenAICompatibleProfile { provider: "groq", base_url: "https://api.groq.com/openai/v1" },
    OpenAICompatibleProfile { provider: "openrouter", base_url: "https://openrouter.ai/api/v1" },
    OpenAICompatibleProfile { provider: "togetherai", base_url: "https://api.together.xyz/v1" },
];

/// `byProvider`.
/// From reference/packages/llm/src/providers/openai-compatible-profile.ts (`byProvider`)
pub fn by_provider(provider: &str) -> Option<OpenAICompatibleProfile> {
    PROFILES.iter().find(|profile| profile.provider == provider).copied()
}
