//! Native auth hooks for the internal providers shipped by OpenCode.
//!
//! The hooks deliberately keep OAuth state inside the provider hook instead of
//! manufacturing credentials. Browser login uses a short-lived loopback
//! listener protected by PKCE/state, while headless login uses the provider's
//! device-code flow. API-key login remains available for both providers.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::blocking::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{form_urlencoded, Url};
use uuid::Uuid;

use oc_provider::provider::auth::{
    ApiCredential, AuthCallbackResult, AuthHook, AuthOAuthResult, Method, MethodType,
    OAuthCredential,
};

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_ISSUER: &str = "https://auth.openai.com";
const OPENAI_DEVICE_URL: &str = "https://auth.openai.com/codex/device";
const OPENAI_BROWSER_PORT: u16 = 1455;
const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_AUTHORIZE_URL: &str = "https://auth.x.ai/oauth2/authorize";
const XAI_TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
const XAI_DEVICE_URL: &str = "https://auth.x.ai/oauth2/device/code";
const XAI_BROWSER_PORT: u16 = 56121;
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const GITHUB_COPILOT_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";
const OAUTH_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const POLL_SAFETY_MARGIN: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeProvider {
    OpenAi,
    Xai,
    GithubCopilot,
}

impl NativeProvider {
    fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Xai => "xai",
            Self::GithubCopilot => "github-copilot",
        }
    }

    fn browser_port(self) -> u16 {
        match self {
            Self::OpenAi => OPENAI_BROWSER_PORT,
            Self::Xai => XAI_BROWSER_PORT,
            Self::GithubCopilot => unreachable!("GitHub Copilot uses device authorization"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserProvider {
    OpenAi,
    Xai,
}

impl BrowserProvider {
    fn native(self) -> NativeProvider {
        match self {
            Self::OpenAi => NativeProvider::OpenAi,
            Self::Xai => NativeProvider::Xai,
        }
    }
}

enum PendingFlow {
    Browser {
        provider: BrowserProvider,
        receiver: Receiver<Result<String, String>>,
        cancel: Arc<AtomicBool>,
        verifier: String,
        redirect_uri: String,
    },
    Device(DeviceFlow),
}

enum DeviceFlow {
    OpenAi {
        device_auth_id: String,
        user_code: String,
        interval: Duration,
    },
    Xai {
        device_code: String,
        interval: Duration,
        expires_at: Instant,
    },
    GithubCopilot {
        domain: String,
        device_code: String,
        interval: Duration,
        expires_at: Instant,
    },
}

struct NativeAuthHook {
    provider: NativeProvider,
    pending: Mutex<Option<PendingFlow>>,
}

/// An API-key-only auth hook for the internal providers without an OAuth
/// flow (kilo, llmgateway, nvidia, cerebras).
///
/// The reference's `plugin/provider/*.ts` hooks for these providers only add
/// catalog request headers (HTTP-Referer/X-Title/X-Source/billing-origin,
/// X-Cerebras-3rd-Party-Integration) and authenticate by API key. Those
/// header transforms are already applied by the provider registry's custom
/// loaders; this hook supplies the headless API-key login surface with no
/// browser dependency.
struct ApiKeyHook {
    provider: &'static str,
}

impl ApiKeyHook {
    fn new(provider: &'static str) -> Self {
        ApiKeyHook { provider }
    }
}

impl AuthHook for ApiKeyHook {
    fn methods(&self) -> Vec<Method> {
        vec![Method {
            r#type: MethodType::Api,
            label: "Enter API Key".to_string(),
            prompts: None,
        }]
    }

    fn validate(&self, _method_index: usize, _key: &str, value: &str) -> Option<String> {
        if value.trim().is_empty() {
            Some("API key is required".to_string())
        } else {
            None
        }
    }

    fn authorize(
        &self,
        method_index: usize,
        _inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error> {
        Err(anyhow!(
            "{} API-key authorization does not use authorize() (method {method_index})",
            self.provider
        ))
    }

    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
        let key = code
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("API key is required"))?;
        Ok(AuthCallbackResult::Success {
            provider: Some(self.provider.to_string()),
            oauth: None,
            api: Some(ApiCredential {
                key: key.to_string(),
                metadata: None,
            }),
        })
    }
}

/// Return the native internal auth hooks currently supported without a JS
/// runtime. OpenAI includes its browser and headless Codex flows; xAI includes
/// browser/headless OAuth and manual API-key login; GitHub Copilot includes
/// the public/enterprise GitHub device flow. kilo, llmgateway, nvidia, and
/// cerebras expose headless API-key login only (no browser OAuth).
pub fn default_auth_hooks() -> BTreeMap<String, Box<dyn AuthHook>> {
    let mut hooks: BTreeMap<String, Box<dyn AuthHook>> = [
        NativeProvider::OpenAi,
        NativeProvider::Xai,
        NativeProvider::GithubCopilot,
    ]
    .into_iter()
    .map(|provider| {
        (
            provider.id().to_string(),
            Box::new(NativeAuthHook {
                provider,
                pending: Mutex::new(None),
            }) as Box<dyn AuthHook>,
        )
    })
    .collect();
    for provider in ["kilo", "llmgateway", "nvidia", "cerebras"] {
        hooks.insert(
            provider.to_string(),
            Box::new(ApiKeyHook::new(provider)) as Box<dyn AuthHook>,
        );
    }
    hooks
}

impl AuthHook for NativeAuthHook {
    fn methods(&self) -> Vec<Method> {
        let mut methods = Vec::new();
        match self.provider {
            NativeProvider::OpenAi => {
                methods.push(oauth_method("ChatGPT Pro/Plus (browser)"));
                methods.push(oauth_method("ChatGPT Pro/Plus (headless)"));
            }
            NativeProvider::Xai => {
                methods.push(oauth_method("xAI Grok OAuth (SuperGrok Subscription)"));
                methods.push(oauth_method("xAI Grok OAuth (headless / remote / VPS)"));
            }
            NativeProvider::GithubCopilot => methods.push(Method {
                r#type: MethodType::OAuth,
                label: "Login with GitHub Copilot".to_string(),
                prompts: Some(vec![
                    oc_provider::provider::auth::Prompt::Select(
                        oc_provider::provider::auth::SelectPrompt {
                            r#type: "select".to_string(),
                            key: "deploymentType".to_string(),
                            message: "Select GitHub deployment type".to_string(),
                            options: vec![
                                oc_provider::provider::auth::SelectOption {
                                    label: "GitHub.com".to_string(),
                                    value: "github.com".to_string(),
                                    hint: Some("Public".to_string()),
                                },
                                oc_provider::provider::auth::SelectOption {
                                    label: "GitHub Enterprise".to_string(),
                                    value: "enterprise".to_string(),
                                    hint: Some("Data residency or self-hosted".to_string()),
                                },
                            ],
                            when: None,
                        },
                    ),
                    oc_provider::provider::auth::Prompt::Text(
                        oc_provider::provider::auth::TextPrompt {
                            r#type: "text".to_string(),
                            key: "enterpriseUrl".to_string(),
                            message: "Enter your GitHub Enterprise URL or domain".to_string(),
                            placeholder: Some(
                                "company.ghe.com or https://company.ghe.com".to_string(),
                            ),
                            when: Some(oc_provider::provider::auth::When {
                                key: "deploymentType".to_string(),
                                op: oc_provider::provider::auth::WhenOp::Eq,
                                value: "enterprise".to_string(),
                            }),
                        },
                    ),
                ]),
            }),
        }
        if self.provider != NativeProvider::GithubCopilot {
            methods.push(Method {
                r#type: MethodType::Api,
                label: "Manually enter API Key".to_string(),
                prompts: None,
            });
        }
        methods
    }

    fn validate(&self, _method_index: usize, key: &str, value: &str) -> Option<String> {
        if value.trim().is_empty() {
            Some("API key is required".to_string())
        } else if self.provider == NativeProvider::GithubCopilot
            && key == "enterpriseUrl"
            && normalize_domain(value).is_none()
        {
            Some("Please enter a valid URL or domain".to_string())
        } else {
            None
        }
    }

    fn authorize(
        &self,
        method_index: usize,
        _inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error> {
        match (self.provider, method_index) {
            (NativeProvider::OpenAi, 0) => {
                let (result, pending) = browser_authorize(BrowserProvider::OpenAi)?;
                *self.pending.lock().expect("native auth pending lock") = Some(pending);
                Ok(result)
            }
            (NativeProvider::OpenAi, 1) => {
                let (result, flow) = openai_device_authorize()?;
                *self.pending.lock().expect("native auth pending lock") =
                    Some(PendingFlow::Device(flow));
                Ok(result)
            }
            (NativeProvider::Xai, 0) => {
                let (result, pending) = browser_authorize(BrowserProvider::Xai)?;
                *self.pending.lock().expect("native auth pending lock") = Some(pending);
                Ok(result)
            }
            (NativeProvider::Xai, 1) => {
                let (result, flow) = xai_device_authorize()?;
                *self.pending.lock().expect("native auth pending lock") =
                    Some(PendingFlow::Device(flow));
                Ok(result)
            }
            (NativeProvider::GithubCopilot, 0) => {
                let (result, flow) = github_copilot_device_authorize(_inputs)?;
                *self.pending.lock().expect("native auth pending lock") =
                    Some(PendingFlow::Device(flow));
                Ok(result)
            }
            (_, _) => Err(anyhow!(
                "{} API-key authorization does not use authorize()",
                self.provider.id()
            )),
        }
    }

    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
        let pending = self
            .pending
            .lock()
            .expect("native auth pending lock")
            .take();
        if let Some(pending) = pending {
            let credential = match pending {
                PendingFlow::Browser {
                    provider,
                    receiver,
                    cancel,
                    verifier,
                    redirect_uri,
                } => {
                    let result = receiver
                        .recv_timeout(OAUTH_TIMEOUT)
                        .map_err(|_| anyhow!("OAuth callback timed out"))?;
                    cancel.store(true, Ordering::Relaxed);
                    let code = result.map_err(|error| anyhow!(error))?;
                    exchange_browser_code(provider, &code, &verifier, &redirect_uri)?
                }
                PendingFlow::Device(flow) => poll_device_flow(flow)?,
            };
            return Ok(AuthCallbackResult::Success {
                provider: Some(self.provider.id().to_string()),
                oauth: Some(credential),
                api: None,
            });
        }

        let key = code
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("API key is required"))?;
        Ok(AuthCallbackResult::Success {
            provider: Some(self.provider.id().to_string()),
            oauth: None,
            api: Some(ApiCredential {
                key: key.to_string(),
                metadata: None,
            }),
        })
    }

    fn refresh(
        &self,
        credential: &oc_provider::auth::Oauth,
    ) -> Result<Option<OAuthCredential>, anyhow::Error> {
        if credential.refresh.trim().is_empty() {
            return Ok(None);
        }
        let client = http_client()?;
        let (url, client_id) = match self.provider {
            NativeProvider::OpenAi => (format!("{OPENAI_ISSUER}/oauth/token"), OPENAI_CLIENT_ID),
            NativeProvider::Xai => (XAI_TOKEN_URL.to_string(), XAI_CLIENT_ID),
            NativeProvider::GithubCopilot => return Ok(None),
        };
        let response = client
            .post(url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", credential.refresh.as_str()),
                ("client_id", client_id),
            ])
            .send()
            .context("OAuth refresh request failed")?;
        let tokens: TokenResponse = json_response(response, "OAuth refresh")?;
        Ok(Some(token_credential(
            self.provider,
            tokens,
            Some(credential),
        )))
    }
}

