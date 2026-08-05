# Agent 02 — Reference Feature Inventory

Auditor: Agent 02 (Feature Inventory). Repo: `/root/opencode-rs` (Rust port, target `v1.18.13`).
Reference (read-only spec): `/root/opencode-rs/reference` (TypeScript/Bun monorepo @ v1.18.13).
Reference oracle binary: `/root/.opencode/bin/opencode` (reports 1.18.13).
Rust binary: `/root/opencode-rs/target/release/opencode`.

## Scope

Inventory every user-facing and server-facing feature from the reference **source**
(`packages/opencode/src`, `packages/core/src`, `packages/server`, `packages/schema`,
`packages/protocol`, `packages/tui`, `packages/plugin`) and classify the Rust port's
implementation status on the evidence ladder (type → function → unit-tested →
crate-exposed → called-by-other-crate → reached-by-executable → workflow → parity).
Both static analysis and black-box runtime probes of the Rust binary were used.
The deliverable is `FEATURE-PARITY.csv` (155 feature rows) plus this prose report.

## Repository areas inspected

- Reference CLI: `reference/packages/opencode/src/index.ts` and `src/cli/cmd/*.ts` (25 command files).
- Reference server: `reference/packages/server/src/{api,routes,cors,auth,handlers/*}.ts` and
  `reference/packages/opencode/src/server/`.
- Reference database: `reference/packages/core/src/database/schema.gen.ts` (19 tables) +
  38 migration files under `database/migration/`.
- Reference config: `reference/packages/opencode/src/config/config.ts` and
  `reference/packages/core/src/config/*`.
- Reference tool registry: `reference/packages/core/src/tool/*.ts` and `reference/packages/opencode/src/tool/*.ts`.
- Reference flags/env: `reference/packages/core/src/flag/flag.ts`, `reference/packages/core/src/global.ts`.
- Reference schema/events: `reference/packages/schema/src/event-manifest.ts`, `src/v1/session.ts`.
- Rust: all 20 crates under `crates/oc-*` (source, tests, Cargo.toml dependency edges).
- Runtime: `target/release/opencode` invoked for `--version`, `--help`, default command, `run`,
  `providers list`, `models`, `stats`, `agent`, `session`, `db path`, `db <query>`, `export`,
  `import`, `completion`, `upgrade`, `uninstall --help`, `debug paths/config/info/startup`.

## Commands executed

Outputs saved under `rust-port-audit/artifacts/`:
- `02-ref-help.txt` / `02-rust-help.txt` — top-level help of both binaries (compared).
- `02-workspace-tests.txt`, `02-workspace-summary.txt` — full `cargo test --workspace` results (1519 passed, exit 0).
- Runtime probes of `opencode run/version/providers/models/stats/agent/session/db/export/import/completion/upgrade/debug` (in report text).
- `cargo test -p oc-server`, `-p oc-database`, `-p oc-config`, `-p oc-session`, `-p oc-command`, `-p oc-provider`.

## Runtime scenarios attempted

| Scenario | Reference | Rust binary | Verdict |
|---|---|---|---|
| `opencode --version` | 1.18.13 | 1.18.13 | MATCH |
| `opencode --help` surface | 24 commands + 17 global/tui options | identical command set + options | MATCH |
| `opencode providers list` | credential table | credential table + env vars | MATCH |
| `opencode models` | models.dev catalog, opencode-first | same sort + refresh works | MATCH (except config providers) |
| `opencode db path` | `$data/opencode.db` | `/root/.local/share/opencode/opencode.db` | MATCH |
| `opencode run "hello"` | runs session, streams events | FAILS: "in-process opencode server is not wired yet" | MISMATCH |
| `opencode` (default TUI) | launches TUI | prints "starting TUI (requires a TTY)" / not_wired | MISMATCH |
| `opencode stats / session / agent / export / completion` | real output | "not yet wired" errors | MISMATCH |
| `opencode serve` | serves HTTP API | binds bare TCP socket, no HTTP | MISMATCH |

## Architecture or behavior summary

The Rust port is a **20-crate decomposition whose crates are largely isolated silos**:

