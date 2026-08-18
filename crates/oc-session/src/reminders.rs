/// From reference/packages/opencode/src/session/reminders.ts
///
/// Plan-mode / build-mode system reminders injected into the latest user
/// message.
use crate::v1::{Part, PartBase, WithParts};

pub const PROMPT_PLAN: &str = include_str!("../assets/prompt/plan.txt");
pub const BUILD_SWITCH: &str = include_str!("../assets/prompt/build-switch.txt");
pub const PLAN_MODE: &str = include_str!("../assets/prompt/plan-mode.txt");

#[derive(Debug, Clone)]
pub struct ReminderContext {
    pub session_id: String,
    pub plan_path: String,
    pub plan_exists: bool,
}

impl ReminderContext {
    /// Build the reminder context from the resolved plan file state
    /// (`oc_session::plan::ensure_plan_file`).
    pub fn from_plan_file(
        session_id: impl Into<String>,
        plan: &crate::plan::PlanFileState,
    ) -> Self {
        ReminderContext {
            session_id: session_id.into(),
            plan_path: plan.path.clone(),
            plan_exists: plan.exists,
        }
    }
}

/// Compute the synthetic reminder text for the latest user message, mirroring
/// the reference `reminders.ts:apply` prompt-level effect. Returns `None` when
/// no reminder applies.
pub fn reminder_text(
    agent_name: &str,
    last_assistant_agent: Option<&str>,
    was_plan: bool,
    ctx: &ReminderContext,
    experimental_plan_mode: bool,
) -> Option<String> {
    if !experimental_plan_mode {
        if agent_name == "plan" {
            return Some(PROMPT_PLAN.to_string());
        }
        if was_plan && agent_name == "build" {
            return Some(BUILD_SWITCH.to_string());
        }
        return None;
    }

    if agent_name != "plan" && last_assistant_agent == Some("plan") {
        let text = if ctx.plan_exists {
            format!(
                "{BUILD_SWITCH}\n\nA plan file exists at {}. You should execute on the plan defined within it",
                ctx.plan_path
            )
        } else {
            BUILD_SWITCH.to_string()
        };
        return Some(text);
    }

    if agent_name != "plan" || last_assistant_agent == Some("plan") {
        return None;
    }

    let plan_info = if ctx.plan_exists {
        format!(
            "A plan file already exists at {}. You can read it and make incremental edits using the edit tool.",
            ctx.plan_path
        )
    } else {
        format!(
            "No plan file exists yet. You should create your plan at {} using the write tool.",
            ctx.plan_path
        )
    };
    Some(PLAN_MODE.replace("${planInfo}", &plan_info))
}

/// From reference `reminders.ts:apply`. Returns the synthetic text parts to
/// append to the latest user message, plus which parts were created.
pub fn apply(
    messages: &[WithParts],
    agent_name: &str,
    ctx: &ReminderContext,
    experimental_plan_mode: bool,
) -> Vec<ReminderPart> {
    let Some(user_message) = messages.iter().rev().find(|msg| msg.info.role() == "user") else {
        return Vec::new();
    };
    let assistant_agent = messages
        .iter()
        .rev()
        .find(|msg| msg.info.role() == "assistant")
        .and_then(|msg| match &msg.info {
            crate::v1::Info::Assistant(assistant) => Some(assistant.agent.clone()),
            _ => None,
        });
    let was_plan = messages
        .iter()
        .any(|msg| matches!(&msg.info, crate::v1::Info::Assistant(a) if a.agent == "plan"));

    let Some(text) = reminder_text(
        agent_name,
        assistant_agent.as_deref(),
        was_plan,
        ctx,
        experimental_plan_mode,
    ) else {
        return Vec::new();
    };
    vec![text_part(user_message, ctx, text)]
}

#[derive(Debug, Clone)]
pub struct ReminderPart {
    pub session_id: String,
    pub message_id: String,
    pub text: String,
}

fn text_part(user_message: &WithParts, ctx: &ReminderContext, text: String) -> ReminderPart {
    let _ = ctx;
    ReminderPart {
        session_id: user_message.info.id_session().to_string(),
        message_id: user_message.info.id().to_string(),
        text,
    }
}

/// Convert a reminder into a persisted V1 text part.
pub fn to_part(reminder: &ReminderPart) -> Part {
    Part::Text(crate::v1::TextPart {
        base: PartBase {
            id: crate::schema::create_part(None),
            session_id: reminder.session_id.clone(),
            message_id: reminder.message_id.clone(),
        },
        type_: "text".into(),
        text: reminder.text.clone(),
        synthetic: Some(true),
        ignored: None,
        time: None,
        metadata: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> WithParts {
        WithParts {
            info: crate::v1::Info::User(crate::v1::User {
                id: id.into(),
                session_id: "s".into(),
                role: "user".into(),
                time: crate::v1::UserTime { created: 0 },
                format: None,
                summary: None,
                agent: "plan".into(),
                model: crate::v1::UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            }),
            parts: vec![],
        }
    }

    #[test]
    fn plan_agent_gets_plan_reminder() {
        let messages = vec![user("m1")];
        let ctx = ReminderContext {
            session_id: "s".into(),
            plan_path: "/work/plans/1.md".into(),
            plan_exists: false,
        };
        let parts = apply(&messages, "plan", &ctx, false);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].text.contains("Plan Mode - System Reminder"));
    }

    #[test]
    fn build_after_plan_gets_build_switch() {
        let assistant = WithParts {
            info: crate::v1::Info::Assistant(crate::v1::Assistant {
                id: "m2".into(),
                session_id: "s".into(),
                role: "assistant".into(),
                time: crate::v1::AssistantTime {
                    created: 1,
                    completed: None,
                },
                error: None,
                parent_id: "m1".into(),
                model_id: "m".into(),
                provider_id: "p".into(),
                mode: "plan".into(),
                agent: "plan".into(),
                path: crate::v1::AssistantPath {
                    cwd: "/w".into(),
                    root: "/w".into(),
                },
                summary: None,
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
        };
        let messages = vec![user("m1"), assistant];
        let ctx = ReminderContext {
            session_id: "s".into(),
            plan_path: "/work/plans/1.md".into(),
            plan_exists: false,
        };
        let parts = apply(&messages, "build", &ctx, false);
        assert_eq!(parts.len(), 1);
        assert!(parts[0]
            .text
            .contains("Your operational mode has changed from plan to build"));
    }

    #[test]
    fn reminder_text_experimental_plan_mode_references_plan_file() {
        let ctx = ReminderContext {
            session_id: "s".into(),
            plan_path: "/work/repo/.opencode/plans/1-abc.md".into(),
            plan_exists: false,
        };
        // Entering plan mode: instructs the plan agent to create the plan file.
        let text = reminder_text("plan", None, false, &ctx, true).unwrap();
        assert!(text.contains("No plan file exists yet."));
        assert!(text.contains("/work/repo/.opencode/plans/1-abc.md"));

        // The plan file already exists: incremental edits.
        let ctx = ReminderContext {
            plan_exists: true,
            ..ctx
        };
        let text = reminder_text("plan", None, false, &ctx, true).unwrap();
        assert!(text.contains("A plan file already exists at"));

        // Switching back to build references the existing plan file.
        let text = reminder_text("build", Some("plan"), false, &ctx, true).unwrap();
        assert!(text.contains("A plan file exists at"));
        assert!(text.contains("execute on the plan defined within it"));

        // No reminder outside plan/build transitions.
        assert!(reminder_text("build", None, false, &ctx, true).is_none());
        assert!(reminder_text("plan", Some("plan"), false, &ctx, true).is_none());
    }
}
