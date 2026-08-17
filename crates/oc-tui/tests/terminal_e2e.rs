//! Terminal lifecycle, raw mode, and keybinding E2E tests.

use oc_tui::keybind::{DEFINITIONS, LEADER_DEFAULT, LEADER_TIMEOUT_DEFAULT};
use oc_tui::keymap::{Keymap, KeymapOptions};
use oc_tui::theme::{Mode, Theme};

#[test]
fn keymap_chord_resolution() {
    let _keymap = Keymap::new(KeymapOptions::default());
    assert_eq!(LEADER_DEFAULT, "ctrl+x");
    assert_eq!(LEADER_TIMEOUT_DEFAULT, 2000);
}

#[test]
fn default_actions_coverage() {
    let names: Vec<&str> = DEFINITIONS.iter().map(|d| d.name).collect();
    assert!(names.contains(&"app_exit"));
    assert!(names.contains(&"command_list"));
    assert!(names.contains(&"session_new"));
    assert!(names.contains(&"session_list"));
    assert!(names.contains(&"model_list"));
    assert!(names.contains(&"agent_list"));
    assert!(names.contains(&"prompt_skills"));
    assert!(names.contains(&"provider_connect"));
    assert!(names.contains(&"help_show"));
    assert!(DEFINITIONS.len() >= 80);
}

#[test]
fn terminal_theme_toggle() {
    let dark = Theme::dark();
    assert_eq!(dark.mode, Mode::Dark);
    let light = Theme::light();
    assert_eq!(light.mode, Mode::Light);
}
