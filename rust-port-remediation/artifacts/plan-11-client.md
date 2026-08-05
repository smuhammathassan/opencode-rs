# Agent 11 — Typed Client & Local/Attached Transport — Implementation Plan

**Agent:** 20-AG-11 · **Domain:** oc-client canonical client, SSE decoding, local + attach transport
**Repo:** `/root/opencode-rs` · **Branch:** `fix/audit-remediation`
**Status:** Wave 0 READ-ONLY plan (no production source modified)

Owned consolidated finding: **SSE-001** (High, CONFIRMED, release blocker, owner 20-AG-11 per
`FINDING-STATUS.csv`). Secondary mandate: make **oc-client** the single canonical HTTP/SSE client and
collapse the CLI `RunClient`, the TUI `HttpSdkClient`, and the ACP `OpencodeClient` onto narrow adapters
over it, plus an in-process/local transport to the real server runtime.

---

## 1. Owned findings

| ID | Severity | File:line (current) | Root cause / gap |
|---|---|---|---|
| SSE-001 | High (release blocker) | `crates/oc-cli/src/cli/cmd/run/client.rs:412-455` | `sse_stream` uses `stream::unfold` with **closure-local** `buffer`/`data`/`in_event`. The closure returns after the first complete event, so bytes for subsequent events in the same network chunk are discarded on the next call. Runtime proof (audit 06): a mock emitting two text events in one chunk → no output, exit 0; per-event delay → both print. |
| Client duplication | High | `run/client.rs:73-410`, `oc-tui/src/client.rs:134-629`, `oc-acp/src/sdk.rs` | Three hand-rolled HTTP/SSE clients (AttachClient, HttpSdkClient, ACP trait with no impl) while `oc-client` — a complete, tested typed client for all v2 `/api/*` groups — has **zero production callers** (`grep 'use oc_client' crates/*/src` → none outside its own crate). |
| Missing endpoint surface | High | `oc-client/src/client.rs` | oc-client covers v2 `/api/*` only. The consumers actually need v1-instance + v1-global endpoints: `/event` (v1 `{id,type,properties}`), `/global/event`, `/config`, `/config/providers`, `/path`, `/agent`, `/skill`, `/session/{id}/message` (v1 prompt), `/session/{id}/command|share|summarize|fork|abort|delete`, `/permission/{id}/reply`, `/question/{id}/reply`, v1 `provider`/`find`/experimental groups. |
| Prompt endpoint ambiguity | High | oc-client `sessions.prompt` → `POST /api/session/{id}/prompt`; reference run.ts uses `POST /session/{id}/message` | run.ts (`reference/packages/opencode/src/cli/cmd/run.ts:859-865`) calls the **v1** message endpoint with `{agent,model,variant,parts}`; oc-client's `prompt` targets the v2 durable-admission endpoint. Adapter must use v1 for run parity. |
| No in-process transport | High | `oc-client/src/transport.rs:17-31,63-68` | `Transport` is reqwest-hardwired; `ClientOptions.http` accepts only `reqwest::Client`. The reference injects `fetch` (`run.ts:943-955`, `Server.Default().app.fetch(...)`); Rust has no equivalent, so `LocalClient` cannot work. |
| No reconnect | Medium | oc-client `sse.rs`; TUI `client.rs:208-234` | Reference v2 SDK SSE auto-reconnects with exponential backoff (`reference/packages/sdk/js/src/v2/gen/core/serverSentEvents.gen.ts:95-233`, default 3000ms, max 30000ms, `Last-Event-ID`). oc-client has none; the TUI rolls its own backoff; run.ts has none either. |
| Partial-retry parity | Medium | `oc-client/src/transport.rs:115-139` | `RetryPolicy` exists (client extension) but no reference retry/error mapping on the run path; adapters must map `Error::Api`/`ClientError` to the reference run.ts error surfacing (`formatRunError`). |

**Not owned here (coordinate only):** CLI-001 `LocalClient` CLI seam (Agent 12), TUI launch CLI-003
(Agent 16), ACP service wiring PROTO-001 (Agent 13), SSE-002/SERVER-05 server framing (Agent 10),
server mount SERVER-03/CLI-002 (Agent 10), `RunClient` trait ownership / oc-app backbone (Agent 02),
oc-schema type promotion (Agent 01).

