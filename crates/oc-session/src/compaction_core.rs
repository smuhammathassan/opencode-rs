/// From reference/packages/core/src/session/compaction.ts
///
/// Context-compaction primitives: settings resolution, conversation
/// serialization, head/recent selection and the summary prompt template.
use crate::util::token;
use crate::v2::Message;

pub const DEFAULT_BUFFER: u64 = 20_000;
pub const DEFAULT_KEEP_TOKENS: u64 = 8_000;
pub const TOOL_OUTPUT_MAX_CHARS: usize = 2_000;
pub const SUMMARY_OUTPUT_TOKENS: u64 = 4_096;

pub const SUMMARY_TEMPLATE: &str = include_str!("templates/summary-template.txt");

#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub seq: u64,
    pub message: &'a Message,
}

/// ConfigV2.Compaction — see reference `core/config/compaction.ts`.
#[derive(Debug, Clone, Default)]
pub struct CompactionSettings {
    pub auto: Option<bool>,
    pub prune: Option<bool>,
    pub keep_tokens: Option<u64>,
    pub buffer: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub auto: bool,
    pub buffer: u64,
    pub tokens: u64,
}

/// From reference `compaction.ts:settings`.
pub fn settings(documents: &[CompactionSettings]) -> Settings {
    documents.iter().fold(
        Settings {
            auto: true,
            buffer: DEFAULT_BUFFER,
            tokens: DEFAULT_KEEP_TOKENS,
        },
        |result, current| Settings {
            auto: current.auto.unwrap_or(result.auto),
            buffer: current.buffer.unwrap_or(result.buffer),
            tokens: current.keep_tokens.unwrap_or(result.tokens),
        },
    )
}

fn truncate(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= TOOL_OUTPUT_MAX_CHARS {
        return value.to_string();
    }
    let head: String = chars[..TOOL_OUTPUT_MAX_CHARS].iter().collect();
    format!("{head}\n[truncated]")
}

