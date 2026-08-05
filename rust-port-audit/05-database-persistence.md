# Agent 05 — Database, Persistence, Migrations, and Data Integrity

**Auditor:** Agent 05 · **Repo:** `/root/opencode-rs` (read-only) · **Reference:** `/root/opencode-rs/reference` v1.18.13 · **Date:** 2026-08-05

## Scope

Persistent state in the Rust port: the `oc-database` crate (rusqlite engine, schema,
migrations, path helpers), plus the consumers that *should* persist state
(`oc-core` durable store, `oc-session` store, `oc-sync` event store, `oc-server`
state, `oc-cli` `db` command). Audited: engine + connection lifecycle, schema DDL
parity vs `schema.gen.ts`, migration parity vs `migration.gen.ts` + `migration.ts`,
transaction/atomicity/locking, crash consistency, journal/drizzle compatibility,
serialization fidelity, path/type-column behavior, and whether the *executable* ever
opens the database.

## Repository areas inspected

- `crates/oc-database/src/{lib,database,schema,tables,sqlite,path,error}.rs`
- `crates/oc-database/src/migration/{mod,gen}.rs` + 38 per-migration files
- `crates/oc-database/tests/{schema_golden,data_access,migrations}.rs`, `tests/fixtures/schema.sql`
- Reference: `reference/packages/core/src/database/{schema.gen.ts, schema.sql.ts, migration.ts, migration.gen.ts, database.ts, path.ts, sqlite.ts, sqlite.bun.ts, sqlite.node.ts}`, `reference/packages/core/src/event/sql.ts`, all 38 reference migrations, `reference/packages/core/test/database-migration.test.ts`, `reference/packages/opencode/src/cli/cmd/db.ts`, `reference/packages/core/src/global.ts`
- Consumers: `crates/oc-core/src/{durable.rs, bus.rs, context.rs, credential.rs, project/directories.rs}`, `crates/oc-session/src/store.rs`, `crates/oc-sync/src/sync/{store.rs, sql.rs}`, `crates/oc-cli/src/cli/cmd/{db,session,export_cmd,import_cmd,stats}.rs`, `crates/oc-server/src/state.rs`

## Commands executed

- `cargo test -p oc-database` (workspace) — **PASSED**: 6 unit + 1 `data_access` + 13 `migrations` + 1 `schema_golden` = 21 tests, 0 failures. Doc-tests 0.
- `cargo build -p oc-cli` — OK (compiles whole graph, incl. oc-database).
- `target/debug/opencode db path` (+ variants: `OPENCODE_DB` abs/rel/`:memory:`, `OPENCODE_CHANNEL=canary`) — runtime.
- `target/debug/opencode db "SELECT 1" --format json` — runtime (stub).
- Custom throwaway crate `/tmp/ocdb-check` (path-dep on oc-database, shared target dir) exercising: full 38-migration replay from scratch; `Database::open` PRAGMA state; drizzle-journal import; FK cascade; event unique constraint; JSON-mode serialization of primitive + object values.
- Python migration-SQL comparison: extracted all `tx.run(\`…\`)` templates from each reference `.ts` and all Rust `run_batch`/`run_exec` strings; canonicalized (whitespace + trailing `;` stripped) and diffed — **38/38 semantically identical**.
- `rg`/`grep` workspace-wide for `oc_database` usage and `Database::open` call sites.

## Runtime scenarios attempted

| Scenario | Result |
|---|---|
| `opencode db path` (no env) | `~/.local/share/opencode/opencode.db` (exit 0) |
| `opencode db path` with `OPENCODE_CHANNEL=canary` | **still** `opencode.db` (reference would print `opencode-canary.db`) |
| `opencode db path` with `OPENCODE_DB=relative.db` | `~/.local/share/opencode/relative.db` (OK) |
| `opencode db "SELECT 1" --format json` | exit 1: "database queries are not yet wired in this build (TODO(integration): oc-database)" |
| `opencode db` (interactive) | exit 1: same not_wired stub |
| `cargo test -p oc-database` | all pass |
| Full 38-migration replay via `apply_only` (fresh file) | 38 applied, 20 tables, no errors |
| `Database::open` PRAGMAs | `journal_mode=wal`, `foreign_keys=1` |
| Drizzle `__drizzle_migrations` → `migration` seed | imported id correctly |
| FK cascade `DELETE session` → message/part | cascaded (0 orphaned rows) |
| Duplicate `(aggregate_id, seq)` event | rejected: `UNIQUE constraint failed` |
| JSON column, primitive value (`prompt="hello"`) | stored as bare `hello` (not JSON `"hello"`); read-back parse would fail |
| JSON column, object value | stored; keys **re-ordered alphabetically** in standalone build |

