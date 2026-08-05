# Agent 07 — Server, Transport, HTTP, SSE, and Process Lifecycle

Date: 2026-08-05. Repo: /root/opencode-rs (Rust port of opencode v1.18.13).

## Scope

Audit the server + transport layer of the Rust port:
- `opencode serve` startup, address binding, port config, and whether it is a real HTTP server or a placeholder.
- Wiring of the `oc-server` (axum) router into the CLI.
- `opencode run` local vs `--attach` transport paths.
- Route-table parity with the reference, handler backing stores (in-memory vs oc-database).
- Auth, authorization, CORS, request limits, timeouts, keep-alive, graceful shutdown, signal handling,
  port conflicts, health endpoints, error/status contracts, SSE framing/heartbeats/backpressure,
  JSON encoding, content types, logging, info leakage.

Reference: `reference/packages/server/` (api.ts, routes.ts, handlers/, middleware/, cors.ts, auth.ts,
location.ts) and `reference/packages/opencode/src/server/` (server.ts, auth.ts, serve.ts).
Rust: `crates/oc-server/`, `crates/oc-cli/src/cli/cmd/serve.rs`, `crates/oc-cli/src/cli/cmd/run/`,
`crates/oc-cli/src/cli/network.rs`, `crates/oc-cli/src/cli/cmd/web.rs`.

## Repository areas inspected

- crates/oc-server: src/{server,router,route,state,sse,event,middleware,auth,cors,location,errors,
  instance_handlers,global_lifecycle,projectors,init_projectors,mdns,proxy_util,pty_environment,
  openapi,lib}.rs, src/handlers/{mod,session,event,health}.rs, tests/{route_table,api}.rs.
- crates/oc-cli: src/cli/cmd/serve.rs, src/cli/network.rs, src/cli/cmd/run/{mod,client}.rs,
  src/cli/cmd/web.rs, Cargo.toml.
- reference: packages/server/src/{routes,api,event,cors,location,middleware/authorization}.ts,
  packages/opencode/src/{server/server.ts,server/global-lifecycle.ts,cli/cmd/serve.ts,cli/cmd/web.ts},
  packages/protocol/src/{api.ts,groups/session.ts}, packages/schema/src/{prompt-input,model}.ts.

## Commands executed

- `grep -rn oc_server crates/oc-cli` (2 hits, both comments).
- `grep -n "oc-server" crates/oc-cli/Cargo.toml` (dep declared at line 32).
- Runtime probes (Rust binary `/root/opencode-rs/target/release/opencode`, reference
  `/root/.opencode/bin/opencode`, both v1.18.13, disposable `HOME=/tmp/oc07-home`):
  - `opencode serve --port 43112/43122` then `curl /api/health`, `/api/session` (GET/POST), `/api/event`.
  - `opencode serve --port 0` (default) on both binaries.
  - Second `serve` on an occupied port.
  - `opencode run --attach http://127.0.0.1:PORT "say hi"` against Rust serve and reference serve.
  - Plain `opencode run "say hi"` (LocalClient path).
- Read shared `cargo test --workspace` log (`/tmp/opencode/ws-test.log`) for oc-server test results
  (shared target dir; did not run cargo build/test myself to avoid clobbering concurrent agents).

## Runtime scenarios attempted

All raw outputs are in `rust-port-audit/artifacts/07-*.txt`.

| Scenario | Rust `serve` | Reference `serve` |
|---|---|---|
| Startup banner | "opencode server listening on http://127.0.0.1:PORT" (matches) | same, + password warning (same) |
| GET /api/health | timeout (curl exit 124), zero bytes | 200 `{"healthy":true}`, `Content-Type: application/json`, `Vary: Origin` |
| GET /api/session | timeout (124) | 200 `{"data":[...],"cursor":{...}}` |
| POST /api/session | timeout (124) | 200 `{"data":{"id":"ses_...",...}}` |
| GET /api/event (SSE) | timeout (124) | 200 `text/event-stream`, headers `Cache-Control: no-cache, no-transform`, `X-Content-Type-Options: nosniff`, `x-accel-buffering: no`; body `data: {...server.connected...}\n\n` then `: heartbeat\n\n` |
| Raw HTTP via nc | connects, never replies | (n/a) |
| `--port 0` | OS random port (observed 39973) | prefers 4096 (observed 4096) |
| Occupied port | `Error: Unexpected error / Address already in use (os error 98)` | (not run; reference propagates EADDRINUSE too) |
| `run --attach <rust-serve>` | hangs (timeout 124), no output | (n/a) |
| `run --attach <reference-serve>` | n/a (this is Rust CLI) | Rust CLI connects, prompts, prints response "Hi", exit 0 |
| plain `run` (no attach) | exit 1: "the in-process opencode server is not wired yet in this build (TODO(integration): oc-server). Try `opencode run --attach <url>`..." | n/a |

