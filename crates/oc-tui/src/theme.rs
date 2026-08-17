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
        if let Some(asset) = preset_asset(name) {
            resolve_preset_into(&mut t, asset, mode);
        } else if mode == Mode::Light {
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
    // Mirrors reference RGBA.fromHex arithmetic exactly:
    // parseInt(hex.replace('#',''), 16) then shifts — non-6-digit input (e.g.
    // "#FFF") is NOT expanded as CSS shorthand; it yields parseInt's value.
    let hex = hex.trim_start_matches('#');
    match u32::from_str_radix(hex, 16) {
        Ok(c) => Color::Rgb(
            ((c >> 16) & 255) as u8,
            ((c >> 8) & 255) as u8,
            (c & 255) as u8,
        ),
        Err(_) => Color::White,
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

pub fn has_theme(name: &str) -> bool {
    Theme::ALL_THEMES.contains(&name)
}

pub fn all_themes() -> &'static [&'static str] {
    Theme::ALL_THEMES
}

pub fn parse_hex_color(hex: &str) -> Option<Color> {
    parse_hex(hex)
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

/// Raw preset data parity: the vendored reference theme asset JSON
/// (`reference/packages/tui/src/theme/assets/<file>.json`) embedded at compile
/// time, so the Rust port consumes byte-identical preset definitions to the
/// reference implementation.
pub fn preset_raw_data(name: &str) -> Option<serde_json::Value> {
    let raw = match name {
        "opencode" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/opencode.json")
        }
        "dracula" => include_str!("../../../reference/packages/tui/src/theme/assets/dracula.json"),
        "nord" => include_str!("../../../reference/packages/tui/src/theme/assets/nord.json"),
        "catppuccin" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/catppuccin.json")
        }
        "gruvbox" => include_str!("../../../reference/packages/tui/src/theme/assets/gruvbox.json"),
        "tokyonight" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/tokyonight.json")
        }
        _ => return None,
    };
    serde_json::from_str(raw).ok()
}

/// Vendored preset assets (`reference/packages/tui/src/theme/assets/*.json`),
/// the single source of truth shared with the reference implementation.
fn preset_asset(name: &str) -> Option<&'static str> {
    Some(match name {
        "aura" => include_str!("../../../reference/packages/tui/src/theme/assets/aura.json"),
        "ayu" => include_str!("../../../reference/packages/tui/src/theme/assets/ayu.json"),
        "carbonfox" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/carbonfox.json")
        }
        "catppuccin-frappe" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/catppuccin-frappe.json")
        }
        "catppuccin-macchiato" => include_str!(
            "../../../reference/packages/tui/src/theme/assets/catppuccin-macchiato.json"
        ),
        "catppuccin" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/catppuccin.json")
        }
        "cobalt2" => include_str!("../../../reference/packages/tui/src/theme/assets/cobalt2.json"),
        "cursor" => include_str!("../../../reference/packages/tui/src/theme/assets/cursor.json"),
        "dracula" => include_str!("../../../reference/packages/tui/src/theme/assets/dracula.json"),
        "everforest" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/everforest.json")
        }
        "flexoki" => include_str!("../../../reference/packages/tui/src/theme/assets/flexoki.json"),
        "github" => include_str!("../../../reference/packages/tui/src/theme/assets/github.json"),
        "gruvbox" => include_str!("../../../reference/packages/tui/src/theme/assets/gruvbox.json"),
        "kanagawa" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/kanagawa.json")
        }
        "lucent-orng" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/lucent-orng.json")
        }
        "material" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/material.json")
        }
        "matrix" => include_str!("../../../reference/packages/tui/src/theme/assets/matrix.json"),
        "mercury" => include_str!("../../../reference/packages/tui/src/theme/assets/mercury.json"),
        "monokai" => include_str!("../../../reference/packages/tui/src/theme/assets/monokai.json"),
        "nightowl" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/nightowl.json")
        }
        "nord" => include_str!("../../../reference/packages/tui/src/theme/assets/nord.json"),
        "one-dark" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/one-dark.json")
        }
        "opencode" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/opencode.json")
        }
        "orng" => include_str!("../../../reference/packages/tui/src/theme/assets/orng.json"),
        "osaka-jade" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/osaka-jade.json")
        }
        "palenight" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/palenight.json")
        }
        "rosepine" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/rosepine.json")
        }
        "solarized" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/solarized.json")
        }
        "synthwave84" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/synthwave84.json")
        }
        "tokyonight" => {
            include_str!("../../../reference/packages/tui/src/theme/assets/tokyonight.json")
        }
        "vercel" => include_str!("../../../reference/packages/tui/src/theme/assets/vercel.json"),
        "vesper" => include_str!("../../../reference/packages/tui/src/theme/assets/vesper.json"),
        "zenburn" => include_str!("../../../reference/packages/tui/src/theme/assets/zenburn.json"),
        _ => return None,
    })
}

