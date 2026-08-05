# Plan 12 — Primary CLI Workflows: run / serve / session / db / export / import

**Agent:** 20-AG-12 · **Domain:** Primary CLI workflows
**Repo:** `/root/opencode-rs` · **Branch:** `fix/audit-remediation` · **Status:** Wave 0 READ-ONLY plan
**Reference spec:** `reference/packages/opencode/src/cli/cmd/{run,serve,session,db,export,import}.ts`
**Reference binary (oracle):** `/root/.opencode/bin/opencode` (1.18.13)
**Owned findings (FINDING-STATUS.csv):** CLI-001, CLI-005 (primary half), SESSION-001. CLI-002 CLI-side (coordinate with Agent 10, who owns serve.rs).

---

## 1. Owned findings

| ID | Severity | Blocker | File:line (current) | Summary |
|---|---|---|---|---|
| CLI-001 | Critical | YES | `crates/oc-cli/src/cli/cmd/run/client.rs:65-69` | `LocalClient::create(_ctx)` hard-`Err("…in-process opencode server is not wired yet…")`. `run/mod.rs:560-571` is the only non-attach branch and always takes this error. `--continue/--fork/--session/--title`, file-attach, and permission-rule code (run/mod.rs:199-298) is unreachable locally. |
| CLI-005 (primary half) | High | YES | `session.rs:12,17`, `export_cmd.rs:10`, `import_cmd.rs:52`, `db.rs:38,41` | `session list/delete`, `export`, `import`, `db <query>`, `db` shell all return `not_wired(...)` + exit 1. Reference: all exit 0 and do real work. Secondary commands (stats, mcp, debug, …) are Agent 17's half. |
| SESSION-001 | Critical | YES | `session.rs:12,17`, `export_cmd.rs:10`, `import_cmd.rs:52` | Session management over oc-database/oc-session is unwired; nothing persists from the CLI side. |
| CLI-002 (CLI-side) | Critical | YES | `serve.rs:40-67` | `serve` binds a bare TCP socket that drains bytes; never serves HTTP. **Agent 10 owns the fix** (`oc_server::server::listen` mount + shutdown, plan-10 §3.1). I coordinate the CLI parity contract and own the exit-code/signal checklist only. |

Also confirmed while planning (static + oracle probes; see /tmp/oc12-oracle):
- `opencode run` local and `run --format json` produce zero provider traffic (no mock OpenAI request ever issued).
- Reference oracle behaviors captured this wave: `session list` table bytes (header `Session ID`/`Title`/`Updated`, `─`×len separator, 12-hour time `1:20 PM · 8/5/2025`, stdout, exit 0); `session list --format json` item shape `{id,title,updated,created,projectId,directory}` pretty-2; `session delete` success → stdout green `Session <id> deleted` exit 0; bad-format id → `Expected a string starting with "ses", got "<id>"` + `Unexpected error` exit 1; missing valid-format id → `Session not found: <id>` exit 1; `db "<query>"` tsv header+rows / json array / empty → exit 0; `db` bad SQL → sqlite error + `Unexpected error` exit 1; `db path` prints path; `export` writes `Exporting session: <id>` to stderr + 2-space JSON to stdout exit 0, `--sanitize` redacts `[redacted:session-title:<id>]` etc.; `export` without id is a TTY-only clack selector (hangs non-TTY → document divergence); `import` inserts the session row non-atomically before message decode failure (session persists, messages empty, exit 1) — parity trap; `serve` banner `opencode server listening on http://127.0.0.1:<port>` stdout, `GET /api/health` → `{"healthy":true}`, SIGTERM → 143.

---

## 2. Architecture decision: LocalClient over the in-process router, not TCP loopback

The reference local run never binds a socket: `run.ts:943-955` routes `createOpencodeClient({ baseUrl:"http://opencode.internal", fetch })` into `Server.Default().app.fetch(...)`. Plan-11 §3 and plan-02 §4 agree the Rust analogue is an **in-process `RouterExecutor`** (tower `oneshot` over `oc_server::router::build(AppState)`), which (a) matches reference parity, (b) avoids EADDRINUSE/port-4096 handling, and (c) keeps the runtime off the network until the SEC-001/002/003 gate lands (plan-10 §6 gate). The existing `--port` run flag is accepted for CLI surface parity but unused by the reference (reference never reads `args.port`); it stays unused.

