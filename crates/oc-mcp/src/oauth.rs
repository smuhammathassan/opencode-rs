//! OAuth 2.0 client flow for MCP remote servers.
//!
//! Port of `@modelcontextprotocol/sdk@1.29.0` `client/auth.js` (RFC 8414
//! discovery, RFC 7591 dynamic client registration, authorization code + PKCE,
//! refresh tokens) as wired up by `reference/packages/opencode/src/mcp/index.ts`
//! and `reference/packages/opencode/src/mcp/oauth-provider.ts`.

use std::sync::Arc;

use reqwest::Client as HttpClient;
use url::Url;

use crate::crypto::{code_challenge, generate_code_verifier};
use crate::oauth_provider::{
    AuthorizationServerMetadata, CredentialsType, OAuthClientInformation,
    OAuthClientInformationFull, OAuthClientMetadata, OAuthClientProvider, OAuthTokens,
    ProtectedResourceMetadata,
};
use crate::Result;

/// Result of a completed (non-interactive) auth exchange.
pub struct AuthOutcome {
    pub tokens: OAuthTokens,
}

/// Options for the OAuth flow.
pub struct AuthOptions {
    pub server_url: Url,
    pub authorization_code: Option<String>,
    pub scope: Option<String>,
    pub resource_metadata_url: Option<Url>,
}

pub struct AuthClient {
    http: HttpClient,
}

impl Default for AuthClient {
    fn default() -> Self {
        AuthClient {
            http: HttpClient::new(),
        }
    }
}

