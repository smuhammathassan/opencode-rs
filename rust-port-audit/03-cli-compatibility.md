# Agent 03 — CLI Command and Behavior Compatibility

- Audit date: 2026-08-05
- Reference executable: `/root/.opencode/bin/opencode` (reports 1.18.13, Bun-compiled yargs CLI)
- Rust executable: `/root/opencode-rs/target/release/opencode` (reports 1.18.13, clap CLI)
- Method: black-box differential testing + source tracing in `crates/oc-cli` and `reference/packages/opencode/src`
- Deliverables: `COMMAND-COMPATIBILITY.csv` (148 rows), artifacts under `rust-port-audit/artifacts/03-*`

## Scope

Behavior and surface compatibility of the compiled Rust binary vs the reference binary:
binary/version/help text, command routing, flag parsing (unknown/missing/repeated flags,
aliases, defaults), positional args, stdout vs stderr placement, JSON output, exit codes,
shell completion, broken pipe, signals (SIGINT/SIGTERM), TTY detection, NO_COLOR,
unicode and invalid UTF-8 args, stdin piping, quiet/verbose (`--print-logs`, `--log-level`),
and "parses but not connected" commands.

## Repository areas inspected

- `crates/oc-cli/src/main.rs` (entry, help/version handling, `show()` mirror)
- `crates/oc-cli/src/cli/args.rs` (full clap surface)
- `crates/oc-cli/src/cli/cmd/*.rs` (all 26 subcommand handlers)
- `crates/oc-cli/src/cli/ui.rs`, `effect_cmd.rs`, `network.rs`, `models_dev.rs`, `error.rs`
- `reference/packages/opencode/src/index.ts`, `cli/ui.ts`, `cli/network.ts`,
  `cli/cmd/*.ts` (all), `cli/cmd/debug/index.ts`, `core/models-dev.ts`, `server/server.ts`

## Commands executed

Every top-level command was invoked against both binaries, most in multiple scenarios:
`--version/-v`, `--help/-h`, `help`, `completion`, `acp`, `mcp` (add/list/ls/auth/logout/debug),
`attach`, `run` (message, `--format json`, `--fork`, `--mini`, `--replay-limit`, `--command`,
`-v`, `--`, repeated flags, stdin, unicode, invalid UTF-8), `debug` (config/info/paths/
scrap/skill/snapshot/startup/v2/file read/list/search/wait), `providers`/`auth` (list/ls/logout/
login), `agent` (list/create), `upgrade`, `uninstall --dry-run`, `stats`, `export`, `import`,
`github`, `pr`, `session` (list/delete), `plugin`/`plug`, `db` (path/query/shell), `generate`,
`console` (login/logout/switch/orgs/open), `serve` (default/port variants), `web`.

## Runtime scenarios attempted

Version bytes; root + per-command help; unknown command/flag; missing/invalid option values
(`--log-level`, `--port`); repeated flags; `--` handling; NO_COLOR; TTY (pty) vs non-TTY;
stdin piping; unicode; invalid UTF-8; broken pipe; SIGINT/SIGTERM on `debug wait`;
long-running `serve`/`web`/default-TUI; `--print-logs`; `--get-yargs-completions`; env
overrides (`OPENCODE_MODELS_PATH`, `OPENCODE_DISABLE_MODELS_FETCH`); model-cache pre-population.

## Architecture or behavior summary

- Reference is a yargs CLI (`.strict()`, `.populate--`, `.completion("completion")`,
  `.help("help","h")`, `.version("version","v")`) with a custom `.fail()` handler that prints
  the logo + full help on unknown args, and a `show()` helper that writes help to **stderr**
  (logo + text) unless output already begins with `opencode `. CLI errors exit non-zero via
  `process.exit()`; many handlers `return` on error, leaving exit code 0 (e.g. TUI
  `chdir` failure, `providers logout` with no credentials).
- Rust is a clap CLI. Parse errors go through `render_help()` / `err.print()`; global options
  are declared `global=true`; `disable_help_subcommand=true`, `disable_version_flag=true`
  with manual `--version`/`-v` handling.
- The Rust binary routes **26 subcommands**, but 15+ are stubs returning
  `not_wired("...not yet wired in this build (TODO(integration): ...)")`; `rg -c "not_wired"`
  shows 47 call sites and 73 `TODO(integration)` markers under `crates/oc-cli/src/`.
- Reference uses stdout for table/box UIs (`┌ ─ ─ ┐`) and the TUI; Rust `ui::println` writes
  everything to **stderr** — a systematic stream divergence.
- Rust `ui::error` emits a literal extra space (`Error:  ` vs reference `Error: `), and
  `ui::println` inserts spaces between style/text segments (` Models cache refreshed `),
  differing from reference's pre-concatenated strings.

