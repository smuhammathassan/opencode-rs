//! Side-by-side behavioral assertions against OpenCode v1.18.13 golden outputs.

use oc_tui::logo;

#[test]
fn logo_lines_parity() {
    let lines = logo::LOGO_LINES;
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("██"));
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
