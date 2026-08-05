# Plan 13 — MCP integration and bounded transports

Agent 13 · Wave 0 read-only planning. Repo `/root/opencode-rs` @ `fix/audit-remediation`.
Domain: MCP protocol-version negotiation, `opencode mcp` CLI + server surfaces, runtime wiring of
configured MCP servers, and bounded/lifecycle hardening of the MCP transports.

---

## 1. Owned findings

| ID | Sev | Evidence | Status |
|----|-----|----------|--------|
| PROTO-001 (MCP parts) | Critical | `oc_mcp` referenced in production source **nowhere** outside the crate. `oc-cli/src/cli/cmd/mcp.rs:19-94` all `not_wired`. `oc-server/src/instance_handlers.rs:950-1010` `mcp_status/mcp_add/mcp_auth_*/mcp_connect/mcp_disconnect` return empty `{}`/`true` stubs. `oc-mcp/src/types.rs:13-16` advertises `2025-06-18`. | CONFIRMED (runtime) |
| PROTO-004 | High | `types.rs:13` `LATEST_PROTOCOL_VERSION = "2025-06-18"`; `:16` supported list omits `2025-11-25`. Oracle re-verified this session: `/root/.opencode/bin/opencode` sends `initialize` with `protocolVersion:"2025-11-25"`, `clientInfo:{name:"opencode",version:"1.18.13"}`, id 0, and **accepts** a server echoing `2025-06-18`. Rust would reject a `2025-11-25` server. Baked into goldens `jsonrpc.rs:199`, `types.rs:463-469`, `tests/stdio.rs:265`. | CONFIRMED (runtime) |
| PROTO-002 | Low | `oc-mcp/src/lib.rs:68-70` `version() = env!("CARGO_PKG_VERSION")` = `0.1.0`; `index.rs:176-179` builds `client_info` from it; `tests/stdio.rs:110-115` hardcodes `"0.1.0"`. Reference sends install version. | CONFIRMED (static) |
| SEC-005 | Medium | `transport/mod.rs:73-104` `SseParser.buffer: String` grows without bound until `\n\n`/`\r\n\r\n`; `http.rs:182,339-360`, `sse.rs:111`, `stdio.rs:89` use `mpsc::unbounded_channel` — a remote server streaming boundary-less data or flooding events is an OOM DoS. | CONFIRMED (static) |
| ASYNC-010 | Medium | `http.rs:195-232` and `sse.rs:123-158` reconnect on fixed 1 s forever — no backoff/jitter/cap; N servers → 1 Hz poll storm + thundering herd. | CONFIRMED (static) |
| ASYNC-013 | Low | `transport/stdio.rs:61-68` spawns without `kill_on_drop`; only `close()` kills/reaps. No `Drop` — dropping a client/transport (panic paths, early returns) orphans the MCP server process. | CONFIRMED (static) |
| PROTO-009 | Low | `jsonrpc.rs:26-29` `RequestId::Number(u64)` rejects negative/fractional ids the JS SDK tolerates; `id:null` parse-error responses fail the untagged `Message` parse and are dropped silently (`stdio.rs:103-111`). | CONFIRMED (static) |
| — (session recovery) | Med | SDK patch (`reference/patches/@modelcontextprotocol%2Fsdk@1.29.0.patch`) adds `onsessionexpired → _initialize` re-run. Rust `http.rs:272-276` has `TODO(integration)`, only clears `session_id` and never re-initializes. | CONFIRMED (static) |
| — (config mirror) | Low | `oc-mcp/src/config.rs` mirrors `oc-config/src/v1/mcp.rs` (`TODO(integration): promote`). Canonical `mcp::Info` now exists; `Value::Enabled` (bare `{enabled}`) must be skipped like reference `isMcpConfigured`. | CONFIRMED (static) |

OAuth/PKCE behavior: verified well-formed — S256 challenge (`oauth.rs:386-399`), 32-hex `state` with
mismatch check (`index.rs:570-576`, CSRF), verifier stored per server (`auth.rs:189`), loopback
callback `127.0.0.1:19876/mcp/oauth/callback` with 5-min timeout (`oauth_callback.rs`). No PKCE defect;
only the CLI wiring is missing. Reference behavior mirrored in `reference/packages/opencode/src/mcp/`.

---

## 2. Files to change

