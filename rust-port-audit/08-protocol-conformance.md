# Agent 08 — JSON-RPC, ACP, MCP, and Wire-Protocol Conformance

Auditor: Agent 08 | Commit audited: `e7fc33e` | Reference: vendored v1.18.13 + `/root/.opencode/bin/opencode` (black-box oracle)

## Scope

Wire protocols only: JSON-RPC 2.0 (MCP + ACP envelopes, IDs, notifications, errors, codes, negotiation), MCP client
transports (stdio, streamable HTTP, legacy SSE), ACP server wire surface, and the opencode client/server HTTP + SSE
wire (route paths, `event:`/`data:` framing, heartbeats, error envelope shapes). Crate-level conformance of
`oc-mcp`, `oc-acp`, `oc-client`, `oc-server`, and their wiring in `oc-cli`.

## Repository areas inspected

- `crates/oc-mcp/src/` (`jsonrpc.rs`, `client.rs`, `types.rs`, `catalog.rs`, `index.rs`, `transport/{mod,stdio,sse,http}.rs`)
- `crates/oc-acp/src/` (`jsonrpc.rs`, `agent.rs`, `service.rs`, `connection.rs`, `error.rs`, `types.rs`, `event.rs`)
- `crates/oc-client/src/` (`transport.rs`, `sse.rs`, `contract.rs`, `error.rs`, `types/`, `generated.rs`)
- `crates/oc-server/src/` (`sse.rs`, `router.rs`, `event.rs`, `errors.rs`, `handlers/session.rs`)
- `crates/oc-cli/src/cli/cmd/{acp,mcp,serve,run/*}.rs`, crate dependency edges (`Cargo.toml`)
- `reference/packages/opencode/src/acp/{agent,service}.ts`, `reference/packages/opencode/src/mcp/{index,catalog}.ts`,
  `reference/packages/opencode/src/cli/cmd/{acp,mcp}.ts`,
  `reference/packages/server/src/handlers/{event,session}.ts`,
  `reference/packages/opencode/src/server/routes/instance/httpapi/handlers/event.ts`,
  `reference/packages/protocol/src/{api,errors}.ts`, `reference/packages/client/src/{contract.ts,generated/client.ts}`

## Commands executed

- Static: `git log`, `grep` for `oc_mcp`/`oc_acp`/`oc_client`/`oc_server` usage across crates, `grep TODO(integration)` in focus crates.
- Runtime (Rust binary `/root/opencode-rs/target/debug/opencode`): `acp --help`, `mcp --help`, `mcp list`, `serve`,
  `run`, `acp` fed a JSON-RPC initialize over stdin.
- Runtime (reference oracle `/root/.opencode/bin/opencode`): `mcp --help`, `mcp list` (against a disposable mock
  MCP stdio server), `acp` fed initialize/unknown methods/malformed/batch lines over stdin.
- `cargo`-built test binaries: `oc_mcp` unit (50), `oc_acp` unit (57), `oc_client` (0 unit + 8 contract + 5 sse),
  `oc_server` unit (34) + api (15) + route_table (3), `oc-mcp/tests/stdio` (3), `oc-mcp/tests/http_oauth` (3),
  `oc-acp/tests/wire_golden` (8) — all PASS (see Test coverage gaps for staleness caveat).

## Runtime scenarios attempted

1. **Rust `opencode acp` vs reference `opencode acp` (JSON-RPC over ndjson stdin/stdout).**
   Reference: `{"jsonrpc":"2.0","id":1,"method":"initialize",...}` → responded
   `{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{...},"authMethods":[{"id":"opencode-login",...}],"agentInfo":{"name":"OpenCode","version":"1.18.13"}}}`
   (full valid ACP initialize result; `protocolVersion:1` integer). Unknown methods → `-32601` with
   `data:{"method":...}`; empty params → `-32602` zod-shaped data. **Rust `opencode acp` produced zero bytes** — it
   only binds a TCP socket and never reads stdin.
2. **Rust `opencode serve` vs HTTP.** `curl` to the Rust server port returned an empty body and no HTTP response
   (connection accepted, bytes drained, nothing written). `serve.rs` binds a bare socket.
