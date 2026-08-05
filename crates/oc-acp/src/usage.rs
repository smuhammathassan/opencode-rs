//! Usage tracking for ACP sessions.
//!
//! From reference/packages/opencode/src/acp/usage.ts and the inline
//! `makeUsageService` in reference/packages/opencode/src/acp/service.ts.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use tokio::sync::Mutex;

use crate::connection::AgentSideConnection;
use crate::sdk::{
    AssistantMessage, Message, OpencodeClient, ProviderInfo, SdkError, SessionMessageResponse,
};
use crate::types::{Cost, SessionUpdate, Usage, UsageUpdate};

/// `{ info: { role } | AssistantMessage }` used by the usage functions.
#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub info: Message,
}

/// Input for fetching a session's messages.
#[derive(Debug, Clone)]
pub struct MessagesInput {
    pub session_id: String,
    pub directory: String,
}

/// Loads messages for a session.
#[async_trait]
pub trait MessageLoader: Send + Sync {
    async fn messages(&self, input: MessagesInput) -> Result<Vec<SessionMessage>, SdkError>;
}

/// Loads providers for a directory.
#[async_trait]
pub trait ContextLimitLoader: Send + Sync {
    async fn providers(&self, directory: &str) -> Result<IndexMap<String, ProviderInfo>, SdkError>;
}

/// `messageLoaderFromSDK` from reference/packages/opencode/src/acp/usage.ts.
pub fn message_loader_from_sdk(sdk: Arc<dyn OpencodeClient>) -> impl MessageLoader {
    SdkMessageLoader { sdk }
}

struct SdkMessageLoader {
    sdk: Arc<dyn OpencodeClient>,
}

#[async_trait]
impl MessageLoader for SdkMessageLoader {
    async fn messages(&self, input: MessagesInput) -> Result<Vec<SessionMessage>, SdkError> {
        let responses: Vec<SessionMessageResponse> = self
            .sdk
            .session_messages(&input.directory, &input.session_id, None)
            .await?;
        Ok(responses
            .into_iter()
            .map(|response| SessionMessage {
                info: response.info,
            })
            .collect())
    }
}

/// `buildUsage` from reference/packages/opencode/src/acp/usage.ts.
pub fn build_usage(message: &AssistantMessage) -> Usage {
    let cached_read_tokens = message.tokens.cache.read;
    let cached_write_tokens = message.tokens.cache.write;
    let thought_tokens = message.tokens.reasoning;

    Usage {
        input_tokens: message.tokens.input,
        output_tokens: message.tokens.output,
        total_tokens: message.tokens.input
            + message.tokens.output
            + thought_tokens
            + cached_read_tokens
            + cached_write_tokens,
        thought_tokens: (thought_tokens > 0).then_some(thought_tokens),
        cached_read_tokens: (cached_read_tokens > 0).then_some(cached_read_tokens),
        cached_write_tokens: (cached_write_tokens > 0).then_some(cached_write_tokens),
    }
}

/// `latestAssistantMessage` from reference/packages/opencode/src/acp/usage.ts.
pub fn latest_assistant_message(messages: &[SessionMessage]) -> Option<&AssistantMessage> {
    messages
        .iter()
        .rev()
        .find_map(|message| match &message.info {
            Message::Assistant(message) if message.role == "assistant" => Some(message.as_ref()),
            _ => None,
        })
}

/// `totalSessionCost` from reference/packages/opencode/src/acp/usage.ts.
pub fn total_session_cost(messages: &[SessionMessage]) -> f64 {
    messages
        .iter()
        .filter_map(|message| match &message.info {
            Message::Assistant(message) if message.role == "assistant" => Some(message.cost),
            _ => None,
        })
        .sum()
}

/// `findContextLimit` from reference/packages/opencode/src/acp/usage.ts.
pub fn find_context_limit(
    providers: &IndexMap<String, ProviderInfo>,
    provider_id: &str,
    model_id: &str,
) -> Option<f64> {
    providers
        .get(provider_id)
        .and_then(|provider| provider.models.get(model_id))
        .and_then(|model| model.limit)
        .map(|limit| limit.context)
}

/// The usage service.
pub struct Service {
    message_loader: Arc<dyn MessageLoader>,
    context_limit_loader: Arc<dyn ContextLimitLoader>,
    limits: Mutex<HashMap<String, SharedLimit>>,
}

type SharedLimit = std::sync::Arc<tokio::sync::Mutex<Option<Option<f64>>>>;

impl Service {
    /// Build a usage service backed by the SDK, mirroring `makeUsageService` in
    /// reference/packages/opencode/src/acp/service.ts.
    pub fn make(sdk: Arc<dyn OpencodeClient>) -> Arc<Self> {
        let message_loader = Arc::new(SdkMessageLoader { sdk: sdk.clone() });
        let context_limit_loader = Arc::new(SdkContextLimitLoader { sdk });
        Arc::new(Self {
            message_loader,
            context_limit_loader,
            limits: Mutex::new(HashMap::new()),
        })
    }

