//! Credential storage abstraction.
//!
//! From reference/packages/core/src/credential.ts.
//!
//! TODO(integration): provide a SQLite-backed `CredentialStore` in oc-database
//! matching `credential/sql.ts`.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Map;

use crate::ids::{CredentialId, IntegrationId};

/// `Credential.OAuth` — `{ type: "oauth", methodID, refresh, access, expires,
/// metadata? }`.
/// From reference/packages/schema/src/credential.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth {
    #[serde(rename = "type")]
    pub kind: String,
    pub methodID: String,
    pub refresh: String,
    pub access: String,
    pub expires: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, serde_json::Value>>,
}

/// `Credential.Key` — `{ type: "key", key, metadata? }`.
/// From reference/packages/schema/src/credential.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    #[serde(rename = "type")]
    pub kind: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, serde_json::Value>>,
}

/// `Credential.Value` — tagged union on `type`.
/// From reference/packages/schema/src/credential.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Value {
    #[serde(rename = "oauth")]
    OAuth(OAuth),
    #[serde(rename = "key")]
    Key(Key),
}

/// `Credential.Info` — `{ id, integrationID, label, value }`.
/// From reference/packages/core/src/credential.ts
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialInfo {
    pub id: CredentialId,
    pub integrationID: IntegrationId,
    pub label: String,
    pub value: Value,
}

/// A stored credential row (label and value are never omitted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRow {
    pub id: String,
    pub integration_id: String,
    pub label: String,
    pub value: Value,
}

/// Storage seam for credentials.
/// TODO(integration): SQLite-backed impl (see `credential/sql.ts`).
pub trait CredentialStore: Send + Sync {
    /// Every stored credential, ordered by creation time ascending.
    fn all(&self) -> Vec<CredentialRow>;
    /// Credentials belonging to one integration, ordered by creation time.
    fn list(&self, integration_id: &str) -> Vec<CredentialRow>;
    fn get(&self, id: &str) -> Option<CredentialRow>;
    /// Replaces any credential for the integration and inserts `row`
    /// (mirrors the reference's delete-then-insert transaction).
    fn replace(&self, row: CredentialRow) -> Result<(), String>;
    /// Updates label and/or value for one credential.
    fn update(&self, id: &str, label: Option<String>, value: Option<Value>) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<(), String>;
}

/// In-memory credential store preserving insertion order.
#[derive(Debug, Default)]
pub struct InMemoryCredentialStore {
    rows: Mutex<Vec<CredentialRow>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn all(&self) -> Vec<CredentialRow> {
        self.rows.lock().unwrap().clone()
    }

    fn list(&self, integration_id: &str) -> Vec<CredentialRow> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.integration_id == integration_id)
            .cloned()
            .collect()
    }

    fn get(&self, id: &str) -> Option<CredentialRow> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .find(|row| row.id == id)
            .cloned()
    }

    fn replace(&self, row: CredentialRow) -> Result<(), String> {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|existing| existing.integration_id != row.integration_id);
        rows.push(row);
        Ok(())
    }

    fn update(&self, id: &str, label: Option<String>, value: Option<Value>) -> Result<(), String> {
        let mut rows = self.rows.lock().unwrap();
        let Some(existing) = rows.iter_mut().find(|row| row.id == id) else {
            return Err(format!("credential not found: {id}"));
        };
        if let Some(label) = label {
            existing.label = label;
        }
        if let Some(value) = value {
            existing.value = value;
        }
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|row| row.id != id);
        Ok(())
    }
}

/// The credential service (`@opencode/v2/Credential`).
#[derive(Clone)]
pub struct CredentialService {
    store: Arc<dyn CredentialStore>,
}

use std::sync::Arc;

impl CredentialService {
    pub fn new(store: Arc<dyn CredentialStore>) -> Self {
        CredentialService { store }
    }