/// Port of `resolveTheme` from `reference/packages/tui/src/theme/index.ts`:
/// resolves the preset's `defs` + `theme` tables into concrete colors for the
/// requested mode. Fields absent from a preset keep the Rust default palette,
/// mirroring how the reference falls back for optional keys
/// (selectedListItemText -> background, backgroundMenu -> backgroundElement,
/// thinkingOpacity -> 0.6).
fn resolve_preset_into(t: &mut Theme, asset: &str, mode: Mode) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(asset) else {
        return;
    };
    let empty = serde_json::Map::new();
    let defs = v.get("defs").and_then(|d| d.as_object()).unwrap_or(&empty);
    let table = match v.get("theme").and_then(|t| t.as_object()) {
        Some(t) => t.clone(),
        None => return,
    };
    let mode_key = match mode {
        Mode::Dark => "dark",
        Mode::Light => "light",
    };
    let resolve_color =
        |color: &serde_json::Value,
         defs: &serde_json::Map<String, serde_json::Value>,
         table: &serde_json::Map<String, serde_json::Value>|
         -> Option<Color> { resolve_color(color, defs, table, mode_key, &mut Vec::new()) };
    if let Some(c) = table
        .get("primary")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.primary = c;
    }
    if let Some(c) = table
        .get("secondary")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.secondary = c;
    }
    if let Some(c) = table
        .get("accent")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.accent = c;
    }
    if let Some(c) = table
        .get("error")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.error = c;
    }
    if let Some(c) = table
        .get("warning")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.warning = c;
    }
    if let Some(c) = table
        .get("success")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.success = c;
    }
    if let Some(c) = table
        .get("info")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.info = c;
    }
    if let Some(c) = table
        .get("text")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.text = c;
    }
    if let Some(c) = table
        .get("textMuted")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.text_muted = c;
    }
    let background = table
        .get("background")
        .and_then(|c| resolve_color(c, defs, &table));
    if let Some(c) = background {
        t.background = c;
    }
    if let Some(c) = table
        .get("backgroundPanel")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.background_panel = c;
    }
    let background_element = table
        .get("backgroundElement")
        .and_then(|c| resolve_color(c, defs, &table));
    if let Some(c) = background_element {
        t.background_element = c;
    }
    t.background_menu = table
        .get("backgroundMenu")
        .and_then(|c| resolve_color(c, defs, &table))
        .or(background_element)
        .unwrap_or(t.background_menu);
    if let Some(c) = table
        .get("border")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.border = c;
    }
    if let Some(c) = table
        .get("borderActive")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.border_active = c;
    }
    if let Some(c) = table
        .get("borderSubtle")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.border_subtle = c;
    }
    if let Some(c) = table
        .get("diffAdded")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_added = c;
    }
    if let Some(c) = table
        .get("diffRemoved")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_removed = c;
    }
    if let Some(c) = table
        .get("diffContext")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_context = c;
    }
    if let Some(c) = table
        .get("diffHighlightAdded")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_highlight_added = c;
    }
    if let Some(c) = table
        .get("diffHighlightRemoved")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_highlight_removed = c;
    }
    if let Some(c) = table
        .get("diffAddedBg")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_added_bg = c;
    }
    if let Some(c) = table
        .get("diffRemovedBg")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_removed_bg = c;
    }
    if let Some(c) = table
        .get("diffContextBg")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_context_bg = c;
    }
    if let Some(c) = table
        .get("diffLineNumber")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_line_number = c;
    }
    if let Some(c) = table
        .get("diffAddedLineNumberBg")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_added_line_number_bg = c;
    }
    if let Some(c) = table
        .get("diffRemovedLineNumberBg")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.diff_removed_line_number_bg = c;
    }
    if let Some(c) = table
        .get("markdownText")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.markdown_text = c;
    }
    if let Some(c) = table
        .get("markdownHeading")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.markdown_heading = c;
    }
    if let Some(c) = table
        .get("markdownCode")
        .and_then(|c| resolve_color(c, defs, &table))
    {
        t.markdown_code = c;
    }
    if let Some(n) = table.get("thinkingOpacity").and_then(|n| n.as_f64()) {
        t.thinking_opacity = (n * 255.0) as u8;
    }
}

/// Port of `resolveColor` (recursive def-name resolution with cycle detection).
fn resolve_color(
    color: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
    table: &serde_json::Map<String, serde_json::Value>,
    mode_key: &str,
    chain: &mut Vec<String>,
) -> Option<Color> {
    match color {
        serde_json::Value::String(s) => {
            if s == "transparent" || s == "none" {
                return Some(Color::Rgb(0, 0, 0));
            }
            if s.starts_with('#') {
                return Some(from_hex(s));
            }
            if chain.iter().any(|c| c == s) {
                return None;
            }
            let next = defs.get(s).or_else(|| table.get(s))?.clone();
            chain.push(s.clone());
            let resolved = resolve_color(&next, defs, table, mode_key, chain);
            chain.pop();
            resolved
        }
        serde_json::Value::Number(n) => {
            let code = n.as_u64()? as u16;
            Some(ansi_to_color(code))
        }
        serde_json::Value::Object(o) => {
            let next = o.get(mode_key)?;
            resolve_color(next, defs, table, mode_key, chain)
        }
        _ => None,
    }
}

/// Port of `ansiToRgba` (ANSI 16 + 6x6x6 cube + grayscale ramp).
fn ansi_to_color(code: u16) -> Color {
    if code < 16 {
        const ANSI: [&str; 16] = [
            "#000000", "#800000", "#008000", "#808000", "#000080", "#800080", "#008080", "#c0c0c0",
            "#808080", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
        ];
        return from_hex(ANSI[code as usize]);
    }
    if code < 232 {
        let index = code - 16;
        let b = index % 6;
        let g = (index / 6) % 6;
        let r = index / 36;
        let val = |x: u16| if x == 0 { 0u8 } else { (x * 40 + 55) as u8 };
        return Color::Rgb(val(r), val(g), val(b));
    }
    if code < 256 {
        let gray = ((code - 232) * 10 + 8) as u8;
        return Color::Rgb(gray, gray, gray);
    }
    Color::Rgb(0, 0, 0)
}
