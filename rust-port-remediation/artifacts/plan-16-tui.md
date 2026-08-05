# Plan 16 — TUI integration, terminal safety, attach mode

Agent 16 · Wave 0 read-only planning. Repo `/root/opencode-rs` @ `fix/audit-remediation`.
Domain: launching the real TUI, terminal-safety (escape sanitization, restoration),
keymap truthfulness, attach/mini modes, NO_COLOR. Mirrors
`reference/packages/opencode/src/cli/tui/*` + `cmd/tui.ts` + `cmd/run.ts`
(`runMini`) + `reference/packages/tui/`.

---

## 1. Owned findings

| ID | Sev | Evidence | Status |
|----|-----|----------|--------|
| CLI-003 | Critical (release blocker) | `attach.rs:168-170` `not_wired("TUI not yet wired")`, `:122-124` `--mini`, `:74` `attach`. `oc_tui::app::run_async` (app.rs:2752) has **zero callers** in the workspace; only re-exported at `lib.rs:19`. Non-TTY fallback `:164-167` prints dim "starting TUI (requires a TTY)" and returns **exit 0** doing nothing. | CONFIRMED (runtime) |
| UX-001 | High (release blocker) | `markdown.rs:88-336` pushes every char incl. `\x1b` into `MdSpan`; ratatui 0.29 `Span::render_ref` writes graphemes with **no** control-char filter (only `Buffer::set_stringn` filters, unused on Paragraph path). Runtime proof: SGR `\x1b[31m…\x1b[0m` + OSC `\x1b]0;TITLE\x07` passed through verbatim. | CONFIRMED (runtime) |
| UX-002 | High (release blocker) | `app.rs:2754-2755` enable_raw_mode + EnterAlternateScreen; restored only on happy path (`:2762-2764`). No panic hook, no Drop guard, no SIGTERM/SIGHUP/SIGINT/SIGTSTP handler, no SIGCONT resume. Mouse + bracketed paste disabled only on happy path (`:2895-2896`). | CONFIRMED (static) |
| UX-003 | Medium | 58 default-enabled bindings dispatch to no-op (`app.rs:1158-1160` `tracing::debug!("unhandled command")`). Re-derived: 140 default-enabled commands, 37 unconditional + ~21 prompt-conditional missing from `dispatch`. Real default keys: `diff.close`=esc/q, `diff.toggle`=enter/space, `diff.next_file`=n/p, `diff.*`=[/], `which-key.*`, `terminal.suspend`=ctrl+z, `model.dialog.favorite`=ctrl+f, `model.dialog.provider`=ctrl+a, `session.pin.toggle`=ctrl+f, `stash.delete`=ctrl+d, `dialog.plugins.*`, `dialog.mcp.*`, `plugins.*`. | CONFIRMED (static) |
| UX-004 | Medium | Non-TTY fallback exits 0 silently; reference reads piped stdin as the initial prompt (`tui.ts:60` `process.stdin.isTTY ? undefined : await Bun.stdin.text()`; combined `piped + "\n" + value` at `:63`). `echo hi | opencode` in CI silently does nothing. | CONFIRMED (runtime) |
| UX-006 | Medium | `theme.rs:119-156` hardcodes `Color::Rgb`; no NO_COLOR / COLORTERM / TERM / color-depth detection anywhere in `crates/`. `Theme::light()` clones dark palette (`theme.rs:58-62`). | CONFIRMED (static) |
| UX-007 | Medium | oc-tui duplicates the HTTP+SSE transport (`client.rs` `HttpSdkClient`) and declares `oc-client` (`Cargo.toml:21`) but never uses it; not connected to any server. | CONFIRMED (static) |
| UX-008 | Medium | Endpoint mismatch: TUI calls `/session/{id}/compact` (`client.rs:365`) but oc-server registers compact only under `/api/session/:sessionID/compact` (`router.rs:321-322`). All other TUI endpoints match the legacy non-`/api` surface (`router.rs:205-288`). Event pipeline (SSE reconnect, sync reducer, bootstrap) never exercised against a live server. | CONFIRMED (static) |
| UX-009 | Low | `Event::FocusGained/Lost` fall to `_ => false` (`app.rs:805`). Mouse = scroll + left-click only. No Windows Ctrl+C guard (reference `terminal-win32.ts`). | CONFIRMED (static) |

