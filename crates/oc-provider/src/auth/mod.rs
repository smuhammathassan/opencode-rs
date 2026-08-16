//! Credential storage for provider auth.
//!
//! From reference/packages/opencode/src/auth/index.ts.

pub mod login;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// `OAUTH_DUMMY_KEY` from `auth/index.ts`.
pub const OAUTH_DUMMY_KEY: &str = "opencode-oauth-dummy-key";

/// `OAuth` from `auth/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Oauth {
    pub refresh: String,
    pub access: String,
    pub expires: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise_url: Option<String>,
}

impl Oauth {
    /// Returns whether the access-token expiry timestamp has passed.
    ///
    /// `expires` is stored as an absolute Unix timestamp in milliseconds,
    /// matching the value consumed by the provider/server boundary. Keeping
    /// the clock as an argument makes refresh decisions deterministic and
    /// avoids coupling this crate to a runtime clock.
    pub fn is_expired_at(&self, now_ms: u64) -> bool {
        self.expires == 0 || self.expires <= now_ms
    }
}

/// `Api` from `auth/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Api {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
}

/// `WellKnown` from `auth/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WellKnown {
    pub key: String,
    pub token: String,
}

/// `Info` from `auth/index.ts`: the union of stored credential shapes,
/// discriminated by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Info {
    Oauth(Oauth),
    Api(Api),
    WellKnown(WellKnown),
}

impl Info {
    pub fn r#type(&self) -> &'static str {
        match self {
            Info::Oauth(_) => "oauth",
            Info::Api(_) => "api",
            Info::WellKnown(_) => "wellknown",
        }
    }
}

/// `AuthError` from `auth/index.ts`.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AuthError {
    pub message: String,
    #[source]
    pub cause: Option<anyhow::Error>,
}

fn fail(message: &str) -> impl FnOnce(anyhow::Error) -> AuthError + '_ {
    |cause| AuthError {
        message: message.to_string(),
        cause: Some(cause),
    }
}

/// `Auth.Service` from `auth/index.ts`.
pub trait AuthStore: Send + Sync {
    fn get(&self, provider_id: &str) -> Result<Option<Info>, AuthError>;
    fn all(&self) -> Result<BTreeMap<String, Info>, AuthError>;
    fn set(&mut self, key: &str, info: Info) -> Result<(), AuthError>;
    fn remove(&mut self, key: &str) -> Result<(), AuthError>;
}

/// Normalizes a key by stripping trailing slashes.
///
/// From `set`/`remove` in `auth/index.ts`.
fn normalize(key: &str) -> String {
    key.trim_end_matches('/').to_string()
}

/// A file-backed [`AuthStore`] storing credentials in
/// `<data_dir>/auth.json` with 0o600 permissions.
///
/// Mirrors the `auth.json` layout from `auth/index.ts`. `OPENCODE_AUTH_CONTENT`
/// overrides the on-disk contents for `all()`/`get()`.
pub struct FileAuthStore {
    path: std::path::PathBuf,
}

impl FileAuthStore {
    /// Creates a store rooted at `data_dir` (`Global.Path.data` in the
    /// reference).
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        FileAuthStore {
            path: data_dir.as_ref().join("auth.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> BTreeMap<String, Info> {
        if let Ok(content) = std::env::var("OPENCODE_AUTH_CONTENT") {
            if let Ok(data) = serde_json::from_str::<BTreeMap<String, Info>>(&content) {
                return data;
            }
        }
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice::<BTreeMap<String, Info>>(&bytes)
                .map(|data| {
                    data.into_iter()
                        .filter(|(_, info)| serde_json::to_value(info).is_ok())
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => BTreeMap::new(),
        }
    }

    fn write(&self, data: &BTreeMap<String, Info>) -> Result<(), anyhow::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(data)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&bytes)?;
        Ok(())
    }
}

impl AuthStore for FileAuthStore {
    fn get(&self, provider_id: &str) -> Result<Option<Info>, AuthError> {
        Ok(self.read().get(provider_id).cloned())
    }

    fn all(&self) -> Result<BTreeMap<String, Info>, AuthError> {
        Ok(self.read())
    }

    fn set(&mut self, key: &str, info: Info) -> Result<(), AuthError> {
        let norm = normalize(key);
        let mut data = self.read();
        if norm != key {
            data.remove(key);
        }
        data.remove(&format!("{}/", norm));
        data.insert(norm, info);
        self.write(&data).map_err(fail("Failed to write auth data"))
    }

