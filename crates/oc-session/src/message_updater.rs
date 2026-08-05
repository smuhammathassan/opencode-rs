/// From reference/packages/core/src/session/message-updater.ts
///
/// In-memory message projection: a [`MemoryState`] holding V2 messages, an
/// [`Adapter`] that reads/updates it, and the [`update`] reducer that applies
/// session events to the adapter.
use crate::v2::{Assistant, AssistantContent, AssistantTool, EventData, Message, Shell};

#[derive(Debug, Default, Clone)]
pub struct MemoryState {
    pub messages: Vec<Message>,
}

pub trait Adapter {
    fn get_current_assistant(&self) -> Option<Assistant>;
    fn get_assistant(&self, message_id: &str) -> Option<Assistant>;
    fn get_current_shell(&self, call_id: &str) -> Option<Shell>;
    fn update_assistant(&mut self, assistant: Assistant);
    fn update_shell(&mut self, shell: Shell);
    fn append_message(&mut self, message: Message);
}

/// From reference `message-updater.ts:memory`.
pub struct Memory<'a> {
    pub state: &'a mut MemoryState,
}

impl<'a> Adapter for Memory<'a> {
    fn get_current_assistant(&self) -> Option<Assistant> {
        let index = self
            .state
            .messages
            .iter()
            .rposition(|message| matches!(message, Message::Assistant(_)))?;
        match &self.state.messages[index] {
            Message::Assistant(assistant) if assistant.time.completed.is_none() => {
                Some(assistant.as_ref().clone())
            }
            _ => None,
        }
    }

    fn get_assistant(&self, message_id: &str) -> Option<Assistant> {
        let index = self
            .state
            .messages
            .iter()
            .rposition(|message| message.id() == message_id)?;
        match &self.state.messages[index] {
            Message::Assistant(assistant) => Some(assistant.as_ref().clone()),
            _ => None,
        }
    }

    fn get_current_shell(&self, call_id: &str) -> Option<Shell> {
        let index = self.state.messages.iter().rposition(
            |message| matches!(message, Message::Shell(shell) if shell.call_id == call_id),
        )?;
        match &self.state.messages[index] {
            Message::Shell(shell) => Some(shell.as_ref().clone()),
            _ => None,
        }
    }

    fn update_assistant(&mut self, assistant: Assistant) {
        let index = self
            .state
            .messages
            .iter()
            .rposition(|message| message.id() == assistant.base.id)
            .expect("assistant exists");
        self.state.messages[index] = Message::Assistant(Box::new(assistant));
    }

    fn update_shell(&mut self, shell: Shell) {
        let index = self
            .state
            .messages
            .iter()
            .rposition(|message| matches!(message, Message::Shell(s) if s.call_id == shell.call_id))
            .expect("shell exists");
        self.state.messages[index] = Message::Shell(Box::new(shell));
    }

    fn append_message(&mut self, message: Message) {
        self.state.messages.push(message);
    }
}