- **RunClient trait** (oc-cli `run/client.rs:14-56`) becomes the shared contract, its home moving to `oc-app` (plan-02 §4). `oc-app` implements `App::local_client() -> Box<dyn RunClient>` (plan-02) over oc-client's `RouterExecutor` (plan-11 §3).
- `oc-cli` depends on `oc-app` (+ trims its 18 dead edges per plan-02). `run/mod.rs` calls the oc-app factory instead of `LocalClient::create`.

### Composition seam (matches plan-02 §3 graph)

`oc_cli::run` → `oc_app::AppBuilder`/`App` (Agent 02) which owns: `Database` (Agent 03), event bus/durable (Agent 03), `LlmClient` (Agent 06), tool registry + permission gate (Agents 07/08/09), runner `RunnerDeps` (Agent 07), `AppState`/router (Agent 10 via `listen_with`), `RouterExecutor` (Agent 11). The CLI never constructs domain services — it only picks which client/command to run.

---

## 3. Files to change

| File | Change |
|---|---|
| `crates/oc-cli/Cargo.toml` | Add `oc-app`; trim dead edges to `oc-app + oc-tui + oc-schema + oc-util` per plan-02 (keep what compiled consumers need). |
| `crates/oc-cli/src/cli/cmd/run/client.rs` | Delete `sse_stream` (SSE-001, Agent 11) and the hand-rolled AttachClient HTTP/URL/parse code; collapse `AttachClient`/`LocalClient` onto oc-client `OpenCode` adapters. `LocalClient::create` builds via `oc_app::App::local_client()` (or its pre-02 fallback keeps the current not-wired error with the hint). Keep the `RunClient` trait shape until its home moves to oc-app. |
| `crates/oc-cli/src/cli/cmd/run/mod.rs` | `LocalClient::create(&ctx)` branch (560-571) → oc-app factory. `execute()`/`event_loop()`/`pick_agent` local branch (currently warns "not found. Falling back…" at 451-459) → real `oc_command`/agent lookup when available; else keep warning (parity with reference localAgent fallback). No domain logic in the CLI. |
| `crates/oc-cli/src/cli/cmd/run/events.rs` | Consume oc-client event items; map `Error::Api/ClientError` to `formatRunError` surfacing (plan-11 §2). Loop break on `session.status` idle unchanged. |
| `crates/oc-cli/src/cli/cmd/run/types.rs` | Alias `GlobalEvent` to oc-client v1 `GlobalEvent` (plan-11 §2) after Agent 01 promotion. |
| `crates/oc-cli/src/cli/cmd/serve.rs` | **Agent 10 owns** (plan-10 §3.1 replaces `listen()` + `pending()` with `oc_server::server::listen`, port-0→4096, SIGINT/SIGTERM + 1s graceful `Listener::stop(true)`). I only verify CLI parity (banner stream/exit). |
| `crates/oc-cli/src/cli/cmd/session.rs` | Implement List/Delete over `oc_database` (Arc handle) + `oc_session::SessionService` (see §4). |
| `crates/oc-cli/src/cli/cmd/export_cmd.rs` | Implement over `Database` + `oc_session::v1` (`WithParts`/`Info`) + `oc_session::session::from_row` (see §4). |
| `crates/oc-cli/src/cli/cmd/import_cmd.rs` | Implement over `Database` insert + `oc_session::session::to_row` (see §4). |
| `crates/oc-cli/src/cli/cmd/db.rs` | `db path` → `oc_database::database::path()` (fixes DB-003; Agent 03 notes this too — I own the CLI file); `db <query>` json/tsv via the shared `Database`; `db` (no query) spawns interactive `sqlite3 <path>` (see §4). |

---

## 4. Per-command wiring design

### 4.1 `opencode run` (CLI-001) — over oc-app LocalClient → router → runtime (Agents 02/03/06/07/09/10/11)

