# Agent 10 — Server Composition, State, SSE, PTY Ticket, Filesystem Security — Implementation Plan

**Agent:** 20-AG-10 · **Domain:** Server mounting, state, SSE, PTY, filesystem security
**Repo:** `/root/opencode-rs` · **Branch:** `fix/audit-remediation`
**Status:** Wave 0 READ-ONLY plan (no production source modified)

---

## 1. Owned findings

Consolidated ownership per `rust-port-remediation/FINDING-STATUS.csv` (rows 4, 10, 11, 32)
plus the SERVER-* items assigned to this domain from `rust-port-audit/07-server-transport.md`.

| ID | Severity | File:line (current) | Root cause / gap |
|---|---|---|---|
| CLI-002 | Critical | `crates/oc-cli/src/cli/cmd/serve.rs:40-67` | `serve` binds a bare `TcpListener` and drains+discards bytes; serves zero HTTP. `oc_server` referenced only in comments (serve.rs:38, run/client.rs:59). |
| SERVER-01 | Critical | serve.rs:40-67 | Same placeholder; every connecting client hangs (curl exit 124, zero bytes). |
| SERVER-03 | Critical | `crates/oc-server/src/lib.rs:40-47`, `server.rs:58`, `router.rs:23` | The axum router is never mounted: `oc_server::app` / `server::listen` / `router::build` have zero production callers. Root cause of SERVER-01/02/04. |
| SERVER-04 | High | serve.rs:41-44 | `serve --port 0` binds `hostname:0` → random OS port; reference prefers 4096 then a free port. The correct fallback exists in `oc_server::server::listen` (server.rs:58-70) but is unused. |
| SERVER-05 / SSE-002 | High/Medium | `sse.rs:26-30`, tests `api.rs:222-225`, `sse.rs:113-131` | Frames are emitted `event: message\ndata: ...`; live reference emits bare `data: {...}` (Effect `Sse` encoder drops the `event` field). V1 heartbeat is an SSE comment (`KeepAlive.text("heartbeat")`, sse.rs:41) but reference v1 emits a `data:` event `{...,"type":"server.heartbeat",...}`. V1 lacks location filtering and `server.instance.disposed` termination. |
| Subscriber overflow | Medium | `sse.rs:56-59`, `sse.rs:81-88` | `filter_map Err(_) => None` silently drops lagged SSE subscribers (tokio broadcast `Lagged`); report 07 feature gap #5. Must surface-not-drop (close the stream) so the client reconnects. |
| SEC-002 | High | `handlers/pty.rs:175,207-215`; `middleware.rs:16-26` | PTY connect ticket is never validated: `let valid = allowed_origin && !ticket.is_empty();` (pty.rs:211). Minted token is deterministic `ticket_{event_id().len()}` (pty.rs:175) — not a secret. Middleware correctly skips auth for a non-empty ticket URL (reference parity); the handler must then consume a random single-use ticket. `ConnectToken` wire shape also diverges (`{token,ptyID,expiresAt}` vs reference `{ticket,expires_in}`). |
| SEC-003 | High | `instance_handlers.rs:937`; also `:909`, `handlers/fs.rs:54,84` | `/file/content` does `PathBuf::from(directory).join(path)` with no containment check → absolute `path` or `../` reads arbitrary files. Reference guards with `FSUtil.contains` and dies "Path escapes the location". `/api/fs/list` and `/api/fs/find` also join client `path` unguarded; `/api/fs/read` (fs.rs:26-28) filters `..` — inconsistent. |
| SERVER-08 | Medium | `web.rs:47-52` | `web` binds a listener and drops it; reference calls `Server.listen`. Same placeholder class. |
| SERVER-09 | Low | serve.rs:27; server.rs:99-106 | No signal handling; `pending::<()>()` blocks forever. Graceful shutdown (oneshot) and `global_lifecycle` exist but are dormant. Reference installs `disposeMiddleware` (server.ts:102) + 1s graceful timeout (server.ts:214). |
| SERVER-12 | Info | `run/client.rs` | `AttachClient` works against a real server; unusable because Rust serve is a sink. Unblocks when the server is mounted. |