    /// `contextLimit` from reference/packages/opencode/src/acp/usage.ts. Results
    /// are cached per (directory, provider, model).
    pub async fn context_limit(
        &self,
        directory: &str,
        provider_id: &str,
        model_id: &str,
    ) -> Option<f64> {
        let key = format!("{directory}\u{0}{provider_id}\u{0}{model_id}");
        let shared = {
            let mut limits = self.limits.lock().await;
            limits.get(&key).cloned().unwrap_or_else(|| {
                let shared = std::sync::Arc::new(tokio::sync::Mutex::new(None));
                limits.insert(key, shared.clone());
                shared
            })
        };
        if let Some(result) = *shared.lock().await {
            return result;
        }
        let providers = self
            .context_limit_loader
            .providers(directory)
            .await
            .map_err(|error| {
                tracing::error!("failed to get providers for usage context limit: {error:?}")
            })
            .ok();
        let result =
            providers.and_then(|providers| find_context_limit(&providers, provider_id, model_id));
        *shared.lock().await = Some(result);
        result
    }

    /// `sendUpdate` from reference/packages/opencode/src/acp/usage.ts.
    pub async fn send_update(
        &self,
        connection: &dyn AgentSideConnection,
        session_id: &str,
        directory: &str,
    ) {
        let messages = self
            .message_loader
            .messages(MessagesInput {
                session_id: session_id.to_string(),
                directory: directory.to_string(),
            })
            .await
            .map_err(|error| {
                tracing::error!("failed to fetch messages for usage update: {error:?}")
            })
            .ok();
        let Some(messages) = messages else {
            return;
        };

        let Some(message) = latest_assistant_message(&messages) else {
            return;
        };
        if message.provider_id.is_empty() || message.model_id.is_empty() {
            return;
        }
        let provider_id = message.provider_id.as_str();
        let model_id = message.model_id.as_str();

        let size = self.context_limit(directory, provider_id, model_id).await;
        let Some(size) = size else {
            return;
        };

        let _ = connection
            .session_update(
                session_id,
                SessionUpdate::UsageUpdate(UsageUpdate {
                    used: message.tokens.input + message.tokens.cache.read,
                    size: size as u64,
                    cost: Some(Cost {
                        amount: total_session_cost(&messages),
                        currency: "USD".into(),
                    }),
                }),
            )
            .await;
    }
}

struct SdkContextLimitLoader {
    sdk: Arc<dyn OpencodeClient>,
}

#[async_trait]
impl ContextLimitLoader for SdkContextLimitLoader {
    async fn providers(&self, directory: &str) -> Result<IndexMap<String, ProviderInfo>, SdkError> {
        let providers = self.sdk.config_providers(directory).await?;
        Ok(providers
            .providers
            .into_iter()
            .map(|provider| (provider.id.clone(), provider))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::Tokens;

    fn assistant(cost: f64, tokens: Tokens) -> AssistantMessage {
        AssistantMessage {
            id: "m1".into(),
            session_id: "s1".into(),
            role: "assistant".into(),
            provider_id: "anthropic".into(),
            model_id: "claude".into(),
            mode: Some("build".into()),
            agent: Some("build".into()),
            cost,
            tokens,
            variant: None,
            error: None,
            path: None,
            model: None,
        }
    }

    #[test]
    fn build_usage_omits_zero() {
        let message = assistant(
            1.0,
            Tokens {
                input: 10,
                output: 20,
                reasoning: 0,
                cache: crate::sdk::CacheTokens { read: 0, write: 5 },
            },
        );
        let usage = build_usage(&message);
        assert_eq!(
            serde_json::to_value(usage).unwrap(),
            serde_json::json!({
                "inputTokens": 10,
                "outputTokens": 20,
                "totalTokens": 35,
                "cachedWriteTokens": 5
            })
        );
    }

    #[test]
    fn latest_assistant_and_cost() {
        let user = Message::User(crate::sdk::UserMessage {
            id: "u1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            model: None,
            agent: None,
        });
        let messages = vec![
            SessionMessage { info: user },
            SessionMessage {
                info: Message::Assistant(Box::new(assistant(2.0, Tokens::default()))),
            },
            SessionMessage {
                info: Message::Assistant(Box::new(assistant(3.0, Tokens::default()))),
            },
        ];
        let latest = latest_assistant_message(&messages).unwrap();
        assert_eq!(latest.cost, 3.0);
        assert_eq!(total_session_cost(&messages), 5.0);
    }
}
