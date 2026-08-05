//! OAuth client providers for MCP servers.
//!
//! From reference/packages/opencode/src/mcp/oauth-provider.ts plus the
//! `OAuthClientProvider` interface of `@modelcontextprotocol/sdk@1.29.0`
//! `client/auth.js`. Providers persist tokens and dynamic client registration
//! via `McpAuth` and surface the authorization redirect to callers.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::auth::McpAuth;
use crate::crypto::random_hex;
use crate::util::BoxFuture;
use crate::Result;

pub const OAUTH_CALLBACK_PORT: u16 = 19876;
pub const OAUTH_CALLBACK_PATH: &str = "/mcp/oauth/callback";

/// OAuth client metadata sent in dynamic client registration (RFC 7591).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OAuthClientMetadata {
    pub redirect_uris: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// OAuth tokens as returned by a token endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthTokens {
    #[serde(rename = "access_token")]
    pub access_token: String,
    #[serde(rename = "token_type", skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(rename = "refresh_token", skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(rename = "expires_in", skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Client credentials (registered or static).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthClientInformation {
    #[serde(rename = "client_id")]
    pub client_id: String,
    #[serde(rename = "client_secret", skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

/// Full client registration response (RFC 7591).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthClientInformationFull {
    #[serde(rename = "client_id")]
    pub client_id: String,
    #[serde(rename = "client_secret", skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(
        rename = "client_id_issued_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_id_issued_at: Option<u64>,
    #[serde(
        rename = "client_secret_expires_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_secret_expires_at: Option<u64>,
}

/// Authorization server metadata (RFC 8414).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AuthorizationServerMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge_methods_supported: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Protected resource metadata (RFC 9728).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ProtectedResourceMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_methods_supported: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Which credentials to invalidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialsType {
    All,
    Client,
    Tokens,
}

pub trait OAuthClientProvider: Send + Sync {
    fn redirect_url(&self) -> String;
    fn client_metadata(&self) -> OAuthClientMetadata;
    fn client_information(&self) -> BoxFuture<'_, Result<Option<OAuthClientInformation>>>;
    fn save_client_information(
        &self,
        info: OAuthClientInformationFull,
    ) -> BoxFuture<'_, Result<()>>;
    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>>>;
    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<()>>;
    fn redirect_to_authorization(&self, url: &Url) -> BoxFuture<'_, Result<()>>;
    fn save_code_verifier(&self, verifier: &str) -> BoxFuture<'_, Result<()>>;
    fn code_verifier(&self) -> BoxFuture<'_, Result<String>>;
    fn save_state(&self, state: &str) -> BoxFuture<'_, Result<()>>;
    fn state(&self) -> BoxFuture<'_, Result<String>>;
    fn invalidate_credentials(&self, ty: CredentialsType) -> BoxFuture<'_, Result<()>>;
}

#[derive(Clone)]
pub struct McpOAuthCallbacks {
    pub on_redirect: Arc<dyn Fn(&Url) -> BoxFuture<'_, Result<()>> + Send + Sync>,
}

impl Default for McpOAuthCallbacks {
    fn default() -> Self {
        McpOAuthCallbacks {
            on_redirect: Arc::new(|_url| Box::pin(async { Ok(()) })),
        }
    }
}

/// OAuth config from the `mcp` config section (mirrors `ConfigMCPV1.OAuth`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct McpOAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scope: Option<String>,
    pub callback_port: Option<u16>,
    pub redirect_uri: Option<String>,
}

impl McpOAuthConfig {
    pub fn from_config(config: Option<&crate::config::OAuthConfig>) -> Self {
        match config {
            Some(config) => McpOAuthConfig {
                client_id: config.client_id.clone(),
                client_secret: config.client_secret.clone(),
                scope: config.scope.clone(),
                callback_port: config.callback_port,
                redirect_uri: config.redirect_uri.clone(),
            },
            None => McpOAuthConfig::default(),
        }
    }
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The default provider: persists credentials via `McpAuth`.
pub struct McpOAuthProvider {
    mcp_name: String,
    server_url: String,
    config: McpOAuthConfig,
    callbacks: McpOAuthCallbacks,
    auth: Arc<McpAuth>,
}

impl McpOAuthProvider {
    pub fn new(
        mcp_name: impl Into<String>,
        server_url: impl Into<String>,
        config: McpOAuthConfig,
        callbacks: McpOAuthCallbacks,
        auth: Arc<McpAuth>,
    ) -> Self {
        McpOAuthProvider {
            mcp_name: mcp_name.into(),
            server_url: server_url.into(),
            config,
            callbacks,
            auth,
        }
    }
}

impl OAuthClientProvider for McpOAuthProvider {
    fn redirect_url(&self) -> String {
        if let Some(redirect_uri) = &self.config.redirect_uri {
            return redirect_uri.clone();
        }
        let port = self.config.callback_port.unwrap_or(OAUTH_CALLBACK_PORT);
        format!("http://127.0.0.1:{port}{OAUTH_CALLBACK_PATH}")
    }