## Positive observations

- `--version` / `-v` are byte-identical (`1.18.13\n`) and exit 0.
- `db path` output is byte-identical (path + newline, exit 0).
- Exit codes match for nearly all parse-failure scenarios (unknown flag, missing value,
  invalid choice, missing required positional) — both exit 1 (reference 0 only for some
  handler-level "return without error" paths).
- `run -v` and `run --version` match (version, exit 0).
- Signal handling on `debug wait` is identical (SIGINT → killed, SIGTERM → killed).
- NO_COLOR is ignored identically by both binaries (ANSI retained in both).
- `--replay-limit` / `--mini` / `--demo` / `--fork` guard messages (modulo the double space)
  match the reference wording and exit codes.
- Logo wordmark in `--help` is identical; help is routed to stderr in both.

## Findings summary

| ID | Command/Scenario | Category | Severity |
|----|------------------|----------|----------|
| CLI-001 | `run` (any message) | NOT_WIRED — reference runs full LLM session | Critical |
| CLI-002 | `run --format json` | NOT_WIRED — reference streams JSON events | Critical |
| CLI-003 | default TUI command (no args, TTY or not) | NOT_WIRED — reference launches TUI | Critical |
| CLI-004 | `serve` / `web` | NOT_WIRED — reference serves real HTTP; rust binds socket/prints placeholder | High |
| CLI-005 | `mcp` (add/list/auth/logout/debug) | NOT_WIRED — reference reads/writes MCP config | High |
| CLI-006 | `session list/delete`, `db <query>`, `stats`, `export`, `import` | NOT_WIRED — reference queries real DB | High |
| CLI-007 | `completion` | NOT_WIRED — reference emits bash script | High |
| CLI-008 | `generate`, `plugin`, `agent`, `github`, `console` | NOT_WIRED | High |
| CLI-009 | `debug config/scrap/skill/snapshot/v2/file search` | NOT_WIRED | High |
| CLI-010 | `debug startup` | OUTPUT_DIFFERENCE — rust prints literal `0.000` | Medium |
| CLI-011 | `models` / `models --verbose` / `models <provider>` | OUTPUT_DIFFERENCE — catalog data + JSON shape diverge | Medium |
| CLI-012 | stdout vs stderr for table/box UIs | STDERR_DIFFERENCE — rust sends boxes to stderr | Medium |
| CLI-013 | `ui::error` double space / `ui::println` padding | STDERR_DIFFERENCE — all error lines differ | Low/Medium |
| CLI-014 | `help` subcommand | MISSING_COMMAND — ref `opencode help` works; rust treats as path | High |
| CLI-015 | `version`/unknown-command as project path, exit 0 vs 1 | EXIT_CODE_DIFFERENCE | Medium |
| CLI-016 | broken pipe on `models` | EXIT_CODE_DIFFERENCE — rust prints error, exit 1 | Medium |
| CLI-017 | invalid UTF-8 arg | PARSING_DIFFERENCE — ref accepts, clap rejects | Low |
| CLI-018 | repeated flags (`-m -m`, `--format --format`) | PARSING_DIFFERENCE — yargs last-wins vs clap reject | Medium |
| CLI-019 | `serve` default port 4096 vs random; `--port abc` | DEFAULT_DIFFERENCE / PARSING_DIFFERENCE | Medium |
| CLI-020 | `--log-level BOGUS --version` exit 0 vs 1 | EXIT_CODE_DIFFERENCE | Medium |
| CLI-021 | `--print-logs` silently ignored | SILENT_FLAG | Medium |
| CLI-022 | `debug file read/list` no path-escape guard | EXIT_CODE_DIFFERENCE (security-relevant) | High |
| CLI-023 | root help: clap layout, leaked GlobalArgs doc-comment, no `[default]` marker | OUTPUT_DIFFERENCE | Low |
| CLI-024 | `upgrade 1.2.3` | SIDE_EFFECT_DIFFERENCE — ref installs; rust refuses | High |
| CLI-025 | `--get-yargs-completions` | MISSING_COMMAND | Medium |

## Detailed findings

### [CLI-001/002/003] Flagship paths are NOT_WIRED — Critical, release blocker
- `opencode run hello` reference starts an in-process server, creates a session, calls the
  `opencode/big-pickle` model, and prints `Hello! How can I help you today?` (exit 0).
  Rust: `Error:  the in-process opencode server is not wired yet in this build
  (TODO(integration): oc-server)` (exit 1) — `crates/oc-cli/src/cli/cmd/run/client.rs:67`.
- `opencode run --format json hello` reference streams `step_start`/`text`/`step_finish` NDJSON
  to stdout (exit 0). Rust: same NOT_WIRED error.