fn oauth_method(label: &str) -> Method {
    Method {
        r#type: MethodType::OAuth,
        label: label.to_string(),
        prompts: None,
    }
}

fn browser_authorize(
    provider: BrowserProvider,
) -> Result<(AuthOAuthResult, PendingFlow), anyhow::Error> {
    let (verifier, challenge) = pkce_codes();
    let state = random_string(32);
    let port = provider.native().browser_port();
    let (receiver, cancel) = start_loopback_listener(port, state.clone())?;
    let redirect_uri = match provider {
        BrowserProvider::OpenAi => format!("http://localhost:{port}/auth/callback"),
        BrowserProvider::Xai => format!("http://127.0.0.1:{port}/callback"),
    };
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("response_type", "code");
    query.append_pair(
        "client_id",
        match provider {
            BrowserProvider::OpenAi => OPENAI_CLIENT_ID,
            BrowserProvider::Xai => XAI_CLIENT_ID,
        },
    );
    query.append_pair("redirect_uri", &redirect_uri);
    query.append_pair(
        "scope",
        match provider {
            BrowserProvider::OpenAi => "openid profile email offline_access",
            BrowserProvider::Xai => XAI_SCOPE,
        },
    );
    query.append_pair("code_challenge", &challenge);
    query.append_pair("code_challenge_method", "S256");
    query.append_pair("state", &state);
    if provider == BrowserProvider::OpenAi {
        query.append_pair("id_token_add_organizations", "true");
        query.append_pair("codex_cli_simplified_flow", "true");
        query.append_pair("originator", "opencode");
    } else {
        query.append_pair("nonce", &random_string(32));
        query.append_pair("plan", "generic");
        query.append_pair("referrer", "opencode");
    }
    let authority = match provider {
        BrowserProvider::OpenAi => format!("{OPENAI_ISSUER}/oauth/authorize"),
        BrowserProvider::Xai => XAI_AUTHORIZE_URL.to_string(),
    };
    let url = format!("{authority}?{}", query.finish());
    let label = match provider {
        BrowserProvider::OpenAi => "ChatGPT",
        BrowserProvider::Xai => "xAI",
    };
    Ok((
        AuthOAuthResult {
            url,
            method: oc_provider::provider::auth::CallbackMethod::Auto,
            instructions: format!(
                "Complete {label} authorization in your browser. This window will close automatically."
            ),
        },
        PendingFlow::Browser {
            provider,
            receiver,
            cancel,
            verifier,
            redirect_uri,
        },
    ))
}

