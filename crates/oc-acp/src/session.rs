//! In-memory ACP session store.
//!
//! From reference/packages/opencode/src/acp/session.ts. Holds the session state
//! (selected model, variant, mode, MCP servers) for active ACP sessions plus
//! metadata recorded from streamed message parts.

use std::collections::HashMap;

use tokio::sync::Mutex;

use crate::error::ACPError;
use crate::types::McpServer;

/// A selected model reference.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedModel {
    pub provider_id: String,
    pub model_id: String,
}

/// Metadata recorded about a message part seen while streaming.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownMessagePartMetadata {
    pub message_id: String,
    pub part_id: String,
    pub part_type: Option<String>,
    pub role: Option<String>,
    pub ignored: Option<bool>,
    pub tool_call_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Session state.
#[derive(Debug, Clone)]
pub struct Info {
    pub id: String,
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
    pub created_at: i64,
    pub model: Option<SelectedModel>,
    pub variant: Option<String>,
    pub mode_id: Option<String>,
    pub known_parts: HashMap<String, KnownMessagePartMetadata>,
}

/// Input for storing a session.
#[derive(Debug, Clone)]
pub struct StoreInput {
    pub id: String,
    pub cwd: String,
    pub mcp_servers: Option<Vec<McpServer>>,
    pub created_at: Option<i64>,
    pub model: Option<SelectedModel>,
    pub variant: Option<String>,
    pub mode_id: Option<String>,
}

