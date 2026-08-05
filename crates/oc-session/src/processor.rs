/// From reference/packages/opencode/src/session/processor.ts
///
/// The part processor: turns an `LLMEvent` stream into persisted parts for
/// the current assistant message. `handle_event` mirrors the reference state
/// machine; the surrounding service interactions are provided by
/// [`ProcessorDeps`].
use std::collections::HashMap;

use crate::llm::LLMEvent;
use crate::provider::ProviderModel;
use crate::v1::{Assistant, Part, PartBase, ReasoningPart, TextPart, ToolPart, ToolState};
use crate::JsonMap;

pub const DOOM_LOOP_THRESHOLD: usize = 3;

pub type Result_ = Result<ProcessResult, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessResult {
    Compact,
    Stop,
    Continue,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub done: bool,
}

#[derive(Debug, Clone)]
pub struct Patch {
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigSnapshot {
    pub compaction_auto: Option<bool>,
    pub continue_loop_on_deny: Option<bool>,
}

/// Service seam for the processor.
///
/// TODO(integration): implement against oc-session-runner / oc-server once the
/// store lands.
pub trait ProcessorDeps {
    fn update_part(&self, part: &Part) -> Result<Part, String>;
    fn update_part_delta(
        &self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        field: &str,
        delta: &str,
    ) -> Result<(), String>;
    fn get_part(
        &self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
    ) -> Result<Option<Part>, String>;
    fn update_message(&self, message: &Assistant) -> Result<(), String>;
    fn snapshot_track(&self) -> Result<String, String>;
    fn snapshot_patch(&self, snapshot: &str) -> Result<Patch, String>;
    fn agent_permission(&self, name: &str) -> Option<crate::v1::Ruleset>;
    fn ask_permission(
        &self,
        permission: &str,
        patterns: &[String],
        session_id: &str,
        metadata: &JsonMap,
        always: &[String],
        ruleset: &crate::v1::Ruleset,
    ) -> Result<(), String>;
    fn stream(&self, input: &StreamInput) -> Result<Vec<LLMEvent>, String>;
    fn get_usage(&self, usage: &crate::llm::Usage) -> crate::session::UsageResult;
    fn summarize(&self, session_id: &str, message_id: &str) -> Result<(), String>;
    fn config(&self) -> ConfigSnapshot;
    fn is_overflow(&self, tokens: &crate::v1::AssistantTokens) -> bool;
    fn plugin_text_complete(
        &self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        text: &str,
    ) -> Result<String, String>;
    fn status_set(&self, session_id: &str, status: &crate::status::Info) -> Result<(), String>;
    fn image_normalize(&self, part: &crate::v1::FilePart) -> Result<crate::v1::FilePart, String>;
    fn now(&self) -> u64;
}

#[derive(Debug, Clone)]
pub struct StreamInput {
    pub model: ProviderModel,
    pub messages: Vec<crate::message_v2::ModelMessage>,
    pub system: Vec<String>,
    pub tools: Vec<String>,
    pub session_id: String,
    pub agent: String,
}

#[derive(Debug, Clone)]
pub struct ProcessorContext {
    pub assistant_message: Assistant,
    pub session_id: String,
    pub model: ProviderModel,
    pub toolcalls: HashMap<String, ToolCall>,
    pub should_break: bool,
    pub snapshot: Option<String>,
    pub blocked: bool,
    pub needs_compaction: bool,
    pub current_text: Option<TextPart>,
    pub reasoning_map: HashMap<String, ReasoningPart>,
}

pub struct Handle<'a, D: ProcessorDeps> {
    pub ctx: ProcessorContext,
    pub deps: &'a D,
}

