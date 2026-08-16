//! Theme palette.
//!
//! Port of the default `opencode` theme (`reference/packages/tui/src/theme/
//! assets/opencode.json`). Theme selection is currently limited to the
//! built-in palette; the resolved mode is still honored.

use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
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
    pub fn dark() -> Self {
        Self::default()
    }

    pub fn light() -> Self {
        let mut theme = Self::dark();
        theme.mode = Mode::Light;
        theme
    }

    /// Build the supported theme from the resolved TUI settings.
    ///
    /// The reference accepts named theme assets. This port currently ships
    /// only the built-in `opencode` palette, so the resolved name falls back
    /// to it while preserving the configured light/dark mode.
    pub fn from_config(config: &crate::config::ResolvedConfig) -> Self {
        let mut theme = Self::dark();
        theme.mode = config.theme_mode;
        theme
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
