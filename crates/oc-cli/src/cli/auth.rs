//! Local credential store backed by `auth.json`.
//! From reference/packages/opencode/src/auth/index.ts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::paths::GlobalPaths;

/// Mirrors the `Auth.Info` union (`oauth` | `api` | `wellknown`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthInfo {
    OAuth {
        refresh: String,
        access: String,
        expires: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        enterprise_url: Option<String>,
    },
    Api {
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, String>>,
    },
    WellKnown {
        key: String,
        token: String,
    },
}

impl AuthInfo {
    /// The credential type label used by `auth list`.
    pub fn type_label(&self) -> &'static str {
        match self {
            AuthInfo::OAuth { .. } => "oauth",
            AuthInfo::Api { .. } => "api",
            AuthInfo::WellKnown { .. } => "wellknown",
        }
    }
}

/// Read/write access to the global `auth.json` credentials file.
#[derive(Debug, Clone)]
pub struct Auth {
    pub file: std::path::PathBuf,
}

impl Auth {
    /// Mirrors `Auth.Service`: the file lives at `Global.Path.data/auth.json`.
    pub fn new(paths: &GlobalPaths) -> Self {
        Self {
            file: paths.data.join("auth.json"),
        }
    }

    /// Mirrors `Auth.all()`: reads the JSON file, honoring
    /// `OPENCODE_AUTH_CONTENT`, and filters out malformed entries.
    pub fn all(&self) -> BTreeMap<String, AuthInfo> {
        let text = match std::env::var("OPENCODE_AUTH_CONTENT") {
            Ok(content) => content,
            Err(_) => std::fs::read_to_string(&self.file).unwrap_or_default(),
        };
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Object(Default::default()));
        let mut result = BTreeMap::new();
        if let Some(map) = value.as_object() {
            for (key, entry) in map {
                if let Ok(info) = serde_json::from_value::<AuthInfo>(entry.clone()) {
                    result.insert(key.clone(), info);
                }
            }
        }
        result
    }

    /// Mirrors `Auth.set(key, info)`.
    pub fn set(&self, key: &str, info: AuthInfo) -> anyhow::Result<()> {
        let normalized = key.trim_end_matches('/');
        let mut data = self.all();
        data.remove(key);
        data.remove(&format!("{normalized}/"));
        data.insert(normalized.to_string(), info);
        self.write(&data)
    }

    /// Mirrors `Auth.remove(key)`.
    pub fn remove(&self, key: &str) -> anyhow::Result<()> {
        let normalized = key.trim_end_matches('/');
        let mut data = self.all();
        data.remove(key);
        data.remove(normalized);
        self.write(&data)
    }

    fn write(&self, data: &BTreeMap<String, AuthInfo>) -> anyhow::Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(data)?;
        #[cfg(unix)]
        use std::io::Write;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.file)?;
            file.write_all(text.as_bytes())?;
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&self.file, text)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn tempdir() -> std::io::Result<Self> {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let mut path = std::env::temp_dir();
            path.push(format!(
                "oc-cli-auth-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path)?;
            Ok(TempDir(path))
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_auth() -> (TempDir, Auth) {
        let dir = TempDir::tempdir().unwrap();
        let auth = Auth {
            file: dir.path().join("auth.json"),
        };
        (dir, auth)
    }

    #[test]
    fn round_trips_credentials() {
        let (_dir, auth) = temp_auth();
        auth.set(
            "openai",
            AuthInfo::Api {
                key: "sk-123".into(),
                metadata: None,
            },
        )
        .unwrap();
        let all = auth.all();
        assert_eq!(all.len(), 1);
        assert_eq!(all["openai"].type_label(), "api");
        auth.remove("openai").unwrap();
        assert!(auth.all().is_empty());
    }

    #[test]
    fn ignores_malformed_entries() {
        let (_dir, auth) = temp_auth();
        std::fs::write(&auth.file, r#"{"broken": {"type": "nope"}}"#).unwrap();
        assert!(auth.all().is_empty());
    }
}
