//! Resolved TUI configuration.
//! From reference/packages/tui/src/config/index.tsx (`resolve`)

use std::collections::HashMap;

use serde_json::Value;

use crate::keybind::{self, LEADER_TIMEOUT_DEFAULT};
use crate::theme::Mode;

#[derive(Debug, Clone, Default)]
pub struct AttentionConfig {
    pub enabled: bool,
    pub notifications: bool,
    pub sound: bool,
    pub volume: f64,
    pub sound_pack: String,
}

#[derive(Debug, Clone, Default)]
pub struct PromptConfig {
    pub max_height: Option<usize>,
    pub max_width: Option<PromptMaxWidth>,
}

#[derive(Debug, Clone)]
pub enum PromptMaxWidth {
    Fixed(usize),
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffStyle {
    Auto,
    Stacked,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub keybinds: HashMap<String, KeybindEntry>,
    pub leader_timeout: u64,
    pub mouse: bool,
    pub theme: String,
    pub theme_mode: Mode,
    pub attention: AttentionConfig,
    pub prompt: PromptConfig,
    pub scroll_speed: Option<f64>,
    pub scroll_acceleration_enabled: bool,
    pub diff_style: DiffStyle,
}

/// Parsed binding for a keybind name.
#[derive(Debug, Clone)]
pub struct KeybindEntry {
    pub command: &'static str,
    pub binding: Option<crate::keymap::Binding>,
}

impl ResolvedConfig {
    /// Resolve the default configuration (no overrides).
    pub fn default_config() -> Self {
        resolve(&Value::Null, ResolveOptions::default())
    }

    /// Resolve TUI settings from `OPENCODE_TUI_CONFIG` and environment
    /// overrides. Invalid or missing files fall back to the normal defaults.
    pub fn from_environment() -> Self {
        let input = std::env::var_os("OPENCODE_TUI_CONFIG")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null);
        let mut resolved = resolve(&input, ResolveOptions::default());
        if truthy_env("OPENCODE_DISABLE_MOUSE") {
            resolved.mouse = false;
        }
        resolved
    }

    /// Bindings for a named keybind (e.g. `"input.move.left"`).
    pub fn get(&self, name: &str) -> Option<&crate::keymap::Binding> {
        self.keybinds.get(name).and_then(|e| e.binding.as_ref())
    }

    /// Whether a keybind is configured at all (enabled or not).
    pub fn has(&self, name: &str) -> bool {
        self.keybinds.contains_key(name)
    }

    /// Return the configured leader key in the form accepted by `Keymap`.
    ///
    /// A disabled leader intentionally returns an empty string so leader
    /// chords are not registered by the application.
    pub fn leader_key(&self) -> String {
        self.keybinds
            .get("leader")
            .and_then(|entry| entry.binding.as_ref())
            .and_then(|binding| binding.sequences.first())
            .and_then(|sequence| sequence.strokes.first())
            .and_then(|stroke| match stroke {
                crate::keymap::Stroke::Key(key) => Some(key.display()),
                crate::keymap::Stroke::Leader => None,
            })
            .unwrap_or_default()
    }
}

fn truthy_env(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

#[derive(Debug, Clone, Copy)]
pub struct ResolveOptions {
    pub terminal_suspend: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        ResolveOptions {
            terminal_suspend: true,
        }
    }
}

