#![allow(unused_imports, dead_code, clippy::all)]
//! Differential scenario harness.
//!
//! Executes the SAME production oc-tui functions the TUI uses and prints one
//! canonical-JSON line `{"scenario": <id>, "result": <payload>}` for the given
//! scenario id. Driven by `scripts/run_differential.py`; payloads mirror the
//! real reference-side evals that import vendored modules under `reference/`.

use oc_tui::clipboard::copy_command_with_lookup;
use oc_tui::keymap::{KeymapOptions, LEADER_TOKEN, OPENCODE_BASE_MODE};
use oc_tui::logo::LOGO;
use oc_tui::prompt::history::{is_duplicate_entry, parse_prompt_history, PromptInfo};
use oc_tui::prompt::interaction::{
    create_interaction_state, transition, Event, PersistedState, PromptPart,
};
use oc_tui::prompt::parts::expand_text_parts;
use oc_tui::prompt::stash::{parse_prompt_stash, MAX_STASH_ENTRIES};
use oc_tui::theme::{Mode, Theme};
use oc_tui::util::display::parse_apply_patch_files;
use oc_tui::util::format::{collapse_tool_output, format_duration};
use oc_tui::util::locale;
use serde_json::{json, Value};

fn prompt_info(input: &str, parts: Value) -> PromptInfo {
    serde_json::from_value(json!({"input": input, "parts": parts})).unwrap()
}

fn rgb_of(c: ratatui::style::Color) -> Value {
    match c {
        ratatui::style::Color::Rgb(r, g, b) => json!({"r": r, "g": g, "b": b}),
        _ => json!(null),
    }
}

fn clipboard_matrix(os: &str, wayland: bool, available: &[&str]) -> Value {
    let cmd = copy_command_with_lookup(os, wayland, |name| available.contains(&name));
    json!(cmd)
}

fn theme_preset_raw(name: &str) -> Value {
    let raw = oc_tui::theme::preset_raw_data(name).unwrap_or(Value::Null);
    json!({"defs": raw.get("defs").cloned().unwrap_or(Value::Null),
           "theme": raw.get("theme").cloned().unwrap_or(Value::Null)})
}

fn resolved_anchor(name: &str, mode: Mode) -> Value {
    let t = Theme::by_name(name, mode);
    json!({
        "primary": rgb_of(t.primary),
        "accent": rgb_of(t.accent),
        "background": rgb_of(t.background),
    })
}

fn interaction_case(event: &Event, persisted: &PersistedState) -> Value {
    let state = create_interaction_state();
    let t = transition(&state, event, persisted);
    serde_json::to_value(&t).unwrap()
}

fn result(id: &str, payload: Value) {
    println!(
        "{}",
        serde_json::to_string(&json!({"scenario": id, "result": payload})).unwrap()
    );
}

