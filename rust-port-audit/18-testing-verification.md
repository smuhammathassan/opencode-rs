# Agent 18 — Testing Strategy, Test Validity, Coverage, and Mutation Resistance

## Scope

Audit of the opencode-rs test suite: what is actually exercised, whether tests
call the real executable, fixture provenance vs the vendored reference, mock
validity, assertion meaningfulness, ignored tests, determinism/parallel
interference, network usage, cleanup reliability, test-name accuracy, and
mutation resistance of 10 high-risk functions. READ-ONLY: no production
source, test, or dependency changes were made.

## Repository areas inspected

- Full workspace test run (`cargo test --workspace`).
- All 40 integration test binaries under `crates/*/tests/` and all `#[test]`
  modules under `crates/*/src/` (40 files enumerated).
- Fixture provenance cross-checks against `reference/`:
  - `reference/packages/core/src/database/schema.gen.ts` vs
    `crates/oc-database/tests/fixtures/schema.sql`
  - `reference/packages/opencode/src/session/prompt/*.txt` vs
    `crates/oc-session/assets/prompt/*.txt`
  - `reference/packages/opencode/src/tool/*.txt` vs `crates/oc-tool/src/prompts/*.txt`
  - `reference/packages/opencode/test/tool/__snapshots__/parameters.test.ts.snap`
    vs `crates/oc-tool/tests/golden.rs`
  - `reference/packages/llm/test/fixtures/recordings/*` vs `crates/oc-llm/tests/*`
  - `reference/packages/plugin/src/example*.ts` vs `crates/oc-plugin/tests/fixtures/`
  - `reference/packages/client/src/contract.ts` vs `crates/oc-client/tests/contract.rs`
  - `reference/packages/opencode/src/tool/registry.ts` vs `crates/oc-tool/tests/golden.rs`
- High-risk code inspected for mutation resistance: oc-session-runner retry,
  oc-session-runner runner loop, oc-server auth + WebSocket pty handler,
  oc-tool tool execution paths, oc-mcp HTTP transport, oc-session/oc-schema
  type duplication.

## Commands executed

- `cargo test --workspace` (full run; output saved to
  `rust-port-audit/artifacts/18-workspace-test.log`). Result: **1519 passed,
  0 failed, 0 ignored** across 82 test binaries; 0 doc-tests.
- Static enumeration of `#[test]` / `#[tokio::test]` attributes per crate.
- Greps: `#[ignore]` (none found anywhere); `CARGO_BIN_EXE`,
  `target/release/opencode`, `/root/.opencode/bin/opencode` in tests (none);
  `Command::new`/`spawn` in tests (git, python3, in-process tokio spawns only).
- Byte-diff scripts comparing Rust fixtures/assets against reference sources.
- `which rg`, `which python3` (both present and required by tests).

## Runtime scenarios attempted

- Full `cargo test --workspace` (time-boxed ~40 min in a shared target dir).
  Completed successfully; log archived.
- `cargo test --workspace -- --ignored` was **not** run because a static grep
  for `#[ignore]` returned zero matches and every test-result line in the
  archived log reports `0 ignored` — there is nothing to re-enable.
- Differential runs against `/root/.opencode/bin/opencode` were attempted at
  the design level only: no test in the repo invokes either binary, so no
  differential e2e harness exists to run.

## Architecture or behavior summary

- The suite is in-process. Tests exercise library code directly; axum routers
  are dispatched with `tower::ServiceExt::oneshot` (oc-server, oc-client), the
  LLM protocols parse in-memory SSE, and subprocesses are limited to `git`,
  `python3` (MCP stdio/HTTP test servers), and `rg`. **No test launches the
  opencode binary** — neither the reference nor the Rust `target/release/opencode`.
- The most behavior-critical crates (oc-session, oc-session-runner) do **not
  use** the shared `oc-schema`/`oc-llm`/`oc-tool` types they depend on; they
  define local mirrors. Their tests therefore validate the mirrors, not
  cross-crate wire consistency.
