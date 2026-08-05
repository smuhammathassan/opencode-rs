# Agent 14 — ACP JSON-RPC Dispatcher, Stdio Transport & `opencode acp` Wiring — Implementation Plan

**Agent:** 20-AG-14 · **Domain:** ACP (Agent Client Protocol) JSON-RPC + transport
**Repo:** `/root/opencode-rs` · **Branch:** `fix/audit-remediation`
**Status:** Wave 0 READ-ONLY plan (no production source modified)

Reference spec: `reference/packages/opencode/src/acp/{agent,service}.ts`,
`reference/packages/opencode/src/cli/cmd/acp.ts`, `@agentclientprotocol/sdk` 0.21.0
(vendored `reference/packages/opencode/package.json:57`). Reference oracle (black-box):
`/root/.opencode/bin/opencode acp`. Wire probes captured at `/tmp/agent14-acp-oracle-probes.md`.

---

## 1. Owned findings

Consolidated ownership: **PROTO-001 (ACP parts)**, **PROTO-002 (ACP parts)**, plus ACP rows of
PROTO-006 / PROTO-010 from `rust-port-audit/08-protocol-conformance.md`. PROTO-001's CSV row
(`FINDING-STATUS.csv:8`) currently names 20-AG-13 only — the MCP half; recommend splitting the row:
**MCP → AG-13, ACP → AG-14**.

| ID | Severity | File:line (current) | Root cause / gap |
|---|---|---|---|
| PROTO-002 / PROTO-001 (ACP) | Critical | `crates/oc-cli/src/cli/cmd/acp.rs:12-27` | `opencode acp` binds a `TcpListener` then `std::future::pending().await` (acp.rs:21-24). Zero bytes answered; never reads stdin. Comment acp.rs:16-17 admits the stub. |
| PROTO-001 (ACP) | Critical | `crates/oc-acp/src/connection.rs:6` | `AgentSideConnection` is a trait with no concrete transport; `// TODO(integration): oc-server / oc-cli stdio connection`. |
| PROTO-001 (ACP) | Critical | `crates/oc-acp/src/jsonrpc.rs:1-77` | Envelope types exist (RpcMessage/Request/Notification/Response/Error) but only unit-test call sites. **No JSON-RPC dispatcher** maps camelCase `method` strings onto `Agent` (agent.rs:37-132); **no params validation** (zod-shaped `-32602`); no request/response id correlation; `session/cancel` notifications unhandled. |
| PROTO-001 (ACP) | Critical | `crates/oc-acp/src/service.rs:105-208` | Service is complete (all 13 methods) and initialize bytes already match the oracle **except `agentInfo.version`** (service.rs:49-51 `installation_version()` returns `"local"` unless `OPENCODE_VERSION` set; oracle reports `"1.18.13"`). |
| PROTO-001 (ACP) | Critical | `crates/oc-acp/src/sdk.rs:10-11,600-642` | `OpencodeClient` trait has no production impl; `// TODO(integration): implement for the oc-client HTTP client once it exists`. The ACP command cannot create sessions until Agent 11 provides the adapter. |
| PROTO-006 (ACP) | Medium | `crates/oc-acp/tests/wire_golden.rs:1-491` | Golden fixtures hand-written from source, not captured from the reference executable; no differential wire test, no transport-level test (PROTO-01 gap "No transport-level test for the ACP wire (no transport exists to test)"). |
| PROTO-010 (ACP) | Info | all oc-acp tests | 57 unit + 8 wire_golden pass but only against synthetic `FakeSdk`/`RecordingConnection`. |
| PROTO-002 (version) | Low | `service.rs:49-51` | Byte-parity of `initialize` requires the shared workspace version constant (Agent 13/19). |

**Not owned (coordinate only):** MCP half of PROTO-001 (Agent 13), oc-client `OpencodeClient`
adapter + SSE (Agent 11, SSE-001), oc-server mount/security gate (Agent 10), version constant
(PROTO-002/RELEASE-002 → Agents 13/19), canonical type promotion (Agent 01, TEST-002), binary
test harness (Agent 18, TEST-001).

