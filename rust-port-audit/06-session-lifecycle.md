# Agent 06 — Session Lifecycle and Application State

Audit of session creation/lifecycle in the opencode-rs Rust port against the
vendored TS reference (v1.18.13) and the stock reference binary
(`/root/.opencode/bin/opencode`). Rust commit `e7fc33e`. READ-ONLY on
production source; all runtime tests used disposable data dirs under
`/tmp/oc-audit-06` and mock servers under `rust-port-audit/artifacts/06/`.

## Scope

Creation, loading, listing, update, deletion, resume, fork, parent-child,
message storage/ordering, parts, tool-call state, streaming, interruption,
retry, cancellation, recovery after process termination, import/export,
compaction, token limits, title generation, provider/model changes,
concurrency, stale state, invalid identifiers, missing sessions, ownership.

## Repository areas inspected

- `crates/oc-cli/src/cli/cmd/run/` — `mod.rs`, `client.rs`, `events.rs`,
  `types.rs` (the only wired session-facing code path)
- `crates/oc-cli/src/cli/cmd/session.rs`, `export_cmd.rs`, `import_cmd.rs`,
  `serve.rs`, `attach.rs`, `effect_cmd.rs`, `args.rs`
- `crates/oc-session/` — `session.rs`, `service.rs`, `store.rs`, `history.rs`,
  `identifier.rs`, `v2.rs`, `compaction.rs`, `compaction_core.rs`, `summary.rs`,
  `retry.rs`, `run_state.rs`, `overflow.rs`
- `crates/oc-session-runner/` — `run_coordinator.rs`, `execution.rs`,
  `runner/llm.rs`, `retry.rs`, `session/services.rs`, `session/schema.rs`
- `crates/oc-database/` — `database.rs`, `tables.rs`, `schema.rs`, migrations
- `crates/oc-server/` — `server.rs`, `state.rs`, `router.rs`,
  `handlers/session.rs`, `handlers/message.rs`, `instance_handlers.rs`
- Reference: `packages/opencode/src/cli/cmd/session.ts`, `run.ts`, `export.ts`,
  `import.ts`, `serve.ts`; `packages/opencode/src/session/session.ts`
- Runtime oracle: reference binary `serve`, `session list`, `export`, `import`,
  `session delete` against a disposable `XDG_DATA_HOME`

## Commands executed

All with `OPENCODE_DATA=/tmp/oc-audit-06/data` (Rust) or `XDG_DATA_HOME=/tmp/oc-audit-06/refdata`
(reference). See `rust-port-audit/artifacts/06/` for mock servers and traces.

```
Rust  run "hello"                          -> "in-process server not wired", exit 1
Rust  run --format json "hello"            -> same error
Rust  session list / export / import       -> "not yet wired", exit 1
Rust  session (bare) / --help              -> help renders, dispatch works
Rust  serve --port N                       -> binds socket, NEVER serves HTTP
Rust  run --attach <mock> hello            -> session create+prompt+idle, exit 0
Rust  run --attach <mock> --continue       -> list(no roots)->create, exit 0
Rust  run --attach <mock> --session S      -> get(S)->prompt, exit 0
Rust  run --attach <mock> --session S --fork -> get(S)->fork->prompt(new id)
Rust  run --attach <mock> --format json    -> json envelopes, exit 0
Rust  run --attach <real ref serve>        -> full provider round-trip, exit 0
Rust  run --attach <real ref serve> --command init -> tools streamed, exit 0
Rust  run --attach <real ref serve> --session ses_doesnotexist -> "Session not found", exit 1
Rust  run --attach <Rust serve>            -> HANGS (timeout 124), serve not HTTP
Reference session list --format json       -> works (1 session from our attach run)
Reference export <id> / import <file>      -> works (persisted to ref SQLite)
Reference session delete <id>              -> works; delete cascades (messages/parts gone)
Reference session delete does_not_exist    -> "Expected a string starting with 'ses'"
```

## Runtime scenarios attempted

