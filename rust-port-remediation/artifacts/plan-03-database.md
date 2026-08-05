# Plan 03 — Database-backed persistence (Agent 03)

**Domain:** 05-database · **Branch:** `fix/audit-remediation` · **Phase:** Wave 0 (READ-ONLY plan)
**Owner:** Agent 03 · **Depends on:** Agent 02 (composition root), Agent 01 (type promotion), Agent 07 (runner writes session projection), Agent 10 (server), Agent 12 (CLI commands), Agent 08 (permission write path), Agent 18 (reference-capture fixtures).

---

## 1. Owned consolidated findings

| ID | Severity | Verdict | Notes |
|---|---|---|---|
| DB-001 | Critical (blocker) | **Confirmed** | `Database::open` has zero production call sites (`rg oc_database` → 0 non-test refs). `InMemoryDurableStore` default in `oc-core/src/bus.rs:86` & `context.rs:54`; server `Stores` is `HashMap`s (`oc-server/src/state.rs:24`); oc-sync keeps `Db { events, sequences }` in `HashMap`s (`oc-sync/src/sync/store.rs:164-168`); all DB CLI commands stubbed (`db.rs`, `session.rs`, `export_cmd.rs`, `import_cmd.rs`, `stats.rs`). Nothing persists. |
| DB-002 | High | **Confirmed** (owner scope = JSON + channel) | `oc-core`, `oc-cli`, `oc-sync` declare `oc-database` deps but compile none of its symbols (forward edges, no cycle risk). The *unused-dep* half is owned by the wiring effort; my owned work is (a) the JSON-mode serialization bug and (b) channel-aware CLI `db path`. |
| DB-003 | Medium | **Confirmed** | `crates/oc-cli/src/cli/cmd/db.rs:12-21` re-implements `path()` ignoring `OPENCODE_CHANNEL`/`OPENCODE_DISABLE_CHANNEL_DB`; correct logic exists and is unit-tested in `oc-database/src/database.rs:14-44`. |
| DB-004 | Low | **Confirmed** | `sqlite.rs:170-194` `json_to_sqlite` stores primitives raw in JSON columns (`prompt="hello"` → bare `hello`); read-back `from_str("hello")` fails. Drizzle `{mode:"json"}` always stores `JSON.stringify`. |
| DB-005 | Low | **Confirmed** | JSON key order depends on `serde_json/preserve_order`; `oc-database` build doesn't enable it (workspace does). Byte-level divergence vs reference and vs standalone build. |
| INFO-002 | Info | **Confirmed** | Crate is faithful (DDL golden, 38/38 migration SQL parity, WAL/FK/busy_timeout, cascade, unique `(aggregate_id,seq)`). Wire-in is the entire DB-001 effort. |

I additionally **own the persistence portions of SESSION-001** (session `ls/delete`, export/import/stats over the DB) and **the durable-event-store wiring** (SQLite `DurableStore`).

---

## 2. Design: DB opens once in the runtime, shared everywhere

Reference opens one SQLite connection via the `Database.Service` layer and hands it to every service. Mirror that:

1. **A single `Arc<oc_database::Database>`** opened by the composition root (Agent 02) via `Database::open(oc_database::database::path())`. `open` runs the PRAGMA battery and the migration algorithm atomically (`migration/mod.rs:apply_inner`), so a fresh/corrupt/pending DB is brought to the current schema at startup.
2. **One shared handle, many views.** A small owned facade (in `oc-core`, `db/mod.rs` or `database.rs`) exposes `Arc<Database>` plus typed store constructors. All consumers hold `Arc<Database>` — never open a second connection for the same file (reference is single-connection; a second writer would fight WAL).
3. **Lazy-open seam for CLI.** Reference opens the DB whenever the `Database` layer is resolved. To avoid file IO on commands that don't need it (`help`, `version`, `auth`), provide `Database::open` through a `OnceLock`/`Lazy` accessor on the CLI `Context`; the server and session/run commands force-open at startup (migrations must run before the router serves). Force-open failure must be a hard startup error (matches reference `Effect.orDie`).
4. **Ownership of the connection.** `oc-database::Sqlite` stays `std::sync::Mutex<Connection>` (matches reference one-permit semaphore). The `std` guard is never held across an await (see §4).

**What the composition root contract is (with Agent 02):** `Services::build_with` (already has seams at `oc-core/src/context.rs:62-66` for `DurableStore`/`CredentialStore`/`ProjectDirectoryStore`) plus a new `Arc<Database>` parameter (or a `DatabaseHandle` struct) threaded into `oc-server::AppState`. Agent 03 delivers the SQLite-backed impls and the shared-handle accessor; Agent 02 calls them in the runtime graph.

