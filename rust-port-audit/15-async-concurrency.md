# Agent 15 — Async, Concurrency, Cancellation, and Backpressure

Auditor: Agent 15. Commit audited: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c` (from `rust-port-audit/00-coordinator-scope.md`).
Date: 2026-08-05. All production source READ-ONLY. Evidence cited as `file:line`.

## Scope

Audited the concurrency architecture of the opencode-rs port: tokio runtime ownership, blocking-in-async,
spawned/detached task ownership, cancellation propagation and safety, channel capacities and backpressure,
lock scope/ordering, deadlock and lost-wakeup potential, slow-consumer behavior (SSE), MCP/LLM retry &
reconnect behavior, plugin FFI reentrancy, process cleanup, and duplicate-work/lost-update races.

Focus crates: `oc-server`, `oc-session-runner`, `oc-llm`, `oc-core`, `oc-mcp`, `oc-plugin`, `oc-tool`,
plus supporting checks in `oc-cli`, `oc-database`, `oc-util`.

## Repository areas inspected

- `oc-session-runner/src/run_coordinator.rs` (per-key serialized drains, wake coalescing, cooperative interrupt)
- `oc-session-runner/src/runner/llm.rs`, `runner/publish_llm_event.rs`, `runner/mod.rs`, `session/services.rs`,
  `execution.rs`, `execution_local.rs`, `retry.rs`
- `oc-llm/src/route/client.rs` (`stream`/`generate`), `route/executor.rs` (retry), `route/transport.rs`, `tool_runtime.rs`, `llm.rs`
- `oc-core/src/bus.rs`, `durable.rs`, `keyed_mutex.rs`, `background_job.rs`, `process.rs`, `credential.rs`
- `oc-server/src/event.rs` (broadcast bus), `sse.rs`, `state.rs`, `server.rs` (shutdown), `mdns.rs`,
  `handlers/session.rs`, `handlers/fs.rs`, `projectors.rs`
- `oc-mcp/src/client.rs`, `transport/stdio.rs`, `transport/http.rs`, `index.rs`
- `oc-plugin/src/host.rs`, `js/runtime.rs`
- `oc-tool/src/core/tool.rs` (`run_future`), `core/bash.rs`, `core/misc.rs`, `core/registry.rs`
- `oc-cli/src/main.rs` (runtime ownership), `oc-database/src/sqlite.rs`, `oc-util/src/glob.rs`
- Reference spec (read-only): `packages/core/src/session/runner/llm.ts`, `packages/core/src/event.ts`,
  `packages/core/src/background-job.ts`, `packages/core/src/session/run-coordinator.ts`

## Commands executed

- `cargo --version`, `rustc --version`
- `grep`/`rg` for `tokio::spawn`, `spawn_blocking`, `block_on`, `run_future`, `Runtime::new`, `Handle::block_on`,
  `CancellationToken`, `cancel`, `interrupt`, `Mutex/RwLock` usage across `crates/`.
- Read tokio 1.53.1 vendored sources: `Handle::block_on` docs (`runtime/handle.rs`), `Notify` (`sync/notify.rs`).
- Read reference: `session/runner/llm.ts`, `core/event.ts`, `core/background-job.ts`.
- `cargo build -q` + execution of two disposable stress harnesses under `/tmp/opencode/` (below).

## Runtime scenarios attempted

1. **`Handle::block_on` inside an async context (tokio 1.53.1)** — `/tmp/opencode/blockon_test`.
   Result: **PANIC** — `Cannot start a runtime from within a runtime. This happens because a function (like block_on)
   attempted to block the current thread while the thread is being used to drive asynchronous tasks.` (runtime proof).
2. **`oc_tool::core::tool::run_future` called from within an async task** — `/tmp/opencode/tool_runfuture` (path-dep on
   `oc-tool`). Result: **PANIC inside the shipped function at `oc-tool/src/core/tool.rs:218:30`** with the same message.
   (runtime proof against production code).
3. **`RunCoordinator` lost-wakeup stress** — `/tmp/opencode/rc_stress` (path-dep on `oc-session-runner`). 6–8 concurrent
   `run()` waiters hammering 4 keys with an instant drain. **HANG reproduced** (run() did not resolve within 2 s while the
   drain completed instantly): `HANG detected at iteration 20713`, `HANG detected at iteration 6670` (2 of 3 runs);
   single-waiter runs (900k iterations) never hung (window is far smaller with one waiter). (runtime proof).
4. **SSE slow-client hammering** — **BLOCKED**: no runnable server binary is wired to the event bus (see Architecture
   summary); the oc-server is a partial in-memory integration and the runner is not connected. Static analysis only.
5. **Plugin JS blocking/DoS** — **BLOCKED** as a live test (would require a hostile plugin + runner); static analysis only.

## Architecture or behavior summary

- **Runtime ownership is single**: `oc-cli/src/main.rs:107-117` builds exactly one multi-thread tokio runtime and
  `block_on`s dispatch. No `#[tokio::main]` appears in lib code. So far, so good.