fn openai_device_authorize() -> Result<(AuthOAuthResult, DeviceFlow), anyhow::Error> {
    #[derive(Serialize)]
    struct Request<'a> {
        client_id: &'a str,
    }
    #[derive(Deserialize)]
    struct Response {
        device_auth_id: String,
        user_code: String,
        #[serde(default)]
        interval: String,
    }
    let response = http_client()?
        .post(format!("{OPENAI_ISSUER}/api/accounts/deviceauth/usercode"))
        .header("User-Agent", "opencode-rust")
        .json(&Request {
            client_id: OPENAI_CLIENT_ID,
        })
        .send()
        .context("OpenAI device authorization request failed")?;
    let data: Response = json_response(response, "OpenAI device authorization")?;
    let interval = data.interval.parse::<u64>().unwrap_or(5).max(1);
    Ok((
        AuthOAuthResult {
            url: OPENAI_DEVICE_URL.to_string(),
            method: oc_provider::provider::auth::CallbackMethod::Auto,
            instructions: format!("Enter code: {}", data.user_code),
        },
        DeviceFlow::OpenAi {
            device_auth_id: data.device_auth_id,
            user_code: data.user_code,
            interval: Duration::from_secs(interval),
        },
    ))
}

fn xai_device_authorize() -> Result<(AuthOAuthResult, DeviceFlow), anyhow::Error> {
    #[derive(Deserialize)]
    struct Response {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default)]
        verification_uri_complete: Option<String>,
        #[serde(default)]
        expires_in: Option<u64>,
        #[serde(default)]
        interval: Option<u64>,
    }
    let response = http_client()?
        .post(XAI_DEVICE_URL)
        .form(&[("client_id", XAI_CLIENT_ID), ("scope", XAI_SCOPE)])
        .send()
        .context("xAI device authorization request failed")?;
    let data: Response = json_response(response, "xAI device authorization")?;
    let interval = Duration::from_secs(data.interval.unwrap_or(5).max(1));
    let expires_at = Instant::now() + Duration::from_secs(data.expires_in.unwrap_or(300));
    let url = data
        .verification_uri_complete
        .clone()
        .unwrap_or_else(|| data.verification_uri.clone());
    Ok((
        AuthOAuthResult {
            url,
            method: oc_provider::provider::auth::CallbackMethod::Auto,
            instructions: format!(
                "Open {} and enter code: {}",
                data.verification_uri, data.user_code
            ),
        },
        DeviceFlow::Xai {
            device_code: data.device_code,
            interval,
            expires_at,
        },
    ))
}