fn main() {
    let id = std::env::args().nth(1).unwrap_or_default();
    match id.as_str() {
        "001-prompt-history-parse" => {
            let corrupt = "{\"input\":\"one\",\"parts\":[]}\nnot-json\n{\"input\":\"two\",\"parts\":[]}\n";
            let overflow: String = (0..55)
                .map(|i| format!("{{\"input\":\"{}\",\"parts\":[]}}\n", i))
                .collect();
            let parsed = parse_prompt_history(corrupt);
            let over = parse_prompt_history(&overflow);
            result(
                &id,
                json!({
                    "corrupt": parsed.iter().map(|p| serde_json::to_value(p).unwrap()).collect::<Vec<_>>(),
                    "overflow_len": over.len(),
                    "overflow_first": over.first().map(|p| p.input.clone()),
                    "empty_len": parse_prompt_history("").len(),
                }),
            );
        }
        "002-prompt-history-dedup" => {
            let h1 = prompt_info("hello", json!([]));
            let h2 = prompt_info("hello", json!([]));
            let h3 = prompt_info("world", json!([]));
            let a = prompt_info("describe this", json!([{"type": "file", "mime": "image/png", "filename": "a.png"}]));
            let b = prompt_info("describe this", json!([{"type": "file", "mime": "image/png", "filename": "b.png"}]));
            result(
                &id,
                json!([
                    is_duplicate_entry(None, &h1),
                    is_duplicate_entry(Some(&h1), &h2),
                    is_duplicate_entry(Some(&h1), &h3),
                    is_duplicate_entry(Some(&a), &b),
                ]),
            );
        }
        "003-prompt-paste-placeholders" => {
            let part = json!([{
                "type": "text",
                "text": "line1\nline2",
                "source": {"text": {"value": "[Pasted ~2 lines]", "start": 0, "end": 17}}
            }]);
            let parts: Vec<Value> = part.as_array().unwrap().clone();
            result(
                &id,
                json!([
                    expand_text_parts("[Pasted ~2 lines] tail", &parts),
                    expand_text_parts("plain tail", &[]),
                ]),
            );
        }
        "004-prompt-stash" => {
            let text = format!(
                "{{\"input\":\"one\"}}\nbad\n{{\"input\":\"two\"}}\n{}",
                (0..(MAX_STASH_ENTRIES + 3))
                    .map(|i| format!("{{\"input\":\"overflow{}\"}}\n", i))
                    .collect::<String>()
            );
            let parsed = parse_prompt_stash(&text);
            result(
                &id,
                json!({
                    "len": parsed.len(),
                    "first": parsed.first().map(|e| e.input.clone()),
                    "max": MAX_STASH_ENTRIES,
                }),
            );
        }
        "005-keymap-leader" => {
            result(&id, json!({"leader_token": LEADER_TOKEN, "base_mode": OPENCODE_BASE_MODE}));
        }
        "006-keymap-chord-timeout" => {
            let o = KeymapOptions::default();
            result(&id, json!({"leader_default": o.leader}));
        }
        "007-theme-presets" => {
            let mut names = Theme::available_themes().to_vec();
            names.sort_unstable();
            result(&id, json!(names));
        }
        "008-theme-preset-data" => {
            result(
                &id,
                json!({
                    "opencode": theme_preset_raw("opencode"),
                    "dracula": theme_preset_raw("dracula"),
                    "nord": theme_preset_raw("nord"),
                }),
            );
        }
        "009-theme-resolve" => {
            let fields: [(&str, fn(&Theme) -> ratatui::style::Color); 8] = [
                ("primary", |t| t.primary),
                ("secondary", |t| t.secondary),
                ("accent", |t| t.accent),
                ("error", |t| t.error),
                ("warning", |t| t.warning),
                ("success", |t| t.success),
                ("text", |t| t.text),
                ("background", |t| t.background),
            ];
            let rgb_of = |c: ratatui::style::Color| match c {
                ratatui::style::Color::Rgb(r, g, b) => json!({"r": r, "g": g, "b": b}),
                _ => Value::Null,
            };
            let mut out = serde_json::Map::new();
            for name in Theme::available_themes() {
                let mut modes = serde_json::Map::new();
                for (mode_name, mode) in [("dark", Mode::Dark), ("light", Mode::Light)] {
                    let t = Theme::by_name(name, mode);
                    let mut anchors = serde_json::Map::new();
                    for (field, get) in fields {
                        anchors.insert(field.to_string(), rgb_of(get(&t)));
                    }
                    modes.insert(mode_name.to_string(), Value::Object(anchors));
                }
                out.insert(name.to_string(), Value::Object(modes));
            }
            result(&id, Value::Object(out));
        }
        "010-format-duration" => {
            let cases = [0, 1, 45, 59, 60, 61, 3599, 3600, 86399, 86400, 604799, 604800, 1209600];
            result(&id, json!(cases.map(format_duration)));
        }
        "011-format-collapse" => {
            let short = collapse_tool_output("hello\nworld", 10, 100);
            let long_lines: String = (1..=20u32).map(|i| format!("line {i}\n")).collect();
            let long = collapse_tool_output(&long_lines, 5, 80);
            let wide = collapse_tool_output("abcdefghij", 10, 5);
            result(
                &id,
                json!({
                    "short": {"output": short.output, "overflow": short.overflow},
                    "long": {"output": long.output, "overflow": long.overflow},
                    "wide": {"output": wide.output, "overflow": wide.overflow},
                }),
            );
        }
        "012-clipboard-lookup" => {
            result(
                &id,
                json!([
                    clipboard_matrix("darwin", false, &["osascript"]),
                    clipboard_matrix("linux", true, &["wl-copy"]),
                    clipboard_matrix("linux", false, &["xclip"]),
                    clipboard_matrix("linux", false, &["xsel"]),
                    clipboard_matrix("win32", false, &["powershell.exe"]),
                    clipboard_matrix("linux", false, &[]),
                ]),
            );
        }
        "013-editor-normalize" => {
            let cases = ["hello\n", "hello\r\n", "a\nb\n", "a\nb", ""];
            result(&id, json!(cases.map(oc_tui::editor::normalize_prompt_content)));
        }
        "014-patch-metadata" => {
            let payload = json!([{
                "filePath": "/tmp/a.ts",
                "relativePath": "a.ts",
                "type": "update",
                "patch": "Index: a.ts\n--- a.ts\n+++ a.ts\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n",
                "additions": 1,
                "deletions": 1
            }]);
            let parsed = parse_apply_patch_files(&payload);
            result(
                &id,
                json!(parsed.iter().map(|p| json!({
                    "relativePath": p.relative_path,
                    "additions": p.additions,
                    "deletions": p.deletions,
                })).collect::<Vec<_>>()),
            );
        }
        "015-locale-duration" => {
            let cases = [0i64, 5000, 65_000, 3_723_000, 86_400_000];
            result(&id, json!(cases.map(locale::duration)));
        }
        "016-prompt-interaction" => {
            let persisted = PersistedState {
                prompt: vec![PromptPart::Text { content: "fix the bug".into() }],
                ..Default::default()
            };
            let empty = PersistedState::default();
            result(
                &id,
                json!([
                    interaction_case(&Event::ModeShell, &empty),
                    interaction_case(&Event::ModeNormal, &empty),
                    interaction_case(&Event::DragEnter, &empty),
                    interaction_case(&Event::FocusEditor, &empty),
                    interaction_case(&Event::InputChanged { value: "!".into(), persist: None }, &empty),
                    interaction_case(&Event::InputChanged { value: "fix @par".into(), persist: None }, &empty),
                    interaction_case(&Event::InputChanged { value: "/fix".into(), persist: None }, &empty),
                    interaction_case(&Event::CommandsOpen, &persisted),
                    interaction_case(&Event::CommandsOpen, &empty),
                    interaction_case(&Event::PopoverQuery { value: "re".into() }, &empty),
                ]),
            );
        }
        "017-logo" => {
            result(&id, json!(LOGO.left.to_vec()));
        }
        "018-locale-titlecase" => {
            let cases = ["patch metadata hunk", "hello WORLD", "MiXeD case Words"];
            result(&id, json!(cases.map(locale::titlecase)));
        }
        "019-locale-truncate" => {
            result(
                &id,
                json!([
                    locale::truncate("a very long line of text", 10),
                    locale::truncate("short", 10),
                    locale::truncate_middle("abcdefghijklmnop", 8),
                ]),
            );
        }
        "020-clipboard-wayland" => result(&id, clipboard_matrix("linux", true, &["wl-copy"])),
        "021-clipboard-macos" => result(&id, clipboard_matrix("darwin", false, &["osascript"])),
        "022-clipboard-x11" => {
            let xclip = clipboard_matrix("linux", false, &["xclip"]);
            let xsel = clipboard_matrix("linux", false, &["xsel"]);
            result(&id, json!({"xclip": xclip, "xsel": xsel}));
        }
        "023-clipboard-none" => result(&id, clipboard_matrix("linux", false, &[])),
        "024-patch-metadata-multi" => {
            let payload = json!([
                {"filePath": "/x/a.rs", "relativePath": "a.rs", "type": "update",
                 "patch": "@@ -1 +1 @@\n-a\n+b\n", "additions": 1, "deletions": 1},
                {"filePath": "/x/c.md", "relativePath": "c.md", "type": "add",
                 "patch": "@@ -0,0 +1 @@\n+new\n", "additions": 1, "deletions": 0},
            ]);
            let parsed = parse_apply_patch_files(&payload);
            result(
                &id,
                json!(parsed.iter().map(|p| json!({
                    "relativePath": p.relative_path,
                    "additions": p.additions,
                    "deletions": p.deletions,
                })).collect::<Vec<_>>()),
            );
        }
        "025-theme-preset-data-2" => {
            result(
                &id,
                json!({
                    "catppuccin": theme_preset_raw("catppuccin"),
                    "gruvbox": theme_preset_raw("gruvbox"),
                    "tokyonight": theme_preset_raw("tokyonight"),
                }),
            );
        }
        "026-editor-multiline" => {
            let cases = ["first\nsecond\nthird\n", "single\n", "trailing\n\n\n"];
            result(&id, json!(cases.map(oc_tui::editor::normalize_prompt_content)));
        }
        _ => {
            eprintln!("unknown scenario: {id}");
            std::process::exit(2);
        }
    }
}