impl<'a, D: ProcessorDeps> Handle<'a, D> {
    /// `SessionProcessor.create` — pre-capture snapshot and build the context.
    pub fn create(input: &HandleInput, deps: &'a D) -> Result<Self, String> {
        let initial_snapshot = deps.snapshot_track().ok();
        Ok(Handle {
            ctx: ProcessorContext {
                assistant_message: input.assistant_message.clone(),
                session_id: input.session_id.clone(),
                model: input.model.clone(),
                toolcalls: HashMap::new(),
                should_break: false,
                snapshot: initial_snapshot,
                blocked: false,
                needs_compaction: false,
                current_text: None,
                reasoning_map: HashMap::new(),
            },
            deps,
        })
    }

    fn read_tool_call(&mut self, tool_call_id: &str) -> Option<(ToolCall, ToolPart)> {
        let call = self.ctx.toolcalls.get(tool_call_id).cloned()?;
        let part = self
            .deps
            .get_part(&call.session_id, &call.message_id, &call.part_id)
            .ok()
            .flatten()?;
        match part {
            Part::Tool(tool) => Some((call, tool)),
            _ => {
                self.ctx.toolcalls.remove(tool_call_id);
                None
            }
        }
    }

    fn update_tool_call(
        &mut self,
        tool_call_id: &str,
        update: impl FnOnce(ToolPart) -> ToolPart,
    ) -> Result<Option<ToolPart>, String> {
        let Some((call, part)) = self.read_tool_call(tool_call_id) else {
            return Ok(None);
        };
        let part = self.deps.update_part(&Part::Tool(update(part)))?;
        let Part::Tool(part) = part else {
            unreachable!()
        };
        self.ctx.toolcalls.insert(
            tool_call_id.to_string(),
            ToolCall {
                part_id: part.base.id.clone(),
                message_id: part.base.message_id.clone(),
                session_id: part.base.session_id.clone(),
                done: call.done,
            },
        );
        Ok(Some(part))
    }

    fn complete_tool_call(
        &mut self,
        tool_call_id: &str,
        output: &ToolOutput,
    ) -> Result<(), String> {
        let Some((_, part)) = self.read_tool_call(tool_call_id) else {
            return Ok(());
        };
        if !matches!(part.state, ToolState::Running(_)) {
            return Ok(());
        }
        let start = match &part.state {
            ToolState::Running(running) => running.time.start,
            _ => 0,
        };
        self.deps.update_part(&Part::Tool(ToolPart {
            base: part.base.clone(),
            type_: "tool".into(),
            call_id: part.call_id.clone(),
            tool: part.tool.clone(),
            state: ToolState::Completed(crate::v1::ToolStateCompleted {
                status: "completed".into(),
                input: input_of(&part.state),
                output: output.output.clone(),
                metadata: output.metadata.clone(),
                title: output.title.clone(),
                time: crate::v1::CompletedTime {
                    start,
                    end: self.deps.now(),
                    compacted: None,
                },
                attachments: output.attachments.clone(),
            }),
            metadata: part.metadata.clone(),
        }))?;
        self.ctx.toolcalls.remove(tool_call_id);
        Ok(())
    }

    fn fail_tool_call(&mut self, tool_call_id: &str, error: &str) -> Result<bool, String> {
        let Some((_, part)) = self.read_tool_call(tool_call_id) else {
            return Ok(false);
        };
        if !matches!(part.state, ToolState::Running(_)) {
            return Ok(false);
        }
        let start = match &part.state {
            ToolState::Running(running) => running.time.start,
            _ => 0,
        };
        let metadata = match &part.state {
            ToolState::Running(running) => running.metadata.clone(),
            _ => None,
        };
        self.deps.update_part(&Part::Tool(ToolPart {
            base: part.base.clone(),
            type_: "tool".into(),
            call_id: part.call_id.clone(),
            tool: part.tool.clone(),
            state: ToolState::Error(crate::v1::ToolStateError {
                status: "error".into(),
                input: input_of(&part.state),
                error: error.to_string(),
                metadata,
                time: crate::v1::CompletedTime {
                    start,
                    end: self.deps.now(),
                    compacted: None,
                },
            }),
            metadata: part.metadata.clone(),
        }))?;
        self.ctx.toolcalls.remove(tool_call_id);
        Ok(true)
    }

