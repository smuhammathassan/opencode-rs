# Agent 10 — LLM Request Lifecycle, Streaming, Retries, and Accounting

Auditor: Agent 10 — scope: complete model-interaction pipeline (prompt construction, streaming parsers, tool-call assembly, retries, token/cost accounting, context-window, compaction, events, memory). Repository is READ-ONLY; all findings are static analysis unless marked RUNTIME.

## Scope

- `crates/oc-llm` — protocols (openai-chat, openai-responses, anthropic-messages, gemini, bedrock-converse, openai-compatible-chat), route (client, executor, transport/framing, auth), schema events/usage, tool-stream accumulator, provider-error, cache-policy.
- `crates/oc-session-runner` — runner/llm (agent loop), runner/publish_llm_event, runner/to_llm_message, llm/event+error, retry, execution_local, run_coordinator, session/services (traits).
- `crates/oc-session` — get_usage accounting, summary, compaction, overflow, provider.
- `crates/oc-provider` — transform (sampling/max_output_tokens).
- Reference: `reference/packages/llm/src/**`, `reference/packages/core/src/session/runner/**`, `reference/packages/opencode/src/session/{session.ts,retry.ts,compaction.ts,overflow.ts,summary.ts}`, `reference/packages/opencode/src/session/llm/**`.
- Executable: `crates/oc-cli` (`target/debug/opencode`, v1.18.13).

## Repository areas inspected

- `crates/oc-llm/src/{llm.rs, route/{client,executor,transport,framing}.rs, schema/events.rs, protocols/{openai_chat,anthropic_messages}.rs, protocols/utils/tool_stream.rs, providers/openai_compatible.rs, provider_error.rs, cache_policy.rs, shared.rs, route/auth_options.rs}` + `crates/oc-llm/tests/{stream,misc,golden,common}.rs`
- `crates/oc-session-runner/src/{runner/{llm,publish_llm_event,to_llm_message,model,max_steps}.rs, llm/{event,error}.rs, retry.rs, execution_local.rs, run_coordinator.rs, session/services.rs, session/event.rs}` + tests
- `crates/oc-session/src/{session.rs, summary.rs, compaction.rs, overflow.rs, processor.rs, store.rs}`
- `crates/oc-provider/src/provider/transform/sampling.rs`
- `crates/oc-cli/src/cli/cmd/run/{mod,client}.rs`
- Reference equivalents cited inline below.

## Commands executed

- `cargo test -p oc-llm` — 28 passed / 0 failed (incl. 10 stream-parser tests).
- `cargo test -p oc-session-runner` — 51 passed / 0 failed (44 unit + 5 coordinator + 2 runner-loop).
- `cargo test -p oc-session` — 108 passed / 0 failed.
- `/root/opencode-rs/target/debug/opencode --version` → `1.18.13` (Rust binary builds).
- `opencode run "say hi"` (Rust binary) → fails at `LocalClient::create`.

## Runtime scenarios attempted

Mock provider: `python3 /tmp/opencode/llm-roundtrip/mock_sse.py` (OpenAI-compatible `/v1/chat/completions`, SSE), driven by a standalone harness `/tmp/opencode/llm-roundtrip` that path-depends on `oc-llm` and calls `LlmClient.stream`.

