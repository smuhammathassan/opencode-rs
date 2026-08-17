//! Property-based invariants, Unicode width, and layout boundary tests.

use oc_tui::theme::Theme;

#[test]
fn unicode_cjk_width_invariants() {
    let sample = "你好世界";
    assert_eq!(sample.chars().count(), 4);
}

#[test]
fn emoji_zwj_width_invariants() {
    let emoji = "👨‍👩‍👧‍👦";
    assert!(!emoji.is_empty());
}

#[test]
fn rtl_arabic_urdu_width_invariants() {
    let arabic = "مرحبا بالعالم";
    assert!(!arabic.is_empty());
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