    fn finish_reasoning(&mut self, reasoning_id: &str) -> Result<(), String> {
        let Some(mut reasoning) = self.ctx.reasoning_map.remove(reasoning_id) else {
            return Ok(());
        };
        let start = reasoning.time.start;
        reasoning.time = crate::v1::PartTime {
            start,
            end: Some(self.deps.now()),
        };
        self.deps.update_part(&Part::Reasoning(reasoning))?;
        Ok(())
    }

    fn ensure_tool_call(
        &mut self,
        id: &str,
        name: &str,
        provider_executed: bool,
    ) -> Result<(ToolCall, ToolPart), String> {
        if let Some((call, part)) = self.read_tool_call(id) {
            if !provider_executed
                || part
                    .metadata
                    .as_ref()
                    .is_some_and(|m| m.get("providerExecuted").is_some())
            {
                return Ok((call, part));
            }
            let mut metadata = part.metadata.clone().unwrap_or_default();
            metadata.insert("providerExecuted".into(), serde_json::Value::Bool(true));
            let updated = self.deps.update_part(&Part::Tool(ToolPart {
                metadata: Some(metadata),
                ..part
            }))?;
            let Part::Tool(updated) = updated else {
                unreachable!()
            };
            self.ctx.toolcalls.insert(
                id.to_string(),
                ToolCall {
                    part_id: updated.base.id.clone(),
                    message_id: updated.base.message_id.clone(),
                    session_id: updated.base.session_id.clone(),
                    done: call.done,
                },
            );
            return Ok((self.ctx.toolcalls[id].clone(), updated));
        }
        let part = self.deps.update_part(&Part::Tool(ToolPart {
            base: PartBase {
                id: crate::schema::create_part(None),
                session_id: self.ctx.assistant_message.session_id.clone(),
                message_id: self.ctx.assistant_message.id.clone(),
            },
            type_: "tool".into(),
            call_id: id.to_string(),
            tool: name.to_string(),
            state: ToolState::Pending(crate::v1::ToolStatePending {
                status: "pending".into(),
                input: JsonMap::new(),
                raw: String::new(),
            }),
            metadata: if provider_executed {
                Some(JsonMap::from_iter([(
                    "providerExecuted".into(),
                    serde_json::Value::Bool(true),
                )]))
            } else {
                None
            },
        }))?;
        let Part::Tool(part) = part else {
            unreachable!()
        };
        let call = ToolCall {
            part_id: part.base.id.clone(),
            message_id: part.base.message_id.clone(),
            session_id: part.base.session_id.clone(),
            done: false,
        };
        self.ctx.toolcalls.insert(id.to_string(), call.clone());
        Ok((call, part))
    }

    fn is_file_part(value: &serde_json::Value) -> bool {
        value.get("type").and_then(|v| v.as_str()) == Some("file")
    }

    /// `SessionProcessor.process` — consumes the event stream and returns the
    /// loop outcome.
    pub fn process(&mut self, input: &StreamInput) -> Result<ProcessResult, String> {
        self.ctx.needs_compaction = false;
        self.ctx.should_break = self.deps.config().continue_loop_on_deny != Some(true);
        self.deps
            .status_set(&self.ctx.session_id, &crate::status::Info::Busy)?;
        let events = self.deps.stream(input)?;
        for event in &events {
            self.handle_event(event)?;
            if self.ctx.needs_compaction {
                break;
            }
        }
        if self.ctx.needs_compaction {
            return Ok(ProcessResult::Compact);
        }
        if self.ctx.blocked || self.ctx.assistant_message.error.is_some() {
            return Ok(ProcessResult::Stop);
        }
        Ok(ProcessResult::Continue)
    }