**Not owned here (coordinate only):** SEC-001 permission gate (Agent 08), ASYNC-004 interrupt→runner (Agent 07), SERVER-06 DB stores (Agent 03), SERVER-07 v1 handler stubs / route handlers over canonical services (Agent 02), SSE-001 attach parser (Agent 11), CLI-001 LocalClient (Agent 12), SERVER-02/CLI-001.

---

## 2. Files to change

| File | Change |
|---|---|
| `crates/oc-cli/src/cli/cmd/serve.rs` | Replace bare-socket `listen` with `oc_server::server::listen`; add signal-driven graceful shutdown; print banner from the real `Listener`. |
| `crates/oc-cli/src/cli/cmd/web.rs` | Same mount for `web` (lower priority; reference web.ts:44). |
| `crates/oc-server/src/server.rs` | Keep `ListenOptions`/4096 fallback. Add force-close + 1s timeout semantics to `Listener::stop`; accept externally-built state (see §3). |
| `crates/oc-server/src/state.rs` | Add `PtyTicketStore`; add `db: Option<Arc<oc_database::Database>>`, `tickets`, `interrupt` seams to `AppState`; keep in-memory `Stores` fallback for tests. |
| `crates/oc-server/src/sse.rs` | Rewrite framing: bare `data:` frames, v1 heartbeat-as-event, v1 location filter + `server.instance.disposed` termination, overflow surfacing. |
| `crates/oc-server/src/handlers/pty.rs` | Random UUID single-use tickets (issue/consume), scope matching, `ConnectToken` response shape. |
| `crates/oc-server/src/schema.rs` | `ConnectToken` → `{ ticket: String, expires_in: i64 }` (reference pty-ticket.ts:6-9). |
| `crates/oc-server/src/instance_handlers.rs` | Containment on `file_content` (and `file_list`); text/binary branch parity; `spawn_blocking`/async fs. |
| `crates/oc-server/src/handlers/fs.rs` | Containment + `..` filtering on `fs_list`/`fs_find`; `spawn_blocking`/async fs. |
| `crates/oc-server/src/handlers/session.rs` | `session_interrupt` → call `AppState.interrupt` registry (Agent 07 seam). |
| `crates/oc-server/src/global_lifecycle.rs` | Emit `server.instance.disposed` (v1 SSE termination trigger) alongside `global.disposed`. |
| `crates/oc-server/tests/api.rs` | Update SSE framing assertions to bare-`data:` golden; add PTY ticket, traversal, overflow, socket-level tests. |
| `crates/oc-server/tests/serve.rs` (new) | Real-socket `server::listen` integration tests (health/session/event/stop). |
| `crates/oc-cli/tests/serve_e2e.rs` (new, under Agent 18 harness) | Binary-level serve/attach/signal tests. |

---

## 3. Mount point & state model

### 3.1 CLI mount (`serve.rs`)

Replace `listen()` (serve.rs:40-67) and the block at serve.rs:26-30:

```rust
use oc_server::server::{listen, ListenOptions};
use oc_server::cors::CorsOptions;
use tokio::signal;

let opts = resolve_network_options(&args.network, server_config(&ctx).as_ref());
let listen_opts = ListenOptions {
    hostname: opts.hostname.clone(),
    port: opts.port,
    cors: CorsOptions { cors: (!opts.cors.is_empty()).then_some(opts.cors.clone()) },
    mdns: opts.mdns,
    mdns_domain: Some(opts.mdns_domain.clone()),
};
let server = listen(listen_opts).await?;
println!("opencode server listening on http://{}:{}", server.hostname, server.port);

tokio::select! {
    _ = signal::ctrl_c() => {}
    _ = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?.recv() => {}
}
// 1s graceful-shutdown timeout, mirroring reference server.ts:214.
let _ = tokio::time::timeout(std::time::Duration::from_secs(1), server.stop(true)).await;
Ok(0)
```

