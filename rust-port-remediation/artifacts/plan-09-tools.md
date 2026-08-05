# Plan 09 — Tool Execution, Filesystem Safety, Process Lifecycle

Agent 09 · Wave 0 read-only planning. Repo `/root/opencode-rs` @ `fix/audit-remediation`.
Domain: `oc-tool` execution boundary, filesystem containment, process lifecycle, output
bounding, blocking-in-async. Static investigation only (no edits, no commit).

---

## 1. Owned findings

Consolidated from `FINDINGS.json` + reports 11/14/15. Severity/evidence as audited at
`e7fc33e8359bb064c745761ce8e2f9023ae0ae8c`.

| ID | Sev | Blocker | Evidence (file:line) | Status |
|----|-----|---------|----------------------|--------|
| TOOLS-002 | High | YES | `core/read_filesystem.rs:149` `std::fs::read(&real)` + `tool/read.rs:376` `std::fs::read` materialize whole file before page/limit logic. No size guard. Reference streams and stops at byte cap (`read-filesystem.ts:284-310`). | CONFIRMED (static + probe) |
| TOOLS-003 | Medium | NO | `util.rs:36-44` `fs_contains` lexical (`std::path::absolute`), `symlink_metadata` follows intermediate components; probe wrote through `ws/link -> outside`. Affects V1/V2 read, write (`tool/write.rs:111-119`), edit, apply_patch, bash workdir. Reference V2 read resolves canonically (`read.ts:60-71`); write side is parity-faithful but still unsafe. | CONFIRMED (RUNTIME probe) |
| TOOLS-004 | Medium | NO | `tool/shell.rs:517,522` + `core/bash.rs:245` `child.kill()` on the direct shell only; `sh -c 'sleep 1000 &'` orphans the sleep. No process group; also `core/bash.rs:223` `kill_on_drop` does not cover grandchildren. | CONFIRMED (static) |
| ASYNC-002 | High | YES | `core/tool.rs:216-223` `run_future` uses `Handle::block_on` inside async context → panics "Cannot start a runtime from within a runtime" (runtime proof); otherwise builds a fresh multi-thread runtime per call. Callers: `core/bash.rs:176`, `core/misc.rs:109,124,347`. | CONFIRMED (RUNTIME) |
| ASYNC-006 | Medium | NO | `oc-core/src/process.rs:147-255` drops `tokio::process::Child` on timeout without `kill_on_drop`/explicit kill → orphan; `oc-util/src/util/process.rs` `kill_pid` errors are trace-only (RUST-004 report-14 variant) and `attach_abort` spawns an unbounded detached task per spawn. | CONFIRMED (static) |
| SEC-004 | Medium | NO | `core/bash.rs:234-239` `read_to_end` accumulates unbounded stdout/stderr before truncating at 1 MiB; `tool/shell.rs:459` unbounded `mpsc` per-chunk; `tool/shell.rs:648-651` per-chunk `ctx.metadata` grows ~3.75× output (TOOLS-10). | CONFIRMED (static) |
| RUST-004 | Medium | NO | `tool/tool.rs:25-34` `sync_execute` runs sync handlers (incl. `ripgrep.rs:118-124` `Command::output()`) on the tokio executor thread; no `spawn_blocking` anywhere in tool/runner/core. `rg` has no timeout and materializes all stdout (`ripgrep.rs:127`). | CONFIRMED (static) |
| RUST-005 | Low | NO | `tool/shell_prompt.rs:64-74` `panic!("Missing shell prompt value")` on `${key}` not in values; `oc-llm/src/llm.rs:214` `unreachable!()` (agent 06 domain); poison-mutex `.lock().unwrap()` in `core/registry.rs:64,86` + `truncate.rs:110,116` (oc-tool side). | CONFIRMED (static) |
| TOOLS-06 | Medium | NO | `tool/shell.rs:170-279` `collect`/`split_segments`/`tokenize` split only on `; | \n` + quotes; misses `&&`/`||`/`$(...)`/backticks/heredocs → `echo ok && rm -rf "$HOME/x"` emitted as one `echo *` pattern → an "always echo" grant auto-approves the destructive suffix. Reference uses tree-sitter (`shell.ts:257-261`). | CONFIRMED (static) |
| TOOLS-07 | Medium | NO | `ripgrep.rs:118-133` `Command::output()` blocks executor, no timeout, full stdout → String before record cap. | CONFIRMED (static) |
| TOOLS-08 | Medium | NO | `oc-server/src/handlers/fs.rs:53-54` `fs_list` joins user `path` without `..` sanitization (dead code today — server not mounted). Owned here for the containment pass; wiring owned by agent 10. | CONFIRMED (static) |
| TOOLS-09 | Low | NO | Read tools reject symlinked files (`tool/read.rs:80,110-111`) while reference follows; parity gap. Will fix as part of canonical containment (follow symlinks, then check). | CONFIRMED (static) |
| TOOLS-10 | Low | NO | Per-chunk metadata accumulation (`tool/shell.rs:648-651`); fold into SEC-004 bounded-buffer design. | CONFIRMED (static) |
| TOOLS-11 | Low | NO | TOCTOU between stat/containment and open (`core/read_filesystem.rs:142-150`, `tool/write.rs:111-119`). Mitigate via canonicalize + open-with-checks; document residual. | CONFIRMED (static) |

