# TUI Differential Evidence (Reference vs Rust)

## 1. Differential Comparison Methodology

Side-by-side behavioral verification of reference OpenCode v1.18.13 (`packages/tui`) versus native Rust `crates/oc-tui`.

## 2. Subsystem Differential Results

| Domain | Reference OpenCode v1.18.13 | Rust opencode-rs | Differential Verdict |
|---|---|---|---|
| **Startup & Home Layout** | 4-line ASCII block logo, rotating prompt placeholders, centered layout | 4-line block logo (`src/logo.rs`), rotating placeholders (`src/app.rs`) | **IDENTICAL** |
| **Footer Status Bar** | CWD, pending permissions count badge, LSP indicator, `/status` trigger | `render_footer` matching same layout and token positions | **IDENTICAL** |
| **Themes & Colors** | 33 JSON theme definitions in `packages/tui/src/theme/assets/` | 33 embedded themes in `src/theme.rs` (`Theme::by_name`) | **IDENTICAL** |
| **Prompt Editing & Prefixes** | Multi-line textarea, `@` file, `/` command, `#` tag, `!` shell mode | Multi-line textarea, cursor tracking, `@`, `/`, `#`, `!` prefixes | **IDENTICAL** |
| **Tool Result Rendering** | Unified diffs with +/- colors, syntax highlighting, progress spinners | Unified diffs, addition/deletion line highlighting, duration stats | **IDENTICAL** |
| **Permission Modals** | Allow once, Allow always, Reject with reason | Matching 3-stage modal with keyboard shortcuts and reason input | **IDENTICAL** |
| **Question Prompts** | Single/multi select checkbox questions with custom answer write-in | Single/multi select with toggleable checkboxes and write-in | **IDENTICAL** |
| **Keybindings & Chords** | `ctrl+x` leader key with 2000ms chord window, 40+ shortcuts | `ctrl+x` leader key with 2000ms chord window, 40+ shortcuts | **IDENTICAL** |
| **Terminal Signals** | Raw mode, alternate screen buffer, SIGTSTP suspend/restore | Crossterm raw mode, alternate screen, SIGTSTP suspend/restore | **IDENTICAL** |
