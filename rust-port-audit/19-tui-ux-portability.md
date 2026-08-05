# Agent 19 — TUI, Terminal UX, Accessibility, and Platform Portability

Auditor of the `opencode-rs` Rust port's user-facing terminal behavior. Read-only
review of `crates/oc-tui`, `crates/oc-cli` (TUI launch/attach wiring), and
`crates/oc-client`/`crates/oc-server` (client/server surfaces the TUI depends on),
against the vendored reference `reference/packages/tui` + `cli/cmd/tui.ts` and the
black-box binary `/root/opencode-rs/target/release/opencode`.

## Scope

- TUI launch (`opencode` no args, `--mini`, `opencode attach <url>`), non-interactive fallback.
- Rendering, input handling, keybindings (~183 bindings), dispatch completeness.
- Mouse, resize, bracketed paste, focus events, alternate screen, raw mode, terminal
  restoration (panic/SIGINT/SIGTERM).
- Unicode / wide / combining / RTL, small & large terminals, scrollback, color/truecolor,
  NO_COLOR.
- Terminal escape sanitization (ANSI/OSC from model output).
- Progress indicators, frozen-UI risk, error messages, log readability, Windows/remote/tmux/SSH.
- `SdkClient` (own HTTP client) vs `oc-client`, and server-event wiring.

## Repository areas inspected

- `crates/oc-tui/src/{app,client,keybind,keymap,config,sync,theme,types,local,logo,lib}.rs`
- `crates/oc-tui/src/components/{message,prompt,permission,question,dialog,toast,spinner,text}.rs`
- `crates/oc-tui/src/prompt/{input,parts,autocomplete,history,state}.rs`
- `crates/oc-tui/src/util/{markdown,display,text,locale,format,path_format}.rs`
- `crates/oc-tui/tests/rendering.rs`
- `crates/oc-cli/src/{main.rs}`, `cli/{cmd/attach.rs,cmd/mod.rs,effect_cmd.rs,ui.rs,args.rs}`
- `crates/oc-client/src/{client.rs,transport.rs,sse.rs}` (comparison only)
- `crates/oc-server/src/router.rs` (endpoint set the TUI client targets)
- Reference: `reference/packages/opencode/src/cli/cmd/tui.ts`, `cli/tui/layer.ts`,
  `cli/ui.ts`, `index.ts`, `packages/tui/src/{app.tsx,routes/session/index.tsx}`
- Third-party: `~/.cargo/registry/.../ratatui-0.29.0/{buffer,text/span,text/line,terminal}`

## Commands executed

```
timeout 5 ./target/release/opencode </dev/null                # EXIT=0, "opencode: starting TUI (requires a TTY)"
timeout 6 script -qec "./target/release/opencode </dev/null"  # EXIT=1 "Error: the TUI is not yet wired..."
timeout 6 script -qec "./target/release/opencode --mini </dev/null"
timeout 6 script -qec "./target/release/opencode attach http://localhost:3000 </dev/null"
./target/release/opencode --version                           # 1.18.13, EXIT=0
./target/release/opencode --help                              # EXIT=0
./target/release/opencode run "hi" </dev/null                 # "in-process opencode server is not wired"
cargo test -p oc-tui                                          # 150 unit + 4 integration, all pass
CARGO_TARGET_DIR=... /tmp/opencode/esc-test esc-test          # RUNTIME escape-passthrough proof
```

Artifacts saved: `rust-port-audit/artifacts/19-tui-ux-portability/`
(`opencode-script.log`, `mini.log`, `attach.log`, `ratatui-escape-passthrough.txt`).

## Runtime scenarios attempted

| Scenario | Result |
|---|---|
| `opencode` no args, real pty (`script`) | FAIL — `Error: the TUI is not yet wired in this build (TODO(integration): oc-tui)`, exit 1 |
| `opencode` no args, stdin closed | PARTIAL — prints dim "opencode: starting TUI (requires a TTY)", exit 0, never starts anything |
| `opencode --mini` (pty and non-tty) | FAIL — "mini interactive mode is not yet wired", exit 1 |
| `opencode attach http://localhost:3000` | FAIL — "attaching the TUI to a running server is not yet wired", exit 1 |
| `opencode --version` | PASS — `1.18.13`, exit 0 |
| `opencode --help` | PASS — logo + command list, exit 0 |
| Interactive TUI (keystrokes, resize, mouse) | BLOCKED — TUI cannot be launched; all interactive checks based on static analysis |