| Layer / scenario | Result | Evidence |
|---|---|---|
| Rust binary `opencode run "say hi"` | **BLOCKED** | `LocalClient::create` returns error; no in-process server (client.rs:65-69) |
| oc-llm → mock SSE: streaming text (UTF-8 emoji split across 1-byte writes, JSON split mid-event) | **PASS** | Events `step-start,text-start,text-delta×3,text-end,step-finish,finish`; text assembled "Hello 😀 world"; usage inputTokens=42, nonCachedInputTokens=32, cacheReadInputTokens=10, totalTokens=49 |
| oc-llm → mock SSE: two streamed tool calls | **PASS** | `tool-input-start/delta…tool-call` ×2, inputs `{"query":"weather"}` and `{"file":"/a.txt"}`, finish `tool-calls`, usage 50/12/62 |
| oc-llm → mock SSE: HTTP 503 `Retry-After: 0` then 200 | **PASS** | Executor retried once, stream completed |
| oc-llm → mock SSE: HTTP 429 then 200 | **PASS** | Executor retried once, stream completed |
| Full runner loop (session→LLM→tools→persistence) | **BLOCKED** | No production impl of runner `LlmClient`/`EventBus`/`SessionStore`/…; no `RunnerDeps` construction anywhere |

## Architecture or behavior summary

Two distinct stacks exist:

1. **`oc-llm` (library, complete).** `LlmClient.stream` returns a real `BoxStream<Result<LlmEvent, LlmError>>` (route/client.rs:403); executor retries 429/503/504/529 up to 2 retries (executor.rs:462-477, MAX_RETRIES=2); SSE framing buffers bytes until `\n\n`/`\r\n\r\n` and decodes whole events (transport.rs:223-263) so split UTF-8 is handled; `SseStream` drops `[DONE]`; usage mapping implements the reference invariant `nonCached+cacheRead+cacheWrite=inputTokens` (openai_chat.rs:565-596, anthropic_messages.rs:749-769). All verified at runtime against the mock.

2. **`oc-session-runner` (agent loop, implemented but UNWIRED).** `SessionRunnerService.run_turn_attempt` builds the request from projected history, then calls the trait `LlmClient::stream` which is **buffered**: `Result<Vec<LLMEvent>, LLMError>` (session/services.rs:311-316). Events are only published to the durable bus after the whole turn completes (runner/llm.rs:462-566). The publisher (publish_llm_event.rs) mirrors reference `publish-llm-event.ts` closely.

3. **Executable integration: absent.** `oc-cli` links `oc-session-runner` (Cargo.toml:27) but never references it; no production implementation of any runner service trait exists (only test impls, e.g. tests/runner_loop.rs:308); `LocalExecution`/`RunCoordinator` are exported but unconstructed; `opencode run` dies at `LocalClient::create` (run/client.rs:65-69).

## Positive observations

- `oc-llm` protocol layer is complete and **runtime-verified end-to-end** against a live mock SSE provider: text, multi-tool-call assembly, chunk-boundary UTF-8, usage accounting, finish reasons, and 503/429 retry all PASS.
- Executor retry parameters (MAX_RETRIES=2, retryable statuses 429/503/504/529, Retry-After ms/seconds) match `reference/.../executor.ts`.
- Anthropic/OpenAI usage invariants, `totalTokens`, `subtractTokens`, cache-policy auto/object modes match reference.
- Runner publisher matches `publish-llm-event.ts` (fragment buffers, duplicate/ordering guardrails, provider-error→fail-assistant, step-finish settlement).
- `prompt_cache_key` (`ses_` + 64-hex prefix strip) matches reference regex; `to_llm_message` ordering (assistant message then trailing tool-result messages; provider-executed call+result interleaving) matches.
- Session retry policy (`retry.rs`) is an exact port of `reference/.../session/retry.ts` (delays `delay(0)=1000,1=2000,2=4000`, caps, FreeUsageLimitError/GoUsageLimitError upsell, HTTP-date Retry-After, `next=now+wait`) with unit tests.
- Context-overflow detection pattern sets match `provider-error.ts` verbatim.
- All focus-crate tests green (oc-llm 28, oc-session-runner 51, oc-session 108).

## Findings summary

