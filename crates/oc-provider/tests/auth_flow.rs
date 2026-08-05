//! Tests for the ProviderAuth authorize/callback flow and the login flows.

use std::collections::BTreeMap;

use oc_provider::auth::login::{
    login_url, resolve_plugin_providers, HasAuth, LoginOptions, LoginPrompt, WellKnownMetadata,
};
use oc_provider::auth::{AuthStore, Info as AuthInfo, MemoryAuthStore};
use oc_provider::provider::auth::{
    AuthCallbackResult, AuthHook, AuthOAuthResult, AuthorizeInput, CallbackInput, CallbackMethod,
    Method, MethodType, OAuthCredential, Prompt, ProviderAuth, ProviderAuthError,
};

/// A scriptable plugin auth hook for tests.
struct MockHook {
    methods: Vec<Method>,
    authorize_result: Result<AuthOAuthResult, anyhow::Error>,
    callback_result: Result<AuthCallbackResult, anyhow::Error>,
}

impl MockHook {
    fn oauth(label: &str) -> MockHook {
        MockHook {
            methods: vec![Method {
                r#type: MethodType::OAuth,
                label: label.to_string(),
                prompts: None,
            }],
            authorize_result: Ok(AuthOAuthResult {
                url: "https://example.com/authorize".to_string(),
                method: CallbackMethod::Code,
                instructions: "Copy the code".to_string(),
            }),
            callback_result: Ok(AuthCallbackResult::Success {
                provider: None,
                oauth: Some(OAuthCredential {
                    refresh: "refresh-token".to_string(),
                    access: "access-token".to_string(),
                    expires: 1_000,
                    account_id: None,
                    enterprise_url: None,
                }),
                api: None,
            }),
        }
    }
}

impl AuthHook for MockHook {
    fn methods(&self) -> Vec<Method> {
        self.methods.clone()
    }
    fn validate(&self, _method_index: usize, _key: &str, value: &str) -> Option<String> {
        if value == "bad" {
            Some("must not be bad".to_string())
        } else {
            None
        }
    }
    fn authorize(
        &self,
        _method_index: usize,
        _inputs: &BTreeMap<String, String>,
    ) -> Result<AuthOAuthResult, anyhow::Error> {
        match &self.authorize_result {
            Ok(ok) => Ok(ok.clone()),
            Err(err) => Err(anyhow::anyhow!("{}", err)),
        }
    }
    fn callback(&self, _code: Option<&str>) -> Result<AuthCallbackResult, anyhow::Error> {
        match &self.callback_result {
            Ok(ok) => Ok(ok.clone()),
            Err(err) => Err(anyhow::anyhow!("{}", err)),
        }
    }
}

fn hooks_map() -> BTreeMap<String, MockHook> {
    BTreeMap::from([("github".to_string(), MockHook::oauth("GitHub OAuth"))])
}