Delivers CLI-002, SERVER-01/03/04 (port-0 → 4096 via `listen`), SERVER-09 (signals +
graceful shutdown). `web.rs` mirrors the same with its existing banner/network-IP printing.

### 3.2 State model

`AppState` grows seam fields while keeping the in-memory fallback so all 52 existing
oc-server tests stay green:

```rust
pub struct AppState {
    pub stores: Arc<RwLock<Stores>>,          // unchanged; Agent 03 swaps impl behind this
    pub events: EventBus,
    pub auth: Arc<AuthConfig>,
    pub cors: Arc<CorsOptions>,
    pub location: Arc<Location>,
    pub tickets: Arc<PtyTicketStore>,         // new (§5)
    pub db: Option<Arc<oc_database::Database>>,   // new seam for Agent 03 (DB-001)
    pub interrupt: Arc<dyn InterruptService>,      // new seam for Agent 07 (ASYNC-004)
}
```

- `AppState::new(...)` keeps defaults (tickets empty, db None, no-op interrupt) for tests.
- New `AppState::with_services(stores, events, db, interrupt)` is the construction path used
  by the CLI composition root once Agent 02/03 land. Until then, `serve` uses `AppState::new`.
- `InterruptService` trait: `fn interrupt(&self, session_id: &str) -> bool`. The no-op impl
  falls back to flipping `record.active` (current behavior); Agent 07's runner-backed impl
  forwards to `RunCoordinator::interrupt`/CancellationToken.

---

## 4. SSE framing spec (byte-for-byte)

Golden fixture source: captured reference bytes in `rust-port-audit/artifacts/07-reference-serve.txt`
(v2 `GET /api/event`).

### 4.1 Universal frame rule

Every event frame is exactly one `data:` line + two LFs — **no `event:` line, no `id:`, no `retry:`**:

```
data: {json}\n\n
```

Drop `.event("message")` from `SseEvent` (sse.rs:26-30) and build via
`SseEvent::default().data(json)`. axum's encoder emits `data: ...\n\n` for a data-only event.
Update the unit test at sse.rs:113-131 and the integration assertion at api.rs:222-225
(both currently pin the wrong `event: message` shape).

### 4.2 v2 `/api/event` (reference/packages/server/src/handlers/event.ts)

- Connected first frame: `data: {"id":"evt_...","type":"server.connected","data":{}}\n\n`
- Live events: `data: {id,type,data,?metadata,?durable,?location}\n\n` (existing `Event` serde
  output is already correct; keep it).
- Heartbeat: literal `: heartbeat\n\n` every 15s. axum `KeepAlive::new().interval(15s).text("heartbeat")`
  already produces exactly this. **Keep.**

### 4.3 v1 `/event` (reference .../httpapi/handlers/event.ts)

- Connected first frame: `data: {"id":"evt_...","type":"server.connected","properties":{}}\n\n`
  (v1 uses `properties`, not `data` — already correct in `v1_event_stream`, sse.rs:84-86).
- Live events filtered by location: keep only events with
  `event.location.directory == instance.directory` and
  (`event.location.workspaceID` absent or == instance workspace). Then map to
  `{id, type, properties: data}` (sse.rs:82-86 already maps; add the filter).
- Heartbeat is a **data event**, not a comment: every 10s emit
  `data: {"id":"evt_...","type":"server.heartbeat","properties":{}}\n\n`.
  Replace the `KeepAlive`-comment approach (sse.rs:41 + `sse_response(stream, 10)`) with a
  stream-merged 10s `IntervalStream` producing that frame. Do not use KeepAlive for v1.
- Termination: stream **ends** after emitting `server.instance.disposed` for this directory
  (`Stream.takeUntil` in reference). Add a `tokio::sync::watch::Receiver<bool>`/oneshot fed by
  `global_lifecycle` when the instance is disposed; emit the disposed frame, then terminate.