1. `run/mod.rs::run` unchanged validation (die messages, stdin piping, file attach, rules) — already parity.
2. Replace the local-client construction (run/mod.rs:560-571) with:
   `let sdk: Box<dyn RunClient> = oc_app::local_client_from(ctx)?;` (Agent 02/11 seam). Attach path unchanged (`AttachClient` collapses onto oc-client over `ReqwestExecutor`, plan-11 §5).
3. `execute()` drives the existing flow over `RunClient`: `resolve_session` → create/list/fork/get (v2 `/api/session…`), `session_prompt` via **v1 `POST /session/{id}/message`** (plan-11 §5 trap: never v2 admission `sessions.prompt`), `session_command` v1 `/session/{id}/command`, `subscribe` v1 `GET /event`, `config_get` v1 `/config`, `app_agents` v1 `/agent`, `path_get` v1 `/path`, `permission_reply` v1 `/permission/{id}/reply`.
4. Events flow: server runner (Agent 07) publishes `message.updated`/`message.part.updated`/`session.status`/`session.error`/`permission.asked` → EventBus → v1 `/event` SSE (Agent 10 framing) → oc-client `SseDecoder` (Agent 11, bare-`data:` frames, chunk-coalescing fixed) → `event_loop` renders.
5. **No domain logic in CLI** — session creation/listing/prompt/runner/compaction are the server+runtime (Agents 02/03/07). CLI only renders events and maps exit codes.

### 4.2 `opencode serve` — Agent 10 owns; I verify parity

`serve.rs` → `oc_server::server::listen(ListenOptions{hostname,port,cors,mdns,mdns_domain, auth:from_env})`, print banner, block on signal, graceful stop. See plan-10 §3.1 for the exact replacement and §6 for tests. My CLI checklist in §5 covers banner stream, warning line, exit codes.

### 4.3 `opencode session list` — Database (Agent 03) + oc-session service