Scope note: UX-010 (per-frame full re-render) → Agent 20/performance; UX-011 (RTL/emoji at
terminal level) → covered only insofar as the sanitizer must **preserve** Unicode; UX-012
(snapshot test style) → addressed by the PTY tests below. `run --mini` is unreachable from
the `run` subcommand in both port and reference (`run/mod.rs:495-497`), so `--mini` is the
default command's mini branch only.

---

## 2. Files to change

Owned (this agent executes):
- `crates/oc-tui/src/util/sanitize.rs` (new) — control-sequence filter (UX-001 core).
- `crates/oc-tui/src/util/markdown.rs` — sanitize `content` at entry of `render()`; unit tests.
- `crates/oc-tui/src/components/text.rs` — `to_ratatui` applies `sanitize` per span (defense in depth; covers dialog items, sidebar titles, prompt parts, tool rows that don't pass through markdown).
- `crates/oc-tui/src/terminal.rs` (new) — `TerminalGuard` (RAII restore), panic hook, signal-handler registry, Windows Ctrl+C guard, SIGCONT resume, suspend/restore for `terminal.suspend`.
- `crates/oc-tui/src/app.rs` — `run_async` refactor to install guard + signal registry; poll loop checks shutdown/restore flags; handle Focus events; `dispatch` fall-through → toast instead of silent debug; wire cheap dialog commands; add `--crash` env-gated fault for restoration tests.
- `crates/oc-tui/src/keybind.rs` — set defaults to `"none"` for unported surfaces (diff viewer, which-key, plugins/MCP/console/dialog, docs); keep definitions (parity) so user config overrides still resolve.
- `crates/oc-tui/src/config.rs` — honor `terminal_suspend` for ctrl+z (already parses `ResolveOptions`); NO_COLOR/color-depth is resolved by `TerminalGuard` and surfaced via a `Theme` override.
- `crates/oc-tui/src/theme.rs` — add `Theme::for_terminal(depth)` mapping `Color::Rgb` → 16/256-color indexes under NO_COLOR/`TERM=dumb`/`COLORTERM` absent.
- `crates/oc-tui/src/client.rs` — fix `/session/{id}/compact` target (align with server); add adapter seam over `oc_client` (Agent 11) while keeping `SdkClient` trait.
- `crates/oc-tui/src/lib.rs` — export `terminal::TerminalGuard`, `sanitize`.
- `crates/oc-tui/Cargo.toml` — add `signal-hook = "0.3"`, `#[cfg(windows)] windows-sys` (Console API); keep `oc-client` (now used).
- `crates/oc-tui/src/components/message.rs` — no change required beyond sanitize in `to_ratatui`; add escape-injection fixture test at render level.
- `crates/oc-cli/Cargo.toml` — add `oc-tui` dependency.
- `crates/oc-cli/src/cli/cmd/attach.rs` — real `run_default_tui`, `run_mini_attach`, `run` (UX-004/CLI-003).
- `crates/oc-cli/src/cli/cmd/run/mod.rs` — shared helper for piped-stdin + initial-input resolution if it diverges from attach (currently `run.rs:529-535` reads piped; keep, no structural change).
- `crates/oc-tui/tests/sanitize.rs`, `crates/oc-tui/tests/keybind_reachability.rs`, `crates/oc-tui/tests/pty.rs` (new).

Co-authored / integration (needs dependency agents, do NOT edit alone):
- `crates/oc-server/src/router.rs` — add legacy `/session/:sessionID/compact` route (Agent 10 owns server; reference registers compact on the legacy surface).
- `crates/oc-server/src/state.rs` / handlers — real session create/prompt/SSE so bootstrap + real create/continue PTY tests pass (Agents 10 + 07/03).
- `crates/oc-client` — `validateSession` helper + typed endpoints the TUI adapter and attach preflight use (Agent 11).
- `crates/oc-cli` args/network surfaces stay stable for the seam (Agent 02; no API change expected — `run_default_tui` already receives `&Cli`).

---

## 3. Sanitization approach (filter strategy preserving legit text)

Design a single idempotent `sanitize(content: &str) -> String` in `util/sanitize.rs` that
**filters control/escape constructs and drops nothing else**. Non-control Unicode — CJK,
emoji (incl. ZWJ/combining), combining marks, RTL/bidi text, tabs, `\n`, `\r`→`\n` — passes
through byte-for-byte, so the existing `unicode-width` wrap/pad math and markdown layout are
unaffected.

Algorithm (ECMA-48/ANSI X3.64):
1. C0 controls: drop `0x00-0x1F` except `\t` (preserve), `\n` (preserve), `\r` (normalize to `\n`). Drop DEL `0x7F`.
2. C1 controls `U+0080..=U+009F`: drop (this also neutralizes `U+009B` CSI-as-single-char and `U+009D` OSC-as-single-char variants).
3. ESC (U+001B) sequence handling — consume and drop the whole construct:
   - `ESC [` CSI: consume params `0x30-0x3F`, intermediates `0x20-0x2F`, then final `0x40-0x7E`. (SGR `\x1b[31m`, clear `\x1b[2J`, private `\x1b[?2004h` all covered.)
   - `ESC ]` OSC: consume until BEL `0x07` or `ESC \` (ST). (`\x1b]0;TITLE\x07` covered.)
   - `ESC P` DCS, `ESC ^` PM, `ESC _` APC, `ESC X` SOS: consume until ST (`ESC \`).
   - `ESC ( / ) / #` charset/`#` sequences: consume next 1-2 bytes.
   - two-char escapes `ESC 7/8/=/D/E/M/…`: consume next byte.
   - bare `ESC` (no valid continuation): drop the `ESC` only.
