# Plan 07 — Session lifecycle, runner, cancellation, concurrency

Agent 07 · Wave 0 read-only planning. Repo `/root/opencode-rs` @ `fix/audit-remediation`.
Domain: session lifecycle ops, `oc-session-runner` wiring, RunCoordinator correctness,
cancellation propagation, partial persistence, recovery, bus/keyed-lock cleanup.

---

## 1. Owned findings

| ID | Sev | Evidence | Status |
|----|-----|----------|--------|
| SESSION-001 | Critical | `oc-cli/src/cli/cmd/run/client.rs:65-69` `LocalClient::create` always errors; `run/mod.rs:560-571` only local branch. No provider request ever made (mock OpenAI trace empty). | CONFIRMED (runtime) |
| SESSION-002 | Critical | `oc-cli/src/cli/cmd/session.rs:9-18`, `export_cmd.rs:7-13`, `import_cmd.rs:33-55` all `not_wired`. | CONFIRMED (runtime) |
| SESSION-003 | Critical | `serve.rs:40-67` binds bare TCP drain; never HTTP. `oc_server::server::listen` (axum) exists but uninvoked. | CONFIRMED (runtime) |
| SESSION-004 | Critical | No wired Rust path persists. `oc-server/state.rs:26-48` in-memory `HashMap` stores. `oc-database` CRUD has no production caller. | CONFIRMED (runtime) |
| SESSION-005 | High | `run/client.rs:412-455` `sse_stream` returns after first event per chunk; coalesced events dropped. | CONFIRMED (runtime) |
| SESSION-006 | High | `oc-session-runner` referenced only by `oc-cli` Cargo.toml; zero production call sites. `RunnerDeps` satisfied only by mocks in `tests/runner_loop.rs`. | CONFIRMED (static) |
| SESSION-007 | High | `oc-server/src/handlers/session.rs:371-429` `session_prompt` appends to vec; no runner, no events, no persistence. | CONFIRMED (static) |
| SESSION-008 | Medium | No recovery-after-crash design: no durable input replay, no resume sweep. | CONFIRMED (static) |
| SESSION-009 | Medium | `session_share` reads `data.url`; reference returns `data.share.url`. | CONFIRMED (static) |
| SESSION-010 | Medium | `RwLock<HashMap>` whole-store write serialization; seq = `messages.len()` (memory-reset). | CONFIRMED (static) |
| SESSION-011 | Medium | Rust delete stub skips `ses_` prefix validation the reference enforces. | CONFIRMED (static) |
| SESSION-012 | Low | Server title `"New Session"` vs reference `"New session - <ISO>"`; `oc-session/src/service.rs:14-68` already implements it — handler doesn't call it. | CONFIRMED (static) |
| TOOLS-001 | High | `ToolRegistry::materialize` / `ToolSettle::settle` (`oc-session-runner/src/session/services.rs:283-297`) are the settlement seam; no production impl, no permission enforcement. Runner tool fibers (`runner/llm.rs:529-557`) unreachable. | CONFIRMED (static) |
| ASYNC-001 | High | `run_coordinator.rs:107-119` check-then-`notified().await`; `settle:218-219` `notify_waiters` stores no permit → lost wake → hang. Reproduced (2/3 runs). | CONFIRMED (runtime) |
| ASYNC-002 | High | `runner/llm.rs:459-484` whole turn buffered into `Vec<LLMEvent>`; published only after stream completes. Mid-stream cancel (select at `:464-470`) discards everything → `fail_assistant` from empty state (`:636-641`). Reference streams incrementally (`llm.ts:232-242`). | CONFIRMED (static) |
| ASYNC-003 | High | `oc-tool/src/core/tool.rs:216-223` `run_future` panics in async context (`Handle::block_on`). Runner tool fibers will hit it → JoinError → masked as "tool execution interrupted" (`runner/llm.rs:735-737`). Owned by Agent 09; runner must not call it from async. | CONFIRMED (runtime) |
| ASYNC-004 | High | `handlers/session.rs:619-640` `session_interrupt` flips `record.active`; never calls `RunCoordinator::interrupt` / `SessionExecution::interrupt`. `oc-server` does not depend on `oc-session-runner`. | CONFIRMED (static) |
| ASYNC-005 | Medium | `oc-core/src/bus.rs:177-194` `subscribe`/`subscribe_all` push `UnboundedSender` into `all_subs`/`typed_subs`, never removed; `durable()` `:217-236` + `subscribe_durable` `:238-248` leak `watch::Sender`s in `durable_wakes`; `wake_durable` `:250-262` sends to whole growing list. | CONFIRMED (static) |
| ASYNC-011 | Low/Med | `run_coordinator.rs:139-150` `interrupt` takes owner before setting `stopping`; settle may restart a fresh drain racing the interrupt. | CONFIRMED (static) |
| ASYNC-012 | Low/Med | `oc-core/src/keyed_mutex.rs:37-62` `users` decremented only after effect; abort mid-await leaks entry. | CONFIRMED (static) |
| — (retry) | High | `oc-session-runner/src/retry.rs` policy (exponential backoff, `retry-after`, context-overflow never retried) is implemented + unit-tested but never invoked by `runner/llm.rs` or any wired path. Reference applies it in `SessionRunnerModel`/LLM executor. | CONFIRMED (static) |