fn github_copilot_device_authorize(
    inputs: &BTreeMap<String, String>,
) -> Result<(AuthOAuthResult, DeviceFlow), anyhow::Error> {
    #[derive(Serialize)]
    struct Request<'a> {
        client_id: &'a str,
        scope: &'a str,
    }
    #[derive(Deserialize)]
    struct Response {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default)]
        interval: Option<u64>,
        #[serde(default)]
        expires_in: Option<u64>,
    }

    let enterprise = inputs
        .get("deploymentType")
        .map(|value| value == "enterprise")
        .unwrap_or(false);
    let domain = if enterprise {
        normalize_domain(
            inputs
                .get("enterpriseUrl")
                .ok_or_else(|| anyhow!("GitHub Enterprise URL is required"))?,
        )
        .ok_or_else(|| anyhow!("invalid GitHub Enterprise URL or domain"))?
    } else {
        "github.com".to_string()
    };
    let device_url = format!("https://{domain}/login/device/code");
    let response = http_client()?
        .post(device_url)
        .header("Accept", "application/json")
        .header("User-Agent", "opencode-rust")
        .json(&Request {
            client_id: GITHUB_COPILOT_CLIENT_ID,
            scope: "read:user",
        })
        .send()
        .context("GitHub device authorization request failed")?;
    let data: Response = json_response(response, "GitHub device authorization")?;
    let interval = Duration::from_secs(data.interval.unwrap_or(5).max(1));
    let expires_at = Instant::now() + Duration::from_secs(data.expires_in.unwrap_or(900));
    Ok((
        AuthOAuthResult {
            url: data.verification_uri.clone(),
            method: oc_provider::provider::auth::CallbackMethod::Auto,
            instructions: format!("Enter code: {}", data.user_code),
        },
        DeviceFlow::GithubCopilot {
            domain,
            device_code: data.device_code,
            interval,
            expires_at,
        },
    ))
}

