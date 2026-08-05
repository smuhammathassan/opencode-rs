# Plan 06 — LLM Transport, Streaming, Retries, and Accounting

Agent: 06 — Domain: `10-llm-streaming-accounting` (+ async streaming, retries, usage accounting).
Wave 0 READ-ONLY planning. Branch: `fix/audit-remediation`.

---

## 1. Owned consolidated findings

| ID | Sev | Title | Rust locations | Reference locations |
|---|---|---|---|---|
| LLM-001 | High (blocker) | Usage/cost accounting undercounts cached tokens (dead metadata fallbacks) | `oc-session/src/session.rs:355-394` | `packages/opencode/src/session/session.ts:347-361` |
| LLM-002 | Medium | Deterministic retry jitter; HTTP-date `Retry-After` dropped at executor | `oc-llm/src/route/executor.rs:150-166,401-407` | `packages/llm/src/route/executor.ts:99-108,345-351` |
| ASYNC-003 | High (blocker) | Runner buffers whole turn; no incremental streaming; cancel drops turn | `oc-session-runner/src/session/services.rs:311-316`, `runner/llm.rs:462-566` | `packages/core/src/session/runner/llm.ts:232-274,302-309` |
| INFO-003 | Info | oc-llm streaming layer solid at crate level (runtime-verified) | `oc-llm` route/transport/protocols | — |

Owned streaming/accounting details from report 10 (keep fixed while touching these files):
- Token normalization invariant: `nonCachedInputTokens + cacheReadInputTokens + cacheWriteInputTokens == inputTokens`; `reasoningTokens <= outputTokens` (`schema/events.rs` Usage doc + `protocols/shared.ts`). Each protocol mapper must keep one side of the sum derived.
- Provider-specific usage mapping: openai-chat (subtract cached), anthropic (sum breakdown; `reasoningTokens` stays `None`), gemini (`cachedContentTokenCount`), bedrock-converse (`cacheReadInputTokens`/`cacheWriteInputTokens`), openai-responses (cached tokens). Raw wire payload must stay under `providerMetadata["<provider>"]`.
- Failure accounting: provider errors are emitted as `LLMEvent::ProviderError` events (not `Err`) for provider-classified failures; transport/framing errors surface as `Err(LlmError)`. `Step.Ended.cost` hardcoded `0.0` matches reference — do not change.

---

## 2. Files to change

**oc-llm**
- `crates/oc-llm/src/route/executor.rs` — LLM-002 (jitter + HTTP-date).
- `Cargo.toml` (workspace `[workspace.dependencies]`) — add `rand = "0.8"` (only new dep; used only by oc-llm executor).
- `crates/oc-llm/tests/` — new `stream_bytes.rs` (LLM-05 chunk-boundary UTF-8 / split-JSON), extend/add `misc.rs` or new `retry.rs` (executor loop tests), new `usage.rs` (provider usage shapes + invariant golden tests).

**oc-session-runner**
- `crates/oc-session-runner/src/session/services.rs:311-316` — replace buffered `LlmClient::stream` with the incremental stream trait (contract below).
- `crates/oc-session-runner/src/runner/llm.rs:457-566` — rewrite the buffered select/iterate loop to consume the stream event-by-event; publish per event; flush on every exit path; preserve cancel partial-persistence semantics.
- `crates/oc-session-runner/tests/runner_loop.rs` — update `MockLlm` (lines ~308) to yield streams; add long-stream + mid-stream-cancel tests.

**oc-session**
- `crates/oc-session/src/session.rs:355-394` — LLM-001: restore `cache_write_candidates` chain; exact-key `nested_number`.
- `crates/oc-session/src/processor.rs:79,596,876-880` — pass the `step-finish` event's `provider_metadata` into `get_usage` (matches reference `metadata: value.providerMetadata`).

**oc-llm crate** needs no stream changes: `LlmClient::stream` already returns a real `BoxStream<'static, Result<LlmEvent, LlmError>>` (`route/client.rs:403`), framing buffers bytes until `\n\n` and decodes whole events (`transport.rs:223-263`), so split UTF-8 is already correct — INFO-003.

---

## 3. Stream API design — SHARED CONTRACT for Agent 07