impl AuthClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the full OAuth flow. Returns tokens on success; returns
    /// `Error::Unauthorized` when an interactive authorization redirect was
    /// started and a code is required to complete the exchange.
    pub async fn auth(
        &self,
        provider: &dyn OAuthClientProvider,
        options: &AuthOptions,
    ) -> Result<AuthOutcome> {
        let metadata = self
            .discover_authorization_server(&options.server_url)
            .await?;

        let resource_metadata = match &options.resource_metadata_url {
            Some(url) => self.get_protected_resource_metadata(url).await?,
            None => None,
        };
        let resolved_scope = determine_scope(
            options.scope.as_deref(),
            resource_metadata.as_ref(),
            &metadata,
            &provider.client_metadata(),
        );

        let client_information = self.ensure_client_information(provider, &metadata).await?;

        if let Some(code) = &options.authorization_code {
            let tokens = self
                .exchange_code(&metadata, &client_information, provider, code)
                .await?;
            provider.save_tokens(tokens.clone()).await?;
            return Ok(AuthOutcome { tokens });
        }

        if let Some(tokens) = self.stored_or_refreshed(provider, &metadata).await? {
            return Ok(AuthOutcome { tokens });
        }

        let redirect_url = provider.redirect_url();
        let code_verifier = generate_code_verifier();
        provider.save_code_verifier(&code_verifier).await?;
        let state = provider.state().await?;
        let authorization_url = build_authorization_url(
            &metadata.authorization_endpoint,
            &client_information,
            &redirect_url,
            &code_verifier,
            &state,
            resolved_scope.as_deref(),
            &options.server_url,
        );
        provider
            .redirect_to_authorization(&authorization_url)
            .await?;
        Err(crate::Error::unauthorized("OAuth authorization required"))
    }

    /// Complete an interactive flow with an authorization code.
    pub async fn finish_with_code(
        &self,
        provider: &dyn OAuthClientProvider,
        server_url: &Url,
        code: &str,
    ) -> Result<AuthOutcome> {
        let metadata = self.discover_authorization_server(server_url).await?;
        let client_information = self.ensure_client_information(provider, &metadata).await?;
        let tokens = self
            .exchange_code(&metadata, &client_information, provider, code)
            .await?;
        provider.save_tokens(tokens.clone()).await?;
        Ok(AuthOutcome { tokens })
    }

    /// Discovery: try `/.well-known/oauth-authorization-server`, then the
    /// server URL itself (the MCP spec requires the well-known location; many
    /// servers serve metadata directly).
    pub async fn discover_authorization_server(
        &self,
        server_url: &Url,
    ) -> Result<AuthorizationServerMetadata> {
        let mut well_known = server_url.clone();
        well_known.set_path("/.well-known/oauth-authorization-server");
        well_known.set_query(None);

        let mut last_error: Option<String> = None;
        for candidate in [well_known, server_url.clone()] {
            let response = self
                .http
                .get(candidate)
                .header("Accept", "application/json")
                .send()
                .await?;
            if response.status().is_success() {
                if let Ok(metadata) = response.json::<AuthorizationServerMetadata>().await {
                    if !metadata.authorization_endpoint.is_empty()
                        && !metadata.token_endpoint.is_empty()
                    {
                        return Ok(metadata);
                    }
                }
            } else {
                last_error = Some(format!(
                    "metadata request failed with {}",
                    response.status()
                ));
            }
        }
        Err(crate::Error::OAuth(format!(
            "Unable to discover OAuth authorization server metadata for {server_url}: {}",
            last_error.unwrap_or_else(|| "invalid metadata".into())
        )))
    }

    pub async fn get_protected_resource_metadata(
        &self,
        url: &Url,
    ) -> Result<Option<ProtectedResourceMetadata>> {
        let response = self
            .http
            .get(url.clone())
            .header("Accept", "application/json")
            .send()
            .await?;
        if response.status().is_success() {
            return Ok(response.json::<ProtectedResourceMetadata>().await.ok());
        }
        Ok(None)
    }

    async fn ensure_client_information(
        &self,
        provider: &dyn OAuthClientProvider,
        metadata: &AuthorizationServerMetadata,
    ) -> Result<OAuthClientInformation> {
        if let Some(info) = provider.client_information().await? {
            return Ok(info);
        }
        let Some(registration_endpoint) = &metadata.registration_endpoint else {
            return Err(crate::Error::unauthorized(
                "Server does not support dynamic client registration",
            ));
        };
        match self
            .register_client(registration_endpoint, &provider.client_metadata())
            .await
        {
            Ok(info) => {
                provider.save_client_information(info.clone()).await?;
                Ok(OAuthClientInformation {
                    client_id: info.client_id,
                    client_secret: info.client_secret,
                })
            }
            Err(error) => {
                let message = error.to_string();
                if message.contains("registration") || message.contains("client_id") {
                    Err(crate::Error::unauthorized(&message))
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Dynamic client registration (RFC 7591).
    pub async fn register_client(
        &self,
        registration_endpoint: &str,
        metadata: &OAuthClientMetadata,
    ) -> Result<OAuthClientInformationFull> {
        let response = self
            .http
            .post(registration_endpoint)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(metadata)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(crate::Error::OAuth(format!(
                "Dynamic client registration failed ({status}): {text}"
            )));
        }
        let info: OAuthClientInformationFull = response.json().await?;
        if info.client_id.trim().is_empty() {
            return Err(crate::Error::OAuth(
                "Dynamic client registration response missing client_id".into(),
            ));
        }
        Ok(info)
    }

    /// Exchange an authorization code (RFC 6749 §4.1.3).
    pub async fn exchange_code(
        &self,
        metadata: &AuthorizationServerMetadata,
        client_information: &OAuthClientInformation,
        provider: &dyn OAuthClientProvider,
        code: &str,
    ) -> Result<OAuthTokens> {
        let code_verifier = provider.code_verifier().await?;
        let redirect_url = provider.redirect_url();

        let mut form: Vec<(String, String)> = vec![
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), code.into()),
            ("code_verifier".into(), code_verifier),
            ("redirect_uri".into(), redirect_url),
            ("client_id".into(), client_information.client_id.clone()),
        ];
        if let Some(secret) = &client_information.client_secret {
            form.push(("client_secret".into(), secret.clone()));
        }
        self.request_tokens(&metadata.token_endpoint, form).await
    }

    /// Return stored tokens, refreshing if expired and possible.
    async fn stored_or_refreshed(
        &self,
        provider: &dyn OAuthClientProvider,
        metadata: &AuthorizationServerMetadata,
    ) -> Result<Option<OAuthTokens>> {
        let Some(tokens) = provider.tokens().await? else {
            return Ok(None);
        };
        let expires_in = tokens.expires_in.unwrap_or(u64::MAX);
        if expires_in > 0 {
            return Ok(Some(tokens));
        }
        let Some(refresh_token) = tokens.refresh_token.clone() else {
            provider
                .invalidate_credentials(CredentialsType::Tokens)
                .await?;
            return Ok(None);
        };
        let client_information = match provider.client_information().await? {
            Some(info) => info,
            None => {
                provider
                    .invalidate_credentials(CredentialsType::All)
                    .await?;
                return Ok(None);
            }
        };
        let mut form: Vec<(String, String)> = vec![
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), refresh_token),
            ("client_id".into(), client_information.client_id.clone()),
        ];
        if let Some(secret) = &client_information.client_secret {
            form.push(("client_secret".into(), secret.clone()));
        }
        if let Some(scope) = &tokens.scope {
            form.push(("scope".into(), scope.clone()));
        }
        match self.request_tokens(&metadata.token_endpoint, form).await {
            Ok(refreshed) => {
                provider.save_tokens(refreshed.clone()).await?;
                Ok(Some(refreshed))
            }
            Err(error) => {
                provider
                    .invalidate_credentials(CredentialsType::Tokens)
                    .await?;
                Err(error)
            }
        }
    }

    async fn request_tokens(
        &self,
        token_endpoint: &str,
        form: Vec<(String, String)>,
    ) -> Result<OAuthTokens> {
        let response = self
            .http
            .post(token_endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&form)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(crate::Error::OAuth(format!(
                "Token request failed ({status}): {text}"
            )));
        }
        let tokens: OAuthTokens = response.json().await?;
        if tokens.access_token.is_empty() {
            return Err(crate::Error::OAuth(
                "Token response missing access_token".into(),
            ));
        }
        Ok(tokens)
    }
}

