# Agent 01 — Architecture, Crate Boundaries, and Modularity

Auditor of the opencode-rs Rust port (workspace root `/root/opencode-rs`, 20 crates under `crates/oc-*`).
Reference spec: `/root/opencode-rs/reference` (vendored opencode v1.18.13, TS/Bun). Oracle binary:
`/root/.opencode/bin/opencode` (1.18.13). Rust release binary: `/root/opencode-rs/target/release/opencode`.
All evidence below is STATIC (source `file:line`) or RUNTIME (black-box execution) as labeled. Nothing was
verified against prior READMEs/CONTEXT.md claims; everything was re-derived from source or execution.

## Scope

Workspace architecture; crate dependency graph and dependency direction; cyclic conceptual deps; public
APIs and shared-domain types; duplicate domain models (local mirror types marked `TODO(integration)`);
conversion layers; feature flags; layering (CLI/app/domain/protocol/infra/persistence/providers/plugins/
tools/UI); meaningful-module vs artificial-splitting assessment; leaky abstractions; core logic depending on
CLI/UI; coupling/cohesion/fan-in/fan-out; god modules; dead/premature abstractions; excessive generics;
global state misuse; hidden init ordering; error-model consistency; whether the 20 crates integrate via
canonical shared types; whether crates compile only because of duplicate local models; remaining schema
promotion/integration work; whether the architecture supports end-to-end integration.

## Repository areas inspected

- Root `Cargo.toml` (workspace members + workspace.dependencies).
- All 20 `crates/oc-*/Cargo.toml` (oc-schema, oc-util, oc-config, oc-database, oc-core, oc-provider,
  oc-llm, oc-plugin, oc-mcp, oc-tool, oc-session, oc-session-runner, oc-acp, oc-server, oc-client,
  oc-tui, oc-command, oc-project, oc-sync, oc-cli).
- `crates/oc-cli/src/**` (main.rs, lib.rs, cli/cmd/{run,serve,acp,db,session,attach,models}.rs).
- `crates/oc-server/src/**` (instance_handlers.rs, handlers/session.rs, schema.rs, state.rs, router.rs,
  route.rs, event.rs, global_lifecycle.rs).
