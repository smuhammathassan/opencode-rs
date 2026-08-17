#![allow(clippy::const_is_empty)]
//! Property-based invariants, Unicode width, and layout boundary tests.
//!
//! Tests unicode grapheme clusters, full-width CJK ideographs, emoji ZWJ
//! sequences, Arabic/Urdu RTL strings, combining diacritics, and prompt editor
//! buffer invariants across extreme boundary conditions.
use oc_tui::components::text::{plain, width};
use oc_tui::prompt::state::PromptState;
use oc_tui::theme::Theme;
use unicode_width::UnicodeWidthStr;

#[test]
fn unicode_cjk_width_invariants() {
    let sample = "你好世界";
    assert_eq!(sample.chars().count(), 4);
    assert_eq!(UnicodeWidthStr::width(sample), 8);
    let line = plain(sample);
    assert_eq!(width(&line), 8);
}

#[test]
fn emoji_zwj_width_invariants() {
    let simple_emoji = "🚀";
    assert_eq!(UnicodeWidthStr::width(simple_emoji), 2);

    let skin_tone_emoji = "👋🏽";
    assert!(UnicodeWidthStr::width(skin_tone_emoji) >= 2);

    let family_emoji = "👨‍👩‍👧‍👦";
    assert!(UnicodeWidthStr::width(family_emoji) >= 2);
}

#[test]
fn rtl_arabic_urdu_width_invariants() {
    let arabic = "مرحبا بالعالم";
    assert!(!arabic.is_empty());
    assert_eq!(width(&plain(arabic)), UnicodeWidthStr::width(arabic));

    let urdu = "اوپن کوڈ";
    assert!(!urdu.is_empty());
    assert_eq!(width(&plain(urdu)), UnicodeWidthStr::width(urdu));
}

#[test]
fn combining_characters_and_accents() {
    let combining = "e\u{0301}"; // e + combining acute accent = é
    assert_eq!(UnicodeWidthStr::width(combining), 1);
    assert_eq!(width(&plain(combining)), 1);

    let german = "Grüße";
    assert_eq!(UnicodeWidthStr::width(german), 5);
}

#[test]
fn prompt_editor_buffer_invariants_never_panic() {
    let mut state = PromptState::default();

    // Insertion of mixed Unicode, CJK, Emoji, Arabic
    state.buffer.insert_str("Hello 你好 👋🏽 مرحبا");
    assert!(state.buffer.cursor() <= state.buffer.len());

    // Backspace from end
    while !state.buffer.is_empty() {
        state.buffer.backspace();
    }
    assert_eq!(state.buffer.text(), "");
    assert_eq!(state.buffer.cursor(), 0);
}

#[test]
fn theme_all_presets_exist() {
    let dark = Theme::dark();
    assert_eq!(dark.name, "opencode");
    assert_eq!(dark.mode, oc_tui::theme::Mode::Dark);
    let light = Theme::light();
    assert_eq!(light.name, "opencode");
    assert_eq!(light.mode, oc_tui::theme::Mode::Light);
}