---

## 2. Files to change

### crates/oc-client (canonical home — this agent's primary crate)

| File | Change |
|---|---|
| `src/transport.rs` | Introduce `HttpExecutor` trait (`async fn execute(&self, req: http::Request<bytes::Bytes>) -> Result<ExecutorResponse, ExecutorError>`; `ExecutorResponse { status, headers, body: BoxStream<'static, Result<Bytes, ExecutorError>> }`). Rename current impl to `ReqwestExecutor`. `Transport` holds `Arc<dyn HttpExecutor>`; `ClientOptions` gains `executor: Option<Arc<dyn HttpExecutor>>` (default `ReqwestExecutor`). `with_retry`/`RetryPolicy` unchanged. |
| `src/sse.rs` | Keep `SseDecoder` (buffer persistence is **already correct**; see §4). Generalize body source from `reqwest::Response` to the executor body stream. Add `ReconnectingSseStream` wrapper mirroring `serverSentEvents.gen.ts` (backoff 3000→30000ms, `max_attempts`, optional `Last-Event-ID`). Add v1-tolerant field handling (`event:`/`id:`/`retry:` ignored for data-only decode; `id:` feeds `Last-Event-ID`). |
| `src/local.rs` (new) | `RouterExecutor` — but see §3: recommended to live in `oc-app` (Agent 02) or `oc-server` (Agent 10), not oc-client, to keep oc-client free of axum/tower. oc-client only defines the trait. |
| `src/client.rs` | Add groups/methods: `config` (GET `/config`, GET `/config/providers`), `path` (GET `/path`), `app` (GET `/agent`, GET `/skill`), `event` (v1 GET `/event`), `global` (GET `/global/event`), v1 session instance methods (`message`, `command`, `share`, `unshare`, `summarize`, `abort`, `delete`, `revert`, `unrevert`, `todo`, `diff`, `shell`, `status`), v1 `permission.reply` (POST `/permission/{id}/reply`), v1 `question.reply`/`reject`, v1 `provider.list`, v1 `find`, `experimental` (`capabilities`, `console`). Each mirrors the SDK path/body/declared-status exactly. |
| `src/types/event.rs` (+`types/`) | Add v1 `GlobalEvent { id, type, properties }` and v1 session/`config`/`path`/`app` result types (serde camelCase), mirroring `reference/packages/sdk/js/src/v2/gen/types.gen.ts`. |
| `src/generated.rs` | Re-export new types. |
| `src/error.rs` | No change needed; `Error::Api`/`ClientError` already map declared-status bodies (`decode_api_error`). |
| `Cargo.toml` | No new deps if `RouterExecutor` lives outside (trait is HTTP-agnostic; `http`/`bytes`/`futures` already transitively available via reqwest). |

### crates/oc-cli (adapter + SSE-001 fix)

| File | Change |
|---|---|
| `src/cli/cmd/run/client.rs` | **Delete `sse_stream` (SSE-001 fix)** and the hand-rolled AttachClient HTTP/URL/parse code. `AttachClient` becomes a thin `RunClient` impl over `oc_client::OpenCode` (network executor). `LocalClient::create` returns the adapter over `OpenCode::make_with_executor(RouterExecutor, http://opencode.local)` once the composition root supplies the router; keep the not-wired error until Agent 12/02 land. Keep the `RunClient` trait shape (trait home moves to oc-app per Agent 02). |
| `src/cli/cmd/run/events.rs` | Consume oc-client `Result<Event, Error>` items; map `Error::Api(ProtocolError::UnauthorizedError…)`/`ClientError` to the reference `formatRunError` surfacing. Loop break on `session.status` idle unchanged. |
| `src/cli/cmd/run/types.rs` | `GlobalEvent` maps to/aliases oc-client v1 `GlobalEvent` (or stays local until Agent 01 promotion). |

### crates/oc-tui

| File | Change |
|---|---|
| `src/client.rs` | Keep `SdkClient` trait + types; reimplement `HttpSdkClient` body as an adapter over `oc_client::OpenCode` (drop `reqwest` + `SseParser`). `subscribe_events` keeps the reference `startSSE` backoff loop (`sdk.tsx:82-116`, 1000ms→30000ms) but drives the oc-client `global` event stream. Map v1 instance methods (`todo`, `diff`, `shell`, `abort`, `unshare`, `revert`, `unrevert`, `summarize`, `status`, `config_providers`, `provider_list`, `experimental_*`, `fs_find`) onto the new oc-client methods. |