    fn client_metadata(&self) -> OAuthClientMetadata {
        OAuthClientMetadata {
            redirect_uris: vec![self.redirect_url()],
            client_name: Some("OpenCode".into()),
            client_uri: Some("https://opencode.ai".into()),
            grant_types: Some(vec!["authorization_code".into(), "refresh_token".into()]),
            response_types: Some(vec!["code".into()]),
            token_endpoint_auth_method: Some(
                if self.config.client_secret.is_some() {
                    "client_secret_post"
                } else {
                    "none"
                }
                .into(),
            ),
            scope: self.config.scope.clone(),
        }
    }

    fn client_information(&self) -> BoxFuture<'_, Result<Option<OAuthClientInformation>>> {
        Box::pin(async move {
            if let Some(client_id) = &self.config.client_id {
                return Ok(Some(OAuthClientInformation {
                    client_id: client_id.clone(),
                    client_secret: self.config.client_secret.clone(),
                }));
            }
            let entry = self
                .auth
                .get_for_url(&self.mcp_name, &self.server_url)
                .await?;
            let Some(client_info) = entry.and_then(|entry| entry.client_info) else {
                return Ok(None);
            };
            if let Some(expires_at) = client_info.client_secret_expires_at {
                if expires_at < now_seconds() {
                    return Ok(None);
                }
            }
            Ok(Some(OAuthClientInformation {
                client_id: client_info.client_id,
                client_secret: client_info.client_secret,
            }))
        })
    }

    fn save_client_information(
        &self,
        info: OAuthClientInformationFull,
    ) -> BoxFuture<'_, Result<()>> {
        let mcp_name = self.mcp_name.clone();
        let server_url = self.server_url.clone();
        Box::pin(async move {
            self.auth
                .update_client_info(
                    &mcp_name,
                    crate::auth::ClientInfo {
                        client_id: info.client_id,
                        client_secret: info.client_secret,
                        client_id_issued_at: info.client_id_issued_at.map(|v| v as f64),
                        client_secret_expires_at: info.client_secret_expires_at.map(|v| v as f64),
                    },
                    Some(&server_url),
                )
                .await
        })
    }

    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>>> {
        Box::pin(async move {
            let entry = self
                .auth
                .get_for_url(&self.mcp_name, &self.server_url)
                .await?;
            let Some(tokens) = entry.and_then(|entry| entry.tokens) else {
                return Ok(None);
            };
            Ok(Some(OAuthTokens {
                access_token: tokens.access_token,
                token_type: Some("Bearer".into()),
                refresh_token: tokens.refresh_token,
                expires_in: tokens
                    .expires_at
                    .map(|expires_at| (expires_at - now_seconds()).max(0.0).floor() as u64),
                scope: tokens.scope,
            }))
        })
    }

    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<()>> {
        let mcp_name = self.mcp_name.clone();
        let server_url = self.server_url.clone();
        Box::pin(async move {
            self.auth
                .update_tokens(
                    &mcp_name,
                    crate::auth::Tokens {
                        access_token: tokens.access_token,
                        refresh_token: tokens.refresh_token,
                        expires_at: tokens
                            .expires_in
                            .map(|expires_in| now_seconds() + expires_in as f64),
                        scope: tokens.scope,
                    },
                    Some(&server_url),
                )
                .await
        })
    }

    fn redirect_to_authorization(&self, url: &Url) -> BoxFuture<'_, Result<()>> {
        let callbacks = self.callbacks.clone();
        let url = url.clone();
        Box::pin(async move { (callbacks.on_redirect)(&url).await })
    }

    fn save_code_verifier(&self, verifier: &str) -> BoxFuture<'_, Result<()>> {
        let mcp_name = self.mcp_name.clone();
        let verifier = verifier.to_string();
        Box::pin(async move { self.auth.update_code_verifier(&mcp_name, verifier).await })
    }

    fn code_verifier(&self) -> BoxFuture<'_, Result<String>> {
        let mcp_name = self.mcp_name.clone();
        Box::pin(async move {
            let entry = self.auth.get(&mcp_name).await?;
            entry.and_then(|entry| entry.code_verifier).ok_or_else(|| {
                crate::Error::message(format!("No code verifier saved for MCP server: {mcp_name}"))
            })
        })
    }

    fn save_state(&self, state: &str) -> BoxFuture<'_, Result<()>> {
        let mcp_name = self.mcp_name.clone();
        let state = state.to_string();
        Box::pin(async move { self.auth.update_oauth_state(&mcp_name, state).await })
    }

    fn state(&self) -> BoxFuture<'_, Result<String>> {
        let mcp_name = self.mcp_name.clone();
        Box::pin(async move {
            let entry = self.auth.get(&mcp_name).await?;
            if let Some(state) = entry.and_then(|entry| entry.oauth_state) {
                return Ok(state);
            }
            let new_state = random_hex(32);
            self.auth
                .update_oauth_state(&mcp_name, new_state.clone())
                .await?;
            Ok(new_state)
        })
    }

    fn invalidate_credentials(&self, ty: CredentialsType) -> BoxFuture<'_, Result<()>> {
        let mcp_name = self.mcp_name.clone();
        Box::pin(async move {
            let Some(mut entry) = self.auth.get(&mcp_name).await? else {
                return Ok(());
            };
            match ty {
                CredentialsType::All => self.auth.remove(&mcp_name).await?,
                CredentialsType::Client => {
                    entry.client_info = None;
                    self.auth.set(&mcp_name, entry, None).await?;
                }
                CredentialsType::Tokens => {
                    entry.tokens = None;
                    self.auth.set(&mcp_name, entry, None).await?;
                }
            }
            Ok(())
        })
    }
}

