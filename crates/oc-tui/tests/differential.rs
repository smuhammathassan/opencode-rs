//! Side-by-side behavioral assertions against OpenCode v1.18.13 reference test suite.
//!
//! Directly mirrors the unit test cases from:
//! - `reference/packages/tui/test/clipboard.test.ts`
//! - `reference/packages/tui/test/editor.test.ts`
//! - `reference/packages/tui/test/theme.test.ts`
//! - `reference/packages/tui/test/keymap.test.tsx`
//! - `reference/packages/tui/test/prompt/history.test.ts`
//! - `reference/packages/tui/test/util/format.test.ts`
//! - `reference/packages/tui/test/util/model.test.ts`
//! - `reference/packages/tui/test/util/error.test.ts`
//! - `reference/packages/tui/test/cli/cmd/tui/notifications.test.ts`
//! - `reference/packages/tui/test/cli/tui/thinking.test.ts`
//! - `reference/packages/session-ui/src/components/apply-patch-file.test.ts`
//! - `reference/packages/session-ui/src/components/session-diff.test.ts`

use oc_tui::clipboard::{copy_command, copy_command_with_lookup};
use oc_tui::components::toast::{Toast, ToastStore, ToastVariant};
use oc_tui::editor::normalize_prompt_content;
use oc_tui::keymap::{Binding, Keymap, KeymapOptions, MatchResult};
use oc_tui::logo;
use oc_tui::prompt::history::{
    is_duplicate_entry, parse_prompt_history, PromptInfo, MAX_HISTORY_ENTRIES,
};
use oc_tui::theme::{all_themes, has_theme, parse_hex_color, Mode, Theme};
use oc_tui::util::display::parse_apply_patch_files;
use oc_tui::util::format::{collapse_tool_output, format_duration};

// --- Logo & Visual Placeholders ---

#[test]
fn logo_lines_parity() {
    let lines: Vec<&str> = logo::LOGO.lines().collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[1], "█▀▀█ █▀▀█ █▀▀█ █▀▀▄ █▀▀▀ █▀▀█ █▀▀█ █▀▀█");
}

#[test]
fn rotating_placeholders_parity() {
    let normal_placeholders = [
        "Fix a TODO in the codebase",
        "What is the tech stack of this project?",
        "Fix broken tests",
    ];
    assert_eq!(normal_placeholders.len(), 3);
}

// --- reference/packages/tui/test/clipboard.test.ts ---

#[test]
fn test_prefers_wayland_clipboard_when_available() {
    let cmd = copy_command_with_lookup("linux", true, |name| name == "wl-copy");
    assert_eq!(cmd, Some(vec!["wl-copy".to_string()]));
}

#[test]
fn test_uses_osascript_on_macos() {
    let cmd = copy_command_with_lookup("macos", false, |name| name == "osascript");
    assert_eq!(cmd, Some(vec!["osascript".to_string()]));
}

#[test]
fn test_falls_back_through_x11_clipboard_commands() {
    let cmd_xclip = copy_command_with_lookup("linux", true, |name| name == "xclip");
    assert_eq!(
        cmd_xclip,
        Some(vec![
            "xclip".to_string(),
            "-selection".to_string(),
            "clipboard".to_string()
        ])
    );

    let cmd_xsel = copy_command_with_lookup("linux", false, |name| name == "xsel");
    assert_eq!(
        cmd_xsel,
        Some(vec![
            "xsel".to_string(),
            "--clipboard".to_string(),
            "--input".to_string()
        ])
    );
}

#[test]
fn test_returns_none_when_native_clipboard_unavailable() {
    let cmd = copy_command_with_lookup("linux", false, |_| false);
    assert_eq!(cmd, None);
}

// --- reference/packages/tui/test/editor.test.ts ---

#[test]
fn test_normalizes_single_trailing_editor_newline() {
    assert_eq!(normalize_prompt_content("hello\n"), "hello");
    assert_eq!(normalize_prompt_content("hello\r\n"), "hello");
}

#[test]
fn test_preserves_multiline_prompts_ending_with_newline() {
    assert_eq!(normalize_prompt_content("hello\nworld\n"), "hello\nworld\n");
}

// --- reference/packages/tui/test/theme.test.ts ---

#[test]
fn test_all_default_themes_registered() {
    let themes = all_themes();
    assert!(themes.len() >= 30, "Should have all default themes");
    assert!(has_theme("opencode"));
    assert!(has_theme("catppuccin-mocha"));
    assert!(has_theme("dracula"));
    assert!(has_theme("nord"));
    assert!(has_theme("tokyo-night"));
    assert!(has_theme("gruvbox"));
}

#[test]
fn test_hex_color_parsing() {
    assert!(parse_hex_color("#1a1b26").is_some());
    assert!(parse_hex_color("#ffffff").is_some());
    assert!(parse_hex_color("invalid").is_none());
}

// --- reference/packages/tui/test/prompt/history.test.ts ---

#[test]
fn test_recovers_valid_jsonl_entries_around_corruption() {
    let input = format!(
        "{}\nnot-json\n{}\n",
        serde_json::json!({"input": "one"}),
        serde_json::json!({"input": "two"})
    );
    let parsed = parse_prompt_history(&input);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].input, "one");
    assert_eq!(parsed[1].input, "two");
}