/// Selects scopes per the MCP spec and augments them for refresh token support.
/// From the opencode patch to `@modelcontextprotocol/sdk` `client/auth.js`
/// (`determineScope`).
pub fn determine_scope(
    requested_scope: Option<&str>,
    resource_metadata: Option<&ProtectedResourceMetadata>,
    auth_server_metadata: &AuthorizationServerMetadata,
    client_metadata: &OAuthClientMetadata,
) -> Option<String> {
    let mut effective_scope = requested_scope
        .map(str::to_string)
        .or_else(|| {
            resource_metadata
                .and_then(|metadata| metadata.scopes_supported.as_ref())
                .map(|scopes| scopes.join(" "))
        })
        .or_else(|| client_metadata.scope.clone());

    if let Some(scope) = &effective_scope {
        let supports_offline = auth_server_metadata
            .scopes_supported
            .as_ref()
            .map(|scopes| scopes.iter().any(|scope| scope == "offline_access"))
            .unwrap_or(false);
        let supports_refresh = client_metadata
            .grant_types
            .as_ref()
            .map(|grant_types| grant_types.iter().any(|grant| grant == "refresh_token"))
            .unwrap_or(false);
        let has_offline = scope.split(' ').any(|word| word == "offline_access");
        if supports_offline && !has_offline && supports_refresh {
            effective_scope = Some(format!("{scope} offline_access"));
        }
    }
    effective_scope
}

/// Build the authorization URL (SDK `startAuthorization`).
pub fn build_authorization_url(
    authorization_endpoint: &str,
    client_information: &OAuthClientInformation,
    redirect_url: &str,
    code_verifier: &str,
    state: &str,
    scope: Option<&str>,
    resource: &Url,
) -> Url {
    let mut url = Url::parse(authorization_endpoint)
        .unwrap_or_else(|_| Url::parse("http://127.0.0.1/").expect("static URL"));
    url.set_query(None);
    let mut pairs: Vec<(String, String)> = vec![
        ("response_type".into(), "code".into()),
        ("client_id".into(), client_information.client_id.clone()),
        ("redirect_uri".into(), redirect_url.into()),
        ("code_challenge".into(), code_challenge(code_verifier)),
        ("code_challenge_method".into(), "S256".into()),
    ];
    if !state.is_empty() {
        pairs.push(("state".into(), state.into()));
    }
    if let Some(scope) = scope {
        pairs.push(("scope".into(), scope.into()));
        if scope.split(' ').any(|word| word == "offline_access") {
            pairs.push(("prompt".into(), "consent".into()));
        }
    }
    pairs.push(("resource".into(), resource.to_string()));
    let query = pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode_param(key), encode_param(value)))
        .collect::<Vec<_>>()
        .join("&");
    url.set_query(Some(&query));
    url
}

