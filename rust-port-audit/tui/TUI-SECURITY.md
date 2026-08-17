# TUI Terminal Security Review

## 1. Threat Model & Untrusted Boundaries

The OpenCode TUI renders untrusted content received from:
1. LLM streaming tokens (may contain malicious ANSI / OSC escape sequences)
2. Tool execution outputs (bash stdout/stderr with arbitrary control codes)
3. Repository filenames and git diff paths

## 2. Mitigations Verified

- **ANSI / Control Code Sanitization:** All raw tool outputs and LLM streaming tokens pass through stripping/escaping layers in `crates/oc-tui/src/components/message.rs` before being formatted into Ratatui `Text` / `Span` structs.
- **OSC Escape Sequence Injection Prevention:** Untrusted strings cannot manipulate the terminal title, inject OSC 8 hyperlinks, or trigger arbitrary terminal emulator clipboard writes.
- **Panic Safety & Bounds Checking:** Ratatui frame buffers enforce strict coordinate clipping `(0..width, 0..height)`, preventing out-of-bounds terminal buffer writes.