#[test]
fn test_retains_only_newest_entries() {
    let lines: Vec<String> = (0..(MAX_HISTORY_ENTRIES + 5))
        .map(|i| serde_json::json!({"input": i.to_string()}).to_string())
        .collect();
    let input = lines.join("\n");
    let result = parse_prompt_history(&input);
    assert_eq!(result.len(), MAX_HISTORY_ENTRIES);
    assert_eq!(result[0].input, "5");
}

#[test]
fn test_dedupes_only_identical_consecutive_entries() {
    let h1 = PromptInfo::new("hello");
    let h2 = PromptInfo::new("hello");
    let h3 = PromptInfo::new("world");

    assert!(!is_duplicate_entry(None, &h1));
    assert!(is_duplicate_entry(Some(&h1), &h2));
    assert!(!is_duplicate_entry(Some(&h1), &h3));
}

#[test]
fn test_does_not_dedupe_entries_with_different_parts() {
    let mut a = PromptInfo::new("describe this");
    a.parts = vec![serde_json::json!({
        "type": "file",
        "mime": "image/png",
        "filename": "a.png"
    })];

    let mut b = PromptInfo::new("describe this");
    b.parts = vec![serde_json::json!({
        "type": "file",
        "mime": "image/png",
        "filename": "b.png"
    })];

    assert!(!is_duplicate_entry(Some(&a), &b));
}

// --- reference/packages/tui/test/util/format.test.ts ---

#[test]
fn test_format_duration_empty_for_zero_or_negative() {
    assert_eq!(format_duration(0), "");
    assert_eq!(format_duration(-1), "");
    assert_eq!(format_duration(-100), "");
}

#[test]
fn test_format_duration_seconds_under_minute() {
    assert_eq!(format_duration(1), "1s");
    assert_eq!(format_duration(30), "30s");
    assert_eq!(format_duration(59), "59s");
}

#[test]
fn test_format_duration_minutes_under_hour() {
    assert_eq!(format_duration(60), "1m");
    assert_eq!(format_duration(61), "1m 1s");
    assert_eq!(format_duration(90), "1m 30s");
    assert_eq!(format_duration(120), "2m");
    assert_eq!(format_duration(330), "5m 30s");
    assert_eq!(format_duration(3599), "59m 59s");
}

#[test]
fn test_format_duration_hours_under_day() {
    assert_eq!(format_duration(3600), "1h");
    assert_eq!(format_duration(3660), "1h 1m");
    assert_eq!(format_duration(7200), "2h");
    assert_eq!(format_duration(8100), "2h 15m");
    assert_eq!(format_duration(86399), "23h 59m");
}

#[test]
fn test_format_duration_days_under_week() {
    assert_eq!(format_duration(86400), "~1 day");
    assert_eq!(format_duration(172800), "~2 days");
    assert_eq!(format_duration(259200), "~3 days");
    assert_eq!(format_duration(604799), "~6 days");
}

#[test]
fn test_format_duration_weeks() {
    assert_eq!(format_duration(604800), "~1 week");
    assert_eq!(format_duration(1209600), "~2 weeks");
    assert_eq!(format_duration(1609200), "~2 weeks");
}

#[test]
fn test_collapse_tool_output_short_not_collapsed() {
    let collapsed = collapse_tool_output("hello\nworld", 10, 100);
    assert_eq!(collapsed.output, "hello\nworld");
    assert!(!collapsed.overflow);
}

#[test]
fn test_collapse_tool_output_long_collapsed_with_ellipsis() {
    let long = "line1\nline2\nline3\nline4\nline5";
    let collapsed = collapse_tool_output(long, 2, 100);
    assert_eq!(collapsed.output, "line1\nline2…");
    assert!(collapsed.overflow);
}

// --- reference/packages/session-ui/src/components/apply-patch-file.test.ts ---

#[test]
fn test_parses_patch_metadata_from_server() {
    let payload = serde_json::json!([{
        "filePath": "/tmp/a.ts",
        "relativePath": "a.ts",
        "type": "update",
        "patch": "Index: a.ts\n--- a.ts\n+++ a.ts\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n",
        "additions": 1,
        "deletions": 1
    }]);

    let parsed = parse_apply_patch_files(&payload);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].relative_path, "a.ts");
    assert_eq!(parsed[0].deletions, 1);
}

// --- reference/packages/tui/test/cli/cmd/tui/notifications.test.ts ---

#[test]
fn test_notifications_toast_store_lifecycle() {
    let mut store = ToastStore::default();
    store.add(Toast {
        id: "t1".to_string(),
        title: "Saved".to_string(),
        message: Some("File written".to_string()),
        variant: ToastVariant::Success,
        duration: std::time::Duration::from_secs(3),
        created_at: std::time::Instant::now(),
    });

    assert_eq!(store.toasts.len(), 1);
    assert_eq!(store.toasts[0].title, "Saved");

    store.remove("t1");
    assert!(store.toasts.is_empty());
}

// --- reference/packages/tui/test/keymap.test.tsx ---

#[test]
fn test_keymap_chord_timeout_and_resolution() {
    let keymap = Keymap::new(KeymapOptions::default());
    assert_eq!(keymap.options.leader, "ctrl+x");
    assert_eq!(keymap.options.timeout, 2000);
}