    fn remove(&mut self, key: &str) -> Result<(), AuthError> {
        let norm = normalize(key);
        let mut data = self.read();
        data.remove(key);
        data.remove(&norm);
        self.write(&data).map_err(fail("Failed to write auth data"))
    }
}

/// An in-memory [`AuthStore`] used for tests and short-lived processes.
#[derive(Debug, Clone, Default)]
pub struct MemoryAuthStore {
    inner: std::sync::Arc<std::sync::Mutex<BTreeMap<String, Info>>>,
}

impl MemoryAuthStore {
    pub fn new() -> Self {
        MemoryAuthStore::default()
    }
}

impl AuthStore for MemoryAuthStore {
    fn get(&self, provider_id: &str) -> Result<Option<Info>, AuthError> {
        Ok(self.inner.lock().unwrap().get(provider_id).cloned())
    }

    fn all(&self) -> Result<BTreeMap<String, Info>, AuthError> {
        Ok(self.inner.lock().unwrap().clone())
    }

    fn set(&mut self, key: &str, info: Info) -> Result<(), AuthError> {
        let norm = normalize(key);
        let mut data = self.inner.lock().unwrap();
        if norm != key {
            data.remove(key);
        }
        data.remove(&format!("{}/", norm));
        data.insert(norm, info);
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), AuthError> {
        let norm = normalize(key);
        let mut data = self.inner.lock().unwrap();
        data.remove(key);
        data.remove(&norm);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_normalizes_trailing_slashes() {
        let mut store = MemoryAuthStore::new();
        store
            .set(
                "https://example.com/",
                Info::WellKnown(WellKnown {
                    key: "TOKEN".to_string(),
                    token: "abc".to_string(),
                }),
            )
            .unwrap();
        let data = store.all().unwrap();
        assert!(data.contains_key("https://example.com"));
        assert!(!data.contains_key("https://example.com/"));
    }

    #[test]
    fn set_cleans_up_pre_existing_trailing_slash_entry() {
        let mut store = MemoryAuthStore::new();
        store
            .set(
                "https://example.com/",
                Info::WellKnown(WellKnown {
                    key: "TOKEN".to_string(),
                    token: "old".to_string(),
                }),
            )
            .unwrap();
        store
            .set(
                "https://example.com",
                Info::WellKnown(WellKnown {
                    key: "TOKEN".to_string(),
                    token: "new".to_string(),
                }),
            )
            .unwrap();
        let data = store.all().unwrap();
        let keys: Vec<&String> = data
            .keys()
            .filter(|key| key.contains("example.com"))
            .collect();
        assert_eq!(keys, vec!["https://example.com"]);
        assert_eq!(
            data["https://example.com"],
            Info::WellKnown(WellKnown {
                key: "TOKEN".to_string(),
                token: "new".to_string(),
            })
        );
    }

    #[test]
    fn remove_deletes_both_normalized_and_trailing_slash_keys() {
        let mut store = MemoryAuthStore::new();
        store
            .set(
                "https://example.com",
                Info::WellKnown(WellKnown {
                    key: "TOKEN".to_string(),
                    token: "abc".to_string(),
                }),
            )
            .unwrap();
        store.remove("https://example.com/").unwrap();
        let data = store.all().unwrap();
        assert!(!data.contains_key("https://example.com"));
    }

    #[test]
    fn set_and_remove_roundtrip() {
        let mut store = MemoryAuthStore::new();
        store
            .set(
                "anthropic",
                Info::Api(Api {
                    key: "sk-test".to_string(),
                    metadata: None,
                }),
            )
            .unwrap();
        assert!(store.get("anthropic").unwrap().is_some());
        store.remove("anthropic").unwrap();
        assert!(store.get("anthropic").unwrap().is_none());
    }

    #[test]
    fn file_store_roundtrip() {
        let dir = std::env::temp_dir().join(format!("oc-provider-auth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("auth.json");
        {
            let mut store = FileAuthStore::new(&dir);
            store
                .set(
                    "anthropic",
                    Info::Api(Api {
                        key: "sk-test".to_string(),
                        metadata: None,
                    }),
                )
                .unwrap();
            assert_eq!(store.get("anthropic").unwrap().unwrap().r#type(), "api");
        }
        let store = FileAuthStore::new(&dir);
        assert_eq!(store.get("anthropic").unwrap().unwrap().r#type(), "api");
        let _ = path;
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn info_serializes_with_type_discriminator() {
        let info = Info::Api(Api {
            key: "sk-test".to_string(),
            metadata: None,
        });
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["type"], "api");
        assert_eq!(json["key"], "sk-test");
        let roundtrip: Info = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip, info);
    }
}
