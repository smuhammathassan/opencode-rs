# Agent 09 — Provider Integrations, Authentication, and Model Handling

Audit of the opencode-rs Rust port (commit `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c`), scope: provider
registry/transform/auth (`oc-provider`), LLM wire protocols (`oc-llm`), auth commands, and the
`opencode models` / `auth` / `run` CLI surfaces. Reference: vendored TS monorepo v1.18.13 under
`reference/`, stock binary `/root/.opencode/bin/opencode` as black-box oracle.

## Scope

- Provider discovery/registration; model discovery/aliases/default selection; auth (API key, OAuth,
  well-known, refresh/storage); custom endpoints/base URLs; proxy; headers; request body
  construction; streaming/non-streaming; tool calling; structured output; images; provider-specific
  params; retries; rate limits; timeouts; error mapping; failover; cancellation; usage accounting;
  context limits; unsupported capabilities; reasoning models; compatibility layers; local
  providers; accidental mock providers; secrets in logs/errors.

## Repository areas inspected

- `crates/oc-provider/src/provider/registry.rs` (1275 L), `provider/mod.rs` (794 L), `provider/auth.rs`,
  `provider/model_status.rs`, `provider/transform/*`, `models_dev.rs`, `credential.rs`,
  `auth/mod.rs`, `auth/login.rs`, `data/models.json` (180 providers / 6057 models).
- `crates/oc-llm/src/route/{client,executor,auth,auth_options,endpoint,framing,protocol,transport}.rs`,
  `protocols/{openai_chat,openai_responses,anthropic_messages,gemini,bedrock_converse,openai_compatible_chat}.rs`,
  `providers/{anthropic,openai,openai_compatible,openai_compatible_profile,xai,openrouter,azure,google,amazon_bedrock,...}.rs`,
  `tests/{stream,golden,misc}.rs`.
- `crates/oc-cli/src/cli/{models_dev.rs, cmd/models.rs, cmd/providers.rs, auth.rs, paths.rs, context.rs,
  cmd/run/mod.rs, cmd/run/client.rs, cmd/serve.rs, main.rs}`.
- `crates/oc-session-runner/src/{llm/mod.rs, session/services.rs}`, `crates/oc-server/src/handlers/*`.
- Reference: `packages/opencode/src/provider/provider.ts`, `packages/opencode/src/cli/cmd/models.ts`,
  `packages/opencode/src/cli/cmd/providers.ts`, `packages/core/src/models-dev.ts`,
  `packages/llm/src/{providers,protocols}/*`.

## Commands executed

- `cargo test -p oc-provider` → 13 auth-flow + 15 registry tests pass.
- `cargo test -p oc-llm` → stream 10/10, golden 8/8, misc 10/10 pass.
- `opencode models` (fresh XDG dir) → "models database is empty" (no cache); after `--refresh` →
  6057 lines (all 180 providers, unfiltered).
- `opencode models --refresh` → fetched `https://models.opencode.ai/api.json` to
  `<cache>/opencode/models.json` (network works).
- `opencode auth list` → lists `auth.json` contents; `opencode auth logout [provider]` → removes.
- `opencode auth login --provider openai` via pty → stored `type: "api"` key, file mode 0600.
- `opencode auth login http://127.0.0.1:41900` → well-known flow stored a `wellknown` credential
  (verified against a mock `.well-known/opencode` server).
- `opencode run "hi" --model openai/gpt-4o` → fails: *"the in-process opencode server is not wired yet
  in this build (TODO(integration): oc-server)"*.
- `opencode serve` then `opencode run --attach http://127.0.0.1:<port> "hi"` → attach fails:
  *connection closed before message completed* (server discards bytes, serves no HTTP).
- Disposable `opencode.json` with custom OpenAI-compatible provider `mockai` pointing at
  `artifacts/09-mock-provider.py` (mock SSE server, Bearer auth):
  - Rust: `opencode run "hi" --model mockai/mock-1` fails at LocalClient seam; **zero** HTTP requests hit the mock.
  - Rust: `opencode models mockai` → "Provider not found: mockai".
  - Stock binary, same config: `opencode models mockai` lists `mockai/mock-1`; `opencode run "hi" --model
    mockai/mock-1` performs two real `/v1/chat/completions` streaming requests (Bearer header verified).