1. **The CLI surface is a faithful mirror.** `oc-cli/src/cli/args.rs` reproduces every command,
   subcommand, flag, alias (`auth`→providers, `ls`, `plug`→plugin) and default value from the
   yargs builders. `main.rs` mirrors index.ts middleware (`AGENT`/`OPENCODE`/`OPENCODE_PID`,
   `--print-logs`/`--log-level`/`--pure` → env), help rendering and error/exit behavior.
2. **The server route tree is a faithful mirror.** `oc-server/src/router.rs` + golden test
   `tests/route_table.rs` prove the full v1+v2 route set matches the reference HttpApi groups.
3. **Schema and database mirror the reference.** `oc-schema` covers the schema package; `oc-database`
   reproduces all 19 tables and all 38 reference migrations with passing golden tests.
4. **BUT zero cross-crate integration exists in code.** No `oc_*` crate imports another `oc_*` crate.
   `oc-cli/Cargo.toml` declares all 18 other crates as dependencies and `oc-server/Cargo.toml`
   declares 10, yet none of them are referenced in any `use` statement. Each crate is tested in
   isolation (1519 tests pass) but nothing connects them.
5. **Consequence:** the executable is a thin self-contained CLI. Only a handful of commands reach
   real behavior (`providers list/login/logout`, `models`, `db path`, `debug paths/info/startup/file`,
   `uninstall` directory removal, argument validation in `run`/`attach`). Everything requiring the
   engine — TUI, `serve`, `web`, `acp`, `run` (local), `session`, `stats`, `export/import`, `mcp`,
   `plugin`, `github`, `db` queries — returns "not yet wired in this build (TODO(integration): ...)".
6. **The oc-server handlers operate on in-memory stores with `serde_json::Value`** and its own
   `crate::schema` types; `session_prompt` only appends a user message — there is no LLM invocation,
   no tool execution, no assistant turn, no persistence.

## Positive observations

- **CLI parity is genuinely excellent**: every command/subcommand/flag/alias/default is present and
  `--help` output is nearly byte-comparable (logo, ordering, defaults).
- **Route table parity is proven by a passing golden test** (v1 + v2, ~240 routes).
- **Database parity is proven**: 19/19 tables, 38/38 migrations, schema golden test passes.
- **Schema/part/event types are comprehensively ported** (`oc-schema`, `v1/session.rs` part union).
- **Each crate carries real, tested logic**: oc-config load pipeline (discovery, JSONC, merge,
  variable substitution, legacy TOML, managed dir), oc-tool registry + 20 tools, oc-llm wire
  protocols (Anthropic/OpenAI/Gemini/Bedrock), oc-mcp client (stdio/SSE/HTTP + OAuth), oc-acp
  (7.9k lines), oc-tui ratatui app (15k lines), oc-provider (129 tests), oc-plugin QuickJS host.
- **The full workspace compiles and 1519 tests pass** (one flaky time-based ID test
  `oc-session/src/identifier.rs::high_timestamp_wraps_48_bits_without_panicking` failed on one run,
  passed on re-run).

## Findings summary

| Status | Count | Meaning |
|---|---|---|
| IMPLEMENTED_CONNECTED | 20 | Real executable path + reference behavior (CLI parse/help/version, providers list/login/logout, models, db path, debug paths/info/startup/file read/list, uninstall dirs, file attach logic, heap) |
| IMPLEMENTED_DISCONNECTED | 61 | Real code + tests, but not reached by any executable path (all engine crates: config, database, server router/handlers, session, runner, llm, provider, tool, mcp, acp, plugin, sync, project, command, tui, client) |
| PARTIAL | 32 | Some sub-behavior works, core path missing (run non-interactive via --attach only, upgrade, uninstall binary removal, import validation only, pr checkout only, auth no-OAuth) |
| STUB | 34 | Parses args, then returns `not_wired` (completion, serve, web, acp, session, stats, export, mcp, plugin, agent, github, debug subcommands, generate, console, tool/pty endpoints) |
| MISSING | 6 | No code found (remote config fetch, config watcher, OAuth plugin methods, export sanitize, share-next, background subagents) |
| UNVERIFIED | 1 | oc-tui session-list UI (cannot launch TUI) |
| INTENTIONALLY_EXCLUDED | 1 | Installer (single binary by design) |

Severity of the 155 rows: Critical 7, High 59, Medium 52, Low 33, Informational 4.

## Detailed findings