---

## 2. Files to change (all under `crates/`)

**oc-tool**
- `src/core/read_filesystem.rs` — streaming read, canonical containment, list symlink handling.
- `src/tool/read.rs` — streaming `read_lines`, canonical containment.
- `src/core/read.rs` — canonical containment + permission-integrated external_directory.
- `src/core/bash.rs` — process group, bounded streaming capture, kill-on-timeout tree, `spawn_blocking`-safe execution.
- `src/tool/shell.rs` — process group creation + tree kill, bounded channel + spill, tokenizer parity, permission integration.
- `src/core/tool.rs` — `run_future` rework (ambient-runtime-aware async exec), `CoreContext.assert` permission enforcement seam.
- `src/util.rs` — canonical `fs_contains`, `resolve_canonical`.
- `src/tool/write.rs`, `src/tool/edit.rs`, `src/core/edit.rs`, `src/core/write.rs`, `src/tool/apply_patch.rs` — canonical containment on every mutation + permission integration.
- `src/tool/external_directory.rs` — canonical boundary for external-directory asks.
- `src/ripgrep.rs` — async `rg` with timeout, streaming JSON parse, bounded buffer.
- `src/tool/glob.rs`, `src/tool/grep.rs`, `src/core/glob_grep.rs` — route through bounded/async rg, spawn_blocking.
- `src/core/misc.rs` — `run_future` callers → async path.
- `src/tool/shell_prompt.rs` — panic → typed error (RUST-005).
- `src/model.rs` — `ToolContext.ask` becomes a real permission-gate call (via Agent 08 service); add `PermissionDenied` error variant.
- `src/tool/tool.rs` — execute wrapper runs through permission gate.
- `src/core/registry.rs` — settle gate + poison-lock handling (RUST-005).
- `src/truncate.rs` — poison-lock handling.
- `src/tool/registry.rs` — no change (assembly), but permission filter hooks confirmed.

**oc-core**
- `src/process.rs` — `kill_on_drop(true)` + kill on timeout + Drop guard (ASYNC-006).

**oc-util**
- `src/util/process.rs` — kill failures → typed error/log; bound `attach_abort` tasks.

**oc-server** (ownership shared w/ agent 10; changes are additive + dead-code-safe)
- `src/handlers/fs.rs` — `fs_list` `..`/absolute sanitization (TOOLS-08).

---

## 3. Safety-fix designs

### 3.1 TOOLS-002 — Bounded streaming reads (V2 `read_filesystem` + V1 `tool/read`)

Port the reference streaming algorithm exactly (`read-filesystem.ts:171-322`, `read.ts:137-180`):
open `File`, read 64 KiB chunks, keep a rolling line/page buffer bounded by
`MAX_READ_BYTES`/`MAX_READ_LINES`, stop early once cap reached; do **not** build the full
`String` first. Binary/PDF/image checks operate on the first 64 KiB sample only (as the
reference does); media ingest is streamed up to `MAX_MEDIA_INGEST_BYTES` and fails past it.
V1 `read_lines` in `tool/read.rs:374-418` is rewritten to stream chunk-by-chunk with a
manual `TextDecoder`-equivalent (carry partial UTF-8 across chunk boundaries) instead of
`String::from_utf8_lossy(&content)` on the whole file.

### 3.2 TOOLS-003/TOOLS-11/TOOLS-09 — Canonical containment (no lexical-only checks)

Add `util::resolve_canonical(path) -> Result<PathBuf>` using `std::fs::canonicalize` with a
fallback that canonicalizes the deepest existing ancestor and rejoins the tail (mirrors the
reference `resolvePath` in `location-mutation.ts:90-118`, which is the correct reference
behavior for read/write boundaries). Replace every `fs_contains` guard on a **to-be-opened**
path with `fs_contains(location_root, canonical)`. Design decisions:

