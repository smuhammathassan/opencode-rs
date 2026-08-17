//! Theme palette.
//!
//! Port of the default `opencode` theme (`reference/packages/tui/src/theme/
//! assets/opencode.json`). Theme selection is currently limited to the
//! built-in palette; the resolved mode is still honored.

use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub mode: Mode,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub text: Color,
    pub text_muted: Color,
    pub background: Color,
    pub background_panel: Color,
    pub background_element: Color,
    pub background_menu: Color,
    pub border: Color,
    pub border_active: Color,
    pub border_subtle: Color,
    pub diff_added: Color,
    pub diff_removed: Color,
    pub diff_context: Color,
    pub diff_highlight_added: Color,
    pub diff_highlight_removed: Color,
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_context_bg: Color,
    pub diff_line_number: Color,
    pub diff_added_line_number_bg: Color,
    pub diff_removed_line_number_bg: Color,
    pub markdown_text: Color,
    pub markdown_heading: Color,
    pub markdown_code: Color,
    pub thinking_opacity: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl Theme {
    pub const ALL_THEMES: &'static [&'static str] = &[
        "opencode",
        "tokyonight",
        "dracula",
        "nord",
        "catppuccin",
        "catppuccin-frappe",
        "catppuccin-macchiato",
        "gruvbox",
        "one-dark",
        "solarized",
        "github",
        "monokai",
        "material",
        "matrix",
        "rosepine",
        "nightowl",
        "everforest",
        "aura",
        "ayu",
        "carbonfox",
        "cobalt2",
        "cursor",
        "flexoki",
        "kanagawa",
        "lucent-orng",
        "mercury",
        "orng",
        "osaka-jade",
        "palenight",
        "synthwave84",
        "vercel",
        "vesper",
        "zenburn",
    ];

    pub fn available_themes() -> &'static [&'static str] {
        Self::ALL_THEMES
    }

    pub fn dark() -> Self {
        Self::by_name("opencode", Mode::Dark)
    }

    pub fn light() -> Self {
        Self::by_name("opencode", Mode::Light)
    }

    pub fn by_name(name: &str, mode: Mode) -> Self {
        let mut t = Self::default();
        t.name = name.to_string();
        t.mode = mode;

        match (name, mode) {
            ("tokyonight", Mode::Dark) => {
                t.primary = from_hex("#82aaff");
                t.secondary = from_hex("#c099ff");
                t.accent = from_hex("#ff966c");
                t.error = from_hex("#ff757f");
                t.warning = from_hex("#ff966c");
                t.success = from_hex("#c3e88d");
                t.info = from_hex("#82aaff");
                t.text = from_hex("#c8d3f5");
                t.text_muted = from_hex("#828bb8");
                t.background = from_hex("#1a1b26");
                t.background_panel = from_hex("#1e2030");
                t.background_element = from_hex("#222436");
                t.border = from_hex("#737aa2");
                t.border_active = from_hex("#9099b2");
                t.border_subtle = from_hex("#545c7e");
            }
            ("dracula", Mode::Dark) => {
                t.primary = from_hex("#bd93f9");
                t.secondary = from_hex("#8be9fd");
                t.accent = from_hex("#ff79c6");
                t.error = from_hex("#ff5555");
                t.warning = from_hex("#ffb86c");
                t.success = from_hex("#50fa7b");
                t.info = from_hex("#8be9fd");
                t.text = from_hex("#f8f8f2");
                t.text_muted = from_hex("#6272a4");
                t.background = from_hex("#282a36");
                t.background_panel = from_hex("#21222c");
                t.background_element = from_hex("#343746");
                t.border = from_hex("#6272a4");
                t.border_active = from_hex("#bd93f9");
                t.border_subtle = from_hex("#44475a");
            }
            ("nord", Mode::Dark) => {
                t.primary = from_hex("#88c0d0");
                t.secondary = from_hex("#81a1c1");
                t.accent = from_hex("#b48ead");
                t.error = from_hex("#bf616a");
                t.warning = from_hex("#ebcb8b");
                t.success = from_hex("#a3be8c");
                t.info = from_hex("#8fbcbb");
                t.text = from_hex("#eceff4");
                t.text_muted = from_hex("#4c566a");
                t.background = from_hex("#2e3440");
                t.background_panel = from_hex("#242933");
                t.background_element = from_hex("#3b4252");
                t.border = from_hex("#434c5e");
                t.border_active = from_hex("#88c0d0");
                t.border_subtle = from_hex("#3b4252");
            }
            ("catppuccin" | "catppuccin-macchiato", Mode::Dark) => {
                t.primary = from_hex("#cba6f7");
                t.secondary = from_hex("#89b4fa");
                t.accent = from_hex("#f38ba8");
                t.error = from_hex("#f38ba8");
                t.warning = from_hex("#fab387");
                t.success = from_hex("#a6e3a1");
                t.info = from_hex("#89dceb");
                t.text = from_hex("#cdd6f4");
                t.text_muted = from_hex("#6c7086");
                t.background = from_hex("#1e1e2e");
                t.background_panel = from_hex("#181825");
                t.background_element = from_hex("#313244");
                t.border = from_hex("#585b70");
                t.border_active = from_hex("#cba6f7");
                t.border_subtle = from_hex("#45475a");
            }
            ("gruvbox", Mode::Dark) => {
                t.primary = from_hex("#fe8019");
                t.secondary = from_hex("#83a598");
                t.accent = from_hex("#fabd2f");
                t.error = from_hex("#fb4934");
                t.warning = from_hex("#fe8019");
                t.success = from_hex("#b8bb26");
                t.info = from_hex("#8ec07c");
                t.text = from_hex("#ebdbb2");
                t.text_muted = from_hex("#928374");
                t.background = from_hex("#282828");
                t.background_panel = from_hex("#1d2021");
                t.background_element = from_hex("#3c3836");
                t.border = from_hex("#504945");
                t.border_active = from_hex("#fe8019");
                t.border_subtle = from_hex("#3c3836");
            }
            ("one-dark", Mode::Dark) => {
                t.primary = from_hex("#61afef");
                t.secondary = from_hex("#c678dd");
                t.accent = from_hex("#e5c07b");
                t.error = from_hex("#e06c75");
                t.warning = from_hex("#d19a66");
                t.success = from_hex("#98c379");
                t.info = from_hex("#56b6c2");
                t.text = from_hex("#abb2bf");
                t.text_muted = from_hex("#5c6370");
                t.background = from_hex("#282c34");
                t.background_panel = from_hex("#21252b");
                t.background_element = from_hex("#2c313a");
                t.border = from_hex("#4b5263");
                t.border_active = from_hex("#61afef");
                t.border_subtle = from_hex("#3e4451");
            }
            ("github", Mode::Dark) => {
                t.primary = from_hex("#58a6ff");
                t.secondary = from_hex("#bc8cff");
                t.accent = from_hex("#d29922");
                t.error = from_hex("#f85149");
                t.warning = from_hex("#d29922");
                t.success = from_hex("#3fb950");
                t.info = from_hex("#58a6ff");
                t.text = from_hex("#c9d1d9");
                t.text_muted = from_hex("#8b949e");
                t.background = from_hex("#0d1117");
                t.background_panel = from_hex("#161b22");
                t.background_element = from_hex("#21262d");
                t.border = from_hex("#30363d");
                t.border_active = from_hex("#58a6ff");
                t.border_subtle = from_hex("#21262d");
            }
            ("matrix", Mode::Dark) => {
                t.primary = from_hex("#00ff41");
                t.secondary = from_hex("#008f11");
                t.accent = from_hex("#00ff41");
                t.error = from_hex("#ff0033");
                t.warning = from_hex("#00ff41");
                t.success = from_hex("#00ff41");
                t.info = from_hex("#008f11");
                t.text = from_hex("#00ff41");
                t.text_muted = from_hex("#008f11");
                t.background = from_hex("#0d0208");
                t.background_panel = from_hex("#001100");
                t.background_element = from_hex("#002200");
                t.border = from_hex("#008f11");
                t.border_active = from_hex("#00ff41");
                t.border_subtle = from_hex("#003b00");
            }
            ("vesper", Mode::Dark) => {
                t.primary = from_hex("#ffc799");
                t.secondary = from_hex("#99ffe4");
                t.accent = from_hex("#ff99aa");
                t.error = from_hex("#ff8080");
                t.warning = from_hex("#ffc799");
                t.success = from_hex("#99ffe4");
                t.info = from_hex("#acd1f0");
                t.text = from_hex("#ffffff");
                t.text_muted = from_hex("#505050");
                t.background = from_hex("#101010");
                t.background_panel = from_hex("#161616");
                t.background_element = from_hex("#232323");
                t.border = from_hex("#282828");
                t.border_active = from_hex("#ffc799");
                t.border_subtle = from_hex("#1c1c1c");
            }
            _ if mode == Mode::Light => {
                t.text = from_hex("#202020");
                t.text_muted = from_hex("#707070");
                t.background = from_hex("#f8f9fa");
                t.background_panel = from_hex("#ffffff");
                t.background_element = from_hex("#e9ecef");
                t.background_menu = from_hex("#e9ecef");
                t.border = from_hex("#ced4da");
                t.border_active = from_hex("#adb5bd");
                t.border_subtle = from_hex("#dee2e6");
                t.diff_added_bg = from_hex("#d4edda");
                t.diff_removed_bg = from_hex("#f8d7da");
                t.diff_context_bg = from_hex("#ffffff");
                t.markdown_text = from_hex("#202020");
            }
            _ => {}
        }
        t
    }

    /// Build the supported theme from the resolved TUI settings.
    pub fn from_config(config: &crate::config::ResolvedConfig) -> Self {
        Self::by_name("opencode", config.theme_mode)
    }

    /// Default agent color palette.
    /// From reference/packages/tui/src/context/local.tsx (`createAgent.colors`)
    pub fn agent_colors(&self) -> Vec<Color> {
        vec![
            self.secondary,
            self.accent,
            self.success,
            self.warning,
            self.primary,
            self.error,
            self.info,
        ]
    }

    /// Color for a named agent.
    /// From reference/packages/tui/src/context/local.tsx (`agent.color`)
    pub fn agent_color(&self, name: &str, agents: &[crate::types::Agent]) -> Color {
        let palette = self.agent_colors();
        let index = agents.iter().position(|a| a.name == name);
        match index {
            None => palette[0],
            Some(index) => {
                let agent = &agents[index];
                if let Some(color) = &agent.color {
                    if let Some(hex) = parse_hex(color) {
                        return hex;
                    }
                    if let Some(theme_color) = self.named(color) {
                        return theme_color;
                    }
                }
                palette[index % palette.len()]
            }
        }
    }

    pub fn named(&self, name: &str) -> Option<Color> {
        Some(match name {
            "primary" => self.primary,
            "secondary" => self.secondary,
            "accent" => self.accent,
            "error" => self.error,
            "warning" => self.warning,
            "success" => self.success,
            "info" => self.info,
            "text" => self.text,
            "textMuted" => self.text_muted,
            "border" => self.border,
            "borderActive" => self.border_active,
            "borderSubtle" => self.border_subtle,
            _ => return None,
        })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            name: "opencode".to_string(),
            mode: Mode::Dark,
            primary: from_hex("#fab283"),
            secondary: from_hex("#5c9cf5"),
            accent: from_hex("#9d7cd8"),
            error: from_hex("#e06c75"),
            warning: from_hex("#f5a742"),
            success: from_hex("#7fd88f"),
            info: from_hex("#56b6c2"),
            text: from_hex("#eeeeee"),
            text_muted: from_hex("#808080"),
            background: from_hex("#0a0a0a"),
            background_panel: from_hex("#141414"),
            background_element: from_hex("#1e1e1e"),
            background_menu: from_hex("#1e1e1e"),
            border: from_hex("#484848"),
            border_active: from_hex("#606060"),
            border_subtle: from_hex("#3c3c3c"),
            diff_added: from_hex("#4fd6be"),
            diff_removed: from_hex("#c53b53"),
            diff_context: from_hex("#828bb8"),
            diff_highlight_added: from_hex("#b8db87"),
            diff_highlight_removed: from_hex("#e26a75"),
            diff_added_bg: from_hex("#20303b"),
            diff_removed_bg: from_hex("#37222c"),
            diff_context_bg: from_hex("#141414"),
            diff_line_number: from_hex("#8f8f8f"),
            diff_added_line_number_bg: from_hex("#1b2b34"),
            diff_removed_line_number_bg: from_hex("#2d1f26"),
            markdown_text: from_hex("#eeeeee"),
            markdown_heading: from_hex("#9d7cd8"),
            markdown_code: from_hex("#7fd88f"),
            thinking_opacity: 0x66,
        }
    }
}

