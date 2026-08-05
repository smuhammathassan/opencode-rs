# Plan 02 — Application Composition Root & Dependency Injection

**Agent:** 02 (integration architect) · **Owned finding:** INTEGRATION-001 (Critical, release blocker)
**Wave:** 0 (READ-ONLY planning) · **Branch:** `fix/audit-remediation`
**Reference spec:** `reference/packages/opencode/src/index.ts`, `core/src/effect/app-node.ts`,
`core/src/effect/layer-node.ts`, `opencode/src/server/server.ts`, `opencode/src/permission/index.ts`,
`core/src/effect/instance-state.ts`, `opencode/src/cli/bootstrap.ts`

---

## 1. Owned findings

### INTEGRATION-001 — Zero production cross-crate integration; no composition root
- `grep 'use oc_<crate>::' crates/*/src` → **0 matches** in production source. All 61 declared
  Cargo edges (oc-cli:18, oc-server:10, oc-session:8 …) are dead. 266 `TODO(integration)` markers;
  17 files return "not yet wired".
- Runtime proof (audit): `opencode run "hi"` → "in-process opencode server is not wired yet";
  `serve` binds a bare TCP socket serving nothing; `acp` blocks in `pending()`; TUI unlinked.
- **Root cause:** no composition root exists. oc-cli `main.rs` → `cmd::dispatch` → per-command
  `run(cli, args)` constructs only a local `cli::context::Context` (oc-cli/src/cli/context.rs, an
  oc-core mirror with a divergent `stable_id` — PARITY-011). No command builds the app.

### Sub-evidence for this plan (static, verified above)
1. `oc-cli/src/cli/cmd/run/client.rs:64-69` — `LocalClient::create(_ctx)` is a hard `Err(...)`.
   The `RunClient` trait (client.rs:14-56) is the correct consumer contract; `AttachClient`
   (client.rs:73+) already implements it over raw reqwest and should collapse onto oc-client.
2. `oc-cli/src/cli/cmd/serve.rs:40-67` — `listen()` binds a bare socket; `server_config()` returns
   `None`; comment `TODO(integration): delegate to oc_server::Server::listen(opts)`.
3. `oc-server/src/server.rs:58` `listen(opts)` builds its **own** `AppState::new` (in-memory stores,
   `Location::default_location()`) at server.rs:82-85. There is **no seam** to supply an externally
   constructed `AppState`/services. `oc-server/src/router.rs:23` `build(state)` takes `AppState`,
   so the router is composable but the listener is not.
4. `oc-server/src/state.rs:52-70` — `AppState` holds `stores: Arc<RwLock<Stores>>` (in-memory
   HashMaps), an internal `EventBus::new(256)` broadcast, auth/cors/location. Handlers never
   persist and never invoke the LLM loop (instance_handlers.rs:461 `session_prompt` appends only).
5. `oc-core/src/context.rs:33-48` — a **working** service-graph precedent: `Services` struct of
   `Arc<...>` handles + `build_with(durable_store, credential_store, directory_store)` (context.rs:62).
   But oc-core sits at the bottom of the graph (deps oc-config/oc-database/oc-schema/oc-util) and
   cannot depend on oc-server/oc-session/oc-llm/oc-tool — **it cannot be the app composition root**.
6. `oc-session-runner/src/session/services.rs:37-316` — the runner already declares its full
   collaborator contract as 15 trait objects in `RunnerDeps` (llm.rs:37-53): `EventBus`, `LlmClient`,
   `Agents`, `ToolRegistry`, `SessionRunnerModel`, `SessionStore`, `LocationService`,
   `SystemContextRegistry`, `SkillGuidance`, `ReferenceGuidance`, `Snapshots`, `SessionInput`,
   `SessionHistory`, `SessionContextEpoch`, `SessionCompaction`. These are the *shapes* the
   composition root must satisfy — but they are currently typed against local mirror types
   (session/mod.rs:1-3 `TODO(integration): promote to oc-session`).
