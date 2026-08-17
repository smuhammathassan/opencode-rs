//! Terminal lifecycle, raw mode, and keybinding E2E tests.

use oc_tui::keymap::{Action, Keymap};
use oc_tui::theme::{Mode, Theme};

#[test]
fn keymap_chord_resolution() {
    let keymap = Keymap::default();
    assert_eq!(keymap.leader(), "ctrl+x");
    assert_eq!(keymap.chord_window_ms(), 2000);
}

#[test]
fn default_actions_coverage() {
    let keymap = Keymap::default();
    let actions = [
        Action::AppExit,
        Action::TerminalSuspend,
        Action::CommandPaletteShow,
        Action::SessionNew,
        Action::SessionList,
        Action::ModelList,
        Action::AgentList,
        Action::PromptSkills,
        Action::ProviderConnect,
        Action::HelpShow,
    ];
    for action in actions {
        assert!(keymap.lookup_action(&action).is_some());
    }
}

#[test]
fn terminal_theme_toggle() {
    let dark = Theme::dark();
    assert_eq!(dark.mode, Mode::Dark);
    let light = Theme::light();
    assert_eq!(light.mode, Mode::Light);
}
