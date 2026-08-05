# Plan 18 — Binary E2E, Differential Testing, Fixture Provenance, Coverage

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the missing test tier that proves the shipped `opencode` binary works end-to-end — spawning the real executable under disposable HOME/XDG/data dirs against a scripted mock provider, differentially against the reference binary `/root/.opencode/bin/opencode`, with fixtures captured from the reference rather than hand-authored.

**Architecture:** A binary-level harness lives in `crates/oc-cli/tests/` (the only package where Cargo sets `CARGO_BIN_EXE_opencode`). A `tests/common/` module provides disposable-env isolation, an in-process scripted mock SSE provider, process spawn wrappers with typed builders (run/serve/acp/attach), JSON-event parsing, and differential helpers. Fixtures are recorded from the reference binary in record mode, committed as goldens, and replayed against the Rust binary. Every Critical/High finding gets a failing-before/passing-after test; mutation-targeted tests cover the 10 high-risk paths.

**Tech Stack:** Rust 1.97 / edition 2021; tokio (dev-dep), reqwest (dev-dep, for serve HTTP clients), futures, serde_json; no new runtime deps. PTY via `script -qec` (util-linux) or tmux for TUI tests; reference binary at `$OPENCODE_REF_BIN` (default `/root/.opencode/bin/opencode`).

## Global Constraints

- **READ-ONLY on production source until the owning agent's fix lands.** I write tests only; test files may reference `TODO(integration)` seams but must not be edited to make tests pass. A test is "passing" only when the *product* makes it pass.
- **Parity target:** opencode v1.18.13. Every new golden cites its reference source (`reference/...:line`) or records its capture provenance. No hand-authored fixture may claim reference provenance.
- **Offline-deterministic:** every test must run with no external network (mock provider on 127.0.0.1), fixed ports via port 0 + sentinel parsing, no fixed sleeps for readiness (poll a published signal — mirror `reference/packages/opencode/test/AGENTS.md` anti-pattern note), normalized timestamps/ids/uuid/ports.
- **No `#[ignore]` for skipped assertions.** Ignore only documented *known-failures* tagged `// KNOWN-FAIL <FINDING-ID>`, tracked by the release gate as `cargo test -- --ignored` reaching 0 before release.
- **No process-global env mutation** (`std::env::set_var`/`remove_var`) inside a parallel test binary (see TEST-014). Isolation is passed via `Command.env_*` per spawn.
- **Every scenario requires the owning fix's finding to be CLOSED** before it flips from KNOWN-FAIL to enabled.
- Verify: `cargo test -p oc-cli` and `cargo test -p oc-cli -- --ignored`.

---

## 1. Owned findings (consolidated, Agent 18)

| ID | Severity | Owned? | State to fix |
|----|----------|--------|--------------|
| TEST-001 | High (blocker) | **YES** | No test invokes the real executable. Build the binary harness + differential suite (this plan). |
| TEST-003 | High | **YES** | `crates/oc-tool/tests/golden.rs:151-171` `task_schema_matches_reference_snapshot` asserts a schema WITHOUT `background`, contradicting `reference/.../test/tool/__snapshots__/parameters.test.ts.snap:300-336`. Capture the true wire schema from the reference runtime def (flag on/off) and rename truthfully. |
| TEST-004 | High | **YES** | `crates/oc-llm/tests/golden.rs` bodies are hand-written; `openai_chat_body` uses `"Say hello."` vs cassette `"Say hello in one short sentence."`. Load the actual cassette file and replay the recorded request/response. |
| TEST-002 | High (blocker) | NO (Agent 01) | Session crates test local mirrors. E2E `run`/session tests depend on Agent 01 promoting canonical types so the binary path is real. |
| TEST-005 | High | Partial | oc-tui `app.rs` untested. I drive the app through a real client + pty E2E; in-crate unit tests of the app loop are written here but land after Agent 16 wires TUI. |
| TEST-006..010, TEST-011, TEST-012..014 | Medium/Low | Partial | Mutation targets (see §6) — tests owned here, land in the owning crates' test dirs after those fixes. |

## 2. Harness layout

Decision: **`crates/oc-cli/tests/`** is the home for all binary E2E. Rationale: `CARGO_BIN_EXE_opencode` is only set for integration tests of the package that defines `[[bin]] name = "opencode"` (`crates/oc-cli/Cargo.toml:8`). A workspace-root `tests/` dir is impossible (root `Cargo.toml` is a virtual manifest), and a separate `oc-e2e` crate would have to guess the binary path. oc-cli currently has **no** `tests/` dir and **no** dev-dependencies — both are added by this plan.