- `xai` panic repro (standalone crate in `/tmp`): `oc_llm::providers::xai::provider().model("grok-4")`
  panics at `oc-llm/src/providers/xai.rs:38` — `Option::unwrap()` on `None`.
- `grep` across workspace: `oc_provider` / `oc_llm` have **no callers outside their own tests** in any
  crate's `src/`.

## Runtime scenarios attempted

| Scenario | Rust result | Stock result |
|---|---|---|
| `models` (no cache) | empty-db warning; requires `--refresh` | lists snapshot (embedded) |
| `models` (cache populated) | 6057 raw catalog lines (incl. deprecated) | provider-registry-filtered |
| `models openai` (env key) | 47 raw cache lines | 48 registry lines incl. `-fast` variants absent from cache |
| `run "hi" --model <real>/<model>` | fails at LocalClient seam | works |
| `run --model mockai/mock-1` (config + live mock) | no HTTP request | 2 real streaming requests |
| `auth login` (API key) | stores key (mode 600), key echoed | hidden password prompt |
| `auth login <url>` (well-known) | works against mock | works |
| `auth list/logout` | works | works |
| `serve` + `run --attach` | attach fails (no HTTP served) | works |

## Architecture or behavior summary

- **oc-llm** is a faithful, well-structured port of `packages/llm`: `Route`/`Protocol`/`Endpoint`/
  `Auth`/`Framing`/`Executor` composition, per-provider protocol state machines (OpenAI Chat &
  Responses, Anthropic Messages, Gemini, Bedrock Converse, OpenAI-compatible Chat), SSE framing,
  request-body construction, tool-call streaming, retry policy (MAX_RETRIES=2, backoff 500ms→10s),
  rate-limit header parsing (incl. Anthropic), and HTTP-error→`LlmErrorReason` mapping. All its tests pass.
- **oc-provider** is a faithful port of the reference registry (`build_registry`), auth store, login
  flows, and models.dev catalog types. All its tests pass.
- **However, the executable never uses either crate.** `oc-cli` depends on `oc-provider` and `oc-llm`
  in `Cargo.toml` (lines 24–25) but references `oc_llm` only in a comment (`agent.rs:30`) and
  `oc_provider` nowhere. It ships its own simplified, divergent implementations: `cli/models_dev.rs`
  (raw catalog cache, no registry filtering), `cli/auth.rs` (duplicate of `FileAuthStore`), and
  `cli/cmd/providers.rs` (API-key + well-known login only, no OAuth). `opencode run`'s local path ends
  at `LocalClient::create`, which returns an error ("in-process opencode server is not wired");
  `opencode serve` binds a bare TCP socket that reads and discards bytes. `oc-session-runner` defines an
  `LlmClient` trait with no implementer and declares its types "private mirrors … oc-llm is still a stub".
  `oc-server` has handlers but is not linked from `oc-cli` and never imports `oc-llm`/`oc-provider`.
- Result: model discovery, credential merge, custom loaders, and every provider wire call are only
  exercised in unit/integration tests. No provider request reaches a real implementation end-to-end.

## Positive observations

- High-quality, test-passing ports of both crates (28 oc-llm + 28 oc-provider tests green).
- Secrets are handled carefully in the LLM layer: `Auth`'s `Debug` impl never prints credential values
  (`oc-llm/src/route/auth.rs:118-129`); `Executor` redacts sensitive headers, query params, and
  key-value JSON body fields and truncates bodies to 16 KiB (`oc-llm/src/route/executor.rs:19-121`).
- `auth.json` is written with mode `0600` (`oc-cli/src/cli/auth.rs:104-114`, `oc-provider/src/auth/mod.rs:148-152`); verified at runtime.
- No mock provider is enabled in production (`mock` only appears in test helpers).
- Well-known `auth login <url>` flow works end-to-end against a mock server.
- Module coverage is 1:1 with the reference LLM package (12 providers, 6 protocols).

## Findings summary

