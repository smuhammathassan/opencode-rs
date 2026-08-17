//! Share transport selection.
//!
//! The reference Share-next service uses the legacy `/api/share` resource
//! without an account and the account-backed `/api/shares` resource when an
//! active account, organization, and bearer token are available.  This port
//! does not yet have the reference Account service, so the account variant is
//! deliberately opt-in through an explicit `shareNext` adapter configuration.
//! That keeps the capability honest and prevents an unauthed request from
//! being sent merely because an enterprise URL happens to be configured.

use serde_json::Value;
use std::fmt;
use url::Url;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareMode {
    Disabled,
    Legacy,
    Account,
    Unavailable,
}

#[allow(dead_code)]
impl ShareMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Legacy => "legacy",
            Self::Account => "account",
            Self::Unavailable => "unavailable",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShareCapability {
    pub mode: ShareMode,
    pub available: bool,
    pub account_based: bool,
    pub base_url: Option<String>,
    pub resource: Option<&'static str>,
    pub reason: Option<String>,
}

impl ShareCapability {
    pub(crate) fn from_config(config: &Value) -> Self {
        match resolve_with(config, |name| std::env::var(name).ok()) {
            Ok(Some(endpoint)) => Self {
                mode: endpoint.mode(),
                available: true,
                account_based: endpoint.is_account_based(),
                base_url: Some(endpoint.base_url().to_string()),
                resource: Some(endpoint.resource()),
                reason: None,
            },
            Ok(None) => Self {
                mode: ShareMode::Disabled,
                available: false,
                account_based: false,
                base_url: None,
                resource: None,
                reason: Some("sharing is disabled in configuration".into()),
            },
            Err(error) => Self {
                mode: ShareMode::Unavailable,
                available: false,
                account_based: true,
                base_url: None,
                resource: Some("shares"),
                reason: Some(error.to_string()),
            },
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ShareEndpoint {
    Legacy {
        base_url: String,
    },
    Account {
        base_url: String,
        bearer_token: String,
        org_id: String,
    },
}

#[allow(dead_code)]
impl ShareEndpoint {
    pub(crate) fn mode(&self) -> ShareMode {
        match self {
            Self::Legacy { .. } => ShareMode::Legacy,
            Self::Account { .. } => ShareMode::Account,
        }
    }

    pub(crate) fn is_account_based(&self) -> bool {
        matches!(self, Self::Account { .. })
    }

    pub(crate) fn base_url(&self) -> &str {
        match self {
            Self::Legacy { base_url } | Self::Account { base_url, .. } => base_url,
        }
    }

    pub(crate) fn resource(&self) -> &'static str {
        match self {
            Self::Legacy { .. } => "share",
            Self::Account { .. } => "shares",
        }
    }

    pub(crate) fn url(&self, share_id: Option<&str>, suffix: &str) -> String {
        let path = match share_id {
            Some(share_id) => format!("/api/{}/{share_id}{suffix}", self.resource()),
            None => format!("/api/{}", self.resource()),
        };
        format!("{}{path}", self.base_url())
    }

    pub(crate) fn apply_headers(
        &self,
        request: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        match self {
            Self::Legacy { .. } => request,
            Self::Account {
                bearer_token,
                org_id,
                ..
            } => request
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {bearer_token}"),
                )
                .header("x-org-id", org_id),
        }
    }
}

impl fmt::Debug for ShareEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Legacy { base_url } => formatter
                .debug_struct("LegacyShareEndpoint")
                .field("base_url", base_url)
                .finish(),
            Self::Account {
                base_url, org_id, ..
            } => formatter
                .debug_struct("AccountShareEndpoint")
                .field("base_url", base_url)
                .field("org_id", org_id)
                .field("bearer_token", &"<redacted>")
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShareConfigError(String);

impl fmt::Display for ShareConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ShareConfigError {}

/// Resolve the configured share endpoint using the process environment for
/// the explicitly named account token variable.
pub(crate) fn resolve(config: &Value) -> Result<Option<ShareEndpoint>, ShareConfigError> {
    resolve_with(config, |name| std::env::var(name).ok())
}

/// Testable resolver seam. The production path supplies `std::env::var`; the
/// adapter never accepts an inline token from JSON configuration.
pub(crate) fn resolve_with<F>(
    config: &Value,
    token: F,
) -> Result<Option<ShareEndpoint>, ShareConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    if config.get("share").and_then(Value::as_str) == Some("disabled") {
        return Ok(None);
    }