### 4.4 Overflow: surface, not drop

In `sse_of` (sse.rs:52-62) and `v1_event_stream` (sse.rs:77-93), replace
`Err(_) => None` with: on `broadcast::error::RecvError::Lagged(_)`, terminate the SSE stream
(yield `Err` so axum closes the connection and the client reconnects). Never silently skip a
frame. This is the honest behavior: a subscriber that cannot keep up is disconnected, matching
reference bounded-stream semantics rather than dropping data.

### 4.5 Headers

`content-type: text/event-stream`; `cache-control: no-cache, no-transform`;
`x-accel-buffering: no`; `x-content-type-options: nosniff`. `Vary: Origin` comes from
`CorsLayer` (present in live capture). Assert presence, not header order.

---

## 5. PTY ticket design (SEC-002)

Mirrors `reference/packages/core/src/pty/ticket.ts` (TTL 60s, capacity 10_000, atomic
single-use) using `uuid` (already an oc-server dep).

### 5.1 Store (state.rs)

```rust
pub struct Ticket { pub pty_id: String, pub directory: String,
                    pub workspace_id: Option<String>, pub expires_at: i64 }
pub struct PtyTicketStore { inner: RwLock<HashMap<String, Ticket>>, capacity: usize }

impl PtyTicketStore {
    pub fn new() -> Self { /* capacity 10_000 */ }
    pub fn issue(&self, scope: Ticket) -> String   // Uuid::new_v4().to_string(); evict expired, then oldest at capacity
    pub fn consume(&self, ticket: &str, scope: &Ticket) -> bool // atomic remove; true iff present, unexpired, all 3 scope fields match
}
```

`issue` stores the scope exactly like the reference `Cache.set(ticket, input)`; `consume`
implements `Cache.invalidateWhen(...)` (check scope matches + atomically remove).

### 5.2 connect-token handler (pty.rs:154-185)

- Build `Ticket` scope from `request_location(...)` (directory + workspace_id) and `pty_id`.
- `let ticket = state.tickets.issue(scope);` and return the fixed shape
  `ConnectToken { ticket, expires_in: 60 }` (schema.rs:196-202 updated to `{ticket, expires_in}`).
- Keep the `x-opencode-ticket: 1` + origin check (reference parity).

### 5.3 connect handler (pty.rs:207-215)

```rust
if let Some(ticket) = query.get("ticket") {
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    let scope = Ticket { pty_id: pty_id.clone(), directory: state.location.directory.clone(),
                         workspace_id: state.location.workspace_id.clone(), expires_at: 0 };
    let valid = is_allowed_request_origin(origin, host, Some(&state.cors))
        && state.tickets.consume(ticket, &scope);
    if !valid { return Ok((StatusCode::FORBIDDEN, "").into_response()); }
}
```

- Ticket absent → fall through: the auth middleware already gated the request (parity with
  reference `if (ticket) { consume }`).
- Empty ticket → middleware does NOT skip auth (has_pty_connect_ticket_url requires non-empty),
  so it is gated normally. **This closes the auth bypass.**
- Deterministic `ticket_{len}` mint (pty.rs:175) is removed.

---

## 6. Containment restoration (SEC-003)

Use the existing faithful port `oc_util::fs_util::contains(parent, child)`
(fs_util.rs:88-95, `strip_prefix` semantics = `FSUtil.contains`). oc-server already depends
on oc-util.

- `instance_handlers.rs:file_content` (937):
  ```rust
  let dir = query.get("directory").cloned().unwrap_or_else(|| state.location.directory.clone());
  let file = PathBuf::from(&dir).join(&query.get("path").cloned().unwrap_or_default());
  if !oc_util::fs_util::contains(&dir, &file) {
      return Err(ApiError::Unknown { message: "Path escapes the location".into(), reference: None }); // mirrors reference Effect.die → HTTP 500 (errors.rs:84,126)
  }
  ```
  Plus parity fixes: `existsSafe` → missing file returns `{type:"text",content:""}`; binary
  branch `{type:"binary",content:<base64>,encoding:"base64",mimeType}` when bytes contain NUL
  or fail strict UTF-8 decode; text content `.trim()`ed (reference file.ts:96-124).
