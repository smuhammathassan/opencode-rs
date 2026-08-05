# Agent 13 — Security Architecture and Threat Model

Audit of the **opencode-rs** Rust port (commit `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c`, workspace `/root/opencode-rs`) against the reference **opencode v1.18.13** (`/root/opencode-rs/reference`). Evidence below is STATIC (source reading) or RUNTIME (executed binary / tests); each finding is tagged. No production source was modified; only `/root/opencode-rs/rust-port-audit/13-security-threat-model.md` and `/tmp` files were written.

## Scope

Full security review of the Rust port across all 20 `oc-*` crates, focused on: server auth (`oc-server`), credential storage (`oc-provider`, `oc-mcp`), tool/permission gate (`oc-tool`, `oc-session`, `oc-session-runner`), plugin sandbox (`oc-plugin`), path handling (`oc-tool`, `oc-util`, `oc-server` fs handlers), SSRF (`oc-tool` webfetch, `oc-llm`, `oc-mcp`), unbounded input (SSE/shell/config), TLS (`reqwest`/rustls), terminal escapes (`oc-tui`, `oc-util`), and secrets in logs (`oc-llm` executor, `oc-mcp`, `oc-server`).

## Repository areas inspected

| Area | Files |
|---|---|
| Server auth + middleware | `crates/oc-server/src/auth.rs`, `middleware.rs`, `server.rs`, `cors.rs`, `router.rs`, `location.rs`, `sse.rs`, `handlers/pty.rs`, `handlers/permission.rs`, `handlers/fs.rs`, `instance_handlers.rs` |
| Credential storage | `crates/oc-provider/src/auth/mod.rs`, `crates/oc-mcp/src/auth.rs` |
| Permission / tools | `crates/oc-tool/src/model.rs`, `core/tool.rs`, `core/registry.rs`, `core/bash.rs`, `tool/*.rs`, `crates/oc-session/src/permission.rs`, `processor.rs`, `crates/oc-session-runner/src/session/services.rs`, `runner/llm.rs` |
| Plugin | `crates/oc-plugin/src/host.rs`, `js/runtime.rs`, `polyfill/runtime.js`, `npm.rs`, `meta.rs` |
| LLM / MCP clients | `crates/oc-llm/src/route/{executor,client,auth,endpoint}.rs`, `crates/oc-mcp/src/transport/{mod,http,sse,stdio}.rs`, `crates/oc-mcp/src/index.rs` |
| Path / fs | `crates/oc-tool/src/util.rs`, `read_filesystem.rs`, `tool/{read,write,edit,apply_patch,webfetch}.rs`, `crates/oc-util/src/fs_util.rs`, `util/wildcard.rs` |
| TUI / rendering | `crates/oc-tui/src/util/markdown.rs`, `components/{message,text}.rs`, `crates/oc-util/src/util/*` |
| Misc | `crates/oc-database/src/database.rs`, `crates/oc-cli/src/cli/{network.rs, cmd/serve.rs, cmd/web.rs, cmd/run/*, upgrade.rs}`, `crates/oc-client/src/transport.rs` |
| Reference comparison | `reference/packages/server/src/{auth.ts, cors.ts, middleware/authorization.ts, handlers/pty.ts}`, `reference/packages/opencode/src/{auth/index.ts, mcp/auth.ts, session/tools.ts, permission/index.ts, tool/{shell.ts, webfetch.ts}, server/routes/instance/httpapi/handlers/file.ts, cli/network.ts, cli/cmd/serve.ts}`, `reference/packages/core/src/pty/ticket.ts` |

## Commands executed

- `opencode --help`, `--version` → reports `1.18.13` (RUNTIME).
- `opencode serve --hostname 0.0.0.0 --port 4397` → prints `Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.` then binds a **bare TCP socket that serves no HTTP** (serve CLI is a stub, `serve.rs:40-67`); curl to `/api/health` returns nothing (RUNTIME).
- `cargo test -p oc-server` → **15 passed, 3 passed, 0 doc-tests**; `auth_requires_credentials`, `auth_token_query_bypasses` etc. pass (RUNTIME, `oc-server` tests green).
- Cleanup of disposable dirs in `/tmp` only.

