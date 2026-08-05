# Agent 11 — Tool Execution, Shell, Filesystem, and Workspace Safety

Audit of the opencode-rs Rust port's agentic tool-execution boundary treated as a hostile-input
surface. Commit audited: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c` (per coordinator).

## Scope

Shell execution (exec style, quoting, cwd, env, PATH, timeouts, cancellation, process trees,
output limits, stdin, exit codes, signals), file read/write/list/glob/grep, patch application,
path traversal (`..`, absolute, symlink, hardlink, TOCTOU), workspace boundary enforcement,
hidden/special files, device files, FIFOs, sockets, oversized files, secret/git metadata,
approval/permission gates, argument validation bypass, and approval-to-command binding.

## Repository areas inspected

- `crates/oc-tool/src/tool/shell.rs` (V1 shell tool: parse `collect`, `run`, kill, truncation)
- `crates/oc-tool/src/shell.rs` (shell selection)
- `crates/oc-tool/src/core/shell.rs` — does not exist; V1 shell is the only shell tool
- `crates/oc-tool/src/tool/{read,write,edit,glob,grep,apply_patch,task,code_mode,registry}.rs`
- `crates/oc-tool/src/core/{read,read_filesystem,write,glob_grep,registry,tool,tool_output_store}.rs`
- `crates/oc-tool/src/{model,util,truncate,ripgrep}.rs`
- `crates/oc-session-runner/src/{runner/llm.rs,session/services.rs,run_coordinator.rs}` + `tests/runner_loop.rs`
- `crates/oc-session/src/{processor.rs,permission.rs,tools.rs}`
- `crates/oc-cli/src/{main.rs,cli/cmd/{mod.rs,run/*,serve.rs,acp.rs}}`
- `crates/oc-server/src/handlers/fs.rs` (+ reachability of `oc_server`)
- Reference side: `reference/packages/opencode/src/tool/{shell,read,write,apply_patch,external-directory}.ts`,
  `reference/packages/core/src/tool/{read,read-filesystem}.ts`, `reference/packages/core/src/fs-util.ts`,
  `reference/packages/opencode/src/project/instance-context.ts`

## Commands executed

- `cargo test -p oc-tool` — 80 unit + 14 golden tests pass (0.9s + 0.19s). Includes
  `tool::shell::tests::executes_and_returns_exit_code`, `reports_timeout_metadata`, `tail_truncates`.
- `cargo test -p oc-tool -- --list` — enumerated the 80 tests; **no tests exist** for permission
  enforcement, device/FIFO/socket reads, oversized-file reads, symlink escape, or path traversal.
- `/root/opencode-rs/target/debug/opencode run "hello"` — **RUNTIME**: fails immediately with
  `Error: the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)`.
- `opencode serve --port 37999` + `curl` — binds a TCP socket; `curl` gets `HTTP 000` (bytes discarded).
- `opencode acp --port 37998` + `curl` — same, `HTTP 000`.
- Standalone probe replicating `oc-tool/src/util.rs:fs_contains` + `write_with_dirs` write path —
  evidence in `rust-port-audit/artifacts/11-probe.rs` / `11-probe-output.txt`.

## Runtime scenarios attempted

- **`opencode run` (no attach):** blocked before any tool work — the embedded client is a stub
  (`crates/oc-cli/src/cli/cmd/run/client.rs:65-69`, `LocalClient::create` returns
  `Err("...server is not wired...")`). `execute()` at `run/mod.rs:552-572` only succeeds via
  `--attach` to an **external** server, which never touches Rust tool code.
- **`opencode serve`:** binds a bare `TcpListener` and discards received bytes
  (`run/../serve.rs:40-67`). No HTTP router, no session, no tools.
- **`opencode acp`:** binds a bare listener and blocks (`cli/cmd/acp.rs:17-29`). No ACP bridge.
- **Direct shell/file execution:** only exercised inside `cargo test -p oc-tool` (passes: `echo hello`
  → exit 0, output `hello`; `sleep 5`/`timeout 100` → `<shell_metadata>` timeout note).
- **Symlink workspace escape probe (RUNTIME, standalone):** with `ws/link -> /tmp/.../outside`,
  `fs_contains(ws, ws/link/evil.txt) == true`, and `std::fs::write(ws/link/evil.txt, ...)` lands
  content in `outside/evil.txt`. `symlink_metadata(ws/link/evil.txt).is_file() == true` (intermediate
  symlink components ARE followed by `symlink_metadata`). `/dev/zero` and a FIFO both report
  `is_file()==false`, so the read tool's `is_file` guard rejects them.

## Architecture or behavior summary

- **Tools are NOT reachable from the executable.** The wiring seam the coordinator flagged is real
  and complete on every path:
  - `oc-cli` `run`/`serve`/`acp` never construct a session/runner/tool stack (`run/client.rs:65-69`,
    `serve.rs:40-67`, `cli/cmd/acp.rs:17-29`).
  - `SessionRunnerService`/`RunnerDeps` (`oc-session-runner/src/runner/llm.rs:37-53`) are constructed
    **only in tests** (`tests/runner_loop.rs:408,498`).
  - The runner's `ToolRegistry`/`ToolSettle` traits (`session/services.rs:283-297`) have **no
    production implementation**; only test mocks (`tests/runner_loop.rs:267-302`).
  - `oc-session`'s `ProcessorDeps` (`processor.rs:49-93`) is implemented **only** by `FakeDeps`
    (`processor.rs:823`, tests).
  - `oc-server` is a linked-but-never-imported dependency of `oc-cli`; its axum handlers (incl.
    `handlers/fs.rs`) are dead code from the binary's perspective.
  - The runner's settle path (`runner/llm.rs:486-528`) invokes `materialization.settle(...)` with no
    permission evaluation anywhere in between.
- The `oc-tool` crate is a well-formed, self-contained library: registries assemble the reference
  tool set (`tool/registry.rs:76-132`; golden test `registry_exposes_exact_reference_tool_set`
  passes), argument schemas are validated in `tool::wrap` (`tool/tool.rs:108-123`), and the shell
  tool faithfully reproduces the reference's spawn/truncation/timeout machinery.
- **The approval gate is a recording stub.** `ToolContext::ask` (`model.rs:386-389`) and
  `CoreContext::assert` (`core/tool.rs:42-45`) only push `PermissionRequest`s into a Vec and return
  `Ok`. `util::evaluate`/`oc-session::permission::evaluate` exist as pure functions but nothing in
  the executable path consumes `asks` to deny/allow. The `-y/--dangerously-skip-permissions` flag
  (`run/mod.rs:471`) is therefore not the only "skip" — *every* invocation currently skips.

## Positive observations

- Shell command is passed as a single argument to `<shell> -c` (`tool/shell.rs:434-452`) — no
  argument-injection shell-quoting layer; matches reference `ChildProcess.make(...,{shell})`.
- stdin is `/dev/null` (`tool/shell.rs:438`); stdin is never exposed to the model's command.
- Timeout is enforced (`timeout + 100ms`, `tool/shell.rs:496-524`) with `kill_on_drop(true)` and a
  clear `<shell_metadata>` message; tested and passing.
- Output is bounded: `tail` cap 2000 lines / 50 KiB (`truncate.rs:8-9`), chunked `keep`=2×maxBytes,
  spill to `tool-output/` (`truncate.rs:62-69`); `tail` handles UTF-8 continuation bytes correctly.
- Device files, FIFOs, and sockets are rejected by the read path because
  `symlink_metadata(...).is_file()` is false (`core/read_filesystem.rs:119-130,142-147`,
  `tool/read.rs:80-111`); verified at runtime — `/dev/zero` cannot be read. The reference guards the
  same way via `fs.stat` type checks.
- `rg` is spawned exec-style with `--no-config` and patterns passed as argv, not through a shell
  (`ripgrep.rs:159-209`) — no shell injection via glob/grep input.
- `fs_read` server handler strips `..` and absolute segments (`handlers/fs.rs:23-29`).
- apply_patch verifies hunks before applying and refuses empty patches (`tool/apply_patch.rs:51-66`);
  `code_mode`/`execute` refuses to run (`tool/code_mode.rs:41`); `task` refuses background without
  the experimental flag (`tool/task.rs:122-128`).

## Findings summary (table)

| ID | Severity | Confidence | Title |
|----|----------|------------|-------|
| TOOLS-01 | High | CONFIRMED (RUNTIME) | Tool execution is not reachable from the executable |
| TOOLS-02 | High | CONFIRMED | Permission/approval gate is a record-only stub; no enforcement |
| TOOLS-03 | High | CONFIRMED | Read tools read entire file before pagination → OOM on large files |
| TOOLS-04 | Medium | CONFIRMED (RUNTIME probe) | Symlink component traverses workspace boundary (read + write) |
| TOOLS-05 | Medium | CONFIRMED | Shell kill does not reap the process group/tree |
| TOOLS-06 | Medium | CONFIRMED | `collect()` tokenizer diverges from tree-sitter scan (permission patterns) |
| TOOLS-07 | Medium | CONFIRMED | `rg` runs synchronously, no timeout, full stdout materialized |
| TOOLS-08 | Medium | CONFIRMED | `fs_list` server handler has no `..` sanitization (dead code today) |
| TOOLS-09 | Low | CONFIRMED | Read tools refuse symlinked files (reference follows) — parity gap |
| TOOLS-10 | Low | CONFIRMED | Per-chunk shell metadata accumulates unboundedly during a call |
| TOOLS-11 | Low | CONFIRMED | TOCTOU between stat/containment checks and file operation |

## Detailed findings

### TOOLS-01 — Tool execution is not reachable from the executable — High, CONFIRMED (RUNTIME)
`opencode run` fails before any session work: `LocalClient::create` returns
`"the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)"`
(`crates/oc-cli/src/cli/cmd/run/client.rs:65-69`); `run()` only proceeds with `--attach`, which
points the HTTP client at an external server (`run/mod.rs:552-572`) and never touches Rust tools.
`serve` (`cli/cmd/serve.rs:40-67`) and `acp` (`cli/cmd/acp.rs:17-29`) bind bare sockets that discard
bytes (verified with `curl`: HTTP 000). `SessionRunnerService` is constructed only in tests
(`oc-session-runner/tests/runner_loop.rs:408,498`). Answering the critical questions: (1) the
executable does **not** reach oc-tool; (2) there is **no** path in the current build where untrusted
model output executes arbitrary commands (tests use mocks; the only network surface is a dead socket).

### TOOLS-02 — Approval gate is a record-only stub — High, CONFIRMED
`ToolContext::ask` (`crates/oc-tool/src/model.rs:386-389`) and `CoreContext::assert`
(`crates/oc-tool/src/core/tool.rs:42-45`) push to `ctx.asks` and return `Ok` unconditionally. The
session/runner layers that should consume `asks` are not wired (TOOLS-01); the runner's settle path
(`runner/llm.rs:486-528`) performs no permission evaluation. The pure evaluator
(`oc-session/src/permission.rs:47-62`, `oc-tool/src/util.rs:185-200`) exists but has no caller in any
reachable path. Net effect: the moment the loop is wired, model tool calls execute without user
approval. The shell/read/write/apply_patch tools do emit correctly-shaped `PermissionRequest`s
(`tool/shell.rs:136-157`, `core/read.rs:94-115`, `tool/write.rs:76-84`), so the *protocol* side is
present; only enforcement is missing.

### TOOLS-03 — Read tools read the whole file before pagination — High, CONFIRMED
`core/read_filesystem.rs:149` (`std::fs::read(&real)`) and `tool/read.rs:375-376` (`read_lines`)
materialize the entire file in memory before any page/limit logic. The reference streams and stops at
the byte cap (`reference/packages/core/src/tool/read-filesystem.ts:284-310`;
`reference/packages/opencode/src/tool/read.ts:137-180`, early `ReadStop`). A model-addressable
multi-GB file inside the workspace (logs, dumps) would allocate its full size → memory exhaustion.
No size guard exists prior to the read (`core/read_filesystem.rs:149-152`).

### TOOLS-04 — Symlink component traverses the workspace boundary — Medium, CONFIRMED (probe)
Containment is purely lexical: `fs_contains` (`crates/oc-tool/src/util.rs:36-44`) uses
`std::path::absolute` (does not resolve symlinks), and `path_resolve` (`util.rs:61-67`) joins without
canonicalizing. `symlink_metadata` follows intermediate components, so a file reached through a
symlink *inside* the workspace that points outside is `is_file()==true` and passes the read guard
(`core/read_filesystem.rs:142-147`), and writes go straight through (`tool/write.rs:111-119`
`write_with_dirs` → `std::fs::write`). Probe output (`artifacts/11-probe-output.txt`):
`fs_contains(ws, ws/link/evil.txt) = true`, file content landed in the outside directory. Affected:
`read` (V1+V2), `write`, `edit`, `apply_patch` — all skip `external_directory` approval for such
paths. **Parity note:** the reference `FSUtil.contains` (`fs-util.ts:270-277`) and the write tool are
lexical too, but the reference V2 **read** resolves canonically via `LocationMutation.resolve`
(`core/src/tool/read.ts:60-71`) and would prompt; the Rust read does not. So the read side is a
regression versus reference, the write side is parity-faithful.

### TOOLS-05 — Shell kill does not reap the process group/tree — Medium, CONFIRMED
On timeout/abort the port calls `child.kill()` on the direct shell process
(`tool/shell.rs:517,522`) plus `kill_on_drop` (`tool/shell.rs:441`); it never signals the process
group. `sh -c 'sleep 1000 &'` leaves the sleep orphaned after the tool "times out". The reference
spawns `detached:true` (own session) and kills the child with a SIGTERM→SIGKILL grace
(`reference/.../shell.ts:293-310,548-555`); it also fails to reap grandchildren, so this is a shared
limitation — but the Rust port additionally **immediately SIGKILLs** (no grace period) and keeps the
child in opencode's own process group (so a Ctrl-C on opencode delivers SIGINT to running commands).
Divergences worth reconciling during integration.

### TOOLS-06 — Tokenizer approximates tree-sitter — Medium, CONFIRMED
`collect` (`tool/shell.rs:170-241`) splits only on `;`, `|`, `\n` and quotes
(`split_segments`, `tool/shell.rs:243-279`). It does not parse `&&`/`||`, `$(...)`, backticks,
here-docs, or control structures. A compound such as `echo ok && rm -rf "$HOME/x"` is emitted as a
single permission pattern with `always = "echo *"` (`tool/shell.rs:234-238`); an "always allow echo"
grant would auto-approve the destructive suffix. The `TODO(integration)` header
(`tool/shell.rs:8-9`) acknowledges the parity gap. The reference parses with web-tree-sitter
(`shell.ts:257-261`) and splits commands properly. The permission *prompt* is bound to the full
segment string (not the exact executed argv), which is arity-wildcarded — matching reference design,
but the segment boundaries differ.

### TOOLS-07 — `rg` runs synchronously with no timeout and full stdout materialization — Medium, CONFIRMED
`run_rg` (`ripgrep.rs:118-133`) uses `Command::output()` on the caller's executor thread (blocks it),
with no timeout, and converts all stdout to `String` before parsing. A grep/glob over a massive or
hung (network-mounted) tree can stall the runtime or exhaust memory. Per-record cap exists
(`MAX_RECORD_BYTES` 64 KiB, `ripgrep.rs:13`) but only after full stdout is buffered. The reference
spawns `rg` and consumes the JSON stream line-by-line (`reference/packages/core/src/ripgrep.ts`).

### TOOLS-08 — `fs_list` has no traversal sanitization — Medium, CONFIRMED (currently dead code)
`crates/oc-server/src/handlers/fs.rs:53-54` joins the user-controlled `path` query param onto the
location directory with no `..`/absolute filtering (contrast `fs_read`, which filters at `fs.rs:23-29`).
A client could list directories outside the workspace. Only exploitable if the oc-server crate is
ever mounted (today it is not reachable from the binary — TOOLS-01).

### TOOLS-09 — Read refuses symlinked files — Low, CONFIRMED (parity gap)
`symlink_metadata` returns the link itself for the final component, so `is_file()` is false and both
read tools reject or "miss" a direct symlink to a regular file (`tool/read.rs:80,110-111`;
`core/read_filesystem.rs:119-130,142-147`). The reference uses `fs.stat` (follows links) and reads
through symlinks (`read.ts:243`). Functional regression; conservative from a security standpoint but
breaks valid workflows and `list()` silently drops symlinked entries.

### TOOLS-10 — Per-chunk shell metadata accumulates — Low, CONFIRMED
`process_chunk` (`tool/shell.rs:648-651`) pushes a new `Metadata` (~30 KiB `last` preview) for every
8 KiB output chunk into `ctx.metadata`. A long output (e.g. `yes`) retains ~3.75× the output size in
memory for the call's lifetime (transient, freed after the call). Reference does the same per-chunk
`ctx.metadata` (`shell.ts:525-529`) — parity, but still a hostile-input memory amplification vector.

### TOOLS-11 — TOCTOU between checks and use — Low, CONFIRMED
Containment/stat checks use lexical or `symlink_metadata` views, then the filesystem operation
(`read`/`write`/`remove_file`) re-resolves the path (`core/read_filesystem.rs:142-150`,
`tool/write.rs:111-119`, `tool/apply_patch.rs:274-288`). A same-user actor in the workspace can swap
a file for a symlink between check and use. Local-only, low severity.

## Feature or behavior gaps

- Permission enforcement (allow/ask/deny + saved rules + `--dangerously-skip-permissions` semantics)
  is entirely unimplemented in the executable path.
- `task` (subagent) does not spawn a real sub-session: it returns a fabricated
  "TODO(integration): subagent result text." result (`tool/task.rs:157-193`). Critical question 4:
  **no** real sub-session is spawned.
- `serve`/`acp`/`run` in-process server, LLM loop wiring, and tool-settlement bridging are all
  pending (`TODO(integration)` across `oc-cli`, `oc-server`, `oc-session-runner`).
- No process-group/kill-tree semantics; no size guard before whole-file reads; no symlink-canonical
  containment; no timeout on `rg`.
- `shell.env` plugin hook and `shell` config override (reference `Shell.acceptable(cfg.shell)`) are
  not ported; the port only honors `$SHELL` (`tool/shell.rs:61-63`).

## Test coverage gaps

- No tests for: permission deny/allow evaluation end-to-end; `ctx.ask`/`ctx.assert` enforcement;
  device/FIFO/socket reads; oversized-file reads; symlink traversal; `..` traversal; hidden-file
  handling; unicode-normalized path collisions; null bytes; output-spill DoS; process-tree cleanup;
  `rg` timeout; `collect` on compound commands. Existing tests cover schema snapshots, arity prefix,
  truncation, and happy-path shell execution only.

## Unverified areas

- True end-to-end model→tool execution and the reference's actual approval UX (bun/node runtime is
  absent; the reference binary's `serve` was not driven). Marked BLOCKED for runtime differential
  approval testing.
- Hardlink behavior (two links to one inode) — containment checks treat them as separate paths;
  effect on read/write not runtime-tested.
- Windows/PowerShell branches (`tool/shell.rs:442-449`, `collect` ps/cmd paths) — untestable on this
  Linux host; code inspection only.
- Unicode normalization collisions between user-supplied paths and workspace files — the patch
  module has an NFC/NFD test (`patch::unicode_normalization_matches`) but path resolution itself is
  byte-based.

## Final domain verdict: **NOT_READY**

The agentic tool-execution boundary is currently *inert* (TOOLS-01: nothing reaches oc-tool from the
executable, proven at runtime), so there is no live hostile-input surface today. It is not ready to
be activated, however: the approval gate that is the boundary's primary control is a record-only stub
(TOOLS-02), the read path can be driven to OOM on large files (TOOLS-03), workspace containment is
lexical and bypassable via symlinks (TOOLS-04), and shell/rg process control is incomplete
(TOOLS-05/07). These must be remediated before the session runner is wired to the binary.