Owned (this agent executes):
- `crates/oc-mcp/src/types.rs` — version constants + `initialize_result_parses` golden.
- `crates/oc-mcp/src/client.rs` — constants drive negotiation (auto); optional re-initialize on 404.
- `crates/oc-mcp/src/index.rs` — `McpOptions.client_version` injection (PROTO-002); optional bounded `events`.
- `crates/oc-mcp/src/lib.rs` — keep `version()` as crate default; document override path.
- `crates/oc-mcp/src/transport/mod.rs` — `SseParser` buffer cap; `MessageReceiver` → bounded `mpsc::Receiver`.
- `crates/oc-mcp/src/transport/http.rs` — bounded channel, backoff/jitter/cap reconnect, session-expiry re-initialize.
- `crates/oc-mcp/src/transport/sse.rs` — bounded channel, backoff reconnect.
- `crates/oc-mcp/src/transport/stdio.rs` — `kill_on_drop` + `Drop` guard; bounded channel.
- `crates/oc-mcp/src/jsonrpc.rs` — golden version update; optional `id:null` tolerance.
- `crates/oc-mcp/tests/stdio.rs`, `tests/http_oauth.rs` — version goldens; new negotiation/bounds/lifecycle tests.
- `crates/oc-cli/src/cli/cmd/mcp.rs` — real `add/list/auth/logout/debug` via in-process `oc_mcp::Mcp`.
- `crates/oc-cli/src/cli/mcp_util.rs` (new) — `oc_config::v1::mcp::Info` → `oc_mcp::config::Info` converter + shared `Mcp` construction.

Co-authored / integration (needs dependency agents, do NOT edit alone):
- `crates/oc-server/Cargo.toml` (+`oc-mcp`), `state.rs` (AppState.mcp), `instance_handlers.rs` (mcp_* handlers) — Agent 10 owns server composition; this plan defines the handler contract.
- `crates/oc-session/src/tools.rs` / runner tool registry — `McpTool` adapter consumption (Agent 02/08/09).
- `crates/oc-config/src/load.rs` / jsonc module — comment-preserving config edit API for `mcp add` (Agent 04).

Reference spec: `packages/opencode/src/mcp/index.ts`, `packages/opencode/src/cli/cmd/mcp.ts`,
`packages/opencode/src/server/routes/instance/httpapi/groups/mcp.ts`,
`packages/opencode/src/server/routes/instance/httpapi/handlers/mcp.ts`, SDK `@modelcontextprotocol/sdk@1.29.0` + its patch.

---

## 3. Protocol-version negotiation fix (PROTO-004, PROTO-002)

1. `types.rs`:
   ```rust
   pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
   pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
       &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
   ```
   `client.rs:104` and `:126` already read `LATEST_PROTOCOL_VERSION`, so advertise + `mcp-protocol-version`
   header update automatically. Keep `2025-06-18` in the supported set (oracle accepts it — verified).
2. `clientInfo.version`: add `McpOptions.client_version: Option<String>` (default `oc_mcp::version()`);
   `Mcp::with_options` uses it for `Implementation`. `oc-cli` passes `oc_cli::version::INSTALLATION_VERSION`
   (`1.18.13`, `crates/oc-cli/src/version.rs:5`). Wire assertion test in `tests/stdio.rs`.
3. Regenerate stale goldens (they encode `2025-06-18` + `0.1.0`): `jsonrpc.rs:199`, `types.rs:463-469`,
   `tests/stdio.rs:265`. `tests/stdio.rs:57` mock **stays** echoing `2025-06-18` — it becomes the
   backward-compat negotiation case.
4. Session recovery (SDK patch parity): on `404` with a stored `mcp-session-id`
   (`http.rs:272-276`), re-run `initialize`/`notifications/initialized` single-flight instead of only
   clearing the id.

---

## 4. Runtime wiring (launching stdio servers; consumed by Agent 02)

The executable reaches MCP when the composition root constructs `oc_mcp::Mcp`:

- `Mcp::with_options(config, directory, opts)` with `config` converted from the merged
  `oc_config` `Info.mcp` (skip `Value::Enabled`), `opts.auth = McpAuth::default()`,
  `opts.events` = forward `McpEvent::ToolsChanged` into the oc-core/server event bus so sessions refresh tools.
- `Mcp::init()` (already implemented, `index.rs:186-230`) spawns each enabled server in
  `tokio::spawn`, connecting local servers through `StdioTransport` (the child process) and remote
  servers through StreamableHTTP→SSE fallback. This is the "executable launches stdio servers" path.
- Agent 02 consumes `Mcp::tools() -> IndexMap<String, McpTool>`: `catalog::convert_input_schema`
  (`catalog.rs:124`) → oc-tool input schema; `McpTool` wraps `client.call_tool(name, args, timeout)`
  and maps `CallToolResult.content`/`structuredContent` → tool output parts. `Mcp::instructions()`
  feeds the MCP section of the system prompt (reference `session/system.ts:112-126`). MCP tool calls
  MUST go through Agent 08's permission gate like any tool (SEC-002).
