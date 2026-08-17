//! TUI performance, startup, and memory footprint benchmarks.

use oc_tui::keymap::Keymap;
use oc_tui::theme::Theme;
use std::time::Instant;

#[test]
fn cold_theme_and_keymap_init_time() {
    let start = Instant::now();
    let theme = Theme::dark();
    let keymap = Keymap::default();
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 50, "Init took {:?}", elapsed);
    assert_eq!(theme.name, "opencode");
    assert_eq!(keymap.leader(), "ctrl+x");
}