Change `oc-session-runner/src/session/services.rs`:

```rust
use futures::Stream;

pub trait LlmClient: Send + Sync {
    /// Stream one provider turn event-by-event. Yields `None` after the
    /// terminal `finish` (or `provider-error`) event. Dropping the stream
    /// aborts the in-flight request; the runner owns cancellation via
    /// `tokio::select!`. The runner awaits each item before polling the next
    /// (natural backpressure).
    fn stream(
        &self,
        request: LLMRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LLMEvent, LLMError>> + Send + '_>>;
}
```

- Item type stays the runner's own `LLMEvent` / `LLMError` (`crates/oc-session-runner/src/llm/{event,error}.rs`) — NOT oc-llm's `LlmEvent`. Zero ripple into the publisher (`publish_llm_event.rs`), `to_llm_message.rs`, `model.rs`, `runner/llm.rs` event handling. The two types serialize to identical JSON; LLM-06 dedup is Agent 02's oc-schema promotion, out of this plan.
- `futures::Stream` is already a workspace dep of `oc-session-runner`.
- Contract invariants the production impl (Agent 07) must honor:
  1. Ordering: `step-start` first; `text|reasoning|tool-input-start` before deltas; matching `-end` after; exactly one `step-finish` then one `finish`, OR a `provider-error` event.
  2. `usage` on `step-finish`/`finish` satisfies `nonCached + cacheRead + cacheWrite == inputTokens`.
  3. Transport/framing/HTTP failures surface as `Err(LlmError)` (remaining events dropped); provider-classified failures surface as `LLMEvent::ProviderError`.
  4. Cancel-safe: dropping the stream must not leak the HTTP body stream (oc-llm's `BoxStream` drop already cancels).
  5. The runner consumes one item at a time; the impl must not buffer the whole turn (oc-llm already streams incrementally).
- Adapter strategy for the production impl (Agent 07, e.g. in `oc-cli`/`oc-server` wiring): call `oc_llm::LlmClient::stream(request_adapted)` and map `oc_llm::schema::LlmEvent` → runner `LLMEvent`. Because both derive serde and serialize identically, a serde round-trip (`serde_json::from_value::<LLMEvent>(serde_json::to_value(oc_event)?)`) is exact and O(n); a hand `From` impl is optional. Request adaptation: runner `LLMRequest` → `oc_llm::LlmRequest` via `oc_llm::llm::request(RequestInput { … })`.

Runner loop rewrite (`runner/llm.rs:457-566`) — replaces the `tokio::select!` over a `Future<Vec>` with:

```rust
let mut stream = self.deps.llm.stream(request.clone());
let mut stream_error: Option<LLMError> = None;
let mut stream_interrupted = false;
let mut stream_completed = false;
loop {
    tokio::select! {
        _ = token.cancelled() => { stream_interrupted = true; break; }
        next = stream.next() => match next {
            None => { stream_completed = true; break; }
            Some(Err(e)) => { stream_error = Some(e); break; }
            Some(Ok(event)) => {
                if overflow_failure.is_some() || publisher.has_provider_error() { break; }
                if let LLMEvent::ProviderError(pe) = &event {
                    if is_context_overflow(&pe.message) && !publisher.has_assistant_started() {
                        overflow_failure = Some(pe.clone()); break;
                    }
                }
                publisher.publish(&event, &[]).await.map_err(turn_error)?;
                // tool-call settlement fiber spawn — unchanged body from lines 486-561
            }
        }
    }
}
publisher.flush().await.map_err(turn_error)?;
// post-loop logic (overflow / stream_error / stream_interrupted /
// await_tool_fibers / step settlement) unchanged, except:
// stream_succeeded = stream_completed && !stream_interrupted && stream_error.is_none()
```

Behavior deltas vs today:
- Each event is published to the durable bus as it arrives (incremental persistence; crash mid-stream keeps partial assistant output).
- `publisher.flush()` still runs on every exit (stream end, error, cancel) — mirrors reference `Effect.ensuring(publish flush)`.
- On cancel mid-stream, events already published persist; `has_active_assistant()` becomes true once `text-start` published, so the existing `fail_assistant("Provider turn interrupted")` path (lines 630-642) persists the partial assistant and fails it — matching `llm.ts:302-309` and fixing the "cancel drops the whole turn" defect.
- Memory is bounded: the runner holds one event at a time; oc-llm holds only the framing buffer.

---

## 4. Accounting fixes (LLM-001)

Reference chain (`session.ts:347-361`), nullish-coalescing — a `0` is respected, never skipped:

```
cacheWriteInputTokens ??
  metadata.anthropic.cacheCreationInputTokens ??
  metadata.vertex.cacheCreationInputTokens ??
  metadata.bedrock.usage.cacheWriteInputTokens ??
  metadata.venice.usage.cacheCreationInputTokens ??
  0
```

`crates/oc-session/src/session.rs`:

1. `cache_write_candidates` (line 355) must return the full ordered chain:
   ```rust
   vec![
       (usage.cache_write_input_tokens, None),
       (None, Some("anthropic")),
       (None, Some("vertex")),
       (None, Some("bedrock")),
       (None, Some("venice")),
   ]
   ```
   Today it returns only `(usage.cache_write_input_tokens, None)`, so `nested_number`'s metadata branch is dead code — cached tokens undercounted, `nonCachedInputTokens` overcounted, input over-charged.

2. `nested_number` (line 361):
   - Top-level provider field match: exact `field == "cacheCreationInputTokens" || field == "cacheWriteInputTokens"` (replace the `contains` superset so behavior matches the reference's exact-key reads).
   - `field == "usage"` branch: check `get("cacheWriteInputTokens")` (bedrock) AND `get("cacheCreationInputTokens")` (venice). Today venice's `cacheCreationInputTokens` under `usage` is missed.
   - Keep first-match-wins order and `Some(0.0) → 0.0` (nullish semantics).
   - Note: `as_f64()` accepts JSON numbers only; reference coerces via `Number(...)`. Numbers are the only emitted shape from oc-llm mappers — acceptable, document it.

3. `crates/oc-session/src/processor.rs:79,596` — the `get_usage` dependency must receive the event's `provider_metadata`:
   - Trait: `fn get_usage(&self, usage: &crate::llm::Usage, metadata: &JsonMap) -> crate::session::UsageResult;`
   - Call site (`processor.rs:596`): pass `provider_metadata.as_ref().unwrap_or(&empty)` from the destructured `step-finish` event.
   - Test impl (`processor.rs:876`): update signature, pass `&JsonMap::new()`.
   - This mirrors reference `metadata: value.providerMetadata`. Event-level metadata is deliberately used (not `usage.provider_metadata`) for exact reference parity.

Untouched but re-verified by new tests: tier selection, copilot `totalNanoAiu`, reasoning@output-rate charge, `safe()` clamps — already match reference.

---

## 5. Retry fixes (LLM-002)

Reference (`executor.ts:99-108,345-351`):
- `retryAfterMs`: `retry-after-ms` numeric → `max(0, ms)`; `retry-after` numeric → `max(0, s*1000)`; else HTTP-date → `max(0, Date.parse(value) - Date.now())`; else `undefined`.
- `retryDelay`: if `retryAfterMs` present → `min(ms, MAX_DELAY_MS)`; else uniform random `nextBetween(min(base*2^attempt*0.8, MAX), min(base*2^attempt*1.2, MAX))`, rounded.

`crates/oc-llm/src/route/executor.rs`:

1. `retry_after_ms` (line 150): add the HTTP-date branch — parse with `chrono::DateTime::parse_from_rfc2822(value)` (chrono already a workspace dep; the same technique as `oc-session-runner/src/retry.rs:154`), convert to `(date - now_utc).num_milliseconds()`, return `max(0, …)`. To keep it testable, thread `now` in: `fn retry_after_ms(headers, now_ms: i64)` with the caller passing `SystemTime::now()` epoch ms. Reference computes `max(0, …)` — a past date yields `0` (immediate retry), unlike retry.rs which falls back to backoff. Match executor.ts exactly.

2. `retry_delay` (line 401):
   - Keep the retry-after path: `(retry_after as u64).min(MAX_DELAY_MS)`.
   - Replace deterministic `(attempt*37)%20` with uniform random in `[min(0.8*base, MAX), min(1.2*base, MAX)]`, rounded — never exceeds `MAX_DELAY_MS` (current code can exceed by ≤19ms).
   - `fn retry_delay(error: &LlmError, attempt: usize, rng: &mut impl Rng) -> u64` so tests use a seeded `StdRng`.
   - `Executor::execute` uses `rand::thread_rng()` per retry decision (mirrors the Effect `Random` service; no Executor state added).

3. `Cargo.toml` workspace: add `rand = "0.8"`; `oc-llm/Cargo.toml` uses it.

Untouched and already matching: retryable statuses 429/503/504/529, `MAX_RETRIES=2`, `BASE_DELAY_MS=500`, `MAX_DELAY_MS=10000`, retry-after-ms precedence, redaction, rate-limit details.

---

## 6. Test list

**oc-llm — `tests/stream_bytes.rs` (LLM-05)**
- Feed a raw SSE body through `transport::frames` as `Bytes` split at arbitrary byte offsets: 4-byte emoji split across 1-byte chunks (reassembled intact, no lossy replacement), and a JSON field value split mid-sequence; assert assembled event JSON + `text-delta` payloads equal the whole-body run.

**oc-llm — executor tests (LLM-002)**
- `retry_after_ms` numeric ms/seconds, HTTP-date (seeded `now`), past-date → 0, malformed → None.
- `retry_delay` bounds: for attempts 0..=5, seeded RNG, assert `low ≤ delay ≤ high ≤ MAX_DELAY_MS`; retry-after path returns `min(ra, MAX)`.
- Loopback retry test (offline, `TcpListener`-based mini SSE server, no python): 503 `Retry-After: 0` → 200; 429 with HTTP-date `Retry-After` → 200; verify `LlmClient.stream` completes after exactly one retry (mirrors audit runtime probe).

**oc-llm — `tests/usage.rs` (accounting + INFO-003 regression)**
- Per-provider usage-shape golden tests: anthropic with `cache_creation_input_tokens` (fixture `recordings/anthropic-messages-cache`), gemini `cachedContentTokenCount` (`gemini-cache`), bedrock-converse `cacheReadInputTokens`/`cacheWriteInputTokens`, openai-compatible `prompt_tokens_details.cached_tokens`, openai-responses cached tokens (`openai-responses-cache`).
- Invariant test across all protocols: `nonCached + cacheRead + cacheWrite == inputTokens`, `reasoning ≤ output`, `totalTokens` logic, `providerMetadata["<provider>"]` carries raw payload.

**oc-session — `session.rs` get_usage golden tests (LLM-001)**
- Metadata fallback per provider shape: `anthropic.cacheCreationInputTokens`, `vertex.cacheCreationInputTokens`, `bedrock.usage.cacheWriteInputTokens`, `venice.usage.cacheCreationInputTokens` → correct `tokens.cache.write`.
- Precedence: `usage.cacheWriteInputTokens` beats metadata; `0` respected over a non-zero metadata fallback (nullish semantics).
- Cost math: full input with `cacheRead`+`cacheWrite` from metadata → exact `Tokens { input: adjusted, cache: {read, write} }` and cost at `ci/co/cc` rates.

**oc-session-runner — `tests/runner_loop.rs` (ASYNC-003)**
- Incremental-publish test: `MockLlm` yields step-start/text-start/text-delta…; assert the bus receives the first `text-delta` before the stream completes (proves per-event publishing, not buffered Vec).
- Long-stream test: ≥50k `text-delta` events; assert order preserved, count == N, finish persists, no unbounded accumulation (runner holds one item at a time).
- Mid-stream cancellation test: stream yields text-start + one text-delta then stalls; cancel token; assert partial assistant message persisted and failed with "Provider turn interrupted" (fixes turn-loss), `TurnOutcome.interrupted`, no `finish` event.
- Mid-stream `Err(LlmError)` test: events before the error persist; `fail_assistant(error.message)`; tool fibers failed.

**oc-session-runner — trait compile test**: any impl of the new `LlmClient` compiles with a `BoxStream<'static>` body.

---

## 7. Dependencies on other agents

- **Agent 02** (INTEGRATION-001 / schema promotion): my stream contract deliberately uses runner-local types, so I do NOT block on oc-schema promotion. After Agent 02 promotes canonical `LLMEvent`/`Usage`, the adapter/serde mapping collapses (LLM-06) — my tests assert JSON-shape stability so they survive promotion. Agent 02's executable wiring is what finally makes LLM-01 E2E reachable; my stream + accounting land first and are test-covered at crate level.
- **Agent 05** (auth, SEC-005): no blocking dependency. Executor `AuthKind`/401/403 mapping already ported. My retry/HTTP-date work is independent of auth-surface changes; coordinate only if Agent 05 rewrites `route/auth.rs` request headers (the retry loop reuses `HttpRequestValue`).
- **Agent 07** (TOOLS-001 / ASYNC-001/004/005 — runner wiring + interrupt): hard consumer of my stream contract (§3). Sequence: (1) I change the trait + rewrite the runner loop; (2) Agent 07 builds the production `LlmClient` adapter (oc-llm → runner events) and binds ASYNC-004's interrupt handler to the `CancellationToken` my loop selects on. Tool-settlement fiber behavior is unchanged, so TOOLS-001 wiring slots in without touching my loop. Agent 07's tests must adopt the incremental trait (no `Vec`).
- **Agent 09** (tools): the tool-call settlement branch I preserve calls `materialization.settle` — unchanged; no dependency.
- **Agent 18** (testing): my loopback retry server and golden fixtures complement its binary/differential harness; no handoff required.

---

## 8. Risks

1. **Trait change blast radius**: `LlmClient::stream` signature change breaks all existing impls (tests/runner_loop.rs + any wiring). Mitigate: single mechanical migration; keep item type `LLMEvent` (runner) so publisher/model/to_llm_message untouched.
2. **Event-model drift (LLM-06)**: two `LLMEvent`/`Usage` types survive until Agent 02's promotion. The adapter's serde round-trip is exact only while both serialize identically; add a JSON-shape regression test in the adapter (Agent 07) or in `runner_loop.rs`.
3. **Cancellation edge**: interrupting between `publish(text-start)` and `flush()` — the existing post-loop failure path must run on cancel with `stream_interrupted=true`. Risk of double-flush or flush-after-drop; the loop keeps a single `flush()` after select.
4. **HTTP-date precision**: reference uses wall clock; SystemTime UTC is equivalent. Past-dated `Retry-After` → 0 vs retry.rs's backoff fallback — keep executor semantics distinct (reference) and cover with a test to prevent "harmonizing" regressions.
5. **RNG dependency**: `rand` is a new workspace dep (supply-chain wave may gate it — coordinate with Agent 19/SUPPLY-001; `rand` is ubiquitous and std-only).
6. **Venice/bedrock shapes**: metadata keys are camelCase AI-SDK conventions; the oc-llm native mappers emit snake_case raw payloads under the same provider keys, so native-payload fallback won't fire for them (matches reference). Only fix the chain; do not "improve" key normalization or we diverge from reference `session.ts`.
7. **Long-stream memory**: bounded by the runner (one item) but the bus/store persists per event — an end-to-end throughput check is deferred to Agent 07 wiring; no unbounded in-memory Vec remains.

---

## 9. Merge-order recommendation

**Wave 2**, after the Wave-1 schema/type-promotion (Agent 02) and dependency/supply-chain groundwork (Agent 19), and **before** any runner wiring (Agent 07), tool wiring (Agent 09), or executable integration (Agent 02/12).

Order within the wave:
1. LLM-002 retry fixes + tests (oc-llm, self-contained, low risk) — lands first.
2. LLM-001 accounting fixes + golden tests (oc-session) — self-contained.
3. ASYNC-003 trait change + runner-loop rewrite + runner tests (oc-session-runner) — last, because it is the shared contract Agent 07 consumes; once green at crate level it unblocks Wave 3 wiring.

Rationale: all three are crate-internal, compile-green, and test-covered without the composition root; shipping them before Agent 07's production `LlmClient` adapter avoids a rewrite of that adapter against the buffered interface.
