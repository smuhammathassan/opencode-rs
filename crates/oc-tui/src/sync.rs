//! Session/message data store maintained from server events.
//!
//! Mirrors `reference/packages/tui/src/context/sync.tsx` (the `SyncProvider`
//! store and its `event.subscribe` reducer).

use std::collections::HashMap;

use crate::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncStatus {
    #[default]
    Loading,
    Partial,
    Complete,
}

impl SyncStatus {
    pub fn is_ready(&self) -> bool {
        !matches!(self, SyncStatus::Loading)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncState {
    pub status: SyncStatus,
    pub providers: Vec<Provider>,
    pub agents: Vec<Agent>,
    pub commands: Vec<Command>,
    pub skills: Vec<Skill>,
    pub config: Config,
    pub sessions: Vec<Session>,
    pub messages: HashMap<String, Vec<Message>>,
    pub parts: HashMap<String, Vec<Part>>,
    pub permissions: HashMap<String, Vec<PermissionRequest>>,
    pub questions: HashMap<String, Vec<QuestionRequest>>,
    pub session_status: HashMap<String, SessionStatus>,
    pub todos: HashMap<String, Vec<Todo>>,
    pub session_diff: HashMap<String, Vec<SnapshotFileDiff>>,
    pub queued_prompts: HashMap<String, Vec<QueuedPrompt>>,
    pub capabilities: ExperimentalCapabilities,
    pub console_state: ConsoleState,
}

const MAX_MESSAGES: usize = 100;

impl SyncState {
    pub fn session(&self, id: &str) -> Option<&Session> {
        binary_search(&self.sessions, id, |s| s.id.clone()).map(|i| &self.sessions[i])
    }

    pub fn messages_for(&self, session_id: &str) -> &[Message] {
        self.messages
            .get(session_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn parts_for(&self, message_id: &str) -> &[Part] {
        self.parts
            .get(message_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn last_assistant(&self, session_id: &str) -> Option<&AssistantMessage> {
        self.messages_for(session_id)
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::Assistant(a) => Some(a),
                _ => None,
            })
    }

    pub fn last_user(&self, session_id: &str) -> Option<&UserMessage> {
        self.messages_for(session_id)
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User(u) => Some(u),
                _ => None,
            })
    }

    /// `session.status`-style derived status for a session.
    /// From reference/packages/tui/src/context/sync.tsx (`session.status`)
    pub fn session_status_derived(&self, session_id: &str) -> &'static str {
        let Some(session) = self.session(session_id) else {
            return "idle";
        };
        if session.time.compacting.is_some() {
            return "compacting";
        }
        let messages = self.messages_for(session_id);
        let Some(last) = messages.last() else {
            return "idle";
        };
        match last {
            Message::User(_) => "working",
            Message::Assistant(a) => {
                if a.time.completed.is_some() {
                    "idle"
                } else {
                    "working"
                }
            }
        }
    }

    /// Apply a global event to the store, mirroring the sync reducer.
    pub fn apply_event(&mut self, event: &GlobalEvent) {
        let payload = &event.payload;
        let props = &payload.properties;
        match payload.r#type.as_str() {
            "session.created" | "session.updated" => {
                if let Some(info) = props.get("info") {
                    if let Ok(info) = serde_json::from_value::<Session>(info.clone()) {
                        self.upsert_session(info);
                    }
                }
            }
            "session.deleted" => {
                if let Some(info) = props.get("info") {
                    if let Ok(info) = serde_json::from_value::<Session>(info.clone()) {
                        if let Some(idx) = binary_search(&self.sessions, &info.id, |s| s.id.clone())
                        {
                            self.sessions.remove(idx);
                        }
                    }
                }
            }
            "session.next.moved" => {
                let session_id = props.get("sessionID").and_then(v_str).map(str::to_string);
                let directory = props
                    .get("location")
                    .and_then(|l| l.get("directory"))
                    .and_then(v_str);
                let workspace_id = props
                    .get("location")
                    .and_then(|l| l.get("workspaceID"))
                    .and_then(v_str);
                let subpath = props
                    .get("subdirectory")
                    .and_then(v_str)
                    .map(str::to_string);
                if let (Some(session_id), Some(directory)) = (session_id, directory) {
                    if let Some(idx) = binary_search(&self.sessions, &session_id, |s| s.id.clone())
                    {
                        let session = &mut self.sessions[idx];
                        session.directory = directory.to_string();
                        session.path = subpath;
                        session.workspace_id = workspace_id.map(str::to_string);
                    }
                }
            }
            "session.next.prompt.admitted" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let message_id = props.get("messageID").and_then(v_str);
                let delivery = props.get("delivery").and_then(v_str);
                let prompt = props.get("prompt").cloned();
                let timestamp = props
                    .get("timestamp")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_default();
                if let (Some(session_id), Some(message_id), Some(delivery), Some(prompt)) =
                    (session_id, message_id, delivery, prompt)
                {
                    if delivery == "queue" {
                        let queued = self
                            .queued_prompts
                            .entry(session_id.to_string())
                            .or_default();
                        if !queued.iter().any(|item| item.id == message_id) {
                            queued.push(QueuedPrompt {
                                id: message_id.to_string(),
                                session_id: session_id.to_string(),
                                prompt,
                                timestamp,
                            });
                            queued.sort_by(|a, b| {
                                a.timestamp.cmp(&b.timestamp).then_with(|| a.id.cmp(&b.id))
                            });
                        }
                    }
                }
            }
            "session.status" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let status = props
                    .get("status")
                    .and_then(|v| serde_json::from_value::<SessionStatus>(v.clone()).ok());
                if let (Some(session_id), Some(status)) = (session_id, status) {
                    if status.kind() == "idle" {
                        self.queued_prompts.remove(session_id);
                    }
                    self.session_status.insert(session_id.to_string(), status);
                }
            }
            "message.updated" => {
                let info = props
                    .get("info")
                    .and_then(|v| serde_json::from_value::<Message>(v.clone()).ok());
                if let Some(info) = info {
                    let session_id = info.session_id().to_string();
                    self.upsert_message(&session_id, info);
                    let oldest_id = self
                        .messages
                        .get(&session_id)
                        .and_then(|messages| {
                            (messages.len() > MAX_MESSAGES)
                                .then(|| messages.first().map(|m| m.id().to_string()))
                        })
                        .flatten();
                    if let Some(oldest_id) = oldest_id {
                        if let Some(messages) = self.messages.get_mut(&session_id) {
                            messages.remove(0);
                        }
                        self.parts.remove(&oldest_id);
                    }
                }
            }
            "message.removed" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let message_id = props.get("messageID").and_then(v_str);
                if let (Some(session_id), Some(message_id)) = (session_id, message_id) {
                    if let Some(messages) = self.messages.get_mut(session_id) {
                        if let Some(idx) =
                            binary_search(messages, message_id, |m| m.id().to_string())
                        {
                            messages.remove(idx);
                        }
                    }
                }
            }
            "message.part.updated" => {
                let part = props
                    .get("part")
                    .and_then(|v| serde_json::from_value::<Part>(v.clone()).ok());
                if let Some(part) = part {
                    let message_id = part.message_id().to_string();
                    self.upsert_part(&message_id, part);
                }
            }
            "message.part.delta" => {
                let message_id = props.get("messageID").and_then(v_str);
                let part_id = props.get("partID").and_then(v_str);
                let field = props.get("field").and_then(v_str);
                let delta = props.get("delta").and_then(v_str);
                if let (Some(message_id), Some(part_id), Some(field), Some(delta)) =
                    (message_id, part_id, field, delta)
                {
                    if let Some(parts) = self.parts.get_mut(message_id) {
                        if let Some(idx) = binary_search(parts, part_id, |p| p.id().to_string()) {
                            apply_delta(&mut parts[idx], field, delta);
                        }
                    }
                }
            }
            "message.part.removed" => {
                let message_id = props.get("messageID").and_then(v_str);
                let part_id = props.get("partID").and_then(v_str);
                if let (Some(message_id), Some(part_id)) = (message_id, part_id) {
                    if let Some(parts) = self.parts.get_mut(message_id) {
                        if let Some(idx) = binary_search(parts, part_id, |p| p.id().to_string()) {
                            parts.remove(idx);
                        }
                    }
                }
            }
            "permission.asked" => {
                if let Ok(request) = serde_json::from_value::<PermissionRequest>(props.clone()) {
                    self.insert_permission(request);
                }
            }
            "permission.replied" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let request_id = props.get("requestID").and_then(v_str);
                if let (Some(session_id), Some(request_id)) = (session_id, request_id) {
                    if let Some(requests) = self.permissions.get_mut(session_id) {
                        if let Some(idx) = binary_search(requests, request_id, |r| r.id.clone()) {
                            requests.remove(idx);
                        }
                    }
                }
            }
            "question.asked" => {
                if let Ok(request) = serde_json::from_value::<QuestionRequest>(props.clone()) {
                    self.insert_question(request);
                }
            }
            "question.replied" | "question.rejected" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let request_id = props.get("requestID").and_then(v_str);
                if let (Some(session_id), Some(request_id)) = (session_id, request_id) {
                    if let Some(requests) = self.questions.get_mut(session_id) {
                        if let Some(idx) = binary_search(requests, request_id, |r| r.id.clone()) {
                            requests.remove(idx);
                        }
                    }
                }
            }
            "todo.updated" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let todos = props
                    .get("todos")
                    .and_then(|v| serde_json::from_value::<Vec<Todo>>(v.clone()).ok());
                if let (Some(session_id), Some(todos)) = (session_id, todos) {
                    self.todos.insert(session_id.to_string(), todos);
                }
            }
            "session.diff" => {
                let session_id = props.get("sessionID").and_then(v_str);
                let diff = props
                    .get("diff")
                    .and_then(|v| serde_json::from_value::<Vec<SnapshotFileDiff>>(v.clone()).ok());
                if let (Some(session_id), Some(diff)) = (session_id, diff) {
                    self.session_diff.insert(session_id.to_string(), diff);
                }
            }
            "server.connected"
            | "server.instance.disposed"
            | "lsp.updated"
            | "vcs.branch.updated"
            | "file.edited"
            | "reference.updated"
            | "plugin.added"
            | "session.idle"
            | "session.compacted"
            | "mcp.tools.changed"
            | "installation.updated"
            | "sync" => {}
            _ => {
                // Unknown event types are ignored, mirroring the switch default.
            }
        }
    }

    pub fn upsert_session(&mut self, info: Session) {
        let id = info.id.clone();
        match binary_search(&self.sessions, &id, |s| s.id.clone()) {
            Some(idx) => self.sessions[idx] = info,
            None => {
                let idx = binary_search_insert(&self.sessions, &id, |s| s.id.clone());
                self.sessions.insert(idx, info);
            }
        }
    }

    pub fn upsert_message(&mut self, session_id: &str, info: Message) {
        let messages = self.messages.entry(session_id.to_string()).or_default();
        let id = info.id().to_string();
        match binary_search(messages, &id, |m| m.id().to_string()) {
            Some(idx) => messages[idx] = info,
            None => {
                let idx = binary_search_insert(messages, &id, |m| m.id().to_string());
                messages.insert(idx, info);
            }
        }
    }

    pub fn upsert_part(&mut self, message_id: &str, part: Part) {
        let parts = self.parts.entry(message_id.to_string()).or_default();
        let id = part.id().to_string();
        match binary_search(parts, &id, |p| p.id().to_string()) {
            Some(idx) => parts[idx] = part,
            None => {
                let idx = binary_search_insert(parts, &id, |p| p.id().to_string());
                parts.insert(idx, part);
            }
        }
    }

    pub fn insert_permission(&mut self, request: PermissionRequest) {
        let requests = self
            .permissions
            .entry(request.session_id.clone())
            .or_default();
        let id = request.id.clone();
        match binary_search(requests, &id, |r| r.id.clone()) {
            Some(idx) => requests[idx] = request,
            None => {
                let idx = binary_search_insert(requests, &id, |r| r.id.clone());
                requests.insert(idx, request);
            }
        }
    }

    pub fn insert_question(&mut self, request: QuestionRequest) {
        let requests = self
            .questions
            .entry(request.session_id.clone())
            .or_default();
        let id = request.id.clone();
        match binary_search(requests, &id, |r| r.id.clone()) {
            Some(idx) => requests[idx] = request,
            None => {
                let idx = binary_search_insert(requests, &id, |r| r.id.clone());
                requests.insert(idx, request);
            }
        }
    }

    pub fn replace_sessions(&mut self, sessions: Vec<Session>) {
        self.sessions = sessions;
    }

    pub fn sync_session_data(&mut self, session: Session, messages: Vec<SessionMessageData>) {
        let session_id = session.id.clone();
        self.upsert_session(session);
        let mut list = Vec::with_capacity(messages.len());
        for data in messages {
            let SessionMessageData { info, parts } = data;
            let message_id = info.id().to_string();
            self.parts.insert(message_id, parts);
            list.push(info);
        }
        list.sort_by(|a, b| a.id().cmp(b.id()));
        if list.len() > MAX_MESSAGES {
            let keep = list.len() - MAX_MESSAGES;
            for removed in list.drain(..keep) {
                let removed_id = removed.id().to_string();
                self.parts.remove(&removed_id);
            }
        }
        self.messages.insert(session_id, list);
    }
}