- Server surface (with Agent 10): `AppState` gains `mcp: Arc<Mcp>`; implement the already-routed
  handlers `instance_handlers.rs:950-1010` over the service, matching `groups/mcp.ts`:
  `GET /mcp` status map, `POST /mcp` add, `POST /mcp/:name/auth` → `{authorizationUrl, oauthState}`,
  `POST /mcp/:name/auth/callback` + `/authenticate` → Status, `DELETE` auth → `{success:true}`,
  `connect`/`disconnect` → bool.

---

## 5. CLI service wiring (`opencode mcp`, PROTO-001/03)

Implement `oc-cli/src/cli/cmd/mcp.rs` over the in-process `Mcp` (no server required, matching the
reference which constructs `MCP.Service` directly):

- `mcp add` — mirror `McpAddCommand`: validate name/url/command/env/header (already ported, `mcp.rs:20-50`),
  build `ConfigMCPV1.Info`, write into the **global** `opencode.json` preserving comments. Requires
  Agent 04's jsonc edit API (`modify`/`applyEdits` parity); if it lags, ship `add --url`/`--` non-interactive
  paths with a plain JSON merge and gate the interactive path on the edit API.
- `mcp list` — `Mcp::status()` + `has_stored_tokens()`; render `✓ connected` / `○ disabled` /
  `⚠ needs authentication` / `✗ failed <error>` / `✗ needs client registration <error>` and the
  type/command/url hint line, plus `N server(s)` outro (reference `mcp.ts:109-168`).
- `mcp auth [name]` / `auth list` — `get_auth_status()` (✓/⚠/✗ + text); `authenticate(name)` opens
  the browser and waits for the callback (full flow exists in `index.rs:510-579`).
- `mcp logout [name]` — `McpAuth::all()` then `remove_auth(name)`.
- `mcp debug <name>` — render config URL, auth status, token/expiry/clientId (masked), then the
  probe: POST `initialize` with `LATEST_PROTOCOL_VERSION` + `InstallationVersion`, surface status /
  `WWW-Authenticate` / OAuth discovery (reference `mcp.ts:659-839`).
- Shared converter + constructor in `oc-cli/src/cli/mcp_util.rs` (`From<oc_config::v1::mcp::Info>`
  for `oc_mcp::config::Info`; `McpOptions { auth, client_version, events }`).

---

## 6. Bounds and lifecycle fixes