- **The runner loop is NOT wired end-to-end.** `oc-session-runner` is depended on only by `oc-cli` (no production usage;
  all usages are `#[cfg(test)]`/`tests/`) and by itself. `oc-server` depends on `oc-session`, not `oc-session-runner`;
  `oc-server`'s sessions are in-memory `HashMap` records in a global `tokio::sync::RwLock<Stores>` and `session_prompt`
  just appends a message; no `RunCoordinator::run/wake/interrupt` is ever invoked by the server. The reference's HTTP
  `interrupt → SessionExecution.interrupt → run-coordinator → Effect interrupt → provider stream abort` chain is
  therefore broken in the port: `session_interrupt` only flips `record.active = false` (`oc-server/src/handlers/session.rs:623-639`).
- **Cancellation is cooperative via `CancellationToken`** (run_coordinator, runner/llm.rs `tokio::select!`, tool fibers).
  It is correct at the boundaries that exist, but (a) does not reach the provider HTTP request because events are
  buffered into a `Vec<LLMEvent>` and published only after the whole turn completes, and (b) is never invoked from the
  server. The report's claim "buffered Vec<LLMEvent> loses events published after a mid-stream cancel" is CONFIRMED
  statically against `runner/llm.rs:462-470` (a mid-stream cancel drops the whole buffered turn before any event is
  published) and matches the reference, which streams incrementally (`llm.ts:232-242`).
- **The concurrency primitives that exist** (RunCoordinator, KeyedMutex, BackgroundJob, bus, MCP client) are largely
  idiomatic tokio with the defects enumerated below. There is no `spawn_blocking` anywhere in the runner/llm/tool/
  server/core crates for blocking work (rusqlite/fs/process), and the SQLite-backed `DurableStore` used by the bus is
  not yet wired (in-memory store is the default: `oc-core/src/durable.rs:120-125`).

## Positive observations

- Single runtime ownership in `oc-cli`; axum server shutdown uses `with_graceful_shutdown` (`oc-server/src/server.rs:100-106`).
- `oc-core/src/process.rs` runs stdout/stderr reads concurrently via `tokio::join!` and caps output with
  `append_limited` (bounded buffers) — good practice.
- `oc-tool/src/core/bash.rs:223` sets `kill_on_drop(true)` and explicitly kills on timeout (`bash.rs:245`) — good.
- `oc-mcp/src/client.rs:197-210` applies per-request `tokio::time::timeout`, cleans the `pending` map on timeout and on
  transport close (`client.rs:172-177`) — good.
- `oc-llm/src/route/executor.rs:462-477` bounds retries (`MAX_RETRIES=2`) with jitter and honors `retry-after`.
- `oc-server` SSE uses a bounded tokio `broadcast` (capacity 256, `state.rs:64`), emits heartbeats (`sse.rs:38-42`), and
  per-stream receivers are unregistered automatically when dropped (no SSE task leak).
- `oc-util/src/glob.rs:248` correctly uses `spawn_blocking` for sync glob scanning.
- `run_coordinator` correctly coalesces wakes via `pending_wake` `AtomicBool` and restarts drained entries (`settle`),
  mirroring the reference's intent.

## Findings summary (table)