/// Provider used during an explicit auth flow (`startAuth`): credentials are
/// buffered in memory until `commit` persists them.
pub struct McpOAuthPendingProvider {
    inner: McpOAuthProvider,
    pending_client_info: std::sync::Mutex<Option<OAuthClientInformationFull>>,
    pending_tokens: std::sync::Mutex<Option<OAuthTokens>>,
}

impl McpOAuthPendingProvider {
    pub fn new(
        mcp_name: impl Into<String>,
        server_url: impl Into<String>,
        config: McpOAuthConfig,
        callbacks: McpOAuthCallbacks,
        auth: Arc<McpAuth>,
    ) -> Self {
        McpOAuthPendingProvider {
            inner: McpOAuthProvider::new(mcp_name, server_url, config, callbacks, auth),
            pending_client_info: std::sync::Mutex::new(None),
            pending_tokens: std::sync::Mutex::new(None),
        }
    }

    /// Persist any buffered tokens (and dynamic client registration) to `McpAuth`.
    pub async fn commit(&self) -> Result<()> {
        let pending_tokens = self.pending_tokens.lock().unwrap().clone();
        let Some(tokens) = pending_tokens else {
            return Ok(());
        };
        let client_info = self.pending_client_info.lock().unwrap().clone();
        let has_explicit_client_id = self.inner.config.client_id.is_some();
        let tokens_entry = crate::auth::Tokens {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens
                .expires_in
                .map(|expires_in| now_seconds() + expires_in as f64),
            scope: tokens.scope.clone(),
        };
        let client_info_entry = match (client_info, has_explicit_client_id) {
            (Some(info), false) => Some(crate::auth::ClientInfo {
                client_id: info.client_id,
                client_secret: info.client_secret,
                client_id_issued_at: info.client_id_issued_at.map(|v| v as f64),
                client_secret_expires_at: info.client_secret_expires_at.map(|v| v as f64),
            }),
            _ => None,
        };
        self.inner
            .auth
            .set(
                &self.inner.mcp_name,
                crate::auth::Entry {
                    tokens: Some(tokens_entry),
                    client_info: client_info_entry,
                    ..Default::default()
                },
                Some(&self.inner.server_url),
            )
            .await
    }
}

impl OAuthClientProvider for McpOAuthPendingProvider {
    fn redirect_url(&self) -> String {
        self.inner.redirect_url()
    }

    fn client_metadata(&self) -> OAuthClientMetadata {
        self.inner.client_metadata()
    }