3. **Rust `opencode run` with a disposable `opencode.json` configuring a mock MCP stdio server.** Failed with
   `the in-process opencode server is not wired yet (TODO(integration): oc-server)`; the mock MCP server was never
   spawned (log file absent). The MCP client is unreachable from the executable.
4. **Reference `opencode mcp list` against the same mock server** (mock implements `initialize` + `tools/list` +
   `tools/call`, see `artifacts/08-mcp-server.py`). Reference reported `audit ✓ connected` and its wire bytes were
   captured: `initialize` sent `protocolVersion:"2025-11-25"` with `id:0`, then `notifications/initialized`,
   then `tools/list`. This proves the reference MCP wire path works end-to-end against a third-party server, and
   proves a **protocol-version negotiation divergence** in `oc-mcp` (see PROTO-08).
5. **Reference `opencode acp` batch/malformed handling.** Non-JSON lines and JSON-RPC batch arrays are silently
   ignored; invalid params produce zod-shaped `-32602`.

## Architecture or behavior summary

The four in-scope wire crates are **complete, internally-consistent libraries that are not wired into the production
executable**:

- `oc-mcp` faithfully mirrors `@modelcontextprotocol/sdk` client behavior: newline-delimited JSON stdio, streamable
  HTTP (`mcp-session-id`/`mcp-protocol-version` headers, 401/403 OAuth, `WWW-Authenticate` challenge parsing, session
  recovery TODO), legacy SSE (`endpoint` event, reconnect loop), pending-map request/response correlation,
  `_meta.progressToken` on `tools/call`, roots/list handler, pagination with duplicate-cursor detection, tolerant
  tool parsing. `opencode mcp` in `oc-cli` is 100% stubbed (`not_wired`).
- `oc-acp` implements the ACP **service** (all 13 methods: initialize, authenticate, newSession, loadSession,
  listSessions, resumeSession, closeSession, forkSession, setSessionConfigOption, setSessionMode,
  setSessionModel, prompt, cancel) and error mapping (`-32601/-32602/-32603/-32000`), matching the reference ACP
  SDK envelopes exactly. It has **no JSON-RPC dispatcher and no stdio transport**; `opencode acp` in `oc-cli` only
  binds a socket.
- `oc-client` is a typed HTTP client whose `SseDecoder`, retry, error decoding, and contract tables match
  `reference/packages/client`. It is referenced only from TODO comments in `oc-cli`; `opencode run` instead uses a
  hand-rolled `reqwest` `RunClient` (`run/client.rs`) and `opencode serve` serves nothing.
- `oc-server` implements the axum route tree (v1 `/event` + v2 `/api/event`, `{data:...}` envelopes,
  `_tag`-tagged errors) and is only exercised by its own tests. `oc-server` is not referenced by `oc-cli` either.

SSE framing (`event: message` + `data:` JSON + blank line) and the client `data:` decoder are faithful mirrors of the
reference's `Sse.encode()` and generated client, including the 1 MiB buffer cap and CRLF normalization.

## Positive observations

- `oc-mcp` JSON-RPC envelope types and serialization match the MCP SDK (`RequestId` string|number, `params` omitted
  when `None`, `notifications/initialized` shape) and are verified against a **real wire server** (Python stdio test).
- `oc-mcp` `tools/call` sends `_meta.progressToken` with `resetTimeoutOnProgress` semantics, matching
  `reference/packages/opencode/src/mcp/catalog.ts:53-67` (`onprogress: () => {}`).
- `oc-acp` `initialize` response JSON (runtime-compared against the reference oracle output) matches exactly,
  including `protocolVersion:1`, `sessionCapabilities.{close,fork,list,resume}`, and `opencode-login` auth method.
- `oc-acp` method-not-found error envelope matches the reference oracle byte-for-byte
  (`{"code":-32601,"message":"\"Method not found\": <m>","data":{"method":<m>}}`, `error.rs:160-170`).
