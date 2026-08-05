//! The durable coding-agent loop.
//!
//! Ports `packages/core/src/session/runner/llm.ts`. One provider turn is
//! `run_turn_attempt`: build the request from projected history, stream the
//! turn through `LLMClient`, persist events through `LLMEventPublisher`, settle
//! local tool calls concurrently, and decide whether to continue. Automatic
//! compaction and overflow recovery bounce the turn through `run_turn` /
//! `run_after_overflow_compaction`. `run` drives the drain until no durable
//! input remains.

use std::sync::Arc;

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::llm::error::{is_context_overflow, is_context_overflow_failure, RunFailure};
use crate::llm::event::{LLMEvent, ProviderErrorEvent};
use crate::llm::message::{
    ContentPart, LLMRequest, Message, OpenAIOptions, ProviderOptions, SystemPart, ToolChoice,
};
use crate::runner::max_steps::MAX_STEPS_PROMPT;
use crate::runner::model;
use crate::runner::publish_llm_event::{LLMEventPublisher, PublishError, PublisherInput};
use crate::runner::to_llm_message::to_llm_messages;
use crate::runner::RunError;
use crate::session::event::SessionEvent;
use crate::session::message::{SessionMessage, UnknownError, UnknownErrorKind};
use crate::session::services::{
    combine_contexts, CompactionInput, Delivery, ExecuteInput, SystemContext, ToolCall,
    ToolSettlementError,
};
use crate::session::util::timestamp_now;

/// Aggregate service bundle the runner orchestrates. Kept as a plain struct of
/// trait objects so `oc-session`/`oc-llm`/`oc-tool`/`oc-provider` can supply
/// `Arc<dyn ...>` implementations without a layer framework.
pub struct RunnerDeps {
    pub events: Arc<dyn crate::session::services::EventBus>,
    pub llm: Arc<dyn crate::session::services::LlmClient>,
    pub agents: Arc<dyn crate::session::services::Agents>,
    pub tools: Arc<dyn crate::session::services::ToolRegistry>,
    pub models: Arc<dyn crate::runner::model::SessionRunnerModel>,
    pub store: Arc<dyn crate::session::services::SessionStore>,
    pub location: Arc<dyn crate::session::services::LocationService>,
    pub system_context: Arc<dyn crate::session::services::SystemContextRegistry>,
    pub skill_guidance: Arc<dyn crate::session::services::SkillGuidance>,
    pub reference_guidance: Arc<dyn crate::session::services::ReferenceGuidance>,
    pub snapshots: Arc<dyn crate::session::services::Snapshots>,
    pub input: Arc<dyn crate::session::services::SessionInput>,
    pub history: Arc<dyn crate::session::services::SessionHistory>,
    pub context_epoch: Arc<dyn crate::session::services::SessionContextEpoch>,
    pub compaction: Arc<dyn crate::session::services::SessionCompaction>,
}

/// Outcome of one provider turn.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    pub needs_continuation: bool,
    pub step: u32,
    pub interrupted: bool,
}

impl TurnOutcome {
    fn done(needs_continuation: bool, step: u32) -> Self {
        Self {
            needs_continuation,
            step,
            interrupted: false,
        }
    }

    fn interrupted() -> Self {
        Self {
            needs_continuation: false,
            step: 1,
            interrupted: true,
        }
    }
}

/// Internal failures that restart the turn through a compaction path.
/// /// From reference/packages/core/src/session/runner/llm.ts
enum TurnFailure {
    Compaction { step: u32 },
    OverflowCompaction { step: u32 },
    Interrupted,
    Error(RunError),
}

/// The agent loop service.
/// /// From reference/packages/core/src/session/runner/llm.ts
pub struct SessionRunnerService {
    deps: Arc<RunnerDeps>,
}

impl SessionRunnerService {
    pub fn new(deps: RunnerDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }

