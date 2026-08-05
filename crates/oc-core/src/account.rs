//! Account and OAuth types.
//! From reference/packages/core/src/account.ts

use serde::{Deserialize, Serialize};

use crate::ids::{AccountId, OrgId};

/// `Account.Info` — `{ id, email, url, active_org_id }`.
/// `active_org_id` is `NullOr(OrgID)` — `null` is significant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub id: AccountId,
    pub email: String,
    pub url: String,
    #[serde(rename = "active_org_id")]
    pub active_org_id: Option<OrgId>,
}

/// `Account.Org` — `{ id, name }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Org {
    pub id: OrgId,
    pub name: String,
}

/// `AccountTransportError` — `_tag: "AccountTransportError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountTransportError {
    pub _tag: String,
    pub method: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl AccountTransportError {
    pub fn new(
        method: String,
        url: String,
        description: Option<String>,
        cause: Option<String>,
    ) -> Self {
        AccountTransportError {
            _tag: "AccountTransportError".to_string(),
            method,
            url,
            description,
            cause,
        }
    }
}

impl std::fmt::Display for AccountTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Could not reach {} {}.\nThis failed before the server returned an HTTP response.\n{}\nCheck your network, proxy, or VPN configuration and try again.",
            self.method,
            self.url,
            self.description.clone().unwrap_or_default()
        )
    }
}

impl std::error::Error for AccountTransportError {}

/// `AccountRepoError` — `_tag: "AccountRepoError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRepoError {
    pub _tag: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// `AccountServiceError` — `_tag: "AccountServiceError"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountServiceError {
    pub _tag: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// Effect `Duration` wire shape (`{ _tag: "Duration", millis }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Duration {
    pub _tag: String,
    pub millis: u64,
}

impl Duration {
    pub fn from_millis(millis: u64) -> Self {
        Duration {
            _tag: "Duration".to_string(),
            millis,
        }
    }
}

/// `Account.Login` — `{ code, user, url, server, expiry, interval }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub code: String,
    pub user: String,
    pub url: String,
    pub server: String,
    pub expiry: Duration,
    pub interval: Duration,
}

/// `PollSuccess`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollSuccess {
    pub _tag: String,
    pub email: String,
}

/// `PollPending`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollPending {
    pub _tag: String,
}

/// `PollSlow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollSlow {
    pub _tag: String,
}

/// `PollExpired`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollExpired {
    pub _tag: String,
}

/// `PollDenied`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollDenied {
    pub _tag: String,
}

/// `PollError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollError {
    pub _tag: String,
    pub cause: String,
}

/// `PollResult` — tagged union on `_tag`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "_tag")]
pub enum PollResult {
    #[serde(rename = "PollSuccess")]
    Success(PollSuccess),
    #[serde(rename = "PollPending")]
    Pending(PollPending),
    #[serde(rename = "PollSlow")]
    Slow(PollSlow),
    #[serde(rename = "PollExpired")]
    Expired(PollExpired),
    #[serde(rename = "PollDenied")]
    Denied(PollDenied),
    #[serde(rename = "PollError")]
    Error(PollError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn account_info_json() {
        let info = AccountInfo {
            id: AccountId("abc".to_string()),
            email: "a@b.c".to_string(),
            url: "https://opencode.ai".to_string(),
            active_org_id: None,
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({ "id": "abc", "email": "a@b.c", "url": "https://opencode.ai", "active_org_id": null })
        );
    }

    #[test]
    fn poll_result_json() {
        let result = PollResult::Pending(PollPending {
            _tag: "PollPending".to_string(),
        });
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({ "_tag": "PollPending" })
        );
    }
}