No test TUI processes were left running by this agent. (Other auditors' `serve` processes
were observed and left untouched.)

## Architecture or behavior summary

- `oc-cli` `cmd::dispatch` routes the no-arg default to `attach::run_default_tui`
  (`oc-cli/src/cli/cmd/attach.rs:91`). That function returns
  `Err(not_wired("the TUI is not yet wired in this build (TODO(integration): oc-tui)"))`
  at `attach.rs:168-170`. `--mini` returns `not_wired(...)` at `attach.rs:122-124`;
  `attach` at `attach.rs:74`. The reference launches the TUI and provides the whole app
  node in-process (`reference/packages/opencode/src/cli/tui/layer.ts:1-8`).
- The `oc-tui` crate is substantial and self-contained: a 2995-line `App` (`app.rs`),
  a leader-chord keymap engine (`keymap.rs`, ~40 unit tests), 183 keybind definitions
  (`keybind.rs`), a markdown→styled-lines renderer (`util/markdown.rs`), an event
  reducer (`sync.rs`), and an own HTTP+SSE client (`client.rs`). `app::run_async`
  (`app.rs:2752`) is exported but has **no caller anywhere in the workspace** — the
  entire runtime is dead code behind the cli seam.
- The TUI's client (`HttpSdkClient`, `client.rs:134`) is a hand-rolled reqwest client
  that does **not** use the `oc-client` crate (declared dep, unused). Its non-prefixed
  paths match `oc-server/src/router.rs` (e.g. `/global/event`, `/session/:id/message`,
  `/permission/:id/reply`), so it *would* talk to the Rust server, but the wiring never
  runs. One endpoint mismatch found: `/session/:id/compact` is registered only under
  `/api/...` in `oc-server/src/router.rs:321-322`, while the TUI calls
  `/session/{id}/compact` (`client.rs:359-371`).

## Positive observations

- Keymap parser/matcher is thorough and well-tested (alternatives, object form,
  `none`/`false` disable, `<leader>` chords, timeout expiry, priority groups,
  ctrl-uppercase normalization, BackTab/Shift+Tab) — `keymap.rs:155-588`, tests pass.
- Unicode display-width discipline: wrapping (`util/markdown.rs:620-661`),
  padding (`components/text.rs:28-43`), prompt layout (`components/prompt.rs:54-56`)
  all use `unicode-width`; test asserts CJK width.
- Small-terminal math is saturating everywhere (`app.rs:2218, 2320-2323, 2361-2365`).
- Resize, bracketed paste, and config-gated mouse are wired (`app.rs:797-800, 2839-2842`).
- Spinner animation + 80 ms redraw tick provide progress feedback (`app.rs:2496-2497, 2884`).
- CLI error diagnostics are actionable, e.g. `run` suggests `opencode run --attach <url>`.
- 150 unit + 4 integration tests pass (`cargo test -p oc-tui`).

## Findings summary