- `crates/oc-session/src/**` (v1.rs, v2.rs, schema.rs, store.rs, llm.rs, lib.rs).
- `crates/oc-schema/src/**` (lib.rs, session_message.rs, filesystem.rs, v1/session.rs, tests/golden_messages.rs).
- `crates/oc-client/src/types/**` (23 local DTO modules), `crates/oc-acp/src/**` (sdk.rs, service.rs, types.rs),
  `crates/oc-tui/src/**` (types.rs, client.rs), `crates/oc-session-runner/src/**` (run_coordinator.rs, execution_local.rs,
  llm/message.rs, session/schema.rs), `crates/oc-llm/src/**` (llm.rs, protocols/*, schema/messages.rs),
  `crates/oc-util/src/ripgrep/mod.rs`, `crates/oc-plugin/src/**` (lib.rs, host.rs, bridge.rs), `crates/oc-sync/src/**`
  (control_plane/global_bus.rs), `crates/oc-database/src/**`, `crates/oc-core/src/lib.rs`, `crates/oc-command/src/**`.

## Commands executed

All from `/root/opencode-rs` unless noted. Outputs saved under `rust-port-audit/artifacts/01-*`.

- `cargo metadata --format-version 1` → `01-cargo-metadata.json` (full package/dependency data).
- Python analysis of metadata: internal edges, topological sort (cycle detection), fan-in/fan-out
  → `01-dep-graph.txt`.
- `grep -rn "oc_<crate>::"` across `crates/*/src` to find actual production cross-crate usage.
- `grep -rln "pub struct/enum X"` across crates for duplicate-type inventory → `01-duplicate-types.txt`.
- `grep -rn "TODO(integration)"` (266 markers), `grep -rln "cfg(feature"` (none), `grep -rc unsafe`.
- Per-crate LOC: `find crates/oc-*/src -name '*.rs' | xargs wc -l` (167,496 total).
- `cargo check -p oc-session-runner -q` → OK (exit 0).
- `cargo check -p oc-server -p oc-acp -p oc-tui -q` → OK (exit 0).
- Runtime probes → `01-runtime.md`.

## Runtime scenarios attempted

1. `opencode run "say hi"` (Rust) → **Error: the in-process opencode server is not wired yet in this build
   (TODO(integration): oc-server)**. RUNTIME proof the primary command is non-functional.
2. `opencode serve --port 19999` (Rust) → prints "opencode server listening", but GET `/health`, `/session`,
   `/config` all return empty and `/` returns curl `000` (nothing served). RUNTIME proof the server is a stub.
3. Reference oracle `serve --port 20001` → GET `/health` returns the OpenCode SPA HTML (200), GET `/session`
   returns `[]`. The reference serves a real HTTP API; the Rust port serves nothing.
4. `opencode session list` (Rust) → "session listing is not yet wired in this build (TODO(integration): oc-database/oc-session)".
5. `opencode acp` (Rust) → binds a socket and blocks in `std::future::pending()` forever (oc-cli/src/cli/cmd/acp.rs:16).
6. `opencode db path` (Rust) → works (prints db path); `opencode db <query>` → "not yet wired".
7. `opencode models` (Rust) → "models database is empty; run `opencode models --refresh`".
8. `opencode` (no args, non-TTY) (Rust) → "opencode: starting TUI (requires a TTY)".

## Architecture or behavior summary

The workspace mirrors the reference package layout by name (oc-schema↔packages/schema, oc-llm↔packages/llm,
oc-session-runner↔core/session/runner, oc-server↔packages/server, etc.), and the internal dependency graph
declared in Cargo.toml is **acyclic and clean** (oc-schema and oc-util are the zero-dep foundations, fan-in
16 and 15; oc-cli is the fan-out 18 entry point).

However, **the declared graph is vestigial**: no production source anywhere in `crates/*/src` contains a single
`use oc_<crate>::` import. The only `oc_*::` references outside each crate's own `tests/` are doc comments
(e.g. oc-cli/src/cli/cmd/serve.rs:38 `TODO(integration): delegate to oc_server::Server::listen(opts)`) and
one internal re-export module (oc-client/src/generated.rs). Every crate therefore compiles as an island that
re-implements the domain logic of its declared dependencies with local mirror types. End-to-end integration
does not exist: the `opencode` binary (oc-cli) implements its own stubs — `run` fails at startup
(run/client.rs:64 `LocalClient` returns `Err("not wired yet")`), `serve` binds a bare socket, `acp` blocks in
`pending()`, `session`/`db`/`attach` return `not_wired`, and the TUI is not even linked (oc-tui is not a
dependency of oc-cli). oc-server does contain a complete axum router with session handlers, but
`session_prompt` (instance_handlers.rs:461) only appends the user message to an in-memory map and never
invokes any LLM; `session_create` (instance_handlers.rs:323) builds its own `SessionInfo`/`Tokens`/
`CacheTokens` from local `oc-server::schema` types. The heavy lifting crates (oc-session, oc-session-runner,
oc-llm with real Anthropic/Bedrock/Gemini/OpenAI protocol adapters, oc-provider registry, oc-tool, oc-database,
oc-sync, oc-acp, oc-plugin) are complete-but-disconnected libraries with no consumers in the binary.

## Positive observations

1. Internal dependency graph is acyclic; oc-schema/oc-util sit at the bottom with zero internal deps
   (foundation-style layering intent is correct).
2. oc-llm contains substantive wire-protocol implementations (anthropic_messages.rs, bedrock_converse.rs,
   gemini.rs, openai_responses.rs) and oc-session-runner a real runner pipeline (to_llm_message.rs,
   publish_llm_event.rs, max_steps.rs) — the port is not merely stubbed inside the leaf crates.
3. oc-server has a real axum router (`router.rs::wire_v1`/`wire_v2`) with route-path conversion
   (`route.rs::axum_path`) and SSE modules — substantial surface exists, it is just disconnected.
4. `opencode db path`, `opencode models` (empty-db message path), `--help` (full subcommand surface) and
   version reporting (1.18.13) work.
5. All 20 crates compile (`cargo check` clean); the workspace builds a release binary.
6. 45 integration-test files exist across 18 crates (each crate tests its own API), plus 5 golden tests in
   oc-schema — the canonical schema crate is at least internally golden-tested.
7. Plugin host consciously isolates itself via `serde_json::Value` (JS interop as JSON strings) — a defensible
   seam, though untyped (see ARCH-007).

## Findings summary

| ID | Severity | Confidence | Title | Release blocker |
|----|----------|------------|-------|-----------------|
| ARCH-001 | Critical | CONFIRMED | Declared internal dependency graph is vestigial — zero cross-crate code coupling | YES |
| ARCH-002 | Critical | CONFIRMED | End-to-end session/LLM flow not wired; `opencode run` fails at startup | YES |
| ARCH-003 | Critical | CONFIRMED | `serve` is a stub (binds socket, serves nothing); oc-server router not connected | YES |
| ARCH-004 | High | CONFIRMED | Canonical shared types unused; identical domain types defined 2–8× across crates | YES |
| ARCH-005 | High | CONFIRMED | oc-client + oc-tui + oc-server + oc-session each ship local DTO/session mirrors marked `TODO(integration): promote to oc-schema` | YES |
| ARCH-006 | High | CONFIRMED | Interactive TUI not linked; `attach`/mini TUI return `not_wired`; `opencode` no-args prints a message | YES |
| ARCH-007 | Medium | CONFIRMED | oc-plugin boundary is stringly-typed JSON (`Value`) with no shared type contract | NO |
| ARCH-008 | Medium | CONFIRMED | Client stack quadruplicated: oc-client, oc-acp sdk, oc-tui HttpSdkClient, oc-cli RunClient | NO |
| ARCH-009 | Medium | CONFIRMED | oc-cli is a thin stub facade with 73 `TODO(integration)` markers; many commands `not_wired` | NO |
| ARCH-010 | Medium | CONFIRMED | No feature flags anywhere; optional domains (plugin/mcp/sync/acp) cannot be gated | NO |
| ARCH-011 | Medium | CONFIRMED | Global/hidden state: process-wide `OnceLock<GlobalBus>` singleton in oc-sync; init ordering risk | NO |
| ARCH-012 | Medium | CONFIRMED | V1+V2 session model implemented twice (oc-schema vs oc-session v1.rs/v2.rs); schema crate unused by production code | NO |
| ARCH-013 | Low | CONFIRMED | Error models inconsistent across crates (anyhow/thiserror/serde-Value hybrids); no shared error taxonomy | NO |
| ARCH-014 | Low | CONFIRMED | oc-util ripgrep `Entry/Submatch/Match` marked promote-to-oc-schema while oc-schema/filesystem.rs already defines them | NO |

Severity counts: Critical 3 · High 3 · Medium 6 · Low 2 (14 total). 10 findings flagged as release blockers.

## Detailed findings

### [ARCH-001] Declared internal dependency graph is vestigial — zero cross-crate code coupling
- Severity: Critical · Confidence: CONFIRMED · Release blocker: YES
- Component: entire workspace
- Reference implementation: real cross-package imports (`@opencode-ai/schema`, `@opencode-ai/client`,
  `packages/llm` used by opencode/session, etc.)
- Rust implementation: 20 standalone crates; Cargo.toml declares 61 internal edges (oc-cli:18, oc-server:10,
  oc-session:8, ...) but no production source imports any sibling crate.
- Evidence: `grep -rn "use oc_<crate>" crates/*/src` → zero matches in production source (only
  oc-client/src/lib.rs:12 doc comment). The only non-test `oc_*::` hits are comments
  (oc-cli/src/cli/cmd/serve.rs:38, oc-cli/src/cli/cmd/db.rs:14) and oc-client/src/generated.rs (self re-export).
- Reproduction: `grep -rn "oc_session::" crates/oc-server/src/` → empty while
  oc-server/Cargo.toml:16 lists `oc-session = { path = "../oc-session" }`.
- Expected: downstream crates import canonical types from oc-schema and services from oc-session/oc-llm.
- Actual: each crate re-implements its dependencies locally (oc-session has its own `crate::llm` module,
  oc-server its own `crate::schema`/`crate::event`/in-memory stores, oc-cli its own server/session code).
- Impact: the "20-crate modularity" is cosmetic. Cargo compile order/feature constraints impose no real
  layering. The binary is effectively oc-cli plus external deps (dead-code-stripped siblings).
- Root cause: crates were built in isolation per CONTEXT.md rule 5 ("define a private local mirror ... do not
  block on other agents") and the integration pass never happened; rule 5 became the end state.
- Recommended remediation: run a dedicated integration pass that replaces every local mirror with
  oc-schema/oc-* imports and asserts `cargo tree -i`/`cargo machete` cleanliness in CI.
- Recommended regression test: CI job that fails if any `crates/*/src` file imports a sibling crate's types
  via a local mirror instead of the canonical crate (e.g. greps for `TODO(integration)` count > 0 in src).

### [ARCH-002] End-to-end session/LLM flow not wired; `opencode run` fails at startup
- Severity: Critical · Confidence: CONFIRMED (RUNTIME) · Release blocker: YES
- Component: oc-cli (run) / oc-server / oc-session-runner / oc-llm
- Reference implementation: `opencode run` drives a full session via the embedded server + LLM loop.
- Rust implementation: `RunClient::create` for `LocalClient` returns
  `Err("the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)")`
  (oc-cli/src/cli/cmd/run/client.rs:57-66). Only `--attach` (remote) is available.
- Evidence: RUNTIME `timeout 15 opencode run "say hi"` → the error above. STATIC: run/client.rs:57-66,
  run/mod.rs `LocalClient`/`AttachClient` (client.rs:12-21). No production code calls
  `oc_llm::llm::generate_object` or oc-session-runner's `RunCoordinator`.
- Reproduction: run the binary with `run` and any prompt; observe error.
- Expected: prompt is processed through session → tool runtime → LLM streaming, mirroring the oracle.
- Actual: command aborts immediately; no session is created.
- Impact: the flagship interactive command of the CLI is unusable out of the box.
- Root cause: wiring point (oc-cli) was never connected to the leaf services; oc-server's prompt handler is
  a stub (see ARCH-003).
- Recommended remediation: implement `LocalClient` over oc-server's router + a real session-prompt pipeline
  that spawns oc-session-runner with oc-llm providers.
- Recommended regression test: `opencode run "hi"` against a mock provider returns an assistant message
  (integration test); plus a unit test that `LocalClient::create` no longer errors.

### [ARCH-003] `serve` is a stub — binds a socket, serves nothing
- Severity: Critical · Confidence: CONFIRMED (RUNTIME) · Release blocker: YES
- Component: oc-cli serve / oc-server
- Reference implementation: headless server answers the full HTTP/SSE/WS API and web app (RUNTIME oracle:
  GET /health → 200 HTML; GET /session → `[]`).
- Rust implementation: oc-cli/src/cli/cmd/serve.rs:37-41 binds a bare `TcpListener`; comment
  `TODO(integration): delegate to oc_server::Server::listen(opts)` (serve.rs:38). The completed axum router
  (oc-server/src/router.rs:23 `build`) is never started by the binary.
- Evidence: RUNTIME — `serve --port 19999` prints "listening" but GET /, /health, /session, /config all
  return empty / `000`. Reference oracle on 20001 returns real responses.
- Reproduction: start Rust `serve`, curl endpoints; then start oracle `serve`, curl the same endpoints.
- Expected: HTTP API + SPA responses (per oracle).
- Actual: no HTTP responses whatsoever.
- Impact: any server-dependent workflow (attach, TUI, SDK, web) is impossible.
- Root cause: binary and server crate disconnected; nobody calls `oc_server::Server`.
- Recommended remediation: route the oc-cli `serve` command through oc-server's router/`Server::listen`;
  keep the socket bind inside oc-server.
- Recommended regression test: integration test that boots `serve` and asserts `GET /session` returns 200.

### [ARCH-004] Canonical shared types unused; identical domain types defined 2–8× across crates
- Severity: High · Confidence: CONFIRMED · Release blocker: YES
- Component: oc-schema vs all consumers
- Reference implementation: single schema source (`packages/schema`) imported everywhere.
- Rust implementation: oc-schema ships canonical `Message` (session_message.rs:419), `Part`
  (v1/session.rs), `Entry/Submatch/Match` (filesystem.rs:33/41/49), etc., yet no production code references
  `oc_schema::` at all. `oc_schema::session_message` appears only in oc-schema/tests/golden_messages.rs.
- Evidence: duplicate-type inventory (`01-duplicate-types.txt`): `Message` in 8 crates (oc-util/util/rpc.rs:17,
  oc-acp/src/sdk.rs, oc-tui/src/types.rs, oc-mcp/src/jsonrpc.rs, oc-session-runner/src/llm/message.rs,
  oc-schema/src/session_message.rs:419, oc-llm/src/schema/messages.rs:505, oc-session/src/v2.rs:404);
  `SessionInfo` in 7; `ModelRef` in 7; `Prompt` in 8; `Entry` in 7. The same `Message` enum variants
  (`AgentSwitched`, `ModelSwitched`, `User`, `Synthetic`, `System`, `Shell`, `Assistant`, `Compaction`)
  exist verbatim in oc-schema/src/session_message.rs and oc-session/src/v2.rs.
- Reproduction: compare oc-schema/src/session_message.rs:419-429 with oc-session/src/v2.rs:404-415.
- Expected: one canonical definition consumed via `oc_schema::`.
- Actual: each crate's local copy is authoritative; a field change in one silently desyncs the others.
- Impact: serialization drift risk; no single place to fix a JSON bug; the "exact JSON parity" contract
  (CONTEXT.md rule 1) cannot be enforced centrally.
- Root cause: rule 5 ("private local mirror ... do not block") was executed without a later promotion pass.
- Recommended remediation: promote oc-session v2/v1, oc-client DTOs, oc-tui types, oc-server schema,
  oc-acp sdk/types, oc-util ripgrep to oc-schema; delete local copies; add a lint banning `pub struct/enum`
  whose name collides with an oc-schema type outside oc-schema.
- Recommended regression test: workspace-wide golden test that every serialized domain value round-trips
  through oc-schema types; CI grep that no `TODO(integration): promote to oc-schema` remains.

### [ARCH-005] oc-client/oc-tui/oc-server/oc-session ship local DTO mirrors marked `TODO(integration): promote to oc-schema`
- Severity: High · Confidence: CONFIRMED · Release blocker: YES
- Component: oc-client, oc-tui, oc-server, oc-session, oc-util
- Reference implementation: schema types come from `@opencode-ai/schema`.
- Rust implementation: oc-client/src/types/{agent,command,connection,credential,event,filesystem,health,
  integration,location,model,permission,permission_saved,project,project_copy,prompt,provider,pty,question,
  reference,revert,schema,session,session_input,session_message,skill}.rs — 23 modules, each headed
  `// TODO(integration): promote to oc-schema` (e.g. types/mod.rs:3-5). oc-tui/src/types.rs:8, oc-server/
  src/schema.rs:3, oc-session/src/v2.rs:4, oc-mcp/src/config.rs:6 repeat the marker. 266 `TODO(integration)`
  markers workspace-wide (oc-cli 73, oc-server 33, oc-client 26, oc-tool 23, ...).
- Evidence: grep counts above; oc-schema already contains the target types (session.rs, session_message.rs,
  filesystem.rs, prompt.rs, ...) so the promotion target exists — the work was simply not done.
- Impact: three+ parallel definitions of the wire contract; the port's "1:1 API JSON parity" cannot be
  validated centrally; any consumer bugfix must be replicated per copy.
- Recommended remediation: single promotion PR per domain (session DTOs, client DTOs, filesystem), then
  re-export from oc-schema and update consumers.
- Recommended regression test: after promotion, `cargo machete`/grep guard that no new `promote to oc-schema`
  markers appear.

### [ARCH-006] Interactive TUI not linked; `attach`/mini TUI return `not_wired`
- Severity: High · Confidence: CONFIRMED (RUNTIME + STATIC) · Release blocker: YES
- Component: oc-cli / oc-tui
- Reference implementation: `opencode` opens the TUI (session-ui) and `attach` attaches to a server.
- Rust implementation: oc-tui is **not a dependency of oc-cli** (oc-cli/Cargo.toml has no oc-tui edge);
  the binary cannot enter the TUI. `opencode` no-args prints "opencode: starting TUI (requires a TTY)".
  `attach` returns `Err("attaching the TUI to a running server is not yet wired in this build
  (TODO(integration): oc-tui/oc-client)")` (oc-cli/src/cli/cmd/attach.rs:74).
- Evidence: RUNTIME (no-args output; attach/session list errors). STATIC attach.rs:73-86.
- Impact: no interactive product; the TUI crate (15,446 lines, the largest in the workspace) is unreachable
  from the shipped binary.
- Recommended remediation: link oc-tui into oc-cli as the default command; wire attach over oc-client.
- Recommended regression test: launch the binary in a PTY and assert the ratatui render starts.

### [ARCH-007] oc-plugin boundary is stringly-typed JSON with no shared type contract
- Severity: Medium · Confidence: CONFIRMED · Release blocker: NO
- Component: oc-plugin (and its 5 dependents)
- Reference implementation: plugins exchange typed `Part`/`Message`/`Event`/`Tool` schema objects.
- Rust implementation: oc-plugin has **zero** internal deps (Cargo.toml) and exchanges everything through
  `serde_json::Value` (`host.rs:24-182`: `trigger/event/config/execute_tool` all take `Value`).
- Evidence: oc-plugin/src/host.rs:110-145; lib.rs:19-22 documents the JSON-string interop decision.
- Impact: plugin-authored parts/tool-definitions cannot be type-checked against the canonical schema;
  a malformed payload from a JS plugin surfaces only at the consuming side. Acceptable as an isolation
  seam, but it is a type-safety hole that the reference does not have at this granularity.
- Recommended remediation: keep `Value` at the FFI edge but add typed adapters (oc-plugin → oc-schema
  `Part`/`Message`/`ToolDefinition`) in the dependents' integration layer.
- Recommended regression test: feed a malformed `Part` JSON to `LoadedPlugin::trigger` and assert the typed
  adapter rejects it at the boundary, not deep in the session.

### [ARCH-008] Client stack quadruplicated
- Severity: Medium · Confidence: CONFIRMED · Release blocker: NO
- Component: oc-client / oc-acp / oc-tui / oc-cli
- Reference implementation: one SDK (`@opencode-ai/sdk`) consumed by TUI, ACP bridge, and CLI.
- Rust implementation: (a) oc-client typed RPC client (client.rs 2,021 lines), (b) oc-acp's own SDK client
  (acp/src/sdk.rs — `TODO(integration): implement for the oc-client HTTP client once it exists`, sdk.rs:602),
  (c) oc-tui's own `HttpSdkClient` (tui/src/client.rs:8-10, "replace with a thin adapter over [oc-client]"),
  (d) oc-cli's `RunClient`/`AttachClient`/`LocalClient` (cli/cmd/run/client.rs:12-57).
- Impact: four parallel HTTP/SDK implementations; three of them explicitly slated to collapse onto oc-client.
- Recommended remediation: make oc-client the single transport; have oc-acp/oc-tui/oc-cli depend on it.
- Recommended regression test: shared integration test driving all consumers through one oc-client instance.

### [ARCH-009] oc-cli is a thin stub facade (73 `TODO(integration)` markers)
- Severity: Medium · Confidence: CONFIRMED · Release blocker: NO
- Component: oc-cli
- Evidence: `grep -rc "TODO(integration)" crates/oc-cli/src` = 73; commands verified stubbed at runtime:
  `run`, `serve`, `acp`, `session list`, `db <query>`; `not_wired` helper used widely
  (effect_cmd.rs, attach.rs:74, db.rs:15).
- Impact: oc-cli (6,402 lines, 46 files) is not a meaningful module boundary yet; it is a placeholder facade
  over a binary that cannot perform its advertised functions.
- Recommended remediation: after ARCH-002/003/006 wiring, strip `not_wired` paths; enforce a CI grep that
  `not_wired` never appears in a compiled binary path.
- Recommended regression test: CLI matrix test running every `--help`-listed subcommand to non-error.

### [ARCH-010] No feature flags anywhere
- Severity: Medium · Confidence: CONFIRMED · Release blocker: NO
- Component: whole workspace
- Evidence: `grep -n "\[features\]" crates/*/Cargo.toml` → none; `grep -rln "cfg(feature" crates/` → none.
- Impact: optional/experimental domains (plugin, mcp, sync, acp, tui) cannot be compiled out; the
  memory/CPU-footprint goal (CONTEXT.md priority 2) has no carve-outs, and the unused-but-declared deps
  inflate the compile graph.
- Recommended remediation: introduce `features = ["tui","plugins","mcp","sync","acp"]` defaulting on, and
  gate oc-plugin/oc-sync/oc-acp usage.
- Recommended regression test: build with `--no-default-features --features minimal` compiles.

### [ARCH-011] Global/hidden state: process-wide singleton in oc-sync
- Severity: Medium · Confidence: CONFIRMED · Release blocker: NO
- Component: oc-sync
- Evidence: oc-sync/src/control_plane/global_bus.rs:43 `static GLOBAL: OnceLock<GlobalBus> = OnceLock::new();`
  — a process-wide global bus. Also oc-util/src/ripgrep/binary.rs:65 `static CACHE: OnceCell<PathBuf>`.
- Impact: if wired, a hidden singleton creates cross-instance coupling and init-order traps (multiple
  servers in one process, tests). Violates the "no global state" expectation for a library crate.
- Recommended remediation: construct the bus via the app state and inject it; avoid `static` singletons in
  reusable crates.
- Recommended regression test: spawn two oc-sync stores in one process and assert isolation.

### [ARCH-012] V1+V2 session model implemented twice; schema crate unused by production code
- Severity: Medium · Confidence: CONFIRMED · Release blocker: NO
- Component: oc-schema / oc-session
- Evidence: oc-session/src/v1.rs (V1 Part/Message/`SessionInfo`) and oc-session/src/v2.rs (V2
  Message/Event/Durable) duplicate oc-schema/src/v1/session.rs and oc-schema/src/session_message.rs.
  oc-session/src/store.rs:7-9 imports `crate::v1::SessionInfo` / `crate::v2::Message`, i.e. the crate's own
  copies. oc-schema (6,703 lines, fan-in 16) is referenced by no production crate.
- Impact: the declared "foundation" crate is a parallel universe; its golden tests pass but certify a model
  nothing ships. Schema promotion/integration work remains the single largest architectural debt.
- Recommended remediation: fold oc-session v1/v2 into oc-schema (move, not copy), re-export from oc-session.
- Recommended regression test: oc-session tests run against oc-schema types via re-export.

### [ARCH-013] Inconsistent error models across crates
- Severity: Low · Confidence: CONFIRMED · Release blocker: NO
- Component: all crates
- Evidence: mix of `anyhow::Result` (oc-cli, oc-tui), `thiserror` enums (oc-acp `ACPError`,
  oc-client `ApiError/ClientError/ProtocolError` in types/mod.rs), and ad-hoc `struct Error { message: String }`
  (oc-util/src/ripgrep/mod.rs:57). No shared error taxonomy; mapping between wire errors and internal errors
  is hand-rolled per crate.
- Impact: error propagation and CLI rendering semantics diverge from the reference's typed `Effect` errors.
- Recommended remediation: define a workspace error trait in oc-core/oc-schema and derive per-crate errors
  from it.
- Recommended regression test: golden test mapping each HTTP error status to the reference's error JSON.

### [ARCH-014] oc-util ripgrep types marked for promotion to a target that already exists
- Severity: Low · Confidence: CONFIRMED · Release blocker: NO
- Component: oc-util / oc-schema
- Evidence: oc-util/src/ripgrep/mod.rs:18-58 defines `Entry`, `Submatch`, `Match` with the comment
  `TODO(integration): promote to oc-schema`, while oc-schema/src/filesystem.rs:33/41/49 already defines
  `Entry`, `Submatch`, `Match`. oc-tool/src/ripgrep.rs repeats the trio (3rd copy).
- Impact: demonstrates the promotion backlog is even present where the destination type already landed.
- Recommended remediation: delete the oc-util/oc-tool copies and import from oc-schema.
- Recommended regression test: `cargo machete`/grep guard for the three names outside oc-schema.

## Feature or behavior gaps (architecture-relevant)

- No end-to-end session prompt pipeline reachable from the binary (ARCH-002/003).
- No TUI/attach path in the shipped binary; oc-tui unlinked (ARCH-006).
- `acp` binds a socket and blocks forever; no ACP protocol traffic (runtime scenario 5).
- `db` queries, `session` management, `attach` return `not_wired` (runtime scenarios 4, 6).
- oc-server `session_prompt` never produces an assistant message and never starts the LLM loop
  (instance_handlers.rs:461-503).
- All 61 internal dependency edges are currently dead weight (ARCH-001).

## Test coverage gaps

- No cross-crate integration tests exist (45 test files are all intra-crate). There is no test that boots
  oc-server and drives it through oc-client or the CLI.
- oc-schema's canonical types have golden tests but zero consumers — the goldens certify unused types.
- No test asserts the `opencode run`/`serve`/`attach` user journeys.
- oc-cli has 0 test files and oc-util has 0 test files (runtime-verified dir listing).
- No test enforces the "no local mirror outside oc-schema" rule (would need a lint/CI grep).
- `opencode run "hi"` end-to-end with a stub provider is untested and currently impossible.

## Unverified areas

- Whether `opencode run --attach <url>` against the reference oracle actually works — I did not run it
  (needs auth); the AttachClient path looks implemented (run/client.rs:67+) but UNVERIFIED at runtime.
- Whether `opencode models --refresh` works — requires network; only the empty-db branch was observed.
- `opencode acp` behavior beyond "blocks forever" — with piped empty stdin it produced no output; whether it
  would serve ACP frames via oc-acp is unverified (oc-cli acp.rs shows it never calls oc-acp).
- Whether oc-session/oc-session-runner/oc-llm pipelines actually produce correct provider wire output — the
  crates compile and have unit tests, but nothing wires them, so RUNTIME proof of the LLM loop is absent.
- Per-crate `cargo test` results beyond `cargo check` — I did not run full workspace tests (shared target dir;
  time-boxed); compile-only verification was used.
- Dependency-level dead code: exact number of declared-but-unlinked extern crates in the final binary was not
  measured (would need `cargo bloat`); the unused-import grep is the evidence.

## Final domain verdict

**NOT_READY.** The crate layout mirrors the reference by name and the declared graph is acyclic, but the
architecture does not support end-to-end integration: no production code imports a sibling crate, the
canonical oc-schema types are unused, 266 `TODO(integration)` markers remain, and the shipped binary's
primary paths (`run`, `serve`, TUI, `attach`) are stubs. The port is a set of well-isolated, individually
compiling libraries plus a disconnected CLI shell. Release requires the integration pass (ARCH-001/002/003/004)
as the critical path.
