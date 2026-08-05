use std::future::Future;
use std::pin::Pin;

use crate::llm::message::Model;
use crate::session::schema::{ModelRef, SessionInfo};

/// `SessionRunnerModel.ModelNotSelectedError`
/// /// From reference/packages/core/src/session/runner/model.ts
#[derive(Debug, Clone, thiserror::Error)]
#[error("No model is available for session {session_id}")]
pub struct ModelNotSelectedError {
    pub session_id: String,
}

/// `SessionRunnerModel.ModelUnavailableError`
/// /// From reference/packages/core/src/session/runner/model.ts
#[derive(Debug, Clone, thiserror::Error)]
#[error("Model unavailable: {provider_id}/{model_id}")]
pub struct ModelUnavailableError {
    pub provider_id: String,
    pub model_id: String,
}

/// `SessionRunnerModel.VariantUnavailableError`
/// /// From reference/packages/core/src/session/runner/model.ts
#[derive(Debug, Clone, thiserror::Error)]
#[error("Variant unavailable for {provider_id}/{model_id}: {variant}")]
pub struct VariantUnavailableError {
    pub provider_id: String,
    pub model_id: String,
    pub variant: String,
}

/// `SessionRunnerModel.UnsupportedApiError`
/// /// From reference/packages/core/src/session/runner/model.ts
#[derive(Debug, Clone, thiserror::Error)]
#[error("Unsupported API for {provider_id}/{model_id}: {api}")]
pub struct UnsupportedApiError {
    pub provider_id: String,
    pub model_id: String,
    pub api: String,
}

/// `SessionRunnerModel.Error`
/// /// From reference/packages/core/src/session/runner/model.rs
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelError {
    #[error(transparent)]
    NotSelected(#[from] ModelNotSelectedError),
    #[error(transparent)]
    Unavailable(#[from] ModelUnavailableError),
    #[error(transparent)]
    VariantUnavailable(#[from] VariantUnavailableError),
    #[error(transparent)]
    UnsupportedApi(#[from] UnsupportedApiError),
    #[error("model authorization failed")]
    Authorization,
}

/// `SessionRunnerModel` — resolves a runnable `Model` for a session.
/// /// From reference/packages/core/src/session/runner/model.ts
pub trait SessionRunnerModel: Send + Sync {
    fn resolve(
        &self,
        session: &SessionInfo,
    ) -> Pin<Box<dyn Future<Output = Result<Model, ModelError>> + Send + '_>>;
}

/// Applies the session's variant to a catalog model: pick `variant` from
/// `model.variants` and merge its headers/body over the base request.
/// Ported from `withVariant`. `variants` is supplied by the catalog model.
/// /// From reference/packages/core/src/session/runner/model.ts
pub fn with_variant(
    model_id: String,
    provider_id: String,
    variant_id: Option<String>,
    base_variant: Option<String>,
    variants: &[Variant],
    headers: &mut serde_json::Map<String, serde_json::Value>,
    body: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), VariantUnavailableError> {
    let id = match variant_id.as_deref() {
        Some("default") | None => base_variant.unwrap_or_default(),
        Some(variant) => variant.to_string(),
    };
    let variant = variants.iter().find(|item| item.id == id);
    if variant.is_none()
        && variant_id
            .as_deref()
            .is_some_and(|value| value != "default")
    {
        return Err(VariantUnavailableError {
            provider_id,
            model_id,
            variant: variant_id.unwrap_or_default(),
        });
    }
    if let Some(variant) = variant {
        for (key, value) in &variant.headers {
            headers.insert(key.clone(), value.clone());
        }
        for (key, value) in &variant.body {
            body.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

/// A catalog model variant (headers/body deltas).
/// /// From reference/packages/schema/src/model.ts
#[derive(Debug, Clone, Default)]
pub struct Variant {
    pub id: String,
    pub headers: serde_json::Map<String, serde_json::Value>,
    pub body: serde_json::Map<String, serde_json::Value>,
}

/// The API surface the native runtime supports: OpenAI Responses, Anthropic
/// Messages, and OpenAI-compatible Chat with a URL.
/// Ported from `supported`.
/// /// From reference/packages/core/src/session/runner/model.ts
pub fn supported(api_type: &str, package: &str, url: &Option<String>) -> bool {
    api_type == "aisdk"
        && (package == "@ai-sdk/openai"
            || package == "@ai-sdk/anthropic"
            || (package == "@ai-sdk/openai-compatible" && url.is_some()))
}

/// `ModelV2.Ref` from a resolved `Model` plus session variant.
/// /// From reference/packages/core/src/session/runner/llm.ts
pub fn ref_from_model(model: &Model, session: &SessionInfo) -> ModelRef {
    ModelRef {
        id: model.id.clone(),
        provider_id: model.provider.clone(),
        variant: session
            .model
            .as_ref()
            .and_then(|model| model.variant.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(id: &str) -> Variant {
        Variant {
            id: id.to_string(),
            headers: Default::default(),
            body: Default::default(),
        }
    }

    #[test]
    fn default_variant_resolves_to_base() {
        let mut headers = Default::default();
        let mut body = Default::default();
        let result = with_variant(
            "gpt".into(),
            "openai".into(),
            Some("default".into()),
            Some("base".into()),
            &[variant("base")],
            &mut headers,
            &mut body,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn unknown_variant_is_an_error() {
        let result = with_variant(
            "gpt".into(),
            "openai".into(),
            Some("missing".into()),
            None,
            &[],
            &mut Default::default(),
            &mut Default::default(),
        );
        assert!(matches!(result, Err(VariantUnavailableError { .. })));
    }

    #[test]
    fn variant_overrides_body() {
        let mut headers = Default::default();
        let mut body = serde_json::json!({ "temperature": 0.5 })
            .as_object()
            .unwrap()
            .clone();
        let mut v = variant("fast");
        v.body = serde_json::json!({ "temperature": 0.0 })
            .as_object()
            .unwrap()
            .clone();
        with_variant(
            "gpt".into(),
            "openai".into(),
            Some("fast".into()),
            None,
            &[v],
            &mut headers,
            &mut body,
        )
        .unwrap();
        assert_eq!(body.get("temperature"), Some(&serde_json::json!(0.0)));
    }

    #[test]
    fn supported_native_providers() {
        assert!(supported("aisdk", "@ai-sdk/openai", &None));
        assert!(supported("aisdk", "@ai-sdk/anthropic", &None));
        assert!(supported(
            "aisdk",
            "@ai-sdk/openai-compatible",
            &Some("https://x".into())
        ));
        assert!(!supported("aisdk", "@ai-sdk/openai-compatible", &None));
        assert!(!supported("native", "@ai-sdk/openai", &None));
    }
}