## Architecture or behavior summary

`oc-server` is a real, self-contained axum server: full route table (v1 instance + v2 `/api` +
global + `/doc`/`/openapi.json`), auth, CORS, SSE, error contracts, and handlers over an in-memory
projection store. It compiles and its own tests pass (34 unit + 15 api integration + 3 route-table,
per `/tmp/opencode/ws-test.log`). **However it is never mounted.** `opencode serve` (crates/oc-cli/
src/cli/cmd/serve.rs:40-67) binds a bare `TcpListener` and spawns a task that reads bytes and
discards them; it never calls `oc_server::server::listen`, `oc_server::app`, or `axum`. `opencode run`
without `--attach` uses `LocalClient`, whose `create()` unconditionally fails
(crates/oc-cli/src/cli/cmd/run/client.rs:64-70). `--attach` uses a functioning `AttachClient`
(verified working against the reference server). The shipped binary therefore has no HTTP server at
all; every claim that "serve works" is false at runtime.

## Positive observations

- `oc-server` crate is well-engineered and green: 52 tests pass (34 unit, 15 `tests/api.rs`, 3
  `tests/route_table.rs`).
- Route table is complete and matches the reference groups (spot-checked session/health/event/message
  against `reference/packages/protocol/src/groups/*`; the `route_table_matches_reference` golden
  test passes). Axum path conversion (`:param` -> `{param}`) is tested.
- Auth (crates/oc-server/src/auth.rs:47-54, middleware.rs:31-74) mirrors the reference
  authorization.ts: constant-time compare, `auth_token` query support, `WWW-Authenticate: Basic
  realm="Secure Area"`, PTY-connect-ticket bypass. `tests/api.rs` covers 401/authorized/query-token.
- CORS (cors.rs:19-41) matches reference cors.ts exactly (loopback, oc://renderer, tauri, opencode.ai
  regex, configured origins).
- Error contracts (errors.rs) implement the reference `{ "_tag": ... }` tagged errors with correct
  status codes; v1 `{name,data}` shape is also present. Tested (404 tags, 401 tag).
- SSE headers (sse.rs:18-22) match the reference's captured headers exactly; heartbeats (15s v2, 10s
  v1) mirror the reference handler.
- `oc_server::server::listen` implements the 4096-port preference for port 0 (server.rs:58-70) and
  graceful shutdown via oneshot (server.rs:99-106) — correctly mirroring reference server.ts, but
  dormant.
- Startup banner/warning text matches the reference.
- The `AttachClient` transport works against a real server (session create/get, prompt, SSE
  subscribe) — client-side HTTP/SSE parsing is functional.

## Findings summary

| ID | Severity | Confidence | Summary |
|---|---|---|---|
| SERVER-01 | Critical | CONFIRMED (static+runtime) | `opencode serve` is a bare-TCP placeholder; serves zero HTTP |
| SERVER-02 | Critical | CONFIRMED (static+runtime) | `opencode run` (local) always fails — LocalClient unwired |
| SERVER-03 | Critical | CONFIRMED (static) | oc-server's axum router is never mounted by the CLI |
| SERVER-04 | High | CONFIRMED (static+runtime) | `serve --port 0` picks a random port, reference prefers 4096 |
| SERVER-05 | High | CONFIRMED (static vs runtime ref) | SSE frame shape diverges: Rust emits `event: message`, reference emits bare `data:` |
| SERVER-06 | Medium | CONFIRMED (static) | Handlers backed by in-memory Stores, not oc-database |
| SERVER-07 | Medium | CONFIRMED (static) | v1 instance surface largely stub shapes (12 TODO(integration)) |
| SERVER-08 | Medium | CONFIRMED (static) | `web` command has the same bare-socket placeholder |
| SERVER-09 | Low | CONFIRMED (static) | No signal handling; graceful shutdown/mdns lifecycle dormant |
| SERVER-10 | Low | CONFIRMED (static) | mDNS is a best-effort beacon, not a real responder |
| SERVER-11 | Informational | CONFIRMED (static) | Auth/CORS/location/error layers are wire-compatible when mounted |
| SERVER-12 | Informational | CONFIRMED (runtime) | Attach client works against a real server; unusable with Rust serve |

## Detailed findings

### SERVER-01 — Critical — `serve` binds a bare TCP sink, not an HTTP server (CONFIRMED)

