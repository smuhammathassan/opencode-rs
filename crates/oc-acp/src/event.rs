//! Event subscription for ACP sessions.
//!
//! From reference/packages/opencode/src/acp/event.ts. Consumes the opencode
//! global event stream and forwards message part updates, deltas and permission
//! requests to the connected ACP client as `session/update` notifications.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::connection::AgentSideConnection;
use crate::content::{self, ReplayPart};
use crate::permission::Handler as PermissionHandler;
use crate::sdk::{
    Event, MessagePartDeltaProperties, MessagePartUpdatedProperties, OpencodeClient, Part,
    SessionMessageResponse, ToolPart, ToolState,
};
use crate::session::{PartMetadataLookupInput, RecordPartMetadataInput, Service as SessionService};
use crate::tool;
use crate::types::{ContentBlock, ContentChunk, SessionUpdate, TextContent};

/// A running event subscription for one ACP connection.
pub struct Subscription {
    sdk: Arc<dyn OpencodeClient>,
    connection: Option<Arc<dyn AgentSideConnection>>,
    session: Arc<SessionService>,
    permission: Arc<PermissionHandler>,
    abort: Arc<AtomicBool>,
    started: AtomicBool,
    shell_snapshots: Mutex<HashMap<String, String>>,
    tool_starts: Mutex<HashSet<String>>,
}

/// `start` from reference/packages/opencode/src/acp/event.ts.
pub fn start(input: StartInput) -> Arc<Subscription> {
    let subscription = Arc::new(Subscription::new(input));
    subscription.start();
    subscription
}

/// Input to [`start`].
pub struct StartInput {
    pub sdk: Arc<dyn OpencodeClient>,
    pub connection: Option<Arc<dyn AgentSideConnection>>,
    pub session: Arc<SessionService>,
}

impl Subscription {
    fn new(input: StartInput) -> Self {
        Self {
            permission: Arc::new(PermissionHandler::new(
                input.sdk.clone(),
                input.connection.clone(),
                input.session.clone(),
            )),
            sdk: input.sdk,
            connection: input.connection,
            session: input.session,
            abort: Arc::new(AtomicBool::new(false)),
            started: AtomicBool::new(false),
            shell_snapshots: Mutex::new(HashMap::new()),
            tool_starts: Mutex::new(HashSet::new()),
        }
    }

