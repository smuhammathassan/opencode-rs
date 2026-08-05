//! Route for OpenAI Chat-compatible providers (no canonical URL).
//! From reference/packages/llm/src/protocols/openai-compatible-chat.ts

use super::openai_chat;
use crate::route::Route;

pub const ADAPTER: &str = "openai-compatible-chat";
pub const PATH: &str = "/chat/completions";

/// `OpenAICompatibleChat.route`.
/// From reference/packages/llm/src/protocols/openai-compatible-chat.ts (`route`)
pub fn route() -> Route {
    Route::make(crate::route::RouteMakeInput {
        id: ADAPTER.to_string(),
        provider: None,
        protocol: openai_chat::protocol(),
        endpoint: crate::route::endpoint::path(PATH, crate::route::EndpointOptions::none()),
        auth: None,
        framing: Some(crate::route::Framing::Sse),
        headers: None,
        defaults: None,
    })
}