### [PARITY-001] No crate imports another crate — the engine is not assembled (Critical)
All 20 crates compile and pass tests in isolation, but `grep 'use oc_...::'` across
`crates/*/src/` finds zero cross-crate imports. `oc-cli/Cargo.toml` declares 18 sibling deps;
`oc-server/Cargo.toml` declares 10 (oc-session, oc-tool, oc-llm, oc-provider, oc-plugin, oc-acp,
oc-sync, ...) — none used in code. The server uses its own `crate::schema::SessionInfo` and raw
`serde_json::Value` (state.rs:14). Impact: every engine feature is IMPLEMENTED_DISCONNECTED at best.
Evidence: `grep -r "^use oc_"` (only `oc_cli::` in main.rs; doc-comment `use oc_client` in oc-client/lib.rs).

### [PARITY-002] `opencode run` cannot run locally — LocalClient is a stub (Critical)
`run/client.rs:62-70` `LocalClient::create` returns "the in-process opencode server is not wired yet".
`run/mod.rs` is otherwise a faithful port (message resolution, file attach, session resolve/fork,
event loop, `--format json`, share). Only `--attach <url>` works. Reference `run.ts:127` runs an
in-process server by default. RUNTIME verified: `opencode run "hello"` fails.

### [PARITY-003] Default TUI and `--mini` are not launched (Critical)
`attach.rs:91-171` `run_default_tui` validates flags then returns `not_wired` (TUI). The 15k-line
`oc-tui/src/app.rs` `run_async` exists but is never invoked by the binary. Reference
`cli/cmd/tui.ts:73` launches the TUI thread for `opencode [project]`.

### [PARITY-004] `serve`/`web`/`acp` bind a socket but serve nothing (Critical)
`serve.rs:40-67` binds a TCP listener and never accepts HTTP; `web.rs:32-83` binds then exits with
"(web interface not yet wired)"; `acp.rs:12-27` binds then `pending()` forever. Reference commands
start `Server.listen` (opencode/src/server/server.ts) plus the ACP `AgentSideConnection` bridge.

### [PARITY-005] No LLM execution path exists (Critical)
`instance_handlers.rs:461-502` `session_prompt` appends a user message to the in-memory store and
returns; no provider call, no streaming, no tool loop. `oc-session-runner` (`runner/llm.rs`)
defines `SessionRunnerService` over trait-object `RunnerDeps` but only `MockTools`/`MockModels`
implement them (tests/runner_loop.rs). The reference core loop (`core/src/session/runner/llm.ts`)
is the heart of the product and has no live execution path in Rust.

### [PARITY-006] Session/message/part data is in-memory, never persisted (High)
Server handlers read/write `state.stores` (HashMap) only. `oc-database` implements the SQLite
layer and all 38 migrations, and `oc-session` implements the store logic, but nothing connects
them; `SessionDb` is only implemented by `MemDb` in tests (oc-session/src/service.rs:194).

### [PARITY-007] Config engine is not wired (High)
`oc-config/src/load.rs` is a high-fidelity port of config.ts (discovery, JSONC, deep merge,
variable substitution, plugin spec resolution, commands/agents/modes discovery, legacy TOML
migration, managed dir, `OPENCODE_PERMISSION`, autoshare). But `debug config` returns not_wired
(debug.rs:38-44) and `serve.rs`'s `server_config()` returns `None`. `load_instance_state` is never
called by any production path.

### [PARITY-008] Server route tree matches, handlers are test-only (High)
`tests/route_table.rs::route_table_matches_reference` passes (golden). But the router is only
exercised via `tower::ServiceExt::oneshot` in tests; `serve` doesn't use `oc-server`. Handler
fidelity varies: session create/list/get shapes match reference tests; `/experimental/tool` returns
`[]` (instance_handlers.rs:1140-1150) though oc-tool implements a full registry.

### [PARITY-009] Tool registry not connected (High)
`oc-tool/src/core/registry.rs` implements register/materialize with tests, but nothing calls it;
session-runner defines its own `ToolRegistry` trait. The reference materializes tools into each
prompt turn (core/src/tool/registry.ts:106).

### [PARITY-010] Export/import/share/stats/session CLIs are stubs (High)
`export_cmd.rs`, `session.rs`, `stats.rs`, `import_cmd.rs`(persistence), `mcp.rs`, `plug.rs`,
`github.rs` all return `not_wired`. Reference implementations are substantial (export JSON +
sanitize, session list table/pager, stats tables, mcp OAuth flows).