---

## 2. Files to change

| File | Change |
|---|---|
| `crates/oc-acp/src/jsonrpc.rs` | Keep envelope types. Add `RpcRequest` → `{jsonrpc,id,method,?params}` already OK; ensure untagged `RpcMessage` also parses client→server `Response`/`Error` (ignored). Minor: `RequestId::Number(i64)` mirrors the ACP SDK client's numeric ids; keep. |
| `crates/oc-acp/src/params.rs` (new) | Zod-shaped request-params validator: per-method schema (required/optional fields + types), emitting the `-32602` error `data` tree exactly like the SDK zod output (`{"_errors":[...]}`; field trees for missing/wrong-typed/`null`-for-required). |
| `crates/oc-acp/src/dispatcher.rs` (new) | `Dispatcher { agent: Arc<Agent> }` — method table (13 entries) → typed `Agent` calls; params validation; response/error envelope construction; notification handling (`session/cancel`); unknown method → `method_not_found`. |
| `crates/oc-acp/src/transport.rs` (new) | Stdio ndjson transport: read stdin line framing, per-line dispatch (concurrent tasks + id correlation), single-atomic write per response; `StdioConnection` implementing `AgentSideConnection` emitting `session/update`, `session/request_permission`, `fs/write_text_file` notifications. |
| `crates/oc-acp/src/lib.rs` | Register `params`, `dispatcher`, `transport`. |
| `crates/oc-cli/src/cli/cmd/acp.rs` | Replace stub with real wiring (§4). |
| `crates/oc-acp/tests/dispatcher.rs` (new) | In-memory dispatcher tests (method table, zod-shaped errors, notifications, id echo). |
| `crates/oc-acp/tests/stdio_transport.rs` (new) | In-process duplex (tokio pipes) framing/dispatch tests incl. cancellation. |
| `crates/oc-cli/tests/acp_e2e.rs` (new; under Agent 18 harness) | Binary-level differential vs captured reference bytes + lifecycle. |
| `rust-port-remediation/FINDING-STATUS.csv` | Split PROTO-001 row: add ACP→20-AG-14. |

No `Cargo.toml` changes: oc-cli already declares `oc-acp`, `oc-server`, `oc-client`, `tokio`
(`oc-cli/Cargo.toml:31-37`); oc-acp already has serde_json/tokio/futures (`oc-acp/Cargo.toml:9-24`).

---

## 3. Dispatcher + transport design

### 3.1 Envelope model (reuse `jsonrpc.rs`)

- Inbound: untagged `RpcMessage`. `Request` (has `id`) and `Notification` (no `id`) are dispatched;
  `Response`/`Error` arriving at the agent are ignored (oracle: no output). `id:null` is a valid
  request id and is echoed (oracle id 4-null probe). `jsonrpc` value is not validated (oracle
  answered `"jsonrpc":"1.0"` with a `"2.0"` response).
- Outbound success: `{"jsonrpc":"2.0","id":<same>,"result":{...}}`.
- Outbound error: `{"jsonrpc":"2.0","id":<same>,"error":{code,message,?data}}` via the existing
  `crate::types::RequestError` (types.rs:852-919), whose `method_not_found`/`invalid_params`/
  `auth_required`/`internal_error` builders already reproduce the oracle envelopes byte-for-byte
  (verified: error.rs:160-170).

### 3.2 Method table

| Method | Agent call | Notes |
|---|---|---|
| `initialize` | `initialize` | `agentInfo.version` from shared version constant (§4). |
| `authenticate` | `authenticate` | service error → `-32602` (oracle id 6 probe). |
| `session/new` | `new_session` | |
| `session/load` | `load_session` | |
| `session/list` | `list_sessions` | |
| `session/resume` | `resume_session` | |
| `session/close` | `close_session` | |
| `session/fork` | `fork_session` | |
| `session/set_config_option` | `set_session_config_option` | |
| `session/set_mode` | `set_session_mode` | |
| `session/set_model` | `set_session_model` | |
| `session/prompt` | `prompt` | long-running; see concurrency §3.4. |
| `session/cancel` | `cancel` | notification only — never produces a response, errors dropped. |
| *(any other)* | — | `RequestError::method_not_found` → `-32601` `data:{method}` (oracle id 2 probe). |

