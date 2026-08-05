/// From reference/packages/opencode/src/session/session.ts
///
/// Session data model and pure helpers. `Info` reuses the V1 session schema;
/// this module adds title logic, plan paths, token/cost accounting and the
/// create/fork inputs.
use serde::{Deserialize, Serialize};

use crate::llm::Usage;
use crate::provider::{CacheCost, CostBase, ProviderModel};
use crate::v1::{
    CacheTokens, SessionInfo, SessionModel, SessionRevert, SessionShare, SessionSummary,
    SessionTokens,
};
use crate::JsonMap;

pub type Info = SessionInfo;

pub const PARENT_TITLE_PREFIX: &str = "New session - ";
pub const CHILD_TITLE_PREFIX: &str = "Child session - ";

pub fn is_default_title(title: &str) -> bool {
    regex::Regex::new(&format!(
        r"^({}|{})\d{{4}}-\d{{2}}-\d{{2}}T\d{{2}}:\d{{2}}:\d{{2}}\.\d{{3}}Z$",
        regex::escape(PARENT_TITLE_PREFIX),
        regex::escape(CHILD_TITLE_PREFIX)
    ))
    .expect("default title regex is valid")
    .is_match(title)
}

/// From reference `session.ts:getForkedTitle`.
pub fn get_forked_title(title: &str) -> String {
    if let Some(captures) = regex::Regex::new(r"^(.+) \(fork #(\d+)\)$")
        .expect("fork title regex is valid")
        .captures(title)
    {
        let base = &captures[1];
        let num: u32 = captures[2].parse().unwrap_or(0);
        return format!("{base} (fork #{})", num + 1);
    }
    format!("{title} (fork #1)")
}

/// From reference `session.ts:sessionPath` — relative path with forward slashes.
pub fn session_path(worktree: &str, cwd: &str) -> String {
    let rel = pathdiff(cwd, worktree);
    rel.replace('\\', "/")
}

fn pathdiff(from: &str, to: &str) -> String {
    use std::path::Path;
    let from = Path::new(from);
    let to = Path::new(to);
    let from_abs = if from.is_absolute() {
        from.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(from)
    };
    let to_abs = if to.is_absolute() {
        to.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(to)
    };
    match pathdiff_rs(&from_abs, &to_abs) {
        Some(rel) => rel.to_string_lossy().to_string(),
        None => to_abs.to_string_lossy().to_string(),
    }
}

