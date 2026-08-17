//! Adapter from executable QuickJS plugin auth hooks to the provider service.
//!
//! Plugin function values stay on the manager's owner thread. This module
//! exposes only the serializable method descriptors to the provider service
//! and forwards validate/authorize/callback calls back through the manager.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use serde_json::Value;

use oc_plugin::{
    AuthAuthorizeRequest, AuthCallbackRequest, AuthValidateRequest, PluginAuthMethodSummary,
    PluginAuthMethodType, PluginAuthPromptSummary, PluginAuthWhenSummary, PluginLoadReport,
    PluginManager,
};
use oc_provider::provider::auth::{
    ApiCredential, AuthCallbackResult, AuthHook, AuthOAuthResult, Method, MethodType,
    OAuthCredential, Prompt, SelectOption, SelectPrompt, TextPrompt, When, WhenOp,
};

/// Build the provider-auth registry advertised by successfully loaded plugins.
#[allow(dead_code)]
pub fn from_plugin_reports(
    manager: Arc<PluginManager>,
    reports: &[PluginLoadReport],
) -> oc_provider::provider::auth::BuiltinProviderAuth {
    from_plugin_reports_with_builtins(manager, reports, BTreeMap::new())
}

/// Build the provider-auth registry with native internal hooks first and
/// executable external-plugin hooks second. This preserves the reference
/// bootstrap order while allowing an explicitly configured plugin to replace
/// a native provider implementation.
pub fn from_plugin_reports_with_builtins(
    manager: Arc<PluginManager>,
    reports: &[PluginLoadReport],
    mut hooks: BTreeMap<String, Box<dyn AuthHook>>,
) -> oc_provider::provider::auth::BuiltinProviderAuth {
    for report in reports {
        let Some(summary) = report.summary.as_ref() else {
            continue;
        };
        for auth in &summary.auth {
            hooks.insert(
                auth.provider.clone(),
                Box::new(PluginAuthHook {
                    manager: manager.clone(),
                    provider: auth.provider.clone(),
                    methods: auth.methods.iter().map(method_from_summary).collect(),
                    pending_method: Mutex::new(None),
                }),
            );
        }
    }
    oc_provider::provider::auth::ProviderAuth::new(hooks)
}

/// Construct a provider-auth registry when no external plugin manager is
/// needed, such as pure mode with only native internal hooks enabled.
pub fn from_builtins(
    hooks: BTreeMap<String, Box<dyn AuthHook>>,
) -> oc_provider::provider::auth::BuiltinProviderAuth {
    oc_provider::provider::auth::ProviderAuth::new(hooks)
}

struct PluginAuthHook {
    manager: Arc<PluginManager>,
    provider: String,
    methods: Vec<Method>,
    pending_method: Mutex<Option<usize>>,
}

impl AuthHook for PluginAuthHook {
    fn methods(&self) -> Vec<Method> {
        self.methods.clone()
    }

    fn validate(&self, method_index: usize, key: &str, value: &str) -> Option<String> {
        self.manager
            .auth_validate(AuthValidateRequest {
                provider: self.provider.clone(),
                method: method_index,
                key: key.to_string(),
                value: value.to_string(),
            })
            .unwrap_or_else(|error| Some(error))
    }

    fn authorize(
        &self,
        method_index: usize,
        inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error> {
        let value = self
            .manager
            .auth_authorize(AuthAuthorizeRequest {
                provider: self.provider.clone(),
                method: method_index,
                inputs: inputs.clone(),
            })
            .map_err(|error| anyhow!(error))?;
        let authorization: oc_provider::provider::auth::Authorization =
            serde_json::from_value(value)
                .map_err(|error| anyhow!("invalid plugin authorization: {error}"))?;
        *self
            .pending_method
            .lock()
            .expect("plugin auth method lock poisoned") = Some(method_index);
        Ok(AuthOAuthResult {
            url: authorization.url,
            method: authorization.method,
            instructions: authorization.instructions,
        })
    }

    fn callback(&self, code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
        let method = self
            .pending_method
            .lock()
            .expect("plugin auth method lock poisoned")
            .take()
            .unwrap_or(0);
        let value = self
            .manager
            .auth_callback(AuthCallbackRequest {
                provider: self.provider.clone(),
                method,
                code: code.map(str::to_owned),
            })
            .map_err(|error| anyhow!(error))?;
        callback_result(value)
    }
}

fn method_from_summary(summary: &PluginAuthMethodSummary) -> Method {
    Method {
        r#type: match summary.r#type {
            PluginAuthMethodType::OAuth => MethodType::OAuth,
            PluginAuthMethodType::Api => MethodType::Api,
        },
        label: summary.label.clone(),
        prompts: summary
            .prompts
            .as_ref()
            .map(|prompts| prompts.iter().map(prompt_from_summary).collect::<Vec<_>>()),
    }
}

