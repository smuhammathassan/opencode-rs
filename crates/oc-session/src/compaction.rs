/// From reference/packages/opencode/src/session/compaction.ts
///
/// V1 compaction orchestration: recent-tail selection, pruning, and the
/// compaction message/part construction.
use crate::overflow::{usable, ConfigV1};
use crate::provider::ProviderModel;
use crate::v1::{Info, Part, PartBase, User, UserModel, WithParts};

pub const PRUNE_MINIMUM: u64 = 20_000;
pub const PRUNE_PROTECT: u64 = 40_000;
pub const TOOL_OUTPUT_MAX_CHARS: usize = 2_000;
pub const PRUNE_PROTECTED_TOOLS: [&str; 1] = ["skill"];
pub const DEFAULT_TAIL_TURNS: u64 = 2;
pub const MIN_PRESERVE_RECENT_TOKENS: u64 = 2_000;
pub const MAX_PRESERVE_RECENT_TOKENS: u64 = 8_000;

#[derive(Debug, Clone)]
pub struct Turn {
    pub start: usize,
    pub end: usize,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct Tail {
    pub start: usize,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct CompletedCompaction {
    pub user_index: usize,
    pub assistant_index: usize,
    pub summary: Option<String>,
}

/// From reference `compaction.ts:summaryText`.
pub fn summary_text(message: &WithParts) -> Option<String> {
    let text = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.text.trim().to_string()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// From reference `compaction.ts:completedCompactions`.
pub fn completed_compactions(messages: &[WithParts]) -> Vec<CompletedCompaction> {
    let mut users = std::collections::HashMap::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg.info.role() != "user" {
            continue;
        }
        if msg
            .parts
            .iter()
            .any(|part| matches!(part, Part::Compaction(_)))
        {
            users.insert(msg.info.id().to_string(), i);
        }
    }
    let mut result = Vec::new();
    for (assistant_index, msg) in messages.iter().enumerate() {
        if msg.info.role() != "assistant" {
            continue;
        }
        let Info::Assistant(assistant) = &msg.info else {
            continue;
        };
        if !assistant.summary.unwrap_or(false)
            || assistant.finish.is_none()
            || assistant.error.is_some()
        {
            continue;
        }
        let Some(user_index) = users.get(&assistant.parent_id) else {
            continue;
        };
        result.push(CompletedCompaction {
            user_index: *user_index,
            assistant_index,
            summary: summary_text(msg),
        });
    }
    result
}

/// From reference `compaction.ts:preserveRecentBudget`.
pub fn preserve_recent_budget(cfg: &ConfigV1, model: &ProviderModel) -> u64 {
    cfg.compaction
        .as_ref()
        .and_then(|c| c.preserve_recent_tokens)
        .unwrap_or_else(|| {
            let usable = usable(&crate::overflow::UsableInput {
                cfg,
                model,
                output_token_max: None,
            });
            (usable * 0.25).floor() as u64
        })
        .clamp(MIN_PRESERVE_RECENT_TOKENS, MAX_PRESERVE_RECENT_TOKENS)
}

/// From reference `compaction.ts:turns`.
pub fn turns(messages: &[WithParts]) -> Vec<Turn> {
    let mut result = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg.info.role() != "user" {
            continue;
        }
        if msg
            .parts
            .iter()
            .any(|part| matches!(part, Part::Compaction(_)))
        {
            continue;
        }
        result.push(Turn {
            start: i,
            end: messages.len(),
            id: msg.info.id().to_string(),
        });
    }
    for i in 0..result.len().saturating_sub(1) {
        result[i].end = result[i + 1].start;
    }
    result
}

/// From reference `compaction.ts:splitTurn`.
pub fn split_turn(
    messages: &[WithParts],
    turn: &Turn,
    budget: u64,
    estimate: &dyn Fn(&[WithParts]) -> u64,
) -> Option<Tail> {
    if budget == 0 {
        return None;
    }
    if turn.end.saturating_sub(turn.start) <= 1 {
        return None;
    }
    for start in (turn.start + 1)..turn.end {
        let size = estimate(&messages[start..turn.end]);
        if size > budget {
            continue;
        }
        return Some(Tail {
            start,
            id: messages[start].info.id().to_string(),
        });
    }
    None
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub head: Vec<WithParts>,
    pub tail_start_id: Option<String>,
}

/// From reference `compaction.ts:select` — choose which recent turns to keep
/// verbatim after compaction.
pub fn select(
    messages: &[WithParts],
    cfg: &ConfigV1,
    model: &ProviderModel,
    estimate: &dyn Fn(&[WithParts]) -> u64,
) -> Selection {
    let limit = cfg
        .compaction
        .as_ref()
        .and_then(|c| c.tail_turns)
        .unwrap_or(DEFAULT_TAIL_TURNS);
    if limit == 0 {
        return Selection {
            head: messages.to_vec(),
            tail_start_id: None,
        };
    }
    let budget = preserve_recent_budget(cfg, model);
    let all = turns(messages);
    if all.is_empty() {
        return Selection {
            head: messages.to_vec(),
            tail_start_id: None,
        };
    }
    let recent: Vec<Turn> = all[all.len().saturating_sub(limit as usize)..].to_vec();
    let sizes: Vec<u64> = recent
        .iter()
        .map(|turn| estimate(&messages[turn.start..turn.end]))
        .collect();

    let mut total: u64 = 0;
    let mut keep: Option<Tail> = None;
    for i in (0..recent.len()).rev() {
        let turn = &recent[i];
        let size = sizes[i];
        if total + size <= budget {
            total += size;
            keep = Some(Tail {
                start: turn.start,
                id: turn.id.clone(),
            });
            continue;
        }
        let remaining = budget.saturating_sub(total);
        let split = split_turn(messages, turn, remaining, estimate);
        if split.is_some() {
            keep = split;
        }
        break;
    }
    match keep {
        Some(keep) if keep.start > 0 => Selection {
            head: messages[..keep.start].to_vec(),
            tail_start_id: Some(keep.id),
        },
        _ => Selection {
            head: messages.to_vec(),
            tail_start_id: None,
        },
    }
}

/// From reference `compaction.ts:prune` — mark old completed tool outputs as
/// compacted to free context.
pub fn prune(messages: &[WithParts], estimate: &dyn Fn(&str) -> u64) -> Vec<usize> {
    let mut total: u64 = 0;
    let mut pruned: u64 = 0;
    let mut to_prune: Vec<usize> = Vec::new();
    let mut turn_count = 0usize;
    'outer: for (msg_index, msg) in messages.iter().enumerate().rev() {
        if msg.info.role() == "user" {
            turn_count += 1;
        }
        if turn_count < 2 {
            continue;
        }
        if let Info::Assistant(assistant) = &msg.info {
            if assistant.summary.unwrap_or(false) {
                break;
            }
        }
        for part in msg.parts.iter().rev() {
            let Part::Tool(tool) = part else { continue };
            let crate::v1::ToolState::Completed(state) = &tool.state else {
                continue;
            };
            if PRUNE_PROTECTED_TOOLS.contains(&tool.tool.as_str()) {
                continue;
            }
            if state.time.compacted.is_some() {
                break 'outer;
            }
            let estimate = estimate(&state.output);
            total += estimate;
            if total <= PRUNE_PROTECT {
                continue;
            }
            pruned += estimate;
            to_prune.push(msg_index);
        }
    }
    let _ = pruned;
    to_prune
}