    /// `SessionProcessor.handleEvent` — the per-event state machine.
    pub fn handle_event(&mut self, value: &LLMEvent) -> Result<(), String> {
        match value {
            LLMEvent::ReasoningStart {
                id,
                provider_metadata,
                ..
            } => {
                if self.ctx.reasoning_map.contains_key(id) {
                    return Ok(());
                }
                let reasoning = ReasoningPart {
                    base: PartBase {
                        id: crate::schema::create_part(None),
                        session_id: self.ctx.assistant_message.session_id.clone(),
                        message_id: self.ctx.assistant_message.id.clone(),
                    },
                    type_: "reasoning".into(),
                    text: String::new(),
                    metadata: provider_metadata.clone(),
                    time: crate::v1::PartTime {
                        start: self.deps.now(),
                        end: None,
                    },
                };
                self.deps.update_part(&Part::Reasoning(reasoning.clone()))?;
                self.ctx.reasoning_map.insert(id.clone(), reasoning);
                Ok(())
            }
            LLMEvent::ReasoningDelta {
                id,
                text,
                provider_metadata,
                ..
            } => {
                if !self.ctx.reasoning_map.contains_key(id) {
                    return Ok(());
                }
                if let Some(reasoning) = self.ctx.reasoning_map.get_mut(id) {
                    reasoning.text.push_str(text);
                    if provider_metadata.is_some() {
                        reasoning.metadata = provider_metadata.clone();
                    }
                    let session_id = reasoning.base.session_id.clone();
                    let message_id = reasoning.base.message_id.clone();
                    let part_id = reasoning.base.id.clone();
                    self.deps.update_part_delta(
                        &session_id,
                        &message_id,
                        &part_id,
                        "text",
                        text,
                    )?;
                }
                Ok(())
            }
            LLMEvent::ReasoningEnd {
                id,
                provider_metadata,
                ..
            } => {
                if provider_metadata.is_some() {
                    if let Some(reasoning) = self.ctx.reasoning_map.get_mut(id) {
                        reasoning.metadata = provider_metadata.clone();
                    }
                }
                self.finish_reasoning(id)
            }
            LLMEvent::ToolInputStart { id, name, .. } | LLMEvent::ToolInputEnd { id, name, .. } => {
                if self.ctx.assistant_message.summary.unwrap_or(false) {
                    return Err(format!(
                        "Tool call not allowed while generating summary: {name}"
                    ));
                }
                self.ensure_tool_call(id, name, false)?;
                Ok(())
            }
            LLMEvent::ToolInputDelta { id, name, .. } => {
                self.ensure_tool_call(id, name, false)?;
                Ok(())
            }
            LLMEvent::ToolCall {
                id,
                name,
                input,
                provider_executed,
                provider_metadata,
                ..
            } => {
                if self.ctx.assistant_message.summary.unwrap_or(false) {
                    return Err(format!(
                        "Tool call not allowed while generating summary: {name}"
                    ));
                }
                self.ensure_tool_call(id, name, provider_executed.unwrap_or(false))?;
                let input_map: JsonMap = match input.as_object() {
                    Some(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    None => JsonMap::from_iter([("value".into(), input.clone())]),
                };
                let mut updated_meta = provider_metadata.clone();
                if let Some((_, part)) = self.read_tool_call(id) {
                    if part
                        .metadata
                        .as_ref()
                        .is_some_and(|m| m.get("providerExecuted").is_some())
                    {
                        if let Some(meta) = &mut updated_meta {
                            meta.insert("providerExecuted".into(), serde_json::Value::Bool(true));
                        }
                    }
                }
                self.update_tool_call(id, |match_part| {
                    let state = match &match_part.state {
                        ToolState::Running(running) => {
                            ToolState::Running(crate::v1::ToolStateRunning {
                                status: "running".into(),
                                input: input_map.clone(),
                                title: running.title.clone(),
                                metadata: running.metadata.clone(),
                                time: running.time.clone(),
                            })
                        }
                        _ => ToolState::Running(crate::v1::ToolStateRunning {
                            status: "running".into(),
                            input: input_map.clone(),
                            title: None,
                            metadata: None,
                            time: crate::v1::RunningTime {
                                start: self.deps.now(),
                            },
                        }),
                    };
                    ToolPart {
                        tool: name.clone(),
                        state,
                        metadata: updated_meta.clone(),
                        ..match_part
                    }
                })?;
                Ok(())
            }
            LLMEvent::ToolResult {
                id, name, result, ..
            } => {
                if let crate::llm::ToolResultValue::Error(map) = result {
                    if self.read_tool_call(id).is_none() {
                        return Ok(());
                    }
                    let message = map
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool error")
                        .to_string();
                    self.fail_tool_call(id, &message)?;
                    return Ok(());
                }
                let raw_output = self.tool_result_output(name, result);
                let mut attachments = Vec::new();
                let mut omitted = 0usize;
                for attachment in raw_output.attachments.clone().unwrap_or_default() {
                    if attachment.mime.starts_with("image/") {
                        match self.deps.image_normalize(&attachment) {
                            Ok(normalized) => attachments.push(normalized),
                            Err(_) => omitted += 1,
                        }
                    } else {
                        attachments.push(attachment);
                    }
                }
                let output = ToolOutput {
                    title: raw_output.title,
                    metadata: raw_output.metadata,
                    output: if omitted == 0 {
                        raw_output.output
                    } else {
                        format!(
                            "{}\n\n[{} image{} omitted: could not be resized below the image size limit.]",
                            raw_output.output,
                            omitted,
                            if omitted == 1 { "" } else { "s" }
                        )
                    },
                    attachments: if attachments.is_empty() {
                        None
                    } else {
                        Some(attachments)
                    },
                };
                self.complete_tool_call(id, &output)?;
                Ok(())
            }
            LLMEvent::ToolError { id, message, .. } => {
                self.fail_tool_call(id, message)?;
                Ok(())
            }
            LLMEvent::ProviderError { message, .. } => Err(message.clone()),
            LLMEvent::StepStart { .. } => {
                if self.ctx.snapshot.is_none() {
                    self.ctx.snapshot = self.deps.snapshot_track().ok();
                }
                let snapshot = self.ctx.snapshot.clone().unwrap_or_default();
                self.deps
                    .update_part(&Part::StepStart(crate::v1::StepStartPart {
                        base: PartBase {
                            id: crate::schema::create_part(None),
                            session_id: self.ctx.session_id.clone(),
                            message_id: self.ctx.assistant_message.id.clone(),
                        },
                        type_: "step-start".into(),
                        snapshot: Some(snapshot),
                    }))?;
                Ok(())
            }
            LLMEvent::StepFinish {
                reason,
                usage,
                provider_metadata,
                ..
            } => {
                let completed_snapshot = self.deps.snapshot_track().ok();
                for id in self.ctx.reasoning_map.keys().cloned().collect::<Vec<_>>() {
                    self.finish_reasoning(&id)?;
                }
                let usage = usage.clone().unwrap_or_default();
                let usage_result = self.deps.get_usage(&usage);
                self.ctx.assistant_message.finish = Some(reason.clone());
                self.ctx.assistant_message.cost += usage_result.cost;
                self.ctx.assistant_message.tokens = assistant_tokens(&usage_result.tokens);
                self.deps
                    .update_part(&Part::StepFinish(crate::v1::StepFinishPart {
                        base: PartBase {
                            id: crate::schema::create_part(None),
                            session_id: self.ctx.assistant_message.session_id.clone(),
                            message_id: self.ctx.assistant_message.id.clone(),
                        },
                        type_: "step-finish".into(),
                        reason: reason.clone(),
                        snapshot: completed_snapshot,
                        cost: usage_result.cost,
                        tokens: crate::v1::StepTokens {
                            total: usage_result.tokens.total,
                            input: usage_result.tokens.input,
                            output: usage_result.tokens.output,
                            reasoning: usage_result.tokens.reasoning,
                            cache: usage_result.tokens.cache.clone(),
                        },
                    }))?;
                self.deps.update_message(&self.ctx.assistant_message)?;
                let snapshot = self.ctx.snapshot.clone();
                if let Some(snapshot) = snapshot {
                    if let Ok(patch) = self.deps.snapshot_patch(&snapshot) {
                        if !patch.files.is_empty() {
                            self.deps.update_part(&Part::Patch(crate::v1::PatchPart {
                                base: PartBase {
                                    id: crate::schema::create_part(None),
                                    session_id: self.ctx.session_id.clone(),
                                    message_id: self.ctx.assistant_message.id.clone(),
                                },
                                type_: "patch".into(),
                                hash: patch.hash,
                                files: patch.files,
                            }))?;
                        }
                    }
                    self.ctx.snapshot = None;
                }
                self.deps
                    .summarize(&self.ctx.session_id, &self.ctx.assistant_message.parent_id)?;
                let tokens = assistant_tokens(&usage_result.tokens);
                if !self.ctx.assistant_message.summary.unwrap_or(false)
                    && self.deps.is_overflow(&tokens)
                {
                    self.ctx.needs_compaction = true;
                }
                let _ = provider_metadata;
                Ok(())
            }
            LLMEvent::TextStart {
                provider_metadata, ..
            } => {
                let text = TextPart {
                    base: PartBase {
                        id: crate::schema::create_part(None),
                        session_id: self.ctx.assistant_message.session_id.clone(),
                        message_id: self.ctx.assistant_message.id.clone(),
                    },
                    type_: "text".into(),
                    text: String::new(),
                    synthetic: None,
                    ignored: None,
                    time: Some(crate::v1::PartTime {
                        start: self.deps.now(),
                        end: None,
                    }),
                    metadata: provider_metadata.clone(),
                };
                self.deps.update_part(&Part::Text(text.clone()))?;
                self.ctx.current_text = Some(text);
                Ok(())
            }
            LLMEvent::TextDelta {
                text,
                provider_metadata,
                ..
            } => {
                let Some(current) = self.ctx.current_text.as_mut() else {
                    return Ok(());
                };
                current.text.push_str(text);
                if provider_metadata.is_some() {
                    current.metadata = provider_metadata.clone();
                }
                let session_id = current.base.session_id.clone();
                let message_id = current.base.message_id.clone();
                let part_id = current.base.id.clone();
                self.deps
                    .update_part_delta(&session_id, &message_id, &part_id, "text", text)?;
                Ok(())
            }
            LLMEvent::TextEnd {
                provider_metadata, ..
            } => {
                let Some(mut current) = self.ctx.current_text.take() else {
                    return Ok(());
                };
                current.text = self.deps.plugin_text_complete(
                    &self.ctx.session_id,
                    &self.ctx.assistant_message.id,
                    &current.base.id,
                    &current.text,
                )?;
                let end = self.deps.now();
                let start = current.time.as_ref().map(|t| t.start).unwrap_or(end);
                current.time = Some(crate::v1::PartTime {
                    start,
                    end: Some(end),
                });
                if provider_metadata.is_some() {
                    current.metadata = provider_metadata.clone();
                }
                self.deps.update_part(&Part::Text(current))?;
                Ok(())
            }
            LLMEvent::Finish { .. } => Ok(()),
        }
    }

    fn tool_result_output(&self, name: &str, result: &crate::llm::ToolResultValue) -> ToolOutput {
        let (output, title, metadata, attachments) = match result {
            crate::llm::ToolResultValue::Json(value) => {
                let map: JsonMap = value
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let output = map
                    .get("output")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default());
                let title = map
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| name.to_string());
                let metadata: JsonMap = map
                    .get("metadata")
                    .and_then(|v| v.as_object())
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let attachments = map
                    .get("attachments")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter(|v| Self::is_file_part(v))
                            .filter_map(|v| serde_json::from_value(v.clone()).ok())
                            .collect::<Vec<crate::v1::FilePart>>()
                    });
                (output, title, metadata, attachments)
            }
            crate::llm::ToolResultValue::Text(text) => {
                (text.clone(), name.to_string(), JsonMap::new(), None)
            }
            crate::llm::ToolResultValue::Error(_) => {
                (String::new(), name.to_string(), JsonMap::new(), None)
            }
        };
        ToolOutput {
            title,
            metadata,
            output,
            attachments,
        }
    }
}