### 3.3 Params validation (zod-shaped `-32602`)

serde deserialization alone is insufficient: `Option<T>` fields accept `null`/absent, but the SDK
zod schemas **reject** `null` and absent values for required fields (oracle id 15:
`additionalDirectories:null` → error; id 9: missing `protocolVersion` → error). Design a small
hand-rolled validator (`params.rs`) per method that mirrors the SDK schema surface:

- top-level must be an object → else `data:{"_errors":["Invalid input: expected object, received <undefined|null|string|array|boolean|number>"]}` (message from oracle: "undefined", "null", "string", …).
- per declared field: required-missing → `"received undefined"`; wrong type → `"expected <type>, received <actual>"` (oracle: "expected number, received string"; "expected array, received null").
- unknown extra fields accepted (oracle id 14 `"extra":"x"`).
- the `_meta`/`clientCapabilities._meta` maps are not deep-validated (oracle id 12 non-bool `terminal-auth` accepted).

Only the fields the reference test suite and the captured fixtures exercise are validated;
the fixture set (below) is the parity contract. This avoids an unbounded reimplementation of zod.

### 3.4 Concurrency & cancellation

Reference behavior requires that a `session/cancel` notification can be received **while a
`session/prompt` request is in flight** (ACP spec + the prompt→cancelled flow in `service.rs:1210-1213`).
Design: the read loop parses one line, then spawns a tokio task per message:

- Request → task runs the typed `Agent` method, awaits, then writes exactly one response line.
- Notification → task runs the handler and writes nothing (errors logged to stderr).

Responses are written through a shared `Mutex<tokio::io::BufWriter<Stdout>>`; each write is a
single full line so lines never interleave. For single-request differential fixtures, response
order is deterministic; the reference test client correlates strictly by `id` (acp-test-client.ts),
so completion order is not part of the parity contract.

### 3.5 Stdio framing (ndjson)

- Read: `tokio::io::BufReader<Stdin>` lines; drop empty lines; JSON-parse failure (incl. batch
  arrays `[...]`) → silently ignore (oracle: `not json`, `[]` both produce nothing).
- Write: `serde_json::to_string` + `\n`, flushed per message.
- **stdout is exclusively JSON-RPC**; all diagnostics go to stderr (reference cli-process.ts:48).
- EOF on stdin → cancel pending tasks, `server.stop(true)`, exit 0 (reference lifecycle parity).

### 3.6 Agent→client notifications (`StdioConnection : AgentSideConnection`)

`session_update` → `{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":...,"update":{...}}}`;
`request_permission` → `"session/request_permission"`; `write_text_file` → `"fs/write_text_file"`.
`ServiceInput.event_subscription` wires the started `event::Subscription` into the transport so
streaming `session/update` frames are emitted (event.rs already produces the `SessionUpdate`
values; the connection is the missing half).

---

## 4. Session-service binding (`oc-cli/src/cli/cmd/acp.rs`)

Sequence mirroring `reference/packages/opencode/src/cli/cmd/acp.ts:19-71`:

1. `Context::load(cwd)` where `cwd = args.cwd.unwrap_or(current_dir)`.
2. `std::env::set_var("OPENCODE_CLIENT", "acp")` (reference acp.ts:23).
3. `resolve_network_options(&args.network, None)` → `ListenOptions` (hostname/port/cors/mdns).
4. `oc_server::server::listen(opts)` → `Listener { url, port, .. }` (server.rs:58; port 0 → 4096 fallback already implemented). **Same mount as Agent 10's `serve`; subject to the same security gate (§9).**
5. Build the ACP SDK client at `listener.url` with `Authorization: Basic` from
   `oc_server::auth::basic_header("opencode", password)` when `OPENCODE_SERVER_PASSWORD` is set
   (`reference/packages/opencode/src/server/auth.ts:32-41` → oc-server auth.rs:58-63). Agent 11's
   adapter consumes `oc_client::ClientOptions { base_url, headers, .. }` (transport.rs:17-21).