| ID | Severity | Confidence | Area | Summary |
|----|----------|-----------|------|---------|
| ASYNC-001 | High | CONFIRMED (runtime) | run_coordinator | Lost-wakeup race in `run()`: a waiter can sleep forever; stress harness reproduced a hang |
| ASYNC-002 | High | CONFIRMED (static) | runner/llm | Whole provider turn buffered into `Vec<LLMEvent>`; mid-stream cancel drops all events; no incremental client streaming |
| ASYNC-003 | High | CONFIRMED (runtime) | oc-tool | `run_future` panics ("Cannot start a runtime from within a runtime") when called from within an async task; creates a fresh multi-thread runtime otherwise |
| ASYNC-004 | High | CONFIRMED (static) | server↔runner | HTTP `interrupt` never reaches the RunCoordinator / CancellationToken; runner not wired to server at all |
| ASYNC-005 | Medium | CONFIRMED (static) | oc-core bus | Subscriber lists (`all_subs`, `typed_subs`, `durable_wakes`) never remove dead senders — unbounded growth + O(leaked) wake cost |
| ASYNC-006 | Medium | CONFIRMED (static) | oc-core process | `run()` drops the child future on timeout/cancel without killing the child (no `kill_on_drop`) → orphan processes |
| ASYNC-007 | Medium | CONFIRMED (static) | oc-core background_job | `cancel()` marks status but does not abort the spawned run task; work continues in background |
| ASYNC-008 | Medium | CONFIRMED (static) | oc-server SSE | Lagging SSE clients silently lose events (`Err(Lagged)` filtered out); reference surfaces `SubscriberOverflowError` |
| ASYNC-009 | Medium | CONFIRMED (static) | oc-tool bash | `read_to_end` reads unbounded output then truncates; a noisy command can exhaust memory |
| ASYNC-010 | Medium | CONFIRMED (static) | oc-mcp http | Reconnect loop retries every fixed 1 s forever with no backoff/jitter/cap |
| ASYNC-011 | Low/Med | CONFIRMED (static) | run_coordinator | `interrupt()` can take the owner before `settle` restarts, leaving a fresh drain running uncancelled |
| ASYNC-012 | Low/Med | CONFIRMED (static) | oc-core keyed_mutex | Entry `users` counter leaks when the lock future is aborted mid-await; map grows per cancelled key |
| ASYNC-013 | Low | CONFIRMED (static) | oc-mcp stdio | Child MCP server is orphaned if the client is dropped without `close()` (no `kill_on_drop`, no `Drop`) |
| ASYNC-014 | Low | CONFIRMED (static) | oc-server handlers | Synchronous `std::fs::read`/`read_dir` inside async handlers block a worker thread |
| ASYNC-015 | Low/Med | CONFIRMED (static) | oc-plugin | Plugin JS calls are synchronous, unbounded QuickJS with no timeout; a bad plugin stalls the worker thread |
| ASYNC-016 | Info | CONFIRMED (static) | oc-core bus | All pub/sub channels are unbounded; slow consumers see no backpressure (bounded variant exists but is unused) |
| ASYNC-017 | Info | CONFIRMED (static) | oc-server | Global `RwLock<Stores>` write-serializes all sessions; every mutation is a whole-map write lock |
| ASYNC-018 | Info | CONFIRMED (static) | oc-core bus | `commit_durable` runs projectors/commit-hooks inside the store transaction closure (latency inside the DB tx) |

## Detailed findings

### ASYNC-001 — RunCoordinator lost-wakeup race: waiter can sleep forever (High, CONFIRMED by runtime stress)

`oc-session-runner/src/run_coordinator.rs`.

The waiter loop (`run`, lines 107-119):
```rust
let result = entry.result.lock().unwrap().clone();
if let Some(result) = result { ... return result; }
entry.notify.notified().await;          // registers AFTER the check
```
and `settle` (lines 218-219):
```rust
*entry.result.lock().unwrap() = Some(result);
entry.notify.notify_waiters();          // loses wake if no waiter registered yet
```
`Notify::notify_waiters` stores no permit; a call made before a task registers via `notified().await` is lost
(tokio docs, `sync/notify.rs`). Interleaving: waiter reads `result == None`, releases the mutex; `settle` stores the
result and calls `notify_waiters` (no registered waiter yet); the waiter then calls `notified().await` and sleeps
forever — the loop only re-checks *after* the await returns, so the lost wake is fatal. A new `run()` on the same key
creates a *fresh* `EntryState` with a fresh `Notify`, so the stale waiter is never woken.