Scope note: `run_future` (report ASYNC-003 / JSON ASYNC-002) belongs to Agent 09 (oc-tool); listed as a blocking dependency for runner wiring, not owned here. SSE `Err(Lagged)` (report ASYNC-008), `background_job` cancel (ASYNC-007), process kill-on-drop (ASYNC-006) belong to oc-server/oc-core agents; only the oc-core bus/keyed-mutex cleanup is owned here per dispatch.

---

## 2. Files to change

Owned (this agent executes):
- `crates/oc-session-runner/src/run_coordinator.rs` — lost-wakeup fix (ASYNC-001), interrupt-before-stopping fix (ASYNC-011), unit tests.
- `crates/oc-session-runner/src/session/services.rs` — `LlmClient::stream` returns a stream (not `Vec`); add permission decision to `ToolSettle`; document DB-backed trait contracts.
- `crates/oc-session-runner/src/runner/llm.rs` — incremental event loop over the stream (ASYNC-002), cancellation-aware stream consumption, retry hookup, wire `fail_interrupted_tools`.
- `crates/oc-session-runner/src/execution.rs` / `execution_local.rs` — no signature change; add recovery sweep entry point.
- `crates/oc-session-runner/tests/coordinator.rs` — add lost-wakeup + interrupt-race stress tests.
- `crates/oc-session-runner/tests/runner_loop.rs` — incremental-stream + mid-stream-cancel tests.
- `crates/oc-core/src/bus.rs` — subscriber/subscription cleanup (ASYNC-005).
- `crates/oc-core/src/keyed_mutex.rs` — Drop-guard cleanup (ASYNC-012).
- `crates/oc-core/src/durable.rs` — wire `SQLiteDurableStore` when Agent 03 lands it (verify async wrapper).

Co-authored / integration (needs dependency agents, do NOT edit alone):
- `crates/oc-server/src/handlers/session.rs`, `state.rs`, `sse.rs`, `event.rs`, `router.rs` — real stores + `session_interrupt` → `SessionExecution::interrupt` (Agent 10 owns server wiring; this plan defines the contract).
- `crates/oc-cli/src/cli/cmd/run/client.rs` (`LocalClient::create`), `run/mod.rs`, `session.rs`, `export_cmd.rs`, `import_cmd.rs`, `serve.rs` — wire lifecycle ops + local client (Agent 02 CLI + Agent 10 server).
- `crates/oc-database/*` — session_input/session/message/part CRUD + `SessionDb` impl (Agent 03).

Reference spec to mirror: `packages/core/src/session/run-coordinator.ts`, `runner/llm.ts`, `execution/local.ts`, `store.ts`, `packages/core/src/event.ts` (finalizer cleanup), `packages/opencode/src/session/session.ts` (lifecycle ops), `packages/server/src/handlers/session.ts` (interrupt endpoint).

---

## 3. Session lifecycle operations to implement (real stores, Agent 03)