6. `Service::make(ServiceInput::new(sdk).connection(stdio_conn).event_subscription(...))`.
7. `Agent::new(Arc::new(service))`, wrap in `Dispatcher`.
8. Run the stdio transport until stdin EOF; then `listener.stop(true)`; `Ok(0)`.

The `OpencodeClient` adapter is Agent 11's seam (see §8): it must map `session_*`, `config_*`,
`app_*`, `command_list`, `mcp_add`, `permission_reply`, and `global_event()` onto oc-client HTTP.
oc-client today has session/agent/skill/command/permission/event groups but **no config or mcp
group** — Agent 11 must add those (`config_providers`, `config_get`, `mcp_add`) or the adapter
uses raw reqwest.

---

## 5. Test list

**In-crate `oc-acp` (unit + tests/):**
1. Dispatcher method table: each of the 13 methods dispatches to the right `Agent` call; unknown method → byte-identical `-32601` envelope (`data:{"method":...}`).
2. Params-validation golden vs captured oracle: no params; `params:null`; `params:"hello"`; missing `protocolVersion`; `protocolVersion:"one"`; `additionalDirectories:null` → byte-identical zod `_errors` trees (fixtures from `/tmp/agent14-acp-oracle-probes.md`).
3. id echo: `id:1`, `id:"str-1"`, `id:null` echoed; `jsonrpc:"1.0"` still answered; extra unknown params accepted; `protocolVersion:2` still returns `protocolVersion:1`.
4. Notifications: `session/cancel` → no response line; invalid-params notification dropped silently; incoming `result`/`error` messages ignored; batch `[]` and non-JSON lines ignored.
5. Agent→client notifications: after `session/new`, stdout receives a `session/update` line with `sessionUpdate:"available_commands_update"`; exact `{"jsonrpc":"2.0","method":"session/update",...}` shape.
6. Cancellation: fake SDK whose `session_prompt` blocks until `session_abort`; send `session/prompt` then `session/cancel` → abort recorded, prompt resolves `stopReason:"cancelled"`.
7. Stdio framing: multi-line/partial-line chunks, empty lines, CRLF, backpressure (pipe-full) with no framing corruption.

**Binary/E2E (oc-cli, under Agent 18 harness / TEST-001 + differential):**
8. `opencode acp` answers `initialize` over stdin; line byte-identical to the captured reference (normalizing `agentInfo.version` until PROTO-002/RELEASE-002 lands).
9. Unknown-method and invalid-params error envelopes byte-identical to captured reference bytes.
10. stdin EOF → clean exit 0 (reference lifecycle.test.ts:14-25 parity).
11. Full round-trip differential vs the reference binary under an identical mock-LLM config (`OPENCODE_CONFIG_CONTENT` + `verifierConfig` analog from helpers.ts:38-68): `initialize` → `session/new` → `session/prompt` → observe `session/update` chunks → `session/close`.
12. `--port 0` → 4096 preference; `OPENCODE_SERVER_PASSWORD` set → SDK requests carry Basic auth.

---

## 6. Dependencies on other agents

| Agent | Finding(s) | What I need from them | What I provide back |
|---|---|---|---|
| **11** | SSE-001, PROTO-001 (client) | `oc_acp::sdk::OpencodeClient` impl over oc-client `OpenCode`: session/config/app/command/mcp/permission + `global_event()` SSE; add missing config & mcp groups; auth headers via `ClientOptions`. | The `OpencodeClient` trait as the binding seam (oc-acp/sdk.rs) + exact method/param shapes it must satisfy. |
| **07** | TOOLS-001, ASYNC-003/004 | Real `session_prompt`/`session_abort` behind the server (currently oc-server session_prompt appends only) so ACP `prompt`/`cancel` drive the actual LLM loop. | Cancel/abort call pattern already in `service.rs:cancel`/`abort_backing_session`. |
| **10** | CLI-002, SSE-002, SEC-002/003 | `oc_server::server::listen` mounted & secured; shared SSE framing contract for `global_event()`. | `opencode acp` reuses the exact same `ListenOptions`/`Listener` seam. |
| **02** | INTEGRATION-001 | Composition-root view: whether `acp` builds its own `AppState` or consumes a shared one; cwd/project context. | Command wiring sequence (§4) as a composition pattern. |
| **13** | PROTO-002 (version) | Shared workspace version constant so `initialize` `agentInfo.version` is byte-identical. | Same constant consumed here. |
| **19** | RELEASE-002 | Same version injection; logging (stdout purity, RELEASE-001) so only JSON-RPC hits stdout. | — |
| **01** | TEST-002 / ARCH-001 | Canonical `oc-schema` types (non-blocking) for sdk.rs mirror reconciliation. | — |
| **18** | TEST-001 | Binary + differential harness (reference `opencode` invocation, mock-LLM config). | Captured oracle fixtures + differential test list (§5.8-12). |

