//! Interactive stdin resolution.
//! From reference/packages/opencode/src/cli/cmd/run/runtime.stdin.ts.

pub const INTERACTIVE_INPUT_ERROR: &str = "--mini requires a controlling terminal for input";

/// Mirrors `resolveInteractiveStdin()`. When stdin is not a TTY, attempt to open
/// the controlling terminal directly; failure yields the interactive-input error.
pub fn resolve_interactive_stdin() -> std::io::Result<InteractiveStdin> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return Ok(InteractiveStdin::Stdin);
    }
    let file = if cfg!(windows) { "CONIN$" } else { "/dev/tty" };
    match std::fs::File::open(file) {
        Ok(file) => Ok(InteractiveStdin::Tty(file)),
        Err(_) => Err(std::io::Error::other(INTERACTIVE_INPUT_ERROR)),
    }
}

pub enum InteractiveStdin {
    Stdin,
    Tty(std::fs::File),
}

impl InteractiveStdin {
    pub fn cleanup(self) {
        if let InteractiveStdin::Tty(file) = self {
            drop(file);
        }
    }
}