4. Keep `\t`, `\n`; never alter UTF-8 code points mid-sequence (work on `char_indices`, copy `char` units).

Application (two layers, both cheap):
- **Layer 1 — markdown:** call `sanitize(content)` at the top of `markdown::render()` so
  every downstream span (inline code, fenced code, tables) and the width/wrap math see clean
  text. This is the primary fix for UX-001 (message, reasoning, tool-output, permission,
  question text all flow through here).
- **Layer 2 — ratatui boundary:** `to_ratatui` (`components/text.rs:69`) sanitizes each span's
  text before `Span::styled`. This is the choke point every `StyledLine` passes through
  (`app.rs:2180,2412,2686,2975`, sidebar, prompt parts, dialog rows), so any future renderer
  is safe by construction. The only direct `Span::styled` outside it is the authored ASCII logo
  (`app.rs:2231`) — constant, no user input, no change.
- Authored ANSI (our own `ui.rs` banner, ratatui Style emission) is untouched — sanitizer runs
  on *content strings* only, never on style emission, so it cannot strip ratatui's styling.

Preservation test matrix: `日本語`, `🚀`, `👨👩👧👦` (ZWJ), `e\u0301` (combining), RTL
`مرحبا`, tabs/newlines — all byte-identical after sanitize.

---

## 4. Restoration-hook design

New `crates/oc-tui/src/terminal.rs`:

1. **`TerminalGuard` (RAII)** — created inside `run_async` before raw mode:
   - enter: `enable_raw_mode`, `EnterAlternateScreen`, `EnableMouseCapture` (if config), `EnableBracketedPaste`.
   - `Drop`: DisableBracketedPaste → DisableMouseCapture → LeaveAlternateScreen → show_cursor → `disable_raw_mode`, each best-effort (`let _ =`). Idempotent via an `entered: bool` (a no-op after a manual restore).
   - Also exposes `restore(&self)` for the happy-path and signal paths so Drop and explicit restore share one code path.