Wire `oc-cli/src/cli/cmd/session.rs`, `export_cmd.rs`, `import_cmd.rs` against
`oc-session::service::SessionService` over an `oc-database` `SessionDb` impl:

- create (`CreateInput`, ISO title via `service::create_next`), get, list (paging `maxCount`), delete (cascade messages/parts; `identifier::with_given`/`ascending` `ses_` validation — SESSION-011), update: `set_title` (SESSION-012 title source), `set_agent_model`, `set_metadata`, `set_permission`, `set_revert`, `set_archived`, `touch`.
- fork (`service::fork` + message-boundary cloning), resume/continue (`--session X` / `--continue` → `SessionExecution::resume`), share (`data.share.url` extraction — SESSION-009), export/import (event JSON round-trip via `EventBus.replay`/`replay_all`).
- compaction: run `oc-session/src/compaction.rs` pipeline (already implemented, unit-tested) triggered from the runner's `SessionCompaction` impl over Agent 03 DB (see §6).
- retry: invoke `oc-session-runner/src/retry.rs` policy in the `SessionRunnerModel`/LLM-executor adapter (Agent 02/06) and surface retry notices as session events.
- recovery (SESSION-008): see §6.

Lifecycle ops are orchestrated in `oc-cli`/`oc-server`; the pure model logic already exists in `oc-session` (service.rs, session.rs, v1.rs, v2.rs). The work is wiring + DB impl.

---

## 4. Runner's new dependency contract (traits)

`oc-session-runner/src/session/services.rs` holds the contracts. Changes:

1. **LlmClient** (owned) — `stream` must become streaming, aligned to `oc-llm::route::client::LlmClient::stream` (Agent 06):
   ```rust
   pub trait LlmClient: Send + Sync {
       fn stream(&self, request: LLMRequest)
           -> Pin<Box<dyn Future<Output = Result<BoxStream<'static, Result<LLMEvent, LLMError>>, LLMError>> + Send + '_>>;
   }
   ```
   A `Result<_, _>` outer (compile/route errors) plus a `BoxStream` of events preserves
   both failure modes. Agent 06's adapter maps `oc_llm::LlmEvent` → this crate's `LLMEvent`
   (`crates/oc-session-runner/src/llm/event.rs`), mirroring `packages/llm/src/schema/events.ts`.
   Contract requirement on the adapter: dropping the stream future MUST abort the underlying
   reqwest body stream (do not buffer on a spawned task) — otherwise interrupt is cosmetic.
   Retry: Agent 02/06 must run `oc-session-runner/src/retry.rs` policy in the executor.

2. **SessionStore / SessionInput / SessionHistory / SessionContextEpoch / SessionCompaction** —
   one DB-backed impl each (Agent 03 + `oc-session`). `SessionStore` already matches
   `oc-session::store::SessionDb` semantics; `SessionInput` needs durable `session_input`
   rows (promote_steers/promote_next_queued/latest_sequence/has_pending); `SessionCompaction`
   calls `oc-session` compaction over DB tables.

3. **ToolRegistry / ToolSettle** (TOOLS-001 seam, Agent 09 + Agent 08) — keep
   `materialize(permissions) -> Option<ToolMaterialization>` and `settle(ExecuteInput)`. Add
   explicit authorization: `settle` must first consult the permission service (Agent 08) and
   return `ToolSettlementError::Declined` for a denied tool, mirroring `PermissionV2.DeclinedError`
   → user-declined halts the loop (`runner/llm.rs:621-628` already handles Declined). Agent 09
   must NOT call `oc-tool::core::tool::run_future` from async (ASYNC-003) — spawn + await.

4. **SessionRunnerModel** (Agent 02) — resolve `SessionInfo` → model via oc-provider; apply retry policy; returns `ModelError` already typed.

5. **EventBus** — production impl forwards `SessionEvent` to `oc_core::EventBus::publish` with a
   durable `Definition` (Agent 03) so each event commits atomically (see §6). Existing trait is fine.

6. **LocationService / Agents / SystemContextRegistry / SkillGuidance / ReferenceGuidance / Snapshots** — as specced today; impls from Agent 02/03.