| Scenario | Result | Evidence |
|---|---|---|
| 1. Local `run "hello"` w/ mock OpenAI (OPENCODE_MODELS) | **FAIL (BLOCKED)** — `LocalClient::create` errors before any provider is consulted; mock OpenAI trace log empty | `run/client.rs:65-69`; `openai_trace.log` empty |
| 2. `session list` / `session` / `export` / `import` | **FAIL** — all return `not_wired("...not yet wired...")`, exit 1 | `session.rs:12,17`; `export_cmd.rs:10-12`; `import_cmd.rs:52-54` |
| 3. `run --attach <url>` against `opencode serve` (real) | **PASS** — full session lifecycle via the reference server: create → prompt → provider call → event stream → text output → idle → exit 0 | artifacts: `ref_attach.json`, `ref_attach2.json`; persisted `session`/`message`/`part` rows in ref SQLite |
| 3b. `run --attach` against **Rust** `serve` | **FAIL** — `serve.rs:40-67` binds a bare TCP socket that drains connections without HTTP; attach times out (exit 124) | `rust_serve.log`; curl timed out 0 bytes |
| 4. Persistence (Rust binary only) | **NONE** — `OPENCODE_DATA` dir stays empty; no session/message storage in any wired path | `/tmp/oc-audit-06/data` empty after all runs |
| 5. Recovery after process termination | **NOT IMPLEMENTED** — Rust local path cannot create a session; Rust server stores are in-memory only; nothing survives restart | `oc-server/state.rs:26-48` |
| 6. Fork / continue / resume / command | **PASS** (attach) — traced via mock + real server | `mock_trace.log`; `ref_attach2.json` |

## Architecture or behavior summary

**Only one wired end-to-end path exists:** `opencode run --attach <url>` →
`AttachClient` (HTTP) → remote opencode server → session lifecycle happens
**on the server**, and the Rust binary consumes the `/event` SSE stream and
renders it. The Rust port owns the *client* half (create/prompt/fork/list/get/
share/command/subscribe over HTTP) plus the non-interactive event loop.

The other half — an in-process server or a direct-to-provider local run — is
**not wired**:

- `LocalClient::create` always returns an error (`run/client.rs:65-69`).
- `oc-server` contains a full axum router and handlers but nothing in `oc-cli`
  calls `oc_server::server::listen`; `serve.rs:40` instead binds a bare TCP
  socket (`serve.rs:40-67`) and never routes HTTP.
- `oc-database` (full Drizzle-equivalent schema, migrations, row CRUD) is
  declared in `oc-cli`/`oc-core`/`oc-sync` Cargo.tomls but **no production code
  calls it** — only TODO comments.
- `oc-session-runner` (agent loop, run-coordinator, retry) is implemented and
  tested but never invoked outside its own crate tests.
- `oc-session` provides pure data-model/service helpers + a `SessionDb`
  trait; its store (`store.rs`) is an abstraction, not a wired implementation.

The Rust **server handlers** (if ever wired) are in-memory only:
`state.rs:26-48` defines `Stores` as `HashMap`s; `session_create` inserts into
the map (`handlers/session.rs:266-268`) and `session_prompt` pushes to
`record.messages` without invoking a runner or emitting events
(`handlers/session.rs:371-429`). No persistence, no runner integration, no
compaction trigger. This diverges sharply from the reference, whose Session
service persists to SQLite (verified: session/message/part tables populated).

## Positive observations

- `run --attach` is genuinely functional against a real opencode server: the
  complete path CLI entry → session create → prompt → **real provider call** →
  SSE event stream → text/tool rendering → idle exit was traced with real model
  output (`ref_attach.json`, `ref_attach2.json`, `--command init` tool events).
- Resume (`--session <id>`), fork (`--session X --fork`), continue
  (`--continue`), and command (`--command init`) paths all execute and are
  correctly ordered client-side.
- Missing-session handling is correct on attach: exit 1 "Session not found".
- ID generation (`oc-session/src/identifier.rs`) implements the reference's
  sortable base62/hex scheme with validation tests.
- V2 message/part/event schema (`oc-session/src/v2.rs`) mirrors the reference
  types; compaction/overflow/summary/retry modules are implemented and unit-tested.
- All focus-crate tests pass: oc-session 93+7+8, oc-database 21,
  oc-session-runner 7, oc-server 52, oc-cli 21.

## Findings summary (table)

| ID | Severity | Confidence | Area | Finding |
|----|----------|-----------|------|---------|
| SESSION-001 | Critical | CONFIRMED | CLI run | Local `opencode run` cannot execute — in-process server never wired; provider/LLM/DB unreachable |
| SESSION-002 | Critical | CONFIRMED | CLI session | `session list`/`delete`, `export`, `import` all "not yet wired" |
| SESSION-003 | Critical | CONFIRMED | serve | `opencode serve` binds a bare socket; never serves HTTP; attach hangs |
| SESSION-004 | Critical | CONFIRMED | persistence | Sessions/messages persist ONLY in server memory (Rust side); nothing written to disk by any wired Rust path |
| SESSION-005 | High | CONFIRMED | SSE parse | `sse_stream` drops all but the first event per network chunk — lost events in real streams |
| SESSION-006 | High | CONFIRMED | runner wiring | Agent loop/retry/compaction in oc-session-runner never called by production code |
| SESSION-007 | High | CONFIRMED | server | oc-server handlers never invoke runner/events; message admission is a stub |
| SESSION-008 | Medium | CONFIRMED | recovery | No recovery-after-termination design exists in the Rust port |
| SESSION-009 | Medium | CONFIRMED | share | Rust `session_share` reads `data.url`; reference returns Session with nested `share.url` — URL never extracted |
| SESSION-010 | Medium | HIGH | concurrency | In-memory `RwLock<HashMap>` server store: no cross-process safety, no durable ordering |
| SESSION-011 | Medium | HIGH | identifiers | Rust `session delete` stub bypasses `ses_` prefix validation the reference enforces |
| SESSION-012 | Low | HIGH | title | Rust server default title "New Session" vs reference ISO-timestamped titles |