### [PARITY-011] Project ID algorithm diverges (High)
`context.rs:50-55` uses Rust `DefaultHasher` for project IDs; the reference derives IDs from
worktree/VCS with its own scheme (opencode/src/project/project.ts:220-242) and persists them.
Storage layout parity is broken.

### [PARITY-012] Auth covers api/wellknown but not OAuth (High)
`auth.rs` round-trips `auth.json` at the correct path with 0600 perms and the correct Info union
(oauth/api/wellknown). CLI login implements API-key and well-known flows; plugin OAuth methods are
a TODO (providers.rs:209) and echo-hiding is missing (providers.rs:267).

### [PARITY-013] `uninstall` omits binary/shell/package cleanup (High)
`uninstall.rs` removes data/cache/config/state dirs with keep/dry-run/force, but never removes the
binary, cleans shell PATH, or runs the package-manager uninstall that reference uninstall.ts:144-230 performs.

### [PARITY-014] Env-var coverage ~26/80 (Medium)
Rust handles the core `OPENCODE_*` flags (config/dir/content/permission/log-level/print-logs/pure/
server-password/username/db/test-home/test-managed-config-dir) but not DISABLE_TERMINAL_TITLE,
SHOW_TTFD, DISABLE_MOUSE, FAKE_VCS, WORKSPACE_ID, TUI_CONFIG, DISABLE_MODELS_FETCH,
EXPERIMENTAL_* (flag.ts:19-70).

### [PARITY-015] One flaky test (Medium)
`oc-session/src/identifier.rs:196-201` `high_timestamp_wraps_48_bits_without_panicking` injects
`u64::MAX` as a base timestamp; when combined with the current wall clock it can exceed 48 bits and
panic. Failed once in the full workspace run, passed on re-run. (Related to the same reference-ID
algorithm as PARITY-011.)

### [PARITY-016] mDNS is a best-effort beacon, not a responder (Medium)
`oc-server/src/mdns.rs` sends a multicast beacon with a TODO for a real responder; reference
`server/mdns.ts` implements full service advertisement.

## Feature or behavior gaps (biggest, ranked)

1. **No executable LLM/tool/session loop** — the core product workflow cannot run locally.
2. **No TUI, no serve/web/acp** — all four interactive/headless entry points are stubs.
3. **No persistence wiring** — DB exists (schema/migrations/SQLite) but the runtime never opens it.
4. **Config engine unplugged** — full loader exists but nothing consumes it.
5. **Project/session/share/stats/export subsystems unplugged** — logic present, entry points stubbed.
6. **Update/install/uninstall binary lifecycle incomplete.**

## Test coverage gaps

- No end-to-end tests run the actual `opencode` binary beyond argument parsing/help (I verified at runtime).
- No golden comparison of CLI output against the reference binary (except route table + schema tests).
- No LLM smoke test against a provider; no live SSE event-loop test against the reference server.
- No TUI rendering harness launched in a PTY.
- Flaky time-dependent ID test needs deterministic clock injection.

## Unverified areas

- `oc-tui` visual/behavioral parity (cannot launch without wiring + PTY).
- Live provider streaming behavior of `oc-llm` (protocols unit-tested only).
- Remote config fetch and console/control-plane workflows (MISSING in Rust).
- Cross-platform behavior (only Linux/x86_64 exercised; macOS/Windows paths in global.ts/managed dir are mirrored statically but untested).

## Final domain verdict

**NOT_READY**

The CLI *surface*, server *route table*, and *database schema/migrations* are parity-excellent and
individually well-tested (1519 passing tests), but the port is architecturally unassembled: no
`oc_*` crate calls another, no executable path reaches the config, database, session, LLM, tool,
MCP/ACP, plugin, or TUI crates, and the core user workflow (`opencode run`, default TUI, `serve`,
`acp`) does not function. A release cannot be cut until the integration layer exists and the
engine entry points are wired.

Deliverables written:
- `/root/opencode-rs/rust-port-audit/FEATURE-PARITY.csv` (155 feature rows).
- This report (`/root/opencode-rs/rust-port-audit/02-reference-feature-inventory.md`).
- Artifacts under `/root/opencode-rs/rust-port-audit/artifacts/02-*`.