7. **DB** — `oc-session::store::SessionDb` impl over `oc-database` (Agent 03); `DbSessionStore` exists and is ready.

---

## 5. Lost-wakeup fix (ASYNC-001) + interrupt race (ASYNC-011)

### Fix for `run()` waiter loop
Current (buggy): read `result` then `notified().await` — `notify_waiters` after the read is lost.
Fix: register the `Notified` future BEFORE re-reading the result each iteration:

```rust
loop {
    let notified = entry.notify.notified();   // registers a waiter NOW
    let stopping = entry.stopping.load(Ordering::Acquire);
    let result = entry.result.lock().unwrap().clone();
    if let Some(result) = result {
        if stopping { break; }                // hand off to fresh execution (mirrors ref run())
        return result;
    }
    notified.await;
}
```

Invariant: registration precedes the check, so any `notify_waiters` either happens while this
waiter is registered (wakes it) or before the check (the stored `result` is observed). Keeps the
multi-waiter `notify_waiters` join semantics (multiple resumes on one key all wake and return the
shared result). Do NOT switch to `notify_one` — it stores only one permit and would strand the
other waiters of a joined resume.

### Fix for `interrupt()` vs `settle()` restart (ASYNC-011)
Order operations so `stopping` is set before the owner is taken, and make `start_entry` self-defensive:

```rust
pub async fn interrupt(&self, key: K) {
    let entry = self.active.lock().unwrap().get(&key).cloned();
    let Some(entry) = entry else { return };
    entry.stopping.store(true, Ordering::Release);   // FIRST
    entry.pending_wake.store(false, Ordering::Release);
    if let Some(cancel) = entry.cancel.lock().unwrap().clone() { cancel.cancel(); }
    let owner = entry.owner.lock().unwrap().take();
    if let Some(owner) = owner { let _ = owner.await; }
}
```
In `start_entry`, after storing the token/owner, re-check `entry.stopping` and cancel the fresh
token if it is already set — closes the window where `interrupt` runs between entry creation and
owner/token storage. This mirrors reference `interrupt` (`run-coordinator.ts:94-101`) which sets
`stopping` before `Fiber.interrupt`.

---

## 6. Cancellation propagation + partial persistence + recovery

### Interrupt endpoint → token (ASYNC-004)
Chain (owned contract, Agent 10 wires):
`session_interrupt` (server) → `SessionExecution::interrupt(session_id)` → `RunCoordinator::interrupt`
→ per-entry `CancellationToken.cancel()` → drain observes token.
- `LocalExecution` already routes `interrupt → coordinator.interrupt` (`execution_local.rs:74-76`).
- `oc-server` gains an `Arc<dyn SessionExecution>` in `AppState` (single process-global instance,
  per `core/src/session/execution/local.ts`). `session_interrupt` calls it and returns 204; idle or
  missing session is a no-op (reference parity).
- The drain (`runner/llm.rs`) must observe the token at three points: (a) stream consumption, (b)
  tool-settlement fibers (`:529-557` already select on token), (c) before starting a new provider
  turn. `RunCoordinator::interrupt` awaits the drain's cleanup so the 204 returns only after the
  run settles (reference `Fiber.interrupt`).

### Incremental publishing / partial persistence (ASYNC-002, SESSION-004/007/008)
Replace the buffered turn with an incremental loop over the `BoxStream` (mirrors reference
`llm.ts:232-242` `Stream.runForEach`):

```rust
let mut stream = Box::pin(self.deps.llm.stream(request.clone()).await?);
loop {
    tokio::select! {
        _ = token.cancelled() => { stream_interrupted = true; break; }
        next = stream.next() => match next {
            None => break,
            Some(Ok(event)) => { /* same per-event logic as today's for-loop; spill tool-call fibers; break on provider_error/overflow */ }
            Some(Err(e)) => { stream_error = Some(e); break; }
        }
    }
}
```
- Each event is published (`publisher.publish`) as it arrives → text/reasoning/tool deltas persist
  incrementally through the durable `EventBus` (below). `publisher.flush()` at stream end flushes
  only still-open fragments (matching reference).