/// From reference `message-updater.ts:update` — apply an event to the adapter.
pub fn update(adapter: &mut dyn Adapter, event: &EventData) {
    match event {
        EventData::AgentSwitched {
            message_id,
            timestamp,
            agent,
            ..
        } => {
            adapter.append_message(Message::AgentSwitched(crate::v2::AgentSwitched {
                base: base(*timestamp, message_id, None),
                type_: "agent-switched".into(),
                agent: agent.clone(),
            }));
        }
        EventData::ModelSwitched {
            message_id,
            timestamp,
            model,
            ..
        } => {
            adapter.append_message(Message::ModelSwitched(crate::v2::ModelSwitched {
                base: base(*timestamp, message_id, None),
                type_: "model-switched".into(),
                model: model.clone(),
            }));
        }
        EventData::Prompted {
            message_id,
            timestamp,
            prompt,
            ..
        } => {
            adapter.append_message(Message::User(crate::v2::User {
                base: base(*timestamp, message_id, None),
                text: prompt.text.clone(),
                files: prompt.files.clone(),
                agents: prompt.agents.clone(),
                type_: "user".into(),
            }));
        }
        EventData::ContextUpdated {
            message_id,
            timestamp,
            text,
            ..
        } => {
            adapter.append_message(Message::System(crate::v2::System {
                base: base(*timestamp, message_id, None),
                type_: "system".into(),
                text: text.clone(),
            }));
        }
        EventData::Synthetic {
            message_id,
            timestamp,
            session_id,
            text,
            ..
        } => {
            adapter.append_message(Message::Synthetic(crate::v2::Synthetic {
                base: base(*timestamp, message_id, None),
                session_id: session_id.clone(),
                text: text.clone(),
                type_: "synthetic".into(),
            }));
        }
        EventData::ShellStarted {
            message_id,
            timestamp,
            call_id,
            command,
            ..
        } => {
            adapter.append_message(Message::Shell(Box::new(Shell {
                base: crate::v2::MessageBaseId {
                    id: message_id.clone(),
                    metadata: None,
                },
                type_: "shell".into(),
                call_id: call_id.clone(),
                command: command.clone(),
                output: String::new(),
                time: crate::v2::ShellTime {
                    created: *timestamp,
                    completed: None,
                },
            })));
        }
        EventData::ShellEnded {
            call_id,
            timestamp,
            output,
            ..
        } => {
            if let Some(current) = adapter.get_current_shell(call_id) {
                let mut next = current;
                next.output = output.clone();
                next.time.completed = Some(*timestamp);
                adapter.update_shell(next);
            }
        }
        EventData::StepStarted {
            assistant_message_id,
            timestamp,
            agent,
            model,
            snapshot,
            ..
        } => {
            if let Some(current) = adapter.get_current_assistant() {
                let mut next = current;
                next.time.completed = Some(*timestamp);
                adapter.update_assistant(next);
            }
            adapter.append_message(Message::Assistant(Box::new(Assistant {
                base: crate::v2::MessageBaseId {
                    id: assistant_message_id.clone(),
                    metadata: None,
                },
                type_: "assistant".into(),
                agent: agent.clone(),
                model: model.clone(),
                content: Vec::new(),
                snapshot: snapshot.clone().map(|start| crate::v2::AssistantSnapshot {
                    start: Some(start),
                    end: None,
                    files: None,
                }),
                finish: None,
                cost: None,
                tokens: None,
                error: None,
                time: crate::v2::AssistantTime {
                    created: *timestamp,
                    completed: None,
                },
            })));
        }
        EventData::StepEnded {
            assistant_message_id,
            timestamp,
            finish,
            cost,
            tokens,
            snapshot,
            files,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                draft.time.completed = Some(*timestamp);
                draft.finish = Some(finish.clone());
                draft.cost = Some(*cost);
                draft.tokens = Some(tokens.clone());
                if snapshot.is_some() || files.is_some() {
                    let current = draft.snapshot.clone().unwrap_or_default();
                    draft.snapshot = Some(crate::v2::AssistantSnapshot {
                        start: current.start,
                        end: snapshot.clone(),
                        files: files.clone(),
                    });
                }
            });
        }
        EventData::StepFailed {
            assistant_message_id,
            timestamp,
            error,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                draft.time.completed = Some(*timestamp);
                draft.finish = Some("error".into());
                draft.error = Some(error.clone());
            });
        }
        EventData::TextStarted {
            assistant_message_id,
            text_id,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                draft
                    .content
                    .push(AssistantContent::Text(crate::v2::AssistantText {
                        type_: "text".into(),
                        id: text_id.clone(),
                        text: String::new(),
                    }));
            });
        }
        EventData::TextDelta {
            assistant_message_id,
            text_id,
            delta,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(text) = draft.content.iter_mut().rev().find_map(|part| match part {
                    AssistantContent::Text(text) if text.id == *text_id => Some(text),
                    _ => None,
                }) {
                    text.text.push_str(delta);
                }
            });
        }
        EventData::TextEnded {
            assistant_message_id,
            text_id,
            text,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(match_text) = draft.content.iter_mut().rev().find_map(|part| match part
                {
                    AssistantContent::Text(text) if text.id == *text_id => Some(text),
                    _ => None,
                }) {
                    match_text.text = text.clone();
                }
            });
        }
        EventData::ToolInputStarted {
            assistant_message_id,
            call_id,
            name,
            timestamp,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                draft
                    .content
                    .push(AssistantContent::Tool(Box::new(AssistantTool {
                        type_: "tool".into(),
                        id: call_id.clone(),
                        name: name.clone(),
                        provider: None,
                        state: crate::v2::ToolState::Pending(crate::v2::ToolStatePending {
                            status: "pending".into(),
                            input: String::new(),
                        }),
                        time: crate::v2::AssistantToolTime {
                            created: *timestamp,
                            ran: None,
                            completed: None,
                            pruned: None,
                        },
                    })));
            });
        }
        EventData::ToolInputEnded {
            assistant_message_id,
            call_id,
            text,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                let tool = latest_tool(draft, Some(call_id));
                if let Some(tool) = tool {
                    if let crate::v2::ToolState::Pending(_pending) = &tool.state {
                        tool.state = crate::v2::ToolState::Pending(crate::v2::ToolStatePending {
                            status: "pending".into(),
                            input: text.clone(),
                        });
                    }
                }
            });
        }
        EventData::ToolCalled {
            assistant_message_id,
            call_id,
            timestamp,
            input,
            provider,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(tool) = latest_tool(draft, Some(call_id)) {
                    tool.provider = Some(crate::v2::ProviderInfo {
                        executed: provider.executed,
                        metadata: provider.metadata.clone(),
                        result_metadata: None,
                    });
                    tool.time.ran = Some(*timestamp);
                    tool.state = crate::v2::ToolState::Running(crate::v2::ToolStateRunning {
                        status: "running".into(),
                        input: input.clone(),
                        structured: crate::JsonMap::new(),
                        content: Vec::new(),
                    });
                }
            });
        }
        EventData::ToolProgress {
            assistant_message_id,
            call_id,
            structured,
            content,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(tool) = latest_tool(draft, Some(call_id)) {
                    if let crate::v2::ToolState::Running(running) = &tool.state {
                        tool.state = crate::v2::ToolState::Running(crate::v2::ToolStateRunning {
                            status: "running".into(),
                            input: running.input.clone(),
                            structured: structured.clone(),
                            content: content.clone(),
                        });
                    }
                }
            });
        }
        EventData::ToolSuccess {
            assistant_message_id,
            call_id,
            timestamp,
            structured,
            content,
            output_paths,
            result,
            provider,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(tool) = latest_tool(draft, Some(call_id)) {
                    if let crate::v2::ToolState::Running(running) = &tool.state {
                        tool.provider = Some(crate::v2::ProviderInfo {
                            executed: provider.executed
                                || tool.provider.as_ref().is_some_and(|p| p.executed),
                            metadata: tool.provider.as_ref().and_then(|p| p.metadata.clone()),
                            result_metadata: provider.metadata.clone(),
                        });
                        tool.time.completed = Some(*timestamp);
                        tool.state =
                            crate::v2::ToolState::Completed(crate::v2::ToolStateCompleted {
                                status: "completed".into(),
                                input: running.input.clone(),
                                attachments: None,
                                content: content.clone(),
                                output_paths: output_paths.clone(),
                                structured: structured.clone(),
                                result: result.clone(),
                            });
                    }
                }
            });
        }
        EventData::ToolFailed {
            assistant_message_id,
            call_id,
            timestamp,
            error,
            result,
            provider,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(tool) = latest_tool(draft, Some(call_id)) {
                    let (input, structured, content) = match &tool.state {
                        crate::v2::ToolState::Pending(_pending) => {
                            (crate::JsonMap::new(), crate::JsonMap::new(), Vec::new())
                        }
                        crate::v2::ToolState::Running(running) => (
                            running.input.clone(),
                            running.structured.clone(),
                            running.content.clone(),
                        ),
                        _ => (crate::JsonMap::new(), crate::JsonMap::new(), Vec::new()),
                    };
                    tool.provider = Some(crate::v2::ProviderInfo {
                        executed: provider.executed
                            || tool.provider.as_ref().is_some_and(|p| p.executed),
                        metadata: tool.provider.as_ref().and_then(|p| p.metadata.clone()),
                        result_metadata: provider.metadata.clone(),
                    });
                    tool.time.completed = Some(*timestamp);
                    tool.state = crate::v2::ToolState::Error(crate::v2::ToolStateError {
                        status: "error".into(),
                        error: error.clone(),
                        input,
                        structured,
                        content,
                        result: result.clone(),
                    });
                }
            });
        }
        EventData::ReasoningStarted {
            assistant_message_id,
            reasoning_id,
            timestamp,
            provider_metadata,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                draft
                    .content
                    .push(AssistantContent::Reasoning(crate::v2::AssistantReasoning {
                        type_: "reasoning".into(),
                        id: reasoning_id.clone(),
                        text: String::new(),
                        provider_metadata: provider_metadata.clone(),
                        time: Some(crate::v2::AssistantReasoningTime {
                            created: *timestamp,
                            completed: None,
                        }),
                    }));
            });
        }
        EventData::ReasoningDelta {
            assistant_message_id,
            reasoning_id,
            delta,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(reasoning) =
                    draft.content.iter_mut().rev().find_map(|part| match part {
                        AssistantContent::Reasoning(reasoning) if reasoning.id == *reasoning_id => {
                            Some(reasoning)
                        }
                        _ => None,
                    })
                {
                    reasoning.text.push_str(delta);
                }
            });
        }
        EventData::ReasoningEnded {
            assistant_message_id,
            reasoning_id,
            timestamp,
            text,
            provider_metadata,
            ..
        } => {
            update_owned_assistant(adapter, assistant_message_id, |draft| {
                if let Some(reasoning) =
                    draft.content.iter_mut().rev().find_map(|part| match part {
                        AssistantContent::Reasoning(reasoning) if reasoning.id == *reasoning_id => {
                            Some(reasoning)
                        }
                        _ => None,
                    })
                {
                    reasoning.text = text.clone();
                    reasoning.time = Some(crate::v2::AssistantReasoningTime {
                        created: reasoning
                            .time
                            .clone()
                            .map(|t| t.created)
                            .unwrap_or(*timestamp),
                        completed: Some(*timestamp),
                    });
                    if provider_metadata.is_some() {
                        reasoning.provider_metadata = provider_metadata.clone();
                    }
                }
            });
        }
        EventData::CompactionEnded {
            message_id,
            timestamp,
            reason,
            text,
            recent,
            ..
        } => {
            adapter.append_message(Message::Compaction(crate::v2::Compaction {
                type_: "compaction".into(),
                reason: reason.clone(),
                summary: text.clone(),
                recent: recent.clone(),
                base: base(*timestamp, message_id, None),
            }));
        }
        // Live-only / no-op events.
        EventData::Moved { .. }
        | EventData::PromptAdmitted { .. }
        | EventData::ToolInputDelta { .. }
        | EventData::Retried { .. }
        | EventData::CompactionStarted { .. }
        | EventData::CompactionDelta { .. }
        | EventData::RevertStaged { .. }
        | EventData::RevertCleared { .. }
        | EventData::RevertCommitted { .. } => {}
    }
}