fn poll_device_flow(flow: DeviceFlow) -> Result<OAuthCredential, anyhow::Error> {
    match flow {
        DeviceFlow::OpenAi {
            device_auth_id,
            user_code,
            interval,
        } => loop {
            #[derive(Serialize)]
            struct Request<'a> {
                device_auth_id: &'a str,
                user_code: &'a str,
            }
            let response = http_client()?
                .post(format!("{OPENAI_ISSUER}/api/accounts/deviceauth/token"))
                .header("User-Agent", "opencode-rust")
                .json(&Request {
                    device_auth_id: &device_auth_id,
                    user_code: &user_code,
                })
                .send()
                .context("OpenAI device token request failed")?;
            if response.status().is_success() {
                #[derive(Deserialize)]
                struct DeviceToken {
                    authorization_code: String,
                    code_verifier: String,
                }
                let data: DeviceToken = json_response(response, "OpenAI device token")?;
                let redirect_uri = format!("{OPENAI_ISSUER}/deviceauth/callback");
                let token_response = http_client()?
                    .post(format!("{OPENAI_ISSUER}/oauth/token"))
                    .form(&[
                        ("grant_type", "authorization_code"),
                        ("code", data.authorization_code.as_str()),
                        ("redirect_uri", redirect_uri.as_str()),
                        ("client_id", OPENAI_CLIENT_ID),
                        ("code_verifier", data.code_verifier.as_str()),
                    ])
                    .send()
                    .context("OpenAI device token exchange failed")?;
                let tokens: TokenResponse = json_response(token_response, "OpenAI token exchange")?;
                return Ok(token_credential(NativeProvider::OpenAi, tokens, None));
            }
            if response.status().as_u16() != 403 && response.status().as_u16() != 404 {
                bail!(
                    "OpenAI device authorization failed with status {}",
                    response.status()
                );
            }
            thread::sleep(interval + POLL_SAFETY_MARGIN);
        },
        DeviceFlow::Xai {
            device_code,
            mut interval,
            expires_at,
        } => loop {
            if Instant::now() >= expires_at {
                bail!("xAI device authorization timed out");
            }
            let response = http_client()?
                .post(XAI_TOKEN_URL)
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", XAI_CLIENT_ID),
                    ("device_code", device_code.as_str()),
                ])
                .send()
                .context("xAI device token request failed")?;
            let status = response.status();
            let body = response
                .text()
                .context("xAI device token response failed")?;
            if status.is_success() {
                let tokens: TokenResponse =
                    serde_json::from_str(&body).context("invalid xAI device token response")?;
                return Ok(token_credential(NativeProvider::Xai, tokens, None));
            }
            let error = serde_json::from_str::<DeviceError>(&body).unwrap_or_default();
            match error.error.as_deref() {
                Some("authorization_pending") => {}
                Some("slow_down") => interval += Duration::from_secs(5),
                Some("access_denied") | Some("authorization_denied") => {
                    bail!("xAI device authorization was denied")
                }
                Some("expired_token") => bail!("xAI device code expired; please re-run login"),
                _ => bail!(
                    "xAI device token exchange failed ({status}){}",
                    error
                        .error_description
                        .as_deref()
                        .map(|value| format!(": {value}"))
                        .unwrap_or_default()
                ),
            }
            let remaining = expires_at.saturating_duration_since(Instant::now());
            thread::sleep((interval + POLL_SAFETY_MARGIN).min(remaining));
        },
        DeviceFlow::GithubCopilot {
            domain,
            device_code,
            mut interval,
            expires_at,
        } => loop {
            if Instant::now() >= expires_at {
                bail!("GitHub device authorization timed out");
            }
            #[derive(Serialize)]
            struct Request<'a> {
                client_id: &'a str,
                device_code: &'a str,
                grant_type: &'a str,
            }
            #[derive(Deserialize)]
            struct Response {
                access_token: Option<String>,
                error: Option<String>,
                interval: Option<u64>,
            }
            let response = http_client()?
                .post(format!("https://{domain}/login/oauth/access_token"))
                .header("Accept", "application/json")
                .header("User-Agent", "opencode-rust")
                .json(&Request {
                    client_id: GITHUB_COPILOT_CLIENT_ID,
                    device_code: &device_code,
                    grant_type: "urn:ietf:params:oauth:grant-type:device_code",
                })
                .send()
                .context("GitHub device token request failed")?;
            let status = response.status();
            let body = response
                .text()
                .context("GitHub device token response failed")?;
            let data: Response = serde_json::from_str(&body)
                .with_context(|| format!("invalid GitHub device token response ({status})"))?;
            if let Some(access_token) = data.access_token {
                return Ok(OAuthCredential {
                    refresh: access_token.clone(),
                    access: access_token,
                    expires: 0,
                    account_id: None,
                    enterprise_url: (domain != "github.com").then_some(domain),
                });
            }
            match data.error.as_deref() {
                Some("authorization_pending") => {}
                Some("slow_down") => {
                    interval = Duration::from_secs(data.interval.unwrap_or(0).max(5))
                }
                Some("access_denied") => bail!("GitHub device authorization was denied"),
                Some("expired_token") => bail!("GitHub device code expired; please re-run login"),
                Some(error) => bail!("GitHub device token exchange failed: {error}"),
                None if !status.is_success() => {
                    bail!("GitHub device token exchange failed with status {status}")
                }
                None => {}
            }
            let remaining = expires_at.saturating_duration_since(Instant::now());
            thread::sleep((interval + POLL_SAFETY_MARGIN).min(remaining));
        },
    }
}