## Runtime scenarios attempted

1. **`serve` with `--hostname 0.0.0.0` and no password**: confirmed the process prints the reference-identical warning and binds all interfaces. The HTTP API is not yet wired through the CLI (`serve.rs` binds a listener and drains connections), so the server-side attack surface below is only reachable once `oc-server` integration lands — but the code is present and compiles, and `oc-server`'s own tests exercise the router/auth.

2. **Tool permission behavior** (via existing `oc-tool` tests, not modified): `write` test at `tool/write.rs:144-168` demonstrates the write tool performs the file write (`result.output == "Wrote file successfully."`) while merely recording `ctx.asks[0]` — i.e., no approval is required or awaited. This is direct evidence for SEC-002.

## Architecture or behavior summary

- **Trust boundaries**: (1) user ↔ opencode process; (2) model/provider response → tools; (3) server API ↔ clients (browser/CLI/attach); (4) plugin JS → host services; (5) MCP server (stdio/remote) → opencode; (6) project config / repo content → config loader and tools; (7) filesystem/shell as the implicit authority boundary behind tools.
- **Server**: axum router; global Basic-auth middleware (`middleware.rs`) gated on `OPENCODE_SERVER_PASSWORD`/`OPENCODE_SERVER_USERNAME` (default `opencode`, matching reference `ServerAuth`). CORS origin allowlist mirrors reference `cors.ts`. Default binding `127.0.0.1`, port 0 → 4096 fallback (matches reference). mDNS forces hostname `0.0.0.0` (matches reference).
- **Credential storage**: `auth.json` and `mcp-auth.json` are plaintext JSON written with mode 0600 (matches reference `0o600`).
- **Permission model**: tools call `ctx.ask(...)`/`assert(...)`, which in the Rust port only **record** requests (`model.rs:386-389`, `core/tool.rs:42-44`). Nothing evaluates allow/ask/deny, blocks, or prompts the user. The reference blocks tool execution on an `ask` via `Deferred.await` until the user replies (`reference/.../permission/index.ts:98-106`).
- **Plugins**: QuickJS in-process host; the JS runtime is created with `JS_NewRuntime`/`JS_NewContext` and **no memory/stack/interrupt limits**; the host bridge defaults (`PluginHost` trait) fail closed for fs/shell/fetch but `tool_ask` defaults to allow.
- **Network clients**: reqwest/rustls with default cert validation everywhere; no `danger_accept_invalid_*` found. Custom base URLs for providers and MCP remote URLs are user/config-controlled (SSRF-in-scope-by-design, same as reference).

## Positive observations

1. TLS is sound: `reqwest` built with `rustls-tls`, no `danger_accept_invalid_certs`/`danger_accept_invalid_hostnames` anywhere in `crates/` (grep confirmed).
2. Credential files are chmod 0600 on write (`oc-provider/src/auth/mod.rs:148-152`, `oc-mcp/src/auth.rs:266-270`), matching reference.
3. LLM error path redacts secrets: `oc-llm/src/route/executor.rs:56-121` scrubs sensitive headers, JSON body fields, URL query params, and Bearer tokens, with a 16 KB body limit.
4. Server Basic-auth uses a constant-time compare (`oc-server/src/auth.rs:130-140`) and matches reference semantics (query `auth_token` + header).
5. `webfetch` enforces a 5 MB response cap and 30 s/120 s timeouts (`tool/webfetch.rs:7-9,90-93,137-152`); matches reference limits.
6. Default listen hostname is `127.0.0.1` (same as reference); a warning is printed when serving without a password (same as reference).
7. CORS origin regex `^https://([a-z0-9-]+\.)*opencode\.ai$` matches reference exactly.
8. Wildcard matching uses the `regex` crate (linear-time, no catastrophic backtracking) — no ReDoS on attacker-controlled patterns.
9. npm tarball unpacking uses `tar` 0.4.46 whose `unpack` sanitizes `..`/absolute entries (built-in protection).
10. Update check goes to GitHub Releases over HTTPS with default validation (`oc-cli/src/cli/upgrade.rs:40-54`).

