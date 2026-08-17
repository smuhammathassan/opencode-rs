//! OAuth credential persistence for MCP servers.
//!
//! From reference/packages/opencode/src/mcp/auth.ts. Credentials are stored as
//! JSON at `<data-dir>/mcp-auth.json` (mode 0600), keyed by server name, with
//! mutations serialized by a lock (the reference uses an effect file flock).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_issued_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
}

/// Resolves the opencode data directory: `OPENCODE_TEST_HOME/data`, else
/// `XDG_DATA_HOME/opencode`, else `~/.local/share/opencode`.
/// From reference/packages/core/src/global.ts (`Global.Path.data`).
pub fn default_data_dir() -> PathBuf {
    if let Ok(home) = std::env::var("OPENCODE_TEST_HOME") {
        return PathBuf::from(home).join("data");
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("opencode");
    }
    std::env::var_os("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("opencode")
        })
        .unwrap_or_else(|| std::env::temp_dir().join("opencode"))
}

pub struct McpAuth {
    path: PathBuf,
    lock: Mutex<()>,
}

impl McpAuth {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        McpAuth {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// `filepath = path.join(Global.Path.data, "mcp-auth.json")`
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        Self::new(default_data_dir().join("mcp-auth.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All entries. Corrupt or missing files decode to an empty map.
    pub async fn all(&self) -> crate::Result<HashMap<String, Entry>> {
        let _guard = self.lock.lock().await;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || read_file(&path))
            .await
            .map_err(|error| crate::Error::message(format!("auth read task failed: {error}")))?
    }

    pub async fn get(&self, name: &str) -> crate::Result<Option<Entry>> {
        Ok(self.all().await?.get(name).cloned())
    }

    /// Like `get` but validates the entry's `serverUrl` matches, so stored
    /// credentials can't leak to a different server URL.
    pub async fn get_for_url(&self, name: &str, server_url: &str) -> crate::Result<Option<Entry>> {
        let entry = self.get(name).await?;
        match entry {
            Some(entry) if entry.server_url.as_deref() == Some(server_url) => Ok(Some(entry)),
            _ => Ok(None),
        }
    }

    pub async fn set(
        &self,
        name: &str,
        entry: Entry,
        server_url: Option<&str>,
    ) -> crate::Result<()> {
        let mut entry = entry;
        if let Some(url) = server_url {
            entry.server_url = Some(url.to_string());
        }
        let name = name.to_string();
        self.mutate(move |mut data| {
            data.insert(name.clone(), entry);
            Some(data)
        })
        .await
    }

    pub async fn remove(&self, name: &str) -> crate::Result<()> {
        let name = name.to_string();
        self.mutate(move |mut data| {
            data.remove(&name);
            Some(data)
        })
        .await
    }

    pub async fn update_tokens(
        &self,
        name: &str,
        tokens: Tokens,
        server_url: Option<&str>,
    ) -> crate::Result<()> {
        let name = name.to_string();
        let server_url = server_url.map(str::to_string);
        self.mutate(move |mut data| {
            let entry = data.entry(name).or_default();
            entry.tokens = Some(tokens);
            if let Some(url) = server_url {
                entry.server_url = Some(url);
            }
            Some(data)
        })
        .await
    }

    pub async fn update_client_info(
        &self,
        name: &str,
        client_info: ClientInfo,
        server_url: Option<&str>,
    ) -> crate::Result<()> {
        let name = name.to_string();
        let server_url = server_url.map(str::to_string);
        self.mutate(move |mut data| {
            let entry = data.entry(name).or_default();
            entry.client_info = Some(client_info);
            if let Some(url) = server_url {
                entry.server_url = Some(url);
            }
            Some(data)
        })
        .await
    }

    pub async fn update_code_verifier(
        &self,
        name: &str,
        code_verifier: String,
    ) -> crate::Result<()> {
        self.update_code_verifier_for_url(name, code_verifier, None)
            .await
    }

    pub async fn update_code_verifier_for_url(
        &self,
        name: &str,
        code_verifier: String,
        server_url: Option<&str>,
    ) -> crate::Result<()> {
        let name = name.to_string();
        let server_url = server_url.map(str::to_string);
        self.mutate(move |mut data| {
            let entry = data.entry(name).or_default();
            entry.code_verifier = Some(code_verifier);
            if let Some(server_url) = server_url {
                entry.server_url = Some(server_url);
            }
            Some(data)
        })
        .await
    }

    pub async fn clear_code_verifier(&self, name: &str) -> crate::Result<()> {
        let name = name.to_string();
        self.mutate(move |mut data| {
            let entry = data.get_mut(&name)?;
            entry.code_verifier = None;
            Some(data)
        })
        .await
    }

    pub async fn update_oauth_state(&self, name: &str, oauth_state: String) -> crate::Result<()> {
        self.update_oauth_state_for_url(name, oauth_state, None)
            .await
    }

    pub async fn update_oauth_state_for_url(
        &self,
        name: &str,
        oauth_state: String,
        server_url: Option<&str>,
    ) -> crate::Result<()> {
        let name = name.to_string();
        let server_url = server_url.map(str::to_string);
        self.mutate(move |mut data| {
            let entry = data.entry(name).or_default();
            entry.oauth_state = Some(oauth_state);
            if let Some(server_url) = server_url {
                entry.server_url = Some(server_url);
            }
            Some(data)
        })
        .await
    }

    pub async fn get_oauth_state(&self, name: &str) -> crate::Result<Option<String>> {
        Ok(self.get(name).await?.and_then(|entry| entry.oauth_state))
    }

    pub async fn get_oauth_state_for_url(
        &self,
        name: &str,
        server_url: &str,
    ) -> crate::Result<Option<String>> {
        Ok(self
            .get_for_url(name, server_url)
            .await?
            .and_then(|entry| entry.oauth_state))
    }

    pub async fn clear_oauth_state(&self, name: &str) -> crate::Result<()> {
        let name = name.to_string();
        self.mutate(move |mut data| {
            let entry = data.get_mut(&name)?;
            entry.oauth_state = None;
            Some(data)
        })
        .await
    }

    async fn mutate(
        &self,
        update: impl FnOnce(HashMap<String, Entry>) -> Option<HashMap<String, Entry>> + Send + 'static,
    ) -> crate::Result<()> {
        let _guard = self.lock.lock().await;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let data = read_file(&path)?;
            let next =
                update(data).ok_or_else(|| crate::Error::message("update returned no data"))?;
            write_file(&path, &next)?;
            Ok(())
        })
        .await
        .map_err(|error| crate::Error::message(format!("auth write task failed: {error}")))?
    }
}