fn exchange_browser_code(
    provider: BrowserProvider,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthCredential, anyhow::Error> {
    let (url, client_id) = match provider {
        BrowserProvider::OpenAi => (format!("{OPENAI_ISSUER}/oauth/token"), OPENAI_CLIENT_ID),
        BrowserProvider::Xai => (XAI_TOKEN_URL.to_string(), XAI_CLIENT_ID),
    };
    let response = http_client()?
        .post(url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", verifier),
        ])
        .send()
        .context("OAuth token exchange failed")?;
    let tokens: TokenResponse = json_response(response, "OAuth token exchange")?;
    Ok(token_credential(provider.native(), tokens, None))
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct DeviceError {
    error: Option<String>,
    error_description: Option<String>,
}

fn token_credential(
    provider: NativeProvider,
    tokens: TokenResponse,
    previous: Option<&oc_provider::auth::Oauth>,
) -> OAuthCredential {
    let account_id = if provider == NativeProvider::OpenAi {
        tokens
            .id_token
            .as_deref()
            .and_then(openai_account_id)
            .or_else(|| openai_account_id(&tokens.access_token))
    } else {
        None
    };
    OAuthCredential {
        refresh: if tokens.refresh_token.is_empty() {
            previous
                .map(|value| value.refresh.clone())
                .unwrap_or_default()
        } else {
            tokens.refresh_token
        },
        access: tokens.access_token,
        expires: now_millis().saturating_add(tokens.expires_in.unwrap_or(3600) * 1000),
        account_id: account_id.or_else(|| previous.and_then(|value| value.account_id.clone())),
        enterprise_url: previous.and_then(|value| value.enterprise_url.clone()),
    }
}

fn openai_account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims
        .get("chatgpt_account_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            claims
                .get("organizations")
                .and_then(serde_json::Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
}

fn http_client() -> Result<Client, anyhow::Error> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build OAuth HTTP client")
}

fn normalize_domain(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return None;
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let parsed = Url::parse(&candidate).ok()?;
    let host = parsed.host_str()?.to_string();
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Some(format!("{host}{port}"))
}

fn json_response<T: DeserializeOwned>(
    response: reqwest::blocking::Response,
    operation: &str,
) -> Result<T, anyhow::Error> {
    let status = response.status();
    let body = response.text().context("failed to read OAuth response")?;
    if !status.is_success() {
        bail!("{operation} failed with status {status}: {body}");
    }
    serde_json::from_str(&body).with_context(|| format!("invalid {operation} response"))
}

fn pkce_codes() -> (String, String) {
    let verifier = random_string(64);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn random_string(length: usize) -> String {
    let mut value = String::new();
    while value.len() < length {
        value.push_str(Uuid::new_v4().simple().to_string().as_str());
    }
    value.truncate(length);
    value
}

fn start_loopback_listener(
    port: u16,
    expected_state: String,
) -> Result<(Receiver<Result<String, String>>, Arc<AtomicBool>), anyhow::Error> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("failed to bind OAuth callback port {port}"))?;
    listener
        .set_nonblocking(true)
        .context("failed to configure OAuth callback listener")?;
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    thread::spawn(move || {
        let deadline = Instant::now() + OAUTH_TIMEOUT;
        while !thread_cancel.load(Ordering::Relaxed) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let result = read_callback(&mut stream, &expected_state);
                    let response = if result.is_ok() {
                        "OAuth login completed. You may close this window."
                    } else {
                        "OAuth login failed. You may close this window."
                    };
                    let _ = write_http_response(&mut stream, response);
                    let _ = sender.send(result);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    let _ = sender.send(Err(format!("OAuth callback listener failed: {error}")));
                    break;
                }
            }
        }
    });
    Ok((receiver, cancel))
}