```
crates/oc-cli/
  tests/
    common/
      mod.rs            # re-exports; harness module
      env.rs            # isolatedEnv() → Command env builder (exact reference env parity)
      fixture.rs        # disposable tmpdir w/ git + opencode.json + cleanup (mirror fixture/fixture.ts)
      mock_provider.rs  # in-process scripted OpenAI-compatible SSE provider (see §3)
      spawn.rs          # OpenCodeCmd: run/serve/acp/attach/spawn builders + RunResult/ServeHandle/AcpHandle
      json_events.rs    # parse --format json output into typed events
      diff.rs           # differential runner: record/replay + normalize
      provenance.rs     # fixture provenance reader/writer + enforce-no-hand-authored check
    smoke.rs            # read-only commands (mirror reference test/cli/smokes/read-only.test.ts)
    run.rs              # `run` E2E scenarios (Group B)
    serve.rs            # serve HTTP/SSE/WS E2E (Group C)
    session.rs          # session persistence/export/import/stats (Group D)
    tools_permission.rs # tool execution + allow/ask/deny + containment (Group E)
    plugins.rs          # plugin load/event/tool/permission/limits (Group F)
    mcp.rs              # MCP stdio + HTTP/SSE (Group G)
    acp.rs              # ACP ndjson JSON-RPC (Group H)
    tui.rs              # TUI via PTY (Group I)
    attach.rs           # attach E2E (Group J)
    interrupt.rs        # SIGINT/SIGTERM/cancellation (Group K)
    crash_restart.rs    # kill -9 + restart recovery (Group L)
    differential.rs     # record/replay differential suite (Group M; gated on OPENCODE_REF_BIN)
    known_fails.rs      # failing-before regression registry: every Critical/High finding (see §5)
  Cargo.toml            # += [dev-dependencies] tokio(full), reqwest(json,rustls), futures, serde_json, tempfile
```

Shared `common/` module conventions follow the repo's existing pattern (`crates/oc-project/tests/common/mod.rs`). Every test file declares `mod common;`.

### 2.1 env.rs — disposable environment

Exact parity with the reference harness `isolatedEnv()` (`reference/packages/opencode/test/lib/cli-process.ts:62-77`), which the Rust port already honors (verified: `crates/oc-config/src/load.rs:30-54`, `crates/oc-util/src/global.rs:7-10`, `crates/oc-provider/src/auth/mod.rs:121`, `crates/oc-cli/src/cli/upgrade.rs:60`).

```rust
pub fn isolated_env(home: &Path, config_json: &str) -> Vec<(&'static str, std::ffi::OsString)> {
    vec![
        ("OPENCODE_TEST_HOME", home.as_os_str().to_owned()),
        ("HOME", home.as_os_str().to_owned()),
        ("XDG_CONFIG_HOME", home.join(".config").into_os_string()),
        ("XDG_DATA_HOME", home.join(".local/share").into_os_string()),
        ("XDG_STATE_HOME", home.join(".local/state").into_os_string()),
        ("XDG_CACHE_HOME", home.join(".cache").into_os_string()),
        ("OPENCODE_CONFIG_CONTENT", config_json.to_owned().into()),
        ("OPENCODE_DISABLE_PROJECT_CONFIG", "1".into()),
        ("OPENCODE_PURE", "1".into()),
        ("OPENCODE_DISABLE_AUTOUPDATE", "1".into()),
        ("OPENCODE_DISABLE_AUTOCOMPACT", "1".into()),
        ("OPENCODE_DISABLE_MODELS_FETCH", "1".into()),
        ("OPENCODE_AUTH_CONTENT", "{}".into()),
    ]
}
```

Used via `Command::envs(...)` only — never `std::env::set_var` (TEST-014).

### 2.2 fixture.rs — disposable tmpdir

```rust
pub struct TmpDir { pub path: PathBuf, _cleanup: tempfile::TempDir }
pub fn tmpdir() -> TmpDir;                       // tempfile::TempDir::new() + realpath
pub fn tmpdir_with_config(model: &str, base_url: &str) -> TmpDir; // writes opencode.json + returns
pub fn git_init(dir: &Path) -> ();               // git init + author envs (GIT_AUTHOR_*)
```