/// Input for recording part metadata.
#[derive(Debug, Clone)]
pub struct RecordPartMetadataInput {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub part_type: Option<String>,
    pub role: Option<String>,
    pub ignored: Option<bool>,
    pub tool_call_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Lookup key for part metadata.
#[derive(Debug, Clone)]
pub struct PartMetadataLookupInput {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
}

/// The ACP session service.
pub struct Service {
    sessions: Mutex<HashMap<String, Info>>,
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Service {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// `store` from reference/packages/opencode/src/acp/session.ts (used by both
    /// `create` and `load`).
    pub async fn store(&self, input: StoreInput) -> Info {
        let session = make_session(input);
        let snapshot = snapshot(&session);
        self.sessions
            .lock()
            .await
            .insert(session.id.clone(), session);
        snapshot
    }

    /// `create` from reference/packages/opencode/src/acp/session.ts.
    pub async fn create(&self, input: StoreInput) -> Info {
        self.store(input).await
    }

    /// `load` from reference/packages/opencode/src/acp/session.ts.
    pub async fn load(&self, input: StoreInput) -> Info {
        self.store(input).await
    }

    /// `tryGet` from reference/packages/opencode/src/acp/session.ts.
    pub async fn try_get(&self, session_id: &str) -> Option<Info> {
        self.sessions.lock().await.get(session_id).map(snapshot)
    }

    /// `get` from reference/packages/opencode/src/acp/session.ts.
    pub async fn get(&self, session_id: &str) -> Result<Info, ACPError> {
        self.try_get(session_id)
            .await
            .ok_or_else(|| ACPError::SessionNotFound {
                session_id: session_id.to_string(),
            })
    }

    /// `update` from reference/packages/opencode/src/acp/session.ts.
    async fn update(
        &self,
        session_id: &str,
        f: impl FnOnce(Info) -> Info,
    ) -> Result<Info, ACPError> {
        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get(session_id) else {
            return Err(ACPError::SessionNotFound {
                session_id: session_id.to_string(),
            });
        };
        let next = f(snapshot(session));
        let snapshot = snapshot(&next);
        sessions.insert(session_id.to_string(), next);
        Ok(snapshot)
    }

    /// `list` from reference/packages/opencode/src/acp/session.ts.
    pub async fn list(&self, cwd: Option<&str>) -> Vec<Info> {
        let mut sessions: Vec<Info> = self
            .sessions
            .lock()
            .await
            .values()
            .filter(|session| cwd.map_or(true, |cwd| session.cwd == cwd))
            .map(snapshot)
            .collect();
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        sessions
    }

    /// `remove` from reference/packages/opencode/src/acp/session.ts.
    pub async fn remove(&self, session_id: &str) -> Option<Info> {
        let mut sessions = self.sessions.lock().await;
        sessions
            .remove(session_id)
            .map(|session| snapshot(&session))
    }

    /// `setModel` from reference/packages/opencode/src/acp/session.ts.
    pub async fn set_model(
        &self,
        session_id: &str,
        model: Option<SelectedModel>,
    ) -> Result<Info, ACPError> {
        self.update(session_id, |mut session| {
            session.model = model;
            session
        })
        .await
    }

    /// `getModel` from reference/packages/opencode/src/acp/session.ts.
    pub async fn get_model(&self, session_id: &str) -> Result<Option<SelectedModel>, ACPError> {
        Ok(self.get(session_id).await?.model)
    }

    /// `setVariant` from reference/packages/opencode/src/acp/session.ts.
    pub async fn set_variant(
        &self,
        session_id: &str,
        variant: Option<String>,
    ) -> Result<Info, ACPError> {
        self.update(session_id, |mut session| {
            session.variant = variant;
            session
        })
        .await
    }

    /// `getVariant` from reference/packages/opencode/src/acp/session.ts.
    pub async fn get_variant(&self, session_id: &str) -> Result<Option<String>, ACPError> {
        Ok(self.get(session_id).await?.variant)
    }

    /// `setMode` from reference/packages/opencode/src/acp/session.ts.
    pub async fn set_mode(
        &self,
        session_id: &str,
        mode_id: Option<String>,
    ) -> Result<Info, ACPError> {
        self.update(session_id, |mut session| {
            session.mode_id = mode_id;
            session
        })
        .await
    }

    /// `getMode` from reference/packages/opencode/src/acp/session.ts.
    pub async fn get_mode(&self, session_id: &str) -> Result<Option<String>, ACPError> {
        Ok(self.get(session_id).await?.mode_id)
    }

    /// `recordPartMetadata` from reference/packages/opencode/src/acp/session.ts.
    pub async fn record_part_metadata(
        &self,
        input: RecordPartMetadataInput,
    ) -> Result<KnownMessagePartMetadata, ACPError> {
        let metadata = KnownMessagePartMetadata {
            message_id: input.message_id.clone(),
            part_id: input.part_id.clone(),
            part_type: input.part_type.clone(),
            role: input.role.clone(),
            ignored: input.ignored,
            tool_call_id: input.tool_call_id.clone(),
            metadata: input.metadata.clone(),
        };
        let key = part_metadata_key(&input.message_id, &input.part_id);
        let metadata_for_store = metadata.clone();
        let session_id = input.session_id.clone();
        self.update(&session_id, |mut session| {
            session.known_parts.insert(key, metadata_for_store);
            session
        })
        .await?;
        Ok(metadata)
    }

    /// `getPartMetadata` from reference/packages/opencode/src/acp/session.ts.
    pub async fn get_part_metadata(
        &self,
        input: &PartMetadataLookupInput,
    ) -> Result<Option<KnownMessagePartMetadata>, ACPError> {
        Ok(self
            .get(&input.session_id)
            .await?
            .known_parts
            .get(&part_metadata_key(&input.message_id, &input.part_id))
            .cloned())
    }

    /// `tryGetPartMetadata` from reference/packages/opencode/src/acp/session.ts.
    pub async fn try_get_part_metadata(
        &self,
        input: &PartMetadataLookupInput,
    ) -> Option<KnownMessagePartMetadata> {
        self.try_get(&input.session_id)
            .await?
            .known_parts
            .get(&part_metadata_key(&input.message_id, &input.part_id))
            .cloned()
    }
}

fn make_session(input: StoreInput) -> Info {
    Info {
        id: input.id,
        cwd: input.cwd,
        mcp_servers: input.mcp_servers.unwrap_or_default(),
        created_at: input.created_at.unwrap_or_else(now_ms),
        model: input.model,
        variant: input.variant,
        mode_id: input.mode_id,
        known_parts: HashMap::new(),
    }
}

fn snapshot(session: &Info) -> Info {
    Info {
        id: session.id.clone(),
        cwd: session.cwd.clone(),
        mcp_servers: session.mcp_servers.clone(),
        created_at: session.created_at,
        model: session.model.clone(),
        variant: session.variant.clone(),
        mode_id: session.mode_id.clone(),
        known_parts: session.known_parts.clone(),
    }
}

fn part_metadata_key(message_id: &str, part_id: &str) -> String {
    format!("{message_id}:{part_id}")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_metadata_key_format() {
        assert_eq!(part_metadata_key("m1", "p1"), "m1:p1");
    }

    #[tokio::test]
    async fn store_and_get() {
        let service = Service::new();
        let info = service
            .create(StoreInput {
                id: "s1".into(),
                cwd: "/tmp".into(),
                mcp_servers: None,
                created_at: Some(1),
                model: None,
                variant: None,
                mode_id: None,
            })
            .await;
        assert_eq!(info.id, "s1");
        let got = service.get("s1").await.unwrap();
        assert_eq!(got.cwd, "/tmp");

        let missing = service.get("nope").await;
        assert!(matches!(missing, Err(ACPError::SessionNotFound { .. })));
    }

    #[tokio::test]
    async fn update_and_remove() {
        let service = Service::new();
        service
            .create(StoreInput {
                id: "s1".into(),
                cwd: "/tmp".into(),
                mcp_servers: None,
                created_at: Some(1),
                model: None,
                variant: None,
                mode_id: None,
            })
            .await;
        let model = SelectedModel {
            provider_id: "anthropic".into(),
            model_id: "claude".into(),
        };
        let updated = service.set_model("s1", Some(model.clone())).await.unwrap();
        assert_eq!(updated.model, Some(model.clone()));
        assert_eq!(service.get_model("s1").await.unwrap(), Some(model));
        let removed = service.remove("s1").await.unwrap();
        assert_eq!(removed.id, "s1");
        assert!(service.try_get("s1").await.is_none());
    }

    #[tokio::test]
    async fn record_and_lookup_part_metadata() {
        let service = Service::new();
        service
            .create(StoreInput {
                id: "s1".into(),
                cwd: "/tmp".into(),
                mcp_servers: None,
                created_at: Some(1),
                model: None,
                variant: None,
                mode_id: None,
            })
            .await;
        let metadata = service
            .record_part_metadata(RecordPartMetadataInput {
                session_id: "s1".into(),
                message_id: "m1".into(),
                part_id: "p1".into(),
                part_type: Some("text".into()),
                role: Some("assistant".into()),
                ignored: Some(false),
                tool_call_id: None,
                metadata: None,
            })
            .await
            .unwrap();
        assert_eq!(metadata.role.as_deref(), Some("assistant"));
        let lookup = service
            .try_get_part_metadata(&PartMetadataLookupInput {
                session_id: "s1".into(),
                message_id: "m1".into(),
                part_id: "p1".into(),
            })
            .await
            .unwrap();
        assert_eq!(lookup.part_type.as_deref(), Some("text"));
    }

    #[tokio::test]
    async fn list_sorted_by_created_at() {
        let service = Service::new();
        for (id, created_at) in [("a", 10), ("b", 30), ("c", 20)] {
            service
                .create(StoreInput {
                    id: id.into(),
                    cwd: "/tmp".into(),
                    mcp_servers: None,
                    created_at: Some(created_at),
                    model: None,
                    variant: None,
                    mode_id: None,
                })
                .await;
        }
        let list = service.list(None).await;
        let ids: Vec<&str> = list.iter().map(|session| session.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
    }
}