2. **Panic hook (process-global backstop):** `run_async` installs (save-prev) a hook that (a) performs an emergency `restore_global()` from a static `OnceLock<RestoreFn>` + `AtomicBool` guard and (b) delegates to the previous hook for message printing. This covers panics on Tokio worker threads (spawned tasks), where unwinding would not drop the guard held in the main task. Restore is thread-safe: raw mode is termios/process-global; writes use a locked stdout handle. The static restore is registered before raw mode and cleared after.
3. **Signal handling (`signal-hook`):** register SIGTERM, SIGHUP, SIGINT (for `kill -INT`, since in raw mode Ctrl+C is a key event, not a signal) as a flag iterator; the 16 ms poll loop checks the flag → sets `app.exiting = true` → loop exits → guard `Drop` restores → clean shutdown with correct exit status. Best-effort: on an already-dead PTY (SSH drop) the restore writes fail harmlessly.
4. **SIGCONT resume + `terminal.suspend` (ctrl+z):** when the `terminal.suspend` command fires (config-gated, `config.rs:189`), guard `restore()`, `kill(getpid(), SIGTSTP)`, and on SIGCONT re-enter raw mode + force a full redraw — mirrors reference `app.tsx:875` `process.once("SIGCONT", () => renderer.resume())`. When `terminal_suspend` is off, the config already reassigns ctrl+z to `input.undo`.
5. **Windows Ctrl+C guard (`#[cfg(windows)]`):** mirror `terminal-win32.ts` with `windows-sys`: clear `ENABLE_PROCESSED_INPUT` on the console stdin handle so Ctrl+C arrives as a key event, hook `set_raw_mode` to re-enforce, 100 ms poll backstop, restore original mode on unguard. Kept behind cfg; untestable on this Linux host (risk noted in §8).
6. **NO_COLOR / COLORTERM:** `TerminalGuard::init` detects `NO_COLOR` (any value) → `Theme` colors collapse to `Color::Reset`/16-color; `TERM=dumb` → no color; `COLORTERM` missing and `TERM` not `xterm*`/`*-256color`/`*-truecolor` → map `Color::Rgb` to nearest ANSI-16 (or 256) index via a small palette in `theme.rs`. Applied by constructing the `Theme` after guard init; exposed as `Theme::for_terminal(depth)` for unit tests.

---

## 5. Launch wiring (oc-cli seam)

`run_default_tui` (attach.rs:91):
1. Existing arg validation stays (network-with-mini, fork, no-replay etc. — `attach.rs:94-142`).
2. `resolve_thread_directory` + chdir (already there, `:144-154`).
3. **Piped stdin (UX-004):** `if !stdin().is_terminal()` read all of stdin → `piped`. Initial prompt = if `--prompt` given `piped + "\n" + value` else `piped` (mirror `tui.ts:59-64`). Prefilled into the home prompt, **not** auto-submitted — matches reference.
4. **stdout not a TTY:** replace the silent exit-0 with a real error (`Error: the TUI requires a TTY for stdout`) and exit 1 — the reference's renderer throws in this case; the current `exit 0` is the foot-gun. (`--mini` already dies in `run.rs:512-514`.)
5. **Server:** if `external` (`attach.rs:158-159`: port/hostname/mdns) → skip in-process server, `url = http://hostname:port`. Else start in-process `oc_server::server::listen(ListenOptions { port: 0, .. })` (Agent 10; port 0 → prefer 4096, fallback free, `server.rs:56-70`), hold `Listener` for the process, `listener.stop(true)` after TUI exit; `url = listener.url`.
6. **Preflight:** if `--session`, validate via the canonical client (`validate-session.ts`): decode `ses_` id + `GET /session/:id`; failure → `Error: Session not found` exit 1.
7. **Launch:** build `oc_tui::app::TuiInput` (url, directory, cwd, home, state_dir, `ResolvedConfig::default_config()` extended with agent/model/continue/session/prompt), `TerminalGuard` wrap, `oc_tui::app::run_async(input).await`; on return (any path) `listener.stop`.

`run` (attach) — same, but url is the given `--url`, no in-process server, optional auth headers; `--mini` → `run_mini_attach`.