fn read_callback(stream: &mut std::net::TcpStream, expected_state: &str) -> Result<String, String> {
    let mut bytes = [0u8; 8192];
    let size = stream
        .read(&mut bytes)
        .map_err(|error| format!("failed to read OAuth callback: {error}"))?;
    let request = String::from_utf8_lossy(&bytes[..size]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "invalid OAuth callback request".to_string())?;
    let url = Url::parse(&format!("http://localhost{target}"))
        .map_err(|error| format!("invalid OAuth callback URL: {error}"))?;
    if let Some((_, error)) = url.query_pairs().find(|(key, _)| key == "error") {
        return Err(format!("OAuth provider returned {error}"));
    }
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| "OAuth callback omitted state".to_string())?;
    if state != expected_state {
        return Err("invalid OAuth state; possible CSRF attack".to_string());
    }
    url.query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| "OAuth callback omitted authorization code".to_string())
}

fn write_http_response(stream: &mut std::net::TcpStream, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{default_auth_hooks, openai_account_id};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use oc_provider::provider::auth::{AuthCallbackResult, MethodType};

    #[test]
    fn exposes_native_oauth_and_manual_api_methods() {
        let hooks = default_auth_hooks();
        assert_eq!(
            hooks.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "cerebras",
                "github-copilot",
                "kilo",
                "llmgateway",
                "nvidia",
                "openai",
                "xai"
            ]
        );
        for provider in ["openai", "xai"] {
            let methods = hooks[provider].methods();
            assert_eq!(methods.len(), 3);
            assert_eq!(methods[0].r#type, MethodType::OAuth);
            assert_eq!(methods[1].r#type, MethodType::OAuth);
            assert_eq!(methods[2].r#type, MethodType::Api);
            assert!(methods[2].label.contains("API Key"));
        }
        let copilot = hooks["github-copilot"].methods();
        assert_eq!(copilot.len(), 1);
        assert_eq!(copilot[0].r#type, MethodType::OAuth);
        assert_eq!(copilot[0].label, "Login with GitHub Copilot");
        assert_eq!(copilot[0].prompts.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn api_key_only_hooks_expose_single_api_method() {
        let hooks = default_auth_hooks();
        for provider in ["kilo", "llmgateway", "nvidia", "cerebras"] {
            let methods = hooks[provider].methods();
            assert_eq!(methods.len(), 1, "{provider} should expose one method");
            assert_eq!(
                methods[0].r#type,
                MethodType::Api,
                "{provider} is API-key only"
            );
            assert!(methods[0].prompts.is_none());
        }
    }

    #[test]
    fn api_key_only_hooks_store_manual_key_and_reject_empty() {
        let hooks = default_auth_hooks();
        for provider in ["kilo", "llmgateway", "nvidia", "cerebras"] {
            let result = hooks[provider].callback(Some(" sk-nvidia ")).unwrap();
            let AuthCallbackResult::Success {
                oauth,
                api,
                provider: stored,
            } = result
            else {
                panic!("{provider}: expected successful API credential")
            };
            assert!(oauth.is_none());
            assert_eq!(stored.as_deref(), Some(provider));
            assert_eq!(api.unwrap().key, "sk-nvidia");
            assert!(hooks[provider].callback(Some(" ")).is_err());
            assert!(hooks[provider].callback(None).is_err());
        }
    }

    #[test]
    fn stores_manual_key_without_fabricating_oauth() {
        let hooks = default_auth_hooks();
        let result = hooks["openai"].callback(Some(" sk-test ")).unwrap();
        let AuthCallbackResult::Success { oauth, api, .. } = result else {
            panic!("expected successful API credential")
        };
        assert!(oauth.is_none());
        assert_eq!(api.unwrap().key, "sk-test");
        assert!(hooks["openai"].callback(Some(" ")).is_err());
    }

    #[test]
    fn extracts_openai_account_id_from_nested_claims() {
        let payload = URL_SAFE_NO_PAD
            .encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct_test"}}"#);
        let token = format!("header.{payload}.signature");
        assert_eq!(openai_account_id(&token).as_deref(), Some("acct_test"));
    }
}