- `instance_handlers.rs:file_list` (909) and `handlers/fs.rs:fs_list` (54) / `fs_find` (84):
  reject `..` segments and assert `contains(&dir, &joined)` before reading.
- `handlers/fs.rs:fs_read` (23-29): keep existing `..` filter and add the same `contains` check.
- **Blocking fs in async handlers:** replace synchronous `std::fs::read`/`read_dir` in the fs
  handlers and `instance_handlers` file handlers with `tokio::task::spawn_blocking` (or
  `tokio::fs::*`), consistent with RUST-004. Path checks stay inline (cheap).

---

## 7. Test list

**In-crate `oc-server` (tower::oneshot + socket):**
1. `health_returns_healthy_true`, `create_session_matches_reference_shape`, `session_list_returns_cursor`,
   `prompt_returns_admitted_input`, `message_roundtrip`, `auth_*` — keep (already green).
2. **SSE framing golden (v2):** update `event_stream_emits_connected_first` (api.rs:222-225) to
   assert first frame is exactly `data: {json}\n\n` with **no** `event:` line; heartbeat frame is
   `: heartbeat\n\n`. Golden from `07-reference-serve.txt` bytes.
3. **SSE framing golden (v1):** connected uses `properties`; a 10s heartbeat is a
   `server.heartbeat` **data** event; events outside the instance directory are filtered out;
   stream terminates after `server.instance.disposed`.
4. **Overflow:** publish > broadcast capacity without draining a slow subscriber → assert the
   SSE body stream terminates (connection closed), not silently continuing.
5. **PTY ticket:** mint→connect OK; replay same ticket → 403; forged/random ticket → 403;
   expired ticket → 403; ticket scoped to another ptyID/directory → 403; empty ticket with
   auth required → 401; no ticket + no password → connect OK.
6. **Traversal:** `/file/content?directory=<tmp>&path=../secret` → 500 "Path escapes the
   location"; `path=/etc/passwd` (absolute) → 500; `/api/fs/list` with `..` → filtered.
7. **Interrupt:** POST `/api/session/:id/interrupt` → 204 and active flag false; with Agent 07
   seam, assert `InterruptService.interrupt` invoked for the session.
8. **Socket-level `server::listen`** (new `tests/serve.rs`): real HTTP GET `/api/health` →
   `{"healthy":true}`; GET/POST `/api/session`; GET `/api/event` first frame; `stop(false)`
   completes; port 0 → 4096 when free, random port when 4096 taken.