- `oc-client` contract tables (`contract.rs`) match `reference/packages/client/src/contract.ts` exactly; SSE decoder
  mirrors the generated client including trailing-CR-across-chunks handling and `MalformedResponse` caps.
- `oc-server` v2 `/api/event` heartbeat is an SSE comment at 15s (matches `reference/.../handlers/event.ts`), event
  bus capacity 256 matches `subscriberCapacity`, and the `{id,type,data}` / `{id,type,properties}` shapes match.
- Error envelopes: v2 `_tag`-tagged and v1 `{name,data}` shapes match `protocol/errors.ts` /
  `instance/httpapi/errors.ts`.

## Findings summary

| ID | Severity | Confidence | Finding |
|----|----------|------------|---------|
| PROTO-01 | Critical | CONFIRMED | No wire crate reachable from the production executable; `serve`/`run` do not serve |
| PROTO-02 | Critical | CONFIRMED | `opencode acp` is a stub — no ACP JSON-RPC dispatcher or stdio transport exists |
| PROTO-03 | Critical | CONFIRMED | `opencode mcp` is fully stubbed (`not_wired`) |
| PROTO-04 | High | CONFIRMED | MCP protocol-version negotiation diverges from reference binary (`2025-06-18` vs `2025-11-25`) |
| PROTO-05 | High | CONFIRMED | v1 `/event` heartbeat is an SSE comment, not the reference's real `server.heartbeat` event; no location filter / `server.instance.disposed` |
| PROTO-06 | Medium | CONFIRMED | Golden fixtures are hand-written (from source-reading), not captured from the reference executable |
| PROTO-07 | Medium | CONFIRMED | oc-client duplicates 26 canonical schema types locally (`TODO(integration): promote to oc-schema`) |
| PROTO-08 | Medium | HIGH | v2 session-events stream emits an initial blank SSE frame the reference does not |
| PROTO-09 | Low | HIGH | MCP `RequestId::Number(u64)` rejects negative/fractional numeric ids the JS SDK tolerates; `id:null` parse failures dropped silently |
| PROTO-10 | Informational | HIGH | oc-client/oc-server/oc-acp/oc-mcp each internally consistent and heavily unit-tested, but only against synthetic mocks |

## Detailed findings

### [PROTO-01] No wire crate is reachable from the production executable (Critical, CONFIRMED)

- `oc_mcp` appears in production code **nowhere** outside `crates/oc-mcp` (only its own `tests/`). `McpCatalog`
  exists only inside `oc-mcp`. `oc-tool`'s `mcp_websearch` is a standalone raw-reqwest `tools/call` client
  (`oc-tool/src/tool/mcp_websearch.rs:112-146`), not the `oc-mcp` client.
- `oc_acp` appears only in `crates/oc-acp` and its own tests; `oc-server` has zero references to `oc_acp`/`oc_mcp`.
- `oc_client` is referenced only in two TODO comments (`oc-cli/src/cli/cmd/attach.rs:72`,
  `oc-cli/src/cli/cmd/run/client.rs:59`). `opencode run` uses a hand-rolled `reqwest` `RunClient`
  (`run/client.rs:73-455`), and its local in-process path **always errors** (`run/client.rs:65-69`).
- `oc_server` referenced only in a TODO (`oc-cli/src/cli/cmd/serve.rs:38`); `opencode serve` binds a bare TCP
  socket that accepts connections and drains bytes without writing an HTTP response (`serve.rs:40-67`).
- RUNTIME: `opencode run` → `Error: the in-process opencode server is not wired yet`; mock MCP server never spawned
  (no log file); `curl` to `opencode serve` returned no HTTP response.
- **Every protocol path in my scope is dead code from the executable's perspective.** No MCP, ACP, oc-client, or
  oc-server wire path is exercised end-to-end by the production binary.

### [PROTO-02] `opencode acp` is a stub; no ACP JSON-RPC dispatcher or transport (Critical, CONFIRMED)

- `oc-cli/src/cli/cmd/acp.rs:16-26`: binds a `TcpListener` then `std::future::pending::<()>().await`; the comment
  says "Today we only bind the listen socket".
