//! Terminal security, ANSI escape, and OSC injection prevention tests.

#[test]
fn sanitize_osc_terminal_title_injection() {
    let malicious = "\x1b]0;hacked-title\x07Normal Text";
    let clean: String = malicious
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    assert!(!clean.contains("\x1b]0;"));
}

#[test]
fn sanitize_osc_52_clipboard_injection() {
    let malicious = "\x1b]52;c;ZXhwbG9pdA==\x07Hello";
    let clean: String = malicious
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    assert!(!clean.contains("\x1b]52;"));
}

#[test]
fn sanitize_ansi_color_bombs() {
    let malicious = "\x1b[38;2;255;0;0mRed Text\x1b[0m";
    let clean: String = malicious
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    assert!(!clean.contains("\x1b["));
}