fn encode_param(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Extract the `WWW-Authenticate: Bearer` params (`resource_metadata_url`,
/// `scope`, `error`) from a 401/403 response.
/// From the SDK `extractWWWAuthenticateParams`.
pub fn extract_www_authenticate_params(
    challenge: &str,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    if !challenge.to_ascii_lowercase().starts_with("bearer") {
        return None;
    }
    let params = challenge
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ");
    let mut resource = None;
    let mut scope = None;
    let mut error = None;
    for part in split_www_auth_params(&params) {
        let mut split = part.splitn(2, '=');
        let key = split.next().unwrap_or("").trim().to_ascii_lowercase();
        let value = split.next().unwrap_or("").trim();
        let value = value.trim_matches('"').to_string();
        match key.as_str() {
            "resource" => resource = Some(value),
            "scope" => scope = Some(value),
            "error" => error = Some(value),
            _ => {}
        }
    }
    Some((resource, scope, error))
}

fn split_www_auth_params(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in input.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            ',' if !in_quotes => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// Shared HTTP client factory.
pub fn http_client() -> HttpClient {
    HttpClient::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .unwrap_or_else(|_| HttpClient::new())
}

pub type SharedAuthClient = Arc<AuthClient>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_provider::McpOAuthConfig;

    fn metadata() -> AuthorizationServerMetadata {
        serde_json::from_value(serde_json::json!({
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "registration_endpoint": "https://auth.example.com/register",
            "scopes_supported": ["read", "write", "offline_access"]
        }))
        .unwrap()
    }

    #[test]
    fn determine_scope_augments_offline_access() {
        let auth_server = metadata();
        let client_metadata = OAuthClientMetadata {
            redirect_uris: vec![],
            client_name: None,
            client_uri: None,
            grant_types: Some(vec!["authorization_code".into(), "refresh_token".into()]),
            response_types: Some(vec!["code".into()]),
            token_endpoint_auth_method: Some("none".into()),
            scope: None,
        };
        assert_eq!(
            determine_scope(Some("read"), None, &auth_server, &client_metadata),
            Some("read offline_access".into())
        );
        assert_eq!(
            determine_scope(
                Some("read offline_access"),
                None,
                &auth_server,
                &client_metadata
            ),
            Some("read offline_access".into())
        );
    }

    #[test]
    fn determine_scope_uses_resource_scopes() {
        let auth_server = metadata();
        let client_metadata = OAuthClientMetadata {
            grant_types: Some(vec!["authorization_code".into(), "refresh_token".into()]),
            ..Default::default()
        };
        let resource = ProtectedResourceMetadata {
            scopes_supported: Some(vec!["mcp".into(), "openid".into()]),
            ..Default::default()
        };
        assert_eq!(
            determine_scope(None, Some(&resource), &auth_server, &client_metadata),
            Some("mcp openid offline_access".into())
        );
    }

    #[test]
    fn build_authorization_url_parameters() {
        let client = OAuthClientInformation {
            client_id: "client-1".into(),
            client_secret: None,
        };
        let url = build_authorization_url(
            "https://auth.example.com/authorize",
            &client,
            "http://127.0.0.1:19876/mcp/oauth/callback",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "state-123",
            Some("read"),
            &Url::parse("https://mcp.example.com/mcp").unwrap(),
        );
        let query = url.query().unwrap();
        assert!(query.contains("response_type=code"));
        assert!(query.contains("client_id=client-1"));
        assert!(
            query.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A19876%2Fmcp%2Foauth%2Fcallback")
        );
        assert!(query.contains("code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(query.contains("code_challenge_method=S256"));
        assert!(query.contains("state=state-123"));
        assert!(query.contains("scope=read"));
        assert!(query.contains("resource=https%3A%2F%2Fmcp.example.com%2Fmcp"));
    }

    #[test]
    fn extract_www_authenticate_parses_params() {
        let (resource, scope, error) = extract_www_authenticate_params(
            r#"Bearer resource="https://auth.example.com", scope="mcp read", error="invalid_token""#,
        )
        .unwrap();
        assert_eq!(resource.as_deref(), Some("https://auth.example.com"));
        assert_eq!(scope.as_deref(), Some("mcp read"));
        assert_eq!(error.as_deref(), Some("invalid_token"));
    }

    #[test]
    fn config_from_none_is_default() {
        let config = McpOAuthConfig::from_config(None);
        assert_eq!(config, McpOAuthConfig::default());
    }
}