`run_default_tui` `--mini` branch (`attach.rs:100-125`): mirror `runMini` (`run.ts:977-1011`) → a compact interactive loop over the in-process server using the same `SdkClient`/`sync`/markdown core. Implemented as `oc-tui/src/mini.rs` (new, reuses `client`, `sync`, `sanitize`, `TerminalGuard`): prompt line + streaming output + replay of the session (`--continue`/`--session`/`--fork`), `--replay-limit` cap, resize-safe. **Landed last** in this plan (largest new surface).

Shared helpers moved into `attach.rs` (or a small `tui_launch.rs`): piped-stdin read, prompt assembly, TuiInput construction — used by both `run_default_tui` and `run`.

---

## 6. Keybind plan (UX-003)

Two tracks, one reachability test:
- **Truthful defaults:** for every default-enabled binding whose surface is not ported (diff viewer `diff.*`, `which-key.*`, `plugins.*`, `dialog.mcp.*`, `dialog.plugins.*`, `docs.open`, `app.console`, `app.heap_snapshot`, MCP/plugin dialogs, console), set `default = "none"` in `keybind.rs`. Definitions stay (183, parity), so user config can still bind them; dispatch of an unbound-surface command shows a toast "not available in this build", never a silent debug.
- **Wire cheap real semantics:** `model.dialog.favorite`/`model.dialog.provider` → `DialogKind::ModelList`/`ProviderList`; `session.pin.toggle` → real pin via `Local`; `stash.delete` → `stash` op; `terminal.suspend` → §4 SIGTSTP path; `diff.*` stays `none` (diff viewer out of scope for this wave; note for follow-up).
- **Fall-through:** change `app.rs:1158-1160` `_ =>` to push `toasts.warn("Unbound command: {command}")` so no binding can silently no-op.
- **Reachability test** (`tests/keybind_reachability.rs`): iterate every default-enabled `DEFINITIONS` command and assert a `dispatch` handler exists (static allow-list of intentionally-`none` commands). Prevents regression.

---

## 7. Test list

Unit (no TTY, `cargo test -p oc-tui`):
- `sanitize` per-construct: CSI SGR, CSI private `\x1b[?2004h`, `\x1b[2J`, OSC `\x1b]0;TITLE\x07`, OSC-ST `\x1b]8;;url\x1b\\`, DCS/PM/APC/SOS, charset `\x1b(B`, two-char `\x1b7`, bare ESC, C0/C1 sweep, CR→LF, tab/newline preserved.
- **Escape-injection fixture (UX-001):** `render_text_part`/`markdown::render` over `prefix\x1b[31mred\x1b[0m\x1b]0;TITLE\x07tail` + `\x1b[2J` + `\x1b[?2004h` → assert resulting span text contains **no** byte in `0x00-0x1F`/`0x7F`/`U+0080-009F` and no `ESC`.
- Preservation: CJK `日本語`, emoji `🚀`, ZWJ `👨👩👧👦`, combining `e\u0301`, RTL `مرحبا`, tabs — byte-identical through `sanitize` and through `render`/`to_ratatui`.
- Keymap reachability (§6). Theme/NO_COLOR: `Theme::for_terminal` under NO_COLOR/TERM=dumb/COLORTERM absent → no `Color::Rgb`.
- Endpoint contract: every path issued by `client.rs` exists in a route list (compact flagged).