| ID | Severity | Confidence | Title |
|----|----------|------------|-------|
| PROVIDER-001 | Critical | CONFIRMED (runtime) | No provider request reaches a real implementation end-to-end |
| PROVIDER-002 | Critical | CONFIRMED (static) | `build_registry` is dead code outside tests; executable never builds the registry |
| PROVIDER-003 | High | CONFIRMED (runtime) | `opencode models` prints the raw models.dev cache, not the computed registry |
| PROVIDER-004 | Critical | CONFIRMED (runtime) | Custom providers in `opencode.json` are ignored by the executable |
| PROVIDER-005 | High | CONFIRMED (static) | No OAuth login in the executable (API-key + well-known only) |
| PROVIDER-006 | High | CONFIRMED (runtime) | `xai` provider facade panics (`by_provider("xai")` → None) |
| PROVIDER-007 | Medium | CONFIRMED (runtime) | API key is echoed during `auth login` (no echo hiding) |
| PROVIDER-008 | Medium | CONFIRMED (static) | Duplicate, divergent auth/models implementations in oc-cli vs oc-provider |
| PROVIDER-009 | Medium | CONFIRMED (static) | Bedrock SigV4 and Google Vertex ADC not implemented in oc-llm |
| PROVIDER-010 | Medium | CONFIRMED (runtime) | `opencode serve` does not serve HTTP; `run --attach` cannot connect |
| PROVIDER-011 | Low | CONFIRMED (static) | `models` command has no snapshot fallback; embedded snapshot unused |
| PROVIDER-012 | Low | CONFIRMED (static) | fuzzysort replaced by lightweight subsequence matcher |
| PROVIDER-013 | Informational | CONFIRMED (static) | Missing `xai` entry in `PROFILES` vs reference's 9 profiles |

## Detailed findings

### [PROVIDER-001] Critical — No provider request reaches a real implementation end-to-end (CONFIRMED, runtime)

- `oc-cli/src/cli/cmd/run/client.rs:64-69` — `LocalClient::create` unconditionally errors with
  "the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)".
- `oc-cli/src/cli/cmd/run/mod.rs:552-572` — the local path always calls `LocalClient::create`; the error
  is surfaced with a hint to use `--attach`.
- `oc-cli/src/cli/cmd/serve.rs:40-67` — `serve` binds a `TcpListener` and spawns tasks that read and
  discard bytes; it serves no HTTP, so `run --attach` fails ("connection closed before message completed").
- `oc-session-runner/src/session/services.rs:311-316` — `LlmClient` trait has no implementer;
  `oc-session-runner/src/llm/mod.rs:1-5` says `oc-llm` is "still a stub".
- Runtime proof: `opencode run "hi" --model openai/gpt-4o` fails immediately; with a live mock provider
  configured, zero HTTP requests are emitted.

### [PROVIDER-002] Critical — `build_registry` is dead code outside tests (CONFIRMED, static)

- `crates/oc-provider/src/provider/registry.rs:1027` defines `build_registry`; the only callers are
  `crates/oc-provider/tests/registry.rs` (lines 422, 453, 473, 487, 506, 555, 579) and
  `tests/auth_flow.rs`. `grep -rn "oc_provider" crates/**/src` returns no hits outside oc-provider.
- Consequence: registry construction — env/auth merge, custom loaders (`registry.rs:240-694`),
  status/blacklist/whitelist filtering, variant computation, default-model selection — never runs in
  the shipped binary. All of it is test-only.

### [PROVIDER-003] High — `opencode models` prints the raw models.dev cache, not the computed registry (CONFIRMED, runtime)

- `oc-cli/src/cli/cmd/models.rs:33-44` loads via `ModelsDev::load` (`cli/models_dev.rs:46-60`), which
  reads `<cache>/opencode/models.json` and returns the raw `BTreeMap` of JSON — no registry filtering.
- Reference `models.ts` builds the registry (`Provider.Service`) and prints `providers[providerID]`,
  which prunes unauthenticated providers, deprecated/alpha models, and the `gpt-5-chat-latest` alias
  (`reference/packages/opencode/src/cli/cmd/models.ts:33-47`; pruning logic mirrored only in
  `registry.rs:1183-1211`).
- Runtime: fresh dir → Rust prints "models database is empty" while stock prints from its embedded
  snapshot. With cache: Rust prints 6057 lines (all 180 providers incl. deprecated `gpt-3.5-turbo` etc.)
  vs stock 8 lines (no credentials) / 48 openai lines with a key (and shows `gpt-5.4-fast`-style
  variants that do not even exist in the fetched cache, proving stock uses a different snapshot).
- The command does not consult the embedded snapshot `oc-provider/src/models_dev.rs:294` (`MODELS_JSON`),
  which is itself unused by any production code.

