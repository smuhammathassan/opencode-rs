//! Port of `reference/packages/opencode/src/tool/question.ts`.

use crate::model::ExecuteResult;
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};

/// `Question.Option` item (`reference/packages/schema/src/question.ts:30`).
fn option_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("label", Schema::string("Display text (1-5 words, concise)")),
            prop("description", Schema::string("Explanation of choice")),
        ],
        "option",
    )
}

/// `description` from `reference/packages/core/src/tool/question.ts:14`
/// (identical to the opencode `question.txt`).
pub const DESCRIPTION: &str = "Use this tool when you need to ask the user questions during execution. This allows you to:
1. Gather user preferences or requirements
2. Clarify ambiguous instructions
3. Get decisions on implementation choices as you work
4. Offer choices to the user about what direction to take.

Usage notes:
- When `custom` is enabled (default), a \"Type your own answer\" option is added automatically; don't include \"Other\" or catch-all options
- Answers are returned as arrays of labels; set `multiple: true` to allow selecting more than one
- If you recommend a specific option, make that the first option in the list and add \"(Recommended)\" at the end of the label";

/// `Question.Prompt` item (`reference/packages/schema/src/question.ts:43`).
pub fn prompt_schema() -> Schema {
    Schema::struct_(
        vec![
            prop("question", Schema::string("Complete question")),
            prop("header", Schema::string("Very short label (max 30 chars)")),
            prop(
                "options",
                Schema::array(option_schema(), "Available choices"),
            ),
            opt_prop(
                "multiple",
                Schema::boolean("Allow selecting multiple choices"),
            ),
        ],
        "question",
    )
}

/// `Parameters` from `reference/packages/opencode/src/tool/question.ts:6`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![prop(
            "questions",
            Schema::array(prompt_schema(), "Questions to ask"),
        )],
        "question",
    )
}

/// `toModelOutput` from `reference/packages/core/src/tool/question.ts:34`.
pub fn to_model_output(questions: &serde_json::Value, answers: &serde_json::Value) -> String {
    let questions = questions.as_array().cloned().unwrap_or_default();
    let answers = answers.as_array().cloned().unwrap_or_default();
    let formatted = questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let text = question
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let answer = answers
                .get(index)
                .and_then(|v| v.as_array())
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|label| label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|joined| !joined.is_empty())
                .unwrap_or_else(|| "Unanswered".to_string());
            format!("\"{text}\"=\"{answer}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "User has answered your questions: {formatted}. You can now continue with the user's answers in mind."
    )
}

/// `QuestionTool` from `reference/packages/opencode/src/tool/question.ts:14`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("question", prompts::QUESTION, parameters(), |args, ctx| {
        let questions = args
            .get("questions")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let tool = ctx
            .call_id
            .clone()
            .map(|call_id| (ctx.message_id.clone(), call_id));
        let answers = ctx
            .services
            .question_ask(&ctx.session_id, &questions, tool)?;
        let answers = answers.as_array().cloned().unwrap_or_default();

        let mut formatted_parts = Vec::new();
        if let Some(questions) = questions.as_array() {
            for (index, question) in questions.iter().enumerate() {
                let text = question
                    .get("question")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let answer = answers
                    .get(index)
                    .and_then(|v| v.as_array())
                    .map(|labels| {
                        labels
                            .iter()
                            .filter_map(|label| label.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .filter(|joined| !joined.is_empty())
                    .unwrap_or_else(|| "Unanswered".to_string());
                formatted_parts.push(format!("\"{text}\"=\"{answer}\""));
            }
        }

        let count = questions.as_array().map(|items| items.len()).unwrap_or(0);
        Ok(ExecuteResult {
                title: format!("Asked {count} question{}", if count > 1 { "s" } else { "" }),
                output: format!(
                    "User has answered your questions: {}. You can now continue with the user's answers in mind.",
                    formatted_parts.join(", ")
                ),
                metadata: serde_json::json!({ "answers": answers }),
                attachments: None,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "questions": {
                        "description": "Questions to ask",
                        "items": {
                            "properties": {
                                "header": { "description": "Very short label (max 30 chars)", "type": "string" },
                                "multiple": { "description": "Allow selecting multiple choices", "type": "boolean" },
                                "options": {
                                    "description": "Available choices",
                                    "items": {
                                        "properties": {
                                            "description": { "description": "Explanation of choice", "type": "string" },
                                            "label": { "description": "Display text (1-5 words, concise)", "type": "string" }
                                        },
                                        "required": ["label", "description"],
                                        "type": "object"
                                    },
                                    "type": "array"
                                },
                                "question": { "description": "Complete question", "type": "string" }
                            },
                            "required": ["question", "header", "options"],
                            "type": "object"
                        },
                        "type": "array"
                    }
                },
                "required": ["questions"],
                "type": "object"
            })
        );
    }
}