STATIC: `crates/oc-cli/src/cli/cmd/serve.rs:40-67` — `listen()` does `TcpListener::bind(addr)`,
spawns a task that loops `socket.read()` and discards bytes (serve.rs:53-59); nothing writes back.
`run()` never references `oc_server` (serve.rs:13-30); the only reference is the TODO comment at
serve.rs:38-39 ("TODO(integration): delegate to `oc_server::Server::listen` ... instead of binding a
bare socket here").

RUNTIME: `opencode serve --port 43122` then `curl /api/health`, `/api/session` (GET+POST),
`/api/event` all time out with zero bytes received (artifacts/07-rust-serve-endpoints.txt, exit 124
for all four). A raw `GET /` via nc connects but never receives a response. The placeholder is worse
than a stub: every client that connects hangs indefinitely with no timeout/close.

### SERVER-02 — Critical — plain `opencode run` is broken (CONFIRMED)

STATIC: `crates/oc-cli/src/cli/cmd/run/client.rs:64-70` — `LocalClient::create` always returns
`Err("the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)")`.
`run/mod.rs:552-565` selects `LocalClient::create` whenever `--attach` is absent.

RUNTIME: `opencode run "say hi"` exits 1 with that error
(artifacts/07-attach-tests.txt, section B). The default local workflow — the primary UX of
`opencode run` — cannot run at all in this build.

### SERVER-03 — Critical — oc-server's router is never mounted (CONFIRMED)

STATIC: `grep oc_server crates/oc-cli` returns exactly two matches, both in comments
(run/client.rs:59, serve.rs:38). The dependency is declared (oc-cli/Cargo.toml:32) but never invoked;
`oc_server::server::listen` (crates/oc-server/src/server.rs:58), `oc_server::app` (lib.rs:40-47), and
`oc_server::router::build` (router.rs:23) have no callers outside oc-server's own tests. This is the
root cause of SERVER-01 and SERVER-02.

### SERVER-04 — High — default port resolution diverges (CONFIRMED)

STATIC: reference `startWithPortFallback` prefers 4096 then any free port
(reference/packages/opencode/src/server/server.ts:117-122); the Rust CLI `serve` binds `hostname:0`
directly (serve.rs:41-44). `oc_server::server::listen` does implement the 4096 fallback correctly
(server.rs:58-70) but is unused.

RUNTIME: reference `serve --port 0` -> `http://127.0.0.1:4096`; Rust `serve --port 0` ->
`http://127.0.0.1:39973` (random OS port). Clients (TUI/sdk) that expect the server on 4096 will not
find the Rust default server. Root cause is SERVER-03; fix lands when serve is wired to
`oc_server::server::listen`.

### SERVER-05 — High — SSE frame shape diverges from the reference (CONFIRMED vs live reference)

STATIC: Rust `sse.rs:26-30` builds every frame with `SseEvent::default().event("message")`, i.e. it
emits `event: message\ndata: {...}\n\n`; the unit test at sse.rs:113-131 asserts this shape.

RUNTIME: the reference binary emits frames with **no `event:` line** — `data: {...}\n\n` then a
`: heartbeat\n\n` comment (artifacts/07-reference-serve.txt, SSE section), despite
reference/packages/server/src/handlers/event.ts:11-17 declaring `event: "message"`. Effect's Sse
encoder drops the field on the wire.

Impact: if oc-server is wired as-is, SSE clients that parse strict framing (and any golden test
derived from live reference output) will observe different bytes. Severity High because the port
goal is 1:1 wire parity and the only live check available caught a real difference. UNVERIFIED
against a live Rust server (cannot be started).

### SERVER-06 — Medium — handlers use in-memory stores, not oc-database (CONFIRMED)

STATIC: `crates/oc-server/src/state.rs:26-32` — `Stores` is `HashMap`s in an `Arc<RwLock>`;
session handlers read/write it (handlers/session.rs:110-115, 266-268). The reference routes.ts
provides `Database.node` (SQLite via Drizzle, reference/packages/server/src/routes.ts:27). The Rust
port never touches oc-database; a server restart loses all sessions. RUNTIME (reference side): the
reference persisted sessions created in an earlier probe across a server restart
(artifacts/07-reference-serve.txt, GET /api/session shows sessions from 08:55 and 09:02), demonstrating
durable storage the Rust port lacks.

### SERVER-07 — Medium — v1 instance surface is largely stub shapes (CONFIRMED)

STATIC: `crates/oc-server/src/instance_handlers.rs:5-6` documents "Many routes depend on oc-core
services that are not integrated yet and return stable empty/default shapes"; 12 `TODO(integration)`
markers. Examples: `config_providers` -> `{"providers":[],"default":{}}` (line 46), `global_upgrade`
-> `"upgrade is not implemented"` (line 93), `experimental_session_background` ->
`{"sessionID":""}` (line 1022), `experimental_console` -> `{...count:0}` (line 1121). The v2 `/api`
session/message surface is genuinely implemented; the legacy surface is mostly scaffolding.

### SERVER-08 — Medium — `web` command is the same placeholder (CONFIRMED)

STATIC: `crates/oc-cli/src/cli/cmd/web.rs:47-52` binds a `TcpListener` and drops it immediately
("TODO(integration): serve the web interface via oc-server once wired"), while the reference
`web.ts` calls `Server.listen` (reference/packages/opencode/src/cli/cmd/web.ts:44).

### SERVER-09 — Low — no signal handling; lifecycle code dormant (CONFIRMED)

STATIC: CLI `serve` blocks on `std::future::pending::<()>()` (serve.rs:27) with no signal handling —
matching the reference's `Effect.never` (serve.ts:22), so this is parity on the surface. But
`oc_server`'s graceful shutdown (server.rs:99-106), `dispose_all_instances_and_emit_global_disposed`
(global_lifecycle.rs:22-28), and projectors are never exercised. The reference additionally installs
`disposeMiddleware` (server.ts:102) and a 1s graceful-shutdown timeout (server.ts:214) that have no
Rust counterpart. Background-task shutdown on SIGINT is therefore untestable in the shipped binary.

### SERVER-10 — Low — mDNS is a partial port (CONFIRMED)

STATIC: `crates/oc-server/src/mdns.rs:4-5` — "Partial port: publishes a `_http._tcp` service via a
best-effort multicast beacon. TODO(integration): real mDNS responder crate". Dormant (only reachable
through the unwired `listen`).