### [PROVIDER-004] Critical — Custom providers in `opencode.json` are ignored by the executable (CONFIRMED, runtime)

- `oc-cli` never parses `opencode.json`/`.jsonc`; `oc_config` is referenced only in comments
  (`cmd/serve.rs:70`, `cmd/debug.rs:39`, `cmd/mcp.rs:52`, `cli/network.rs:6`).
- `Context::load` (`cli/context.rs:74-92`) reads no config; `models`/`providers`/`run` never read config
  providers.
- Runtime: a disposable `opencode.json` defining provider `mockai` (base URL + apiKey + model) produced
  `Provider not found: mockai` from `opencode models mockai` and, from `opencode run --model
  mockai/mock-1`, the LocalClient error with no HTTP request to the mock server. The stock binary, given
  the same config, listed `mockai/mock-1` and streamed two real requests to the mock (Bearer
  `test-key-12345` verified server-side). Mock server: `rust-port-audit/artifacts/09-mock-provider.py`.

### [PROVIDER-005] High — No OAuth login in the executable (CONFIRMED, static)

- `oc-cli/src/cli/cmd/providers.rs:203-226` only supports API-key entry; `:209` has
  `TODO(integration): support plugin auth methods (oauth) once oc-plugin lands`.
- `oc-provider/src/auth/login.rs` (660 L) implements OAuth-capable flows (`authorize`, callback methods,
  `AuthCallbackResult`) but is never called by oc-cli (only its tests). Reference `providers.ts` supports
  plugin OAuth methods for GitHub Copilot, Google, Cloudflare, etc.
- `oc-cli/src/cli/cmd/plug.rs:17-19` confirms plugin installation is also unwired.

### [PROVIDER-006] High — `xai` provider facade panics (CONFIRMED, runtime)

- `oc-llm/src/providers/xai.rs:38` and `:57` call `openai_compatible_profile::by_provider("xai").unwrap()`.
- `oc-llm/src/providers/openai_compatible_profile.rs:10-43` defines only 8 profiles (baseten, cerebras,
  deepinfra, deepseek, fireworks, groq, openrouter, togetherai); **xai is missing**. Reference
  `openai-compatible-profile.ts:15` includes `xai: https://api.x.ai/v1`.
- Repro (standalone crate, shared target dir): `oc_llm::providers::xai::provider().model("grok-4")`
  panics: `called 'Option::unwrap()' on a 'None' value` at `xai.rs:38:65`. Any wired `--model xai/…`
  path would crash.
- No test covers `xai::configure` (grep of `crates/**/tests` shows no `xai` usage), so the panic is latent.

### [PROVIDER-007] Medium — API key is echoed during `auth login` (CONFIRMED, runtime)

- `oc-cli/src/cli/cmd/providers.rs:263-271` `read_secret` reads plain stdin without echo hiding
  (`TODO(integration): echo-hiding (e.g. termios) for password input`).
- Reference uses `Prompt.password` (`reference/packages/opencode/src/cli/cmd/providers.ts:173,480`).
- Runtime: pty session showed the entered key echoed in the transcript.

### [PROVIDER-008] Medium — Duplicate, divergent auth/models implementations (CONFIRMED, static)

- oc-cli's `cli/auth.rs` (AuthInfo/Auth store) duplicates `oc-provider/src/auth/mod.rs`
  (`FileAuthStore`/`Info`) with slightly different normalization (`Auth::set` removes `key` and
  `normalized/`, whereas `FileAuthStore::set` removes `key` and `{norm}/` — `cli/auth.rs:80-87` vs
  `auth/mod.rs:167-176`).
- oc-cli's `cli/models_dev.rs` duplicates `oc-provider/src/models_dev.rs`. Two sources of truth for the
  same contracts invites drift (e.g. snapshot fallback exists in one, not the other).

### [PROVIDER-009] Medium — Bedrock SigV4 and Google Vertex ADC not implemented (CONFIRMED, static)

- `oc-llm/src/providers/amazon_bedrock.rs:43-45`: "AWS SigV4 request signing is not implemented … models
  only work if bearer key is configured, otherwise they fail with a clear error."
- `oc-llm/src/providers/google.rs` contains no ADC/oauth token fetch (reference relies on
  `google-auth-library`). Registry TODO at `registry.rs:237-239` confirms.