1. **SseParser cap** (`transport/mod.rs`): add `MAX_SSE_BUFFER_BYTES` (1 MiB, matching oc-client's SSE cap).
   If `feed` would exceed the cap without finding a boundary, clear the buffer and return a transport
   error (→ reconnect), never grow unbounded. Also cap a single event's `data` size.
2. **Bounded channels**: `MessageReceiver = mpsc::Receiver<Message>` (capacity e.g. 1024). `start()` in
   all three transports constructs `mpsc::channel(CAP)`; the read loops
   (`http.rs:339-360,427-453`, `sse.rs:271-307,350-376`, `stdio.rs:91-117`) use `tx.send().await` for
   real backpressure. `client.rs:137` read loop is unchanged (already `recv().await`). Do **not** switch
   to `try_send`+drop — that would silently lose requests.
3. **Reconnect backoff** (`http.rs:195-232`, `sse.rs:123-158`): exponential backoff (e.g. base 1 s, ×2,
   cap 30 s) with ±25% random jitter. Keep infinite retries (reference/SDK behavior — parity) but bounded
   in frequency; reset the backoff after a successful reconnect. Optionally surface `Status::Failed` after
   a long consecutive-outage threshold.
4. **Child lifecycle** (`stdio.rs`): set `kill_on_drop(true)` on the child and add a `Drop` impl on
   `StdioTransport` that kills/waits the child if `close()` wasn't called (closes ASYNC-013). `close()`
   already kills+reaps — keep.
5. **Request-id tolerance** (PROTO-009, optional): keep `Number(u64)` for outbound, but add a
   `#[serde(untagged)]` fallback so `id:null`/numeric-string parse-error responses do not kill the whole
   `Message` parse; surface them as a dropped/ignored line rather than a transport error.

---

## 7. Test list

`oc-mcp` (this agent):
1. **Version negotiation**: mock echoes `2025-11-25` → connected; echoes `2025-06-18` → connected
   (backward compat); echoes `2025-09-01` → rejected with `Server's protocol version is not supported`.
2. **Wire goldens** (`tests/stdio.rs`, `jsonrpc.rs`): update to `protocolVersion:"2025-11-25"`,
   `clientInfo.version` = injected install version (1.18.13).
3. **Malformed server bounded**: mock streams endless SSE `data:` with no `\n\n` → client memory stays
   bounded (SseParser cap), channel does not grow, connection fails/recovers gracefully; and a server
   flooding valid messages is backpressured (bounded channel) without OOM.
4. **Child termination on drop**: spawn stdio server; drop client/transport without `close()` → child pid
   is gone (`kill(pid,0)` fails); `close()` still reaps. Covers `kill_on_drop` + `Drop`.
5. **Reconnect backoff**: unreachable remote → observed reconnect delays grow (capped, jittered), no 1 Hz
   storm; success resets backoff.
6. **Session expiry**: HTTP server returns 404 with session id → client re-runs `initialize` and completes
   `tools/list` (SDK patch parity).

Integration / E2E (with deps):
7. **Binary round-trip** (Agent 18 TEST-001 harness): `opencode mcp list` against the audit mock
   (`rust-port-audit/artifacts/08-mcp-server.py`) → `✓ audit connected`, wire log shows
   `protocolVersion 2025-11-25` (differential vs oracle).
8. **Runtime wiring** (Agent 02): `opencode run`/TUI with a configured stdio server → child spawned,
   `tools/list` + `tools/call` results reach a session; `tools.list_changed` refreshes the registry.

---

## 8. Dependencies on other agents

- **Agent 02** (runtime composition): constructs `Mcp` in the composition root with merged config +
  directory + event bus; consumes `McpTool`/`instructions()` in the tool registry and system prompt;
  triggers `Mcp::init()`. Blocks test 8.
- **Agent 04** (config): `load_instance_state` merged config; jsonc comment-preserving `modify`/`applyEdits`
  for `mcp add` writes. Blocks full `add` parity.
- **Agent 08** (permission): MCP tool calls must traverse the SEC-002 allow/ask/deny gate with other tools.
- **Agent 10** (server): real `serve` HTTP + `AppState` composition with `mcp: Arc<Mcp>`; makes the
  `instance_handlers.rs` mcp_* handlers reachable (handlers themselves are written here).
- **Agent 18** (TEST-001): binary differential `mcp list` harness. Optional: Agent 09 for shared
  `convert_input_schema` tool-schema helpers.

Contract handshake: the `AppState.mcp` field + `McpTool` adapter shape are the two interfaces to agree
with Agents 10 and 02 before Wave 3 composition.

---

## 9. Risks

1. **Bounded-channel refactor**: touches all three transports + the `MessageReceiver` alias. Must land
   as one change with the backpressure tests; using `try_send`+drop would regress correctness.
2. **Version change breaks existing goldens/mocks** — expected; update `tests/stdio.rs`, `jsonrpc.rs`,
   `types.rs` in the same commit. Keep the `2025-06-18`-echoing mock as a compat case.
3. **Reconnect backoff vs reference parity**: reference/SDK retries indefinitely on a fixed delay; capping
   + jitter is a deliberate hardening divergence (SEC/ASYNC), must not turn into giving up permanently on
   transient errors. Document in code.
4. **`mcp add` config writes**: jsonc-preserving edit API depends on Agent 04; a naive JSON rewrite drops
   comments and diverges from reference. Gate interactive add on the edit API; ship non-interactive first.
5. **`opencode` as an MCP server command**: reference examples use `opencode x @modelcontextprotocol/...`;
   the Rust binary has no `x`/MCP-server mode, so `BUN_BE_BUN` env is set but inert. Arbitrary stdio
   servers work; document the gap for `mcp add` examples.
6. **Session-expiry re-initialize** can race in-flight requests on the HTTP transport; re-run single-flight
   and only on 404-with-session-id (SDK patch semantics).
7. **Runtime wiring is blocked** on Agent 02/10 composition; the CLI `mcp` commands are independently
   shippable (in-process `Mcp`), so de-risk by landing CLI before composition.

---

## 10. Merge-order recommendation (Wave 2/3)

1. **Wave 2 — oc-mcp crate-local, no deps**: version constants + fixture regen (PROTO-004/PROTO-001
   version part); `client_version` injection (PROTO-002); `SseParser` cap + bounded channels (SEC-005);
   `kill_on_drop` + `Drop` (ASYNC-013); reconnect backoff/jitter/cap (ASYNC-010); session-expiry
   re-initialize; request-id tolerance (PROTO-009). All verifiable by crate tests alone.
2. **Wave 2 — CLI surface**: `opencode mcp add/list/auth/logout/debug` over in-process `Mcp` (Agent 04
   for jsonc writes). First user-visible MCP path; no server needed.
3. **Wave 3 — composition**: oc-server `AppState.mcp` + mcp_* handlers (Agent 10), tool-registry
   consumption of `McpTool` + permission gate (Agent 02/08/09), and the binary differential `mcp list`
   test (Agent 18). This resolves the remaining PROTO-001 runtime parts together.

The oc-mcp hardening (Wave 2 #1) is safe to land before any wiring because it is crate-local with
mock-driven tests; the runtime composition is the single gated milestone.