- Golden fixtures split into three provenance tiers: (a) byte-copied from the
  reference (database DDL, session/tool prompt text, plugin example sources),
  (b) hand-transcribed from reference source (oc-schema goldens, oc-client
  contract, oc-config, oc-llm bodies, oc-sync DDL), and (c) hand-written
  claims that overstate provenance (oc-tool task schema, oc-llm "recorded
  request").

## Positive observations

1. **Verified total: 1519 tests pass, 0 fail, 0 ignored** — the claim is
   independently reproduced on this machine (artifacts/18-workspace-test.log).
2. **oc-database golden is genuinely from the reference**: all 36 objects
   (19 tables + 17 indexes) in `tests/fixtures/schema.sql` byte-match the DDL
   in `reference/packages/core/src/database/schema.gen.ts`, and the test runs
   the real `schema_up` and compares `sqlite_master` output.
3. **Prompt assets are verbatim reference copies**: all 14
   `oc-session/assets/prompt/*.txt` and all 16 `oc-tool/src/prompts/*.txt`
   match the reference files byte-for-byte.
4. **Plugin integration tests run the real QuickJS host** against verbatim
   copies of `reference/packages/plugin/src/example.ts` and
   `example-workspace.ts`, asserting actual tool execution output
   (`crates/oc-plugin/tests/integration.rs:57-80`).
5. **Real subprocess/protocol integration exists**: MCP stdio tests drive a
   real `python3` MCP server (`crates/oc-mcp/tests/stdio.rs`); oc-llm parses
   real provider SSE; oc-project snapshot/worktree tests run against real git
   repos with isolated `OPENCODE_TEST_HOME`.
6. **Auth middleware is tested at the HTTP layer** (401 + `WWW-Authenticate`
   + `UnauthorizedError` body + query-token bypass) in
   `crates/oc-server/tests/api.rs:295-328`.
7. **No external network calls**: every `http(s)://` in tests is a fake
   domain, `.test`, localhost, or `127.0.0.1`; oc-llm/oc-client/oc-server use
   in-process or loopback servers. The suite is offline-safe.
8. **Retry decision logic is well covered** (`retryable` branches for
   ContextOverflow, 4xx/5xx, FreeUsageLimitError, plain-text rate limits,
   JSON rate-limit shapes), with exact `delay()` backoff math asserted
   (`crates/oc-session-runner/src/retry.rs:415-509`).

## Findings summary (table)

| ID | Severity | Confidence | Finding |
|----|----------|------------|---------|
| TEST-01 | High | CONFIRMED | oc-session & oc-session-runner test local mirrors, not oc-schema/oc-llm/oc-tool types; declared deps unused (0 references in src) |
| TEST-02 | High | CONFIRMED | No test invokes the real executable (Rust or reference); zero binary/e2e/differential tests |
| TEST-03 | High | CONFIRMED | oc-tool task "golden" asserts a schema that does NOT match the reference snapshot it cites |
| TEST-04 | High | CONFIRMED | oc-llm "matches recorded cassette" claims are overstated; bodies hand-written, prompt differs from cassette |
| TEST-05 | High | CONFIRMED | oc-tui app.rs (2995 lines) has zero tests; MockSdkClient stubs return empty/Err for nearly every method |
| TEST-06 | Medium | CONFIRMED | GoUsageLimitError retry branch (reset-in formatting, metadata parse) is untested |
| TEST-07 | Medium | CONFIRMED | Runner-loop interruption handling (`fail_interrupted_tools`, `TurnFailure::Interrupted`) untested |
| TEST-08 | Medium | CONFIRMED | WebSocket/pty upgrade path in oc-server is untested |
| TEST-09 | Medium | CONFIRMED | skill, websearch, question, lsp, read, grep tool EXECUTION untested (schema/registry only) |
| TEST-10 | Medium | CONFIRMED | Constant-time `subtle_equal` auth comparison is not mutation-resistant (timing not asserted) |
| TEST-11 | Medium | CONFIRMED | oc-sync workspace DDL golden is hand-derived from drizzle renderer reasoning, not the reference |
| TEST-12 | Low | CONFIRMED | Test names overstate coverage (oc-tui "snapshot", oc-tool "matches_reference_snapshot") |
| TEST-13 | Low | CONFIRMED | Minor cleanup leaks (oc-database pid-keyed temp dir not pre-cleaned; best-effort `let _` cleanup) |
| TEST-14 | Low | CONFIRMED | `test_home_env_override` mutates a process-global env var in a parallel test binary |

## Detailed findings

### TEST-01 — Session crates test local mirrors; shared foundation unused
Severity: High · Confidence: CONFIRMED · STATIC + RUNTIME

`crates/oc-session/src/v1.rs:471` defines its own `enum Part`, and
`crates/oc-session/src` contains **zero** references to `oc_schema`, `oc_llm`,
`oc_tool`, `oc_plugin`, `oc_mcp`, `oc_provider`, `oc_util`, or `oc_core`
(verified by grep across the whole `src/` tree), even though all are declared
in `crates/oc-session/Cargo.toml`. `crates/oc-session-runner/src` likewise
contains zero references to its declared `oc-session`, `oc-llm`, `oc-tool`,
`oc-plugin`, `oc-provider` deps. Meanwhile `crates/oc-schema/src/v1/session.rs:770`
independently defines the same `enum Part`. Consequences for the test suite:
- `crates/oc-session/tests/roundtrip.rs` and the 93 in-crate unit tests
  validate the local `oc_session::v1` mirror only. If `oc-schema::v1::Part`
  diverges from `oc_session::v1::Part` (field names, optionality, defaults),
  **no test fails**.
- `crates/oc-session-runner/tests/runner_loop.rs` builds `RunnerDeps` from the
  crate's own `session::services::*` traits; the mocks reproduce the crate's
  own contract, not the real oc-llm/oc-tool/oc-database implementations
  (scope item 4). It tests the runner's orchestration faithfully, but the
  boundary with the real stores/LLM is not exercised anywhere.
- This is the single largest validity risk in the suite: the two
  behavior-critical crates are self-consistent but unintegrated.

### TEST-02 — No test exercises the real executable
Severity: High · Confidence: CONFIRMED · STATIC

No occurrence of `CARGO_BIN_EXE`, `target/release/opencode`,
`target/debug/opencode`, or `/root/.opencode/bin/opencode` exists anywhere in
`crates/`. `oc-cli` has no `tests/` directory and its binary (`src/main.rs`)
has 0 tests. All server tests dispatch via `oneshot` (no socket), and all CLI
coverage is unit-level (21 tests across helper modules:
`crates/oc-cli/src/cli/...`). There is therefore **no end-to-end proof that
the shipped binary boots, parses argv, or serves** — and no differential test
against the reference executable that the audit was set up to allow.

### TEST-03 — oc-tool task schema golden contradicts the reference snapshot
Severity: High · Confidence: CONFIRMED · STATIC

`crates/oc-tool/tests/golden.rs:151-171` (`task_schema_matches_reference_snapshot`)
asserts the task tool JSON Schema **without** `background`, and
`crates/oc-tool/src/tool/task.rs:74-78` exposes `base_parameters()` (no
`background`) as the LLM-facing schema. The reference snapshot
`reference/packages/opencode/test/tool/__snapshots__/parameters.test.ts.snap:300-336`
shows the task wire schema **with** `background`. Nuance (verified against
`reference/packages/opencode/src/tool/task.ts:343-350`): the reference runtime
tool def also returns `jsonSchema: ToolJsonSchema.fromSchema(BaseParameters)`
when the experimental flag is off, so the Rust *runtime* behavior matches the
reference *def*. But the test's title and framing ("matches reference
snapshot") are false, and it pins the divergence in place: the reference test
itself (`parameters.test.ts`) snapshots the full `Parameters` schema. If the
port ever needs to align its task tool wire shape with the reference's
published schema, this golden actively enforces the wrong expectation.

### TEST-04 — oc-llm golden tests overstate cassette provenance
Severity: High · Confidence: CONFIRMED · STATIC

The header comment (`crates/oc-llm/tests/golden.rs:3`) claims bodies are
"compared … against expectations derived from the reference cassettes", but no
test loads any file under
`reference/packages/llm/test/fixtures/recordings/`. Expected bodies are
hand-written `r#"..."#` strings. `openai_chat_body`
(`crates/oc-llm/tests/golden.rs:26-35`) uses prompt `"Say hello."` while the
recorded request in `reference/packages/llm/test/fixtures/recordings/openai-chat/streams-text.json`
contains `"Say hello in one short sentence."` — i.e. the hand-built input
deliberately differs from the cassette, and the serialization shape is
asserted against a hand-authored expectation. The stream tests
(`crates/oc-llm/tests/stream.rs`) parse hand-written SSE chunks, not cassette
payloads. The tests are still meaningful for pinning wire format, but the
provenance claim is overstated and would not catch a drift from the true
recorded request/response.

### TEST-05 — oc-tui application logic is untested
Severity: High · Confidence: CONFIRMED · STATIC

`crates/oc-tui/src/app.rs` (2,995 lines) contains no `#[cfg(test)]` module and
0 tests. `MockSdkClient` (`crates/oc-tui/src/client.rs:685-816`) returns
empty/`Err` for every method except the pre-queued event stream, so no test
can drive real sync/session/agent flows through the app. The `rendering.rs`
integration tests build `SyncState` by hand, call pure render functions, and
assert `contains(...)` — valid unit checks of renderers, but not behavior.
There is no test that a keypress changes screens, that a `session.part.updated`
event mutates state, or that prompt submission calls the client. A mutation of
the app event loop would not fail any test.

### TEST-06 — GoUsageLimitError retry branch untested (mutation target #1)
Severity: Medium · Confidence: CONFIRMED · STATIC

`crates/oc-session-runner/src/retry.rs:207-265` (GoUsageLimitError handling:
JSON metadata parse, `workspace`/`limitName` extraction, the `reset_in`
humanized-duration formatter at `retry.rs:218-255`, and the
`https://opencode.ai/workspace/{workspace}/go` link) has **no test**. The
analogous FreeUsageLimitError branch is tested at `retry.rs:453`. Deleting or
inverting this block (e.g. returning the wrong reset string, dropping the
workspace slug) would leave all 1519 tests green. HIGH-confidence gap.

### TEST-07 — Runner-loop interruption untested (mutation target #2)
Severity: Medium · Confidence: CONFIRMED · STATIC

`crates/oc-session-runner/src/runner/llm.rs` handles
`TurnFailure::Interrupted` (`llm.rs:142,198,228`) and `fail_interrupted_tools`
(`llm.rs:245`) but has only two unit tests, both about session-id→prompt-key
strings (`llm.rs:789-796`). `runner_loop.rs` covers only the happy
tool-call→continuation cycle and the no-work noop. No test cancels the
`CancellationToken` mid-turn to verify interrupted tools are failed and the
loop exits cleanly. A mutation removing `fail_interrupted_tools`'s tool-fail
publishing would not be caught.

### TEST-08 — WebSocket/pty upgrade untested (mutation target #3)
Severity: Medium · Confidence: CONFIRMED · STATIC

`crates/oc-server/src/handlers/pty.rs:195-221` (`WebSocketUpgrade`,
`pty_socket`) has no test; the route-table test only checks the route exists.
`crates/oc-server/src/proxy_util.rs:55-85` unit-tests header/protocol helpers
only. No test opens a WS connection. The 52 oc-server tests are all HTTP
`oneshot` dispatches. A defect in socket framing, upgrade validation, or
pty message relay would pass.

### TEST-09 — Built-in tool EXECUTION gaps
Severity: Medium · Confidence: CONFIRMED · STATIC

Executed end-to-end: `apply_patch` (`crates/oc-tool/src/tool/apply_patch.rs:354-413`),
`edit` (2), `write` (2), `todo` (2), `shell` (3), `webfetch` (1), `glob` (1),
core `read` (`crates/oc-tool/src/core/read_filesystem.rs:350-400`), ripgrep
glob/grep (`crates/oc-tool/src/ripgrep.rs:272-`). Not executed via the tool
def: `skill` (**0 tests**), `websearch` (schema only), `question` (schema
only), `lsp` (schema only), `read` tool def (core only), `grep` tool def
(ripgrep module only, `grep.rs:184` schema-only test). Mutating the execution
bodies of skill/websearch/question/lsp would not fail tests.

### TEST-10 — Constant-time comparison not mutation-resistant
Severity: Medium · Confidence: CONFIRMED · STATIC

`crates/oc-server/src/auth.rs:130-140` (`subtle_equal`) is asserted only for
correctness; replacing it with `a == b` passes all tests (no timing
measurement exists). The middleware-level tests
(`crates/oc-server/tests/api.rs:295-328`) validate outcomes, not
constant-time behavior. A timing side-channel regression would be invisible to
the suite.

### TEST-11 — oc-sync DDL golden is derived, not copied
Severity: Medium · Confidence: CONFIRMED · STATIC

`crates/oc-sync/src/control_plane/workspace_sql.rs:90-132` asserts a
hand-rendered `CREATE TABLE workspace` string. The module comment
(`crates/oc-sync/src/sync/sql.rs:7-12`) states the strings "are derived from
the drizzle-kit `SQLiteCreateTableConvertor` renderers" because the reference
has no checked-in migration — i.e. re-derived by reasoning, not extracted from
reference output, and drizzle-kit is not runnable here. Note also the
oc-database fixture's `workspace` table uses a **named** FK constraint
(`CONSTRAINT fk_workspace_project_id_project_id_fk … ON DELETE CASCADE`)
whereas the oc-sync render emits an **unnamed** `FOREIGN KEY … ON UPDATE no
action ON DELETE cascade` — two different DDLs for the same logical table, and
no test reconciles them (oc-sync is not wired to oc-database; `TODO(integration)`
at `crates/oc-sync/src/sync/sql.rs:13-14`).

### TEST-12 — Test names overstate coverage
Severity: Low · Confidence: CONFIRMED · STATIC

- `crates/oc-tui/tests/rendering.rs:110` `session_message_list_snapshot` is a
  `contains` check, not a snapshot; no reference TUI output is used.
- `crates/oc-tool/tests/golden.rs` function names say "matches reference
  snapshot" but embed hand-written JSON (see TEST-03).
- `crates/oc-llm/tests/golden.rs` "matches the recorded … request" (TEST-04).
- `crates/oc-session/tests/golden.rs` asserts environment-block text with a
  fixed date/`today` literal; it will silently drift if the reference's date
  formatting changes (today's date is baked in as `"Mon Aug 05 2026"`).

### TEST-13 — Cleanup reliability
Severity: Low · Confidence: CONFIRMED · STATIC

- oc-mcp tests clean up with best-effort `let _ = std::fs::remove_dir_all`
  (`stdio.rs:220,282,356`; `http_oauth.rs:150,356,426`) — a panic between
  create and remove leaks the dir.
- oc-database concurrency test
  (`crates/oc-database/tests/migrations.rs:577-594`) uses a pid-keyed temp dir
  without pre-cleaning; a stale `embedded.sqlite` from a prior run with the
  same pid could mask/alter results (current assertions tolerate it).
- oc-project tests remove the pid-keyed home at start
  (`crates/oc-project/tests/common/mod.rs:10-16`) but never at end — leaks a
  full git-worktree tree per run.
- Overall: cleanup is best-effort but functional; no shared mutable global
  fixtures are reused across test binaries.

### TEST-14 — Env-var mutation in parallel test process
Severity: Low · Confidence: CONFIRMED · STATIC

`crates/oc-util/src/global.rs:86-90` (`test_home_env_override`) sets and then
removes the process-global `OPENCODE_TEST_HOME`. Within the same test binary
(157 tests run in parallel threads) another test reading `path::home()` during
that window would see the overridden value. No current oc-util test actually
reads it in that window (they only join path fragments), so today it is
harmless, but it is a latent parallel-interference hazard and is exactly the
kind of global-state mutation the audit was asked to flag.

## Ignored-test analysis

There are **no ignored tests**. `grep '#[ignore'` across `crates/` returns
nothing, and every one of the 82 test-result lines in
`artifacts/18-workspace-test.log` reports `0 ignored`. So incomplete
integration is **not** hidden behind `#[ignore]` (a positive). The trade-off:
nothing is gated, so genuinely-unwired features (e.g. oc-tool `task` returning
`TODO(integration): subagent result text.` at `crates/oc-tool/src/tool/task.rs:185`,
or the oc-sync→oc-database `TODO(integration)`) are either exercised as
stubs or not at all, and the suite cannot distinguish "feature done" from
"feature stubbed" without reading source.

## Feature or behavior gaps

- **No e2e/CLI/TUI differential testing** of the real binary (TEST-02).
- **Session/session-runner are self-contained mirrors** — cross-crate parity
  with oc-schema/oc-llm/oc-tool is unproven (TEST-01).
- **oc-sync workspace table is not wired to oc-database**; its DDL and the
  database's `workspace` DDL differ in FK shape with no reconciliation test
  (TEST-11).
- **Background subagents**: the Rust `task` tool returns a hardcoded
  `TODO(integration)` output (`crates/oc-tool/src/tool/task.rs:181-186`) and
  the depth check is stubbed (`task.rs:130-138`); no test asserts real
  subagent semantics.
- **oc-server WebSocket/pty**, **oc-tui app loop**, **oc-cli binary**
  behavior are untested (TEST-08, TEST-05, TEST-02).

## Test coverage gaps (mutation-resistance analysis, 10 targets)

| # | Function / location | What a mutation could break | Would tests fail? |
|---|---------------------|-----------------------------|-------------------|
| 1 | `retryable()` GoUsageLimitError branch `crates/oc-session-runner/src/retry.rs:207-265` | reset-in formatting, workspace slug, Go link | NO (no test touches it) |
| 2 | `delay()` RFC2822 date branch `crates/oc-session-runner/src/retry.rs:154` | HTTP-date `retry-after` handling | NO (only ms/seconds tested) |
| 3 | `fail_interrupted_tools` / `TurnFailure::Interrupted` `crates/oc-session-runner/src/runner/llm.rs:142,198,228,245` | interrupted-tool failure publishing | NO |
| 4 | WebSocket upgrade + pty relay `crates/oc-server/src/handlers/pty.rs:195-221` | WS framing/upgrade auth | NO |
| 5 | `skill` tool execute `crates/oc-tool/src/tool/skill.rs` | skill execution entirely | NO (0 tests) |
| 6 | `websearch` tool execute `crates/oc-tool/src/tool/websearch.rs` | provider call/result | NO (schema only) |
| 7 | `question`/`lsp`/`grep`/`read` tool `def().execute` | execution bodies | NO (schema/core-only) |
| 8 | `subtle_equal` `crates/oc-server/src/auth.rs:130-140` | constant-time property | NO (correctness-only asserts) |
| 9 | HTTP transport SSE dispatch/redirects `crates/oc-mcp/src/transport/http.rs:456-465` | frame handling beyond `parse_sse_message` | NO (only happy path via integration) |
| 10 | model-filter exclusions (gpt-4*/gpt-oss* cases) `crates/oc-tool/tests/golden.rs:361-385` | filter condition edge cases | Partial (only gpt-5 vs claude sampled) |

Also note `crates/oc-tui/src/app.rs` and `crates/oc-cli/src/cli/cmd/*` command
handlers have effectively zero direct mutation resistance.

## Unverified areas

- Whether `oc-session::v1::Part` and `oc-schema::v1::Part` actually agree:
  would require a cross-crate serialization harness that does not exist.
  **BLOCKED_BY_MISSING_EVIDENCE** (no test compares them).
- Whether the hand-derived oc-sync DDL truly matches drizzle-kit output:
  drizzle-kit/bun unavailable. **UNVERIFIED** (MEDIUM).
- Whether oc-schema golden strings (hand-transcribed, e.g.
  `golden_core.rs:2`) are error-free for all 120 cases; one sample
  (`agent_empty`) verified correct against
  `reference/packages/schema/src/agent.ts`. **UNVERIFIED** for the remainder.
- Timing side-channel properties of `subtle_equal`; no runtime evidence.
- External-network behavior (websearch/webfetch tool execution) — not tested,
  so runtime correctness is unproven.
- Differential parity of request/response bodies vs the true reference
  cassettes for oc-llm (cassettes not loaded by tests).

## Final domain verdict

**READY_WITH_MINOR_REMEDIATION** — for the *test-suite* domain specifically.
The suite is real, deterministic, offline, and the 1519-pass claim is
verified; fixture provenance is strong where it matters most (database DDL,
prompt assets, plugin examples). But it does not validate the two
behavior-critical session crates against the shared schema foundation, never
runs the actual binary, overstates several golden/cassette provenance claims,
and leaves high-risk retry/interruption/WebSocket/tool-execution paths
mutation-proof. Remediation targets: cross-crate serialization tests,
integration of oc-session with oc-schema types, tool-execution tests for
skill/websearch/question/lsp, interruption tests for the runner, and correcting
the oc-tool task / oc-llm provenance claims.
