//! TUI performance, startup, and memory footprint benchmarks.
//!
//! Measures cold initialization time, theme loading latency, keymap trie
//! compilation speed, markdown rendering throughput, and message line layout
//! performance under reproducible headless conditions.

use oc_tui::components::text::plain;
use oc_tui::config::ResolvedConfig;
use oc_tui::keymap::{Keymap, KeymapOptions};
use oc_tui::prompt::state::PromptState;
use oc_tui::sync::SyncState;
use oc_tui::theme::Theme;
use oc_tui::util::markdown::{render, MarkdownOptions};
use std::time::Instant;

#[test]
fn cold_theme_and_keymap_init_time() {
    let start = Instant::now();
    let theme = Theme::dark();
    let _keymap = Keymap::new(KeymapOptions::default());
    let config = ResolvedConfig::default_config();
    let prompt = PromptState::default();
    let sync = SyncState::default();
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 50,
        "Subsystem cold initialization took {:?}",
        elapsed
    );
    assert_eq!(theme.name, "opencode");
    assert_eq!(oc_tui::keybind::LEADER_DEFAULT, "ctrl+x");
    assert!(config.mouse);
    assert_eq!(prompt.text(), "");
    assert!(sync.sessions.is_empty());
}

#[test]
fn all_33_themes_load_latency_benchmark() {
    let start = Instant::now();
    for name in Theme::available_themes() {
        let _dark = Theme::by_name(name, oc_tui::theme::Mode::Dark);
        let _light = Theme::by_name(name, oc_tui::theme::Mode::Light);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 150,
        "Loading all 33 themes took {:?}",
        elapsed
    );
}

#[test]
fn markdown_rendering_throughput_benchmark() {
    let sample_markdown = r#"
# Heading 1
Here is a paragraph with **bold** text, *italic* text, and `inline code`.

- List item 1
- List item 2 with [a link](https://opencode.ai)
- List item 3

```rust
fn main() {
    println!("Hello from high performance Rust TUI!");
}
```

> Blockquote with some information.
---
"#;

    let start = Instant::now();
    let iterations = 100;
    for _ in 0..iterations {
        let lines = render(sample_markdown, &MarkdownOptions::default());
        assert!(!lines.is_empty());
    }
    let elapsed = start.elapsed();
    let avg_us = elapsed.as_micros() / iterations as u128;
    assert!(
        avg_us < 500,
        "Average markdown render time {:?}us should be sub-millisecond",
        avg_us
    );
}

#[test]
fn styled_lines_layout_throughput_benchmark() {
    let start = Instant::now();
    let iterations = 1000;
    for i in 0..iterations {
        let text = format!("Line {i}: Status ok, elapsed time 0.05s");
        let line = plain(text);
        assert_eq!(oc_tui::components::text::width(&line) > 0, true);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 20,
        "Layout of 1000 styled lines took {:?}",
        elapsed
    );
}