### crates/oc-acp

| File | Change |
|---|---|
| `src/sdk.rs` | Implement the `OpencodeClient` trait over `oc_client::OpenCode` (global_event → `global`; session_* → v1/v2 sessions; config_* → `config`; app_agents/app_skills → `app`; command_list → `command`; permission_reply → v1 `permission`; mcp_add → TODO until Agent 13). Keep ACP data shapes as local mirrors (Agent 01 promotion). |
| `Cargo.toml` | Add `oc-client = { path = "../oc-client" }`. |

---

## 3. Local transport design

**Recommendation: in-process router call, not TCP loopback** — matches the reference exactly
(`run.ts:943-955` `Server.Default().app.fetch(new Request(...))`), requires no port allocation, and keeps
local sessions **not network-visible** (critical for the plan-10 security gate: no socket before
SEC-001/002/003 merge).

```
oc-client              oc-app (Agent 02) / oc-server (Agent 10)
  HttpExecutor            RouterExecutor
  ├─ ReqwestExecutor ──►  reqwest → real server socket      (attach mode)
  └─ (trait impl)      ◄─ axum::Router (from server/router::build(AppState))
       OpenCode::make_with_executor(executor, base_url)
```

- **Trait (oc-client):** `HttpExecutor::execute(http::Request<Bytes>) -> ExecutorResponse` with a
  `BoxStream` body. `SseDecoder` and `execute_inner` operate on `ExecutorResponse`, so network and
  local paths share **one** code path (single source of truth; the SSE-001 fix and all goldens apply
  to both).
- **RouterExecutor (oc-app or oc-server):** `router.clone().oneshot(request)` (tower), body via
  `axum::body::Body::into_data_stream()`. This is the proven pattern from `oc-server/tests/api.rs`.
  Base URL is cosmetic (`http://opencode.local`).
- **Why not loopback:** reference parity (no socket), no EADDRINUSE/port-4096 handling, and — decisive —
  a loopback server would expose the in-process runtime on the network, violating the Agent 10/13
  security gate. Loopback remains available for `--attach`-style debugging via the existing
  `ReqwestExecutor`.
- **Retry/reconnect:** `RetryPolicy` retry stays in `Transport` (resends the request); the reconnect
  loop is a stream-level wrapper (`ReconnectingSseStream`) so both executors inherit it.

---

## 4. SSE decoder fix

**The canonical decoder is already correct.** `oc-client/src/sse.rs:17-87` (`SseDecoder`) persists
`buffer` as a struct field, drains **all** `\n\n` blocks per chunk (loop in `next_value`),
normalizes CRLF/`\r`, carries a trailing `\r` across chunks, caps at 1 MiB, and flushes the final
block on EOF. Verified by `oc-client/tests/sse.rs::events_subscribe_splits_events_across_chunks`.

The bug is isolated to `oc-cli/.../run/client.rs:412-455`: closure-local state reset per `unfold`
invocation. **Fix = delete that function and route `RunClient::subscribe` through oc-client's
decoder** (§5). No buffer-persistence change is needed in oc-client.

Gaps to close against the reference for full parity:
1. `event:`/`id:`/`retry:` SSE fields are ignored by data-only decode (v1 `/event` uses `data:` only,
   but tolerating extra fields matches the v2 SDK parser `serverSentEvents.gen.ts:160-178`).
2. `id:` → `Last-Event-ID` header on reconnect (reconnect wrapper).
3. Strict-vs-lenient JSON: oc-client errors `MalformedResponse` on non-JSON data (mirrors
   `client.ts:229-234`); the v2 SDK yields the raw string. **Keep strict** (documented) — all opencode
   event payloads are JSON; strictness caught the SSE-001 test case.
4. Golden against captured reference bytes (`rust-port-audit/artifacts/07-reference-serve.txt`: bare
   `data:` frames + `: heartbeat`) — shared with Agent 10 (SSE-002 framing contract).

---

## 5. Adapter plan

Endpoint mapping (v1 vs v2) — **the single biggest correctness trap**:

| RunClient method | Endpoint (reference) | oc-client group |
|---|---|---|
| session_get / list / fork / create | v2 `/api/session…` | `sessions` (exists) |
| session_prompt | **v1** `POST /session/{id}/message` | **new** `sessions.message` (v1) — do NOT use `sessions.prompt` (v2 admission) |
| session_command / share | v1 `/session/{id}/command`·`/share` | **new** v1 methods |
| config_get | v1 GET `/config` | **new** `config` |
| app_agents | v1 GET `/agent` | **new** `app` |
| permission_reply | v1 POST `/permission/{id}/reply` | **new** `permission.reply` |
| path_get | v1 GET `/path` | **new** `path` |
| subscribe | v1 GET `/event` | **new** `event` (v1 GlobalEvent) |

- **RunClient** (`oc-cli`): re-implement trait over `OpenCode`; `AttachClient::new(url, dir, pw, user)`
  builds `ClientOptions { base_url, headers: Basic-auth }` + `ReqwestExecutor`. `LocalClient::create`
  builds `OpenCode` over `RouterExecutor` (router supplied by composition root).
- **TUI `SdkClient`** (`oc-tui`): keep trait; adapter over `OpenCode`. `subscribe_events` keeps the
  `startSSE` backoff loop; everything else maps to groups. Delete `SseParser` + raw reqwest.
- **ACP `OpencodeClient`** (`oc-acp`): adapter over `OpenCode`; trait surface unchanged; `mcp_add`
  deferred to Agent 13.

All three adapters are intentionally "narrow": the trait + data shapes stay in the consumer crate;
only the HTTP/SSE mechanics move into oc-client. `RunClient` trait home moves to `oc-app` (Agent 02)
as the shared contract; adapters implement it there or in oc-cli per Agent 02's layout.

---

## 6. Test list

**oc-client (extend `tests/sse.rs`, `tests/http.rs`):**
1. Multi-event-per-chunk: 3+ events in one chunk, exact order, zero loss (regression for SSE-001).
2. Event split across chunk boundary mid-frame and mid-`data:` line.
3. CRLF line endings (`\r\n`) within and across chunks.
4. Multiline data: multiple `data:` lines joined with `\n` and parsed.
5. Comments: `: heartbeat` and other comment-only blocks yield nothing.
6. `event:`/`id:`/`retry:` fields ignored (data-only decode); `id:` sets `Last-Event-ID` on reconnect.
7. Trailing `\r` carried across chunk boundary.
8. Final block without trailing `\n\n` flushed on EOF.
9. >1 MiB buffer → `MalformedResponse`; non-JSON data → `MalformedResponse` (strict).
10. Golden: decode captured reference bytes (`artifacts/07-reference-serve.txt`), shared with Agent 10.
11. `ReconnectingSseStream`: backoff 3s→6s→…→30s cap; `max_attempts` termination; reconnect re-sends
    the request (assert via recorded request count).
12. v1 endpoint goldens for every new method (path/body/status, like `http.rs`).

**Local transport (oc-app or oc-server tests):**
13. **Local-vs-network contract equality:** same requests through `RouterExecutor` and
    `ReqwestExecutor` (against the same mock/real server) yield byte-identical parsed responses for
    health / session create+get+list / prompt / event stream.
14. Local SSE: two events in one body chunk → both decoded through the local path.
15. Local mode binds **no** socket (assert no listener created).

**Adapters (oc-cli / oc-tui / oc-acp):**
16. RunClient-via-oc-client reproduces the SESSION-005 repro: two text events in one chunk → both
    printed, exit 0 (binary/E2E under Agent 18 harness).
17. `run --attach` against the real reference server still round-trips (regression).
18. TUI `subscribe_events` delivers events through the reconnect loop.
19. ACP `OpencodeClient` against a mock server: session round-trip + `permission.asked` via
    `global_event`.

---

## 7. Dependencies on other agents