    let Some(raw) = config.get("shareNext").or_else(|| config.get("share_next")) else {
        return Ok(Some(ShareEndpoint::Legacy {
            base_url: legacy_base_url(config),
        }));
    };
    let object = raw.as_object().ok_or_else(|| {
        ShareConfigError("shareNext must be an object when account sharing is configured".into())
    })?;

    let mode = object
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("account");
    if mode != "account" {
        return Err(ShareConfigError(format!(
            "shareNext.mode must be \"account\" (got \"{mode}\")"
        )));
    }

    let base_url = object
        .get("url")
        .or_else(|| object.get("baseUrl"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ShareConfigError("shareNext.url is required for account sharing".into()))?
        .trim_end_matches('/')
        .to_string();
    validate_base_url(&base_url)?;

    let org_id = object
        .get("orgID")
        .or_else(|| object.get("activeOrgID"))
        .or_else(|| object.get("active_org_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ShareConfigError("shareNext.orgID is required for account sharing".into()))?
        .to_string();

    let token_env = object
        .get("tokenEnv")
        .or_else(|| object.get("token_env"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ShareConfigError(
                "shareNext.tokenEnv is required because the account service is not wired; "
                    .to_string()
                    + "set it to the environment variable containing the bearer token",
            )
        })?;
    let bearer_token = token(token_env)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ShareConfigError(format!(
                "shareNext token environment variable \"{token_env}\" is not set"
            ))
        })?;

    Ok(Some(ShareEndpoint::Account {
        base_url,
        bearer_token,
        org_id,
    }))
}

fn validate_base_url(base_url: &str) -> Result<(), ShareConfigError> {
    let parsed = Url::parse(base_url)
        .map_err(|_| ShareConfigError("shareNext.url must be an absolute URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ShareConfigError(
            "shareNext.url must use http or https and include a host".into(),
        ));
    }
    Ok(())
}

fn legacy_base_url(config: &Value) -> String {
    config
        .get("enterprise")
        .and_then(|enterprise| enterprise.get("url"))
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .unwrap_or("https://opncd.ai")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_to_legacy_endpoint() {
        let endpoint = resolve_with(
            &json!({
                "enterprise": { "url": "https://share.example.test/" }
            }),
            |_| None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(endpoint.mode(), ShareMode::Legacy);
        assert_eq!(endpoint.resource(), "share");
        assert_eq!(
            endpoint.url(None, ""),
            "https://share.example.test/api/share"
        );
    }

    #[test]
    fn account_endpoint_requires_explicit_capability_and_uses_bearer_headers() {
        let endpoint = resolve_with(
            &json!({
                "shareNext": {
                    "mode": "account",
                    "url": "https://account.example.test/",
                    "orgID": "org_test",
                    "tokenEnv": "OPENCODE_TEST_ACCOUNT_TOKEN"
                }
            }),
            |name| (name == "OPENCODE_TEST_ACCOUNT_TOKEN").then(|| "token_test".into()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(endpoint.mode(), ShareMode::Account);
        assert_eq!(endpoint.resource(), "shares");
        assert_eq!(
            endpoint.url(Some("share_test"), "/sync"),
            "https://account.example.test/api/shares/share_test/sync"
        );
    }

    #[test]
    fn missing_account_token_is_unavailable_instead_of_falling_back() {
        let capability = ShareCapability::from_config(&json!({
            "shareNext": {
                "url": "https://account.example.test",
                "orgID": "org_test",
                "tokenEnv": "OPENCODE_TEST_MISSING_TOKEN"
            }
        }));
        assert_eq!(capability.mode, ShareMode::Unavailable);
        assert!(!capability.available);
        assert!(capability
            .reason
            .as_deref()
            .unwrap()
            .contains("OPENCODE_TEST_MISSING_TOKEN"));
    }
}