/// From reference `compaction.ts:create` — construct the compaction user
/// message + part.
pub fn create_compaction_message(
    session_id: &str,
    agent: &str,
    model: &UserModel,
    auto: bool,
    overflow: Option<bool>,
    time: u64,
) -> (User, Part) {
    let msg = User {
        id: crate::schema::create_message(None),
        session_id: session_id.to_string(),
        role: "user".to_string(),
        time: crate::v1::UserTime { created: time },
        format: None,
        summary: None,
        agent: agent.to_string(),
        model: model.clone(),
        system: None,
        tools: None,
    };
    let part = Part::Compaction(crate::v1::CompactionPart {
        base: PartBase {
            id: crate::schema::create_part(None),
            session_id: session_id.to_string(),
            message_id: msg.id.clone(),
        },
        type_: "compaction".into(),
        auto,
        overflow,
        tail_start_id: None,
    });
    (msg, part)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_part(message_id: &str, text: &str) -> Part {
        Part::Text(crate::v1::TextPart {
            base: PartBase {
                id: crate::schema::create_part(None),
                session_id: "s".into(),
                message_id: message_id.into(),
            },
            type_: "text".into(),
            text: text.into(),
            synthetic: None,
            ignored: None,
            time: None,
            metadata: None,
        })
    }

    fn user(id: &str) -> WithParts {
        WithParts {
            info: Info::User(User {
                id: id.into(),
                session_id: "s".into(),
                role: "user".into(),
                time: crate::v1::UserTime { created: 0 },
                format: None,
                summary: None,
                agent: "primary".into(),
                model: UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            }),
            parts: vec![text_part(id, id)],
        }
    }

    fn assistant(id: &str, parent: &str, summary: Option<bool>) -> WithParts {
        WithParts {
            info: Info::Assistant(crate::v1::Assistant {
                id: id.into(),
                session_id: "s".into(),
                role: "assistant".into(),
                time: crate::v1::AssistantTime {
                    created: 0,
                    completed: None,
                },
                error: None,
                parent_id: parent.into(),
                model_id: "m".into(),
                provider_id: "p".into(),
                mode: "primary".into(),
                agent: "primary".into(),
                path: crate::v1::AssistantPath {
                    cwd: "/w".into(),
                    root: "/w".into(),
                },
                summary,
                cost: 0.0,
                tokens: crate::v1::AssistantTokens {
                    total: None,
                    input: 0.0,
                    output: 0.0,
                    reasoning: 0.0,
                    cache: crate::v1::CacheTokens {
                        read: 0.0,
                        write: 0.0,
                    },
                },
                structured: None,
                variant: None,
                finish: Some("stop".into()),
            }),
            parts: vec![],
        }
    }

    #[test]
    fn turns_skip_compaction_messages() {
        let mut compaction_user = user("c");
        compaction_user.parts = vec![Part::Compaction(crate::v1::CompactionPart {
            base: PartBase {
                id: "p".into(),
                session_id: "s".into(),
                message_id: "c".into(),
            },
            type_: "compaction".into(),
            auto: true,
            overflow: None,
            tail_start_id: None,
        })];
        let messages = vec![
            user("m1"),
            assistant("a1", "m1", None),
            compaction_user,
            user("m2"),
        ];
        let result = turns(&messages);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "m1");
        assert_eq!(result[1].id, "m2");
    }

    #[test]
    fn completed_compactions_only_for_summary_assistant() {
        let mut compaction_user = user("m1");
        compaction_user.parts = vec![Part::Compaction(crate::v1::CompactionPart {
            base: PartBase {
                id: "p".into(),
                session_id: "s".into(),
                message_id: "m1".into(),
            },
            type_: "compaction".into(),
            auto: true,
            overflow: None,
            tail_start_id: None,
        })];
        let messages = vec![compaction_user, assistant("a1", "m1", Some(true))];
        let result = completed_compactions(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].assistant_index, 1);
    }

    #[test]
    fn summary_text_joins_text_parts() {
        let msg = WithParts {
            info: Info::User(User {
                id: "m".into(),
                session_id: "s".into(),
                role: "user".into(),
                time: crate::v1::UserTime { created: 0 },
                format: None,
                summary: None,
                agent: "primary".into(),
                model: UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            }),
            parts: vec![text_part("m", "  first  "), text_part("m", "second")],
        };
        assert_eq!(summary_text(&msg).as_deref(), Some("first\n\nsecond"));
    }

    #[test]
    fn create_compaction_message_builds_user_and_part() {
        let (msg, part) = create_compaction_message(
            "ses1",
            "primary",
            &UserModel {
                provider_id: "p".into(),
                model_id: "m".into(),
                variant: None,
            },
            true,
            Some(false),
            42,
        );
        assert_eq!(msg.agent, "primary");
        assert_eq!(msg.id, part.message_id());
        let Part::Compaction(compaction) = part else {
            panic!("expected compaction part")
        };
        assert!(compaction.auto);
        assert_eq!(compaction.overflow, Some(false));
    }
}