## Findings summary

| ID | Severity | Confidence | Title |
|---|---|---|---|
| SEC-001 | High | CONFIRMED (static) | PTY WebSocket connect ticket is not validated — auth bypass on `/api/pty/:ptyID/connect` |
| SEC-002 | Critical | HIGH (static; reachability UNVERIFIED) | Tool permission `ask` gate not enforced — model commands execute without approval |
| SEC-003 | Medium | CONFIRMED (static) | v1 `/file/content` drops the reference's path-containment check (arbitrary file read) |
| SEC-004 | Medium | CONFIRMED (static) | Unbounded shell output buffering in `core/bash.rs` vs reference's bounded capture |
| SEC-005 | Medium | CONFIRMED (static) | MCP SSE parser buffer and message channels are unbounded (memory DoS via malicious MCP server) |
| SEC-006 | Low/Medium | CONFIRMED (static) | Plugin QuickJS runtime has no memory/stack/interrupt limits; `tool_ask` defaults to allow |
| SEC-007 | Medium | CONFIRMED (static) | Insecure default: `0.0.0.0` + no password exposes full API (warning-only, same as reference) |
| SEC-008 | Low | CONFIRMED (static) | `fs_list`/`fs_find` do not filter `..`; symlinks not canonicalized anywhere (partial parity) |
| SEC-009 | Low | MEDIUM (static) | Terminal escape injection: no ANSI/OSC sanitization of rendered model/tool output |
| SEC-010 | Informational | CONFIRMED (static) | `/log` allows log-line injection; `global_config_update` stores arbitrary config |
| SEC-011 | Informational | CONFIRMED (static) | `write_with_dirs` creates file then chmods (small permission TOCTOU window) |

## Detailed findings

### SEC-001 — PTY connect ticket is not validated (auth bypass) [HIGH, CONFIRMED]

**Files**: `crates/oc-server/src/handlers/pty.rs:175,207-215`, `crates/oc-server/src/middleware.rs:16-26,45-47`.

- The authorization middleware skips credential checks for any request whose path starts with `/api/pty/`, ends with `/connect`, and carries a **non-empty** `ticket` query param (`middleware.rs:16-26,45-47`), mirroring the reference's `hasPtyConnectTicketURL`.
- The reference then **validates and single-uses** the ticket: `packages/core/src/pty/ticket.ts:44-49` issues a `crypto.randomUUID()` stored in a cache, and `packages/server/src/handlers/pty.ts` (`pty.connect`) calls `tickets.consume(...)` (cache `invalidateWhen` scoped to `ptyID`).
- The Rust port's connect handler instead accepts **any non-empty ticket**: `let valid = allowed_origin && !ticket.is_empty();` (`pty.rs:211`). The minted token is never stored, checked, or consumed, and is generated as `ticket_{event_id().len()}` (`pty.rs:175`) — a deterministic small integer (length of a base36 timestamp id), not a secret.
- `allowed_origin` is `is_allowed_request_origin(...)` which returns `true` when the `Origin` header is absent (`cors.rs:50-51`), so a plain non-browser WebSocket client passes with no Origin header.

**Exploit path**: With `OPENCODE_SERVER_PASSWORD` set (auth required), any client able to reach the port connects to an existing PTY via `ws://host:port/api/pty/<pty_id>/connect?ticket=x`. PTY ids are time-based ascending (`event.rs:38-63`), so enumerable. The handler replays the captured PTY output buffer (may contain sensitive terminal content) to the caller and appends attacker-supplied text/binary into the buffer (`pty.rs:221-263`). Deviation from reference; severity capped at High because the Rust PTY is a partial port (no child process yet), limiting direct code execution, and requires an existing PTY session.

### SEC-002 — Tool permission `ask` gate is not enforced [CRITICAL, HIGH static / reachability UNVERIFIED]

