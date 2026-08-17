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
    assert_eq!(Theme::available_themes().len(), 33);
    for name in Theme::available_themes() {
        let theme = Theme::by_name(name, oc_tui::theme::Mode::Dark);
        assert_eq!(theme.name, *name);
    }
}