**Binary/E2E (oc-cli, under Agent 18's harness / TEST-001):**
9. `opencode serve --port N` → curl `/api/health` 200 `{"healthy":true}` (replaces the
   CLI-002 reproduction: curl exit 000 → 200).
10. `serve --port 0` → banner `http://127.0.0.1:4096` (SERVER-04).
11. `opencode run --attach http://127.0.0.1:PORT "say hi"` against the Rust server → round-trip,
    exit 0 (SERVER-12; requires Agent 11 SSE parser + Agent 12 LocalClient/attach flow).
12. Occupied port → clean EADDRINUSE error.
13. SIGINT/SIGTERM → clean exit within ~1s; `global.disposed`/`server.instance.disposed` emitted.
14. Differential vs reference binary: `/api/health` body and `/api/event` first-frame bytes.

---

## 8. Dependencies on other agents

| Agent | Finding(s) | Dependency for me | What I provide back |
|---|---|---|---|
| **02** | INTEGRATION-001 | Composition root builds `AppState` over canonical oc-core services (event bus backing, project detection) so route handlers run over real services (SERVER-06/07). I consume `AppState` unchanged. | `server::listen` + `AppState` seam definition the composition root fills. |
| **03** | DB-001, INFO-002 | DB-backed stores (session/message tables) behind `AppState.db`; my handlers prefer DB when present, else in-memory. | `AppState.db: Option<Arc<Database>>` seam + handler switch points. |
| **07** | TOOLS-001, ASYNC-001, ASYNC-004 | `InterruptService` impl backed by `RunCoordinator::interrupt`/CancellationToken so `session_interrupt` really cancels (ASYNC-004). Lost-wakeup fix (ASYNC-001) required for the interrupt E2E to be reliable. | `InterruptService` trait + handler call site; runner must be registered per session. |
| **08** | SEC-001 | Permission gate (allow/ask/deny evaluation + prompt round trip) must replace the `effect:"allow"` stub in `handlers/permission.rs`. Server must NOT be network-exposed before this lands. | No-op permission handler left in place; PTY/containment/SSE work is independent. |
| **11** | SSE-001 | Client SSE parser (`run/client.rs`) must read my bare-`data:` frames byte-for-byte (parser already only consumes `data:` lines; coordination on the shared golden fixture). | Framing spec (§4) as the shared contract + golden bytes. |
| **12** (informational) | CLI-001 | LocalClient wiring depends on a live mounted server; unblocks `run` local + attach flows. | Mounted server as the LocalClient transport target. |

---

## 9. Risks

1. **SSE byte-parity oracle:** v1 heartbeat/disposed-termination behavior is inferred from
   source (event.ts), not yet captured from a live reference. Capture v1 `/event` reference
   bytes before finalizing the v1 golden; treat v2 goldens (already captured) as authoritative.
2. **Heartbeat timing:** axum `KeepAlive`/`IntervalStream` first-tick timing differs from
   `Stream.tick`; assert heartbeat *content*, not exact timing, in goldens.
3. **Force-close on shutdown:** axum has no `closeAllConnections`; `stop(true)` may need to
   abort the serve task, dropping in-flight requests. Acceptable for CLI shutdown; documented
   limitation vs reference `forceClose`.
4. **DB store swap (Agent 03)** must not change session/message JSON shapes — serialization
   goldens are the guard.
5. **Client-controlled `directory` (SEC-007 parity):** containment is relative to an
   attacker-chosen directory; the deeper fix is auth-by-default, which is out of scope here.
6. **Exposure gate:** mounting the server makes every oc-server handler reachable. If SEC-001
   (permission) or SEC-002/SEC-003 are not merged first, the running binary regresses from
   "dormant" to "exploitable" (the exact scenario Agent 13 warned about).
7. **`global.disposed` vs `server.instance.disposed`:** the Rust lifecycle currently emits only
   `global.disposed`; v1 SSE termination needs the per-instance `server.instance.disposed`
   emission wired through the runtime lifecycle (Agent 02/07 coordination).

---

## 10. Merge-order recommendation

**Wave 1 — oc-server internal hardening (safe: server not mounted, nothing network-visible):**
SEC-002 PTY ticket + ConnectToken shape, SEC-003 containment + spawn_blocking, SSE-002/SERVER-05
framing, overflow surfacing, v1 event lifecycle. All in-crate green; merge any time.

**Wave 2 — dependency providers (parallel):** Agent 03 DB stores, Agent 08 SEC-001 permission
gate, Agent 07 runner + ASYNC-001/ASYNC-004 interrupt, Agent 02 composition root + schema
promotion.

**Wave 3 — server composition (gated):** CLI-002 serve mount, SERVER-04 port-4096, SERVER-09
signals/graceful shutdown, SERVER-08 web, then LocalClient (Agent 12) and attach flows.

**Hard gate before any Wave 3 merge:** SEC-002 and SEC-003 must be merged, **and** Agent 08's
permission gate (SEC-001) must be merged. The server must never be exposed to the network
before the security fixes — mounting the real router turns dormant stubs into a remotely
reachable command-execution + arbitrary-file-read surface (Agent 13's explicit warning).
Within Wave 3, land `serve` before `web`, and gate `web` on the same security set.