Mirrors `reference/packages/opencode/test/fixture/fixture.ts`. Guarantees removal on drop; pid-keyed dirs are avoided (TEST-013).

### 2.3 spawn.rs — typed builders

```rust
pub struct RunResult { pub exit: i32, pub stdout: String, pub stderr: String, pub duration_ms: u128 }
pub struct RunHandle  { pub interrupt: (), pub result: RunResult }
pub struct ServeHandle { pub url: String, pub port: u16, pub kill: (), pub exited: i32 }
pub struct AcpHandle  { pub send: &dyn Fn(&serde_json::Value), pub receive: ..., pub close: (), pub exited: i32 }

pub struct OpenCodeCmd { bin: &'static str /* env!("CARGO_BIN_EXE_opencode") */ }
impl OpenCodeCmd {
    pub fn spawn(&self, args: &[&str], env: &[(&str, OsString)], cwd: &Path) -> RunResult;
    pub fn run(&self, msg: &str, opts: &RunOpts) -> RunResult;           // argv parity w/ reference runArgs()
    pub fn start_run(&self, msg: &str, opts: &RunOpts) -> RunHandle;     // SIGINT-able
    pub fn serve(&self, opts: &ServeOpts) -> ServeHandle;                // parses "listening on http://" sentinel
    pub fn acp(&self, opts: &AcpOpts) -> AcpHandle;                      // ndjson framing, buffered receive queue
    pub fn expect_exit(&self, r: &RunResult, expected: i32, label: &str); // dumps stderr/stdout on mismatch
}
```

All long-lived children are `kill()`ed in a drop guard or test-scope finalizer (mirrors `serve-process.test.ts` "kills the subprocess on scope close" — a regression in the kill path must fail the suite). Ports are always `--port 0` and the real port parsed off stdout (reference AGENTS.md: no hard-coded ports).

### 2.4 mock_provider.rs — scripted SSE provider

Replaces the audit's `rust-port-audit/artifacts/09-mock-provider.py` with an in-process Rust server so no `python3` is needed at test time. Modeled on `reference/packages/opencode/test/lib/llm-server.ts` (queue of scripted items) plus the mock's request-log.

```rust
pub struct MockProvider { addr: SocketAddr, hits: Arc<Mutex<Vec<MockHit>>>, script: Arc<Mutex<VecDeque<Scripted>>> }
pub struct MockHit { pub method: String, pub path: String, pub auth: Option<String>, pub body: serde_json::Value }
pub enum Scripted { Text(&'static str), ToolCall(&'static str, serde_json::Value), Usage{..},
                    HttpError(u16, serde_json::Value), GoLimit{workspace:&str, limit_name:&str, retry_after:f64},
                    Hang, #[allow(dead_code)] Raw(&'static str) }
impl MockProvider {
    pub async fn start() -> Self;                       // binds 127.0.0.1:0, serves POST /v1/chat/completions SSE
    pub fn url(&self) -> String;                        // http://127.0.0.1:PORT/v1
    pub fn hits(&self) -> Vec<MockHit>;                 // request log (assert body/auth reached provider)
    pub fn push(&self, items: impl IntoIterator<Item = Scripted>); // next scripted response for the next request
}
pub fn test_provider_config(llm_url: &str) -> serde_json::Value; // reference test-provider.ts equivalent
```

Server loop: read `Content-Length`, log `{path, auth, body}` to `hits`, pop one `Scripted`, emit OpenAI-chat SSE chunks + `data: [DONE]`, honoring the reference chunk shapes used by `oc-llm`'s parser tests. A `GoLimit` scripted item emits the `{"error":{"type":"GoUsageLimitError","metadata":{"workspace":"w","limitName":"L"}}}` body plus `retry-after` header needed to exercise `crates/oc-session-runner/src/retry.rs:207-265`.

## 3. Differential harness design

Goal (TEST-001): the same argv, the same isolated env, the same mock provider — run against `/root/.opencode/bin/opencode` (reference) and `CARGO_BIN_EXE_opencode` (Rust) — and assert parity. Works because **both binaries honor the identical env surface** (`OPENCODE_CONFIG_CONTENT`, `OPENCODE_TEST_HOME`, `OPENCODE_DISABLE_*`, `OPENCODE_AUTH_CONTENT`).