- Mid-stream cancel: already-published events stay; `fail_unsettled_tools` + `fail_assistant`
  (if active) emit `Step.Failed` from the real partial state (reference `llm.ts:302-310`), not from
  an empty state. The `stream_interrupted`/`Interrupted` outcomes feed `TurnFailure::Interrupted`
  as today (`llm.rs:198,228`), which returns cleanly from the drain.
- Durable commit: the production `EventBus` impl publishes each `SessionEvent` as a durable event
  with `aggregateID = session_id` through `oc_core::EventBus::publish` → transactional insert
  (`bus.rs:496-653`). Projectors (Agent 03) fold events into session/message/part tables. A crash
  mid-stream leaves the partial assistant message + durable `session_input` rows in SQLite.
- `session_prompt` (server) becomes: admit one durable `session_input` row (delivery=steer/queue) →
  `SessionExecution::wake(session_id)` (advisory) — mirroring the V2 contract in `reference/AGENTS.md`
  and `session.ts:prompt`. No direct message append; the runner promotes at the safe boundary.

### Recovery after process termination (SESSION-008)
- Durable inputs make resume deterministic: on startup, `oc-cli` runs a recovery sweep over sessions
  with unpromoted/promoted-but-unsettled `session_input` rows or a dangling running state, calling
  `LocalExecution::resume` per session. `SessionRunner::run` drains eligible inputs and reconciles
  pending/running tools via `fail_interrupted_tools` (`llm.rs:245-286`).
- Conservative rule (reference `AGENTS.md`): only resume sessions with unconsumed durable input;
  never blind-retry provider work. Mark the sweep as a separate, explicitly-reviewed slice — it is
  the "post-crash continuation recovery" the reference leaves open.