- `oc-acp/src/connection.rs:6` explicitly documents `TODO(integration): oc-server / oc-cli stdio connection`.
- `oc-acp/src/agent.rs:37-132` implements all 13 methods as a typed facade, but nothing maps JSON-RPC `method`
  strings (camelCase) onto them, matches `id`s, handles `session/cancel` notifications, or reads/writes
  newline-delimited JSON. `oc-acp/src/jsonrpc.rs` types are used only by unit tests.
- RUNTIME: reference `opencode acp` answered `initialize` over stdin; Rust `opencode acp` emitted nothing.

### [PROTO-03] `opencode mcp` fully stubbed (Critical, CONFIRMED)

- `oc-cli/src/cli/cmd/mcp.rs:19-94`: `add`, `list`, `auth`, `logout`, `debug` all return
  `not_wired("...not yet wired...")`. Only argument validation is ported.
- RUNTIME: Rust `opencode mcp list` → `Error: MCP server listing is not yet wired...`; reference `opencode mcp list`
  connected to the mock server and printed `audit ✓ connected`.

### [PROTO-04] MCP protocol-version negotiation diverges from the reference binary (High, CONFIRMED)

- `oc-mcp/src/types.rs:13` `LATEST_PROTOCOL_VERSION = "2025-06-18"`; `:16` `SUPPORTED_PROTOCOL_VERSIONS =
  ["2025-06-18","2025-03-26","2024-11-05"]`. `client.rs:116-121` rejects any other echoed version.