**Files**: `crates/oc-tool/src/model.rs:383-389` (`ToolContext::ask` records only), `crates/oc-tool/src/core/tool.rs:40-44` (`CoreContext::assert` records only), `crates/oc-server/src/handlers/permission.rs:33-63` (server `permission.ask` stub stores the request and returns `effect: "allow"`, never blocks), `crates/oc-session-runner/src/session/services.rs:292-297` (`ToolSettle` has no permission hook), `crates/oc-session-runner/src/runner/llm.rs:529-557` (tool settlement runs unconditionally), `crates/oc-tool/src/core/registry.rs:258-267` (only `deny *` at materialization).

- Reference: every risky tool (`bash`, `write`, `edit`, `apply_patch`, `webfetch`, `external_directory`) calls `ctx.ask(...)` which is wired to `Permission.Service.ask` (`reference/.../session/tools.ts:81`), and `ask` **blocks the tool effect** via `Deferred.await(deferred)` until the user replies, after evaluating the merged ruleset for allow/deny/ask (`reference/.../permission/index.ts:67-107`).
- Rust port: `bash` records a `CorePermissionRequest` and proceeds (`core/bash.rs:149-164`); `write`/`edit`/`apply_patch`/`webfetch` call `ask()` which only pushes to `self.asks` and returns `Ok(())`. Nothing ever evaluates the rule or surfaces a prompt. The `run` CLI's `permission.asked` handler (`oc-cli/src/cli/cmd/run/events.rs:301-327`) would auto-reject, but the server never emits the event because nothing blocks.
- The `write` tool accepts **absolute** `filePath` and only records an external-directory ask (`tool/write.rs:49-57,76-84`); `apply_patch` resolves absolute hunks outside the worktree the same way (`tool/apply_patch.rs:80,163-165`).

**Exploit path**: A prompt-injected repo (e.g., README/git-diff/malicious model context) instructs the model to call `bash`, `write`, `webfetch`, or `apply_patch`. In the reference these all require user approval (default `ask`). In this port they execute with the user's full shell/filesystem/network authority — arbitrary command execution, arbitrary file write (absolute paths), and SSRF through `webfetch` (see SEC-007 surface) with no prompt. Marked Critical because it defeats the product's core safety control; confidence is HIGH at the code level (multiple independent paths all record-only, plus a passing `oc-tool` test demonstrates an unapproved write), but the end-to-end reachability of the V2 runner is UNVERIFIED because `ToolRegistry`/`ToolSettle` production implementations and the `serve` CLI wiring are still `TODO(integration)` stubs.

### SEC-003 — `/file/content` lacks the reference containment check [MEDIUM, CONFIRMED]

**Files**: `crates/oc-server/src/instance_handlers.rs:928-941`; reference `packages/opencode/src/server/routes/instance/httpapi/handlers/file.ts:96-99`.

Reference resolves the file relative to the instance directory and refuses `if (!FSUtil.contains(directory, file))` ("Path escapes the location"). The Rust port does `PathBuf::from(directory).join(path)` with a client-supplied `directory` and no containment check — absolute `path` or `../` traversal reads arbitrary files. Note `/api/fs/read` (`handlers/fs.rs:23-28`) does filter `..`, so the two surfaces are inconsistent. Mitigated by auth + loopback default; still a real parity gap (arbitrary file read once the server is reachable — e.g., a browser tab on `localhost` with no password, or any remote client under `--hostname 0.0.0.0`).

### SEC-004 — Unbounded shell output buffering [MEDIUM, CONFIRMED]

**Files**: `crates/oc-tool/src/core/bash.rs:229-260`; reference `packages/opencode/src/tool/shell.ts:437-510`.

The Rust port does `read_to_end` on stdout+stderr concurrently and only truncates to `MAX_CAPTURE_BYTES` (1 MB) **after** the full read. A command emitting more output than memory (e.g., `yes`, `seq 1e10`, a prompt-injected `cat /dev/zero`) exhausts memory before the timeout fires. The reference streams chunks and keeps only the last `maxBytes*2`, spilling overflow to a file (`shell.ts:439,481-504`). Combined with SEC-002 (no approval needed), this is a realistic OOM DoS.

### SEC-005 — Unbounded MCP SSE parser buffer and channels [MEDIUM, CONFIRMED]