| Agent | Finding(s) | Dependency for me | What I provide back |
|---|---|---|---|
| **02** | INTEGRATION-001, ARCH-008 | `RunClient` trait home + `oc-app` backbone; `RouterExecutor` placement; oc-client types promoted to oc-schema. | `HttpExecutor` trait + adapter implementations the composition root wires (`App::local_client()`). |
| **07** | TOOLS-001, ASYNC-004 | Server handlers emit real `message.part.*`/`session.status` events (runner wiring) so adapters see a live stream; interrupt semantics. | Client that consumes their event stream; interrupt call sites. |
| **10** | SERVER-03/05, SSE-002, SEC-002/003 | Mounted axum router + `AppState` that `RouterExecutor` wraps; bare-`data:` SSE framing as the shared golden contract. | Parser that reads those frames byte-for-byte; shared fixture; attach E2E once serve is real. |
| **12** | CLI-001 | `LocalClient::create` in oc-cli builds the router + hands it to my local transport; attach flow. | The local transport + `RunClient` adapter the CLI seam uses. |
| **14** | RUST-004, clippy | `-D warnings` gate (45 baseline errors); no `unwrap`/`unsafe` in new code; blocking IO out of async paths. | Clippy-clean transport/adapter code. |
| **16** | CLI-003 | oc-tui consumes my adapter via the TUI launch seam. | `SdkClient` adapter over oc-client. |
| (13) | PROTO-001 | ACP service wiring; `mcp_add` endpoint. | ACP `OpencodeClient` adapter. |

---

## 8. Risks

1. **v1/v2 endpoint mix** is the dominant failure mode: run/TUI/ACP use different surfaces, and a wrong
   path silently breaks a flow. Mitigate with per-endpoint request goldens (http.rs pattern).
2. **Prompt endpoint ambiguity:** oc-client `sessions.prompt` (v2 admission) must not be reused for the
   run flow; the v1 `/session/{id}/message` adapter method is required for reference parity.
3. **Event-envelope mismatch:** v1 `/event` (`{id,type,properties}`) vs v2 `/global/event`
   (`GlobalEvent` envelope) differ; a client mixing them fails to parse. Separate types + tests.
4. **RouterExecutor streaming:** `oneshot` returns the response before the body is consumed; SSE body
   must be an unbuffered stream (use `into_data_stream`, not `to_bytes`) to avoid buffering a long
   event stream / deadlock.
5. **Cross-crate ownership:** touching oc-cli/oc-tui/oc-acp requires coordinator agreement on
   boundaries; coordinate with 02/12/13/16 to avoid merge conflicts with their consumer changes.
6. **Type-promotion stall (Agent 01):** adapter mapping may use local mirrors until canonical oc-schema
   types land; keep mirrors `TODO(integration)`-marked.
7. **Reconnect hangs:** reference run.ts effectively retries forever (no `sseMaxRetryAttempts`); match
   it but log after the first failure so a dead server is diagnosable.
8. **Security gate:** `RouterExecutor` runs real handlers in-process; local mode must still enforce the
   permission gate (Agent 08) and never bind a socket before SEC-001/002/003 land (same gate as plan-10).
9. **SSE-001 regression must ship with its test** (two-events-in-one-chunk repro) or the silent-loss bug
   can quietly return.

---

## 9. Merge-order recommendation

**Wave 1 — oc-client standalone (behavior-preserving, in-crate green, merge any time):** SSE decoder
hardening + goldens (incl. captured reference bytes), `HttpExecutor`/`ReqwestExecutor` refactor, new
v1/v1-instance group methods with request/response goldens, `ReconnectingSseStream`. No consumer churn;
workspace stays green (`cargo build --workspace && cargo test -p oc-client`).

**Wave 2 — transport (depends on Agent 02 backbone + Agent 01 types):** `RouterExecutor` in oc-app;
`OpenCode::make_with_executor`; local-vs-network contract-equality tests; `RunClient` adapter + trait
move to oc-app.

**Wave 3 — consumer cut-over, alongside server (Agent 10), gated on Agent 12/13/16:** switch oc-cli
RunClient (delete `sse_stream` + AttachClient HTTP → SSE-001 fixed), oc-tui SdkClient, oc-acp
OpencodeClient onto oc-client; wire `LocalClient::create` (Agent 12) and attach; binary E2E under
Agent 18. Land together with the server mount so attach/local round-trips are testable in one PR.

**Hard gate before Wave 3:** SEC-001/002/003 merged (same gate as plan-10) — mounting the real router
(and the local in-process path) turns dormant stubs into a live tool-execution + file-read surface.