    pub fn with_store<S: CredentialStore + 'static>(store: S) -> Self {
        CredentialService::new(Arc::new(store))
    }

    pub async fn all(&self) -> Vec<CredentialInfo> {
        self.store.all().into_iter().filter_map(stored).collect()
    }

    pub async fn list(&self, integration_id: &IntegrationId) -> Vec<CredentialInfo> {
        self.store
            .list(&integration_id.0)
            .into_iter()
            .filter_map(stored)
            .collect()
    }

    pub async fn get(&self, id: &CredentialId) -> Option<CredentialInfo> {
        self.store.get(&id.0).and_then(stored)
    }

    /// `create({ integrationID, value, label? })`.
    pub async fn create(&self, input: CreateInput) -> Result<CredentialInfo, String> {
        let credential = CredentialInfo {
            id: CredentialId::create(),
            integrationID: input.integration_id.clone(),
            label: input.label.unwrap_or_else(|| "default".to_string()),
            value: input.value,
        };
        self.store.replace(CredentialRow {
            id: credential.id.0.clone(),
            integration_id: credential.integrationID.0.clone(),
            label: credential.label.clone(),
            value: credential.value.clone(),
        })?;
        Ok(credential)
    }

    /// `update(id, updates)`.
    pub async fn update(
        &self,
        id: &CredentialId,
        label: Option<String>,
        value: Option<Value>,
    ) -> Result<(), String> {
        if label.is_none() && value.is_none() {
            return Ok(());
        }
        self.store.update(&id.0, label, value)
    }

    pub async fn remove(&self, id: &CredentialId) -> Result<(), String> {
        self.store.remove(&id.0)
    }
}

pub struct CreateInput {
    pub integration_id: IntegrationId,
    pub value: Value,
    pub label: Option<String>,
}

fn stored(row: CredentialRow) -> Option<CredentialInfo> {
    Some(CredentialInfo {
        id: CredentialId(row.id),
        integrationID: IntegrationId(row.integration_id),
        label: row.label,
        value: row.value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_value() -> Value {
        Value::Key(Key {
            kind: "key".to_string(),
            key: "secret".to_string(),
            metadata: None,
        })
    }

    #[tokio::test]
    async fn create_replaces_integration_credential() {
        let service = CredentialService::with_store(InMemoryCredentialStore::new());
        let integration = IntegrationId("anthropic".to_string());
        let first = service
            .create(CreateInput {
                integration_id: integration.clone(),
                value: key_value(),
                label: Some("one".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(first.label, "one");
        assert!(first.id.0.starts_with("cred_"));

        let second = service
            .create(CreateInput {
                integration_id: integration.clone(),
                value: key_value(),
                label: None,
            })
            .await
            .unwrap();
        assert_eq!(second.label, "default");
        let listed = service.list(&integration).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, second.id);
    }

    #[tokio::test]
    async fn update_and_remove() {
        let service = CredentialService::with_store(InMemoryCredentialStore::new());
        let integration = IntegrationId("x".to_string());
        let credential = service
            .create(CreateInput {
                integration_id: integration,
                value: key_value(),
                label: None,
            })
            .await
            .unwrap();
        service
            .update(&credential.id, Some("renamed".to_string()), None)
            .await
            .unwrap();
        assert_eq!(service.get(&credential.id).await.unwrap().label, "renamed");
        service.remove(&credential.id).await.unwrap();
        assert!(service.get(&credential.id).await.is_none());
    }

    #[test]
    fn value_json_shapes() {
        assert_eq!(
            serde_json::to_value(key_value()).unwrap(),
            serde_json::json!({ "type": "key", "key": "secret" })
        );
        let oauth = Value::OAuth(OAuth {
            kind: "oauth".to_string(),
            methodID: "m".to_string(),
            refresh: "r".to_string(),
            access: "a".to_string(),
            expires: 100,
            metadata: None,
        });
        assert_eq!(
            serde_json::to_value(oauth).unwrap(),
            serde_json::json!({ "type": "oauth", "methodID": "m", "refresh": "r", "access": "a", "expires": 100 })
        );
    }
}