    /// `start` from reference/packages/opencode/src/acp/event.ts.
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.run().await;
        });
    }

    /// `stop` from reference/packages/opencode/src/acp/event.ts.
    pub fn stop(&self) {
        self.abort.store(true, Ordering::SeqCst);
    }

    /// `run` from reference/packages/opencode/src/acp/event.ts. Reconnects the
    /// event stream every second once it ends.
    async fn run(&self) {
        while !self.abort.load(Ordering::SeqCst) {
            let stream = self.sdk.global_event();
            futures::pin_mut!(stream);
            while let Some(event) = stream.next().await {
                if self.abort.load(Ordering::SeqCst) {
                    return;
                }
                let Some(event) = event else {
                    continue;
                };
                let _ = self.handle(&event).await;
            }
            if !self.abort.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
        }
    }

    /// `handle` from reference/packages/opencode/src/acp/event.ts.
    async fn handle(&self, event: &Event) {
        match event {
            Event::PermissionAsked { properties, .. } => self.permission.handle(properties).await,
            Event::MessagePartUpdated { properties, .. } => {
                self.handle_part_updated(properties).await
            }
            Event::MessagePartDelta { properties, .. } => self.handle_part_delta(properties).await,
            Event::Other(_) => {}
        }
    }

    /// `replayMessage` from reference/packages/opencode/src/acp/event.ts.
    pub async fn replay_message(&self, message: &SessionMessageResponse) {
        let role = message.info.role();
        if role != "assistant" && role != "user" {
            return;
        }

        let cwd = if role == "assistant" {
            message.info.assistant_cwd().map(str::to_string)
        } else {
            None
        };
        let session_id = message.info.session_id().to_string();
        for part in &message.parts {
            let _ = self.record_fetched_part(&session_id, message, part).await;
            if part.part_type() == "tool" {
                let Part::Tool(tool_part) = part else {
                    continue;
                };
                let fallback = std::env::current_dir()
                    .map(|dir| dir.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.handle_tool_part(&session_id, tool_part, cwd.as_deref().unwrap_or(&fallback))
                    .await;
                continue;
            }
            self.replay_content_part(message, part).await;
        }
    }

    /// `replayContentPart` from reference/packages/opencode/src/acp/event.ts.
    async fn replay_content_part(&self, message: &SessionMessageResponse, part: &Part) {
        if !matches!(part.part_type(), "text" | "file" | "reasoning") {
            return;
        }
        let session_update = if part.part_type() == "reasoning" {
            "agent_thought_chunk"
        } else if message.info.role() == "user" {
            "user_message_chunk"
        } else {
            "agent_message_chunk"
        };
        let Some(replay_part) = replay_part(part) else {
            return;
        };
        for chunk in content::parts_to_content_chunks(std::slice::from_ref(&replay_part)) {
            let update = match session_update {
                "agent_thought_chunk" => {
                    SessionUpdate::AgentThoughtChunk(with_message_id(chunk, message.info.id()))
                }
                "user_message_chunk" => {
                    SessionUpdate::UserMessageChunk(with_message_id(chunk, message.info.id()))
                }
                _ => SessionUpdate::AgentMessageChunk(with_message_id(chunk, message.info.id())),
            };
            self.send_update(&message.info.session_id(), update).await;
        }
    }

    /// `handlePartUpdated` from reference/packages/opencode/src/acp/event.ts.
    async fn handle_part_updated(&self, properties: &MessagePartUpdatedProperties) {
        let part = &properties.part;
        let session_id = part
            .session_id()
            .unwrap_or(&properties.session_id)
            .to_string();
        let Some(session) = self.session.try_get(&session_id).await else {
            return;
        };

        let _ = self
            .session
            .record_part_metadata(RecordPartMetadataInput {
                session_id: session.id.clone(),
                message_id: part.message_id().unwrap_or_default().to_string(),
                part_id: part.id().unwrap_or_default().to_string(),
                part_type: Some(part.part_type().to_string()),
                role: (part.part_type() == "reasoning").then(|| "assistant".to_string()),
                ignored: if part.part_type() == "text" {
                    part.ignored()
                } else {
                    None
                },
                tool_call_id: if part.part_type() == "tool" {
                    part.call_id().map(str::to_string)
                } else {
                    None
                },
                metadata: part.metadata().cloned().map(Value::Object),
            })
            .await;
        if part.part_type() == "tool" {
            let Part::Tool(tool_part) = part else {
                return;
            };
            self.handle_tool_part(&session.id, tool_part, &session.cwd)
                .await;
        }
    }

    /// `handlePartDelta` from reference/packages/opencode/src/acp/event.ts.
    async fn handle_part_delta(&self, properties: &MessagePartDeltaProperties) {
        let Some(session) = self.session.try_get(&properties.session_id).await else {
            return;
        };

        let known = self
            .session
            .try_get_part_metadata(&PartMetadataLookupInput {
                session_id: session.id.clone(),
                message_id: properties.message_id.clone(),
                part_id: properties.part_id.clone(),
            })
            .await;
        let known_role_and_type = known.as_ref().map_or(false, |metadata| {
            metadata.role.is_some() && metadata.part_type.is_some()
        });
        let metadata = if known_role_and_type {
            known
        } else {
            self.fetch_part_metadata(
                &session.id,
                &session.cwd,
                &properties.message_id,
                &properties.part_id,
            )
            .await
        };
        let Some(metadata) = metadata else {
            return;
        };
        if metadata.role.as_deref() != Some("assistant") {
            return;
        }
        if metadata.part_type.as_deref() == Some("text")
            && properties.field == "text"
            && metadata.ignored != Some(true)
        {
            self.send_update(
                &session.id,
                SessionUpdate::AgentMessageChunk(ContentChunk {
                    message_id: Some(properties.message_id.clone()),
                    content: ContentBlock::Text(TextContent {
                        text: properties.delta.clone(),
                        annotations: None,
                    }),
                }),
            )
            .await;
            return;
        }
        if metadata.part_type.as_deref() == Some("reasoning") && properties.field == "text" {
            self.send_update(
                &session.id,
                SessionUpdate::AgentThoughtChunk(ContentChunk {
                    message_id: Some(properties.message_id.clone()),
                    content: ContentBlock::Text(TextContent {
                        text: properties.delta.clone(),
                        annotations: None,
                    }),
                }),
            )
            .await;
        }
    }

    /// `fetchPartMetadata` from reference/packages/opencode/src/acp/event.ts.
    async fn fetch_part_metadata(
        &self,
        session_id: &str,
        cwd: &str,
        message_id: &str,
        part_id: &str,
    ) -> Option<crate::session::KnownMessagePartMetadata> {
        let message = self
            .sdk
            .session_message(cwd, session_id, message_id)
            .await
            .ok()?;
        let part = message
            .parts
            .iter()
            .find(|item| item.id() == Some(part_id))?;
        self.record_fetched_part(session_id, &message, part)
            .await
            .ok()
    }

    /// `recordFetchedPart` from reference/packages/opencode/src/acp/event.ts.
    async fn record_fetched_part(
        &self,
        session_id: &str,
        message: &SessionMessageResponse,
        part: &Part,
    ) -> Result<crate::session::KnownMessagePartMetadata, crate::error::ACPError> {
        self.session
            .record_part_metadata(RecordPartMetadataInput {
                session_id: session_id.to_string(),
                message_id: part.message_id().unwrap_or_default().to_string(),
                part_id: part.id().unwrap_or_default().to_string(),
                part_type: Some(part.part_type().to_string()),
                role: Some(message.info.role().to_string()),
                ignored: if part.part_type() == "text" {
                    part.ignored()
                } else {
                    None
                },
                tool_call_id: if part.part_type() == "tool" {
                    part.call_id().map(str::to_string)
                } else {
                    None
                },
                metadata: part.metadata().cloned().map(Value::Object),
            })
            .await
    }

    /// `handleToolPart` from reference/packages/opencode/src/acp/event.ts.
    async fn handle_tool_part(&self, session_id: &str, part: &ToolPart, cwd: &str) {
        self.tool_start(session_id, part, cwd).await;

        match &part.state {
            ToolState::Pending(_) => {
                self.shell_snapshots.lock().await.remove(&part.call_id);
            }
            ToolState::Running(state) => self.running_tool(session_id, part, state, cwd).await,
            ToolState::Completed(state) => {
                self.clear_tool(&part.call_id).await;
                self.send_update(
                    session_id,
                    SessionUpdate::ToolCallUpdate(tool::completed_tool_update(
                        tool::CompletedToolUpdateInput {
                            tool_call_id: part.call_id.clone(),
                            tool_name: &part.tool,
                            state: state.clone(),
                            cwd: Some(cwd),
                        },
                    )),
                )
                .await;
            }
            ToolState::Error(state) => {
                self.clear_tool(&part.call_id).await;
                self.send_update(
                    session_id,
                    SessionUpdate::ToolCallUpdate(tool::error_tool_update(
                        tool::ErrorToolUpdateInput {
                            tool_call_id: part.call_id.clone(),
                            tool_name: &part.tool,
                            state: state.clone(),
                            cwd: Some(cwd),
                        },
                    )),
                )
                .await;
            }
        }
    }

    /// `runningTool` from reference/packages/opencode/src/acp/event.ts.
    async fn running_tool(
        &self,
        session_id: &str,
        part: &ToolPart,
        state: &crate::sdk::ToolStateRunning,
        cwd: &str,
    ) {
        let output = if part.tool == "bash" {
            tool::shell_output_snapshot(state)
        } else {
            None
        };
        if let Some(output) = &output {
            let mut snapshots = self.shell_snapshots.lock().await;
            if snapshots.get(&part.call_id) == Some(output) {
                drop(snapshots);
                self.send_update(
                    session_id,
                    SessionUpdate::ToolCallUpdate(tool::duplicate_running_tool_update(
                        tool::RunningToolUpdateInput {
                            tool_call_id: part.call_id.clone(),
                            tool_name: &part.tool,
                            state: tool::running_state_from_tool_state(state),
                            output: None,
                            cwd: Some(cwd),
                        },
                    )),
                )
                .await;
                return;
            }
            snapshots.insert(part.call_id.clone(), output.clone());
        }

        self.send_update(
            session_id,
            SessionUpdate::ToolCallUpdate(tool::running_tool_update(
                tool::RunningToolUpdateInput {
                    tool_call_id: part.call_id.clone(),
                    tool_name: &part.tool,
                    state: tool::running_state_from_tool_state(state),
                    output,
                    cwd: Some(cwd),
                },
            )),
        )
        .await;
    }

    /// `toolStart` from reference/packages/opencode/src/acp/event.ts.
    async fn tool_start(&self, session_id: &str, part: &ToolPart, cwd: &str) {
        {
            let mut starts = self.tool_starts.lock().await;
            if !starts.insert(part.call_id.clone()) {
                return;
            }
        }
        let state = match &part.state {
            ToolState::Pending(state) => tool::RunningToolInput {
                input: state.input.clone(),
                title: None,
            },
            ToolState::Running(state) => tool::RunningToolInput {
                input: state.input.clone(),
                title: state.title.clone(),
            },
            ToolState::Completed(state) => tool::RunningToolInput {
                input: state.input.clone(),
                title: Some(state.title.clone()),
            },
            ToolState::Error(state) => tool::RunningToolInput {
                input: state.input.clone(),
                title: None,
            },
        };
        self.send_update(
            session_id,
            SessionUpdate::ToolCall(tool::pending_tool_call(tool::PendingToolCallInput {
                tool_call_id: part.call_id.clone(),
                tool_name: &part.tool,
                state,
                cwd: Some(cwd),
            })),
        )
        .await;
    }

    /// `clearTool` from reference/packages/opencode/src/acp/event.ts.
    async fn clear_tool(&self, tool_call_id: &str) {
        self.tool_starts.lock().await.remove(tool_call_id);
        self.shell_snapshots.lock().await.remove(tool_call_id);
    }

    async fn send_update(&self, session_id: &str, update: SessionUpdate) {
        if let Some(connection) = &self.connection {
            let _ = connection.session_update(session_id, update).await;
        }
    }
}