Integration — PTY-driven (`tests/pty.rs`, spawn `script -qec` if present, else skip with a clear message; CI-safe):
- **First frame:** `opencode` under a pty (attach mode against a mock/stub server or in-process server) → captured output contains `\x1b[?1049h` (alt screen), raw mode active, and the home prompt placeholder renders.
- **Real session create/continue:** drive attach mode against a live `oc-server` (Agent 10) — send a prompt keystrokes through the pty, assert `POST /session` + `/session/:id/prompt` fired (server-side check) and the message list renders; then `--session <id>` and `--continue` resume the same session.
- **Restoration, normal exit:** send the exit key (q / ctrl+c) → output ends with `\x1b[?1049l` + cursor show + raw mode off (child's termios via `stty` from a shell on the pty or post-exit echo).
- **Restoration, signal:** `kill -TERM` and `kill -HUP` the child → restore sequences emitted before exit, clean status.
- **Restoration, panic:** run with `OPENCODE_TEST_CRASH=1` (env-gated panic in the render loop) → terminal restored, panic message printed.
- **Mini:** first frame + prompt→response loop + `--continue` replay, `--replay-limit`.
- **Piped stdin:** `echo "hi" | opencode` under a pty → home prompt prefilled with `hi` (assert via rendered buffer or subsequent submit reaching the server).
- `opencode </dev/null` with non-TTY stdout → nonzero exit + clear error (UX-004 regression).

---

## 8. Dependencies on other agents

- **Agent 10 (oc-server):** in-process `oc_server::server::listen` usable from oc-cli; real session create/prompt/SSE + persistence so bootstrap/create/continue work; add legacy `/session/:sessionID/compact` route. This plan defines the launch contract; Agent 10 owns the server edits.
- **Agent 11 (oc-client):** canonical client for `validateSession` preflight and the `SdkClient` adapter (oc-tui already depends on it). TUI works without it via `HttpSdkClient`, but attach preflight + no-duplicate-transport remediation need it.
- **Agent 02 (oc-cli):** owns the overall command surface/args; `run_default_tui(&Cli)` signature already passes everything the seam needs; coordinate the non-TTY error exit (UX-004) with Agent 02's run/console error conventions.
- **Agents 03/07/06/09:** server-side stores + runner + LLM are upstream prerequisites for real create/continue PTY tests; not code dependencies of oc-tui itself.
- oc-tui code lands independently (sanitize, terminal guard, keybinds, endpoint fix all compile standalone).

## 9. Risks

- **Piped-stdin/exit-code behavior change (UX-004):** `opencode </dev/null` in scripts that currently "succeed" silently will now error. Intended (matches reference), but must be called out in release notes; PTY + piped tests pin the new behavior.
- **Panic hook across Tokio threads:** emergency restore may run on a worker thread; raw-mode restore is process-global so it works, but stdout writes must use a locked handle and be failure-tolerant. Keep the hook install scoped to the TUI lifetime (installed/removed in `run_async`).
- **PTY test flakiness:** no PTY in some CI images; gate on `script` availability + `TERM`, skip loudly, keep unit coverage as the deterministic gate.
- **Windows:** ctrl+c guard + raw-mode restore differences on Console/CMD vs Windows Terminal are untestable here; cfg-gated, ship with a documented follow-up verification on Windows.
- **NO_COLOR fidelity:** naive Rgb→16-color mapping can look wrong on truecolor terms; only apply when a real indicator (NO_COLOR/TERM=dumb/COLORTERM absent) says so; keep default path untouched.
- **Mini-mode scope:** replay/resize/split-footer is a large surface; risk of Wave-4 slippage. Mitigation: land full TUI + attach first, treat mini as the final sub-item, allow it to ship behind the same seam without blocking the other blockers.
- **Sanitizer over-broadness:** must never touch style emission or authored ANSI; tests assert ratatui styles survive (a styled `**bold**` line still emits SGR from ratatui, content-only filtering).

## 10. Merge-order recommendation

**Wave 4**, after Agent 10 (server) and Agent 11 (client) and their upstreams (03/07) are green — the launch seam and PTY integration tests require a real server + canonical client. Suggested sequence within Wave 4, each independently mergable:

1. `util/sanitize.rs` + markdown/to_ratatui layers + unit tests (UX-001) — zero deps.
2. `terminal.rs` TerminalGuard + panic/signal/SIGCONT + Windows guard + tests (UX-002) — zero deps.
3. Keybind truthfulness + reachability test + dispatch toast (UX-003) — zero deps.
4. Endpoint fix (`/session/:id/compact`) — needs Agent 10 route decision.
5. oc-cli launch wiring for default TUI + piped-stdin + non-TTY error (UX-004, CLI-003) — needs Agent 10 in-process server + Agent 11 preflight.
6. `attach <url>` mode + auth headers — needs Agent 11.
7. `--mini` (`mini.rs`) — last, largest.

Blocking release-gate: items 1-5 (TUI launchable, escape-safe, restoration-safe, keymap truthful). Items 6-7 are parity completion.