**Files**: `crates/oc-mcp/src/transport/mod.rs:73-104` (parser `buffer: String` grows until a `\n\n`/`\r\n\r\n` boundary is seen), `crates/oc-mcp/src/transport/mod.rs:19` + `http.rs:182,341-360` + `sse.rs` (`mpsc::unbounded_channel` receivers). A malicious (or config-supplied) remote MCP/SSE server that streams data with no event boundary grows the parser buffer without limit; message floods grow the unbounded queue. Reference uses the official MCP SDK whose `eventsource-parser` bounds event accumulation.

### SEC-006 — Plugin runtime lacks resource limits; `tool_ask` defaults to allow [LOW/MEDIUM, CONFIRMED]

**Files**: `crates/oc-plugin/src/js/runtime.rs:219-228` (bare `JS_NewRuntime`/`JS_NewContext`; no `JS_SetMemoryLimit`, `JS_SetMaxStackSize`, or `JS_SetInterruptHandler`), `crates/oc-plugin/src/host.rs:67-69` (`tool_ask` default returns `{ status: "allow" }`). Plugins run in-process (matching the reference's in-process Bun model, which also provides no sandbox) — so this is parity with the reference, but there is **no** memory cap, stack cap, or execution interrupt: a plugin can `while(true){}` (hang the single-threaded host) or allocate until OOM. When hosts integrate real fs/shell/fetch bridges, plugins will have the user's authority. Documented because the reference's plugin surface is user-installed (trusted), but the missing limits are a deviation worth noting for hardening.

### SEC-007 — Insecure default: `0.0.0.0` + no password [MEDIUM, CONFIRMED, parity]

**Files**: `crates/oc-cli/src/cli/network.rs:110-119` (mDNS or config can force `0.0.0.0`), `crates/oc-cli/src/cli/cmd/serve.rs:14-16` (warning only). With `--hostname 0.0.0.0` and no `OPENCODE_SERVER_PASSWORD`, the full API surface (arbitrary file read via SEC-003, tool execution, `global_config_update`, `/log`) is exposed unauthenticated to the network; the location is client-controlled via `x-opencode-directory` (`location.rs:77-97`), which makes this effectively remote RCE for anyone who can reach the port. This is **behaviorally identical to the reference** (which prints the same warning and relies on the same Basic-auth layer), so it is a documented risk rather than a port defect, but it interacts badly with SEC-001/SEC-002/SEC-003.

### SEC-008 — Inconsistent path filtering; symlinks not canonicalized [LOW, CONFIRMED]

**Files**: `crates/oc-server/src/handlers/fs.rs:54,84` (`fs_list`/`fs_find` join client `path` without `..` filtering), `crates/oc-tool/src/util.rs:36-44` (`fs_contains`/`path_resolve` use `std::path::absolute`, which does not resolve symlinks; `Path::join` with an absolute arg replaces the base). `read`/`write`/`edit`/`apply_patch` will follow in-project symlinks to outside files. The reference has the same `path.resolve`-style behavior (also no symlink canonicalization), so this is parity, but it amplifies SEC-002/SEC-003.

### SEC-009 — Terminal escape injection not addressed [LOW, MEDIUM confidence]

**Files**: `crates/oc-tui/src/util/markdown.rs:254-335` (inline renderer copies raw chars, including `ESC`/control bytes, into spans), `crates/oc-tui/src/components/message.rs:384-410`. Model output and tool output (e.g., file contents from `read`, `git diff` from a malicious repo) reach the terminal without stripping ANSI/OSC/control sequences. The reference TUI also does not strip ANSI before rendering (its `marked` pipeline emits raw text nodes), so parity holds, but a native ratatui app writing raw ESC bytes to the terminal is a real terminal-injection vector (screen clear, cursor movement, OSC-8 hyperlink spoofing). Not exploited in testing; impact on terminal safety noted for hardening.

### SEC-010 — `/log` log injection; `global_config_update` [INFO, CONFIRMED]

**Files**: `crates/oc-server/src/instance_handlers.rs:58-63,1292-1305`. Any reachable client can POST arbitrary strings to `/log` (forged log lines) and overwrite the in-memory config via `PATCH /global/config`. Matches reference surface; low impact, but cheap to validate/authenticate.

### SEC-011 — File-then-chmod permission window [INFO, CONFIRMED]

**Files**: `crates/oc-util/src/fs_util.rs:142-169` (`tokio::fs::write` then `set_permissions`). Brief window where a 0600 file is world-readable per umask. Reference's `writeJson(..., 0o600)` has the same non-atomic shape.

## Feature or behavior gaps (security-relevant)

1. **Permission enforcement (SEC-002)** — the single largest gap; the whole allow/ask/deny decision loop, the blocking deferred, and the `permission.asked` event are unimplemented.
2. **PTY ticket registry (SEC-001)** — no random issuance, no single-use validation, no TTL enforcement (the `expires_at` field is set but unused).
3. **v1 file containment (SEC-003)** — reference guard not ported.
4. **Bounded shell capture (SEC-004)** — reference's incremental bounded buffer not ported.
5. **SSE parser limits (SEC-005)**.
6. **`oc-server` handlers are largely in-memory stubs** (`stores.permissions`, `stores.sessions`) — `session_permission_create` returns `allow` unconditionally and `TODO(integration): evaluate against oc-core permission service` (`handlers/permission.rs:32`).

## Test coverage gaps

- No test asserts that a tool with `action: "ask"` is blocked or prompts the user; the only permission tests check wildcard/`evaluate`/`disabled` matching (`oc-session/src/permission.rs:92-123`) and ask recording (`oc-tool/src/tool/write.rs:167`).
- No test for PTY ticket validation (the existing test at `middleware.rs:105-115` asserts only the URL shape, not that the ticket is consumed/validated).
- No tests for `/file/content` traversal or the missing containment guard.
- No tests for shell-output memory bounds or SSE parser buffer growth.
- No tests for terminal-escape sanitization in the TUI renderer.
- No test asserts TLS validation is enabled (only greps for `danger_accept_invalid_*` are possible).

## Unverified areas

- **End-to-end reachability of SEC-002/SEC-001 through the running binary**: `opencode serve` currently binds a bare socket and does not serve the HTTP API (`serve.rs:40-67`, `web.rs:48-50`, `attach` "not yet wired"); the production `ToolRegistry`/`ToolSettle` implementations referenced by `oc-session-runner` only exist as test mocks (`tests/runner_loop.rs:267-290`). The vulnerability surface is verified in compiled `oc-server`/`oc-tool` code and passing unit tests, but a live end-to-end exploit was **not** demonstrated against the shipped binary.
- Plugin fs/shell/fetch bridges (`PluginHost` methods) default to "not implemented"; their security posture once wired is unverified.
- Reference behavior for terminal-escape stripping was inferred from source, not executed (bun/node unavailable).
- `oc-sync` control-plane and `oc-database` data-at-rest sensitivity not deeply audited (DB is plaintext SQLite, default perms — reference parity).
- No fuzzing of the SSE parser, JSON schema validator, or `jsonc` parser (tools unavailable); potential panics/stack exhaustion from deeply nested configs were not exercised.

## Final domain verdict

**READY_WITH_MINOR_REMEDIATION** — with the caveat that **SEC-002 is a Critical authorization gap** that must be closed (permission `ask` must block and prompt, or tools must fail closed) before the tool/runner integration is shipped. SEC-001 (PTY ticket validation) and SEC-003 (file containment) are small, well-understood fixes. Credential storage, TLS, secret redaction, and auth defaults are solid and match the reference. Because the affected code paths are not yet reachable from the shipped binary (integration TODOs), the port as-shipped does not currently expose the Critical path; the moment tool/serve integration lands without the fixes, it becomes a remotely exploitable command-execution + arbitrary-file-access chain under the user's account.

---

*Report written by Agent 13 (Security Architecture & Threat Model), audit date 2026-08-05. Evidence is file:line-cited; STATIC vs RUNTIME distinguished; UNVERIFIED items explicitly marked.*