7. `oc-config/src/load.rs:201` `load_instance_state(&LoadOptions) -> InstanceState{config,
   directories, plugin_origins}` is the full reference config pipeline, **never called in prod**.
8. `oc-database/src/database.rs:66` `Database::open` (WAL PRAGMAs + 38 migrations, process-wide
   serialized lock) — never opened by the executable (DB-001).
9. Reference composition shape (layer-node.ts): an `AppNodeBuilder.build(root, replacements)` compiles
   a typed graph of `LayerNode`s tagged `global` | `location`; `server.ts` merges the HTTP router with
   `AppNodeBuilder.build(...)` + a fresh ConfigProvider per listener. Location-scoped services
   (runner, model resolution, tool registry, permissions, filesystem) are per-`InstanceState`
   (ScopedCache keyed by directory); global services (bus, durable store, DB, SessionExecution) are
   process-wide. **The Rust port needs the Rust analogue of this split.**

---

## 2. New-crate decision: **YES — add `crates/oc-app`**

| Option | Verdict |
|---|---|
| In oc-cli (declares all 18 crates already) | Rejected: binary+lib hybrid makes the composition root untestable from other crates, couples `serve`/`run`/TUI wiring to one command module, and oc-tui (not a dep of oc-cli today) would need the graph too. |
| In oc-server (declares 10 crates) | Rejected: oc-server lacks oc-config/oc-database/oc-client/oc-command/oc-tui edges; server would wrongly own CLI/exec lifecycle. |
| In oc-core | Impossible: cycle (oc-server→oc-core). |
| **New `oc-app`** (above all domain crates) | **Adopted.** Mirrors reference `AppNodeBuilder` + `opencode/src` bootstrapping. Library crate, binary-agnostic, testable with N instances in one process, single import surface for oc-cli. |

`oc-app` `Cargo.toml` deps (path): oc-schema, oc-util, oc-config, oc-database, oc-core, oc-provider,
oc-llm, oc-tool, oc-session, oc-session-runner, oc-plugin, oc-mcp, oc-server, oc-client, oc-sync,
oc-acp, oc-project, oc-command, oc-tui (TUI reachability; or gate behind `tui` feature per ARCH-010).

oc-cli then depends on **oc-app + oc-tui + oc-schema + oc-util** and trims its 18 dead edges.

### Proposed module layout (`crates/oc-app/src/`)
```
lib.rs                 // crate docs, re-exports: App, AppRuntime, AppServices, RuntimeContext, AppBuilder
app.rs                 // App: owns AppServices + a RuntimeContext; entry facade for commands/tests
app_runtime.rs         // AppRuntime: a running app (tokio runtime handle, Listener, cancellation, dispose)
services.rs            // AppServices: the full global service graph (plain struct of Arc handles)
runtime_context.rs     // RuntimeContext: per-instance (location-scoped) state + per-session runner map
builder.rs             // AppBuilder: prod + test construction, with replaceable stores/services
config.rs              // AppConfig: resolves oc-config InstanceState -> config + providers + rulesets
factories.rs           // LocalClient/RemoteClient factories + RunClient adapters (client.rs contract)
shutdown.rs            // ShutdownCoordinator: CancellationToken + signal handling + Listener.stop
adapters/              // thin trait-adapter impls wiring domain crates into AppServices
  event_bus.rs         //   runner EventBus <- oc-core bus
  session_store.rs     //   runner SessionStore/SessionInput/SessionHistory/... <- oc-session+oc-database
  llm.rs               //   runner LlmClient <- oc-llm LlmClient stream (Agent 06)
  tools.rs             //   runner ToolRegistry <- oc-tool CoreToolRegistry + permission gate
  providers.rs         //   runner SessionRunnerModel <- oc-provider registry
  server.rs            //   oc-server AppState <- AppServices (Agent 10 seam)
tests/
  boot.rs, session_roundtrip.rs, serve.rs, two_apps.rs, shutdown.rs, events.rs   // §6
```

---

## 3. The service graph — who constructs what, in what order