---

## 7. Risks

1. **Full zod error-tree parity is unbounded.** The SDK's zod output for deep/multi-error inputs
   is complex. Mitigation: validator covers exactly the fields exercised by the reference suite and
   the captured fixtures; those fixtures become the regression contract; residual divergence on
   exotic inputs is documented and acceptable.
2. **Transport concurrency semantics are not vendored** (`@agentclientprotocol/sdk` absent from
   `reference/node_modules`). The concurrent-notifications + id-correlated-responses design is
   inferred from ACP spec and observed behavior; ordering under pipelined requests is verified only
   via differential fixtures (single-in-flight), which is the realistic client pattern.
3. **`session/new` is not locally verifiable**: the oracle produced no response in this sandbox
   (no provider/config; needs the mock-LLM `verifierConfig` harness). Session round-trip tests
   therefore depend on Agent 18; local parity is limited to initialize/authenticate/error envelopes.
4. **Version-string divergence today**: `initialize` returns `"version":"local"` vs oracle
   `"1.18.13"`. Tests must normalize until PROTO-002/RELEASE-002 lands; do not bake `"local"` into a
   golden.
5. **Security exposure gate**: `opencode acp` mounts the HTTP server (SDK loopback), exposing the
   same handler surface as `serve`. Must NOT merge before SEC-001 (permission gate), SEC-002, and
   SEC-003 are in (Agent 10's hard gate, plan-10 §10). The stdio port itself is a local-process
   interface; the risk is the server-side endpoints the SDK hits.
6. **stdout purity**: any `println!`/tracing-to-stdout corrupts ndjson framing. Coordination with
   RELEASE-001 logging; unit-test the transport over pipes to catch regressions.
7. **`RequestId::Number(i64)`** rejects fractional numeric ids (JS `number` accepts them); ACP SDK
   clients send integers, matching the oracle probes — accepted limitation (PROTO-09 analog, not
   in scope).

---

## 8. Merge-order recommendation

- **Wave 1/2:** land `oc-acp` dispatcher + params validator + stdio transport + in-crate tests as
  **additive modules with zero production callers** (safe, crate-internal; de-risks later wiring).
- **Wave 2/3 (parallel):** Agent 11 `OpencodeClient` adapter, Agent 10 `serve` mount + security
  gate (SEC-001/002/003), Agent 07 runner-backed session endpoints, Agent 13/19 version constant.
- **Wave 4:** wire `oc-cli/src/cli/cmd/acp.rs` to the composition (§4) — gated on the Agent 10
  security set and on Agent 11's adapter. This closes PROTO-001 (ACP) and PROTO-002 (ACP).
  Land ACP ahead of full MCP tool wiring (Agent 13): the ACP command needs only the
  session/config/command/skill/agent endpoints the oc-server router already declares
  (router.rs:111,159-161,210-245), and it is the reference's primary external-agent surface.
- **Final differential wave:** binary ACP round-trip against the reference under a shared mock-LLM
  config (Agent 18 harness), plus the error/initialize byte-parity tests.

**Gate:** Wave 4 ACP wiring must not merge before SEC-001/002/003 and the mounted-server path
(Agent 10) — mounting the router turns dormant stubs into a reachable surface.