## Architecture or behavior summary

The `oc-database` crate is a faithful, high-quality port of the reference database
module. It is a single-connection SQLite client (`rusqlite`, bundled, `Mutex<Connection>`
mirroring the reference's 1-permit Effect semaphore), applying the exact reference
PRAGMA battery (WAL, synchronous=NORMAL, busy_timeout=5000, cache_size=-64000,
foreign_keys=ON, wal_checkpoint(PASSIVE)) and running the reference migration
algorithm: empty DB → embedded `schema.up` + `migration` journal pre-filled; DB with a
`session` table → `applyOnly` (per-migration transactions, journal insert inside the
transaction, drizzle-journal import when the journal is empty); any other non-empty DB
→ rejected. Global init lock mirrors the reference's module semaphore.

**However, none of it is reachable from the production executable.** `Database::open`
has zero non-test call sites; `oc_database::` symbols are referenced in no crate other
than oc-database itself. The durable event bus uses `InMemoryDurableStore`
(`oc-core/src/bus.rs:86`, `context.rs:54`), the server uses in-memory projection stores
(`oc-server/src/state.rs:24`), oc-sync keeps definitions/sequences in `HashMap`s, and
every DB-backed CLI command (`db query`, `db` shell, `session ls/delete`, `export`,
`import`, `stats`) returns `not_wired(...)`. Sessions, messages, events, and
credentials are **never persisted** in the current build.

## Positive observations

- **Schema DDL parity (runtime):** `tests/schema_golden.rs::schema_matches_reference_ddl`
  passes — `schema::TABLES` (19) + `INDEXES` (16) reproduce the `fixtures/schema.sql`
  DDL (derived from `schema.gen.ts`) byte-for-byte as SQLite stores it
  (`crates/oc-database/tests/schema_golden.rs:57`).
- **Migration inventory parity:** all 38 reference migration ids present in
  `migration/gen.rs`, in the exact `migration.gen.ts` order (including the
  hyphenated ids `20260410174513_workspace-name` and `20260511173437_session-metadata`,
  and the `20260530232709_lovely_romulus` temporary-replacement compatibility).
- **Migration SQL parity:** automated extraction/diff of all 38 migrations shows
  semantic identity with the reference TS (differences limited to trailing
  `;`/whitespace that SQLite normalizes away). The `PRAGMA table_info(session)`
  guard in `session-metadata` and the raw-string `WHERE "credential"."active" = 1`
  partial index in `credential` are both present (`m20260511173437_session_metadata.rs:10`,
  `m20260611035744_credential.rs:22`). Table-rebuild migrations toggle
  `PRAGMA foreign_keys=OFF/ON` exactly like the reference (a mid-transaction no-op in
  SQLite — identical ineffectiveness in both, so parity is preserved).
- **Runtime full-replay:** applying all 38 migrations in sequence on a fresh DB
  succeeds and yields 20 tables (19 + `migration`), matching the reference layout.
- **Persistence invariants (runtime):** FK cascades enforced (session → message/part
  delete cascade, no orphans); unique index `event_aggregate_seq_idx` rejects
  duplicate `(aggregate_id, seq)`; drizzle journal import + idempotency guarded;
  non-empty-without-session DB rejected (`mod.rs:104-107`).
- **Path column codec:** `path.rs` faithfully ports `path.ts` (absolute validation,
  Windows `\`→`/` storage normalization, `absoluteArray` JSON round-trip,
  empty-directory legacy tolerance).
- **Migration runner correctness:** embedded init and per-migration journal insert
  share one transaction, so a failed/interrupted migration rolls back atomically and
  re-runs on next start — matches reference `migration.ts:71-78`.
- **Library `path()` is correct:** `database.rs::path()` honors `OPENCODE_DB`
  (`:memory:`/absolute/relative), `OPENCODE_CHANNEL`, and
  `OPENCODE_DISABLE_CHANNEL_DB` and is unit-tested (`database.rs:193`); data dir
  `~/.local/share/opencode` matches reference `global.ts` xdg path.
- **Concurrency:** process-wide `APPLY_LOCK` serializes embedded init (tested with 8
  concurrent opens in `migrations.rs:576`), per-connection mutex + `busy_timeout`
  mirror the reference's semaphore + PRAGMA.

## Findings summary

| ID | Severity | Title | Confidence |
|---|---|---|---|
| DB-001 | Critical | Production executable never opens the database; nothing is persisted | CONFIRMED |
| DB-002 | High | `oc-database` is a declared-but-unused dependency (dead scaffold) | CONFIRMED |
| DB-003 | Medium | `opencode db path` ignores `OPENCODE_CHANNEL` / `OPENCODE_DISABLE_CHANNEL_DB` | CONFIRMED |
| DB-004 | Low | JSON-mode columns store primitive values raw instead of JSON-stringified | CONFIRMED |
| DB-005 | Low | Stored JSON key order depends on `serde_json/preserve_order` (build-dependent) | CONFIRMED |
| DB-006 | Info | Sensitive values at rest in plaintext; 0644 file mode (matches reference) | CONFIRMED |
| DB-007 | Info | No corrupt-DB detection/recovery beyond SQLite defaults (matches reference) | CONFIRMED |
| DB-008 | Info | Fresh-install journal timestamps shared vs per-migration (cosmetic) | CONFIRMED |
| DB-009 | Info | `load_extension` uses `unsafe`/dlopen; not reachable on production path | CONFIRMED |

## Detailed findings

### DB-001 — Critical — The executable does not use the database; persistence is absent
- No production call sites of `oc_database::` anywhere outside `crates/oc-database`
  itself (verified by `rg "oc_database"` across the workspace excluding tests). The
  only references in other crates are comments/TODO strings
  (`crates/oc-core/src/durable.rs:6`, `crates/oc-session/src/store.rs:6`,
  `crates/oc-sync/src/sync/store.rs:12`, …).
- `Database::open` is invoked only by `oc-database`'s own tests
  (`crates/oc-database/src/database.rs:231`, `tests/migrations.rs:584`).
- The durable event bus defaults to `InMemoryDurableStore`
  (`crates/oc-core/src/bus.rs:86`, `crates/oc-core/src/context.rs:54`); no
  SQLite-backed `DurableStore` implementation exists anywhere.
- `oc-server` state is explicitly "In-memory projection store"
  (`crates/oc-server/src/state.rs:24`); oc-sync durable definitions/sequences live in
  `HashMap`s (`crates/oc-sync/src/sync/store.rs:47-50,167`).
- DB-backed CLI commands are stubbed: `db query` / interactive shell
  (`crates/oc-cli/src/cli/cmd/db.rs:32-42`), `session ls/delete` (`session.rs:11-17`),
  `export` (`export_cmd.rs:7-12`), `import` (`import_cmd.rs:51`), `stats`
  (`stats.rs:264-266`).
- **Runtime proof:** `opencode db "SELECT 1" --format json` → exit 1,
  "database queries are not yet wired in this build".
- Impact: sessions, messages, events, credentials, permissions, todos are lost on
  restart. Violates the port's stated 1:1 functional parity goal.

### DB-002 — High — Declared-but-unused dependency
- `oc-database` is listed in `crates/oc-core/Cargo.toml:26`, `crates/oc-cli/Cargo.toml:22`,
  `crates/oc-sync/Cargo.toml:24`, yet no crate compiles any `oc_database::` symbol.
  These are forward-looking edges for integration that is entirely TODO.

### DB-003 — Medium — `opencode db path` channel handling diverges
- `crates/oc-cli/src/cli/cmd/db.rs:12-21` only honors `OPENCODE_DB` and otherwise
  returns `data/opencode.db`. Reference `Database.path()`
  (`database.ts:43-55`) returns `opencode-{channel}.db` for non-prod channels and
  honors `OPENCODE_DISABLE_CHANNEL_DB`.
- **Runtime proof:** with `OPENCODE_CHANNEL=canary`, Rust prints `opencode.db`;
  reference would print `opencode-canary.db`. The correct logic exists in the library
  (`crates/oc-database/src/database.rs:14-44`, unit-tested) but the CLI re-implements a
  wrong subset instead of calling it.

### DB-004 — Low — JSON-mode columns: primitives not JSON-stringified
- `crates/oc-database/src/sqlite.rs:170-194` (`json_to_sqlite`): for `as_json` columns
  only objects/arrays are serialized; strings/numbers/bools/`null` are stored raw.
  Reference drizzle `{mode:"json"}` always stores `JSON.stringify(value)` (e.g. a
  string stored as `"hello"`).
- **Runtime proof:** `session_input.prompt="hello"` persisted as `hello`; the read
  path (`sqlite.rs:135-140`) would then fail `serde_json::from_str("hello")`.
- Impact limited because real payloads (`prompt` objects, message/part `data`
  objects, `metadata`, arrays) are non-primitive. Still a latent round-trip trap and a
  byte-level divergence from reference-created rows.

### DB-005 — Low — JSON key ordering is build-dependent
- serde_json defaults to a `BTreeMap` (sorted keys) unless the `preserve_order`
  feature is unified on. `oc-core`, `oc-llm`, `oc-config` enable `preserve_order`
  (`crates/oc-core/Cargo.toml:10`), so workspace builds preserve insertion order, but
  a standalone `oc-database` build does not.
- **Runtime proof (standalone build):** `{"zebra":1,"alpha":{"y":2,"a":3},"mid":[1,2]}`
  was stored as `{"alpha":{"a":3,"y":2},"mid":[1,2],"zebra":1}`.
- Semantically equivalent JSON, but byte-for-byte stored data differs from the
  reference and varies between build configurations.

### DB-006..009 — Informational
- **DB-006** Account tokens, `credential.value`, and `session_share.secret` are
  plaintext at rest; files created 0644 (default umask). Identical to the reference
  (no chmod/encryption in either). Symlink-following of `OPENCODE_DB` matches.
- **DB-007** No `PRAGMA integrity_check` / explicit corrupt-file handling; recovery
  relies on SQLite WAL/journal rollback. Matches reference; corrupt-DB path untested.
- **DB-008** Fresh-install journal inserts all ids with one shared `now_ms()`
  (`crates/oc-database/src/migration/mod.rs:114`) vs the reference's per-migration
  `Date.now()`. Cosmetic only.
- **DB-009** `load_extension` (`sqlite.rs:342-355`) uses `unsafe` + dlopen with a
  caller-controlled path; mirrors the reference trust boundary and is unreachable in
  the current binary (no caller).

## Feature or behavior gaps

1. **No persistence in the executable** (DB-001/002) — the dominant gap; every
   reference feature that reads/writes SQLite (sessions, messages, durable events,
   credentials, permissions, todos, sync) is either stubbed or in-memory.
2. `db query` / interactive `sqlite3` shell unimplemented.
3. No SQLite-backed `DurableStore` for the event bus / oc-sync.
4. No credential store, `project_directory` store, or session store binding to
   oc-database (all `TODO(integration)`).
5. CLI `db path` does not use the correct channel-aware library `path()` (DB-003).

## Test coverage gaps

- Only ~10 of 38 migrations are exercised by the crate's own `apply_only` tests; the
  other ~28 (e.g. `workspace-name` rebuild, `chief_energizer`, `slow_nightmare`,
  `lowly_union_jack`, `project_dir_strategy`, `simplify_*`) execute only via my
  throwaway full-replay harness — not part of `cargo test`. Suggest a
  "replay every migration over its predecessor schema" test.
- No test runs all 38 migrations in sequence inside the crate's suite.
- No corrupt-DB, interrupted-migration, or cross-process contention tests.
- No Windows-path tests (reference has them; not runnable on Linux).
- No test asserts a reference (bun)-created DB file opens and migrates under Rust.

## Unverified areas

- **Reference-created DB compatibility end-to-end** — blocked: no bun runtime.
  DDL parity is proven (golden fixture + manual + automated diff), and identical DDL
  implies compatible file format, but opening an actual reference DB was not possible.
- Cross-process concurrent read/write behavior (WAL + busy_timeout only statically
  verified).
- Windows-specific `cfg!(windows)` branches in `path.rs`/`database.rs` untested on
  Linux.
- `db export`/serialize byte-image parity with reference `native.serialize()`.

## Final domain verdict

**NOT_READY.**

The `oc-database` crate itself is a faithful, well-tested port (DDL golden parity,
38/38 migration SQL parity, migration algorithm, PRAGMA battery, cascades, unique
constraints, journal import, atomicity, concurrency). But the domain under audit —
*database, persistence, and data integrity in the running port* — is not delivered:
the production executable never opens the database, persists nothing, and every
DB-backed command is a stub. Persistence must be wired (DB-001/DB-002) before the
domain can be considered functional, with DB-003 and DB-004/005 as follow-on
corrections.