    /// Drains eligible durable work. Explicit runs perform one provider
    /// attempt even when no work is eligible.
    /// /// From reference/packages/core/src/session/runner/llm.ts
    pub async fn run(
        &self,
        session_id: &crate::session::SessionID,
        force: bool,
        token: &CancellationToken,
    ) -> Result<(), RunError> {
        let has_steer = self
            .deps
            .input
            .has_pending(session_id, Delivery::Steer)
            .await;
        let has_queue = if has_steer {
            false
        } else {
            self.deps
                .input
                .has_pending(session_id, Delivery::Queue)
                .await
        };
        if !force && !has_steer && !has_queue {
            return Ok(());
        }
        self.fail_interrupted_tools(session_id).await;
        let mut promotion: Option<Delivery> = if has_steer {
            Some(Delivery::Steer)
        } else if has_queue {
            Some(Delivery::Queue)
        } else {
            None
        };
        let mut should_run = force || has_steer || has_queue;
        while should_run {
            let mut needs_continuation = true;
            let mut step = 1u32;
            while needs_continuation {
                let outcome = self.run_turn(session_id, promotion, step, token).await?;
                if outcome.interrupted {
                    return Ok(());
                }
                needs_continuation = outcome.needs_continuation;
                step = outcome.step + 1;
                promotion = Some(Delivery::Steer);
                if !needs_continuation {
                    needs_continuation = self
                        .deps
                        .input
                        .has_pending(session_id, Delivery::Steer)
                        .await;
                }
            }
            should_run = self
                .deps
                .input
                .has_pending(session_id, Delivery::Queue)
                .await;
            promotion = if should_run {
                Some(Delivery::Queue)
            } else {
                None
            };
        }
        Ok(())
    }