- **Two modes:**
  - **Record mode** (`OPENCODE_RECORD_FIXTURES=1`): run the **reference** binary only, capture {exit, stdout, stderr, parsed JSON events, `opencode.db` snapshot, session JSON} into `crates/oc-<area>/tests/fixtures/<name>/` with a `.provenance.json` sidecar (see §4). Recorded artifacts become Tier-A fixtures.
  - **Replay mode** (default): run the **Rust** binary with the identical argv/env and assert its output equals the recorded fixture.
  - **Live differential** (`OPENCODE_RUN_DIFFERENTIAL=1`, requires `OPENCODE_REF_BIN`): run both live in one test and diff normalized outputs directly. This is the fastest guard for parity regressions and is used during development; replay mode is the CI-safe form (no reference binary needed on CI).
- **Normalization (`common/diff.rs`):** strip/adjust — timestamps (`created`, `recordedAt`, `expires_at`), session/message IDs, ports, absolute paths under the tmpdir home, `durationMs`, usage token counts where provider-driven, UUIDs. Same rule-set as the reference snapshot serializers. Every fixture's `.provenance.json` lists which normalizations were applied.
- **Event stream comparison:** `--format json` stdout is parsed per-line (`common/json_events.rs`, mirrors `parseJsonEvents` at `cli-process.ts:483-489`); compare event `type` sequence and payloads with normalized fields. Assert order matters for protocol parity.
- **DB artifact comparison:** after the same command sequence, diff `opencode.db` via the reference schema `reference/packages/core/src/database/schema.gen.ts` (byte-match already proven by oc-database goldens, TEST positive #2) — rows of `session`/`session_message`/`part` with normalized ids.
- **Gating:** differential tests use `#[ignore = "requires OPENCODE_REF_BIN"]` only as a *skip-if-unavailable*, and each begins with `let ref_bin = std::env::var("OPENCODE_REF_BIN").ok(); if ref_bin.is_none() { return; }`. CI runs them with `OPENCODE_REF_BIN=/root/.opencode/bin/opencode`. This is the one allowed use of `#[ignore]` (environment-gated, not skipped-assertion).

## 4. Fixture-provenance policy

Replace the audit's three-tier ambiguity (18-testing-verification.md §"Golden fixtures") with four tiers:

| Tier | Source | Allowed | Rule |
|------|--------|---------|------|
| **A** | Byte-copied from reference source | Yes (already: schema.sql DDL, prompts, plugin examples) | Keep; verify a hash-sync check in CI |
| **B** | **Captured from the running reference binary** (record mode) | **Required for all new goldens** | `.provenance.json`: `{source:"reference-binary", version:"1.18.13", input:"argv+env hash", recordedAt, normalizations:[...]}` |
| **C** | Hand-transcribed from reference source with citation | Only when capture impossible (e.g. drizzle-kit) | `/// From reference/...:line` in the fixture header + review |
| **D** | Hand-written claims of reference provenance | **Forbidden for new fixtures** | — |

**Fix the two HIGH offenders:**
- **TEST-003 (oc-tool task golden):** the LLM-facing `jsonSchema` in the Rust runtime (`crates/oc-tool/src/tool/task.rs:74-78`) matches the reference *def* (`reference/packages/opencode/src/tool/task.ts:343-350`), but the reference *published* wire schema has `background` (`parameters.test.ts.snap:300-336`). Fix: assert **both** shapes — capture the runtime def from the reference binary record-mode AND assert the reference snapshot shape as a Tier-B fixture; rename the test to something truthful (e.g. `task_runtime_def_matches_reference_runtime_def`) so it no longer pins the wrong expectation.
- **TEST-004 (oc-llm goldens):** load the real cassette `reference/packages/llm/test/fixtures/recordings/openai-chat/streams-text.json`, extract its request `body`, and drive the identical input (`"Say hello in one short sentence."`). Assert exact request body AND replay the recorded SSE response through the stream parser. Add a provenance guard test that fails if the cassette's input prompt no longer matches the test's, so the fixture cannot silently drift.
- **TEST-011 (oc-sync DDL):** no reference migration is checked in; keep as Tier C but add a reconciliation test that fails if oc-sync's emitted DDL ever diverges structurally from the oc-database `workspace` fixture (`schema.sql`) FK shape — the two must agree on the logical table (named vs unnamed FK currently differ; `crates/oc-sync/src/sync/sql.rs:90-132` vs `crates/oc-database/tests/fixtures/schema.sql`).
- **Capture flow:** `OPENCODE_RECORD_FIXTURES=1 cargo test -p oc-cli --test differential -- --ignored` runs only reference-side captures, writes fixtures, and aborts on any secret-looking content (reuse the reference `http-recorder` redaction concept from `cassette.ts:69-70` — fail if an interaction body contains `Authorization`/`sk-`/`key=`). Committer reviews the diff (fixture provenance = reviewer-visible).
- **No hand-authored golden bodies** for new E2E assertions; a scripted mock-provider scenario that is exercised live by both binaries replaces hand-written SSE in all new tests.

## 5. E2E scenario list — 80 scenarios mapped to mission requirements

Mission requirements = release gates (`rust-port-audit/RELEASE-GATE.md`) + parity goals (CONTEXT.md). Status: **FB** = failing-before (KNOWN-FAIL now, enable when the owning finding closes); **P** = passes today; **D** = differential.

### A. Boot & CLI surface (7) — Build/CLI-parity gates, Agent 02 inventory
- E2E-001 `opencode --version` = `1.18.13`, byte-identical to reference (D). P
- E2E-002 `opencode --help` exit 0, all 24 commands present, matches reference surface. P
- E2E-003 `opencode run --help` exit 0. P
- E2E-004 `opencode` no-args non-TTY prints TUI-start line (contract). P
- E2E-005 unknown command → nonzero exit + usage on stderr (D). P
- E2E-006 `debug paths` resolves isolated HOME/XDG under tmpdir. P
- E2E-007 `providers list` exit 0, prints "Credentials" (mirror read-only.test.ts). P

### B. `run` E2E (12) — Provider-functionality + End-to-end gates, CLI-001, INTEGRATION-001
- E2E-008 `run "hi"` against mock provider → exit 0, assistant text event (FB: CLI-001). 
- E2E-009 `run --format json` emits parseable JSON event lines; event types match reference (FB: CLI-001, D).
- E2E-010 `run --model test/test-model` uses the configured model; mock provider records the hit (FB: CLI-001).
- E2E-011 `run --agent <name>` selects agent (FB: CLI-001).
- E2E-012 `run --print-logs` prints logs to stderr (FB: CLI-001).
- E2E-013 `run` reads prompt from stdin when non-TTY (FB: CLI-001).
- E2E-014 `run --continue` continues the previous session (FB: SESSION-001).
- E2E-015 `run` persists session+parts to `opencode.db` in isolated data dir (FB: DB-001).
- E2E-016 `run --mini` → documented not-wired error (contract, like reference README). P
- E2E-017 `run` usage/token accounting recorded in DB (FB: SESSION-001, DB-001).
- E2E-018 `run` with scripted tool-call → tool executes against workspace (FB: TOOLS-001).
- E2E-019 `run` full tool loop: tool-call → result → final text (FB: TOOLS-001, SEC-001).

### C. `serve` HTTP/SSE/WS (10) — Server-functionality gate, CLI-002, SERVER-01
- E2E-020 `serve --port 0` prints `listening on http://host:port`; /global/health 200 (FB: CLI-002). Currently the Rust serve binds a bare TCP socket (audit probe: HTTP 000).
- E2E-021 /api/health 200 (FB: CLI-002).
- E2E-022 `/global/event` SSE streams events to a connected client (FB: SERVER-01, D).
- E2E-023 POST /api/session/:id/prompt → assistant response via HTTP (FB: CLI-001/002).
- E2E-024 session message list + pagination over HTTP (FB: SESSION-001).
- E2E-025 `/api/pty/:ptyID/connect` WebSocket upgrade + binary relay (FB: SERVER-01; mutation §6.4).
- E2E-026 auth: /api without token → 401 + `WWW-Authenticate` (P — already unit-covered; add binary level).
- E2E-027 CORS preflight + `vary: origin` (P at unit; add binary level).
- E2E-028 serve killed on scope close; no orphan process (P once serve is real; mirrors serve-process.test.ts).
- E2E-029 `serve --port 0` + HTTP request with correct Server/API JSON shape vs reference (D).

### D. Session persistence (7) — Database-compat gate, DB-001, SESSION-001
- E2E-030 `session list` lists sessions created by `run` (FB: DB-001).
- E2E-031 `session list --json` emits schema-conformant array (FB: DB-001, D).
- E2E-032 `session get <id>` returns session detail (FB: DB-001).
- E2E-033 `session export` produces JSON matching reference export shape (FB: SESSION-001, D).
- E2E-034 `session import <file>` → appears in `session list` (FB: SESSION-001).
- E2E-035 `db path` prints `$XDG_DATA_HOME/opencode/opencode.db` under isolation (P).
- E2E-036 `stats` aggregates usage from DB (FB: DB-001).

### E. Tool execution & permission (10) — Tool-safety + Security gates, TOOLS-001..004, SEC-001/003
- E2E-037 `apply_patch` tool executes against workspace file (FB: TOOLS-001).
- E2E-038 `write`/`edit` tools execute (FB: TOOLS-001).
- E2E-039 `bash` tool executes; process-group kill on cancel (FB: TOOLS-001/004; mutation §6).
- E2E-040 `read` tool enforces byte cap / streaming (FB: TOOLS-002).
- E2E-041 `grep`/`glob` tools execute (FB: TOOLS-001).
- E2E-042 `permission: {bash: "allow"}` config → tool runs without prompt (FB: SEC-001).
- E2E-043 `permission: {bash: "deny"}` → tool blocked, zero side effects (FB: SEC-001).
- E2E-044 `permission: "ask"` → server round-trip, approve-once → tool runs (FB: SEC-001).
- E2E-045 `permission: "ask"` → deny → tool not run (FB: SEC-001).
- E2E-046 containment: tool writing outside workspace denied (FB: SEC-003).

### F. Plugins (6) — Plugin-isolation gate, PLUGIN-001..004
- E2E-047 plugin loads from config dir through QuickJS host (FB: PLUGIN-004).
- E2E-048 plugin event handler fires on message event (FB: PLUGIN-004).
- E2E-049 plugin-defined tool executes (FB: PLUGIN-004).
- E2E-050 plugin tool subject to permission gate (FB: SEC-001 + PLUGIN-004).
- E2E-051 plugin memory/runtime limit enforced (FB: PLUGIN-001).
- E2E-052 `plugin list` exit 0 (P — CLI surface).

### G. MCP (5) — Protocol-compat gate, PROTO-001
- E2E-053 `mcp list` exit 0, no servers configured (P — CLI surface; now wired? falls back FB if not).
- E2E-054 MCP stdio server (mock `08-mcp-server.py` pattern) connected + `mcp list` reports it (FB: PROTO-001).
- E2E-055 MCP tool invoked through the server during `run` (FB: PROTO-001, TOOLS-001).
- E2E-056 MCP HTTP/SSE remote server (FB: PROTO-001; mutation §6.9).
- E2E-057 MCP protocolVersion parity vs reference (D, FB: PROTO-001).

### H. ACP (4) — Protocol gate, PROTO-001
- E2E-058 `acp` initialize handshake over ndjson (D, FB: PROTO-001 — reference answers, Rust emits zero bytes today).
- E2E-059 `acp` new session via JSON-RPC (FB: PROTO-001).
- E2E-060 `acp` prompt → response over ACP (FB: PROTO-001, CLI-001).
- E2E-061 `acp` exits 0 on stdin EOF (FB: PROTO-001).

### I. TUI (6) — TUI-functionality gate, CLI-003, UX-001/002
- E2E-062 TUI launches under PTY, renders initial screen (FB: CLI-003).
- E2E-063 TUI keypress submits prompt → assistant reply rendered (FB: CLI-003, CLI-001).
- E2E-064 TUI `session.part.updated` event mutates visible state (FB: CLI-003).
- E2E-065 TUI resize does not crash (FB: CLI-003).
- E2E-066 TUI escape/non-UTF8 byte passthrough preserved (FB: UX-001, uses `19-tui-ux-portability/ratatui-escape-passthrough.txt` artifact).
- E2E-067 TUI exits cleanly on Ctrl-C (FB: CLI-003).

### J. attach (3) — Session-lifecycle gate
- E2E-068 `attach <session>` connects to a running serve session (FB: SESSION-001, CLI-002).
- E2E-069 attach streams events from the server (FB: SESSION-001).
- E2E-070 attach with invalid session id → clean error, nonzero exit (FB: SESSION-001).

### K. Cancellation & interruption (4) — Session-lifecycle/Async gates (mutation §6.2/6.3)
- E2E-071 SIGINT during `run` → graceful exit, interrupted tools failed (FB: TOOLS-001 + SESSION-001).
- E2E-072 SIGTERM kills `serve` cleanly (FB: CLI-002).
- E2E-073 cancel mid-turn (scripted Hang provider) → no orphaned tool state (FB: SESSION-001).
- E2E-074 `start_run` handle interrupt → RunResult non-zero + interruption event in JSON stream (FB: CLI-001).

### L. Crash & restart (4) — Session-lifecycle gate
- E2E-075 kill -9 mid-run → session marked interrupted on restart (FB: SESSION-001, DB-001).
- E2E-076 restart continues the session and re-prompts (FB: SESSION-001).
- E2E-077 partial DB write survives crash (journal integrity) (FB: DB-001).
- E2E-078 retry after crash: pending tools failed via `fail_interrupted_tools` (FB: SESSION-001; mutation §6.3).

### M. Differential parity (2) — CLI-parity gate
- E2E-079 same argv + env against both binaries → normalized stdout/exit parity across the Group A read-only commands (D).
- E2E-080 same `--format json` run against both binaries → event-type sequence parity (D, FB: CLI-001).

## 6. Mutation-targeted test list

Each row: mutation target → test placement → E2E scenario backstop. A mutation in any listed path must fail ≥1 test.

| # | Mutation target (source) | Unit/component test (written here, in owning crate's tests/) | E2E backstop |
|---|---|---|---|
| M1 | `retryable()` GoUsageLimitError branch `oc-session-runner/src/retry.rs:207-265` | Feed JSON `{"type":"GoUsageLimitError","metadata":{"workspace":"w","limitName":"L"}}` + `retry-after: 90000`; assert reset string "1 day 1 hour", Go link `https://opencode.ai/workspace/w/go`, `RetryReason::AccountRateLimit` | E2E-019 variant: scripted `GoLimit` → CLI prints upsell + retry |
| M2 | `delay()` RFC2822 branch `retry.rs:154` | Assert HTTP-date `retry-after` parses to the same delay as the ms form | E2E-019 with `retry-after: <date>` |
| M3 | `fail_interrupted_tools` / `TurnFailure::Interrupted` `runner/llm.rs:142,198,228,245` | Extend `runner_loop.rs`: cancel `CancellationToken` mid-turn; assert pending/running tools published failed + loop returns cleanly | E2E-071/073/078 |
| M4 | WebSocket upgrade + pty relay `oc-server/src/handlers/pty.rs:195-221` + `pty_connect_token` | axum WS client connects after ticket; wrong/missing ticket → 403; missing pty → 404 | E2E-025, E2E-026 |
| M5 | `skill` tool execute `oc-tool/src/tool/skill.rs` | Execute via tool def against a `OPENCODE_SKILL_*` skill | E2E-018/019 tool loop |
| M6 | `websearch` tool execute `websearch.rs` | Mock websearch provider returns results; assert tool output | E2E-018 |
| M7 | `question`/`lsp`/`grep`/`read` tool `def().execute` | Execute each def with a stub context asserting side effects | E2E-018/041 |
| M8 | `subtle_equal` `oc-server/src/auth.rs:130-140` | Correctness oracle for constant-time impl (unequal-length/equal); assert it uses the constant-time primitive (structural), not `==`; document timing limits | E2E-026 (auth enforced end-to-end) |
| M9 | HTTP transport SSE dispatch/redirects `oc-mcp/src/transport/http.rs:456-465` | Mock MCP HTTP server: happy SSE, error frame, redirect | E2E-056 |
| M10 | model-filter exclusions `oc-tool/tests/golden.rs:361-385` | Extend to all gpt-4*/gpt-oss* cases (currently only gpt-5 vs claude sampled) | — |
| M11 | oc-tui app loop `oc-tui/src/app.rs` | In-crate: enhanced `MockSdkClient` returning real sync/session data; assert keypress→screen transitions (fixes TEST-005) | E2E-063/064 |

Plus: `oc-cli/src/cli/cmd/*` command handlers get binary-level coverage via Groups A–M (currently ~0 mutation resistance).

## 7. Dependencies on all other agents (tests land after their fixes)

| Finding (fixing agent) | Gate for my scenarios |
|---|---|
| INTEGRATION-001, CLI-001 (AG-12) | E2E-008..019, 023, 030..036, 069..074, 080 |
| CLI-002 (AG-10) | E2E-020..025, 028, 029, 072 |
| CLI-003, UX-001/002 (AG-16) | E2E-062..067 |
| DB-001, SESSION-001 (AG-03, AG-12) | E2E-014, 015, 017, 030..036, 075..078 |
| SEC-001 (AG-08) | E2E-042..046, 050 |
| SEC-002/003, SERVER-01 (AG-10) | E2E-025, 026, 046, M4 |
| TOOLS-001..004 (AG-07/09) | E2E-018, 019, 037..041, M1..M3, M5..M7 |
| PLUGIN-001..004 (AG-15) | E2E-047..051, 052 |
| PROTO-001, SSE-002 (AG-13) | E2E-053..057, 058..061, M9 |
| TEST-002 (AG-01) | E2E-015/017 (real oc-schema types across binary boundary) |
| Fixture regen (AG-03/07/13) | TEST-003/004 fixes + all D scenarios |
| RUST-001..004 (AG-15), RUST-005.. (AG-14) | plugin crash-safety E2E (E2E-051) |

**Ordering rule:** I merge the harness scaffold and the PASS-today scenarios in Wave 1. Every FB scenario ships with its finding's test written and tagged `// KNOWN-FAIL <FINDING-ID>`, and flips to enabled in the same merge as the owning agent's fix. No FB test is enabled before its finding closes (gate: `FINDING-STATUS.csv` Status=CLOSED).

## 8. Risks

- **Reference binary unavailable on CI** → differential tests are env-gated (`OPENCODE_REF_BIN`), replay fixtures are the CI path; a CI job with the reference binary runs the full D set.
- **PTY/TUI flakiness** → E2E-062..067 run under `script -qec` (util-linux) with a readiness sentinel rendered by the TUI; if `script` is absent they skip with a logged reason. No fixed sleeps; poll for the sentinel.
- **Parallel interference** (TEST-014) → every spawn gets a fresh tmpdir + port 0; `set_var` is banned in test code.
- **Mock-provider determinism** → provider pushes a scripted response per request from a queue; a test that asserts `hits()` ordering requires exactly N requests and fails on N+1 (catches stray retries).
- **Normalization drift** → `.provenance.json` pins the normalization list; a fixture change without provenance update fails the provenance guard (Tier-D linter).
- **FB tests in `--ignored` could be forgotten** → the release gate adds `cargo test -p oc-cli -- --ignored` must reach 0; a `known_fails.rs` manifest asserts every KNOWN-FAIL has a still-OPEN finding, so a fixed-but-untagged finding fails loudly.
- **Test-suite runtime growth** → each scenario targets one behavior; Group A/B/C smoke reuse one spawn when possible; keep total binary-E2E wall time < ~15 min (timeboxed).

## 9. Merge-order recommendation

1. **Wave 1 (scaffold + PASS-today):** `crates/oc-cli/tests/common/*` + dev-deps + `smoke.rs` (E2E-001..007), `known_fails.rs` registry, `differential.rs` harness shell, fixture-provenance tooling + Tier-D linter, and the TEST-003/TEST-004 golden corrections (fixture capture from reference runtime/cassettes). This is the first merge that exercises `CARGO_BIN_EXE_opencode`.
2. **Wave 2 (after AG-12/10/03 wiring):** enable Group B/C/D scenarios + E2E-069..074, 080. Each lands in the same merge as the owning CLI/db fix.
3. **Wave 3 (after SEC-001, TOOLS-001..004):** Group E + M1..M3, M5..M10.
4. **Wave 4 (after PLUGIN-004, PROTO-001):** Group F/G/H + M9; Group L crash tests after SESSION-001.
5. **Wave 5 (after CLI-003/UX):** Group I TUI + M11; then final differential sweep (all D), `--ignored` → 0, fixture-provenance audit, wire into CI + RELEASE-GATE.md "End-to-end tests" gate.

## 10. Verification

- After each merge: `cargo test -p oc-cli` (enabled scenarios green) and `cargo test -p oc-cli -- --ignored` (only documented KNOWN-FAILs / env-gated D tests).
- Mutation checks (M1..M11) run as `cargo test -p oc-session-runner -p oc-server -p oc-tool -p oc-tui` as their owning fixes land.
- Release gate: RELEASE-GATE.md "End-to-end tests" flips FAIL→PASS only when `-- --ignored` reaches 0 with `OPENCODE_REF_BIN` set and all 80 scenarios green.