### SERVER-11 — Informational — auth/CORS/error layers are wire-compatible (CONFIRMED)

Auth, CORS, location, schema-error truncation (middleware.rs:78-89) and error bodies (errors.rs) are
faithful ports with passing tests. If SERVER-03 is fixed, these layers should be drop-in compatible
with the reference wire contract.

### SERVER-12 — Informational — AttachClient works against a real server (CONFIRMED)

RUNTIME: Rust CLI `run --attach http://127.0.0.1:43125 "say hi"` against the reference server
connected, subscribed to `/event`, prompted, and printed "Hi" (exit 0; artifacts/07-attach-tests.txt,
section C). The same command against the Rust `serve` hangs (section A). Client-side transport is
functional; the server side is the blocker.

## Feature or behavior gaps

1. No HTTP server in the shipped binary (serve/web/run-local all placeholder or failing).
2. No 4096 default-port preference for `serve --port 0` in the CLI (present only in oc-server).
3. SSE frames include an extra `event: message` line vs the reference's bare `data:` framing.
4. No durable persistence — in-memory stores instead of oc-database.
5. No request body-size limits, explicit read/write timeouts, or keep-alive tuning in oc-server
   (the reference relies on defaults too, so this is low; but slow-client/backpressure behavior is
   untested: tokio `broadcast` silently drops lagging SSE subscribers via `filter_map Err -> None`,
   sse.rs:56-59, which diverges from the reference's bounded-stream semantics — UNVERIFIED live).
6. UI catch-all fallback always returns 404 (router.rs:54-60); embedded UI is a TODO.
7. `--mini` interactive mode unwired (run/mod.rs:292-297).

## Test coverage gaps

- No integration test boots the CLI binary and exercises `serve`/`run` end to end — the exact gap
  this audit found (all oc-server tests use `tower::oneshot`, tests/api.rs:41-43; the only socket
  test, server.rs:134-147, checks only that TCP connect succeeds, not that HTTP responds).
- The route-table golden test (tests/route_table.rs) is hand-transcribed from the reference and never
  diffs against the live reference source/binary at test time, so a transcription drift would pass.
- No golden SSE-framing test against captured reference bytes (the SERVER-05 divergence would not be
  caught; tests/api.rs:210-229 asserts the Rust's own `event: message` shape).
- No tests for: port-conflict fallback at the CLI layer, 4096 preference, slow/disconnected SSE
  clients, broadcast-lag drop behavior, graceful-shutdown timing, or signal handling.

## Unverified areas

- Live behavior of the axum server (SSE heartbeats, backpressure, WebSocket PTY connect, graceful
  shutdown, mDNS) cannot be exercised because the server is not wired (BLOCKED by SERVER-03).
- Backpressure/drop semantics of slow SSE subscribers — UNVERIFIED live.
- `--attach` permission/question flows — only the happy path was verified.
- Exact SSE framing of a live Rust server — UNVERIFIED live; the divergence is inferred from source
  (sse.rs) plus captured reference bytes.

## Final domain verdict

**NOT_READY**

The server/transport layer does not function end to end in the shipped binary. `oc-server` is a
well-built, well-tested axum crate, but `opencode serve` runs a bare TCP byte-sink that serves no
HTTP, `opencode run` (local) always errors, and `web` is the same placeholder. The agent report's
claim ("serve binds a bare TCP socket as a stand-in") is CONFIRMED exactly. The critical path to
READY is wiring `oc_server::server::listen` into the CLI `serve`/`run`/`web` paths, then re-running
the endpoint matrix in this report against the real server.