- The default command `opencode` reference launches the real TUI (renders frames even with no
  TTY). Rust prints `opencode: starting TUI (requires a TTY)` and exits 0 —
  `crates/oc-cli/src/cli/cmd/attach.rs:164-170` (`run_default_tui`). This is the primary
  user-facing product surface; none of it functions.

### [CLI-004] `serve`/`web` are cosmetic shells
- `serve` binds a bare TCP socket (reads bytes, serves nothing) then blocks on `pending()`
  (`serve.rs:40-62`) — reference runs the full HTTP server. Default port: reference resolves
  0 → 4096 (`reference/.../server/server.ts:120-121`); rust binds OS-random. `web` binds a
  listener, drops it, prints `(web interface not yet wired)` and exits 0 (`web.rs:81`).

### [CLI-005..CLI-009] NOT_WIRED inventory (all return exit 1 vs reference's real behavior)
- `completion` (`completion.rs:12`), `mcp add` (`mcp.rs:55`), `mcp list` (`mcp.rs:66`),
  `mcp auth` (`mcp.rs:78`), `mcp auth list` (`mcp.rs:73`), `mcp logout` (`mcp.rs:85`),
  `mcp debug` (`mcp.rs:92`), `attach <url>` (`attach.rs:74`), `attach --mini` (`attach.rs:86,123`),
  `agent create/list` (`agent.rs:33,41`), `db <query>` (`db.rs:38`), `db` shell (`db.rs:41`),
  `session list/delete` (`session.rs:12,17`), `stats` (`stats.rs:266`), `export`
  (`export_cmd.rs:11`), `github install/run` (`github.rs:20,27`), `plugin` (`plug.rs:19`),
  `generate` (`generate.rs:9`), `console` all subcommands (`console.rs:21`),
  `debug config/lsp/rg/file-search/scrap/skill/snapshot/agent/v2`
  (`debug.rs:42,51,59,71,103,109,118,131,137`).
- Reference exit codes for the same invocations: `mcp list`=0, `mcp add`=0, `session list`=0,
  `db select 1`=0, `export <id>`=0, `generate`=0, `console logout`=0, `plugin`=0,
  `completion`=0, `debug config`=0, etc. The Rust returns 1 in every case, so
  NOT_WIRED rows almost always carry an EXIT_CODE_DIFFERENCE too.

### [CLI-010] `debug startup` prints a meaningless value
`debug.rs:123-125` prints `started_at().elapsed()` where `started_at()` is lazily initialized
at first call (inside the handler) → `0.000`. Reference prints real process startup in ms
(measured ~1445 in this environment). `debug info` also differs (`os:` format and content:
`linux Ubuntu 24.04.4 LTS x86_64` vs `Linux 6.8.0-90-generic x64`), and `debug paths` prints
`home` last instead of first.

### [CLI-011] `models` data and schema divergence
The Rust `ModelsDev` (`models_dev.rs:46-60`) reads `~/.cache/opencode/models.json` and, when
absent, fetches `https://models.opencode.ai/api.json` (network reachable here). It lists all
180 providers raw. The reference's ProviderV2 registry schema-validates the catalog and
rejects the current api.json (its own malformed-entry test dies on `Z.limit.context`), so it
lists only 8 builtin `opencode/*-free` models. Consequences: `models` output,
`models --verbose` (JSON shape: reference normalized Model object vs rust raw entry), and
`models anthropic` (ref "Provider not found" exit 1 vs rust exit 0) all diverge. Setting
`OPENCODE_MODELS_PATH` to a valid 180-provider cache does not change the reference result.
Root cause is ambiguous (reference schema too strict for current data vs port lacking
validation); recorded as an observable OUTPUT_DIFFERENCE.

### [CLI-012/013] Stream placement and text formatting
Reference table boxes (`providers list`, `providers logout`, `uninstall --dry-run`,
`mcp add/list`, `stats`) go to **stdout**; the Rust emits them to **stderr** via
`ui::println` (`ui.rs:28-36`). Reference `UI.error` = `"Error: " + msg` (`ui.ts:125`); Rust
`ui::error` inserts an extra space (`ui.rs:47-53`), producing `Error:  <msg>` on every error.
`ui::println` splits style and text into separate joined segments, adding padding
(` Models cache refreshed ` vs `Models cache refreshed`). These are systemic and appear in
nearly every interactive/error output.

### [CLI-014/015] `help`/`version`/unknown-command semantics
Reference yargs registers a `help` command (prints help to **stdout**, exit 0) and treats any
unknown bare word as the TUI `[project]` positional (chdir fails → `Failed to change directory
to /root/<word>` with exit **0**, per `tui.ts:203-206`). Rust sets
`disable_help_subcommand=true` (`args.rs:88`), so `opencode help` is treated as a project path
and exits 1, and unknown commands exit 1 with an extra `Unexpected error` + OS error
(`attach.rs:148-154` → `dispatch` `Err` → `main.rs` exit 1).

