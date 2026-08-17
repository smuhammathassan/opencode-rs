# TUI Differential Evidence (Reference vs Rust)

## 1. Differential Comparison Methodology

Direct, process-backed behavioral verification comparing the vendored OpenCode v1.18.13 reference implementation (`reference/packages/tui` and `reference/packages/session-ui`) against native Rust `crates/oc-tui`.

The differential runner (`scripts/run_differential.py`):
1. Directly loads vendored TypeScript modules into Node.js using an ESM loader (`scripts/ts_loader.mjs`).
2. Directly executes production Rust functions via `cargo run -q -p oc-tui --example diff_scenarios -- <scenario_id>`.
3. Normalizes both outputs into canonical JSON (`sort_keys=True, separators=(",", ":")`).
4. Asserts exit code `0`, `outputs_equal == True`, and cryptographic `SHA-256` output equality.
5. Saves machine-verifiable paired process frames and results under `rust-port-audit/tui/differential/<scenario_id>/`.

## 2. 26 Machine-Verified Differential Scenarios

| Scenario ID | Name | Reference Source | Rust Target | Verdict |
|---|---|---|---|---|
| `001-prompt-history-parse` | Prompt History JSONL Parse & Recovery | `reference/packages/tui/src/prompt/history.tsx` | `parse_prompt_history` | **PASS** |
| `002-prompt-history-dedup` | Prompt History Consecutive Deduplication | `reference/packages/tui/src/prompt/history.tsx` | `is_duplicate_entry` | **PASS** |
| `003-prompt-paste-placeholders` | Pasted Text Placeholder Expansion | `reference/packages/tui/src/prompt/part.ts` | `expand_text_parts` | **PASS** |
| `004-prompt-stash` | Prompt Stash Parse & Cap | `reference/packages/tui/src/prompt/stash.tsx` | `parse_prompt_stash` | **PASS** |
| `005-keymap-leader` | Keymap Leader Token & Base Mode | `reference/packages/tui/src/keymap.tsx` | `LEADER_TOKEN`, `OPENCODE_BASE_MODE` | **PASS** |
| `006-keymap-chord-timeout` | Keybind Leader Default | `reference/packages/tui/src/config/keybind.ts` | `KeymapOptions::default` | **PASS** |
| `007-theme-presets` | Registered 33 Theme Preset Names | `reference/packages/tui/src/theme/index.ts` | `Theme::available_themes` | **PASS** |
| `008-theme-preset-data` | Theme Preset Raw Data (opencode/dracula/nord) | `reference/packages/tui/src/theme/assets/*.json` | `preset_raw_data` | **PASS** |
| `009-theme-resolve` | Theme Resolution Anchor Colors (Dark) | `reference/packages/tui/src/theme/index.ts` | `Theme::by_name` | **PASS** |
| `010-format-duration` | Duration Formatting Boundaries | `reference/packages/tui/src/util/format.ts` | `format_duration` | **PASS** |
| `011-format-collapse` | Tool Output Collapse (Short/Long/Wide) | `reference/packages/tui/src/util/collapse-tool-output.ts` | `collapse_tool_output` | **PASS** |
| `012-clipboard-lookup` | Clipboard Command Lookup Matrix | `reference/packages/tui/src/clipboard.ts` | `copy_command_with_lookup` | **PASS** |
| `013-editor-normalize` | External Editor Prompt Normalization | `reference/packages/tui/src/editor.ts` | `normalize_prompt_content` | **PASS** |
| `014-patch-metadata` | Apply-Patch Single File Metadata | `reference/packages/session-ui/src/components/apply-patch-file.ts` | `parse_apply_patch_files` | **PASS** |
| `015-locale-duration` | Locale Duration Formatting | `reference/packages/tui/src/util/locale.ts` | `locale::duration` | **PASS** |
| `016-prompt-interaction` | Prompt Input Interaction State Machine | `reference/packages/session-ui/src/v2/components/prompt-input/machine.ts` | `prompt::interaction::transition` | **PASS** |
| `017-logo` | Home Logo Left Column | `reference/packages/tui/src/logo.ts` | `LOGO.left` | **PASS** |
| `018-locale-titlecase` | Locale Titlecase | `reference/packages/tui/src/util/locale.ts` | `locale::titlecase` | **PASS** |
| `019-locale-truncate` | Locale Truncate & Truncate Middle | `reference/packages/tui/src/util/locale.ts` | `locale::truncate`, `locale::truncate_middle` | **PASS** |
| `020-clipboard-wayland` | Clipboard Wayland Selection | `reference/packages/tui/src/clipboard.ts` | `copy_command_with_lookup` | **PASS** |
| `021-clipboard-macos` | Clipboard macOS osascript | `reference/packages/tui/src/clipboard.ts` | `copy_command_with_lookup` | **PASS** |
| `022-clipboard-x11` | Clipboard X11 Fallback Sequence | `reference/packages/tui/src/clipboard.ts` | `copy_command_with_lookup` | **PASS** |
| `023-clipboard-none` | Clipboard Unavailable Returns Null/Undefined | `reference/packages/tui/src/clipboard.ts` | `copy_command_with_lookup` | **PASS** |
| `024-patch-metadata-multi` | Apply-Patch Multi-File Parsing | `reference/packages/session-ui/src/components/apply-patch-file.ts` | `parse_apply_patch_files` | **PASS** |
| `025-theme-preset-data-2` | Theme Preset Raw Data (catppuccin/gruvbox/tokyonight) | `reference/packages/tui/src/theme/assets/*.json` | `preset_raw_data` | **PASS** |
| `026-editor-multiline` | Editor Multiline Normalization | `reference/packages/tui/src/editor.ts` | `normalize_prompt_content` | **PASS** |

## 3. Honest Exclusion List & Observable Coverage

The following behaviors are excluded from pure module differential testing and are instead verified via interactive OS pseudo-terminal (PTY) end-to-end testing in `crates/oc-tui/tests/interactive_pty.rs` and `crates/oc-tui/tests/terminal_e2e.rs`:

| Category | Excluded Item | Technical Rationale | Verification Venue |
|---|---|---|---|
| **Terminal Rendering Loop** | `@opentui/core` CliRenderer canvas & cell draw loops | `@opentui/core` is a bun-native compiled UI library not vendored as a pure JavaScript library. | Verified via `crates/oc-tui/tests/interactive_pty.rs` (`tui_launches_renders_home_and_quits_cleanly`). |
| **Interactive Textarea** | `@opentui/core` TextareaRenderable keystroke dispatcher | Keystroke handling inside terminal widgets requires an allocated PTY master/slave pair. | Verified via `crates/oc-tui/tests/interactive_pty.rs` (`tui_typing_appears_in_prompt`). |
| **Terminal Mode Switching** | Alternate screen enter (`\x1b[?1049h`) and exit (`\x1b[?1049l`) | Terminal escape emission requires child process lifecycle execution under a real PTY. | Verified via `crates/oc-tui/tests/interactive_pty.rs` (`tui_sigterm_exits_and_restores`). |
| **Bracketed Paste Handling** | Terminal bracketed paste sequences (`\x1b[200~...\x1b[201~`) | Requires raw mode terminal stream parsing from PTY stdin. | Verified via `crates/oc-tui/tests/interactive_pty.rs` (`tui_bracketed_paste_into_prompt`). |
| **Window Resizing** | `SIGWINCH` / `TIOCSWINSZ` reactive redraw | OS terminal signal dispatch. | Verified via `crates/oc-tui/tests/interactive_pty.rs` (`tui_resize_keeps_responsive`). |
