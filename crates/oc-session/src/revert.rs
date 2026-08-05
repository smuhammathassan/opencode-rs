/// From reference/packages/opencode/src/session/revert.ts
///
/// Session revert / unrevert / cleanup.
use crate::v1::{Part, WithParts};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertInput {
    pub session_id: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RevertResult {
    pub revert: crate::session::Revert,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
    /// Part ids (in the target message) removed from view.
    pub patches: Vec<String>,
    /// Messages (with parts) removed.
    pub removed: Vec<WithParts>,
}

/// From reference `revert.ts:revert` — finds the revert point in history.
pub fn compute_revert(
    all: &[WithParts],
    input: &RevertInput,
    existing_snapshot: Option<&str>,
) -> Option<RevertResult> {
    let mut last_user: Option<&str> = None;
    let mut revert: Option<crate::session::Revert> = None;
    let mut patches: Vec<String> = Vec::new();
    for msg in all {
        if msg.info.role() == "user" {
            last_user = Some(msg.info.id());
        }
        if let Some(_rev) = &revert {
            for part in &msg.parts {
                if let Part::Patch(patch) = part {
                    patches.push(patch.base.id.clone());
                }
            }
            continue;
        }
        let hit = (msg.info.id() == input.message_id && input.part_id.is_none())
            || msg
                .parts
                .iter()
                .any(|part| Some(part.id()) == input.part_id.as_deref());
        if hit {
            let has_text_or_tool = msg
                .parts
                .iter()
                .any(|item| matches!(item, Part::Text(_) | Part::Tool(_)));
            let part_id = if has_text_or_tool {
                input.part_id.clone()
            } else {
                None
            };
            revert = Some(crate::session::Revert {
                message_id: match (part_id.is_none(), last_user) {
                    (true, Some(last_user)) => last_user.to_string(),
                    _ => msg.info.id().to_string(),
                },
                part_id,
                snapshot: None,
                diff: None,
            });
        }
    }
    let mut revert = revert?;
    revert.snapshot = existing_snapshot.map(|s| s.to_string());
    let removed: Vec<WithParts> = all
        .iter()
        .filter(|msg| msg.info.id() >= revert.message_id.as_str())
        .cloned()
        .collect();
    let snapshot = revert.snapshot.clone();
    Some(RevertResult {
        revert,
        snapshot,
        diff: None,
        patches,
        removed,
    })
}

/// From reference `revert.ts:cleanup` — remove messages/parts after the revert
/// point. Returns message ids to remove and (message_id, part_id) pairs for
/// the target message.
#[derive(Debug, Clone, Default)]
pub struct CleanupResult {
    pub remove_messages: Vec<String>,
    pub remove_parts: Vec<(String, String)>,
}

pub fn cleanup(session: &crate::session::Info, messages: &[WithParts]) -> CleanupResult {
    let mut result = CleanupResult::default();
    let Some(revert) = &session.revert else {
        return result;
    };
    let message_id = &revert.message_id;
    let mut target: Option<&WithParts> = None;
    for msg in messages {
        if msg.info.id() < message_id.as_str() {
            continue;
        }
        if msg.info.id() > message_id.as_str() {
            result.remove_messages.push(msg.info.id().to_string());
            continue;
        }
        if revert.part_id.is_some() {
            target = Some(msg);
        } else {
            result.remove_messages.push(msg.info.id().to_string());
        }
    }
    if let Some(target) = target {
        if let Some(part_id) = &revert.part_id {
            if let Some(idx) = target.parts.iter().position(|part| part.id() == part_id) {
                for part in &target.parts[idx..] {
                    result
                        .remove_parts
                        .push((target.info.id().to_string(), part.id().to_string()));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_with_text(id: &str, part_id: &str) -> WithParts {
        WithParts {
            info: crate::v1::Info::User(Box::new(crate::v1::User {
                id: id.into(),
                session_id: "s".into(),
                role: "user".into(),
                time: crate::v1::UserTime { created: 0 },
                format: None,
                summary: None,
                agent: "primary".into(),
                model: crate::v1::UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            })),
            parts: vec![Part::Text(crate::v1::TextPart {
                base: crate::v1::PartBase {
                    id: part_id.into(),
                    session_id: "s".into(),
                    message_id: id.into(),
                },
                type_: "text".into(),
                text: "hi".into(),
                synthetic: None,
                ignored: None,
                time: None,
                metadata: None,
            })],
        }
    }

    #[test]
    fn revert_points_at_last_user_without_part_id() {
        let messages = vec![user_with_text("m1", "p1"), user_with_text("m2", "p2")];
        let input = RevertInput {
            session_id: "s".into(),
            message_id: "m2".into(),
            part_id: None,
        };
        let result = compute_revert(&messages, &input, None).unwrap();
        assert_eq!(result.revert.message_id, "m2");
        assert!(result.revert.part_id.is_none());
    }

    #[test]
    fn cleanup_removes_messages_after_revert_point() {
        let messages = vec![
            user_with_text("m1", "p1"),
            user_with_text("m2", "p2"),
            user_with_text("m3", "p3"),
        ];
        let session = crate::session::Info {
            id: "s".into(),
            revert: Some(crate::v1::SessionRevert {
                message_id: "m2".into(),
                part_id: None,
                snapshot: None,
                diff: None,
            }),
            ..crate::session::Info::default()
        };
        let result = cleanup(&session, &messages);
        assert_eq!(result.remove_messages, vec!["m2", "m3"]);
    }
}