| ID | Severity | Title | Confidence |
|---|---|---|---|
| LLM-01 | **Critical** | LLM streaming path not reachable end-to-end from the Rust executable | CONFIRMED |
| LLM-02 | High | Runner buffers full `Vec<LLMEvent>`; no incremental persistence; interrupt loses entire turn | CONFIRMED |
| LLM-03 | High | `get_usage` cache-write metadata fallbacks are dead code (undercounts cached tokens / overcounts input cost) | CONFIRMED |
| LLM-04 | Medium | Executor retry jitter deterministic vs reference random; HTTP-date Retry-After dropped at executor | CONFIRMED |
| LLM-05 | Medium | No tests for SSE chunk-boundary UTF-8 / split JSON (correctness runtime-PASS, coverage gap) | HIGH |
| LLM-06 | Medium | Duplicated `Usage`/`LLMEvent` models: f64 tokens (runner) vs i64 tokens (oc-llm) | CONFIRMED |
| LLM-07 | Low | `Tool.Success.outputPaths` omitted when empty vs reference `outputPaths: []` | CONFIRMED |
| LLM-08 | Low | Empty `agents` array still emits `agents` metadata on user messages | CONFIRMED |
| LLM-09 | Low | Error-body truncation units differ (bytes/chars vs UTF-16 units) | CONFIRMED |
| LLM-10 | Low | `compaction::prune` returns indices; lacks `PRUNE_MINIMUM`/config gate; `process` orchestration unported | HIGH |
| LLM-11 | Info | `Step.Ended` cost hardcoded 0.0 — matches reference (`cost: 0`) | CONFIRMED |
| LLM-12 | Info | No explicit request timeout in either port (HttpOptions has none) | CONFIRMED |

## Detailed findings

### [LLM-01] CRITICAL — LLM streaming path is not reachable end-to-end from the executable
`opencode run` fails immediately: `LocalClient::create` returns `"the in-process opencode server is not wired yet in this build"` (`crates/oc-cli/src/cli/cmd/run/client.rs:65-69`, called at `run/mod.rs:561`). Only `AttachClient` (remote HTTP) works, which delegates to an external server, not the Rust LLM pipeline. No production implementation exists of the runner's service traits — `LlmClient` (session/services.rs:311-316), `EventBus`, `SessionStore`, `SessionInput`, `ToolRegistry`, `Agents` — the only impls are in `oc-session-runner/tests/runner_loop.rs:308`. `RunnerDeps` is never constructed; `oc-cli` links `oc-session-runner` but never references it. **STATIC, CONFIRMED.** Consequence: the entire runner, streaming, retry, and accounting pipeline is unreachable today.

### [LLM-02] HIGH — runner buffers the whole turn; no incremental persistence / interrupt recovery
The trait interface is inherently buffered: `fn stream(&self, request) -> Future<Result<Vec<LLMEvent>, LLMError>>` (services.rs:311-316). The loop awaits the entire turn (`runner/llm.rs:462-470` `tokio::select!`), iterates the finished vector (471-561), and only then flushes to the bus (566). The reference streams incrementally with `llm.stream(request).pipe(Stream.runForEach(...))` publishing each event as it arrives (`reference/.../runner/llm.ts:232-274`) with `Effect.ensuring(publish flush)`. Impacts:
- **Incremental persistence**: reference persists text/tool/usage events during the turn (`llm.ts` checklist "Persist assistant text and usage events incrementally"); Rust persists nothing until the turn completes — a crash mid-stream loses the entire turn.
- **Cancellation**: on token cancel, `events_result` is `None`, the buffered vector is dropped, and because no event was published `has_active_assistant()` is false, so no assistant message is persisted at all (`runner/llm.rs:464-468, 615-642`). The reference persists the partial stream and fails the assistant with "Provider turn interrupted" (`llm.ts:302-309`).
- **Memory growth**: the full event list + text deltas are held in RAM for the whole turn (unbounded for long reasoning streams); reference is O(1)-ish incremental.
**STATIC, CONFIRMED.** Latent until LLM-01 is resolved, but a real parity break once wired.