Runtime proof (`/tmp/opencode/rc_stress`): 6–8 concurrent `run()` waiters, 4 keys, instant drain — a waiter failed to
resolve within 2 s in 2 of 3 runs (`HANG detected at iteration 20713`, `...6670`); single-waiter (900k iterations)
never hung. The multi-waiter case is exactly the reference's "join the active execution" scenario (two resumes on the
same session). Severity High: a prompt/resume can hang indefinitely under a narrow-but-reproducible interleaving.
The correct pattern is to register the `Notified` future before the check (or hold a lock across the check+await, or
use `notify_one` permit semantics).

### ASYNC-002 — Provider turn fully buffered; mid-stream cancel drops everything (High, CONFIRMED static)

`oc-session-runner/src/runner/llm.rs:459-484`. The `LlmClient` trait returns `Vec<LLMEvent>`
(`session/services.rs:311-315`), and `run_turn_attempt` wraps the whole stream in
```rust
let events_result = tokio::select! {
    _ = token.cancelled() => { stream_interrupted = true; None }
    result = self.deps.llm.stream(request.clone()) => Some(result),
};
if let Some(Ok(events)) = &events_result { for event in events { publisher.publish(&event, &[]).await ... } }
```
- No event is published until the ENTIRE provider turn completes. The reference streams incrementally
  (`reference/.../runner/llm.ts:232-242` `llm.stream(request).pipe(...) yield* publish(event)`), so clients would see
  tokens in real time; the port shows nothing until the provider finishes. This is both a latency/UX deviation and a
  parity break.