### Bus / keyed-lock cleanup (ASYNC-005, ASYNC-012)
- `bus.rs`: wrap each `mpsc::UnboundedSender` / `watch::Sender` in a handle that removes itself on
  drop (like the reference's finalizers in `core/event.ts:162`). As a safety net, `notify`
  (`:463-493`) prunes closed senders (`tx.is_closed()`) and `wake_durable` (`:250-262`) prunes
  `watch::Sender`s whose receivers are gone (`send` returning `Err`). This bounds list growth and
  makes publish cost O(live subscribers).
- `keyed_mutex.rs`: decrement `users` via a `Drop` guard (and hold the `OwnedSemaphorePermit`
  through the guard) so abort mid-await still releases the entry; add a `size()`-based leak test.

---

## 7. Test list

`oc-session-runner` (this agent):
1. **Coordinator lost-wakeup stress** — port `/tmp/opencode/rc_stress` into `tests/coordinator.rs`:
   N∈{1,4,8} waiters, instant drain, 100k+ iterations, assert no waiter exceeds timeout (ASYNC-001).
2. **Interrupt-vs-restart race** — hammer `interrupt` against a drain that finishes concurrently with
   `wake`; assert the successor drain always sees a cancelled token (ASYNC-011) and `active()`
   converges to empty.
3. **Mid-stream cancel persistence** — streaming mock LLMClient emitting text deltas; cancel after k
   deltas; assert published events == first k deltas + `Step.Failed` (no full-turn drop, ASYNC-002).
4. **Incremental delivery** — mock stream: assert events appear before the stream completes (latency
   parity) and delta ordering is preserved.
5. **Tool-fiber cancel** — cancel during a slow settle; assert `ToolFailed` + `Step.Failed`, no
   orphan fiber, `abort_all` path (`llm.rs:615-617`).
6. **Overflow + compaction recovery** — overflow event before assistant start → `compact_after_overflow`
   → restart once; second overflow → `RunError::Defect`.
7. **Restart recovery sweep** — durable input rows + a dangling pending tool in context; resume →
   `fail_interrupted_tools` emits `ToolFailed`, drain completes (SESSION-008).
8. **`oc-core` bus**: 10k subscribe/drop cycles → `all_subs`/`typed_subs`/`durable_wakes` size bounded;
   `KeyedMutex` abort mid-await → `size()` returns to 0 (ASYNC-005/012).

Integration (with deps): `run --attach` SSE chunk-coalescing golden test (SESSION-005, Agent 06);
server interrupt → token end-to-end (Agent 10); permission-decline halts loop (Agent 08/09).

---

## 8. Dependencies on other agents

- **Agent 02** (config/provider/CLI): `SessionRunnerModel` impl, model resolution, CLI dispatch for
  session/export/import/serve, `LocalClient::create` construction.
- **Agent 03** (database): SQLite `SessionDb` + `session_input`/`session/message/part` CRUD,
  `SQLiteDurableStore` + projectors, `SessionCompaction` DB impl. Blocks most lifecycle ops and recovery.
- **Agent 06** (LLM adapter): streaming `LlmClient` adapter (new contract §4.1) + retry policy in
  executor + drop-aborts-request guarantee; SSE chunk fix (SESSION-005).
- **Agent 08** (permission): authorization inside `ToolSettle::settle` → `Declined` semantics.
- **Agent 09** (tools): `ToolRegistry::materialize`/`ToolSettle` real impl; must fix/avoid
  `run_future` in async (ASYNC-003) or the runner's first tool call panics.
- **Agent 10** (server): `session_interrupt` → `SessionExecution::interrupt`; real `serve` HTTP;
  SSE event streaming; shared `AppState` gets the execution handle.

Contract handshake: the `LlmClient` trait shape (§4.1) and `session_interrupt` semantics (§6) are
the two interfaces that must be agreed before Agent 06/10 integrate; everything else reuses existing
`services.rs` traits.

---

## 9. Risks

1. **Trait-shape churn**: changing `LlmClient::stream` to a stream breaks `tests/runner_loop.rs`
   and any in-flight Agent 06 work. Mitigate: land the trait change + mock stream early; keep a
   small `.collect().await` shim for the old signature during transition.
2. **`run_future` panic (ASYNC-003)** surfaces as tool-interrupt noise if Agent 09 lags; runner
   should spawn tool settlement in a way that fails loudly, not silently (temporarily assert/log).
3. **Recovery sweep correctness**: blind resume could duplicate provider work. Keep it conservative
   (durable-input-only), behind a flag, reviewed separately (reference explicitly leaves this open).
4. **Notify re-registration loop**: the register-before-check loop must create a fresh `Notified`
   each iteration; a reused one silently hangs. Covered by stress test 1.
5. **Partial-persistence cost**: per-delta durable commits on SQLite add write amplification
   (ASYNC-018 projectors inside tx). Measure; batch deltas with a flush window if needed, but
   preserve reference event granularity.
6. **`interrupt` await-drain latency**: 204 waits for the drain to settle; a long tool call delays
   interrupt. This matches reference `Fiber.interrupt`, but document the cooperative semantics.

---

## 10. Merge-order recommendation (Wave 3 composition of the runner)

1. **Wave 1 — foundations, no deps**: `oc-core` bus + keyed_mutex cleanup (ASYNC-005/012);
   `oc-session-runner` RunCoordinator lost-wakeup + interrupt fix with stress tests (ASYNC-001/011).
2. **Wave 2 — runner crate-local**: `LlmClient` streaming trait + incremental loop + mid-stream
   cancel semantics + retry hookup (ASYNC-002, retry) — coordinate trait shape with Agent 06 first.
3. **Wave 3 — composition** (the "runner wiring" milestone): oc-server gains
   `Arc<dyn SessionExecution>`; `session_interrupt` wired; real `serve` HTTP (Agent 10); CLI
   lifecycle commands + `LocalClient::create` (Agent 02); DB-backed impls of the runner's
   service traits (Agent 03); LLM adapter (Agent 06); tool/permission settlement (Agent 08/09).
   This is where SESSION-001..008, TOOLS-001, ASYNC-004 all resolve together.
4. **Wave 4 — recovery sweep** (SESSION-008): durable-input resume on startup, behind a flag,
   reviewed separately.

The runner's own concurrency fixes (Wave 1-2) are safe to land before any dependency lands because
they are crate-local with mock-driven tests; the composition wave is the single gated milestone.