fn read_file(path: &Path) -> crate::Result<HashMap<String, Entry>> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HashMap::new());
        }
        Err(error) => return Err(error.into()),
    };
    match serde_json::from_slice(&data) {
        Ok(entries) => Ok(entries),
        Err(_) => Ok(HashMap::new()),
    }
}

fn write_file(path: &Path, data: &HashMap<String, Entry>) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(data)?;
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_auth() -> (McpAuth, PathBuf) {
        let dir = std::env::temp_dir().join(format!("oc-mcp-auth-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("mcp-auth.json");
        (McpAuth::new(path.clone()), path)
    }

    #[tokio::test]
    async fn set_get_roundtrip() {
        let (auth, path) = temp_auth();
        auth.set(
            "server-a",
            Entry {
                tokens: Some(Tokens {
                    access_token: "tok".into(),
                    refresh_token: Some("refresh".into()),
                    expires_at: Some(1234.5),
                    scope: None,
                }),
                ..Default::default()
            },
            Some("https://example.com/mcp"),
        )
        .await
        .unwrap();

        let entry = auth.get("server-a").await.unwrap().unwrap();
        assert_eq!(entry.tokens.unwrap().access_token, "tok");
        assert_eq!(entry.server_url.as_deref(), Some("https://example.com/mcp"));

        // Bytes on disk must be valid JSON.
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: HashMap<String, Entry> = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed["server-a"]
                .tokens
                .as_ref()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("refresh")
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn get_for_url_matches_exact_url() {
        let (auth, path) = temp_auth();
        auth.set("s", Entry::default(), Some("https://a.example/mcp"))
            .await
            .unwrap();
        assert!(auth
            .get_for_url("s", "https://a.example/mcp")
            .await
            .unwrap()
            .is_some());
        assert!(auth
            .get_for_url("s", "https://b.example/mcp")
            .await
            .unwrap()
            .is_none());
        assert!(auth
            .get_for_url("other", "https://a.example/mcp")
            .await
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn update_and_clear_fields() {
        let (auth, path) = temp_auth();
        auth.update_oauth_state("s", "state-1".to_string())
            .await
            .unwrap();
        assert_eq!(
            auth.get_oauth_state("s").await.unwrap().as_deref(),
            Some("state-1")
        );
        auth.clear_oauth_state("s").await.unwrap();
        assert_eq!(auth.get_oauth_state("s").await.unwrap(), None);

        auth.update_code_verifier("s", "verifier".into())
            .await
            .unwrap();
        let entry = auth.get("s").await.unwrap().unwrap();
        assert_eq!(entry.code_verifier.as_deref(), Some("verifier"));
        auth.clear_code_verifier("s").await.unwrap();
        assert_eq!(auth.get("s").await.unwrap().unwrap().code_verifier, None);

        auth.remove("s").await.unwrap();
        assert!(auth.get("s").await.unwrap().is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn corrupt_file_decodes_to_empty() {
        let (auth, path) = temp_auth();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(auth.all().await.unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