fn base(
    timestamp: u64,
    message_id: &str,
    metadata: Option<crate::JsonMap>,
) -> crate::v2::MessageBase {
    crate::v2::MessageBase {
        id: message_id.to_string(),
        metadata,
        time: crate::v2::MessageTime { created: timestamp },
    }
}

fn latest_tool<'a>(
    assistant: &'a mut Assistant,
    call_id: Option<&str>,
) -> Option<&'a mut AssistantTool> {
    assistant
        .content
        .iter_mut()
        .rev()
        .find_map(|part| match part {
            AssistantContent::Tool(tool) => {
                if call_id.is_none_or(|id| tool.id == id) {
                    Some(tool.as_mut())
                } else {
                    None
                }
            }
            _ => None,
        })
}

fn update_owned_assistant(
    adapter: &mut dyn Adapter,
    message_id: &str,
    recipe: impl FnOnce(&mut Assistant),
) {
    if let Some(mut assistant) = adapter.get_assistant(message_id) {
        recipe(&mut assistant);
        adapter.update_assistant(assistant);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::{EventData, ModelRef};

    #[test]
    fn prompted_appends_user_message() {
        let mut state = MemoryState::default();
        let mut memory = Memory { state: &mut state };
        update(
            &mut memory,
            &EventData::Prompted {
                timestamp: 1000,
                session_id: "ses1".into(),
                message_id: "msg_1".into(),
                prompt: crate::v2::Prompt {
                    text: "hello".into(),
                    files: None,
                    agents: None,
                },
                delivery: "steer".into(),
            },
        );
        assert_eq!(state.messages.len(), 1);
        let Message::User(user) = &state.messages[0] else {
            panic!("expected user")
        };
        assert_eq!(user.text, "hello");
    }

    #[test]
    fn step_started_then_text_delta_accumulates() {
        let mut state = MemoryState::default();
        let mut memory = Memory { state: &mut state };
        update(
            &mut memory,
            &EventData::StepStarted {
                timestamp: 1000,
                session_id: "ses1".into(),
                assistant_message_id: "msg_1".into(),
                agent: "primary".into(),
                model: ModelRef {
                    id: "gpt-4o".into(),
                    provider_id: "openai".into(),
                    variant: None,
                },
                snapshot: None,
            },
        );
        update(
            &mut memory,
            &EventData::TextStarted {
                timestamp: 1001,
                session_id: "ses1".into(),
                assistant_message_id: "msg_1".into(),
                text_id: "t1".into(),
            },
        );
        update(
            &mut memory,
            &EventData::TextDelta {
                timestamp: 1002,
                session_id: "ses1".into(),
                assistant_message_id: "msg_1".into(),
                text_id: "t1".into(),
                delta: "Hel".into(),
            },
        );
        update(
            &mut memory,
            &EventData::TextDelta {
                timestamp: 1003,
                session_id: "ses1".into(),
                assistant_message_id: "msg_1".into(),
                text_id: "t1".into(),
                delta: "lo".into(),
            },
        );
        let Message::Assistant(assistant) = &state.messages[0] else {
            panic!("expected assistant")
        };
        match &assistant.content[0] {
            AssistantContent::Text(text) => assert_eq!(text.text, "Hello"),
            _ => panic!("expected text"),
        }
    }
}
