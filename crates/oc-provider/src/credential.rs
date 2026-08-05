//! V2 integration credential store.
//!
//! From reference/packages/core/src/credential.ts and
//! reference/packages/schema/src/credential.ts.
//!
//! The reference persists credentials in SQLite (`credential` table, see
//! `core/src/credential/sql.ts`). This crate owns the credential abstraction
//! (types + store trait) so `oc-provider` is canonical; the SQLite binding
//! lives in `oc-database`.
//!
//! TODO(integration): bind [`CredentialStore`] to `oc-database`'s SQLite
//! implementation of the `credential` table (insert/delete-by-integration for
//! `create`, update by id, order by `time_created`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `Credential.ID` from `schema/credential.ts`: `cred_` + ascending counter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

static ASCENDING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Id {
    /// `ascending()` counter from `schema/identifier.ts`.
    pub fn create() -> Id {
        let n = ASCENDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Id(format!("cred_{}", n + 1))
    }
}

/// `Credential.OAuth` from `schema/credential.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth {
    pub method_id: String,
    pub refresh: String,
    pub access: String,
    pub expires: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

/// `Credential.Key` from `schema/credential.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

/// `Credential.Value` from `schema/credential.ts`, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Value {
    OAuth(OAuth),
    Key(Key),
}

/// `Credential.Info` from `core/credential.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: Id,
    pub integration_id: String,
    pub label: String,
    pub value: Value,
}

/// `Credential.Service` from `core/credential.ts`.
pub trait CredentialStore: Send + Sync {
    fn all(&self) -> Result<Vec<Info>, CredentialError>;
    fn list(&self, integration_id: &str) -> Result<Vec<Info>, CredentialError>;
    fn get(&self, id: &Id) -> Result<Option<Info>, CredentialError>;
    fn create(
        &mut self,
        integration_id: &str,
        value: Value,
        label: Option<&str>,
    ) -> Result<Info, CredentialError>;
    fn update(&mut self, id: &Id, updates: CredentialUpdate) -> Result<(), CredentialError>;
    fn remove(&mut self, id: &Id) -> Result<(), CredentialError>;
}

/// Partial updates for `CredentialStore::update`.
#[derive(Debug, Clone, Default)]
pub struct CredentialUpdate {
    pub label: Option<String>,
    pub value: Option<Value>,
}

impl CredentialUpdate {
    pub fn is_empty(&self) -> bool {
        self.label.is_none() && self.value.is_none()
    }
}

/// Errors surfaced by [`CredentialStore`].
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("{0}")]
    Failed(String),
}

/// An in-memory [`CredentialStore`] used for tests and short-lived processes.
///
/// `create` replaces any existing credential for the integration, mirroring
/// the reference's delete-then-insert transaction.
#[derive(Debug, Clone, Default)]
pub struct MemoryCredentialStore {
    inner: std::sync::Arc<std::sync::Mutex<Vec<Info>>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        MemoryCredentialStore::default()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn all(&self) -> Result<Vec<Info>, CredentialError> {
        Ok(self.inner.lock().unwrap().clone())
    }

    fn list(&self, integration_id: &str) -> Result<Vec<Info>, CredentialError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|info| info.integration_id == integration_id)
            .cloned()
            .collect())
    }

    fn get(&self, id: &Id) -> Result<Option<Info>, CredentialError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .find(|info| &info.id == id)
            .cloned())
    }

    fn create(
        &mut self,
        integration_id: &str,
        value: Value,
        label: Option<&str>,
    ) -> Result<Info, CredentialError> {
        let mut inner = self.inner.lock().unwrap();
        inner.retain(|info| info.integration_id != integration_id);
        let info = Info {
            id: Id::create(),
            integration_id: integration_id.to_string(),
            label: label.unwrap_or("default").to_string(),
            value,
        };
        inner.push(info.clone());
        Ok(info)
    }

    fn update(&mut self, id: &Id, updates: CredentialUpdate) -> Result<(), CredentialError> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut inner = self.inner.lock().unwrap();
        if let Some(info) = inner.iter_mut().find(|info| &info.id == id) {
            if let Some(label) = updates.label {
                info.label = label;
            }
            if let Some(value) = updates.value {
                info.value = value;
            }
        }
        Ok(())
    }

    fn remove(&mut self, id: &Id) -> Result<(), CredentialError> {
        let mut inner = self.inner.lock().unwrap();
        inner.retain(|info| &info.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_value(key: &str) -> Value {
        Value::Key(Key {
            key: key.to_string(),
            metadata: None,
        })
    }

    #[test]
    fn create_replaces_per_integration() {
        let mut store = MemoryCredentialStore::new();
        let first = store.create("anthropic", key_value("key-1"), None).unwrap();
        let second = store.create("anthropic", key_value("key-2"), None).unwrap();
        assert_ne!(first.id, second.id);
        let all = store.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].value, key_value("key-2"));
    }

    #[test]
    fn roundtrip() {
        let mut store = MemoryCredentialStore::new();
        let created = store
            .create(
                "anthropic",
                Value::OAuth(OAuth {
                    method_id: "oauth2".to_string(),
                    refresh: "refresh".to_string(),
                    access: "access".to_string(),
                    expires: 100,
                    metadata: None,
                }),
                Some("work"),
            )
            .unwrap();
        assert_eq!(store.get(&created.id).unwrap().unwrap().label, "work");
        assert_eq!(store.list("anthropic").unwrap().len(), 1);
        store
            .update(
                &created.id,
                CredentialUpdate {
                    label: Some("personal".to_string()),
                    value: None,
                },
            )
            .unwrap();
        assert_eq!(store.get(&created.id).unwrap().unwrap().label, "personal");
        store.remove(&created.id).unwrap();
        assert!(store.get(&created.id).unwrap().is_none());
    }

    #[test]
    fn id_prefix() {
        assert!(Id::create().0.starts_with("cred_"));
    }

    #[test]
    fn value_serializes_with_type_discriminator() {
        let value = key_value("sk-test");
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["type"], "key");
        assert_eq!(json["key"], "sk-test");
        let roundtrip: Value = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip, value);
    }
}
