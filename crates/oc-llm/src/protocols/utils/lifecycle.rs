//! Lifecycle helper: step/text/reasoning block event emission.
//! From reference/packages/llm/src/protocols/utils/lifecycle.ts

use std::collections::BTreeSet;

use crate::schema::{FinishReason, LlmEvent, ProviderMetadata, Usage};

/// `Lifecycle.State`.
/// From reference/packages/llm/src/protocols/utils/lifecycle.ts
#[derive(Debug, Clone, Default)]
pub struct State {
    pub step_started: bool,
    pub text: BTreeSet<String>,
    pub reasoning: BTreeSet<String>,
}

/// `Lifecycle.initial()`.
pub fn initial() -> State {
    State::default()
}

/// `Lifecycle.stepStart(state, events)`.
pub fn step_start(state: &State, events: &mut Vec<LlmEvent>) -> State {
    if state.step_started {
        return state.clone();
    }
    events.push(LlmEvent::StepStart { index: 0 });
    State {
        step_started: true,
        ..state.clone()
    }
}

/// `Lifecycle.textDelta(state, events, id, text)`.
pub fn text_delta(state: &State, events: &mut Vec<LlmEvent>, id: &str, text: &str) -> State {
    let stepped = step_start(state, events);
    if stepped.text.contains(id) {
        events.push(LlmEvent::TextDelta {
            id: id.to_string(),
            text: text.to_string(),
            provider_metadata: None,
        });
        return stepped;
    }
    events.push(LlmEvent::TextStart {
        id: id.to_string(),
        provider_metadata: None,
    });
    events.push(LlmEvent::TextDelta {
        id: id.to_string(),
        text: text.to_string(),
        provider_metadata: None,
    });
    let mut text_set = stepped.text.clone();
    text_set.insert(id.to_string());
    State {
        text: text_set,
        ..stepped
    }
}

/// `Lifecycle.reasoningStart(state, events, id, providerMetadata)`.
pub fn reasoning_start(
    state: &State,
    events: &mut Vec<LlmEvent>,
    id: &str,
    provider_metadata: Option<&ProviderMetadata>,
) -> State {
    if state.reasoning.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LlmEvent::ReasoningStart {
        id: id.to_string(),
        provider_metadata: provider_metadata.cloned(),
    });
    let mut reasoning = stepped.reasoning.clone();
    reasoning.insert(id.to_string());
    State {
        reasoning,
        ..stepped
    }
}

/// `Lifecycle.reasoningDelta(...)`.
pub fn reasoning_delta(
    state: &State,
    events: &mut Vec<LlmEvent>,
    id: &str,
    text: &str,
    provider_metadata: Option<&ProviderMetadata>,
) -> State {
    let started = reasoning_start(state, events, id, provider_metadata);
    events.push(LlmEvent::ReasoningDelta {
        id: id.to_string(),
        text: text.to_string(),
        provider_metadata: provider_metadata.cloned(),
    });
    started
}

/// `Lifecycle.reasoningEnd(...)`.
pub fn reasoning_end(
    state: &State,
    events: &mut Vec<LlmEvent>,
    id: &str,
    provider_metadata: Option<&ProviderMetadata>,
) -> State {
    if !state.reasoning.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LlmEvent::ReasoningEnd {
        id: id.to_string(),
        provider_metadata: provider_metadata.cloned(),
    });
    let mut reasoning = stepped.reasoning.clone();
    reasoning.remove(id);
    State {
        reasoning,
        ..stepped
    }
}

/// `Lifecycle.textEnd(...)`.
pub fn text_end(
    state: &State,
    events: &mut Vec<LlmEvent>,
    id: &str,
    provider_metadata: Option<&ProviderMetadata>,
) -> State {
    if !state.text.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LlmEvent::TextEnd {
        id: id.to_string(),
        provider_metadata: provider_metadata.cloned(),
    });
    let mut text = stepped.text.clone();
    text.remove(id);
    State { text, ..stepped }
}

fn close_open_blocks(state: &State, events: &mut Vec<LlmEvent>) -> State {
    let mut next = state.clone();
    for id in &state.reasoning {
        events.push(LlmEvent::ReasoningEnd {
            id: id.clone(),
            provider_metadata: None,
        });
    }
    for id in &state.text {
        events.push(LlmEvent::TextEnd {
            id: id.clone(),
            provider_metadata: None,
        });
    }
    next.reasoning.clear();
    next.text.clear();
    next
}

/// `Lifecycle.finish(state, events, input)`.
pub fn finish(
    state: &State,
    events: &mut Vec<LlmEvent>,
    reason: FinishReason,
    usage: Option<&Usage>,
    provider_metadata: Option<&ProviderMetadata>,
) -> State {
    let stepped = close_open_blocks(&step_start(state, events), events);
    events.push(LlmEvent::StepFinish {
        index: 0,
        reason,
        usage: usage.cloned(),
        provider_metadata: provider_metadata.cloned(),
    });
    events.push(LlmEvent::Finish {
        reason,
        usage: usage.cloned(),
        provider_metadata: provider_metadata.cloned(),
    });
    State {
        step_started: false,
        ..stepped
    }
}

#[allow(unused)]
fn _marker(_: &BTreeSet<String>) {}