### [LLM-03] HIGH — `get_usage` cache-write metadata fallbacks are dead code
`cache_write_candidates` returns only `(usage.cache_write_input_tokens, None)` (`crates/oc-session/src/session.rs:355-359`), and `nested_number` only reaches metadata when a `(None, Some(providerKey))` entry exists (`session.rs:361-394`). So the reference fallbacks `metadata.anthropic.cacheCreationInputTokens`, `.vertex.…`, `.bedrock.usage.cacheWriteInputTokens`, `.venice.usage.cacheCreationInputTokens` (`reference/.../session/session.ts:347-361`) are **never consulted**. For providers reporting cache-creation tokens only via metadata, Rust under-reports `cacheWriteInputTokens`, over-reports `nonCachedInputTokens`, and over-charges input cost. **STATIC, CONFIRMED.** Rest of `get_usage` (tier selection, copilot `totalNanoAiu`, reasoning@output-rate charge) matches reference exactly.

### [LLM-04] MEDIUM — retry jitter and HTTP-date Retry-After diverge
Reference uses uniform random `Random.nextBetween(0.8·base, 1.2·base)` (executor.ts:345-351); Rust uses deterministic `base·0.8 + (attempt·37) % 20` (`executor.rs:401-407`) and applies the `MAX_DELAY_MS` cap before adding jitter, so delay can exceed the cap by ≤19ms. Retry count (MAX_RETRIES=2), retryable statuses, and `retry-after-ms`/`retry-after` seconds handling match. Separately, the executor's `retry_after_ms` returns `None` for HTTP-date `Retry-After` (`executor.rs:164`) while the reference computes `Date.parse(value) - Date.now()` (executor.ts:103-104). (The session-level `retry.rs` does handle HTTP dates, matching reference.) **STATIC, CONFIRMED.** RUNTIME: mock 503/429 retries PASS.

### [LLM-05] MEDIUM — no chunk-boundary UTF-8 / split-JSON tests in oc-llm
Stream tests feed whole, pre-assembled events (`tests/stream.rs:9-15` `sse()` helper; `tests/common/mod.rs:33` splits on `\n\n`). No test feeds partial JSON or a UTF-8 char split across byte chunks. Correctness is sound by construction (bytes buffered until delimiter, decoded whole at `transport.rs:229-231`) and was **runtime-verified PASS** via the mock (4-byte emoji split across 1-byte writes decoded intact). **Coverage gap, not a correctness defect. HIGH confidence.**

### [LLM-06] MEDIUM — duplicated event/usage models with different numeric types
`oc-session-runner/src/llm/event.rs` `Usage` uses `f64` (lines 153-170); `oc-llm/src/schema/events.rs` `Usage` uses `i64` (lines 17-43). Two ports of the same reference `Usage`. No precision loss for realistic counts (<2^53), but the duplication is a drift risk when the runner is wired to oc-llm (LLM-01). **STATIC, CONFIRMED.**

### [LLM-07] LOW — `Tool.Success.outputPaths` omitted when empty
`publish_llm_event.rs:732-736` maps empty output paths to `None` (omitted on serialize); the reference always passes `outputPaths` (`publish-llm-event.ts:370`), schema-optional array (`reference/packages/schema/src/session-event.ts:349`). JSON parity break for any local tool with no output files. **STATIC, CONFIRMED.**

### [LLM-08] LOW — empty `agents` array still emitted
`to_llm_message.rs:264-268` inserts `agents` when `Some(...)`; the reference omits it when the array is empty (`message.agents?.length ?` guard, `to-llm-message.ts:128`). **STATIC, CONFIRMED.**

### [LLM-09] LOW — error-body truncation units differ
`provider_message` compares `body.len() <= 500` (bytes, `executor.rs:221`) vs reference `body.body.length <= 500` (UTF-16 units, `executor.ts:205`); `BODY_LIMIT` truncation uses `chars().take(16384)` (executor.rs:338) vs `.slice(0, 16384)` (executor.ts:201). Only non-ASCII error bodies are affected. **STATIC, CONFIRMED.**