fn input_of(state: &ToolState) -> JsonMap {
    match state {
        ToolState::Pending(pending) => pending.input.clone(),
        ToolState::Running(running) => running.input.clone(),
        ToolState::Completed(completed) => completed.input.clone(),
        ToolState::Error(error) => error.input.clone(),
    }
}

fn assistant_tokens(tokens: &crate::session::Tokens) -> crate::v1::AssistantTokens {
    crate::v1::AssistantTokens {
        total: tokens.total,
        input: tokens.input,
        output: tokens.output,
        reasoning: tokens.reasoning,
        cache: tokens.cache.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub title: String,
    pub metadata: JsonMap,
    pub output: String,
    pub attachments: Option<Vec<crate::v1::FilePart>>,
}

#[derive(Debug, Clone)]
pub struct HandleInput {
    pub assistant_message: Assistant,
    pub session_id: String,
    pub model: ProviderModel,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDeps {
        parts: std::cell::RefCell<Vec<Part>>,
        events: Vec<LLMEvent>,
        now: std::cell::Cell<u64>,
    }

    impl FakeDeps {
        fn new(events: Vec<LLMEvent>) -> Self {
            FakeDeps {
                parts: std::cell::RefCell::new(Vec::new()),
                events,
                now: std::cell::Cell::new(1000),
            }
        }
    }

    impl ProcessorDeps for FakeDeps {
        fn update_part(&self, part: &Part) -> Result<Part, String> {
            self.parts.borrow_mut().push(part.clone());
            Ok(part.clone())
        }
        fn update_part_delta(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<(), String> {
            Ok(())
        }
        fn get_part(&self, _: &str, _: &str, part_id: &str) -> Result<Option<Part>, String> {
            Ok(self
                .parts
                .borrow()
                .iter()
                .rev()
                .find(|part| part.id() == part_id)
                .cloned())
        }
        fn update_message(&self, _: &Assistant) -> Result<(), String> {
            Ok(())
        }
        fn snapshot_track(&self) -> Result<String, String> {
            Ok("snap".into())
        }
        fn snapshot_patch(&self, _: &str) -> Result<Patch, String> {
            Ok(Patch {
                hash: "h".into(),
                files: vec![],
            })
        }
        fn agent_permission(&self, _: &str) -> Option<crate::v1::Ruleset> {
            None
        }
        fn ask_permission(
            &self,
            _: &str,
            _: &[String],
            _: &str,
            _: &JsonMap,
            _: &[String],
            _: &crate::v1::Ruleset,
        ) -> Result<(), String> {
            Ok(())
        }
        fn stream(&self, _: &StreamInput) -> Result<Vec<LLMEvent>, String> {
            Ok(self.events.clone())
        }
        fn get_usage(&self, usage: &crate::llm::Usage) -> crate::session::UsageResult {
            crate::session::get_usage(&crate::session::GetUsageInput {
                model: &self.model(),
                usage,
                metadata: &JsonMap::new(),
            })
        }
        fn summarize(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn config(&self) -> ConfigSnapshot {
            ConfigSnapshot::default()
        }
        fn is_overflow(&self, _: &crate::v1::AssistantTokens) -> bool {
            false
        }
        fn plugin_text_complete(
            &self,
            _: &str,
            _: &str,
            _: &str,
            text: &str,
        ) -> Result<String, String> {
            Ok(text.to_string())
        }
        fn status_set(&self, _: &str, _: &crate::status::Info) -> Result<(), String> {
            Ok(())
        }
        fn image_normalize(
            &self,
            part: &crate::v1::FilePart,
        ) -> Result<crate::v1::FilePart, String> {
            Ok(part.clone())
        }
        fn now(&self) -> u64 {
            self.now.get()
        }
    }

    trait ModelRef {
        fn model(&self) -> ProviderModel;
    }
    impl ModelRef for FakeDeps {
        fn model(&self) -> ProviderModel {
            ProviderModel::empty("gpt-4o", "openai")
        }
    }

    fn assistant() -> Assistant {
        Assistant {
            id: "msg_1".into(),
            session_id: "ses1".into(),
            role: "assistant".into(),
            time: crate::v1::AssistantTime {
                created: 1000,
                completed: None,
            },
            error: None,
            parent_id: "msg_0".into(),
            model_id: "gpt-4o".into(),
            provider_id: "openai".into(),
            mode: "primary".into(),
            agent: "primary".into(),
            path: crate::v1::AssistantPath {
                cwd: "/w".into(),
                root: "/w".into(),
            },
            summary: None,
            cost: 0.0,
            tokens: crate::v1::AssistantTokens {
                total: None,
                input: 0.0,
                output: 0.0,
                reasoning: 0.0,
                cache: crate::v1::CacheTokens {
                    read: 0.0,
                    write: 0.0,
                },
            },
            structured: None,
            variant: None,
            finish: None,
        }
    }

    #[test]
    fn text_stream_produces_text_part() {
        let deps = FakeDeps::new(vec![
            LLMEvent::TextStart {
                id: "t1".into(),
                provider_metadata: None,
            },
            LLMEvent::TextDelta {
                id: "t1".into(),
                text: "Hello".into(),
                provider_metadata: None,
            },
            LLMEvent::TextEnd {
                id: "t1".into(),
                provider_metadata: None,
            },
            LLMEvent::Finish {
                reason: "stop".into(),
                usage: None,
                provider_metadata: None,
            },
        ]);
        let mut handle = Handle::create(
            &HandleInput {
                assistant_message: assistant(),
                session_id: "ses1".into(),
                model: ProviderModel::empty("gpt-4o", "openai"),
            },
            &deps,
        )
        .unwrap();
        let result = handle
            .process(&StreamInput {
                model: ProviderModel::empty("gpt-4o", "openai"),
                messages: vec![],
                system: vec![],
                tools: vec![],
                session_id: "ses1".into(),
                agent: "primary".into(),
            })
            .unwrap();
        assert_eq!(result, ProcessResult::Continue);
        let parts = deps.parts.borrow();
        let text_count = parts
            .iter()
            .filter(|part| matches!(part, Part::Text(t) if t.text == "Hello"))
            .count();
        assert_eq!(text_count, 1);
    }

    #[test]
    fn summary_message_rejects_tool_calls() {
        let mut msg = assistant();
        msg.summary = Some(true);
        let deps = FakeDeps::new(vec![LLMEvent::ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            input: serde_json::json!({}),
            provider_executed: None,
            provider_metadata: None,
        }]);
        let mut handle = Handle::create(
            &HandleInput {
                assistant_message: msg,
                session_id: "ses1".into(),
                model: ProviderModel::empty("gpt-4o", "openai"),
            },
            &deps,
        )
        .unwrap();
        let err = handle.handle_event(&LLMEvent::ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            input: serde_json::json!({}),
            provider_executed: None,
            provider_metadata: None,
        });
        assert!(err.is_err());
    }
}