### [CLI-016/017/018] Streams, encoding, and repeated flags
- Broken pipe (`models | head -1`): reference exits 0 silently; Rust prints
  `Error:  Unexpected error / Broken pipe (os error 32)` and exits 1.
- Invalid UTF-8 arg: reference accepts (runs the session); clap rejects with
  `error: invalid UTF-8 was detected in one or more arguments`, exit 1.
- Repeated flags (`run -m a/b -m c/d`, `run --format json --format default`): yargs
  last-wins and proceeds to its own message validation; clap rejects duplicates.

### [CLI-019/020/021] Defaults, validation ordering, ignored flags
- `serve` default port: reference 4096, rust random (`DEFAULT_DIFFERENCE`).
- `--log-level BOGUS --version`: reference short-circuits to version (exit 0); rust validates
  choices first (exit 1).
- `--print-logs`: reference emits INFO logs to stderr; rust accepts the flag with no effect.

### [CLI-022] `debug file read/list` lacks the location guard
Reference rejects out-of-project paths (`Path escapes the location`, exit 1). Rust
(`debug.rs:73-97`) reads/lists any path with no guard and exits 0 — a security-relevant
behavioral divergence.

### [CLI-023] Root help
Rust root help leaks the `GlobalArgs` doc comment as the top-level description, omits the
`[default]` marker on the TUI command, and reorders/labels options differently
(clap `Usage:`/`Arguments:` vs yargs command list). The logo and overall option set match;
formatting does not. Every subcommand `--help` likewise differs in layout while exposing an
equivalent option set.

### [CLI-024/025] Upgrade and completion protocol
`upgrade 1.2.3` reference performs a real curl install (exit 0); rust reports
`Using method: unknown` and `automatic upgrades are not supported` (exit 0, no side effect).
`--get-yargs-completions ...` (yargs completion protocol used by `completion` output) is
unimplemented; `completion` itself is NOT_WIRED.

## Feature or behavior gaps

1. `run`, default TUI, `attach` — the core product loop (LLM, server, TUI) is entirely
   unwired; all three are release blockers.
2. `serve`/`web` do not serve HTTP.
3. MCP, sessions, db, stats, export/import, agent, github, console, plugin, generate,
   completion, and most `debug` subcommands are stubs (73 `TODO(integration)` markers).
4. stdout/stderr placement, `ui::error` spacing, `ui::println` padding — systemic formatting
   and stream divergence.
5. Broken pipe handling, `help`/`version` subcommands, repeated-flag semantics,
   invalid-UTF-8 tolerance, `--print-logs`, `--get-yargs-completions` protocol,
   `serve` default port, `debug file` path guard, `debug startup` value.

## Test coverage gaps

- No end-to-end `run` test (LLM loop), no server test (HTTP respond), no TUI test.
- No golden tests comparing help text; the leaked doc-comment would have been caught.
- No tests for stream placement (stdout vs stderr), error spacing, broken pipe,
  invalid UTF-8, repeated flags, or signal handling.
- `models` has no test asserting catalog output shape or reference-compatible schema handling.

## Unverified areas

- Actual `run` JSON event schema parity beyond the first events (rust cannot run).
- `export`/`import` JSON payload parity (rust not wired).
- Interactive prompts (`providers login` device flow, `console login`, `mcp auth`) could not
  be completed; only initial TTY frames compared.
- `upgrade`/`uninstall` real destructive paths were not exercised (side effects; dry-run only).
- Reference `models` catalog-load failure root cause (schema vs network) is not fully isolated;
  marked MEDIUM confidence.
- ACP bridge behavior unverified (both exit 0 with no output).

## Final domain verdict

**NOT RELEASE READY.** The Rust CLI parses the same command/flag surface and matches exit
codes on most pure parse-failure cases, but the primary product paths (`run`, default TUI,
`serve`, `web`, `attach`) are unwired stubs, and roughly half of the commands return
`not yet wired` errors where the reference performs real work with exit 0. Systematic
stdout/stderr and text-formatting divergence affects every interactive/error output.

Severity counts (from CSV): Critical 9, High 29, Medium 33, Low 64, Informational 13 —
of 148 rows (17 equivalent, 11.5%). `COMMAND-COMPATIBILITY.csv` (148 rows) written to
`rust-port-audit/COMMAND-COMPATIBILITY.csv`; evidence under
`rust-port-audit/artifacts/03-cli/*.json` and `03-*`.