## Detailed findings

### SESSION-001 — Local run path is dead (Critical, CONFIRMED)

`run/client.rs:65-69`: `LocalClient::create` unconditionally returns
`Err("the in-process opencode server is not wired yet ...")`. `run/mod.rs:560-571`
is the only non-attach branch and always takes this error. Runtime proof: with
`OPENCODE_MODELS` and a live mock OpenAI server on 127.0.0.1:4399, `opencode run`
exits 1 immediately and the mock OpenAI trace file is empty — **no provider
request is ever made**. The CLI reachability, `--continue`, `--fork`,
`--session`, `--title`, file attach, and permission-rule code in
`run/mod.rs:199-298` is thus unreachable in local mode.

### SESSION-002 — Session management commands are stubs (Critical, CONFIRMED)

- `session.rs:9-13` List: `not_wired("session listing is not yet wired ...")`.
- `session.rs:14-18` Delete: `not_wired(...)`.
- `export_cmd.rs:7-13`: `not_wired(...)`.
- `import_cmd.rs:33-55`: validates file/URL existence then `not_wired(...)`.

Runtime proof (Rust binary): all four return the "not yet wired" message with
exit 1. Contrast: reference `session list --format json`, `export <id>`,
`import <file>`, and `session delete <id>` all execute successfully against the
reference DB.

### SESSION-003 — `serve` is a TCP drain, not a server (Critical, CONFIRMED)

`serve.rs:40-67` binds a `TcpListener` and spawns a loop that reads and discards
bytes forever; no HTTP response is ever written. Runtime proof: curl to the Rust
serve port times out with 0 bytes after 5s; `run --attach http://127.0.0.1:43130`
hangs (timeout 124). `oc-server` has a real `axum::serve` listener
(`server.rs:100-106`) but nothing calls it (`serve.rs:38` carries a TODO).

### SESSION-004 — No Rust-side persistence (Critical, CONFIRMED)

The wired run path persists nothing. `OPENCODE_DATA=/tmp/oc-audit-06/data`
remained empty after every Rust invocation. The Rust server, if wired, would
store sessions in `AppState.stores` (`state.rs:26-48`) — a process-local
`HashMap` — with no DB write (`handlers/session.rs:266-268`). Only the
reference server's SQLite persists (verified session/message/part rows in
`refdata/opencode/opencode.db`). `oc-database` implements full persistence
CRUD (`database.rs`, `tables.rs`) and 40+ migrations, but no production caller
exists (grep: only TODO comments in `oc-cli`, `oc-core`, `oc-sync`).

### SESSION-005 — SSE stream drops events within a chunk (High, CONFIRMED)

`run/client.rs:412-455` `sse_stream` uses `stream::unfold` with closure-local
`buffer`/`data`. The closure returns after the **first** complete event, so any
remaining buffered bytes (subsequent events in the same read) are discarded on
the next closure call. Runtime proof: a mock server emitting two text events in
one chunk yields **no output** and exit 0 (only the first `session.status`
event processed); the identical server with a 0.4s per-event delay prints both
texts and exits correctly. Real opencode streams routinely coalesce events
(step-start/step-finish/tool deltas), so this will silently drop events and can
exit "successfully" with truncated output.

### SESSION-006 — Runner engine is orphaned (High, CONFIRMED)

`oc-session-runner` implements `run_coordinator.rs`, `runner/llm.rs`
(`SessionRunnerService` with full drain loop), retry (`retry.rs`), and
interruption. Its own tests pass (7/7), but grep shows **no production crate
references `oc_session_runner`** except the Cargo.toml dependency in `oc-cli`.
The `RunnerDeps` bundle (`runner/llm.rs:37-53`) is satisfied only by mock
services in `tests/runner_loop.rs`.

### SESSION-007 — Server message admission is a stub (High, CONFIRMED)

