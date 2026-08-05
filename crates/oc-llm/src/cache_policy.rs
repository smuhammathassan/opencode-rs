//! Prompt-cache policy: inject `CacheHint`s onto designated parts.
//! From reference/packages/llm/src/cache-policy.ts

use crate::schema::messages::{ContentPart, Message, ToolDefinition};
use crate::schema::options::{
    mark_last_message_content, CachePolicy, CachePolicyMessages, CachePolicyObject,
};
use crate::schema::{CacheHint, LlmRequest};

const AUTO: CachePolicyObject = CachePolicyObject {
    tools: Some(true),
    system: Some(true),
    messages: Some(CachePolicyMessages::LatestUserMessage),
    ttl_seconds: None,
};

const NONE: CachePolicyObject = CachePolicyObject {
    tools: None,
    system: None,
    messages: None,
    ttl_seconds: None,
};

const RESPECTS_INLINE_HINTS: &[&str] = &["anthropic-messages", "bedrock-converse"];

/// `resolve(policy)`.
/// From reference/packages/llm/src/cache-policy.ts
fn resolve(policy: Option<&CachePolicy>) -> CachePolicyObject {
    match policy {
        None | Some(CachePolicy::Auto) => AUTO,
        Some(CachePolicy::None) => NONE,
        Some(CachePolicy::Object(object)) => object.clone(),
    }
}

fn make_hint(ttl_seconds: Option<u64>) -> CacheHint {
    CacheHint::ephemeral(ttl_seconds)
}

fn mark_last_tool(tools: &[ToolDefinition], hint: CacheHint) -> Vec<ToolDefinition> {
    if tools.is_empty() {
        return tools.to_vec();
    }
    let last = tools.len() - 1;
    if tools[last].cache.is_some() {
        return tools.to_vec();
    }
    tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            if i == last {
                let mut next = tool.clone();
                next.cache = Some(hint.clone());
                next
            } else {
                tool.clone()
            }
        })
        .collect()
}

fn mark_last_system(
    system: &[crate::schema::SystemPart],
    hint: CacheHint,
) -> Vec<crate::schema::SystemPart> {
    if system.is_empty() {
        return system.to_vec();
    }
    let last = system.len() - 1;
    if system[last].cache.is_some() {
        return system.to_vec();
    }
    system
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i == last {
                let mut next = part.clone();
                next.cache = Some(hint.clone());
                next
            } else {
                part.clone()
            }
        })
        .collect()
}

fn last_index_of_role(messages: &[Message], role: crate::schema::MessageRole) -> Option<usize> {
    messages.iter().rposition(|m| m.role == role)
}

fn mark_messages(
    messages: &[Message],
    strategy: &CachePolicyMessages,
    hint: CacheHint,
) -> Vec<Message> {
    if messages.is_empty() {
        return messages.to_vec();
    }
    match strategy {
        CachePolicyMessages::LatestUserMessage => {
            let index = last_index_of_role(messages, crate::schema::MessageRole::User);
            match index {
                Some(index) => mark_last_message_content(messages, index, hint),
                None => messages.to_vec(),
            }
        }
        CachePolicyMessages::LatestAssistant => {
            let index = last_index_of_role(messages, crate::schema::MessageRole::Assistant);
            match index {
                Some(index) => mark_last_message_content(messages, index, hint),
                None => messages.to_vec(),
            }
        }
        CachePolicyMessages::Tail { tail } => {
            let start = messages.len().saturating_sub(*tail);
            let mut next = messages.to_vec();
            for i in start..messages.len() {
                next = mark_last_message_content(&next, i, hint.clone());
            }
            next
        }
    }
}

/// `applyCachePolicy(request)`.
/// From reference/packages/llm/src/cache-policy.ts (`applyCachePolicy`)
pub fn apply_cache_policy(request: &LlmRequest) -> LlmRequest {
    if !RESPECTS_INLINE_HINTS.contains(&request.model.route.protocol.id.as_str()) {
        return request.clone();
    }
    let policy = resolve(request.cache.as_ref());
    if policy.tools != Some(true) && policy.system != Some(true) && policy.messages.is_none() {
        return request.clone();
    }

    let hint = make_hint(policy.ttl_seconds);
    let tools = match policy.tools {
        Some(true) => mark_last_tool(&request.tools, hint.clone()),
        _ => request.tools.clone(),
    };
    let system = match policy.system {
        Some(true) => mark_last_system(&request.system, hint.clone()),
        _ => request.system.clone(),
    };
    let messages = match &policy.messages {
        Some(strategy) => mark_messages(&request.messages, strategy, hint),
        None => request.messages.clone(),
    };

    if tools == request.tools && system == request.system && messages == request.messages {
        return request.clone();
    }
    let mut patch = crate::schema::LlmRequestPatch::empty();
    patch.tools = Some(tools);
    patch.system = Some(system);
    patch.messages = Some(messages);
    LlmRequest::update(request, patch)
}

#[allow(unused)]
fn _content_part_marker(part: &ContentPart) {
    let _ = part;
}