fn with_message_id(chunk: ContentChunk, message_id: &str) -> ContentChunk {
    ContentChunk {
        message_id: Some(message_id.to_string()),
        content: chunk.content,
    }
}

/// Convert an opencode `Part` into a `ReplayPart`.
fn replay_part(part: &Part) -> Option<ReplayPart> {
    match part {
        Part::Text(text) => Some(ReplayPart::Text {
            text: text.text.clone(),
            synthetic: text.synthetic,
            ignored: text.ignored,
        }),
        Part::File(file) => Some(ReplayPart::File {
            url: file.url.clone(),
            mime: file.mime.clone(),
            filename: file.filename.clone(),
        }),
        Part::Reasoning(reasoning) => Some(ReplayPart::Reasoning {
            text: reasoning.text.clone(),
        }),
        _ => None,
    }
}

/// Accessor helpers for raw `Part` values.
impl Part {
    fn session_id(&self) -> Option<&str> {
        match self {
            Part::Text(part) => Some(&part.session_id),
            Part::File(part) => Some(&part.session_id),
            Part::Reasoning(part) => Some(&part.session_id),
            Part::Tool(part) => Some(&part.session_id),
            Part::Other(value) => value.get("sessionID").and_then(Value::as_str),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdk::{AssistantMessage, CacheTokens, Message, TextPart, Tokens};
    use serde_json::Map;

    fn message_response() -> SessionMessageResponse {
        SessionMessageResponse {
            info: Message::Assistant(AssistantMessage {
                id: "m1".into(),
                session_id: "s1".into(),
                role: "assistant".into(),
                provider_id: "anthropic".into(),
                model_id: "claude".into(),
                mode: Some("build".into()),
                agent: Some("build".into()),
                cost: 0.5,
                tokens: Tokens {
                    input: 1,
                    output: 1,
                    reasoning: 0,
                    cache: CacheTokens { read: 0, write: 0 },
                },
                variant: None,
                error: None,
                path: Some(crate::sdk::MessagePath {
                    cwd: "/cwd".into(),
                    root: "/".into(),
                }),
                model: None,
            }),
            parts: vec![Part::Text(TextPart {
                id: "p1".into(),
                session_id: "s1".into(),
                message_id: "m1".into(),
                text: "hello".into(),
                synthetic: None,
                ignored: None,
                metadata: None,
            })],
        }
    }

    #[test]
    fn replay_part_conversion() {
        let part = Part::Text(TextPart {
            id: "p1".into(),
            session_id: "s1".into(),
            message_id: "m1".into(),
            text: "hi".into(),
            synthetic: None,
            ignored: None,
            metadata: None,
        });
        match replay_part(&part) {
            Some(ReplayPart::Text { text, .. }) => assert_eq!(text, "hi"),
            _ => panic!("expected text replay part"),
        }
    }

    #[test]
    fn part_accessors() {
        let part = Part::Tool(ToolPart {
            id: "p1".into(),
            session_id: "s1".into(),
            message_id: "m1".into(),
            call_id: "c1".into(),
            tool: "bash".into(),
            state: ToolState::Pending(crate::sdk::ToolStatePending {
                input: Map::new(),
                raw: String::new(),
            }),
            metadata: None,
        });
        assert_eq!(part.part_type(), "tool");
        assert_eq!(part.call_id(), Some("c1"));
        assert_eq!(part.session_id(), Some("s1"));
    }
}
