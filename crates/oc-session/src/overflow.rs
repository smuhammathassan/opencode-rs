/// From reference/packages/opencode/src/session/overflow.ts
///
/// Context-window overflow detection. Mirrors `ConfigV1.Info.compaction` from
/// reference/packages/core/src/v1/config/config.ts.
///
/// TODO(integration): promote to oc-config once the config crate lands.
use serde::{Deserialize, Serialize};

use crate::provider::{transform, ProviderModel};
use crate::v1::AssistantTokens;

pub const COMPACTION_BUFFER: f64 = 20_000.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prune: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_turns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserve_recent_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserved: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionConfig>,
}

/// From reference `overflow.ts:usable`.
pub fn usable(input: &UsableInput) -> f64 {
    let context = input.model.limit.context;
    if context == 0.0 {
        return 0.0;
    }
    let reserved = input
        .cfg
        .compaction
        .as_ref()
        .and_then(|c| c.reserved)
        .map(|value| value as f64)
        .unwrap_or_else(|| {
            COMPACTION_BUFFER.min(transform::max_output_tokens(
                input.model,
                input.output_token_max,
            ))
        });
    match input.model.limit.input {
        Some(limit) => (limit - reserved).max(0.0),
        None => {
            (context - transform::max_output_tokens(input.model, input.output_token_max)).max(0.0)
        }
    }
}

/// From reference `overflow.ts:isOverflow`.
pub fn is_overflow(input: &OverflowInput) -> bool {
    if let Some(compaction) = &input.cfg.compaction {
        if compaction.auto == Some(false) {
            return false;
        }
    }
    if input.model.limit.context == 0.0 {
        return false;
    }
    let tokens = input.tokens;
    let count = tokens
        .total
        .unwrap_or(tokens.input + tokens.output + tokens.cache.read + tokens.cache.write);
    count
        >= usable(&UsableInput {
            cfg: input.cfg,
            model: input.model,
            output_token_max: input.output_token_max,
        })
}

#[derive(Debug, Clone)]
pub struct UsableInput<'a> {
    pub cfg: &'a ConfigV1,
    pub model: &'a ProviderModel,
    pub output_token_max: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OverflowInput<'a> {
    pub cfg: &'a ConfigV1,
    pub tokens: &'a AssistantTokens,
    pub model: &'a ProviderModel,
    pub output_token_max: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderApiInfo, ProviderLimit, ProviderModel};

    fn model(context: f64, input: Option<f64>, output: f64) -> ProviderModel {
        ProviderModel {
            id: "m".into(),
            provider_id: "p".into(),
            api: ProviderApiInfo {
                id: "m".into(),
                npm: None,
                type_: "native".into(),
            },
            name: "m".into(),
            family: None,
            capabilities: Default::default(),
            cost: Default::default(),
            limit: ProviderLimit {
                context,
                input,
                output,
            },
            status: "active".into(),
            options: Default::default(),
            headers: Default::default(),
            release_date: String::new(),
            variants: None,
        }
    }

    fn tokens(total: Option<f64>, input: f64) -> AssistantTokens {
        AssistantTokens {
            total,
            input,
            output: 0.0,
            reasoning: 0.0,
            cache: Default::default(),
        }
    }

    #[test]
    fn zero_context_never_overflows() {
        let cfg = ConfigV1::default();
        let model = model(0.0, None, 1000.0);
        assert!(!is_overflow(&OverflowInput {
            cfg: &cfg,
            tokens: &tokens(Some(100.0), 100.0),
            model: &model,
            output_token_max: None,
        }));
    }

    #[test]
    fn auto_false_disables_overflow() {
        let cfg = ConfigV1 {
            compaction: Some(CompactionConfig {
                auto: Some(false),
                ..Default::default()
            }),
        };
        let model = model(8000.0, None, 1000.0);
        assert!(!is_overflow(&OverflowInput {
            cfg: &cfg,
            tokens: &tokens(Some(10_000.0), 9000.0),
            model: &model,
            output_token_max: None,
        }));
    }

    #[test]
    fn overflow_when_count_at_or_above_usable() {
        let cfg = ConfigV1::default();
        let model = model(8000.0, None, 1000.0);
        // usable = 8000 - max(20000.min(1000), ...) = 8000 - 1000 = 7000
        assert!(!is_overflow(&OverflowInput {
            cfg: &cfg,
            tokens: &tokens(Some(6999.0), 100.0),
            model: &model,
            output_token_max: None,
        }));
        assert!(is_overflow(&OverflowInput {
            cfg: &cfg,
            tokens: &tokens(Some(7000.0), 100.0),
            model: &model,
            output_token_max: None,
        }));
    }
}