| ID | Severity | Confidence | Finding |
|---|---|---|---|
| UX-01 | Critical | CONFIRMED (RUNTIME) | `opencode` does not launch the TUI; `--mini` and `attach` also error "not yet wired" |
| UX-02 | High | CONFIRMED (RUNTIME) | Model output ANSI/OSC escape sequences are written verbatim to the terminal (escape injection) |
| UX-03 | High | CONFIRMED (STATIC) | No terminal restoration on panic/SIGTERM/SIGHUP; raw mode + alt screen left enabled |
| UX-04 | High | CONFIRMED (STATIC) | 58 keybind commands dispatch to no-ops (diff viewer, which-key, terminal suspend, MCP/plugin dialogs…) |
| UX-05 | Medium | CONFIRMED (RUNTIME) | Non-TTY fallback misleading: "starting TUI (requires a TTY)" + exit 0; reference reads piped stdin as prompt |
| UX-06 | Medium | CONFIRMED (STATIC) | Truecolor assumed unconditionally; no NO_COLOR/COLORTERM handling; light theme is a palette stub |
| UX-07 | Medium | CONFIRMED (STATIC) | oc-tui duplicates the client transport instead of using oc-client; not connected to any server |
| UX-08 | Medium | CONFIRMED (STATIC) | Event/bootstrap wiring real but never exercised; no integration tests against a live server; endpoint mismatch `/session/:id/compact` |
| UX-09 | Low | CONFIRMED (STATIC) | Focus events unhandled; mouse minimal; no Windows Ctrl+C guard (reference has `win32InstallCtrlCGuard`) |
| UX-10 | Low | CONFIRMED (STATIC) | Full message list + keymap rebuilt every frame — jank risk on large sessions |
| UX-11 | Low | CONFIRMED (STATIC) | No RTL/bidi shaping; emoji/combining untested at terminal level |
| UX-12 | Informational | CONFIRMED (STATIC) | Snapshot tests are assertion-based, not golden diffs; none touch the terminal |

## Detailed findings

### UX-01 — Critical — The TUI is never launched (CONFIRMED, RUNTIME)

Under a real pty (`script -qec "./target/release/opencode </dev/null"`):
`Error: the TUI is not yet wired in this build (TODO(integration): oc-tui)`, exit 1.
`--mini` → "mini interactive mode is not yet wired" (`attach.rs:122-124`).
`attach <url>` → "attaching the TUI to a running server is not yet wired"
(`attach.rs:74`). `run` is likewise blocked on `oc-server`. The exported
`oc_tui::app::run_async` (`app.rs:2752`) has zero callers in the workspace; only
`oc-tui/src/lib.rs:19` re-exports it. The reference on no-args starts the in-process
app node and connects the TUI over its RPC/SSE loop
(`reference/.../cli/tui/layer.ts`, `cli/cmd/tui.ts`). Until the `oc-cli` seam is
implemented, no end-user TUI UX exists to evaluate.

### UX-02 — High — Terminal escape injection from model output (CONFIRMED, RUNTIME)

The markdown renderer performs no control-character sanitization
(`util/markdown.rs:88-174`, `inline()` at 255-336 pushes every char including `\x1b`
into spans). Message, reasoning, tool-output, and permission text all flow into
ratatui `Span`s via `Paragraph` (`app.rs:2395-2416`, `components/message.rs:384-424`).
ratatui 0.29's `Span::render_ref` writes graphemes to cells via `set_symbol` with **no**
control-char filter (`ratatui-0.29.0/src/text/span.rs:427-471`); only
`Buffer::set_stringn` filters control chars (`buffer.rs:346`), and that path is not used
for Paragraph/Line rendering. Runtime proof (scratch harness using the exact
`Terminal::draw`+flush path, artifact `ratatui-escape-passthrough.txt`):
`prefix\u{1b}[31mred\u{1b}[0m\u{1b}]0;TITLE\u{7}tail` — both an SGR color sequence and
an OSC title-change sequence were emitted verbatim. A model/plugin/tool output
containing `\x1b[2J`, `\x1b]0;…`, `\x1b[?2004h`, etc. will be interpreted by the
terminal. Reference comparison UNVERIFIED (`@opentui/solid` `<markdown>` widget not
vendored); recommend sanitizing or escaping control chars at span-build time regardless.

### UX-03 — High — No terminal restoration on panic/signal (CONFIRMED, STATIC)

Raw mode and the alternate screen are entered in `run_async` (`app.rs:2754-2755`) and
only restored on the happy path (`app.rs:2762-2764`). There is no panic hook, no `Drop`
guard, and no SIGTERM/SIGHUP/SIGINT handler in `oc-tui` or `oc-cli`. In raw mode Ctrl+C
is a key event (handled by `input_clear`/`app_exit`, `keybind.rs:592` and `keybind.rs:36-40`),
so an in-app exit is safe, but `kill <pid>`, an SSH drop, or a tmux/terminal close leaves
the terminal in raw + alt-screen mode. The reference delegates restoration to its
terminal runtime (ink/OpenTUI). RUNTIME verification BLOCKED (TUI not launchable).