- **Follow symlinks for read** (parity + TOOLS-09): canonicalize then containment-check. A
  symlink inside the workspace pointing outside is treated as external → prompts
  `external_directory` (matches reference V2 read).
- **Write**: canonicalize the parent, containment-check the canonical parent, then create the
  file. Writing *through* a symlink that resolves outside is rejected without
  `external_directory` approval. This is stricter than the lexical reference write, but
  closes the confirmed escape; document as a deliberate hardening divergence.
- **TOCTOU (TOOLS-11)**: after canonicalize, `open` the file handle and re-canonicalize via
  `/proc/self/fd` on Linux (or `fstat` inode compare) before read; for write, write via the
  opened handle (`OpenOptions`) rather than re-resolving the path string. Residual TOCTOU is
  same-user-local, documented, accepted (matches reference).
- **Bash/glob/grep workdir** containment also routes through canonical resolve.

### 3.3 TOOLS-004 + ASYNC-006 — Process-group lifecycle and Drop guards

- Both `tool/shell.rs` and `core/bash.rs` spawn with `process_group(0)` on Unix
  (tokio `Command::process_group`, confirmed available in tokio 1.53.1) so the child is its
  own group leader; `kill_on_drop(true)` stays. On timeout/abort kill **the group**:
  `libc::kill(-pgid, SIGTERM)`, wait a short grace (~3 s, matching reference
  `forceKillAfter: "3 seconds"` in `bash.ts:163` and `shell.ts:550-555`), then
  `SIGKILL` the group. Windows keeps `taskkill /T` (already in `oc-util process.stop`).
- Wrap the child+kill responsibility in a `ProcessGuard` RAII struct whose `Drop` sends
  SIGTERM→SIGKILL to the group if not yet reaped (covers panic paths and task abort).
- `oc-core/src/process.rs` (`git` path): add `kill_on_drop(true)` and, on `timeout_at`
  expiry, kill the child before returning (ASYNC-006).
- `oc-util/src/util/process.rs`: make `kill_pid` return `io::Result`, log real failures,
  bound `attach_abort` spawns (track/abort on parent drop, or make it a single guarded task).

### 3.4 SEC-004 / TOOLS-10 — Bounded shell capture with spill (both bash engines)

`core/bash.rs` `run_command`: replace `read_to_end` with a bounded reader loop that
`append_limited`s into fixed-size `Vec`s (cap `MAX_CAPTURE_BYTES`), continuing to drain
stdout/stderr to EOF (so the child never blocks on a full pipe) but retaining only the cap;
set `truncated`. Optionally spill the tail beyond the cap to `tool-output/` via
`truncate::write` (keeps the "full output saved" UX). `tool/shell.rs` is already bounded via
`keep = 2×maxBytes` + spill; the gap is the unbounded per-chunk `ctx.metadata` accumulation —
cap `metadata` pushes to the last N chunks (only the newest ~keep bytes) and stop after `cut`.
Also bound the stdout/stderr reader tasks' in-flight channel: use a bounded `mpsc` channel
(capacity ~4) so a slow consumer cannot grow memory, and `select!` on chunk send vs abort.

### 3.5 TOOLS-07 / RUST-004 — rg with timeout, streaming, off the executor

Rewrite `ripgrep.rs` to spawn `rg` via `tokio::process::Command` (still exec-style argv, no
shell), with:
- a hard timeout (config default, e.g. 60 s) enforced by `tokio::time::timeout` + kill,
- line-by-line streaming of stdout into a bounded buffer, applying `MAX_RECORD_BYTES` and
  `limit` as it reads (never materialize all stdout),
- capped stderr.
Callers (`tool/glob.rs`, `tool/grep.rs`, `core/glob_grep.rs`) become async; sync V1 leaves are
run via `spawn_blocking` so the executor thread is not blocked (RUST-004). Because the
`Def::execute` is already `async` (`tool/tool.rs:55-61`), moving the sync body into
`tokio::task::block_in_place`/`spawn_blocking` is a contained change.

### 3.6 ASYNC-002 — `run_future` rework

Replace `core/tool.rs:216-223` with a single entry point that:
1. If a `Handle` is current → `Handle::spawn` the future and `.await` the JoinHandle (never
   `block_on`).