- `oc-llm/src/lib.rs:37-45` also lists WebSocket transport and `Provider.make` dynamic `apis` as
  unimplemented.

### [PROVIDER-010] Medium — `opencode serve` does not serve HTTP (CONFIRMED, runtime)

- `oc-cli/src/cli/cmd/serve.rs:46-61` discards all connection bytes. `run --attach` to it errors
  `connection closed before message completed`. Reference serves the full HTTP/SDK surface.

### [PROVIDER-011] Low — models command has no snapshot fallback (CONFIRMED, static)

- `oc-cli/src/cli/models_dev.rs:46-60` returns an empty database when the cache file is absent (requires
  network `--refresh`); the reference falls back to the baked `OPENCODE_MODELS_DEV` snapshot
  (`reference/packages/core/src/models-dev.ts:198-230`). oc-provider's embedded `MODELS_JSON`
  (`oc-provider/src/models_dev.rs:294`) exists but is unused by the executable.

### [PROVIDER-012] Low — fuzzysort approximated (CONFIRMED, static)

- `oc-provider/src/provider/mod.rs:247-276` replaces fuzzysort with a subsequence matcher and documents
  `TODO(integration): replace with a faithful fuzzysort scoring port if exact suggestion parity is
  required`.

### [PROVIDER-013] Informational — profile count divergence (CONFIRMED, static)

- Rust `PROFILES` = 8 entries vs reference 9; missing `xai` (root cause of PROVIDER-006). Base URLs of
  the remaining 8 match the reference exactly.

## Feature or behavior gaps

- No in-process execution path at all (run/serve/attach seams). No HTTP server wired into the binary.
- No OAuth login; no plugin auth hooks; no plugin model/auth hooks applied to the registry
  (`registry.rs:12-14` TODO).
- Custom `provider` config (baseURL/apiKey/models) not honored anywhere in the binary.
- `models` output wrong (raw catalog vs registry), includes deprecated models, ignores auth filtering.
- No model-not-found suggestions surfaced (registry `model_suggestions` is test-only).
- Bedrock SigV4 / Vertex ADC / gitlab workflow-model discovery / cloudflare AI-gateway helpers are
  TODOs (`registry.rs:237-239, 549-551, 609-612`; `oc-llm/src/lib.rs:37-45`).
- `models --refresh` network fetch has no timeout/retry/flock (reference has 10s timeout, retry, cross-process lock) — `cli/models_dev.rs:105-139`.

## Test coverage gaps

- No test exercises `providers::xai` (PROVIDER-006 would be caught).
- No oc-cli integration test asserts `models` output parity with the registry; the divergence is untested.
- No test asserts `run`/`serve` reach the LLM layer (they cannot, by construction).
- oc-llm goldens cover only openai-chat / openai-responses / anthropic-messages / gemini bodies;
  bedrock-converse, azure, cloudflare, openai-compatible profile, media/attachments, structured output,
  and retry timing are untested.
- No golden tests for auth.json JSON shape vs the reference, nor for `models.json` cache parsing edge
  cases (truncated file, stale cache).

## Unverified areas

- Retry/backoff and rate-limit behavior cannot be exercised through the binary (no wire path); only
  executor unit logic was reviewed statically.
- Streaming behavior of Bedrock/Azure/Cloudflare providers is untested at runtime; Azure base-URL
  derivation (`registry.rs:290-339`) reviewed statically only.
- Whether the stock binary's embedded snapshot equals models.dev at runtime is inferred (differing
  model lists observed); not directly comparable.
- `opencode auth login` OAuth device flows in the reference could not be compared runtime (Rust lacks them).

## Final domain verdict

**NOT_READY**

The `oc-llm` and `oc-provider` crates are high-quality, well-tested ports, but nothing in the shipped
binary exercises them: `opencode run` aborts at the "in-process server not wired" seam, `serve` does not
serve HTTP, the executable never builds the provider registry, ignores `opencode.json` custom providers,
and `opencode models` prints the raw models.dev catalog instead of the computed registry. No provider
request reaches a real implementation end-to-end. Remediation requires wiring oc-provider's registry and
oc-llm's `LlmClient` into the run/serve path and deleting/consolidating the divergent oc-cli duplicates,
plus fixing the `xai` profile panic.