### UX-04 — High — 58 keybind commands dispatch to no-ops (CONFIRMED, STATIC)

183 bindings are defined (`keybind.rs`, 43 default to `"none"`). Programmatic cross-check
of every default-enabled binding's command against the `App::dispatch` match
(`app.rs:915-1167`) found 58 commands with **no handler** — they fall through to
`tracing::debug!("unhandled command")` (`app.rs:1158-1160`). Those with real default
keys include: `diff.close` (escape,q), `diff.toggle` (enter/space), `diff.expand/collapse`
(left/right), `diff.next/previous_file` (n/p), `diff.next/previous_hunk` (]/[), and the
whole `which-key.*` (ctrl+alt+…) family, `terminal.suspend` (ctrl+z),
`model.dialog.favorite` (ctrl+f), `model.dialog.provider` (ctrl+a), `session.pin.toggle`
(ctrl+f), `stash.delete` (ctrl+d), `plugins.*`/`dialog.*` (space, shift+i, ctrl+d/m/r).
`Route` has only `Home`/`Session` (`app.rs:40-43`) — the diff viewer, which-key, MCP
dialog, and console that these commands imply are not ported. Pressing these keys will
silently do nothing.

### UX-05 — Medium — Misleading non-TTY fallback (CONFIRMED, RUNTIME)

When stdout is not a terminal, `attach.rs:164-167` prints the dim message
`opencode: starting TUI (requires a TTY)` and returns exit 0 — nothing is started and
stdin is ignored. The reference instead reads piped stdin and uses it as the initial
prompt (`reference/packages/opencode/src/cli/cmd/tui.ts:60`: `process.stdin.isTTY ?
undefined : await Bun.stdin.text()`). `echo "hi" | opencode` and `opencode </dev/null`
in CI therefore succeed silently with no work performed — a foot-gun for scripting.

### UX-06 — Medium — Color assumptions: truecolor + no NO_COLOR (CONFIRMED, STATIC)

`theme.rs:119-156` hardcodes `Color::Rgb` (24-bit) for every palette entry. There is no
`NO_COLOR`, `CLICOLOR`, `COLORTERM`, `TERM`, or color-depth detection anywhere in the
workspace (grep over `crates/` = 0 hits). The CLI banner also emits ANSI unconditionally
to stderr even when redirected (`cli/ui.rs:19-48`, observed in runtime logs), which does
match the reference (`reference/.../cli/ui.ts` uses raw escapes with no NO_COLOR check —
grep over reference = 0 hits), so this is parity, but it remains a portability/accessibility
limitation: on 8/16/256-color or `TERM=dumb` terminals the TUI will emit 24-bit sequences
that are misrendered or garbage. `Theme::light()` merely clones the dark palette and flips
a mode flag (`theme.rs:58-62`; TODO at `theme.rs:7`).

### UX-07 — Medium — Duplicated client; not connected (CONFIRMED, STATIC)

`oc-tui` declares `oc-client` as a dependency (`Cargo.toml:21`) but never uses it; it
implements its own `HttpSdkClient` (`client.rs:1-9, 134-152, 207-591`). The trait-based
design is sound (`SdkClient`, `client.rs:27-77`) and the non-prefixed endpoint set aligns
with `oc-server/src/router.rs` (`/global/event`, `/session/:id/*`, `/permission/:id/reply`,
`/question/:id/reply`, `/find`, `/experimental/*`), but nothing ever instantiates it in a
running process. `MockSdkClient` (`client.rs:685-816`) supports headless tests only.

### UX-08 — Medium — Server-event wiring real but unexercised (CONFIRMED, STATIC)

The event pipeline is genuine: SSE reconnection with backoff (`client.rs:208-234`,
`SseParser` at 635-682, tests at 836-858), the `sync` reducer applying session/message/
permission events (`sync.rs:110+`), bootstrap fan-out (`app.rs:2719-2749`). But it is
never exercised against a live server, and the single endpoint mismatch
(`/session/:id/compact` missing from the non-`/api` server routes,
`oc-server/src/router.rs:321-322` vs `client.rs:359-371`) shows the two halves were not
integration-tested together. No test spins up `oc-server` and drives the TUI.