pub fn from_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let parse = |start: usize| u8::from_str_radix(&hex[start..start + 2], 16).unwrap_or(0);
    if hex.len() == 6 {
        Color::Rgb(parse(0), parse(2), parse(4))
    } else {
        Color::White
    }
}

pub fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let parse = |start: usize| u8::from_str_radix(&hex[start..start + 2], 16).ok();
    Some(Color::Rgb(parse(0)?, parse(2)?, parse(4)?))
}

/// Foreground color for text on a highlighted (colored) background.
/// From reference/packages/tui/src/context/theme/index.tsx (`selectedForeground`)
pub fn selected_foreground(background: Color) -> Color {
    match background {
        Color::Rgb(r, g, b) => {
            // Luminance heuristic: dark backgrounds get light text.
            let lum = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
            if lum > 140.0 {
                Color::Black
            } else {
                Color::White
            }
        }
        _ => Color::White,
    }
}

/// Blend two colors toward `amount` (0..=1). Mirrors `tint`.
/// From reference/packages/tui/src/context/theme/index.tsx (`tint`)
pub fn tint(from: Color, to: Color, amount: f64) -> Color {
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let blend = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * amount) as u8;
            Color::Rgb(blend(r1, r2), blend(g1, g2), blend(b1, b2))
        }
        (from, _) => from,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parse() {
        assert_eq!(from_hex("#0a0a0a"), Color::Rgb(10, 10, 10));
        assert_eq!(parse_hex("fab283"), Some(Color::Rgb(250, 178, 131)));
        assert_eq!(parse_hex("zzzzzz"), None);
    }

    #[test]
    fn selected_foreground_contrast() {
        assert_eq!(selected_foreground(Color::Rgb(10, 10, 10)), Color::White);
        assert_eq!(selected_foreground(Color::Rgb(255, 255, 255)), Color::Black);
    }

    #[test]
    fn tint_blends() {
        assert_eq!(
            tint(Color::Rgb(0, 0, 0), Color::Rgb(100, 0, 0), 0.5),
            Color::Rgb(50, 0, 0)
        );
    }

    #[test]
    fn agent_color_uses_palette() {
        let theme = Theme::dark();
        let agents = vec![];
        assert_eq!(
            theme.agent_color("unknown", &agents),
            theme.agent_colors()[0]
        );
    }

    #[test]
    fn from_config_honors_resolved_light_mode() {
        let config = crate::config::resolve(
            &serde_json::json!({ "theme": "opencode", "theme_mode": "light" }),
            crate::config::ResolveOptions::default(),
        );

        assert_eq!(Theme::from_config(&config).mode, Mode::Light);
    }

    #[test]
    fn from_config_falls_back_to_supported_palette() {
        let config = crate::config::resolve(
            &serde_json::json!({ "theme": "not-installed", "theme_mode": "dark" }),
            crate::config::ResolveOptions::default(),
        );
        let theme = Theme::from_config(&config);

        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(theme.primary, Theme::dark().primary);
    }
}
