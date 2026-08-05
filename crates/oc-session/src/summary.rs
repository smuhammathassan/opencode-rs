/// From reference/packages/opencode/src/session/summary.ts
///
/// Session summary + diff logic. `unquote_git_path` decodes git's C-style
/// quoting of file names; `compute_diff` walks step-start/step-finish
/// snapshots.
use crate::v1::{FileDiff, Info, WithParts};

/// From reference `summary.ts:unquoteGitPath`.
pub fn unquote_git_path(input: &str) -> String {
    if !input.starts_with('"') || !input.ends_with('"') {
        return input.to_string();
    }
    let body: Vec<char> = input[1..input.len() - 1].chars().collect();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if body[i] != '\\' {
            bytes.push(byte_of(body[i]));
            i += 1;
            continue;
        }
        let next = body.get(i + 1);
        let Some(next) = next else {
            bytes.push(b'\\');
            i += 1;
            continue;
        };
        if next.is_ascii_digit() && *next >= '0' && *next <= '7' {
            let chunk: String = body[i + 1..(i + 4).min(body.len())].iter().collect();
            let match_len = chunk.chars().take_while(|c| matches!(c, '0'..='7')).count();
            if match_len == 0 {
                bytes.push(byte_of(*next));
                i += 2;
                continue;
            }
            let octal: String = chunk[..match_len].to_string();
            if let Ok(value) = u32::from_str_radix(&octal, 8) {
                bytes.push(value as u8);
            }
            i += 1 + match_len;
            continue;
        }
        let escaped: Option<u8> = match next {
            'n' => Some(b'\n'),
            'r' => Some(b'\r'),
            't' => Some(b'\t'),
            'b' => Some(0x08),
            'f' => Some(0x0c),
            'v' => Some(0x0b),
            '\\' | '"' => Some(byte_of(*next)),
            _ => None,
        };
        bytes.push(escaped.unwrap_or_else(|| byte_of(*next)));
        i += 2;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn byte_of(c: char) -> u8 {
    if c.is_ascii() {
        c as u8
    } else {
        '?'.encode_utf8(&mut [0u8; 4]).as_bytes()[0]
    }
}

/// From reference `summary.ts:computeDiff` — resolve `from`/`to` snapshots and
/// run the snapshot diff via the provided callback.
pub fn compute_diff(
    messages: &[WithParts],
    diff_full: impl Fn(&str, &str) -> Vec<FileDiff>,
) -> Vec<FileDiff> {
    let mut from: Option<String> = None;
    let mut to: Option<String> = None;
    for item in messages {
        if from.is_none() {
            for part in &item.parts {
                if let crate::v1::Part::StepStart(part) = part {
                    if let Some(snapshot) = &part.snapshot {
                        from = Some(snapshot.clone());
                        break;
                    }
                }
            }
        }
        for part in &item.parts {
            if let crate::v1::Part::StepFinish(part) = part {
                if let Some(snapshot) = &part.snapshot {
                    to = Some(snapshot.clone());
                }
            }
        }
    }
    match (from, to) {
        (Some(from), Some(to)) => diff_full(&from, &to),
        _ => Vec::new(),
    }
}

/// From reference `summary.ts:summarize`.
#[allow(clippy::too_many_arguments)]
pub fn summarize(
    messages: &[WithParts],
    session_id: &str,
    message_id: &str,
    snapshot_enabled: bool,
    set_summary: impl Fn(crate::v1::SessionSummary),
    publish_diff: impl Fn(Vec<FileDiff>),
    update_message: impl Fn(&WithParts),
    diff_full: impl Fn(&str, &str) -> Vec<FileDiff>,
) {
    set_summary(crate::v1::SessionSummary {
        additions: 0.0,
        deletions: 0.0,
        files: 0.0,
        diffs: None,
    });
    publish_diff(Vec::new());
    if !snapshot_enabled {
        return;
    }
    if messages.is_empty() {
        return;
    }
    let selected: Vec<WithParts> = messages
        .iter()
        .filter(|m| {
            m.info.id() == message_id
                || (m.info.role() == "assistant"
                    && matches!(&m.info, Info::Assistant(a) if a.parent_id == message_id))
        })
        .cloned()
        .collect();
    let target = selected.iter().find(|m| m.info.id() == message_id);
    let Some(target) = target else { return };
    if target.info.role() != "user" {
        return;
    }
    let msg_diffs = compute_diff(&selected, diff_full);
    let mut next = target.clone();
    if let Info::User(user) = &mut next.info {
        let summary = user.summary.get_or_insert_with(|| crate::v1::UserSummary {
            title: None,
            body: None,
            diffs: Vec::new(),
        });
        summary.diffs = msg_diffs;
    }
    update_message(&next);
    let _ = session_id;
}

/// From reference `summary.ts:diff`.
pub fn diff(messages: &[WithParts], message_id: Option<&str>) -> Vec<FileDiff> {
    let Some(message_id) = message_id else {
        return Vec::new();
    };
    let message = messages.iter().find(|item| item.info.id() == message_id);
    let Some(message) = message else {
        return Vec::new();
    };
    if message.info.role() != "user" {
        return Vec::new();
    }
    let diffs = match &message.info {
        crate::v1::Info::User(user) => user
            .summary
            .as_ref()
            .map(|s| s.diffs.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    diffs
        .into_iter()
        .map(|mut item| {
            if let Some(file) = &item.file {
                let unquoted = unquote_git_path(file);
                if unquoted != *file {
                    item.file = Some(unquoted);
                }
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1;

    #[test]
    fn unquotes_plain_strings() {
        assert_eq!(unquote_git_path("simple"), "simple");
        assert_eq!(unquote_git_path("\"simple\""), "simple");
    }

    #[test]
    fn unquotes_escaped_names() {
        assert_eq!(unquote_git_path("\"a b.txt\""), "a b.txt");
        assert_eq!(unquote_git_path("\"a\\tb.txt\""), "a\tb.txt");
        assert_eq!(unquote_git_path("\"\\303\\251.txt\""), "\u{00e9}.txt");
    }

    fn user(parts: Vec<v1::Part>) -> WithParts {
        WithParts {
            info: v1::Info::User(Box::new(v1::User {
                id: "msg1".into(),
                session_id: "ses1".into(),
                role: "user".into(),
                time: v1::UserTime { created: 1 },
                format: None,
                summary: None,
                agent: "primary".into(),
                model: v1::UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            })),
            parts,
        }
    }

    #[test]
    fn compute_diff_uses_step_snapshots() {
        let base = v1::PartBase {
            id: "p".into(),
            session_id: "s".into(),
            message_id: "m".into(),
        };
        let messages = vec![
            user(vec![v1::Part::StepStart(v1::StepStartPart {
                base: base.clone(),
                type_: "step-start".into(),
                snapshot: Some("a".into()),
            })]),
            WithParts {
                info: v1::Info::Assistant(Box::new(v1::Assistant {
                    id: "a".into(),
                    session_id: "s".into(),
                    role: "assistant".into(),
                    time: v1::AssistantTime {
                        created: 2,
                        completed: None,
                    },
                    error: None,
                    parent_id: "m".into(),
                    model_id: "m".into(),
                    provider_id: "p".into(),
                    mode: "primary".into(),
                    agent: "primary".into(),
                    path: v1::AssistantPath {
                        cwd: "/w".into(),
                        root: "/w".into(),
                    },
                    summary: None,
                    cost: 0.0,
                    tokens: v1::AssistantTokens {
                        total: None,
                        input: 0.0,
                        output: 0.0,
                        reasoning: 0.0,
                        cache: v1::CacheTokens {
                            read: 0.0,
                            write: 0.0,
                        },
                    },
                    structured: None,
                    variant: None,
                    finish: None,
                })),
                parts: vec![v1::Part::StepFinish(v1::StepFinishPart {
                    base,
                    type_: "step-finish".into(),
                    reason: "stop".into(),
                    snapshot: Some("b".into()),
                    cost: 0.0,
                    tokens: v1::StepTokens {
                        total: None,
                        input: 0.0,
                        output: 0.0,
                        reasoning: 0.0,
                        cache: v1::CacheTokens {
                            read: 0.0,
                            write: 0.0,
                        },
                    },
                })],
            },
        ];
        let diffs = compute_diff(&messages, |from, to| {
            vec![FileDiff {
                file: Some(format!("{from}-{to}")),
                patch: None,
                additions: 1.0,
                deletions: 0.0,
                status: None,
            }]
        });
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].file.as_deref(), Some("a-b"));
    }

    #[test]
    fn diff_unquotes_git_paths() {
        let mut msg = user(vec![]);
        if let v1::Info::User(user) = &mut msg.info {
            user.summary = Some(v1::UserSummary {
                title: None,
                body: None,
                diffs: vec![FileDiff {
                    file: Some("\"my file.txt\"".into()),
                    patch: None,
                    additions: 1.0,
                    deletions: 0.0,
                    status: None,
                }],
            });
        }
        let diffs = diff(&[msg], Some("msg1"));
        assert_eq!(diffs[0].file.as_deref(), Some("my file.txt"));
    }
}