    /// One turn with compaction recovery; restarts through a compacted request.
    /// /// From reference/packages/core/src/session/runner/llm.ts (`runTurn`)
    async fn run_turn(
        &self,
        session_id: &crate::session::SessionID,
        promotion: Option<Delivery>,
        step: u32,
        token: &CancellationToken,
    ) -> Result<TurnOutcome, RunError> {
        let mut promotion = promotion;
        let mut step = step;
        loop {
            match self
                .run_turn_attempt(session_id, promotion, step, true, token)
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(TurnFailure::Compaction { step: next }) => {
                    tokio::task::yield_now().await;
                    promotion = None;
                    step = next;
                }
                Err(TurnFailure::OverflowCompaction { step: next }) => {
                    tokio::task::yield_now().await;
                    return self
                        .run_after_overflow_compaction(session_id, next, token)
                        .await;
                }
                Err(TurnFailure::Interrupted) => return Ok(TurnOutcome::interrupted()),
                Err(TurnFailure::Error(error)) => return Err(error),
            }
        }
    }

    /// Post-overflow-compaction attempt: a second overflow is a defect.
    /// /// From reference/packages/core/src/session/runner/llm.ts (`runAfterOverflowCompaction`)
    async fn run_after_overflow_compaction(
        &self,
        session_id: &crate::session::SessionID,
        step: u32,
        token: &CancellationToken,
    ) -> Result<TurnOutcome, RunError> {
        let mut step = step;
        loop {
            match self
                .run_turn_attempt(session_id, None, step, false, token)
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(TurnFailure::OverflowCompaction { .. }) => {
                    return Err(RunError::Defect(
                        "Post-compaction provider attempt cannot recover another overflow".into(),
                    ));
                }
                Err(TurnFailure::Compaction { step: next }) => {
                    tokio::task::yield_now().await;
                    step = next;
                }
                Err(TurnFailure::Interrupted) => return Ok(TurnOutcome::interrupted()),
                Err(TurnFailure::Error(error)) => return Err(error),
            }
        }
    }

    async fn load_system_context(&self, agent: &str) -> SystemContext {
        combine_contexts(vec![
            self.deps.system_context.load().await,
            self.deps.skill_guidance.load(agent).await,
            self.deps.reference_guidance.load().await,
        ])
    }

    /// Fail any pending/running tools recorded in projected history. Called
    /// once per drain so stale tool parts from a previous run settle.
    /// /// From reference/packages/core/src/session/runner/llm.ts (`failInterruptedTools`)
    async fn fail_interrupted_tools(&self, session_id: &crate::session::SessionID) {
        let context = self.deps.store.context(session_id).await;
        for message in context {
            let SessionMessage::Assistant(assistant) = &message else {
                continue;
            };
            for item in &assistant.content {
                let Some(tool) = item.as_tool() else { continue };
                let interrupted = matches!(
                    tool.state,
                    crate::session::message::ToolState::Pending { .. }
                        | crate::session::message::ToolState::Running { .. }
                );
                if !interrupted {
                    continue;
                }
                self.deps
                    .events
                    .publish(SessionEvent::ToolFailed {
                        timestamp: timestamp_now(),
                        session_id: session_id.to_string(),
                        assistant_message_id: assistant.id.clone(),
                        call_id: tool.id.clone(),
                        error: UnknownError {
                            kind: UnknownErrorKind::Unknown,
                            message: "Tool execution interrupted".into(),
                        },
                        result: None,
                        provider: crate::session::event::Provider::new(
                            tool.provider
                                .as_ref()
                                .map(|provider| provider.executed)
                                .unwrap_or(false),
                            tool.provider
                                .as_ref()
                                .and_then(|provider| provider.metadata.clone()),
                        ),
                    })
                    .await;
            }
        }
    }

    /// One provider turn attempt. `recover_overflow` gates the automatic
    /// overflow compaction retry.
    /// /// From reference/packages/core/src/session/runner/llm.ts (`runTurnAttempt`)
    async fn run_turn_attempt(
        &self,
        session_id: &crate::session::SessionID,
        promotion: Option<Delivery>,
        step: u32,
        recover_overflow: bool,
        token: &CancellationToken,
    ) -> Result<TurnOutcome, TurnFailure> {
        let session = self.deps.store.get(session_id).await.ok_or_else(|| {
            TurnFailure::Error(RunError::SessionNotFound {
                session_id: session_id.to_string(),
            })
        })?;

        let current_location = self.deps.location.current();
        if !current_location.owns(&session.location) {
            return Err(TurnFailure::Interrupted);
        }

        let agent = self.deps.agents.select(session.agent.as_deref()).await;

        let system_context = self.load_system_context(&agent.id).await;
        let initialized = self
            .deps
            .context_epoch
            .initialize(session_id, system_context.clone())
            .await;

        let mut tool_fibers = JoinSet::new();
        let mut needs_continuation = false;
        let mut current_step = step;

        if let Some(promotion) = promotion {
            let cutoff = self.deps.input.latest_sequence(session_id).await;
            let mut promoted = 0u64;
            match promotion {
                Delivery::Steer => {
                    promoted += self.deps.input.promote_steers(session_id, cutoff).await
                }
                Delivery::Queue => {
                    promoted += u64::from(self.deps.input.promote_next_queued(session_id).await);
                    promoted += self.deps.input.promote_steers(session_id, cutoff).await;
                }
            }
            if promoted > 0 {
                current_step = 1;
            }
        }

        let system = match initialized {
            Some(initialized) => initialized,
            None => {
                self.deps
                    .context_epoch
                    .prepare(session_id, system_context)
                    .await
            }
        };

        let resolved_model = self
            .deps
            .models
            .resolve(&session)
            .await
            .map_err(|error| TurnFailure::Error(RunError::Model(error)))?;
        let entries = self
            .deps
            .history
            .entries_for_runner(session_id, system.baseline_seq)
            .await;
        let context_messages = entries
            .iter()
            .map(|entry| &entry.message)
            .cloned()
            .collect::<Vec<_>>();
        let is_last_step = agent
            .info
            .as_ref()
            .and_then(|info| info.steps)
            .map(|steps| current_step >= steps)
            .unwrap_or(false);

        let tool_materialization = if is_last_step {
            None
        } else {
            self.deps
                .tools
                .materialize(
                    agent
                        .info
                        .as_ref()
                        .map(|info| info.permissions.as_slice())
                        .unwrap_or(&[]),
                )
                .await
        };

        let prompt_cache_key = prompt_cache_key(session_id);

        let mut system_parts = Vec::new();
        for part in [
            agent.info.as_ref().and_then(|info| info.system.clone()),
            Some(system.baseline.clone()),
        ]
        .into_iter()
        .flatten()
        {
            if !part.is_empty() {
                system_parts.push(SystemPart::make(part));
            }
        }

        let mut messages = to_llm_messages(&context_messages, &resolved_model);
        if is_last_step {
            messages.push(Message::assistant(vec![ContentPart::text(
                MAX_STEPS_PROMPT,
            )]));
        }

        let request = LLMRequest {
            id: None,
            model: resolved_model.clone(),
            system: system_parts,
            messages,
            tools: tool_materialization
                .as_ref()
                .map(|materialization| materialization.definitions.clone())
                .unwrap_or_default(),
            tool_choice: if is_last_step {
                Some(ToolChoice::mode("none"))
            } else {
                None
            },
            generation: None,
            provider_options: Some(ProviderOptions {
                openai: Some(OpenAIOptions {
                    prompt_cache_key: Some(prompt_cache_key),
                }),
            }),
            http: None,
        };

        if self
            .deps
            .compaction
            .compact_if_needed(CompactionInput {
                session_id: session_id.to_string(),
                entries: entries.clone(),
                model: resolved_model.clone(),
                request: request.clone(),
            })
            .await
        {
            return Err(TurnFailure::Compaction { step: current_step });
        }

        let start_snapshot = self.deps.snapshots.capture().await;
        let publisher = Arc::new(LLMEventPublisher::new(
            self.deps.events.clone(),
            PublisherInput {
                session_id: session_id.to_string(),
                agent: agent.id.clone(),
                model: model::ref_from_model(&resolved_model, &session),
                snapshot: start_snapshot.clone(),
            },
        ));

        let mut overflow_failure: Option<ProviderErrorEvent> = None;

        // One provider turn. The reference streams events and forks settlements
        // concurrently; `LlmClient.stream` buffers the events here but the
        // settlement tasks still run concurrently and are joined together.
        let mut stream_error: Option<crate::llm::LLMError> = None;
        let mut stream_interrupted = false;
        let events_result = tokio::select! {
            _ = token.cancelled() => {
                stream_interrupted = true;
                None
            }
            result = self.deps.llm.stream(request.clone()) => Some(result),
        };
        if let Some(Ok(events)) = &events_result {
            for event in events {
                if overflow_failure.is_some() || publisher.has_provider_error() {
                    break;
                }
                if let LLMEvent::ProviderError(ref provider_error) = event {
                    if is_context_overflow(&provider_error.message)
                        && !publisher.has_assistant_started()
                    {
                        overflow_failure = Some(provider_error.clone());
                        continue;
                    }
                }
                publisher.publish(event, &[]).await.map_err(turn_error)?;

                if let LLMEvent::ToolCall {
                    id,
                    name,
                    input,
                    provider_executed,
                    provider_metadata,
                } = &event
                {
                    if *provider_executed == Some(true) {
                        continue;
                    }
                    match &tool_materialization {
                        None => {
                            publisher
                                .fail_unsettled_tools(
                                    "Tools are disabled after the maximum agent steps",
                                    false,
                                )
                                .await
                                .map_err(turn_error)?;
                        }
                        Some(materialization) => {
                            needs_continuation = true;
                            let assistant_message_id = publisher
                                .assistant_message_id(id)
                                .await
                                .map_err(turn_error)?;
                            let call_id = id.clone();
                            let name = name.clone();
                            let input = input.clone();
                            let provider_metadata = provider_metadata.clone();
                            let call = ToolCall {
                                id: call_id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                                provider_executed: false,
                                provider_metadata: provider_metadata.clone(),
                            };
                            let publisher = publisher.clone();
                            let materialization = materialization.clone();
                            let session_id = session_id.to_string();
                            let agent_id = agent.id.clone();
                            let token = token.clone();
                            tool_fibers.spawn(async move {
                                match tokio::select! {
                                    _ = token.cancelled() => Err(ToolSettlementError::Interrupted),
                                    result = materialization.settle.settle(ExecuteInput {
                                        session_id,
                                        agent: agent_id,
                                        assistant_message_id,
                                        call,
                                    }) => result,
                                } {
                                    Ok(settlement) => {
                                        let event = LLMEvent::ToolResult {
                                            id: call_id.clone(),
                                            name,
                                            result: settlement.result,
                                            output: settlement.output,
                                            provider_executed: None,
                                            provider_metadata: None,
                                        };
                                        publisher
                                            .publish(&event, &settlement.output_paths)
                                            .await
                                            .map_err(|error| {
                                                ToolSettlementError::Failed(error.to_string())
                                            })
                                    }
                                    Err(error) => Err(error),
                                }
                            });
                        }
                    }
                }
            }
        } else if let Some(Err(error)) = &events_result {
            stream_error = Some(error.clone());
        }

        publisher.flush().await.map_err(turn_error)?;

        // Overflow recovery: rebuild once through the no-recovery path.
        let overflow = overflow_failure
            .clone()
            .map(RunFailure::Event)
            .or_else(|| stream_error.clone().map(RunFailure::Error));
        if recover_overflow
            && !publisher.has_assistant_started()
            && overflow
                .as_ref()
                .map(is_context_overflow_failure)
                .unwrap_or(false)
        {
            let recovered = self
                .deps
                .compaction
                .compact_after_overflow(CompactionInput {
                    session_id: session_id.to_string(),
                    entries: entries.clone(),
                    model: resolved_model.clone(),
                    request: request.clone(),
                })
                .await;
            if recovered {
                return Err(TurnFailure::OverflowCompaction { step: current_step });
            }
        }

        if let Some(overflow_failure) = &overflow_failure {
            publisher
                .publish(&LLMEvent::ProviderError(overflow_failure.clone()), &[])
                .await
                .map_err(turn_error)?;
        }

        if let Some(error) = &stream_error {
            if !publisher.has_provider_error() {
                publisher
                    .fail_unsettled_tools("Provider did not return a tool result", true)
                    .await
                    .map_err(turn_error)?;
                publisher
                    .fail_assistant(error.reason.message())
                    .await
                    .map_err(turn_error)?;
            }
        }

        if stream_interrupted {
            tool_fibers.abort_all();
        }

        let settlement = self.await_tool_fibers(&mut tool_fibers).await;

        if matches!(settlement, ToolSettlement::Declined) {
            tool_fibers.abort_all();
            publisher
                .fail_unsettled_tools("Tool execution interrupted", false)
                .await
                .map_err(turn_error)?;
            return Err(TurnFailure::Interrupted);
        }

        if stream_interrupted || matches!(settlement, ToolSettlement::Interrupted) {
            tool_fibers.abort_all();
            publisher
                .fail_unsettled_tools("Tool execution interrupted", false)
                .await
                .map_err(turn_error)?;
            if publisher.has_active_assistant() {
                publisher
                    .fail_assistant("Provider turn interrupted")
                    .await
                    .map_err(turn_error)?;
            }
        }

        if let ToolSettlement::Failed(message) = &settlement {
            publisher
                .fail_unsettled_tools(&format!("Tool execution failed: {message}"), false)
                .await
                .map_err(turn_error)?;
        }

        if let Some(step_settlement) = publisher.step_settlement() {
            if !publisher.has_provider_error() {
                let end_snapshot = self.deps.snapshots.capture().await;
                let files = match (&start_snapshot, &end_snapshot) {
                    (Some(from), Some(to)) => self.deps.snapshots.files(from, to).await,
                    _ => None,
                };
                let assistant_message_id = publisher.start_assistant().await.map_err(turn_error)?;
                self.deps
                    .events
                    .publish(SessionEvent::StepEnded {
                        timestamp: timestamp_now(),
                        session_id: session_id.to_string(),
                        assistant_message_id,
                        finish: step_settlement.finish,
                        cost: 0.0,
                        tokens: step_settlement.tokens,
                        snapshot: end_snapshot,
                        files,
                    })
                    .await;
            }
        }

        if publisher.has_provider_error() {
            publisher
                .fail_unsettled_tools("Tool execution interrupted", false)
                .await
                .map_err(turn_error)?;
        }

        let stream_succeeded = events_result.is_some() && stream_error.is_none();
        if stream_succeeded && !publisher.has_provider_error() {
            publisher
                .fail_unsettled_tools("Provider did not return a tool result", true)
                .await
                .map_err(turn_error)?;
        }

        if let Some(error) = stream_error {
            return Err(TurnFailure::Error(RunError::Llm(error)));
        }
        if matches!(settlement, ToolSettlement::Interrupted) {
            return Err(TurnFailure::Interrupted);
        }

        Ok(TurnOutcome::done(
            !publisher.has_provider_error() && needs_continuation,
            current_step,
        ))
    }

    /// Join all settlement fibers. Mirrors `awaitToolFibers`/`FiberSet.join`:
    /// a declined or interrupted settlement ends the wait early (remaining
    /// fibers are aborted); regular failures drain fully.
    /// /// From reference/packages/core/src/session/runner/llm.ts
    async fn await_tool_fibers(
        &self,
        tool_fibers: &mut JoinSet<Result<(), ToolSettlementError>>,
    ) -> ToolSettlement {
        let mut declined = false;
        let mut interrupted = false;
        let mut failed: Option<String> = None;
        while !tool_fibers.is_empty() {
            match tool_fibers.join_next().await {
                Some(Ok(Ok(()))) => {}
                Some(Ok(Err(ToolSettlementError::Declined))) => {
                    declined = true;
                    break;
                }
                Some(Ok(Err(ToolSettlementError::Interrupted))) => {
                    interrupted = true;
                    break;
                }
                Some(Ok(Err(ToolSettlementError::OutputStore(error)))) => {
                    if failed.is_none() {
                        failed = Some(error.0);
                    }
                }
                Some(Ok(Err(ToolSettlementError::Failed(message)))) => {
                    if failed.is_none() {
                        failed = Some(message);
                    }
                }
                Some(Err(_)) => {
                    interrupted = true;
                    break;
                }
                None => break,
            }
        }
        if declined {
            tool_fibers.abort_all();
            ToolSettlement::Declined
        } else if interrupted {
            tool_fibers.abort_all();
            ToolSettlement::Interrupted
        } else if let Some(failed) = failed {
            ToolSettlement::Failed(failed)
        } else {
            ToolSettlement::Ok
        }
    }
}