---

## 3. Store-interface mapping (trait → SQLite impl)

All target tables already exist in the schema (`schema.rs` TABLES) and have typed rows in `tables.rs` — no new DDL.

| Trait / interface (owner crate) | SQLite impl | Backing table(s) | Home crate | Notes |
|---|---|---|---|---|
| `oc_core::durable::DurableStore` (`durable.rs`) | `SqliteDurableStore` | `event`, `event_sequence` | `oc-core` (already depends on `oc-database`) | `latest_sequence`/`read_after` = SELECTs; `remove_aggregate`/`claim` = DELETE/UPDATE in a tx; `transaction` = §4. `append_event` upsert already in `tables.rs:356`. |
| `oc_core::credential::CredentialStore` (`credential.rs`) | `SqliteCredentialStore` | `credential` | `oc-core` | `replace` = delete-then-insert in one tx (matches `credential.ts`); `value` is a `{mode:"json"}` column → JSON-encode `CredentialRow.value`. |
| `oc_core::project::directories::ProjectDirectoryStore` | `SqliteProjectDirectoryStore` | `project_directory` | `oc-core` | create/remove/list/get/contains map to `onConflictDoNothing`/`onConflictDoUpdate` semantics already encoded in the in-memory impl. |
| `oc_session::store::SessionDb` (`store.rs`) | `SqliteSessionDb` | `session`, `session_message`, `message`, `part`, `session_context_epoch`, `session_input` | `oc-session` (add `oc-database` dep) | `context_epoch_baseline` = `SELECT baseline_seq FROM session_context_epoch WHERE session_id=?`; `latest_compaction_seq` = `SELECT seq FROM session_message WHERE session_id=? AND type='compaction' ORDER BY seq DESC LIMIT 1`; `message_rows`/`message_row` = `session_message` projection rows; `session_row` = `session` row → `SessionInfo`. |
| `oc_session::store::SessionStore` | **exists** — `DbSessionStore<'a, D>` wraps any `SessionDb` | — | `oc-session` | Wire `SqliteSessionDb` in; no new code needed. |
| `oc_sync::sync::store::Store` | replace `Db` `HashMap`s with SQLite `event`/`event_sequence` reads/writes via `Arc<Database>` | `event`, `event_sequence` | `oc-sync` (already depends on `oc-database` + `oc-core`) | Same two tables as `oc-core`'s store — see risk R6 (single-owner decision). |
| `oc_server::state::Stores` | DB-backed `sessions` projection + `permissions` (saved) | `session`; `permission` | `oc-server` | In-flight `questions`/`pty`/`config` stay in-memory (transient in reference too). `sessions` should be read from `SessionStore` (DB), not a HashMap. |
| Permission persistence | `permission_saved_list` / `permission_saved_remove` (`oc-server/src/handlers/permission.rs:127-139`) over `permission` table | `permission` | `oc-server` + Agent 08 (write path = allow/ask/deny decisions) | Unique `(project_id, action, resource)`; cascades on project delete. |
| Session/todo storage | `get_message`/`list_parts`/`list_messages_page`/`list_todos`/`list_sessions` already in `oc-database/src/tables.rs` | `session`, `message`, `part`, `todo` | consumed by CLI + server | Use as-is. |

**Count: 8 store seams converted to SQLite-backed** (durable, credential, project-directory, session-db, sync store, server sessions projection, saved-permissions, plus the CLI/DB command layer).

---

## 4. Transaction / atomicity approach

Reference wraps each durable commit, plus projectors and the local commit hook, in **one** SQLite transaction (`event.ts:240` `db.transaction(..., { behavior: "immediate" })`); `oc-core/bus.rs:539` already mirrors this shape with an async `TxClosure` over the sync `DurableTx` view.

**Rule: rusqlite is blocking → every DB call on an async path runs under `tokio::task::spawn_blocking`; the `std::sync::Mutex` guard is only ever held inside a `spawn_blocking` closure (never across an await).**