fn prompt_from_summary(summary: &PluginAuthPromptSummary) -> Prompt {
    match summary {
        PluginAuthPromptSummary::Text {
            key,
            message,
            placeholder,
            when,
        } => Prompt::Text(TextPrompt {
            r#type: "text".into(),
            key: key.clone(),
            message: message.clone(),
            placeholder: placeholder.clone(),
            when: when.as_ref().and_then(when_from_summary),
        }),
        PluginAuthPromptSummary::Select {
            key,
            message,
            options,
            when,
        } => Prompt::Select(SelectPrompt {
            r#type: "select".into(),
            key: key.clone(),
            message: message.clone(),
            options: options
                .iter()
                .map(|option| SelectOption {
                    label: option.label.clone(),
                    value: option.value.clone(),
                    hint: option.hint.clone(),
                })
                .collect(),
            when: when.as_ref().and_then(when_from_summary),
        }),
    }
}

fn when_from_summary(summary: &PluginAuthWhenSummary) -> Option<When> {
    let op = match summary.op.as_str() {
        "eq" => WhenOp::Eq,
        "neq" => WhenOp::Neq,
        _ => return None,
    };
    Some(When {
        key: summary.key.clone(),
        op,
        value: summary.value.clone(),
    })
}

fn callback_result(value: Value) -> Result<AuthCallbackResult, anyhow::Error> {
    if value.get("type").and_then(Value::as_str) != Some("success") {
        return Ok(AuthCallbackResult::Failed);
    }

    let provider = value
        .get("provider")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let oauth = match (
        value.get("refresh").and_then(Value::as_str),
        value.get("access").and_then(Value::as_str),
    ) {
        (Some(refresh), Some(access)) => Some(OAuthCredential {
            refresh: refresh.to_owned(),
            access: access.to_owned(),
            expires: value.get("expires").and_then(Value::as_u64).unwrap_or(0),
            account_id: value
                .get("accountID")
                .or_else(|| value.get("accountId"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            enterprise_url: value
                .get("enterpriseURL")
                .or_else(|| value.get("enterpriseUrl"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        (None, None) => None,
        _ => {
            return Err(anyhow!(
                "plugin OAuth callback returned incomplete credentials"
            ))
        }
    };
    let api = value
        .get("key")
        .and_then(Value::as_str)
        .map(|key| ApiCredential {
            key: key.to_owned(),
            metadata: value
                .get("metadata")
                .and_then(|metadata| serde_json::from_value(metadata.clone()).ok()),
        });
    if oauth.is_none() && api.is_none() {
        return Err(anyhow!("plugin auth callback returned no credentials"));
    }
    Ok(AuthCallbackResult::Success {
        provider,
        oauth,
        api,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_maps_oauth_and_api_shapes() {
        let oauth = callback_result(serde_json::json!({
            "type": "success",
            "provider": "fixture",
            "refresh": "r",
            "access": "a",
            "expires": 123,
        }))
        .unwrap();
        assert!(matches!(
            oauth,
            AuthCallbackResult::Success { oauth: Some(_), .. }
        ));

        let api = callback_result(serde_json::json!({
            "type": "success",
            "key": "secret",
            "metadata": {"kind": "test"},
        }))
        .unwrap();
        assert!(matches!(
            api,
            AuthCallbackResult::Success { api: Some(_), .. }
        ));
    }

    #[test]
    fn unknown_when_operator_is_ignored() {
        assert!(when_from_summary(&PluginAuthWhenSummary {
            key: "mode".into(),
            op: "unknown".into(),
            value: "x".into(),
        })
        .is_none());
    }
}