`handlers/session.rs:371-429`: `session_prompt` pushes a user message into an
in-memory vec, sets `active = true`, returns an `Admitted` envelope — it never
starts a runner, never publishes events, and never persists. `session_compact`
(`handlers/session.rs:431-444`) only checks existence. The v1
`instance_handlers.rs:461-502` prompt handler is the same shape. So even a
wired Rust server would not produce assistant messages or a provider stream.

### SESSION-008 — No recovery design (Medium, CONFIRMED)

Because nothing persists and the runner is never invoked, there is no
"recovery after process termination": no durable session input rows
(`session_input`), no event replay, no resume-after-crash. The reference
persists every message/part and supports resume; the Rust port has no
equivalent in any wired path.

### SESSION-009 — Share URL extraction mismatch (Medium, CONFIRMED)

`client.rs:325-336` `session_share` does `unwrap_data(...).get("url")`, but the
reference `session.share` returns a full `Session` object whose URL lives at
`data.share.url` (openapi.json schema; `session.ts:69,133`). The Rust client
will never extract the URL, so `--share`/auto-share printing silently no-ops.
(Real server also returned `InternalServerError` for share in this sandbox.)

### SESSION-010 — In-memory concurrency model (Medium, HIGH)

`state.rs:24-48`: `Stores` is `RwLock<HashMap>`; all session mutations serialize
on one lock with no transaction or ordering guarantee across restarts. Ordering
in `handlers/session.rs:409` uses `record.messages.len()` as `admitted_seq`,
which resets with process memory. No cross-process safety exists.

### SESSION-011 — Missing `ses_` prefix validation (Medium, HIGH)

Reference `session delete does_not_exist` rejects with "Expected a string
starting with 'ses'". The Rust delete stub accepts any string; once wired it
would need `identifier::with_given`/`ascending` validation parity
(`identifier.rs:46-72`) which is implemented but unused in this path.

### SESSION-012 — Server default title (Low, HIGH)

`handlers/session.rs:252` sets `title: "New Session"`; the reference generates
`New session - <ISO timestamp>` / `Child session - ...`
(`oc-session/src/session.rs:18-19,22-28` implements it, but the server handler
does not use it). CLI `run` title truncation at 50 chars matches the reference
(`run.ts:453` vs `run/mod.rs:171-182`).

## Feature or behavior gaps

- Local (in-process) session lifecycle: creation, persistence, provider loop —
  entirely absent. This is the single largest gap and matches the agents'
  claim ("run --attach works; local server path not wired").
- Listing/deletion/export/import: CLI stubs.
- Real `serve` (HTTP): absent.
- Interactive `--mini` / TUI attach: `not_wired` (`run/mod.rs:292-297`,
  `attach.rs:74`).
- Compaction/summarization/overflow: implemented in `oc-session` but never
  invoked by a wired path.
- Interruption/cancellation: `RunCoordinator::interrupt` and server
  `session_interrupt` exist but are unwired.
- `db` and `stats` commands also `not_wired` (adjacent persistence gaps).

## Test coverage gaps

- No integration test drives `run --attach` against a real or mock server (the
  `AttachClient` is only unit-tested for URL encoding — `client.rs:457-473`).
- No test for `sse_stream` buffering; the chunk-coalescing bug is untested.
- `oc-cli` has no tests for `session`/`export`/`import`/`serve` dispatch paths.
- `oc-server` tests (`api.rs`) exercise handlers in-memory but not persistence
  or runner integration; no test covers session message admission producing
  assistant output.
- No test for share response parsing (`data.share.url`).
- Runner tests use mocks only; no fixture-backed golden test links
  `oc-session-runner` output to reference event JSON.

## Unverified areas

- `oc-database` correctness against the reference: migrations are present and
  tested (`tests/migrations.rs`, `tests/schema_golden.rs`), but since no
  production path opens the DB, end-to-end DB behavior is UNVERIFIED.
- `oc-session` compaction/summary against a real model provider: UNVERIFIED.
- Provider/model switching inside a live session (server-side) — the Rust
  client's `session_command`/prompt with model selection worked against the
  reference server, but the Rust server handler path is unwired.
- Recovery/rerun of an interrupted session on the Rust side: BLOCKED by missing
  wiring (no evidence either way).
- Rust `serve` behavior beyond socket-level (mDNS, auth, CORS): BLOCKED — the
  real listener is not invoked.

## Final domain verdict

**NOT_READY** — Session lifecycle and application state are not functionally
present in the Rust binary. Only the `run --attach` *client* half is wired and
works (verified against a real server). Local session creation, persistence,
listing, deletion, export/import, and an HTTP-capable `serve` are either
"not yet wired" stubs or absent, and the SSE parser has a confirmed
event-dropping defect that would corrupt attach output under coalesced chunks.