- RUNTIME (reference oracle): the reference sends `"protocolVersion":"2025-11-25"` in `initialize` and **accepts**
  `2025-11-25` in the response (probe #5). The Rust client would advertise `2025-06-18` and reject a `2025-11-25`
  server with "Server's protocol version is not supported". This is baked into a golden test (`jsonrpc.rs:199`,
  `tests/stdio.rs:265`), so the fixtures encode the stale version rather than reference behavior.

### [PROTO-05] v1 `/event` heartbeat is an SSE comment, not a `server.heartbeat` event (High, CONFIRMED)

- Reference (`.../routes/instance/httpapi/handlers/event.ts:63-66`) emits `{id,type:"server.heartbeat",properties:{}}`
  every 10s as a **real event**, filters events by location (`:35-39`), and terminates on `server.instance.disposed`
  (`:59-62`).
- Rust (`oc-server/src/sse.rs:77-93`) uses axum `KeepAlive::text("heartbeat")`, which serializes to the SSE comment
  `: heartbeat\n\n` (verified in `axum-0.7.9/src/response/sse.rs:441-446,375-378`), does no location filtering, and
  never terminates. Clients that depend on `server.heartbeat` events (reference TUI/session-ui) will see different
  wire data.

### [PROTO-06] Golden fixtures are hand-written, not derived from the reference (Medium, CONFIRMED)

- `oc-mcp/tests/stdio.rs:24-96` uses a purpose-built Python server; the exact-wire assertions
  (`stdio.rs:263-279`) are hand-crafted from source reading, and are themselves affected by PROTO-04 (the
  `protocolVersion` string). They are still genuine runtime wire tests (not pure serialization goldens).
- `oc-acp/tests/wire_golden.rs` drives the service with an in-memory `FakeSdk` and `RecordingConnection`; expected
  values are hand-written.
- `oc-client/tests/{sse,contract,http}.rs` use a hand-written `MockServer`; contract lists are hand-copied from
  `contract.ts` (verified identical).
- No fixture in the focus crates was captured from the reference executable. Since bun/node is unavailable,
  differential wire capture against the reference was only possible through the stock binary (used in PROTO-01/02/03/04).

### [PROTO-07] Local type mirrors duplicate canonical `oc-schema` types (Medium, CONFIRMED)

- Every one of the 26 `oc-client/src/types/*.rs` files carries `// TODO(integration): promote to oc-schema`
  (e.g. `types/session.rs:4`). `oc-client` contains zero `use oc_schema` references; `SessionInfo` duplicates
  `oc-schema::session::Info` with subtly different field types (`SessionTime` vs `Time`, `f64` vs `Finite`).
- `oc-mcp/src/config.rs:6` mirrors `oc-config` MCP config; `oc-acp/src/connection.rs` lacks a concrete transport.
  Risk: silent wire drift between the canonical schema and the client/server mirrors.

### [PROTO-08] v2 session-events stream emits an initial blank SSE frame (Medium, HIGH)

- `oc-server/src/sse.rs:97-99` passes `SseEvent::default()` as the first event; axum `Event::default().finalize()`
  serializes to a lone `\n` frame (`axum-0.7.9/.../sse.rs:375-378`). Reference `session.events`
  (`handlers/session.ts:357-364`) streams no initial frame. Tolerant clients skip the empty block; still a wire deviation.

### [PROTO-09] MCP numeric request-id constraints narrower than JS SDK (Low, HIGH)

- `oc-mcp/src/jsonrpc.rs:24-29` `RequestId::Number(u64)` rejects negative and fractional numeric ids that the JS
  SDK (`string | number`) would accept; a response with `"id": null` (JSON-RPC parse-error responses) fails the
  untagged `Message` parse and is silently dropped by the stdio read loop (`transport/stdio.rs:103-111`) instead of
  being surfaced.

### [PROTO-10] Internal consistency (Informational, HIGH)

- All focus crates' test suites pass from the current build graph: oc-mcp 50 unit + 3 stdio + 3 http_oauth,
  oc-acp 57 unit + 8 wire_golden, oc-client 8 contract + 5 sse, oc-server 34 unit + 15 api + 3 route_table.
  Caveat: the shared target dir is concurrently rebuilt by other agents; the one stale binary I caught
  (`oc_mcp-12ce...`) failed a `preserve_order` golden that the current source (commit `fd99c06`) already corrected.
  These tests validate crate-internal conformance only, not the production wiring.

## Feature or behavior gaps

- `opencode acp`: no JSON-RPC server, no `AgentSideConnection`/`sessionUpdate` push path, no `prompt`/tool events.
- `opencode mcp`: `add`/`list`/`auth`/`logout`/`debug` all unimplemented; MCP tool/resource/prompt loading is never
  surfaced into sessions (reference does so in `session/prompt.ts` and `session/system.ts` via `McpCatalog`).
- `opencode serve`: binds a socket but serves no HTTP/SSE; `/api/event`, `/event`, `/api/session/.../event`, route
  tree, CORS, and auth middleware are never exercised.
- `opencode run` (local): in-process server path errors; MCP tools unavailable.

## Test coverage gaps

- No differential (black-box) wire tests against the reference binary for any crate (would require bun/network).
- No transport-level test for the ACP wire (no transport exists to test).
- No tests for: MCP version negotiation with a `2025-11-25` server; v1 event heartbeat/`server.instance.disposed`;
  MCP stdio partial-line/multi-frame reads and backpressure; duplicate/out-of-order request ids; batch JSON-RPC.
- `oc-client` has no test that its SSE decoder consumes a real `oc-server` stream (both exist only against mocks).

## Unverified areas

- Reference SDK `SUPPORTED_PROTOCOL_VERSIONS` contents (SDK not vendored); PROTO-04 rests on the reference binary's
  wire behavior + the fact the reference accepts `2025-11-25`.
- OAuth flows (`oc-mcp/oauth*.rs`) — tested only against a local mock; not wired to a real provider.
- `oc-server` router reachability of every declared route (handler logic largely unexecuted in production).

## Final domain verdict

**NOT_READY**

The four wire-protocol crates are well-built libraries with passing internal tests and faithful mirrors of the
reference's message shapes, SSE framing, and error envelopes — but none of them is reachable from the production
binary. `opencode acp`, `opencode mcp`, `opencode serve`, and local `opencode run` are all non-functional stubs, and
the MCP client/ACP server/HTTP server/RPC client are exercised only by their own tests. Combined with the confirmed
MCP protocol-version divergence (`2025-06-18` vs the reference binary's `2025-11-25`), the wire-protocol layer
cannot be considered conformance-complete.