    fn client_information(&self) -> BoxFuture<'_, Result<Option<OAuthClientInformation>>> {
        Box::pin(async move {
            if self.inner.config.client_id.is_none() {
                let pending = self.pending_client_info.lock().unwrap().clone();
                if let Some(pending) = pending {
                    return Ok(Some(OAuthClientInformation {
                        client_id: pending.client_id,
                        client_secret: pending.client_secret,
                    }));
                }
                return Ok(None);
            }
            Ok(Some(OAuthClientInformation {
                client_id: self.inner.config.client_id.clone().unwrap(),
                client_secret: self.inner.config.client_secret.clone(),
            }))
        })
    }

    fn save_client_information(
        &self,
        info: OAuthClientInformationFull,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            *self.pending_client_info.lock().unwrap() = Some(info);
            Ok(())
        })
    }

    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>>> {
        Box::pin(async move { Ok(self.pending_tokens.lock().unwrap().clone()) })
    }

    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            *self.pending_tokens.lock().unwrap() = Some(tokens);
            Ok(())
        })
    }

    fn redirect_to_authorization(&self, url: &Url) -> BoxFuture<'_, Result<()>> {
        self.inner.redirect_to_authorization(url)
    }

    fn save_code_verifier(&self, verifier: &str) -> BoxFuture<'_, Result<()>> {
        self.inner.save_code_verifier(verifier)
    }

    fn code_verifier(&self) -> BoxFuture<'_, Result<String>> {
        self.inner.code_verifier()
    }

    fn save_state(&self, state: &str) -> BoxFuture<'_, Result<()>> {
        self.inner.save_state(state)
    }

    fn state(&self) -> BoxFuture<'_, Result<String>> {
        self.inner.state()
    }

    fn invalidate_credentials(&self, ty: CredentialsType) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let mut pending_client_info = self.pending_client_info.lock().unwrap();
            let mut pending_tokens = self.pending_tokens.lock().unwrap();
            if matches!(ty, CredentialsType::All | CredentialsType::Client) {
                *pending_client_info = None;
            }
            if matches!(ty, CredentialsType::All | CredentialsType::Tokens) {
                *pending_tokens = None;
            }
            Ok(())
        })
    }
}

/// Parse a redirect URI into `(port, path)` for the callback server.
/// From reference/packages/opencode/src/mcp/oauth-provider.ts
/// (`parseRedirectUri`).
pub fn parse_redirect_uri(redirect_uri: Option<&str>) -> (u16, String) {
    let Some(redirect_uri) = redirect_uri else {
        return (OAUTH_CALLBACK_PORT, OAUTH_CALLBACK_PATH.to_string());
    };
    match Url::parse(redirect_uri) {
        Ok(url) => {
            let port = url
                .port()
                .unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
            let path = if url.path().is_empty() {
                OAUTH_CALLBACK_PATH.to_string()
            } else {
                url.path().to_string()
            };
            (port, path)
        }
        Err(_) => (OAUTH_CALLBACK_PORT, OAUTH_CALLBACK_PATH.to_string()),
    }
}

/// The MCP spec resource indicator for the authorization request.
pub fn resource_url(server_url: &Url) -> Url {
    server_url.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_redirect_uri_defaults() {
        assert_eq!(
            parse_redirect_uri(None),
            (OAUTH_CALLBACK_PORT, OAUTH_CALLBACK_PATH.to_string())
        );
        assert_eq!(
            parse_redirect_uri(Some("not a url")),
            (OAUTH_CALLBACK_PORT, OAUTH_CALLBACK_PATH.to_string())
        );
    }

    #[test]
    fn parse_redirect_uri_custom() {
        assert_eq!(
            parse_redirect_uri(Some("http://127.0.0.1:9999/mcp/oauth/callback")),
            (9999, "/mcp/oauth/callback".to_string())
        );
        assert_eq!(
            parse_redirect_uri(Some("http://127.0.0.1:19876/cb")),
            (19876, "/cb".to_string())
        );
    }

    #[test]
    fn client_metadata_matches_reference() {
        let provider = McpOAuthProvider::new(
            "s",
            "https://example.com",
            McpOAuthConfig {
                client_secret: Some("secret".into()),
                scope: Some("read".into()),
                ..Default::default()
            },
            McpOAuthCallbacks::default(),
            Arc::new(McpAuth::new(std::env::temp_dir().join("unused.json"))),
        );
        let metadata = provider.client_metadata();
        assert_eq!(
            serde_json::to_value(&metadata).unwrap(),
            json!({
                "redirect_uris": ["http://127.0.0.1:19876/mcp/oauth/callback"],
                "client_name": "OpenCode",
                "client_uri": "https://opencode.ai",
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "client_secret_post",
                "scope": "read"
            })
        );
    }
}