enum ToolSettlement {
    Ok,
    Declined,
    Interrupted,
    Failed(String),
}

fn turn_error(error: PublishError) -> TurnFailure {
    TurnFailure::Error(RunError::Publish(error.to_string()))
}

/// True when a session id is the durable `ses_` + 64-hex shape used for prompt
/// caching keys. Mirrors the reference regex `/^ses_[0-9a-f]{64}$/`.
/// /// From reference/packages/core/src/session/runner/llm.ts
pub fn is_ses_hex_id(session_id: &str) -> bool {
    session_id.len() == 68
        && session_id.starts_with("ses_")
        && session_id[4..]
            .chars()
            .all(|c| c.is_ascii_lowercase() && c.is_ascii_hexdigit())
}

/// The prompt cache key: `ses_`-prefixed 64-hex ids lose the prefix.
/// /// From reference/packages/core/src/session/runner/llm.ts
pub fn prompt_cache_key(session_id: &str) -> String {
    if is_ses_hex_id(session_id) {
        session_id[4..].to_string()
    } else {
        session_id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ses_hex_id_prompt_key_strips_prefix() {
        let id = format!("ses_{}", "a".repeat(64));
        assert!(is_ses_hex_id(&id));
        assert_eq!(prompt_cache_key(&id), "a".repeat(64));
    }

    #[test]
    fn non_hex_id_prompt_key_is_unchanged() {
        let id = "ses_abc";
        assert!(!is_ses_hex_id(id));
        assert_eq!(prompt_cache_key(id), id);
    }
}