2. Else (off-runtime thread, e.g. sync tests) → `Handle::block_on` is still fine, or create a
   runtime if none. Recommended: `pub async fn run_future_async<T>(fut) -> Result<T, ToolError>`
   that always awaits (used by the runner), plus a sync shim that detects the ambient runtime
   and does `block_on` only from a non-async thread. `core/bash.rs`/`core/misc.rs` callers
   switch to the async form; `poll` in `core/tool.rs:203-214` becomes async or is delegated
   from the runner (agent 07) via the fiber path.

### 3.7 RUST-005 — User-triggerable panics → typed errors

- `tool/shell_prompt.rs:70`: `render_prompt` returns `Result<String, ToolError>`; missing
  `${key}` → error string instead of panic (reference renders `undefined`).
- `core/registry.rs:64,86` + `truncate.rs:110,116`: replace poisoned `.lock().unwrap()` with
  `unwrap_or_else(|p| p.into_inner())` (data is `Copy`/cheap) so a poisoned lock degrades, not
  panics.
- Add `ToolError::PermissionDenied { ... }` for the permission gate (3.8).

### 3.8 TOOLS-06 + TOOLS-02 — Permission integration + tokenizer parity

- **Tokenizer (TOOLS-06)**: replace the `split_segments`/`tokenize` approximation in
  `tool/shell.rs:170-279` with a small bash tokenizer that handles `&&`/`||`, `;`, pipes,
  `$(...)`/backticks (treated as dynamic), single/double quotes, heredocs, and comments, and
  emits **one pattern per top-level command** (matching tree-sitter `command` nodes in
  `shell.ts:392-414`). Keep `bash_arity_prefix` (already parity-correct vs
  `permission/arity.ts`). This prevents compound commands from collapsing into a single
  over-broad `always` pattern. Document remaining divergence (no full grammar) as a residual
  risk; any unparseable segment degrades to *ask*, never auto-`always`.
- **Permission gate (TOOLS-02, integration with Agent 08)**: `ToolContext::ask` and
  `CoreContext::assert` no longer record-and-return-`Ok`. They call Agent 08's permission
  service (`evaluate` over merged agent ruleset + session-approved `always` rules, matching
  `reference/permission/index.ts:67-107`), and:
  - `allow` → continue,
  - `deny` → `Err(ToolError::PermissionDenied)`,
  - `ask` → surface a permission request and await the user reply (session round-trip owned
    by Agent 08/07); `--dangerously-skip-permissions` short-circuits to allow.
  Because every dangerous tool (bash, read, write, edit, apply_patch, glob, grep, webfetch,
  task) already emits correctly-shaped asks through `ctx.ask`/`ctx.assert`, enforcement is
  centralized in the two context methods + the settle wrapper (`core/registry.rs`
  `settle_registration` + `tool/tool.rs` execute wrapper), not per-tool.

---

## 4. Test list (new)

Unit (in `crates/oc-tool/src/**` + `crates/oc-tool/tests/`):
1. **Large-file bounded read**: write a >64 MiB text file (sparse), assert `read` returns a
   TextPage ≤ `MAX_READ_BYTES` and peak RSS stays bounded (measure via `mallinfo`/`getrusage`
   delta, or assert we never allocate > cap by construction with a `CountingRead`-style
   fixture). `/dev/zero` is rejected (device guard) — reuse the 11-probe.
2. **Paged read streaming**: offset/limit reads over a huge file return correct lines without
   whole-file materialization (chunk-boundary UTF-8 continuation covered).
3. **`/dev/zero` + FIFO + socket bounded**: read tool returns `ToolError` and allocates ≤ cap.
4. **Shell timeout kills grandchildren**: run `sh -c 'sleep 1000 & wait'` with a short
   timeout; assert the `sleep` PID is gone after the tool returns (pgid kill). Also
   `bash` core engine equivalent.
5. **ProcessGuard drop kills tree**: spawn a background child, drop the guard mid-run, assert
   reaped.
6. **Symlink escape rejection**: `ws/link -> /tmp/outside`; read/write/edit/apply_patch via
   the link require `external_directory` or fail; assert content never lands outside without
   approval. Regression of the 11-probe.
7. **Symlinked-file read works (TOOLS-09)**: direct symlink to an internal file reads fine.
8. **Permission-integrated tool execution**: mock Agent 08 service; assert (a) deny → tool
   error, no side effect; (b) allow → executes; (c) ask + approve → executes once; (d)
   `--dangerously-skip-permissions` → allow-all. Applied to bash, write, edit, read.
9. **Tokenizer parity**: `echo ok && rm -rf /x` yields **two** patterns (`echo *`, `rm *`);
   `$(...)`/backtick segments flagged dynamic; heredoc body not tokenized as commands.