fn v_str(v: &serde_json::Value) -> Option<&str> {
    v.as_str()
}

/// Append a `message.part.delta` to a part's string field.
/// From reference/packages/tui/src/context/sync.tsx (`message.part.delta`)
fn apply_delta(part: &mut Part, field: &str, delta: &str) {
    match part {
        Part::Text(t) if field == "text" => t.text.push_str(delta),
        Part::Reasoning(r) if field == "text" => r.text.push_str(delta),
        _ => {}
    }
}

/// Binary search over a sorted slice. Returns index of the item.
fn binary_search<T, K: AsRef<str>>(
    items: &[T],
    key: &str,
    key_of: impl Fn(&T) -> K,
) -> Option<usize> {
    let mut lo = 0usize;
    let mut hi = items.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        let value = key_of(&items[mid]);
        match value.as_ref().cmp(key) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Binary search for an insert position.
fn binary_search_insert<T, K: AsRef<str>>(
    items: &[T],
    key: &str,
    key_of: impl Fn(&T) -> K,
) -> usize {
    let mut lo = 0usize;
    let mut hi = items.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        let value = key_of(&items[mid]);
        if value.as_ref() < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(type_: &str, properties: serde_json::Value) -> GlobalEvent {
        GlobalEvent {
            directory: "/tmp".to_string(),
            project: None,
            workspace: None,
            payload: EventPayload {
                id: "evt_1".to_string(),
                r#type: type_.to_string(),
                properties,
            },
        }
    }

    fn part_json(id: &str, message_id: &str, text: &str) -> serde_json::Value {
        json!({
            "id": id,
            "sessionID": "ses_1",
            "messageID": message_id,
            "type": "text",
            "text": text
        })
    }

    #[test]
    fn session_upsert_and_remove() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "session.created",
            json!({ "sessionID": "ses_1", "info": {
                "id": "ses_1", "slug": "s", "projectID": "p", "directory": "/a", "title": "T",
                "version": "1", "time": { "created": 1, "updated": 1 }
            }}),
        ));
        assert!(sync.session("ses_1").is_some());
        sync.apply_event(&event(
            "session.deleted",
            json!({ "sessionID": "ses_1", "info": {
                "id": "ses_1", "slug": "s", "projectID": "p", "directory": "/a", "title": "T",
                "version": "1", "time": { "created": 1, "updated": 1 }
            }}),
        ));
        assert!(sync.session("ses_1").is_none());
    }

    #[test]
    fn message_upsert_sorted() {
        let mut sync = SyncState::default();
        for (id, text) in [("msg_b", "second"), ("msg_a", "first"), ("msg_c", "third")] {
            sync.apply_event(&event(
                "message.updated",
                json!({ "sessionID": "ses_1", "info": {
                    "id": id, "sessionID": "ses_1", "role": "user", "agent": "build",
                    "model": { "id": "m", "providerID": "p" },
                    "time": { "created": 1 }
                }}),
            ));
            sync.apply_event(&event(
                "message.part.updated",
                json!({ "sessionID": "ses_1", "part": part_json(&format!("{id}_p1"), id, text) }),
            ));
        }
        let messages = sync.messages_for("ses_1");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].id(), "msg_a");
        assert_eq!(messages[2].id(), "msg_c");
        assert_eq!(sync.parts_for("msg_b").len(), 1);
    }

    #[test]
    fn part_delta_appends() {
        let mut sync = SyncState::default();
        let part = part_json("pt_1", "msg_1", "");
        sync.upsert_part("msg_1", serde_json::from_value(part).unwrap());
        sync.apply_event(&event(
            "message.part.delta",
            json!({ "sessionID": "ses_1", "messageID": "msg_1", "partID": "pt_1", "field": "text", "delta": "hel" }),
        ));
        sync.apply_event(&event(
            "message.part.delta",
            json!({ "sessionID": "ses_1", "messageID": "msg_1", "partID": "pt_1", "field": "text", "delta": "lo" }),
        ));
        let parts = sync.parts_for("msg_1");
        match &parts[0] {
            Part::Text(t) => assert_eq!(t.text, "hello"),
            _ => panic!("expected text part"),
        }
    }

    #[test]
    fn permission_flow() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "permission.asked",
            json!({ "id": "per_1", "sessionID": "ses_1", "permission": "bash", "patterns": [], "metadata": {}, "always": [] }),
        ));
        assert_eq!(sync.permissions.get("ses_1").unwrap().len(), 1);
        sync.apply_event(&event(
            "permission.replied",
            json!({ "sessionID": "ses_1", "requestID": "per_1", "reply": "once" }),
        ));
        assert!(sync.permissions.get("ses_1").unwrap().is_empty());
    }

    #[test]
    fn question_flow() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "question.asked",
            json!({ "id": "que_1", "sessionID": "ses_1", "questions": [{ "question": "q", "header": "h", "options": [] }] }),
        ));
        assert_eq!(sync.questions.get("ses_1").unwrap().len(), 1);
        sync.apply_event(&event(
            "question.rejected",
            json!({ "sessionID": "ses_1", "requestID": "que_1" }),
        ));
        assert!(sync.questions.get("ses_1").unwrap().is_empty());
    }

    #[test]
    fn todo_and_diff() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "todo.updated",
            json!({ "sessionID": "ses_1", "todos": [{ "content": "x", "status": "pending", "priority": "low" }] }),
        ));
        assert_eq!(sync.todos.get("ses_1").unwrap()[0].content, "x");
        sync.apply_event(&event(
            "session.diff",
            json!({ "sessionID": "ses_1", "diff": [{ "additions": 1, "deletions": 0 }] }),
        ));
        assert_eq!(sync.session_diff.get("ses_1").unwrap()[0].additions, 1);
    }

    #[test]
    fn session_status_event() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "session.status",
            json!({ "sessionID": "ses_1", "status": { "type": "retry", "attempt": 2, "message": "boom", "next": 123 } }),
        ));
        assert_eq!(sync.session_status.get("ses_1").unwrap().kind(), "retry");
    }

    #[test]
    fn queued_prompt_event_is_sorted_and_cleared_when_idle() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "session.next.prompt.admitted",
            json!({
                "sessionID": "ses_1", "messageID": "msg_b", "timestamp": 20,
                "delivery": "queue", "prompt": { "text": "second" }
            }),
        ));
        sync.apply_event(&event(
            "session.next.prompt.admitted",
            json!({
                "sessionID": "ses_1", "messageID": "msg_a", "timestamp": 10,
                "delivery": "queue", "prompt": { "text": "first" }
            }),
        ));
        assert_eq!(
            sync.queued_prompts["ses_1"]
                .iter()
                .map(|item| item.summary())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        sync.apply_event(&event(
            "session.status",
            json!({ "sessionID": "ses_1", "status": { "type": "idle" } }),
        ));
        assert!(!sync.queued_prompts.contains_key("ses_1"));
    }

    #[test]
    fn steer_prompt_is_not_shown_as_queued() {
        let mut sync = SyncState::default();
        sync.apply_event(&event(
            "session.next.prompt.admitted",
            json!({
                "sessionID": "ses_1", "messageID": "msg_a", "timestamp": 10,
                "delivery": "steer", "prompt": { "text": "interrupt" }
            }),
        ));
        assert!(sync.queued_prompts.is_empty());
    }

    #[test]
    fn sync_session_data_replaces_messages() {
        let mut sync = SyncState::default();
        let info: Message = serde_json::from_value(json!({
            "id": "msg_1", "sessionID": "ses_1", "role": "user", "agent": "build",
            "model": { "id": "m", "providerID": "p" }, "time": { "created": 1 }
        }))
        .unwrap();
        let session: Session = serde_json::from_value(json!({
            "id": "ses_1", "slug": "s", "projectID": "p", "directory": "/a", "title": "T",
            "version": "1", "time": { "created": 1, "updated": 1 }
        }))
        .unwrap();
        sync.sync_session_data(
            session,
            vec![SessionMessageData {
                info,
                parts: vec![serde_json::from_value(part_json("pt_1", "msg_1", "hi")).unwrap()],
            }],
        );
        assert_eq!(sync.messages_for("ses_1").len(), 1);
        assert!(sync.session("ses_1").is_some());
        assert_eq!(sync.parts_for("msg_1").len(), 1);
    }

    #[test]
    fn derived_status() {
        let mut sync = SyncState::default();
        let session: Session = serde_json::from_value(json!({
            "id": "ses_1", "slug": "s", "projectID": "p", "directory": "/a", "title": "T",
            "version": "1", "time": { "created": 1, "updated": 1 }
        }))
        .unwrap();
        sync.upsert_session(session);
        assert_eq!(sync.session_status_derived("ses_1"), "idle");

        let info: Message = serde_json::from_value(json!({
            "id": "msg_1", "sessionID": "ses_1", "role": "assistant",
            "time": { "created": 1 },
            "parentID": "msg_0", "modelID": "m", "providerID": "p", "mode": "primary",
            "agent": "build", "path": { "cwd": "/a", "root": "/a" }, "cost": 0,
            "tokens": { "input": 0, "output": 0, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
        }))
        .unwrap();
        sync.upsert_message("ses_1", info);
        assert_eq!(sync.session_status_derived("ses_1"), "working");
    }
}