`SqliteDurableStore::transaction(f)`:
1. `tokio::sync::Mutex<()>` serializes transactions (same as `InMemoryDurableStore.tx`), so the std connection lock is never contended between transactions.
2. `spawn_blocking(move || {` lock `std::sync::Mutex<Connection>`; `conn.transaction()` (rusqlite); build a `DurableTx` view over the `rusqlite::Transaction`; drive `f(&view)` with `futures::executor::block_on`; commit on `Ok`, rollback on `Err`; return the boxed result `})`.
3. Constraint (same as reference): projectors/commit hooks run **inside** the transaction and must not make nested DB calls on the same connection (self-lock deadlock — identical to the reference's non-re-entrant single-permit semaphore). Document this; the bus already runs projectors *before* the row writes, but still inside the tx.
4. `read_after`/`latest_sequence`/`remove_aggregate`/`claim`: `spawn_blocking` single statements; `remove_aggregate` = `DELETE FROM event WHERE aggregate_id=?` + `DELETE FROM event_sequence WHERE aggregate_id=?` in one tx.

Non-bus transactions (credential replace, import upsert, permission save) reuse the existing sync `Sqlite::transaction` (`sqlite.rs:313`) from sync contexts, or the same `spawn_blocking` wrapper when called from async handlers.

**Fallback if `block_on` inside `spawn_blocking` proves fragile** (projector awaits tokio primitives): restrict projectors/commit hooks to sync-observable side channels and perform the DB writes in a first phase, then dispatch projectors after commit — but this weakens the atomicity guarantee the reference provides, so it is a documented fallback, not the default.

---

## 5. Restart-recovery test design

1. **Persist-across-restart (session):** open `Database` at temp path → insert `SessionRow` + messages/parts via the DB helpers → drop `Arc<Database>` → reopen → assert `get_session`/`list_sessions`/`list_messages_page` return identical rows (round-trip through the typed `Row` codec + `JSON_COLUMNS`).
2. **Durable events survive restart:** `SqliteDurableStore` commit via the bus → reopen store → `read_after(-1)` returns all rows; `replay` after restart is idempotent and divergence-checked.
3. **Migration mid-failure recovery:** open a DB, kill before the last migration's journal insert (simulate by leaving a partial DB), reopen → the interrupted migration re-runs to completion, no duplicate rows (already guarded by per-migration journal-inside-tx at `migration/mod.rs:151-163`). Test asserts atomicity by dropping the file/connection at an injected failure point.
4. **Drizzle-journal import:** seed `__drizzle_migrations`, verify one-time seed into `migration` and no SQL replay (exists in `tests/migrations.rs`; promote to the restart suite).
5. **Cross-process (optional, informational):** two processes WAL + `busy_timeout` — verify no `SQLITE_BUSY` errors during concurrent read/write; matches reference `busy_timeout=5000`.
6. **CLI restart loop (Agent 18 co-op):** `run → create session → exit → run → session list` binary E2E.

---

## 6. Reference-DB compatibility fixtures

- **Golden DDL parity already passes** (`tests/schema_golden.rs`), proving file-format compatibility; add a test that a *reference-created* DB (or the golden `schema.sql` + `__drizzle_migrations` journal) opens under Rust and runs `apply_only` with zero migrations pending.
- **Oracle capture (Agent 18):** the reference binary exists (`/root/.opencode/bin/opencode`, 1.18.13). Have Agent 18 capture a real reference-created DB (a small session + messages + a durable event) and check it in as `crates/oc-database/tests/fixtures/reference.db`; then test open/migrate/read under Rust. This closes the one audited gap ("reference-created DB opens").
- **Byte-level JSON fixtures:** `JSON_COLUMNS` round-trip fixtures asserting `JSON.stringify`-equivalent bytes (fixes DB-004/DB-005), including object key order under `preserve_order`.
- **Export/import fixture:** a captured `opencode export` JSON re-imported and re-exported byte-identically (mirrors `export.ts`/`import.ts`), with `--sanitize` redaction cases.

---

## 7. Files to change (implementation phase — not in this wave)

- `crates/oc-database/Cargo.toml` — add `serde_json = { features = ["preserve_order"] }` (DB-005); `crates/oc-database/src/sqlite.rs:170-194` — JSON-stringify every non-null JSON-mode value (DB-004); `tables.rs` — confirm `SessionRow`/`MessageRow` mapping helpers for `SessionDb`.
- `crates/oc-core/src/durable.rs` (+ new `sqlite_store.rs`) — `SqliteDurableStore`.
- `crates/oc-core/src/credential.rs` (+ `sqlite_credential.rs`) — `SqliteCredentialStore`.
- `crates/oc-core/src/project/directories.rs` (+ `sqlite_directories.rs`) — `SqliteProjectDirectoryStore`.
- `crates/oc-core/src/db/mod.rs` (new) — shared `DatabaseHandle` accessor (lazy open, `Arc<Database>`).
- `crates/oc-session/Cargo.toml` + `crates/oc-session/src/store.rs` — `SqliteSessionDb` (+ row→`SessionInfo`/`MessageRow` conversion).
- `crates/oc-sync/src/sync/store.rs` — back `Db` with the SQLite event tables.
- `crates/oc-server/src/state.rs` — DB-backed sessions projection + `permission` saved store; `handlers/permission.rs:127-139` — real list/remove.
- `crates/oc-cli/src/cli/cmd/db.rs` — `db path` calls `oc_database::database::path()` (DB-003); `db <query>` json/tsv via `Database`; `db` spawns `sqlite3` shell (reference `db.ts`).
- `crates/oc-cli/src/cli/cmd/{session,export_cmd,import_cmd,stats}.rs` — SESSION-001 + export/import/stats over `SessionStore`/`Database` (mirror `session.ts`, `export.ts`, `import.ts`, `stats.ts`).

---

## 8. Dependencies on other agents

- **Agent 02 (composition root):** my `DatabaseHandle` + store impls are consumed by their runtime graph; contract: open once, `Services::build_with(...)`, thread `Arc<Database>` into `AppState`. Merge after they define the seam.
- **Agent 01 (schema promotion):** `oc-sync` and `oc-core` duplicate event/type models — dedupe (or at least align the row types) so one SQLite event store serves both; needs their `oc-schema` promotion first.
- **Agent 07 (runner/tools):** runner must write `session_message`/`message`/`part`/`session_input`/`session_context_epoch` projection rows; my `SessionDb` reads them. Coordinate write-side ownership.
- **Agent 10 (server):** consumes DB-backed stores for session/permission handlers.
- **Agent 12 (CLI):** session/export/import/stats/db command handlers sit in `oc-cli`; I supply the DB access + store layer, they own CLI surface/formatting.
- **Agent 08 (security):** owns allow/ask/deny evaluation (SEC-001); I own the `permission` table read/remove + persistence of saved decisions.
- **Agent 18 (testing):** reference-capture fixtures (`.db`, export JSON) for §6.

---

## 9. Risks

- **R1 — async rusqlite.** Blocking `Connection` in async code; mitigated by `spawn_blocking` everywhere (§4). Residual: `block_on` driving projectors inside a tx can misbehave if a projector awaits a runtime-bound future — validate early with a real projector test.
- **R2 — transaction deadlock.** Nested DB calls from projectors/commit hooks on the single connection self-deadlock (also true in the reference). Enforce via docs + a test that panics on re-entry.
- **R3 — concurrency.** One connection serializes all DB work; WAL + `busy_timeout=5000` handle cross-process. Single-connection throughput is fine for a session engine but must not be called in hot loops; keep queries off the async hot path.
- **R4 — migration ordering.** Migrations must complete before any store is used; server startup must force-open (fail fast) before the router serves; lazy CLI open must not race the server (two processes → SQLITE_BUSY handled by busy_timeout).
- **R5 — dual writers to `event`/`event_sequence`** (oc-core bus vs oc-sync store). Must pick one owner or route both through one impl (R6 below), else sequence races.
- **R6 — oc-sync duplication.** Its hand-rolled `Store` reimplements durable logic; keep its API, back it with the same tables, and coordinate sequence allocation (both currently start at `-1`/`0`). Risk of divergent idempotency semantics.
- **R7 — JSON bytes.** DB-004/DB-005 fixes change stored bytes; existing reference-created rows are already `JSON.stringify`-encoded, so the fix *aligns* Rust with them — verify against the captured reference DB fixture.
- **R8 — command parity.** `session list` pager, `export --sanitize`, `stats` aggregation formatting are surface-level but easy to get byte-wrong; pair with Agent 18 differential fixtures.

---

## 10. Merge-order recommendation

- **Wave 1:** Agent 02 composition root + Agent 01 type promotion (defines where the DB opens; schema/type alignment). Agent 03 can land the `oc-database`-internal fixes (DB-004/DB-005) and the new store impl modules in parallel without being wired.
- **Wave 2 (this domain):** Agent 03 wiring — `SqliteDurableStore`, credential/directories/session-db/sync/server stores, CLI `db`/`session`/`export`/`import`/`stats` over the DB, `db path` fix. Merge as one slice **after** the Wave-1 composition-root seam exists so the single-open design holds.
- **Wave 3:** Agent 07 runner projection writes + Agent 08 permission write path consume the stores; Agent 18 differential fixtures gate release.

Recommended gate: restart-persistence test (create → restart → list) green + reference-DB fixture opens + `db path` channel parity, before DB-001 is marked fixed.