/// Resolve the raw `tui` config object into concrete settings.
/// From reference/packages/tui/src/config/index.tsx (`resolve`)
pub fn resolve(input: &Value, options: ResolveOptions) -> ResolvedConfig {
    let obj = input.as_object().cloned().unwrap_or_default();

    let get_str = |key: &str| obj.get(key).and_then(Value::as_str);
    let get_bool =
        |key: &str, default: bool| obj.get(key).and_then(Value::as_bool).unwrap_or(default);
    let get_number = |key: &str| obj.get(key).and_then(Value::as_f64);

    let mut keybinds: HashMap<String, KeybindEntry> = HashMap::new();
    let overrides = obj.get("keybinds").and_then(Value::as_object);

    for def in keybind::definitions() {
        let raw = overrides
            .and_then(|o| o.get(def.name))
            .map(render_binding_value)
            .unwrap_or_else(|| def.default.to_string());
        let binding = if raw == "none" || raw == "false" {
            None
        } else {
            crate::keymap::Binding::from_string(def.command, def.desc, &raw)
        };
        keybinds.insert(
            def.name.to_string(),
            KeybindEntry {
                command: def.command,
                binding,
            },
        );
    }

    let leader_timeout = obj
        .get("leader_timeout")
        .and_then(Value::as_u64)
        .unwrap_or(LEADER_TIMEOUT_DEFAULT)
        .max(1);

    let attention_obj = obj
        .get("attention")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let attention = AttentionConfig {
        enabled: attention_obj
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        notifications: attention_obj
            .get("notifications")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        sound: attention_obj
            .get("sound")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        volume: attention_obj
            .get("volume")
            .and_then(Value::as_f64)
            .unwrap_or(0.4),
        sound_pack: attention_obj
            .get("sound_pack")
            .and_then(Value::as_str)
            .unwrap_or("opencode.default")
            .to_string(),
    };

    let prompt_obj = obj
        .get("prompt")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let max_width = prompt_obj.get("max_width").and_then(|v| {
        if let Some(n) = v.as_u64() {
            Some(PromptMaxWidth::Fixed(n as usize))
        } else if v.as_str() == Some("auto") {
            Some(PromptMaxWidth::Auto)
        } else {
            None
        }
    });
    let prompt = PromptConfig {
        max_height: prompt_obj
            .get("max_height")
            .and_then(Value::as_u64)
            .map(|v| v as usize),
        max_width,
    };

    let diff_style = match get_str("diff_style") {
        Some("stacked") => DiffStyle::Stacked,
        _ => DiffStyle::Auto,
    };

    let theme = get_str("theme").unwrap_or("opencode").to_string();
    let theme_mode = match get_str("theme_mode") {
        Some("light") => Mode::Light,
        _ => Mode::Dark,
    };

    let scroll_accel = obj
        .get("scroll_acceleration")
        .and_then(Value::as_object)
        .and_then(|o| o.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // From reference/packages/tui/src/config/index.tsx: when terminal suspend
    // is unavailable, `input_undo` gains `ctrl+z`.
    if !options.terminal_suspend {
        if let Some(entry) = keybinds.get_mut("input_undo") {
            if let Some(binding) = &mut entry.binding {
                if let Some(seq) = parse_sequence_parts("ctrl+z") {
                    binding.sequences.push(seq);
                }
            }
        }
    }

    ResolvedConfig {
        keybinds,
        leader_timeout,
        mouse: get_bool("mouse", true),
        theme,
        theme_mode,
        attention,
        prompt,
        scroll_speed: get_number("scroll_speed"),
        scroll_acceleration_enabled: scroll_accel,
        diff_style,
    }
}

fn parse_sequence_parts(raw: &str) -> Option<crate::keymap::Sequence> {
    crate::keymap::parse_binding(raw).and_then(|p| p.sequences.into_iter().next())
}

/// Render a keybind config value (string, object or boolean) to a string form
/// consumable by the keymap parser.
fn render_binding_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Object(map) => {
            let mut parts = Vec::new();
            if let Some(k) = map.get("key") {
                parts.push(format!("key:{}", render_binding_value(k)));
            }
            if let Some(pd) = map.get("preventDefault") {
                parts.push(format!("preventDefault:{pd}"));
            }
            format!("{{{}}}", parts.join(","))
        }
        _ => value.to_string(),
    }
}

/// Compute the home-screen prompt max width.
/// From reference/packages/tui/src/routes/home.tsx (`promptMaxWidth`)
pub fn home_prompt_max_width(config: &ResolvedConfig, width: u16) -> usize {
    match config.prompt.max_width.as_ref() {
        Some(PromptMaxWidth::Auto) => (width as f64 * 0.7).floor().max(75.0) as usize,
        Some(PromptMaxWidth::Fixed(n)) => *n,
        None => 75,
    }
}