Single `AppBuilder::build()` executes this exact order (topological; one construction site per
process, matching the reference LayerNode compile). Global = process-wide; Location = per-directory.

| # | Service | Constructor / source | Depends on |
|---|---|---|---|
| 1 | `GlobalPaths`/data dirs | oc-util/oc-config `paths` | — |
| 2 | `AppConfig` | oc-config `load_instance_state(LoadOptions)` → providers/agents/commands/permission rulesets | 1 |
| 3 | `Database` | oc-database `Database::open(OPENCODE_DB\|:memory:\|path())` | 1 |
| 4 | `DurableStore` (SQLite) | Agent 03 impl over `Database` (today: oc-core `InMemoryDurableStore`) | 3 |
| 5 | `EventBus` | oc-core `EventBus::new(durable, durable_registry)` — **never** oc-sync `OnceLock` | 4 |
| 6 | `core.Services` | oc-core `Services::build_with(durable, file_auth_store, dir_store)` | 4,5 |
| 7 | auth + `ProviderRegistry` | oc-provider `FileAuthStore::new(data_dir)` + config providers + `models_dev::snapshot` | 2,1 |
| 8 | shared `reqwest::Client` | one per App (injected for tests) | — |
| 9 | `LlmClient` | oc-llm `LlmClient::with_http_client(http)` | 8 |
| 10 | MCP catalog | oc-mcp catalog from config `mcp` servers | 2,8 |
| 11 | plugin host + registry | oc-plugin `PluginBuilder::new(host, resolver)` from `plugin_origins`; `NoopHost` in tests | 2,1 |
| 12 | `CoreToolRegistry` | oc-tool `CoreToolRegistry::new(ApplicationTools)` + builtin/plugin/MCP tool registration | 10,11 |
| 13 | `PermissionService` | Agent 08 service (evaluate/ask/reply/list over config ruleset + server prompt) | 2, server |
| 14 | session store | oc-session `SessionService`/`SessionStore` impl over `Database` (Agent 01 types) | 3 |
| 15 | `RunnerFactory` | oc-session-runner `SessionRunnerService::new(RunnerDeps)` + `RunCoordinator`; RunnerDeps assembled from 5,9,6,12,7,14 + guidance/snapshots/input/history/context-epoch/compaction | 4-14 |
| 16 | `AppState`/router | oc-server `AppState::new(auth,cors,location)` with real stores wired + `router::build(state)` + `init_projectors` — via new `server::listen_with` seam (Agent 10) | 2,5,13,14 |
| 17 | `RuntimeContext` (Location) | per `bootstrap(directory)` — directory/worktree/project id, `Location`, local tool registrations, per-session `HashMap<SessionID, SessionRunnerHandle>`, disposal | 2,15 |
| 18 | `AppRuntime` | `CancellationToken` (tokio_util) + signal handler + `Listener`; `dispose()` drains runners, stops listener, emits `server.instance.disposed` | 16,17 |

Rules enforced by the builder:
- **Exactly-once** construction of 3,4,5 (hidden-order trap: `Database::open` holds a process-wide
  migration lock; the SQLite `DurableStore` shares the one `Sqlite` handle).
- **Global vs Location split** mirrors reference AGENTS.md: runner, model resolution, tool registry,
  permission, filesystem are Location-scoped; bus, durable store, DB, SessionExecution global.
- No domain crate constructs another; **oc-app is the only cross-crate wiring point**, so the 61
  declared edges become real `use` sites exactly once.

---

## 4. Canonical cross-crate API contracts this plan defines