10. **rg timeout + streaming**: grep over a FIFO/slow tree times out; huge match stream
    stops at `limit` with bounded memory (no full stdout).
11. **Bounded shell output spill**: `yes | head -c 200M` → truncated=true, bounded RSS,
    full output saved to `tool-output/`.
12. **RUST-005**: missing shell-prompt key returns error (no panic); poisoned-lock registry
    degrade test.
13. **oc-core process timeout reaps child** (ASYNC-006).
14. **fs_list `..` sanitization** (oc-server, dead-code-safe unit test).

---

## 5. Dependencies

- **Agent 07** (session/runner wiring): consumes the reworked `run_future`/async `settle`
  (3.6) and the permission-gated settle path; must NOT invoke `Handle::block_on`. This plan's
  async-first API is a prerequisite for TOOLS-001 wiring.
- **Agent 08** (permission service): provides the `PermissionService` trait/`evaluate`
  + ask/reply round-trip + `--dangerously-skip-permissions` semantics that 3.8 binds into
  `ToolContext::ask`/`CoreContext::assert`. **Hard prerequisite** — see §7.
- **Agent 02** (oc-schema integration): canonical promotion of the shared
  `PermissionRequest`/`Rule`/`ToolCall`/`ToolError` mirror types this plan's gate depends on
  (currently private in `oc-tool/src/model.rs` + `oc-session/src/v1.rs`); coordinate to avoid
  two copies of `PermissionRule`.
- **Agent 13** (protocol/MCP): grep/glob `rg` exec shape and the tool-output spill path
  (`tool-output` URI/globs) must match the reference protocol; confirm the truncation dir
  glob remains addressable by read/grep (it is used by 3.4 spill).
- **Agent 10** (server handlers): TOOLS-08 `fs_list` sanitization lives in `oc-server`; this
  plan provides the canonical-containment helper; agent 10 owns mounting it when the server
  is wired.
- **Agent 06** (LLM streaming): RUST-005 `llm.rs:214` `unreachable!` is owned there; only the
  oc-tool panic paths are owned here.

## 6. Risks

- **Behavioral divergence on writes**: canonical write containment (3.2) is stricter than the
  lexical reference write; may prompt where the reference silently writes. Acceptable
  hardening, but must be documented in the release notes / parity report.
- **Process-group kill side effects**: killing the group on timeout may kill
  session-shared helper processes spawned *into* the child's group; mitigated by using a
  fresh `process_group(0)` per invocation so nothing is shared.
- **Canonicalize fallback correctness**: paths that don't yet exist (write/create) need the
  ancestor-canonicalize + rejoin logic; edge cases (parent is a symlink, trailing slashes)
  need the probe-style tests.
- **Tokenizer parity debt**: hand-rolled tokenizer still diverges from tree-sitter on exotic
  bash; residual risk capped by ask-on-unparseable.
- **Permission-gate latency**: ask/reply round-trip per tool call adds a user interaction;
  existing `always` rules must be honored to avoid prompt fatigue (matches reference).
- **`rg` behavior on absent binary**: `tokio::process::Command` fails to spawn if `rg` is not
  on PATH — same as today, but now an error path that must be surfaced (not crash).
- **`block_in_place`/`spawn_blocking` pool exhaustion**: many concurrent large greps could
  saturate the blocking pool; cap concurrent tool spawns or rely on timeouts.
- **Windows**: process-group/kill-tree paths untestable on this Linux host; keep
  `#[cfg(windows)]` `taskkill /T` path, add `#[cfg(unix)]` gated tests.

## 7. Merge-order recommendation (security-first)

1. **Agent 08 permission service FIRST** (SEC-001) — the gate that makes every dangerous tool
   safe. **TOOL EXECUTION MUST NOT BECOME REACHABLE BEFORE PERMISSION (Agent 08) MERGES.**
2. **Agent 09 containment + bounded I/O** (TOOLS-002/003, SEC-004, ASYNC-006, RUST-004,
   TOOLS-07) — can land independent of Agent 08 since `ctx.ask` keeps a safe default
   (deny-by-default in the interim, then flip to Agent 08 enforcement).
3. **Agent 09 async rework** (ASYNC-002 run_future, shell process-group, rg streaming).
4. **Agent 07 runner wiring** (TOOLS-001) — only after 1–3 are merged; the runner fiber must
   call the async settle path, never `block_on`.
5. **Agent 02 schema promotion** and **Agent 13 protocol checks** land as enabling refactors
   before 4.
6. **Agent 10 server mounting + TOOLS-08** last (fs handlers only expose filesystem after the
   server + permission chain exist).