- A mid-stream cancel (`token.cancelled()` wins the `select!`) discards the buffered events — **events that would have
  been published are lost** (the report's claim is confirmed). The partial assistant text the reference would have
  streamed and then closed with `Step.Failed` is never emitted; the port instead emits `fail_assistant("Provider turn
  interrupted")` from an empty state (`llm.rs:636-641`).
- Cancellation reaches the network only by dropping the stream future. Whether the not-yet-written production adapter
  (no `impl LlmClient` outside tests exists) aborts the reqwest body on drop is UNVERIFIED; a future that collects the
  `BoxStream` on a spawned task would keep the HTTP request running after "interrupt".

### ASYNC-003 — `run_future` panics inside an async context (High, CONFIRMED by runtime proof)

`oc-tool/src/core/tool.rs:216-223`:
```rust
pub fn run_future<T>(future: BoxFuture<'_, Result<T, ToolError>>) -> Result<T, ToolError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(future),          // <- panics inside an async task
        Err(_) => tokio::runtime::Runtime::new().expect(...).block_on(future),  // <- fresh multi-thread pool per call
    }
}
```
`Handle::block_on`'s documented contract (tokio 1.53.1 `runtime/handle.rs`): **panics** when called from within an
asynchronous context. Runtime proof against the shipped function: `/tmp/opencode/tool_runfuture` panics at
`tool.rs:218:30` — `Cannot start a runtime from within a runtime...`. Callers: `core/bash.rs:176` (the `bash` tool),
`core/misc.rs:109,124,347` (fetch/provider tools), `core/bash` via `poll`/`settle` (`core/tool.rs:203-213`). The session
runner's tool-settlement fibers run inside tokio tasks (`runner/llm.rs:529-557`), so the first tool execution that
reaches this path will panic (observed in the runner as a JoinError → "tool execution interrupted", `llm.rs:735-737`,
masking the real cause). Off-runtime threads instead pay for a brand-new multi-thread runtime (one thread pool per tool
call). Fix: spawn the future and `.await` it (or `spawn_blocking` + `Handle::block_on` from the blocking thread).

### ASYNC-004 — HTTP interrupt never reaches the runner (High, CONFIRMED static)

`oc-server/src/handlers/session.rs:623-639`: `session_interrupt` sets `record.active = false` on the in-memory record
and returns 204. It never calls `RunCoordinator::interrupt` / `SessionExecution::interrupt`
(`execution_local.rs:74-76`), and `oc-server` does not depend on `oc-session-runner` at all (`oc-server/Cargo.toml` has
only `oc-session`). Meanwhile `oc-cli` depends on `oc-session-runner` but has zero production usage of it. So the entire
cooperative-cancellation machinery (`CancellationToken` in `runner/llm.rs:464`, tool fibers `llm.rs:530-531`, and
`RunCoordinator::interrupt`) is currently unreachable from any user-facing path. The reference chain
(`session/execution.ts` → `run-coordinator.ts` interrupt) is not wired. Marked High because it is a functional
cancellation gap, but it is an integration-gap manifestation; downgrade if the runner integration is planned separately.

### ASYNC-005 — Event bus subscriber leak / unbounded growth (Medium, CONFIRMED static)

`oc-core/src/bus.rs`:
- `subscribe` (177-187) and `subscribe_all` (190-194) push `mpsc::UnboundedSender` into `all_subs`/`typed_subs` and
  never remove them when the receiver is dropped; `notify` (477-492) clones and sends to every entry, silently ignoring
  `Err` (dead senders). Over a long-running process, each dropped stream leaks one sender and every publish pays
  O(leaked subscribers).
- `durable()` (217-236) spawns a task that registers a `watch::Sender` in `durable_wakes` (`subscribe_durable`,
  238-248) and never removes it on exit; `wake_durable` (250-262) sends to the whole (growing) list on every commit.
The reference removes subscriptions via scope finalizers (`reference/core/event.ts:162`, `allBounded` addFinalizer).
The port has no equivalent cleanup. (The SSE layer does not use this bus — it uses oc-server's broadcast — so the leak
is in the oc-core bus consumer path, currently used by the runner's projector/durable streams once wired.)

### ASYNC-006 — Timed-out / cancelled subprocesses are orphaned (Medium, CONFIRMED static)

`oc-core/src/process.rs:122-260`: `run()` wraps the whole `run_fut` in `timeout_at`; on timeout the future (and the
`tokio::process::Child`) is dropped without `kill_on_drop(true)` and without an explicit `kill()`. tokio drops a
`Child` without killing by default, so the timed-out command keeps running as an orphan. The same applies if the
caller cancels the future mid-run (no cancellation hook). Note the tool-level `bash` (`oc-tool/src/core/bash.rs:223`)
does set `kill_on_drop(true)`, so the gap is specific to the `AppProcess.run` git wrapper path (`git.rs:178-180,296...`)
and any future shell integration that reuses `oc-core::process`.

### ASYNC-007 — `BackgroundJob::cancel` does not stop the work (Medium, CONFIRMED static)

`oc-core/src/background_job.rs:356-377`: `cancel()` flips status to `cancelled` and resolves waiters, but the spawned
run task (`start` at 187-191, `extend` at 222-229) is never aborted; `settle` (380-435) sees status != running and
discards the result, but the `run` closure keeps executing in the background (e.g., a long shell operation). The
reference interrupts the job's scope: `background-job.ts:356` `Scope.close(result.scope, Exit.void)`. Also, two
concurrent `start()` calls with the same id (139-147, 184) can both pass the check and the second `insert` orphans the
first job's channels (its `wait`ers never resolve) — duplicate-work race (Low).

### ASYNC-008 — SSE slow clients silently drop events (Medium, CONFIRMED static)

`oc-server/src/sse.rs:53-59` filters `Err(_)` from `BroadcastStream`, which is exactly `RecvError::Lagged`: a client
that cannot keep up silently misses events with no signal. The reference's per-subscriber bounded queue fails with
`SubscriberOverflowError` (`reference/core/event.ts:152-164`). The oc-server broadcast is bounded (256) so producers
never block (no backpressure on publishers — arguably fine), but consumers get no overflow notice, and a stale
`Err(Lagged)` stream position means the client can desync permanently (e.g., missed `step.ended`). Heartbeats and
receiver cleanup on disconnect are correct (`sse.rs:37-42`; broadcast receiver drop unregisters).

### ASYNC-009 — `bash` tool reads unbounded output (Medium, CONFIRMED static)

`oc-tool/src/core/bash.rs:234-239`: `tokio::io::AsyncReadExt::read_to_end(&mut out, ...)` accumulates the *entire*
stdout/stderr into a `Vec` before the 1 MiB `MAX_CAPTURE_BYTES` truncation is applied (251-259). A command producing
multi-GB output exhausts memory instead of truncating while streaming (compare `oc-core/src/process.rs`'s
`append_limited`).

### ASYNC-010 — MCP HTTP reconnect loop: fixed 1 s retries forever (Medium, CONFIRMED static)

`oc-mcp/src/transport/http.rs:195-232`: on stream failure the loop calls `open_stream_again` and on any non-401 error
sits `sleep(1s)` and retries indefinitely — no exponential backoff, no jitter, no attempt cap. With many MCP servers
unreachable this becomes a steady per-server 1 Hz polling storm, and when the server returns all clients reconnect
simultaneously (thundering herd).

### ASYNC-011 — `RunCoordinator::interrupt` can miss a restarted drain (Low/Med, CONFIRMED static)

`run_coordinator.rs:139-150`: `interrupt` takes `entry.owner` (line 142) *before* setting `stopping` (line 144). If the
drain completes between the `take` and the `stopping.store`, `settle` may observe `stopping == false` and restart the
entry (`start_entry`, 197-200), storing a NEW owner. `interrupt` then cancels the OLD token and awaits the OLD
(already-completed) owner, returning while a fresh drain (with an uncancelled token) keeps running. TOCTOU window is
narrow but real. Fix: set `stopping` before taking the owner.

### ASYNC-012 — `KeyedMutex` entry leak on cancellation (Low/Med, CONFIRMED static)

`oc-core/src/keyed_mutex.rs:37-62`: `users` is incremented before `acquire().await` and decremented only after the
`effect.await` completes. If the calling task is aborted while waiting on the semaphore or mid-effect (cancellation,
`abort_all`), `users` is never decremented and the entry (plus semaphore Arc) is never removed — the map grows one
entry per cancelled key. Functionality remains correct (semaphore permits are released on guard drop), so this is a
bounded memory leak, not a deadlock.

### ASYNC-013 — MCP stdio child orphaned if not explicitly closed (Low, CONFIRMED static)

`oc-mcp/src/transport/stdio.rs:61-68` spawns without `kill_on_drop`; `close()` (158-169) kills and reaps, but there is
no `Drop` implementation, so dropping a client/transport without calling `close()` (panic paths, early returns) leaves
the MCP server process running. Call sites in `index.rs` do call `close()`, so this is a robustness gap, not a leak in
the normal path.

### ASYNC-014 — Synchronous filesystem calls in async handlers (Low, CONFIRMED static)

`oc-server/src/handlers/fs.rs:30,56,86` and `instance_handlers.rs:911,938` call `std::fs::read` / `std::fs::read_dir`
directly in axum async handlers, blocking a tokio worker thread per request. For large files this stalls unrelated
requests on the same worker. Prefer `tokio::fs` or `spawn_blocking`.

### ASYNC-015 — Plugin JS execution: unbounded, synchronous, no timeout (Low/Med, CONFIRMED static)

`oc-plugin/src/host.rs:110-212`: `trigger`/`event`/`config`/`execute_tool` are synchronous QuickJS calls
(`js/runtime.rs:247-267` `JS_Eval`, `call_function`, `pump_jobs` 279-288 which drains promise jobs to completion).
`LoadedPlugin` is documented not `Send`/`Sync` (`host.rs:98-104`), so plugin execution must be confined to one thread,
and each hook call blocks that thread for the whole JS evaluation — including `while(true)` or a never-settling promise —
with no timeout or interrupt path. Invoked from an async handler on a tokio worker, one hostile plugin stalls the
runtime worker indefinitely. (The reference has the same single-loop property on Bun, so this is parity-adjacent, but
the port has no guard.)

### ASYNC-016 — Unbounded event channels, no backpressure (Info, CONFIRMED static)

`oc-core/src/bus.rs:69-70,178,191,218`: every subscriber channel is `mpsc::unbounded_channel`. A slow consumer
(projector, durable-stream listener) grows memory without bound; the bounded variant `all_bounded` (420-430) drops
overflow silently and is not used by the runner path.

### ASYNC-017 — Global session-store RwLock serializes all sessions (Info, CONFIRMED static)

`oc-server/src/state.rs:52-58` + `handlers/session.rs:396,475,508,526,631`: every mutation takes the write lock on the
single process-wide `Stores` map, so a burst of prompts across different sessions serializes all of them; also readers
(`session_list`, `session_get`) take the read lock across cloning. Per-key or sharded locking would reduce contention
(and is what the reference's run-coordinator achieves per Session).

### ASYNC-018 — Projectors/commit-hooks run inside the store transaction (Info, CONFIRMED static)

`oc-core/src/bus.rs:620-625` invokes `projector` and `commit_hook` closures *inside* the `transaction` closure. If a
projector is slow (or does I/O), the whole aggregate's commit path is serialized behind it; the reference wraps commit
in `Effect.uninterruptible` (`reference/core/event.ts:237-239`) and keeps the DB transaction tight.

## Feature or behavior gaps

- Incremental streaming: the runner buffers a whole provider turn before publishing (`ASYNC-002`); there is no
  token-by-token `session.next.text.delta` stream to clients.
- Interrupt end-to-end: no HTTP path reaches the CancellationToken (`ASYNC-004`); `interrupt` semantics ("cooperative
  interrupt") are implemented but unreachable.
- Process cancellation: `oc-core::process` has no kill-on-drop/timeout-kill (`ASYNC-006`); background jobs can't be
  truly cancelled (`ASYNC-007`).
- SSE overflow signaling: no `SubscriberOverflowError` equivalent (`ASYNC-008`).
- MCP reconnect: fixed 1 s forever, no backoff/cap (`ASYNC-010`).

## Test coverage gaps

- `run_coordinator.rs` has **zero unit tests** despite being the most concurrency-sensitive module; the hang in
  ASYNC-001 is not exercised by `tests/runner_loop.rs` (which drives `SessionRunnerService::run` directly, never the
  coordinator).
- No test calls `oc-tool::core::tool::run_future` from inside a tokio context (the panic in ASYNC-003 is untested;
  only schema golden tests exist for bash).
- No concurrency tests for: bus subscriber cleanup/leaks, `KeyedMutex` cancellation, SSE lag/overflow, MCP
  reconnect, background-job cancel-of-running-work, `interrupt`-vs-restart races.
- No `spawn_blocking`/blocking-in-async lint or tests.

## Unverified areas

- Provider-network cancellation on stream drop (ASYNC-002): the production `impl LlmClient` that adapts
  `oc-llm`'s `BoxStream` into the runner's `Vec` does not exist; whether dropping the buffering future aborts the
  reqwest connection is **BLOCKED/UNVERIFIED**.
- SQLite-backed `DurableStore` in async context: `oc-database::Sqlite` is a synchronous `Mutex<Connection>`
  (`sqlite.rs:249-378`) with no `spawn_blocking` wrapper, and nothing wires it to the async `DurableStore` trait yet —
  **UNVERIFIED** how DB work would run once integrated.
- SSE slow-client hammering scenario: **BLOCKED** (server not runnable against the runner; in-memory stores only).
- Plugin blocking DoS: **BLOCKED** (needs a live plugin+runner).

## Final domain verdict

**NOT_READY** for the concurrency/cancellation domain in its current wiring state.

Rationale: three High-severity defects are confirmed — one with a reproduced runtime hang in the shipped
`RunCoordinator` (ASYNC-001), one with a reproduced panic in shipped `run_future` (ASYNC-003), and one behavioral
regression that discards an entire provider turn on mid-stream cancel (ASYNC-002) — plus the end-to-end interrupt
chain being absent (ASYNC-004). The primitives are generally idiomatic, but the most safety-critical component
(the coordinator) is both racy and entirely untested, and blocking work has no `spawn_blocking` discipline. Because
much of the runner/server plumbing is still `TODO(integration)`, several items are marked UNVERIFIED rather than
confirmed, but the confirmed defects are sufficient to gate on remediation.