The composition root defines **service seams (traits) and construction types only — never domain
data types** (Message/Part/SessionInfo/etc. remain Agent 01's `oc-schema`). Every trait is typed
against oc-schema; local-mirror copies in oc-session-runner/services.rs, oc-server/schema.rs,
oc-client/types/, oc-tui/types.rs are deleted in Agent 01's promotion PR.

- `AppServices` — plain struct of `Arc<dyn ...>`/`Arc<...>` handles (the graph of §3), cloneable,
  `Send + Sync`. Mirrors oc-core `Services` precedent (context.rs:33).
- `AppBuilder` — `new()`, setters `with_database(path)`, `with_durable_store(Arc<dyn DurableStore>)`,
  `with_credential_store`, `with_directory_store`, `with_http_client`, `with_plugin_host`,
  `with_permission_service`, `with_location`; `async fn build() -> Result<App, AppError>`.
- `App::builder(services).runtime(tokio_handle) -> AppRuntime`; `App::local_client() -> Box<dyn RunClient>`.
- `RuntimeContext` — location-scoped bag (project, `Location`, local tool/permission state, session
  runner map, instance disposal). Mirrors reference `InstanceState.make`.
- `RunClient` (owned by oc-cli today, client.rs:14-56) becomes the **shared client contract**;
  oc-app implements `LocalClient` over the real router (tower service) and an adapter over
  oc-client `OpenCode` (Agent 11) for `AttachClient`.
- `ShutdownCoordinator` — `CancellationToken` + `Listener::stop(force)` + SIGINT/SIGTERM handling.
- `PermissionService` (Agent 08 owns the impl): `ask(input)`, `reply(input)`, `list()` typed with
  oc-schema permission types; used by tool settle (Agent 09) and server handlers (Agent 10).
- `ServerSeam` (Agent 10): `server::listen_with(opts, state: AppState)` so oc-app supplies services.
- The runner's 15 `RunnerDeps` traits **move ownership to the crate that owns the contract without
  creating cycles**: trait definitions land in oc-app (top of graph) or, where pure, oc-schema/oc-core
  (bottom). They must NOT remain in oc-session-runner (its dependents oc-session/oc-tool/oc-provider
  would form cycles). oc-app provides the adapter impls in `adapters/`.

---

## 5. Dependencies on other agents (from FINDING-STATUS.csv)

| Agent | Finding(s) | What oc-app needs |
|---|---|---|
| 01 | ARCH-001/004/005/012, TEST-002 | oc-schema canonical types; delete mirrors in oc-session-runner, oc-server, oc-client, oc-tui, oc-util |
| 03 | DB-001, DB-002, INFO-002 | SQLite `DurableStore` + session/message/part/event SQL stores in oc-database |
| 04 | CONFIG-001/002/003 | `load_instance_state` error/parse contract parity |
| 05 | SEC-005 | provider registry wiring + mock-provider harness for round-trip tests |
| 06 | LLM-001/002, ASYNC-003 | real streaming `LlmClient` + accounting; runner `LlmClient` adapter |
| 07 | ASYNC-001/002/004/005, TOOLS-001 | RunCoordinator fixes; session store impl + runner wiring |
| 08 | SEC-001 | `PermissionService` trait + impl (allow/ask/deny + server prompt round-trip) |
| 09 | TOOLS-002/003/004, SEC-004, RUST-004/005 | `CoreToolRegistry` materialize + permission gate binding |
| 10 | CLI-002, SSE-002, SEC-002/003 | `server::listen_with` seam; AppState accepts real stores |
| 11 | SSE-001, ARCH-008 | oc-client `OpenCode` + `RunClient` adapter; SSE stream fix |
| 13 | PROTO-001 | MCP catalog wiring into tool registry + server endpoints |
| 15 | PLUGIN-001..004, RUST-001..003 | production `PluginHost` + registry (limiter/interrupt/containment) |
| 12 | CLI-001/005 | oc-cli commands over oc-app (LocalClient + serve + session + db) |
| 16 | CLI-003, UX-004 | oc-tui linked as default command over oc-app local client |
| 18 | TEST-001 | binary E2E harness (mock provider full-session round-trip) |
| 20 | PERF-001 | equivalent-work benchmark of the wired app |

---

## 6. Proposed integration tests (new, in `crates/oc-app/tests/`)

1. `boot.rs` — `AppBuilder` with `:memory:` db + `NoopHost` + mock provider; assert every service
   constructed exactly once and handles are shared (Arc ptr equality where required).
2. `session_roundtrip.rs` — create session → prompt → mock provider stream → assert assistant
   message + tool call in session history. **The INTEGRATION-001 regression test.**
3. `serve.rs` — `AppRuntime::serve(port 0)`; GET `/health` 200, `/session` 200 (ARCH-003 test).
4. `events.rs` — `local_client().subscribe()` receives `session.next.*` events during round-trip.
5. `two_apps.rs` — two in-memory Apps in one process; assert store/bus/runner isolation
   (ARCH-011 multi-runtime regression).
6. `shutdown.rs` — cancel token interrupts an in-flight drain and stops the listener.
7. `cli_e2e.rs` (after Agent 18) — binary `opencode run "hi"` against mock provider.

---

## 7. Risks

1. **Hidden init ordering.** `Database::open` process-wide migration lock; bus requires durable
   store; runner requires everything. Mitigation: one `AppBuilder::build()` with strict §3 order,
   `tracing` spans, and a `debug_assert` that no global is initialized twice. oc-sync `OnceLock`
   (ARCH-011) is bypassed entirely — bus injected.
2. **Cycle risk in trait ownership.** Runner's 15 traits currently live in oc-session-runner; its
   dependents would form cycles if traits stay there after type promotion. Mitigation: traits move
   to oc-app/oc-schema/oc-core (bottom/top), adapters live in oc-app `adapters/`.
3. **Multiple runtimes / singletons.** oc-sync `OnceLock<GlobalBus>`, oc-util ripgrep `OnceCell`,
   per-process tokio runtime in main.rs. Mitigation: inject bus; App owns its runtime handle;
   `App::build` takes an optional `Handle` for tests.
4. **Stalled by type promotion.** Composition cannot compile until Agent 01 lands oc-schema types.
   Mitigation: oc-app skeleton merges immediately after 01; in-memory stores keep it green before
   DB/LLM/plugin wiring lands.
5. **oc-llm buffers `Vec<LLMEvent>`** (ASYNC-003) — the runner's `LlmClient` trait returns Vec, so
   streaming parity is blocked on Agent 06. Flagged; round-trip test tolerates buffered first.

---

## 8. Merge-order recommendation (structural backbone of Wave 1)

1. **Wave 1 · Step 1 — Agent 01**: oc-schema type promotion + mirror deletion (workspace still compiles
   because nothing imports cross-crate yet).
2. **Wave 1 · Step 2 — Agent 02**: merge `oc-app` skeleton — `AppBuilder`/`AppServices`/`RuntimeContext`/
   `AppRuntime` with oc-core + oc-config + oc-database + in-memory durable + `NoopHost` + mock-provider
   seam; `LocalClient` over the real router; `two_apps.rs`/`boot.rs`/`serve.rs` green. **This is the
   backbone**: every later PR merges into the compiled graph, never again a disconnected crate.
3. **Wave 1 · Steps 3+ (parallel, each a PR INTO the backbone, workspace green per merge)**:
   Agent 03 (SQLite durable + session/message/event stores) → Agent 07 (runner wiring + ASYNC fixes)
   → Agent 06 (real llm stream) → Agent 05 (providers) → Agent 09 (tools + permission gate) →
   Agent 08 (permission service) → Agent 10 (`listen_with` + server stores) → Agent 11 (oc-client
   adapter) → Agent 13 (MCP catalog) → Agent 15 (production plugin host) →
   `session_roundtrip.rs` goes green end-to-end.
4. **Wave 1 · Step 4 — Agent 12/16**: oc-cli commands (`run`, `serve`, `session`, `db`) and TUI
   consume oc-app; strip `not_wired` paths.
5. **Wave 1 · Step 5 — Agent 18**: binary E2E harness (`cli_e2e.rs`) locks the INTEGRATION-001 gate.

Gate: at every merge `cargo build --workspace && cargo test -p oc-app` must pass; the app never
shuts down integration again.