1. Open the shared DB: `Arc<oc_database::Database>` via Agent 03's `DatabaseHandle`/`AppServices.database` (or direct `Database::open(oc_database::database::path())` behind a lazy accessor).
2. `svc.list({ roots:true, limit:maxCount })` equivalent: `Database::list_sessions(false)` (already newest-first, `time_archived IS NULL`) → `oc_session::session::from_row` → `Info`.
3. **table** (default): header `Session ID` + spaces(`max(20,max id len)`-10) + `  Title` + spaces(`max(25,max title len)`-5) + `  Updated`; separator `─`.repeat(header.len()); rows `id.padEnd(maxIdWidth) + "  " + truncate(title,maxTitleWidth).padEnd(maxTitleWidth) + "  " + timeStr`; timeStr = `Locale.todayTimeOrDateTime` → `%I:%M %p` (no leading zero) or `%I:%M %p · %-m/%-d/%Y`. To **stdout** via `println!`. Pager: spawn `less -R -S` with stdin piped when `stdout.is_terminal() && maxCount.is_none() && format=="table"` (mirror session.ts:93-114). Empty list → no output, exit 0.
4. **json**: `serde_json::to_string_pretty(vec![{id,title,updated,created,projectId,directory}])` to stdout (verify exact field set/order vs oracle; `projectId` = session's `project_id`).

### 4.4 `opencode session delete <id>` — Database + identifier validation

1. Validate: if `!id.starts_with("ses")` → return `Err(anyhow!("Expected a string starting with \"ses\", got \"{id}\""))` (reproduces reference `Schema.String.check(isStartsWith("ses"))` message; top-level dispatch renders `Unexpected error` + cause, exit 1 — matches oracle bytes).
2. `SessionService.get(id)` → not found → `fail("Session not found: {id}")` (CliError → clean `Error: Session not found: <id>`, exit 1).
3. Delete cascade (reference `Session.remove` → children then rows; audit 06 verified message/part rows cascade): delete `part` WHERE session_id, `message` WHERE session_id, `session_message`, `session_input`, `session_context_epoch`, `event`/`event_sequence` (aggregate), then `session`. Use Agent 03's `SqliteSessionDb`/DB helpers or `Database::delete_by*` in one transaction. No DB helper exists today — add `delete_session(session_id)` to oc-database (Agent 03 or me; coordinate).
4. Success: `println!("Session {id} deleted")` with success-bold style to stdout (oracle uses green bold `Session <id> deleted`, no reset issue — match byte stream), exit 0.

### 4.5 `opencode db` — Database only

1. `db path`: `println!("{}", oc_database::database::path().display())`, exit 0 (already works; switch to canonical impl for DB-003 channel parity).
2. `db "<query>"`: `db.db.run(sql.raw(query))` → rows. **tsv**: header = keys of first row joined by `\t`; each row = `keys.map(k => row[k]).join("\t")`; empty → no output. **json**: `JSON.stringify(rows, null, 2)`. exit 0. Errors → `Err(sqlite_error)` → top-level `Unexpected error` + message, exit 1 (oracle parity; the reference uses `Effect.orDie`).
3. `db` (no query): `std::process::Command::new("sqlite3").arg(path).status()` with `stdio: inherit`, return child's exit code. If `sqlite3` is missing, reference spawn fails — mirror (spawn error propagates, nonzero).

### 4.6 `opencode export [id] [--sanitize]` — Database + oc-session v1

1. `Exporting session: {id ?? "latest"}` to **stderr**.
2. If `--session` absent: reference runs a TTY-only clack selector on stderr. **Rust**: when `stdin`/`stdout` is a TTY → minimal interactive list on stderr (reuse `oc-command`/prompt if available; else a simple `Select` loop); when non-TTY → print `Exporting session: latest`, export the latest session (documented divergence — reference hangs on non-TTY, verified exit 124).
3. `info` = `SessionService.get(id)` → `oc_session::session::Info` (serialize with omit-none for optional fields — oracle export omits `agent/model/parentID/workspaceID/metadata/share/summary/revert` when absent); `messages` = `svc.messages({sessionID})` = `Vec<WithParts>` from `message`/`part` tables (newest-first paging at 50 like session.ts:830-853 — reuse `Database::list_messages_page` + `list_parts_by_messages`).
4. Output `{ "info": …, "messages": […] }` `to_string_pretty` + trailing newline to **stdout**, exit 0. `--sanitize`: apply export.ts redact tree (session-title/directory, text, file-url/name, tool-input/output/raw, patch files/hash, cwd/root, summary diffs, etc. — `[redacted:<kind>:<id>]`, empty→kept, data→`{redacted:"<kind>:<id>"}`). Note `path` is **not** redacted (verified). Implement as a recursive `sanitize(Value) -> Value` mirroring export.ts:11-220.
5. Not-found → `fail("Session not found: {id}")`, exit 1.

### 4.7 `opencode import <file|url>` — Database + to_row

1. Keep existing `parse_share_url` / `format_import_file_error` (already parity).
2. URL path: fetch `GET {base}/api/share/{slug}/data` (fallback per import.ts:144-146) → flat `ShareData[]` → `transformShareData` grouping into `{info, messages:[{info,parts}]}` (port transformShareData, import.ts:60-90). Scope: implement the HTTP fetch; transform + insert. Share fetch failures print message to stdout, exit 0 (reference returns early, exit 0 — `Failed to fetch share data`, `Share not found or empty`).
3. File path: read JSON → `{info, messages}`.
4. Decode `info` → `oc_session::session::Info`, then `to_row(info)` with overrides `projectID=ctx.project.id, directory=ctx.directory, path=relative(ctx.worktree→ctx.directory)` (import.ts:179-184). Upsert `session` row (on conflict update project_id/directory/path). For each message: decode `SessionV1.Info`, strip `{id, sessionID}`, insert `message` (id, session_id, time_created=time.created??now, data=rest) on-conflict-nothing; for each part: insert `part` (id, message_id, session_id, data=rest) on-conflict-nothing. Use `Database::insert(table, row, JSON_COLUMNS)` + `Sqlite::transaction` for the batch.
5. **Non-atomic parity:** reference decodes+inserts session first, then decodes each message inside the loop — a malformed message leaves the session inserted and exits 1. Mirror that order exactly (decode-insert session, then per-message decode-insert); do not wrap in a rollback transaction.
6. Success: `Imported session: {id}` to stdout, exit 0.

---

## 5. Output / exit-code parity checklist (differential vs oracle)

| Scenario | stdout | stderr | exit |
|---|---|---|---|
| `run "hi"` (mock provider, non-TTY) | per text part `text\n`; `> agent · model` banner | — | 0 |
| `run --format json "hi"` | NDJSON `{type,timestamp,sessionID,...}` for step_start/text/step_finish/tool_use/error | — | 0 |
| `run` bad session (`--session ses_x`) | — | `Session not found` | 1 |
| `run` no message, non-TTY, no stdin | — | `You must provide a message or a command` | 1 |
| `run --fork` without `--continue/--session` | — | `--fork requires --continue or --session` | 1 |
| `run` session.error | — | error text | 1 |
| `run --command` prompt error | json: `error` envelope / else formatted | — | 1 |
| `session list` empty | (none) | — | 0 |
| `session list` table | header/─/rows (byte check) | — | 0 |
| `session list --format json` | pretty array | — | 0 |
| `session delete <valid>` | green `Session <id> deleted` | — | 0 |
| `session delete <bad-format>` | — | `Error: Unexpected error\n\nExpected a string starting with "ses", got "<id>"` | 1 |
| `session delete <missing valid>` | — | `Error: Session not found: <id>` | 1 |
| `db path` | path | — | 0 |
| `db "<q>"` tsv | header\t rows | — | 0 |
| `db "<q>" --format json` | pretty array / `[]` | — | 0 |
| `db "<bad sql>"` | — | `Unexpected error` + sqlite msg | 1 |
| `export <id>` | pretty `{info,messages}` | `Exporting session: <id>` | 0 |
| `export <id> --sanitize` | redacted pretty JSON | same | 0 |
| `export <missing>` | — | `Exporting session: <id>` + `Session not found: <id>` | 1 |
| `import <file>` | `Imported session: <id>` | — | 0 |
| `import <missing file>` | — | `File not found: <file>` | 1 (clean, no `Unexpected error` — shared contract, Agent 17 §3) |
| `import <malformed>` | — | `Invalid JSON in <file>: …` | 1 |
| `serve --port N` | `opencode server listening on http://…` (+ warning if no password) | — | 0 while running; SIGTERM→143, SIGINT→130 |
| `serve --port 0` | banner `:4096` (preferred) | — | — |
| attach round-trip via local client | as `run` | — | 0 |
| broken pipe (`run | head -1`) | silent | — | 0 (Agent 17 §3.3 owns global fix; run must not double-print) |

Systemic CLI-004 (stream placement, `ui::error` double space, `ui::println` padding, repeated flags, `help` subcommand, broken pipe) is Agent 17's shared contract layer; my commands must be written against the corrected `ui`/`error` so they inherit the fix. Where a reference message is `Error: <msg>` single-space, my code must rely on the shared formatter, not emit its own prefix.

---

## 6. Test list (implementation phase; binary-level where possible)

Run against the **Rust binary** with a disposable `XDG_DATA_HOME`/`OPENCODE_DB` and, for run/serve, a mock OpenAI-compatible provider (Rust port of `rust-port-audit/artifacts/09-mock-provider.py`, plan-05 §6; port `{port}/v1/chat/completions`, SSE `[DONE]`, Bearer `test-key-12345`, config `mockai { options:{ baseURL:"http://127.0.0.1:{port}/v1", apiKey } }`).

1. **Local run end-to-end**: `opencode run "hi"` with mock provider → streaming text on stdout, exit 0; mock log records exactly one `POST /v1/chat/completions`. (CLI-001 regression; INTEGRATION-001 gate.)
2. **Local run JSON**: `opencode run --format json "hi"` → NDJSON with `step_start`/`text`/`step_finish`, exit 0; assert `timestamp`/`sessionID` present and event order.
3. **SSE chunk-coalescing (SSE-001 regression via local path)**: mock emits 2+ text events in one chunk → both printed, exit 0.
4. **run flags parity**: `--continue`, `--session <id> --fork`, `--command init`, `--model mockai/mock-1`, `-f <file>`, stdin piped message, `--share` no-op (no server share), permission auto-reject path.
5. **Serve HTTP**: `opencode serve --port 0` → banner `http://127.0.0.1:4096`; `curl /api/health` → `{"healthy":true}`; `curl /api/session` 200; `GET /api/event` first frame `data: {…server.connected…}\n\n`; SIGINT/SIGTERM → clean exit. (Agent 10 tests; CLI verifies banner + exit.)
6. **Attach round-trip**: `run --attach http://127.0.0.1:{port}` against a live Rust `serve` → session create/prompt/idle, exit 0 (SERVER-12; needs 07/10/11 wiring).
7. **Session lifecycle after restart**: `run "hi"` → exit → `session list --format json` shows the session (id/title/updated) → `session delete <id>` → `session list` empty. (SESSION-001 + SESSION-004 persistence gate; plan-03 restart test at binary level.)
8. **Export→import round-trip**: `run`/seed session → `export <id>` → `import <file>` into a fresh data dir → `export <id>` again → **byte-identical** JSON (modulo `projectID`/`directory`/`path` overrides, matching oracle behavior).
9. **Export `--sanitize`**: assert `[redacted:session-title:<id>]`, `[redacted:session-directory:<id>]`, text redaction; `path` unredacted.
10. **Import error parity**: missing file → `File not found: <file>` clean, exit 1; malformed JSON → `Invalid JSON in <file>: …`, exit 1; malformed message inside valid file → session inserted, messages empty, exit 1 (non-atomic parity).
11. **db parity**: `db path` vs oracle path; `db "select 1 as a, 2 as b"` tsv/json byte-equal; empty result; bad SQL message + exit 1; interactive shell skipped when `sqlite3` absent (test guards).
12. **session delete error parity**: bad format message + exit 1; missing valid id message + exit 1 (byte-compare stderr modulo CLI-013 spacing fix).
13. **Diff harness (Agent 18)**: add the §5 table as differential fixtures (stdout/stderr/exit) under `rust-port-remediation/artifacts/12-*`.

---

## 7. Dependencies on other agents

| Agent | Finding(s) | What I need | What I provide back |
|---|---|---|---|
| **02** | INTEGRATION-001 | `oc-app` composition root; `App::local_client()` + `RunClient` trait home; LocalClient over real router. **Blocks CLI-001.** | `run` calls the factory; oc-cli trims edges to oc-app; serve/session/db consume `AppServices.database`. |
| **03** | DB-001, DB-003, DB-004/005 | `Arc<oc_database::Database>` accessor; `SqliteSessionDb`; `delete_session` helper (or approve mine); `db path` channel fix. **Blocks SESSION-001/db/export/import.** | CLI handlers (session/db/export/import) over their layer; boundary per plan-03 §8: they supply store/DB, I own CLI surface/formatting. |
| **07** | TOOLS-001, ASYNC-001/004 | Runner wired into server handlers so `/session/{id}/message` (v1) runs the loop and publishes `message.part.*`/`session.status`; interrupt seam. **Blocks run E2E.** | `run` event-loop consumption of their event stream; interrupt call sites. |
| **10** | CLI-002, SERVER-01/03/04/09, SSE-002 | `serve.rs` mount + port-0→4096 + signals; mounted `AppState`/router for the local client; v1 `/event` framing (bare `data:`, `server.heartbeat`, disposed termination). **Blocks serve + local transport target.** | CLI parity checklist §5; LocalClient/attach consuming their SSE frames. |
| **11** | SSE-001, ARCH-008 | oc-client `OpenCode` + v1 methods (`message`/`command`/`share`/`config`/`app`/`path`/`permission.reply`/`event` GlobalEvent) + `RouterExecutor`/`ReqwestExecutor` + `HttpExecutor` trait. **Blocks the RunClient collapse.** | oc-cli RunClient adapters over oc-client; `LocalClient::create` wiring the router into their transport. |
| 01 / 08 / 09 / 06 | TEST-002 / SEC-001 / TOOLS-002..004 / LLM-001 | oc-schema canonical types; permission gate (local run must not execute tools pre-gate — plan-10 §6 hard gate); tool registry materialize; real LLM stream. | — |
| 17 | CLI-004 | Shared stream/`ui::error`/broken-pipe/help contract my commands inherit. | No double-own: 17 owns `stats/mcp/debug/…`, 16 owns attach/TUI, 10 owns serve. Verified against plan-17 §2 ownership table. |
| 18 | TEST-001/003 | Differential harness + reference-captured fixtures (export JSON, reference `.db`). | §5 parity table + fixture inputs. |

---

## 8. Risks

1. **v1/v2 endpoint trap (run).** `run` must hit v1 `/session/{id}/message` + v1 `/event`; oc-client's v2 `sessions.prompt` (admission) and `/api/event` are the wrong surfaces. Mitigate: per-endpoint request goldens (plan-11 §6) + §6 tests.
2. **Server-side dead end.** Even with LocalClient wired, `run` produces no assistant output until Agent 07 wires the runner into the handlers (SESSION-007). Scope: CLI-001 cannot be marked fixed until the oc-app `session_roundtrip.rs` gate (plan-02 §6) is green.
3. **Security gate.** Local in-process router runs real handlers; must not ship before SEC-001/002/003 (plan-10 §6, plan-11 §8). LocalClient stays behind the not-wired error if the gate isn't met.
4. **Session delete cascade completeness.** Reference `remove` recurses children + clears event/sequence rows. A partial delete leaves orphan rows that break `session list` (orphans still projected). Test #7 asserts cascade.
5. **Export/import non-atomicity parity.** Reference inserts the session before failing on a bad message. If I wrap in a transaction, round-trip parity breaks. Mirror the exact order; test #10.
6. **`session list` table bytes / locale.** `Locale.todayTimeOrDateTime` (12-hour, `·`, no zero padding) is locale-sensitive; pin with a golden from oracle (#7) and treat local-timezone tests carefully (oracle ran in UTC+? — set TZ explicitly in tests).
7. **Interactive export selector.** Reference clack selector hangs non-TTY (verified exit 124). Rust fallback (latest-session on non-TTY) is a documented divergence; flag in KNOWN-DEVIATIONS.md.
8. **CLI-013 double space.** Until Agent 17 lands the shared `ui::error` fix, `Error:  ` double-space appears in every failure output; my diff tests must expect the fixed single space, so merge my E2E assertions AFTER 17's contract layer.
9. **Boundary overlap.** plan-03 §7 lists the CLI command files as Agent 03's; plan-03 §8 and plan-17 §1 both assign them to me (CLI surface/formatting). Resolve in the coordinator so 03 provides the store layer and 12 owns the `session.rs`/`export_cmd.rs`/`import_cmd.rs`/`db.rs` handlers.

---

## 9. Merge-order recommendation

**Wave 4 CLI**, gated on the Wave 1-3 backbone — never merge the CLI wiring before the underlying services exist, or we reintroduce INTEGRATION-001 (a CLI that parses but can't run).

1. **Wave 1** (backbone, Agent 01+02): oc-schema promotion + `oc-app` skeleton with `AppBuilder`/`AppServices`/`LocalClient` over the real router. `run`/`serve` compile against `oc-app`; LocalClient may still return the not-wired error.
2. **Wave 2** (03/07/06/05/09/08 stores+services): DB stores, runner wiring, real LLM stream, tool+permission gate. `session_roundtrip.rs` (plan-02) goes green — **this is the run gate**.
3. **Wave 3** (10+11): `serve` mount (`listen_with`), SSE framing, oc-client adapters. **SEC-001/002/003 must be merged before this** (plan-10 §6 hard gate). `attach` round-trip green.
4. **Wave 4 (this plan)** — one PR slice per command, workspace green per merge, in this order:
   1. `db` (path/query/shell) — smallest, only needs Agent 03's Database; fixes CLI-005 rows 86-89/137.
   2. `session` (list/delete) — needs `SqliteSessionDb`; gates SESSION-001; includes `delete_session` DB helper.
   3. `export`/`import` — needs oc-session v1 + to_row; round-trip test is the SESSION-001 completion proof.
   4. `run` local via `App::local_client()` — **last**, after serve/attach (Wave 3) so the local client target is real; fixes CLI-001. Land `run` and `serve` attach-together with the Agent 18 `cli_e2e.rs` harness so the INTEGRATION-001 gate is locked by binary test.
   5. `serve` CLI parity verification against Agent 10's already-merged serve.rs.
5. **Wave 5**: Agent 18 differential fixtures for §5 table; release gate.

Every merge must keep `cargo build --workspace && cargo test -p oc-app -p oc-cli` green and must not ship a new socket until the security gate is met.