fn pathdiff_rs(base: &std::path::Path, target: &std::path::Path) -> Option<std::path::PathBuf> {
    let base: Vec<_> = base.components().collect();
    let target: Vec<_> = target.components().collect();
    let common = base
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = std::path::PathBuf::new();
    for _ in common..base.len() {
        result.push("..");
    }
    for component in &target[common..] {
        result.push(component);
    }
    if result.as_os_str().is_empty() {
        Some(std::path::PathBuf::from("."))
    } else {
        Some(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub additions: f64,
    pub deletions: f64,
    pub files: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<crate::v1::FileDiff>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: CacheTokens,
}

impl Default for Tokens {
    fn default() -> Self {
        Tokens {
            total: None,
            input: 0.0,
            output: 0.0,
            reasoning: 0.0,
            cache: CacheTokens {
                read: 0.0,
                write: 0.0,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Time {
    pub created: u64,
    pub updated: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compacting: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revert {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelRef {
    pub id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<SessionModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonMap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<crate::v1::Ruleset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkInput {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub worktree: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalInfo {
    #[serde(flatten)]
    pub info: Info,
    pub project: Option<ProjectInfo>,
}

/// From reference `session.ts:plan` — the plan file path for a session.
pub fn plan(input: &PlanInput, worktree: Option<&str>, data_dir: &str) -> String {
    let base = match worktree {
        Some(worktree) => std::path::Path::new(worktree)
            .join(".opencode")
            .join("plans"),
        None => std::path::Path::new(data_dir).join("plans"),
    };
    base.join(format!("{}-{}.md", input.time_created, input.slug))
        .to_string_lossy()
        .to_string()
}

#[derive(Debug, Clone)]
pub struct PlanInput {
    pub slug: String,
    pub time_created: u64,
}

/// From reference `session.ts:getUsage`.
pub fn get_usage(input: &GetUsageInput) -> UsageResult {
    let safe = |value: f64| {
        if !value.is_finite() {
            0.0
        } else {
            value.max(0.0)
        }
    };
    let input_tokens = safe(input.usage.input_tokens.unwrap_or(0.0));
    let output_tokens = safe(input.usage.output_tokens.unwrap_or(0.0));
    let reasoning_tokens = safe(input.usage.reasoning_tokens.unwrap_or(0.0));

    let cache_read_input_tokens = safe(input.usage.cache_read_input_tokens.unwrap_or(0.0));
    let cache_write_input_tokens = safe(nested_number(
        input.metadata,
        cache_write_candidates(input.usage),
    ));

    // AI SDK v6 normalized inputTokens to include cached tokens across all
    // providers. Always subtract cache tokens for the non-cached input count.
    let adjusted_input_tokens =
        safe(input_tokens - cache_read_input_tokens - cache_write_input_tokens);

    let total = input.usage.total_tokens;

    let tokens = Tokens {
        total,
        input: adjusted_input_tokens,
        output: safe(output_tokens - reasoning_tokens),
        reasoning: reasoning_tokens,
        cache: crate::v1::CacheTokens {
            write: cache_write_input_tokens,
            read: cache_read_input_tokens,
        },
    };

    let context_tokens = input_tokens;
    let tier_selected = input
        .model
        .cost
        .tiers
        .as_ref()
        .and_then(|tiers| {
            let mut filtered: Vec<&CostTier> = tiers
                .iter()
                .filter(|item| item.tier.type_ == "context" && context_tokens > item.tier.size)
                .collect();
            filtered.sort_by(|a, b| {
                b.tier
                    .size
                    .partial_cmp(&a.tier.size)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            filtered.first().cloned().map(|tier| CostBase {
                input: tier.input,
                output: tier.output,
                cache: tier.cache.clone(),
            })
        })
        .or_else(|| {
            if context_tokens > 200_000.0 {
                input.model.cost.experimental_over_200_k.clone()
            } else {
                None
            }
        })
        .or_else(|| {
            if input.model.cost.input > 0.0 || input.model.cost.output > 0.0 {
                Some(CostBase {
                    input: input.model.cost.input,
                    output: input.model.cost.output,
                    cache: input.model.cost.cache.clone(),
                })
            } else {
                None
            }
        });
    let cost_info = tier_selected.map(|c| (c.input, c.output, c.cache));

    let total_nano_aiu = input
        .metadata
        .get("copilot")
        .and_then(|v| v.get("totalNanoAiu"))
        .and_then(|v| v.as_f64());

    let cost = match total_nano_aiu {
        Some(nano) if nano.is_finite() && nano >= 0.0 => nano / 100_000_000_000.0,
        _ => {
            let (ci, co, cc) = cost_info.unwrap_or((
                0.0,
                0.0,
                CacheCost {
                    read: 0.0,
                    write: 0.0,
                },
            ));
            safe(
                tokens.input * ci / 1_000_000.0
                    + tokens.output * co / 1_000_000.0
                    + tokens.cache.read * cc.read / 1_000_000.0
                    + tokens.cache.write * cc.write / 1_000_000.0
                    // charge reasoning tokens at the same rate as output tokens
                    + tokens.reasoning * co / 1_000_000.0,
            )
        }
    };

    UsageResult { cost, tokens }
}

use crate::provider::CostTier;

#[derive(Debug, Clone)]
pub struct GetUsageInput<'a> {
    pub model: &'a ProviderModel,
    pub usage: &'a Usage,
    pub metadata: &'a JsonMap,
}

#[derive(Debug, Clone)]
pub struct UsageResult {
    pub cost: f64,
    pub tokens: Tokens,
}

fn cache_write_candidates(usage: &Usage) -> Vec<(Option<f64>, Option<&str>)> {
    // Resolution order mirrors the reference: usage field, then anthropic/
    // vertex/bedrock/venice metadata fallbacks.
    vec![(usage.cache_write_input_tokens, None)]
}

fn nested_number(metadata: &JsonMap, candidates: Vec<(Option<f64>, Option<&str>)>) -> f64 {
    for (value, key) in candidates {
        if let Some(v) = value {
            return v;
        }
        if let Some(key) = key {
            for (provider_key, provider_value) in metadata {
                if provider_key != key {
                    continue;
                }
                if let Some(map) = provider_value.as_object() {
                    for (field, candidate) in map {
                        if field.contains("cacheCreationInputTokens")
                            || field.contains("cacheWriteInputTokens")
                        {
                            if let Some(n) = candidate.as_f64() {
                                return n;
                            }
                        }
                        if field == "usage" {
                            if let Some(usage) = candidate
                                .get("cacheWriteInputTokens")
                                .and_then(|v| v.as_f64())
                            {
                                return usage;
                            }
                        }
                    }
                }
            }
        }
    }
    0.0
}

/// From reference `session.ts:BusyError`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("SessionBusyError({session_id})")]
pub struct BusyError {
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub slug: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
    pub directory: String,
    pub path: Option<String>,
    pub title: String,
    pub agent: Option<String>,
    pub model: Option<SessionModel>,
    pub version: String,
    pub share_url: Option<String>,
    pub summary_additions: Option<f64>,
    pub summary_deletions: Option<f64>,
    pub summary_files: Option<f64>,
    pub summary_diffs: Option<Vec<crate::v1::FileDiff>>,
    pub metadata: Option<JsonMap>,
    pub cost: f64,
    pub tokens_input: f64,
    pub tokens_output: f64,
    pub tokens_reasoning: f64,
    pub tokens_cache_read: f64,
    pub tokens_cache_write: f64,
    pub revert: Option<SessionRevert>,
    pub permission: Option<crate::v1::Ruleset>,
    pub time_created: u64,
    pub time_updated: u64,
    pub time_compacting: Option<u64>,
    pub time_archived: Option<f64>,
}

/// From reference `session.ts:fromRow`.
pub fn from_row(row: &SessionRow) -> Info {
    let summary = if row.summary_additions.is_some()
        || row.summary_deletions.is_some()
        || row.summary_files.is_some()
    {
        Some(SessionSummary {
            additions: row.summary_additions.unwrap_or(0.0),
            deletions: row.summary_deletions.unwrap_or(0.0),
            files: row.summary_files.unwrap_or(0.0),
            diffs: row.summary_diffs.clone(),
        })
    } else {
        None
    };
    let share = row
        .share_url
        .as_ref()
        .map(|url| SessionShare { url: url.clone() });
    let revert = row.revert.clone();
    let model = row.model.clone();
    Info {
        id: row.id.clone(),
        slug: row.slug.clone(),
        project_id: row.project_id.clone(),
        workspace_id: row.workspace_id.clone(),
        directory: row.directory.clone(),
        path: row.path.clone(),
        parent_id: row.parent_id.clone(),
        summary,
        cost: Some(row.cost),
        tokens: Some(SessionTokens {
            input: row.tokens_input,
            output: row.tokens_output,
            reasoning: row.tokens_reasoning,
            cache: CacheTokens {
                read: row.tokens_cache_read,
                write: row.tokens_cache_write,
            },
        }),
        share,
        title: row.title.clone(),
        agent: row.agent.clone(),
        model,
        version: row.version.clone(),
        metadata: row.metadata.clone(),
        time: crate::v1::SessionTime {
            created: row.time_created,
            updated: row.time_updated,
            compacting: row.time_compacting,
            archived: row.time_archived,
        },
        permission: row.permission.clone(),
        revert,
    }
}

/// From reference `session.ts:toRow`.
pub fn to_row(info: &Info) -> SessionRow {
    let tokens = match info.tokens.clone() {
        Some(tokens) => tokens,
        None => crate::v1::SessionTokens {
            input: 0.0,
            output: 0.0,
            reasoning: 0.0,
            cache: CacheTokens {
                read: 0.0,
                write: 0.0,
            },
        },
    };
    let summary = info.summary.clone();
    SessionRow {
        id: info.id.clone(),
        slug: info.slug.clone(),
        project_id: info.project_id.clone(),
        workspace_id: info.workspace_id.clone(),
        parent_id: info.parent_id.clone(),
        directory: info.directory.clone(),
        path: info.path.clone(),
        title: info.title.clone(),
        agent: info.agent.clone(),
        model: info.model.clone(),
        version: info.version.clone(),
        share_url: info.share.clone().map(|s| s.url),
        summary_additions: summary.as_ref().map(|s| s.additions),
        summary_deletions: summary.as_ref().map(|s| s.deletions),
        summary_files: summary.as_ref().map(|s| s.files),
        summary_diffs: summary.as_ref().and_then(|s| s.diffs.clone()),
        metadata: info.metadata.clone(),
        cost: info.cost.unwrap_or(0.0),
        tokens_input: tokens.input,
        tokens_output: tokens.output,
        tokens_reasoning: tokens.reasoning,
        tokens_cache_read: tokens.cache.read,
        tokens_cache_write: tokens.cache.write,
        revert: info.revert.clone(),
        permission: info.permission.clone(),
        time_created: info.time.created,
        time_updated: info.time.updated,
        time_compacting: info.time.compacting,
        time_archived: info.time.archived,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_titles_match_reference() {
        let created = "New session - 2024-06-01T12:00:00.000Z";
        assert!(is_default_title(created));
        assert!(is_default_title("Child session - 2024-06-01T12:00:00.000Z"));
        assert!(!is_default_title("My custom title"));
    }

    #[test]
    fn fork_title_increments() {
        assert_eq!(get_forked_title("Session"), "Session (fork #1)");
        assert_eq!(get_forked_title("Session (fork #1)"), "Session (fork #2)");
        assert_eq!(get_forked_title("Session (fork #42)"), "Session (fork #43)");
    }

    #[test]
    fn usage_counts_zero_when_nothing_reported() {
        let model = ProviderModel::empty("gpt-4o", "openai");
        let usage = Usage::default();
        let result = get_usage(&GetUsageInput {
            model: &model,
            usage: &usage,
            metadata: &JsonMap::new(),
        });
        assert_eq!(result.cost, 0.0);
        assert_eq!(result.tokens.input, 0.0);
        assert_eq!(result.tokens.output, 0.0);
    }

    #[test]
    fn usage_subtracts_cache_from_input() {
        let model = ProviderModel::empty("gpt-4o", "openai");
        let usage = Usage {
            input_tokens: Some(100.0),
            cache_read_input_tokens: Some(30.0),
            output_tokens: Some(20.0),
            reasoning_tokens: Some(5.0),
            ..Usage::default()
        };
        let result = get_usage(&GetUsageInput {
            model: &model,
            usage: &usage,
            metadata: &JsonMap::new(),
        });
        assert_eq!(result.tokens.input, 70.0);
        assert_eq!(result.tokens.output, 15.0);
        assert_eq!(result.tokens.reasoning, 5.0);
        assert_eq!(result.tokens.cache.read, 30.0);
    }
}