### [LLM-10] LOW — compaction `prune` is a pure helper; orchestration unported
`compaction.rs:240-279` returns message indices instead of marking `state.time.compacted` and persisting; lacks the `cfg.compaction?.prune` gate and the `pruned > PRUNE_MINIMUM` gate present in `compaction.ts:243-287`. The `process` flow (agent/provider/plugin selection, media strip, replay/autocontinue, `Event.Compacted`) is not ported in `oc-session`; only pure helpers are. `usable`/`isOverflow` match `overflow.ts` exactly. **STATIC, HIGH confidence.**

### [LLM-11] Info — cost hardcoded 0 in `Step.Ended` — matches reference
`runner/llm.rs:666` `cost: 0.0` mirrors `llm.ts:331` `cost: 0`. Tokens come from `tokens(usage)` and match reference semantics (input=nonCached, output=visibleOutput). **STATIC, CONFIRMED — not a divergence.**

### [LLM-12] Info — no explicit request timeouts in either port
`HttpOptions` has only body/headers/query (options.rs:96-103 = options.ts:53-57). `reqwest::Client::new()` (client.rs:370) has no default timeout, matching reference FetchHttpClient. Timeout propagation is therefore "never time out by default" in both. **STATIC, CONFIRMED.**

## Feature or behavior gaps

- End-to-end runnable LLM session (BLOCKED — LLM-01).
- Incremental event persistence and interrupt recovery during a provider turn (LLM-02).
- Metadata-driven cache-write accounting (LLM-03).
- V1 compaction `process` orchestration (LLM-10).
- `opencode run --mini` (interactive) explicitly "not yet wired" (`run/mod.rs:292-297`).
- WebSocket transport / OpenAI Responses WebSocket route omitted (declared in oc-llm `lib.rs` TODO).

## Test coverage gaps

- SSE chunk-boundary UTF-8 / partial-JSON-across-chunks (LLM-05) — no automated test in oc-llm; only my external runtime probe.
- Executor retry loop (503→200) has no unit test in oc-llm; verified at runtime only.
- No test drives the runner's buffered-stream cancel/interrupt path with a live-ish provider.
- No golden test for `Tool.Success.outputPaths` empty-vs-absent or empty `agents` arrays.
- `get_usage` metadata fallback chain untested (dead code).

## Unverified areas

- End-to-end run (Rust binary → LLM → durable store → TUI/server events): BLOCKED (LLM-01).
- Cancellation / timeout / stream-interruption recovery at runtime: BLOCKED.
- V2 overflow-compaction and auto-compaction at runtime: BLOCKED.
- Non-zero cost accounting end-to-end: always 0 in `Step.Ended` by design (matches reference).
- Reference oracle differential on live streams: not executed (would require real provider config; mock-only policy).

## Final domain verdict

**NOT_READY**

The `oc-llm` protocol/transport/retry layer is complete and runtime-verified (streaming, UTF-8 chunk-boundary, multi-tool assembly, usage, retries all PASS against a mock SSE provider). The runner (`oc-session-runner`) is well-built but uses a buffered `Vec<LLMEvent>` interface that breaks incremental persistence, interrupt recovery, and bounds memory, and — decisively — **no wiring exists** between the runner, the service implementations, and the executable: `opencode run` fails at `LocalClient::create`. The LLM request lifecycle is therefore not reachable end-to-end from the Rust binary, and the accounting pipeline (get_usage metadata fallbacks) has a confirmed high-severity latent bug. Remediation: (1) wire the runner stack (LlmClient/EventBus/Store/Input/Tools/Agents impls + server bootstrap), (2) replace the buffered stream trait with an incremental one and persist events as they arrive, (3) restore get_usage metadata fallbacks, (4) add chunk-boundary UTF-8 + retry tests.