#[test]
fn methods_returns_plugin_methods() {
    let service = ProviderAuth::new(hooks_map());
    let methods = service.methods();
    assert_eq!(methods.len(), 1);
    let github = &methods["github"];
    assert_eq!(github[0].label, "GitHub OAuth");
    assert_eq!(github[0].r#type, MethodType::OAuth);
}

#[test]
fn authorize_returns_url_and_method() {
    let service = ProviderAuth::new(hooks_map());
    let result = service
        .authorize(
            "github",
            &AuthorizeInput {
                method: 0,
                inputs: None,
            },
        )
        .unwrap();
    let authorization = result.unwrap();
    assert_eq!(authorization.url, "https://example.com/authorize");
    assert_eq!(authorization.method, CallbackMethod::Code);
    assert_eq!(authorization.instructions, "Copy the code");
}

#[test]
fn authorize_non_oauth_returns_none() {
    let hook = MockHook {
        methods: vec![Method {
            r#type: MethodType::Api,
            label: "API key".to_string(),
            prompts: None,
        }],
        ..MockHook::oauth("x")
    };
    let service = ProviderAuth::new(BTreeMap::from([("api".to_string(), hook)]));
    let result = service
        .authorize(
            "api",
            &AuthorizeInput {
                method: 0,
                inputs: None,
            },
        )
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn callback_stores_oauth_credentials() {
    let service = ProviderAuth::new(hooks_map());
    let mut auth = MemoryAuthStore::new();
    service
        .authorize(
            "github",
            &AuthorizeInput {
                method: 0,
                inputs: None,
            },
        )
        .unwrap();
    service
        .callback(
            "github",
            &CallbackInput {
                method: 0,
                code: Some("abc".to_string()),
            },
            &mut auth,
        )
        .unwrap();
    let stored = auth.get("github").unwrap().unwrap();
    match stored {
        AuthInfo::Oauth(oauth) => {
            assert_eq!(oauth.access, "access-token");
            assert_eq!(oauth.refresh, "refresh-token");
            assert_eq!(oauth.expires, 1_000);
        }
        _ => panic!("expected oauth credential"),
    }
}

#[test]
fn callback_without_pending_returns_oauth_missing() {
    let service = ProviderAuth::new(hooks_map());
    let mut auth = MemoryAuthStore::new();
    let err = service
        .callback(
            "github",
            &CallbackInput {
                method: 0,
                code: Some("abc".to_string()),
            },
            &mut auth,
        )
        .unwrap_err();
    assert!(matches!(err, ProviderAuthError::OauthMissing(_)));
}

#[test]
fn callback_code_method_without_code_returns_code_missing() {
    let service = ProviderAuth::new(hooks_map());
    service
        .authorize(
            "github",
            &AuthorizeInput {
                method: 0,
                inputs: None,
            },
        )
        .unwrap();
    let err = service
        .callback(
            "github",
            &CallbackInput {
                method: 0,
                code: None,
            },
            &mut MemoryAuthStore::new(),
        )
        .unwrap_err();
    assert!(matches!(err, ProviderAuthError::OauthCodeMissing(_)));
}

#[test]
fn callback_failed_result_returns_oauth_callback_failed() {
    let hook = MockHook {
        callback_result: Ok(AuthCallbackResult::Failed),
        ..MockHook::oauth("x")
    };
    let service = ProviderAuth::new(BTreeMap::from([("github".to_string(), hook)]));
    service
        .authorize(
            "github",
            &AuthorizeInput {
                method: 0,
                inputs: None,
            },
        )
        .unwrap();
    let err = service
        .callback(
            "github",
            &CallbackInput {
                method: 0,
                code: Some("abc".to_string()),
            },
            &mut MemoryAuthStore::new(),
        )
        .unwrap_err();
    assert!(matches!(err, ProviderAuthError::OauthCallbackFailed(_)));
}

#[test]
fn authorize_runs_text_prompt_validation() {
    let hook = MockHook {
        methods: vec![Method {
            r#type: MethodType::OAuth,
            label: "OAuth".to_string(),
            prompts: Some(vec![Prompt::Text(
                oc_provider::provider::auth::TextPrompt {
                    r#type: "text".to_string(),
                    key: "account".to_string(),
                    message: "Account".to_string(),
                    placeholder: None,
                    when: None,
                },
            )]),
        }],
        ..MockHook::oauth("x")
    };
    let service = ProviderAuth::new(BTreeMap::from([("provider".to_string(), hook)]));
    let err = service
        .authorize(
            "provider",
            &AuthorizeInput {
                method: 0,
                inputs: Some(BTreeMap::from([("account".to_string(), "bad".to_string())])),
            },
        )
        .unwrap_err();
    assert!(matches!(err, ProviderAuthError::ValidationFailed(_)));
}

/// A mock `LoginPrompt` that records calls and returns scripted values.
struct ScriptedPrompt {
    api_key: Option<String>,
    recorded: std::sync::Mutex<Vec<String>>,
}

impl ScriptedPrompt {
    fn new(api_key: Option<&str>) -> Self {
        ScriptedPrompt {
            api_key: api_key.map(str::to_string),
            recorded: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl LoginPrompt for ScriptedPrompt {
    fn intro(&self, title: &str) {
        self.recorded.lock().unwrap().push(format!("intro:{title}"));
    }
    fn log_info(&self, message: &str) {
        self.recorded
            .lock()
            .unwrap()
            .push(format!("info:{message}"));
    }
    fn log_warn(&self, message: &str) {
        self.recorded
            .lock()
            .unwrap()
            .push(format!("warn:{message}"));
    }
    fn log_error(&self, message: &str) {
        self.recorded
            .lock()
            .unwrap()
            .push(format!("error:{message}"));
    }
    fn log_success(&self, message: &str) {
        self.recorded
            .lock()
            .unwrap()
            .push(format!("success:{message}"));
    }
    fn outro(&self, message: &str) {
        self.recorded
            .lock()
            .unwrap()
            .push(format!("outro:{message}"));
    }
    fn text(
        &self,
        _message: &str,
        _placeholder: Option<&str>,
        _validate: Option<&dyn Fn(&str) -> Option<String>>,
    ) -> Option<String> {
        None
    }
    fn password(&self, _message: &str) -> Option<String> {
        self.api_key.clone()
    }
    fn select(&self, _message: &str, _options: &[(String, String)]) -> Option<usize> {
        None
    }
    fn autocomplete(&self, _message: &str, _options: &[(String, String)]) -> Option<String> {
        None
    }
    fn spinner_start(&self, _message: &str) {}
    fn spinner_stop(&self, _message: &str, _failed: bool) {}
}

#[test]
fn login_url_stores_wellknown_credential() {
    let mut auth = MemoryAuthStore::new();
    let prompt = ScriptedPrompt::new(None);
    let fetch = |url: &str| -> Result<WellKnownMetadata, anyhow::Error> {
        assert_eq!(url, "https://auth.example.com");
        Ok(WellKnownMetadata {
            auth: oc_provider::auth::login::WellKnownAuth {
                command: vec!["opencode-auth".to_string(), "token".to_string()],
                env: "AUTH_TOKEN".to_string(),
            },
        })
    };
    let run = |_command: &[String]| -> Result<(i32, String), anyhow::Error> {
        Ok((0, "  tok123\n".to_string()))
    };
    login_url("https://auth.example.com/", &mut auth, &prompt, fetch, run).unwrap();
    let stored = auth.get("https://auth.example.com").unwrap().unwrap();
    match stored {
        AuthInfo::WellKnown(wellknown) => {
            assert_eq!(wellknown.key, "AUTH_TOKEN");
            assert_eq!(wellknown.token, "tok123");
        }
        _ => panic!("expected wellknown credential"),
    }
    assert!(prompt
        .recorded
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.starts_with("success:")));
}

#[test]
fn login_url_failed_command_does_not_store() {
    let mut auth = MemoryAuthStore::new();
    let prompt = ScriptedPrompt::new(None);
    let fetch = |_url: &str| -> Result<WellKnownMetadata, anyhow::Error> {
        Ok(WellKnownMetadata {
            auth: oc_provider::auth::login::WellKnownAuth {
                command: vec!["cmd".to_string()],
                env: "TOKEN".to_string(),
            },
        })
    };
    let run =
        |_command: &[String]| -> Result<(i32, String), anyhow::Error> { Ok((1, String::new())) };
    login_url("https://auth.example.com", &mut auth, &prompt, fetch, run).unwrap();
    assert!(auth.get("https://auth.example.com").unwrap().is_none());
}

#[test]
fn resolve_plugin_providers_filters_known_and_disabled() {
    struct Hook(&'static str);
    impl HasAuth for Hook {
        fn auth_provider(&self) -> Option<&str> {
            Some(self.0)
        }
    }
    let hooks = [Hook("github"), Hook("github"), Hook("disabled-one")];
    let existing = BTreeMap::from([(
        "github".to_string(),
        AuthInfo::Api(oc_provider::auth::Api {
            key: "k".to_string(),
            metadata: None,
        }),
    )]);
    let disabled = std::collections::HashSet::from(["disabled-one".to_string()]);
    let names = BTreeMap::from([("github".to_string(), "GitHub".to_string())]);
    let result = resolve_plugin_providers(&hooks, &existing, &disabled, &None, &names);
    assert!(
        result.is_empty(),
        "existing + disabled providers are skipped"
    );
}

#[test]
fn login_api_key_flow_stores_credential() {
    let mut auth = MemoryAuthStore::new();
    let prompt = ScriptedPrompt::new(Some("sk-test"));
    let catalog = BTreeMap::from([(
        "anthropic".to_string(),
        oc_provider::auth::login::CatalogProvider {
            name: "Anthropic".to_string(),
            env: vec!["ANTHROPIC_API_KEY".to_string()],
        },
    )]);
    let options = LoginOptions {
        provider: Some("anthropic".to_string()),
        method: None,
    };
    let result = oc_provider::auth::login::login(
        &mut auth,
        &prompt,
        &options,
        &catalog,
        &BTreeMap::new(),
        &std::collections::HashSet::new(),
        &None,
    );
    result.unwrap();
    let stored = auth.get("anthropic").unwrap().unwrap();
    match stored {
        AuthInfo::Api(api) => assert_eq!(api.key, "sk-test"),
        _ => panic!("expected api credential"),
    }
}

#[test]
fn login_unknown_provider_errors() {
    let mut auth = MemoryAuthStore::new();
    let prompt = ScriptedPrompt::new(None);
    let options = LoginOptions {
        provider: Some("nope".to_string()),
        method: None,
    };
    let result = oc_provider::auth::login::login(
        &mut auth,
        &prompt,
        &options,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &std::collections::HashSet::new(),
        &None,
    );
    assert!(result.is_err());
}