/// From reference `compaction.ts:serializeToolContent`.
pub fn serialize_tool_content(content: &[crate::v2::ToolContent]) -> String {
    content
        .iter()
        .map(|item| match item {
            crate::v2::ToolContent::Text(t) => t.text.clone(),
            crate::v2::ToolContent::File(f) => format!(
                "[Attached {}{}]",
                f.mime,
                match &f.name {
                    Some(name) => format!(": {name}"),
                    None => String::new(),
                }
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// From reference `compaction.ts:serialize`.
pub fn serialize(message: &Message) -> String {
    match message {
        Message::User(user) => {
            let files = user
                .files
                .as_ref()
                .map(|files| {
                    files
                        .iter()
                        .map(|file| {
                            format!(
                                "[Attached {}: {}]",
                                file.mime,
                                file.name.clone().unwrap_or_else(|| file.uri.clone())
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut parts = vec![format!("[User]: {}", user.text)];
            parts.extend(files);
            parts.join("\n")
        }
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .flat_map(|part| match part {
                crate::v2::AssistantContent::Text(t) => vec![format!("[Assistant]: {}", t.text)],
                crate::v2::AssistantContent::Reasoning(r) => {
                    if r.text.trim().is_empty() {
                        vec![]
                    } else {
                        vec![format!("[Assistant reasoning]: {}", r.text)]
                    }
                }
                crate::v2::AssistantContent::Tool(tool) => {
                    let input = match &tool.state {
                        crate::v2::ToolState::Pending(p) => p.input.clone(),
                        crate::v2::ToolState::Running(r) => {
                            serde_json::to_string(&r.input).unwrap_or_default()
                        }
                        crate::v2::ToolState::Completed(c) => {
                            serde_json::to_string(&c.input).unwrap_or_default()
                        }
                        crate::v2::ToolState::Error(e) => {
                            serde_json::to_string(&e.input).unwrap_or_default()
                        }
                    };
                    match &tool.state {
                        crate::v2::ToolState::Completed(c) => vec![
                            format!("[Assistant tool call]: {}({input})", tool.name),
                            format!(
                                "[Tool result]: {}",
                                truncate(&serialize_tool_content(&c.content))
                            ),
                        ],
                        crate::v2::ToolState::Error(e) => vec![
                            format!("[Assistant tool call]: {}({input})", tool.name),
                            format!("[Tool error]: {}", e.error.message),
                        ],
                        _ => vec![format!("[Assistant tool call]: {}({input})", tool.name)],
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Message::System(system) => format!("[System update]: {}", system.text),
        Message::Synthetic(synthetic) => format!("[Synthetic context]: {}", synthetic.text),
        Message::Shell(shell) => format!("[Shell]: {}\n{}", shell.command, truncate(&shell.output)),
        _ => String::new(),
    }
}

/// From reference `compaction.ts:select` — split the conversation into a
/// compacted `head` and a preserved `recent` tail.
pub fn select(entries: &[Entry], tokens: u64) -> Option<(String, String)> {
    let conversation: Vec<String> = entries
        .iter()
        .filter(|entry| !matches!(entry.message, Message::Compaction(_)))
        .map(|entry| serialize(entry.message))
        .filter(|text| !text.is_empty())
        .collect();
    if conversation.is_empty() {
        return None;
    }
    let mut total: u64 = 0;
    let mut split = conversation.len();
    let mut split_prefix = String::new();
    let mut split_suffix = String::new();
    for index in (0..conversation.len()).rev() {
        let next = total + token::estimate(&conversation[index]);
        if next > tokens {
            let remaining = tokens.saturating_sub(total) * 4;
            if remaining > 0 {
                let chars: Vec<char> = conversation[index].chars().collect();
                let head_end = chars.len().saturating_sub(remaining as usize);
                split_prefix = chars[..head_end].iter().collect();
                split_suffix = chars[head_end..].iter().collect();
                split = index + 1;
            }
            break;
        }
        total = next;
        split = index;
    }
    let head: Vec<String> = conversation[..split]
        .iter()
        .cloned()
        .chain(std::iter::once(split_prefix))
        .filter(|text| !text.is_empty())
        .collect();
    let recent: Vec<String> = std::iter::once(split_suffix)
        .chain(conversation[split..].iter().cloned())
        .filter(|text| !text.is_empty())
        .collect();
    Some((head.join("\n\n"), recent.join("\n\n")))
}

/// From reference `compaction.ts:buildPrompt`.
pub fn build_prompt(previous_summary: Option<&str>, context: &[&str]) -> String {
    let anchor = match previous_summary {
        Some(summary) => format!(
            "Update the anchored summary below using the conversation history above.\nPreserve still-true details, remove stale details, and merge in the new facts.\n<previous-summary>\n{summary}\n</previous-summary>"
        ),
        None => "Create a new anchored summary from the conversation history.".to_string(),
    };
    std::iter::once(anchor)
        .chain(std::iter::once(SUMMARY_TEMPLATE.to_string()))
        .chain(context.iter().map(|c| (*c).to_string()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2;

    fn user_message(text: &str, files: Vec<v2::FileAttachment>) -> Message {
        Message::User(v2::User {
            base: v2::MessageBase {
                id: "msg_1".into(),
                metadata: None,
                time: v2::MessageTime { created: 1 },
            },
            text: text.to_string(),
            files: if files.is_empty() { None } else { Some(files) },
            agents: None,
            type_: "user".into(),
        })
    }

    fn text_part(text: &str) -> v2::AssistantContent {
        v2::AssistantContent::Text(v2::AssistantText {
            type_: "text".into(),
            id: "t1".into(),
            text: text.to_string(),
        })
    }

    fn assistant_message(content: Vec<v2::AssistantContent>) -> Message {
        Message::Assistant(v2::Assistant {
            base: v2::MessageBaseId {
                id: "msg_2".into(),
                metadata: None,
            },
            type_: "assistant".into(),
            agent: "primary".into(),
            model: v2::ModelRef {
                id: "m".into(),
                provider_id: "p".into(),
                variant: None,
            },
            content,
            snapshot: None,
            finish: None,
            cost: None,
            tokens: None,
            error: None,
            time: v2::AssistantTime {
                created: 2,
                completed: None,
            },
        })
    }

    #[test]
    fn settings_merge_matches_reference() {
        let docs = vec![
            CompactionSettings {
                auto: Some(false),
                ..Default::default()
            },
            CompactionSettings {
                buffer: Some(5000),
                ..Default::default()
            },
        ];
        let result = settings(&docs);
        assert!(!result.auto);
        assert_eq!(result.buffer, 5000);
        assert_eq!(result.tokens, DEFAULT_KEEP_TOKENS);
    }

    #[test]
    fn select_splits_head_and_recent() {
        let messages = [
            user_message("hello", vec![]),
            assistant_message(vec![text_part("hi there")]),
            user_message("world", vec![]),
            assistant_message(vec![text_part("again")]),
        ];
        let entries: Vec<Entry> = messages
            .iter()
            .enumerate()
            .map(|(i, m)| Entry {
                seq: i as u64,
                message: m,
            })
            .collect();
        let (head, recent) = select(&entries, 10).expect("selection");
        assert!(head.contains("[User]: hello"));
        assert!(recent.contains("[User]: world"));
    }

    #[test]
    fn serialize_user_includes_attachments() {
        let message = user_message(
            "look",
            vec![v2::FileAttachment {
                uri: "file:///a.png".into(),
                mime: "image/png".into(),
                name: None,
                description: None,
                source: None,
            }],
        );
        let text = serialize(&message);
        assert!(text.contains("[User]: look"));
        assert!(text.contains("[Attached image/png: file:///a.png]"));
    }

    #[test]
    fn build_prompt_without_previous_summary() {
        let prompt = build_prompt(None, &[]);
        assert!(
            prompt.starts_with("Create a new anchored summary from the conversation history.\n\n")
        );
        assert!(prompt.contains("## Objective"));
    }

    #[test]
    fn build_prompt_anchors_previous_summary() {
        let prompt = build_prompt(Some("previous"), &["context"]);
        assert!(prompt.contains("<previous-summary>\nprevious\n</previous-summary>"));
        assert!(prompt.ends_with("context"));
    }
}
