#![allow(clippy::all)]
//! Integration tests: snapshot the rendered layouts of the core screens with
//! fixed-width input.

use std::collections::HashSet;

use oc_tui::components::message::{render_messages, SessionRender};
use oc_tui::prompt::state::PromptState;
use oc_tui::sync::SyncState;
use oc_tui::theme::Theme;
use oc_tui::types::{Message, Part};
use serde_json::json;

fn session_fixture() -> SyncState {
    let mut sync = SyncState::default();
    sync.agents = serde_json::from_value(json!([
        { "name": "build", "mode": "primary", "permission": [], "options": {} }
    ]))
    .unwrap();
    sync.providers = serde_json::from_value(json!([
        { "id": "openrouter", "name": "OpenRouter", "source": "config", "env": [],
          "options": {}, "models": {
            "model-x": { "id": "model-x", "providerID": "openrouter", "name": "Model X",
                "capabilities": {"input": {}, "output": {}}, "cost": {"input": 0, "output": 0, "cache": {"read": 0, "write": 0}},
                "limit": {"context": 1000, "output": 1000}, "status": "active", "options": {}, "headers": {}, "release_date": "" } }
        }
    ]))
    .unwrap();
    let session: oc_tui::types::Session = serde_json::from_value(json!({
        "id": "ses_1", "slug": "s", "projectID": "p", "directory": "/proj", "title": "Fix bug",
        "version": "1", "time": { "created": 1, "updated": 1 }
    }))
    .unwrap();
    sync.upsert_session(session);

    let user: Message = serde_json::from_value(json!({
        "id": "msg_a", "sessionID": "ses_1", "role": "user", "agent": "build",
        "model": { "id": "model-x", "providerID": "openrouter" }, "time": { "created": 1700000000000u64 }
    }))
    .unwrap();
    sync.upsert_message("ses_1", user);
    let text_part: Part = serde_json::from_value(json!({
        "id": "pt_1", "sessionID": "ses_1", "messageID": "msg_a", "type": "text",
        "text": "Fix the bug in src/main.rs"
    }))
    .unwrap();
    sync.upsert_part("msg_a", text_part);

    let assistant: Message = serde_json::from_value(json!({
        "id": "msg_b", "sessionID": "ses_1", "role": "assistant", "time": { "created": 2, "completed": 5000 },
        "parentID": "msg_a", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
        "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0.01,
        "tokens": { "input": 10, "output": 20, "reasoning": 5, "cache": { "read": 0, "write": 0 } },
        "finish": "end_turn"
    }))
    .unwrap();
    sync.upsert_message("ses_1", assistant);

    let reasoning: Part = serde_json::from_value(json!({
        "id": "pt_r", "sessionID": "ses_1", "messageID": "msg_b", "type": "reasoning",
        "text": "**Inspecting**\n\nchecking the code", "time": { "start": 2, "end": 3000 }
    }))
    .unwrap();
    sync.upsert_part("msg_b", reasoning);

    let text_part2: Part = serde_json::from_value(json!({
        "id": "pt_2", "sessionID": "ses_1", "messageID": "msg_b", "type": "text",
        "text": "I found the issue. Here is the **fix**:\n\n```rust\nlet x = 1;\n```"
    }))
    .unwrap();
    sync.upsert_part("msg_b", text_part2);

    let glob: Part = serde_json::from_value(json!({
        "id": "pt_g", "sessionID": "ses_1", "messageID": "msg_b", "type": "tool", "callID": "call_g",
        "tool": "glob", "state": {
            "status": "completed", "input": { "pattern": "**/*.rs" }, "output": "[]",
            "title": "glob", "metadata": { "count": 3 }, "time": { "start": 2, "end": 4000 }
        }
    }))
    .unwrap();
    sync.upsert_part("msg_b", glob);

    let bash: Part = serde_json::from_value(json!({
        "id": "pt_b", "sessionID": "ses_1", "messageID": "msg_b", "type": "tool", "callID": "call_b",
        "tool": "bash", "state": {
            "status": "completed",
            "input": { "command": "cargo test", "workdir": "/proj" },
            "output": "running 3 tests\nall passed",
            "title": "bash", "metadata": {}, "time": { "start": 2, "end": 5000 }
        }
    }))
    .unwrap();
    sync.upsert_part("msg_b", bash);
    sync
}

fn render_to_text(sync: &SyncState, width: usize) -> Vec<String> {
    let render = SessionRender {
        width,
        sync,
        theme: &Theme::dark(),
        conceal: true,
        thinking_mode: "hide",
        show_timestamps: true,
        show_details: true,
        show_generic_tool_output: true,
        expanded_tools: &HashSet::new(),
        reasoning_expanded: &HashSet::new(),
        session_id: "ses_1",
        cwd: "/proj",
    };
    render_messages(&render)
        .into_iter()
        .map(|l| l.line.into_iter().map(|(s, _)| s).collect::<String>())
        .collect()
}

#[test]
fn session_message_list_snapshot() {
    let sync = session_fixture();
    let lines = render_to_text(&sync, 80);

    // User message with left border.
    assert!(lines
        .iter()
        .any(|l| l.starts_with('┃') && l.contains("Fix the bug in src/main.rs")));
    // Reasoning collapsed in hide mode.
    assert!(lines
        .iter()
        .any(|l| l.contains("Thought") && l.contains("Inspecting")));
    assert!(!lines.iter().any(|l| l.contains("checking the code")));
    // Markdown text rendered with code block concealed.
    assert!(lines.iter().any(|l| l.contains("I found the issue")));
    assert!(lines.iter().any(|l| l.contains("concealed")));
    // Tool rows.
    assert!(lines
        .iter()
        .any(|l| l.contains("Glob") && l.contains("3 matches")));
    assert!(lines.iter().any(|l| l.contains("$ cargo test")));
    assert!(lines.iter().any(|l| l.contains("all passed")));
    // Assistant footer with mode/model/duration.
    assert!(lines
        .iter()
        .any(|l| l.contains("▣") && l.contains("Primary") && l.contains("OpenRouter")));
}

#[test]
fn message_list_wraps_to_fixed_width() {
    let sync = session_fixture();
    let lines = render_to_text(&sync, 40);
    for line in &lines {
        let text_width = unicode_width::UnicodeWidthStr::width(line.as_str());
        assert!(text_width <= 44, "line too wide ({text_width}): {line:?}");
    }
}

#[test]
fn prompt_layout_renders_placeholder_and_cursor() {
    let _prompt = PromptState::default();
    let (lines, cursor) = oc_tui::components::prompt::prompt_lines(
        "",
        40,
        0,
        &[],
        &Theme::dark(),
        Theme::dark().border,
        Some("Ask anything... \"Fix broken tests\""),
    );
    assert!(!lines.is_empty());
    assert!(lines[0].iter().any(|(s, _)| s.contains("Ask anything...")));
    assert!(lines.iter().any(|l| l.iter().any(|(s, _)| s.contains("▀"))));
    assert!(cursor.is_some());
}

#[test]
fn prompt_expands_text_parts_on_submit() {
    let mut prompt = PromptState::default();
    prompt.buffer.insert_str("[Pasted ~3 lines] rest");
    let part = json!({
        "type": "text", "text": "line1\nline2\nline3",
        "source": { "text": { "value": "[Pasted ~3 lines]", "start": 0, "end": 17 } }
    });
    prompt.parts.push(part);
    let expanded = oc_tui::prompt::parts::expand_text_parts(&prompt.text(), &prompt.parts);
    assert_eq!(expanded, "line1\nline2\nline3 rest");
}
