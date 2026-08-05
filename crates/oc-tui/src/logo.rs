//! Home-screen logo.
//! From reference/packages/tui/src/logo.ts

/// The OpenCode logo rendered on the home screen.
/// From reference/packages/tui/src/logo.ts (`logo`)
pub const LOGO: Logo = Logo {
    left: [
        "                   ",
        "█▀▀█ █▀▀█ █▀▀█ █▀▀▄",
        "█__█ █__█ █^^^ █__█",
        "▀▀▀▀ █▀▀▀ ▀▀▀▀ ▀~~▀",
    ],
    right: [
        "             ▄     ",
        "█▀▀▀ █▀▀█ █▀▀█ █▀▀█",
        "█___ █__█ █__█ █^^^",
        "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀",
    ],
};

#[derive(Debug, Clone, Copy)]
pub struct Logo {
    pub left: [&'static str; 4],
    pub right: [&'static str; 4],
}

impl Logo {
    pub fn lines(&self) -> Vec<String> {
        self.left
            .iter()
            .zip(self.right.iter())
            .map(|(l, r)| format!("{l} {r}"))
            .collect()
    }

    pub fn height(&self) -> usize {
        self.left.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_has_four_lines() {
        assert_eq!(LOGO.lines().len(), 4);
        assert_eq!(LOGO.height(), 4);
    }

    #[test]
    fn logo_lines_match_reference() {
        let lines = LOGO.lines();
        assert_eq!(lines[1], "█▀▀█ █▀▀█ █▀▀█ █▀▀▄ █▀▀▀ █▀▀█ █▀▀█ █▀▀█");
    }
}