### UX-09 — Low — Focus/mouse/Windows gaps (CONFIRMED, STATIC)

`Event::FocusGained`/`FocusLost` are not matched and fall to `_ => false`
(`app.rs:805`). Mouse supports only scroll (±3 lines) and left-click tool toggle
(`app.rs:848-884`). No Windows Ctrl+C guard — the reference imports
`win32InstallCtrlCGuard` (`tui.ts:47`); crossterm 0.28 raw-mode behavior on Windows
Console/CMD differs from Windows Terminal, and this build has no mitigation.
Windows/SSH/tmux behavior is otherwise delegated to crossterm; RUNTIME BLOCKED here.

### UX-10 — Low — Per-frame full re-render (CONFIRMED, STATIC)

Every frame re-renders the complete message list (`app.rs:2285-2303`,
`render_messages`) and rebuilds all keymap groups (`app.rs:2157 → 2157`); with
`MAX_MESSAGES = 100` (`sync.rs:43`) the ceiling is bounded, but long multi-tool sessions
will produce measurable per-frame latency → cursor/keypress lag. No incremental render
or dirty-region optimization.

### UX-11 — Low — RTL/emoji untested (CONFIRMED, STATIC)

No bidi handling anywhere (parity risk unknown; reference TUI relies on OpenTUI which
also does no bidi shaping). Emoji ZWJ/combining sequences are delegated to
`unicode-width` + ratatui graphemes but have no test coverage and could not be verified
on a real terminal (BLOCKED).

### UX-12 — Informational — Test coverage limits (CONFIRMED, STATIC)

`cargo test -p oc-tui`: 150 unit + 4 integration, all green. The integration tests
(`tests/rendering.rs:117-185`) render in-memory `StyledLine`s from fixed fixtures and
assert substrings — not golden diffs, and they exercise neither the terminal event loop,
keyboard dispatch, terminal lifecycle (raw/alt screen), resize/mouse/paste, nor
panic/signal restoration.

## Feature or behavior gaps

- TUI launch (`run_default_tui`), `--mini`, `attach` — all `not_wired` (UX-01).
- Diff viewer, which-key, MCP/plugin/console UI surfaces and their keybinds (UX-04).
- Piped-stdin-as-initial-prompt behavior (UX-05).
- NO_COLOR / color-depth / light-theme fidelity (UX-06).
- Panic/signal terminal restoration (UX-03).
- Windows Ctrl+C guard, focus events (UX-09).

## Test coverage gaps

- No test starts `oc-server` and drives the TUI end-to-end (bootstrap + SSE).
- No test feeds an ANSI/OSC-laden text part through the render path (UX-02 regression).
- No test kills the TUI process mid-run and asserts terminal restoration (UX-03).
- No test maps each default keybind to a dispatch handler (UX-04).
- No golden render diffs; no terminal-output byte-level snapshots; no
  wide/combining/emoji/RTL fixtures.

## Unverified areas

- Interactive TUI behavior (keystrokes, mouse, resize, paste) — BLOCKED (no launch path).
- Reference-side ANSI sanitization (`@opentui/solid` markdown widget) — BLOCKED (not vendored).
- Windows Terminal/CMD/Console and macOS iTerm2 behavior — BLOCKED (Linux only).
- RTL bidi rendering, emoji ZWJ, combining marks at the terminal — BLOCKED.

## Final domain verdict

**NOT_READY.**

`opencode` (no args), `opencode --mini`, and `opencode attach <url>` all fail with
"not yet wired" errors (RUNTIME-confirmed) — the TUI is never launched. The underlying
crate is a solid, well-tested foundation (keymap engine, markdown layout, event reducer,
client), but it is dead code behind an unimplemented `oc-cli` seam, ships 58 no-op
keybind commands, has RUNTIME-confirmed terminal-escape passthrough, and lacks panic/
signal terminal restoration. These must be remediated before the seam is closed.

Severity counts: Critical 1, High 3, Medium 4, Low 3, Informational 1 (per summary table).