/// Compute the prompt textarea max height.
/// From reference/packages/tui/src/component/prompt/index.tsx (`maxHeight`)
pub fn prompt_max_height(config: &ResolvedConfig, height: u16) -> usize {
    config
        .prompt
        .max_height
        .unwrap_or_else(|| (height as f64 / 3.0).floor().max(6.0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_defaults() {
        let config = resolve(&Value::Null, ResolveOptions::default());
        assert_eq!(config.leader_timeout, LEADER_TIMEOUT_DEFAULT);
        assert!(config.mouse);
        assert_eq!(config.theme, "opencode");
        assert_eq!(config.theme_mode, Mode::Dark);
        assert!(config.has("input_move_left"));
        assert!(config.has("command_list"));
    }

    #[test]
    fn resolve_with_overrides() {
        let input = json!({
            "keybinds": {
                "command_list": "ctrl+y",
                "agent_cycle": "none",
                "leader": "ctrl+space"
            },
            "leader_timeout": 500,
            "mouse": false
        });
        let config = resolve(&input, ResolveOptions::default());
        assert_eq!(config.leader_timeout, 500);
        assert!(!config.mouse);
        // Disabled binding resolves to None.
        assert!(config.get("agent_cycle").is_none());
        // Override binding re-parses.
        let b = config.get("command_list").unwrap();
        assert_eq!(b.command, "command.palette.show");
        assert_eq!(b.sequences.len(), 1);
        assert_eq!(config.leader_key(), "ctrl+space");
    }

    #[test]
    fn disabled_leader_does_not_register_a_chord() {
        let config = resolve(
            &json!({ "keybinds": { "leader": "none" } }),
            ResolveOptions::default(),
        );
        assert_eq!(config.leader_key(), "");
    }

    #[test]
    fn object_binding_value() {
        let input =
            json!({ "keybinds": { "input_paste": { "key": "ctrl+v", "preventDefault": false } } });
        let config = resolve(&input, ResolveOptions::default());
        assert!(config.get("input_paste").is_some());
    }

    #[test]
    fn unknown_keybinds_ignored() {
        let input = json!({ "keybinds": { "bogus_key": "ctrl+x" } });
        let config = resolve(&input, ResolveOptions::default());
        assert!(!config.has("bogus_key"));
        assert!(config.has("command_list"));
    }

    #[test]
    fn terminal_suspend_adds_ctrl_z_undo() {
        let config = resolve(
            &Value::Null,
            ResolveOptions {
                terminal_suspend: false,
            },
        );
        let undo = config.get("input_undo").unwrap();
        let keys: Vec<String> = undo
            .sequences
            .iter()
            .flat_map(|s| {
                s.strokes.iter().map(|st| match st {
                    crate::keymap::Stroke::Key(k) => k.display(),
                    crate::keymap::Stroke::Leader => "leader".to_string(),
                })
            })
            .collect();
        assert!(keys.contains(&"ctrl+z".to_string()));
    }

    #[test]
    fn prompt_width_helpers() {
        let config = resolve(&Value::Null, ResolveOptions::default());
        assert_eq!(home_prompt_max_width(&config, 100), 75);
        let config = resolve(
            &json!({ "prompt": { "max_width": "auto" } }),
            ResolveOptions::default(),
        );
        assert_eq!(home_prompt_max_width(&config, 200), 140);
        let config = resolve(
            &json!({ "prompt": { "max_width": 50 } }),
            ResolveOptions::default(),
        );
        assert_eq!(home_prompt_max_width(&config, 200), 50);
        let config = resolve(
            &json!({ "prompt": { "max_height": 3 } }),
            ResolveOptions::default(),
        );
        assert_eq!(prompt_max_height(&config, 30), 3);
        let config = resolve(&Value::Null, ResolveOptions::default());
        assert_eq!(prompt_max_height(&config, 30), 10);
    }

    #[test]
    fn diff_style_parsing() {
        let config = resolve(
            &json!({ "diff_style": "stacked" }),
            ResolveOptions::default(),
        );
        assert_eq!(config.diff_style, DiffStyle::Stacked);
        let config = resolve(&Value::Null, ResolveOptions::default());
        assert_eq!(config.diff_style, DiffStyle::Auto);
    }

    #[test]
    fn theme_and_mode_parsing() {
        let config = resolve(
            &json!({ "theme": "opencode", "theme_mode": "light" }),
            ResolveOptions::default(),
        );
        assert_eq!(config.theme, "opencode");
        assert_eq!(config.theme_mode, Mode::Light);

        let config = resolve(
            &json!({ "theme": 42, "theme_mode": "system" }),
            ResolveOptions::default(),
        );
        assert_eq!(config.theme, "opencode");
        assert_eq!(config.theme_mode, Mode::Dark);
    }

    #[test]
    fn environment_config_loads_file_and_disables_mouse() {
        let path = std::env::temp_dir().join(format!(
            "opencode-tui-config-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(
            &path,
            r#"{"mouse":true,"leader_timeout":777,"theme":"opencode","theme_mode":"light"}"#,
        )
        .unwrap();
        let previous_path = std::env::var_os("OPENCODE_TUI_CONFIG");
        let previous_mouse = std::env::var_os("OPENCODE_DISABLE_MOUSE");
        std::env::set_var("OPENCODE_TUI_CONFIG", &path);
        std::env::set_var("OPENCODE_DISABLE_MOUSE", "1");

        let config = ResolvedConfig::from_environment();

        assert_eq!(config.leader_timeout, 777);
        assert!(!config.mouse);
        assert_eq!(config.theme, "opencode");
        assert_eq!(config.theme_mode, Mode::Light);
        match previous_path {
            Some(value) => std::env::set_var("OPENCODE_TUI_CONFIG", value),
            None => std::env::remove_var("OPENCODE_TUI_CONFIG"),
        }
        match previous_mouse {
            Some(value) => std::env::set_var("OPENCODE_DISABLE_MOUSE", value),
            None => std::env::remove_var("OPENCODE_DISABLE_MOUSE"),
        }
        let _ = std::fs::remove_file(path);
    }
}
