# Plan 05 — Providers, Authentication, Models (Wave 0 READ-ONLY)

- Agent: 05 | Domain: provider registry, auth/credentials, models CLI
- Repo: opencode-rs @ `fix/audit-remediation` (baseline `90727e19860b8e0c1b0cf6469b696ef3b3efaeb1`)
- Status: READ-ONLY PLAN — no source modified in this pass
- Inputs: `rust-port-audit/09-provider-integration.md`, `FINDINGS.json` (SEC-005), `FINDING-STATUS.csv`, `rust-port-audit/artifacts/09-mock-provider.py`, `crates/oc-provider/**`, `crates/oc-cli/**`, `crates/oc-llm/**`, reference `packages/opencode/src/provider/provider.ts`, `cli/cmd/{models,providers}.ts`, `auth/index.ts`, `packages/llm/src/providers/{xai,openai-compatible-profile}.ts`, `packages/opencode/src/plugin/xai.ts`.

## 1. Owned findings

| ID | Sev | Status | Title |
|----|-----|--------|-------|
| SEC-005 | Low | CONFIRMED (runtime) | `auth login` echoes the API key (no echo hiding); no OAuth login in the binary |
| PROVIDER-002 | Critical | CONFIRMED (static) | `build_registry` is dead code outside tests; executable never builds the registry |
| PROVIDER-003 | High | CONFIRMED (runtime) | `opencode models` prints raw models.dev cache, not the computed registry |
| PROVIDER-004 | Critical | CONFIRMED (runtime) | Custom providers in `opencode.json` are ignored by the executable |
| PROVIDER-005 | High | CONFIRMED (static) | No OAuth login (API-key + well-known only) |
| PROVIDER-006 | High | CONFIRMED (runtime) | `xai` facade panics: `openai_compatible_profile::by_provider("xai").unwrap()` → None |
| PROVIDER-007 | Medium | CONFIRMED (runtime) | API key echoed during `auth login` |
| PROVIDER-008 | Medium | CONFIRMED (static) | Duplicate divergent auth/models impls in oc-cli vs oc-provider |
| PROVIDER-009 | Medium | CONFIRMED (static) | Bedrock SigV4 / Vertex ADC unimplemented (defer; registry loaders already gate) |
| PROVIDER-011 | Low | CONFIRMED (static) | `models` has no snapshot fallback; embedded `MODELS_JSON` unused |
| PROVIDER-012 | Low | CONFIRMED (static) | fuzzysort → subsequence matcher (documented approximation) |
| PROVIDER-013 | Informational | CONFIRMED (static) | `PROFILES` missing `xai` (root cause of PROVIDER-006) |

Cross-agent: PROVIDER-001 (no E2E run) and PROVIDER-010 (`serve`) are run/serve seams owned by Agents 12/02; PROVIDER-006 is oc-llm code (Agent 06 owns oc-llm) but is a one-line profile gap driven from this domain, so I claim the fix and coordinate with Agent 06.

## 2. Target state (what "done" means for this domain)

A **composition-root registry service** in oc-cli that (1) loads config via oc-config, (2) resolves the models.dev catalog via oc-provider (cache → snapshot fallback), (3) snapshots env, (4) reads auth via oc-provider `FileAuthStore`, (5) calls `oc_provider::provider::build_registry`, and is the single source for `models`, `providers/auth`, `run` model resolution, and the server provider handlers. oc-cli's `cli/auth.rs` and `cli/models_dev.rs` are deleted. `models` prints the registry (`Provider.list` semantics), auth login/list/logout reuse oc-provider `auth::login`, password input is echo-hidden, and xai no longer panics.

## 3. Files to change (ownership map)

Owned by this agent (modify):
- `crates/oc-provider/src/models_dev.rs` — add `load(paths/cache) -> IndexMap<String,Provider>` with cache-read + embedded-`MODELS_JSON` fallback + refresh (port reference `ModelsDev.populate/refresh` incl. 5-min TTL, timeout, retry; flock optional). `snapshot()` already exists.
- `crates/oc-provider/src/auth/mod.rs` — keep `FileAuthStore` canonical (already correct). No change unless DB-backed store lands (see §7).
- `crates/oc-provider/src/provider/auth.rs` — unchanged; `AuthHook`/`ProviderAuth` already model OAuth. Optionally expose a `refresh` helper (§6.3).
- `crates/oc-llm/src/providers/openai_compatible_profile.rs` — add `xai: { provider: "xai", base_url: "https://api.x.ai/v1" }` to `PROFILES` (PROVIDER-006/013).
- `crates/oc-cli/src/cli/auth.rs` — DELETE (superseded by oc-provider `FileAuthStore`).
- `crates/oc-cli/src/cli/models_dev.rs` — DELETE (superseded by oc-provider `models_dev`).
- `crates/oc-cli/src/cli/cmd/models.rs` — rewrite against the registry service (§5).
- `crates/oc-cli/src/cli/cmd/providers.rs` — rewrite against oc-provider `auth::login::{login,login_url,logout}` + a `LoginPrompt` impl + echo hiding (§6).
- `crates/oc-cli/src/cli/provider_service.rs` (new) — composition-root registry service (§4).
- `crates/oc-cli/src/cli/cmd/run/*` — minimal hook: model resolution must use the registry service; full run wiring stays with Agent 12/02.
- `crates/oc-cli/Cargo.toml` — add `rpassword` (echo hiding), keep oc-provider/oc-llm deps (already present, currently unused).
- Tests: `crates/oc-cli/tests/provider_e2e.rs` (new, §8), `crates/oc-llm/src/route/executor.rs` redaction tests (§9).

Not owned (coordination only):
- `crates/oc-server/src/handlers/provider.rs` / `model.rs` — register the registry in `AppState`; provider endpoints return `provider_list`/`get` (Agent 02/13).
- `crates/oc-session-runner/src/llm/mod.rs` — swap private mirrors for oc-llm schema (Agent 06), which the registry→llm lowering feeds.
- `crates/oc-config` — already has `load_config` + `v1::Info{provider, disabled_providers, enabled_providers}`; Agent 04 hardens CONFIG-001/002.

## 4. Registry-in-runtime wiring (PROVIDER-002)

Reference invocation point: `reference/.../provider/provider.ts` `layer` builds the registry inside an Effect layer consumed by every command via `Provider.Service`; `models.ts` calls `provider.list()`. Rust analog: a `provider_service.rs` in oc-cli exposing

```rust
pub struct RegistryService { /* cached IndexMap<String, Info> */ }
pub fn load(ctx: &Context, config: &oc_config::v1::Info) -> anyhow::Result<IndexMap<String, oc_provider::provider::Info>>
```

steps, mirroring `provider.ts` `layer` + `build_registry` exactly:
1. `catalog = oc_provider::models_dev::load(&ctx.paths)` (disk cache → embedded `MODELS_JSON` snapshot; `--refresh` = fetch+TTL). This replaces `cli/models_dev.rs`.
2. `envs`: snapshot `std::env::vars` into `BTreeMap<String, Option<String>>` (reference `Env.all()`).
3. `auths = FileAuthStore::new(&ctx.paths.data).all()` (oc-provider `AuthStore`). Replaces `cli/auth.rs`.
4. `build_registry(&RegistryInput { catalog, config: ConfigInput{ provider, disabled, enabled }, envs, auths, enable_experimental_models: cli flag })`.
5. Cache per process; invalidate on `auth login/logout`. Expose `get_model(provider, model) -> Result<Model, ModelNotFoundError>` (incl. `model_suggestions`) so `run` and server handlers share it.

Consumers wired in this plan: `models` cmd (§5), `providers/auth` cmd (§6), and the `run` model-resolution hook. Server handler population is Agent 02's step but consumes this service.

## 5. `opencode models` rewrite (PROVIDER-003, PROVIDER-011)

Reference `models.ts` prints `providers[providerID]` from `provider.list()` — the **registry**, which already prunes unauthenticated providers, deprecated/alpha models, chat aliases, and applies whitelist/blacklist (`registry.rs:1183-1211`). Rewrite `cmd/models.rs`:
- `--refresh` → `models_dev::refresh(paths, force)`.
- No cache → fall back to embedded snapshot (fixes the "models database is empty" + requires-network regression); keep a warning only if the snapshot is also unusable.
- List = sorted registry keys (`opencode*` first, then alpha), each provider printing `id/model` sorted, `--verbose` dumps the `Model` JSON (already schema-canonical).
- Provider filter → `Provider not found: X` when absent from the registry (matches reference `fail(...)`).
- Output must match the reference oracle (`/root/.opencode/bin/opencode`): with no credentials, only `opencode` public models remain; with `OPENAI_API_KEY`, openai models appear. Add a differential note under §8.

## 6. Auth/login flows (SEC-005, PROVIDER-005, PROVIDER-007, PROVIDER-008)

### 6.1 Masking (PROVIDER-007, part of SEC-005)
Replace `read_secret` (`cmd/providers.rs:263-271`) with `rpassword::prompt_password` (cross-platform echo hiding; reference uses `Prompt.password`). Add non-TTY guard stays. Test: pty transcript must not contain the key.

### 6.2 Reuse oc-provider login logic (PROVIDER-008, PROVIDER-005)
`cmd/providers.rs` currently duplicates what `oc-provider/src/auth/login.rs` already implements (`login`, `login_url`, `logout`, `catalog_providers`, `resolve_plugin_providers`). Rewrite to:
- implement `LoginPrompt` in oc-cli over the existing `cli/ui.rs` clack-style printer + `rpassword`/`dialoguer`-free prompt reads;
- call `login::login(auth, prompt, options, catalog_providers(&catalog, ...), plugin_hooks, disabled, enabled)`, `login::login_url`, `login::logout`;
- delete `cli/auth.rs` (use `FileAuthStore`).
This makes list/logout/well-known behavior identical and enables the plugin-auth path (OAuth) immediately.

### 6.3 OAuth design (PROVIDER-005)
Reference OAuth is bundled-plugin-driven (`packages/opencode/src/plugin/xai.ts`, snowflake, cloudflare, codex/openai). oc-provider already has the full `AuthHook`/`ProviderAuth` OAuth machinery (`provider/auth.rs`, `auth/login.rs`) with zero production callers. Plan:
- Ship native Rust `AuthHook` implementations in oc-provider (or a new `oc-provider/src/auth/hooks/`) for the OAuth providers the reference bundles — start with **xai** (RFC 8628 device-code flow + loopback callback; fully headless) since it is the clearest and the profile fix lands anyway. Others (github-copilot, google, snowflake-cortex, cloudflare) follow the same trait.
- `cmd/providers.rs` builds the hook map from (a) native hooks and (b) `oc-plugin` hooks once Agent 15 lands; `login::handle_plugin_auth` already drives method selection, `authorize` (auto/code), callback, and credential storage.
- **Token refresh**: reference does single-flight refresh inside the provider `auth.loader` fetch override (xai.ts:458-549) with a 120 s skew, JWT `exp` decode, and rotated-pair persistence via `auth.set`. Rust analog: add `oc-provider` `refresh` helper — `fn refresh_oauth(store, provider, token_url) -> Result<Info::Oauth>` with single-flight (`OnceLock`/`Mutex<Option<Future>>`), skew check, `expires_in ?? 3600`; oc-cli's registry→llm lowering binds it into an oc-llm `Auth::Custom` closure (seam already exists at `route/auth.rs:115`, `Auth::Custom`) so the request path re-resolves and persists tokens before building headers. Registered as part of the registry service so every provider request gets it.

### 6.4 Credential storage + DB (Agent 03)
Legacy `auth.json` (oc-provider `FileAuthStore`) stays the canonical store for provider auth through this wave. oc-provider also owns the V2 `CredentialStore` abstraction (`credential.rs`) whose SQLite binding lives in `oc-database` (`tables.rs:165 CredentialRow`, `schema.rs:79 credential` table) — currently unwired. Keep the abstraction; Agent 03 wires `CredentialStore` → SQLite. This agent will add a seam in the registry service (`credential_sink: Option<Arc<dyn CredentialStore>>`) so `auth set/remove` can mirror into the DB when Agent 03 lands — non-blocking.

## 7. Dependencies on other agents

- **Agent 03 (oc-database, DB-001/DB-002)**: owns the SQLite `credential` table. This plan only needs the `CredentialStore` trait seam; no ordering blocker. If Agent 03's DB becomes the auth source of truth in a later wave, oc-cli drops `FileAuthStore`.
- **Agent 02 (oc-server, CLI-001/002, INTEGRATION-001)**: owns server wiring + composition root. This plan's `RegistryService` is the provider half of the composition root; Agent 02 mounts it into `AppState` for `handlers/provider.rs` and `handlers/model.rs`. Merge after this plan so the server has a real registry to serve.
- **Agent 06 (oc-llm, ASYNC-003, LLM-001)**: owns oc-llm schema/stream. The registry→llm lowering (config `Info` → `openai_compatible::configure` / `openai::configure` Route with `baseURL` = `options["baseURL"] ?? model.api.url`, `apiKey` = `options["apiKey"] ?? provider.key`) must be built against oc-llm; xai profile change (PROVIDER-006) is coordinated with Agent 06. Prefer Agent 06's canonical `LlmClient`/schema so `oc-session-runner/src/llm/mod.rs` mirrors are deleted.
- **Agent 12 (run/serve, CLI-005)**: `LocalClient`/run wiring consumes the registry service for `--model` resolution; the custom-provider E2E at §8 is the acceptance test both sides can run.
- **Agent 15 (oc-plugin, PLUGIN-001..004)**: plugin auth hooks (`auth.loader`) augment the hook map in §6.3; not required for Wave 2 (native xai hook covers the demo path).
- **Agent 18 (TEST-001/003)**: differential harness against the stock binary should assert `models` output parity (§5) and mock-provider request parity; this plan provides the fixtures/oracle notes.

## 8. Custom-provider E2E test (PROVIDER-004 acceptance)

New `crates/oc-cli/tests/provider_e2e.rs` (no external binaries; `09-mock-provider.py` is the oracle-only version of the same contract):
1. Spin up a tokio mock OpenAI-compatible server (SSE `chat/completions`, Bearer `test-key-12345`, records requests) — a small Rust port of `09-mock-provider.py` (port `{{port}}/v1/chat/completions`, `[DONE]` terminator, usage).
2. Write a disposable `opencode.json` (via `OPENCODE_CONFIG`/`--config` or the `paths` test seam) defining provider `mockai { options:{ baseURL:"http://127.0.0.1:{port}/v1", apiKey:"test-key-12345" }, models:{ "mock-1": {...} } }`.
3. `RegistryService::load` → assert `mockai/mock-1` present, status/whitelist applied.
4. Lowering → `openai_compatible_chat::route()` with base URL + `Auth::Custom`/bearer.
5. `LlmClient::stream(request)` → collect text events, assert streamed content == `Hello from mock!`.
6. Assert the mock log received exactly one `POST /v1/chat/completions` with `Authorization: Bearer test-key-12345` and a valid chat-completion body (model id, messages, stream:true).
7. Regression: `models mockai` lists `mockai/mock-1`; `models` (no creds) is registry-filtered.

This test is the gate for PROVIDER-001/004 on the provider side and doubles as the smoke test for the whole registry→llm path before run/serve wiring lands.

## 9. Secret redaction verification (SEC-005 hygiene)

The LLM layer already redacts (`route/executor.rs:19-121`: sensitive headers/query/body keys, body→16 KiB, `Auth` Debug hides values). **There are no tests.** Add `#[cfg(test)]` cases in `executor.rs`: (a) `status_error` body/headers/url contain `<redacted>` for `authorization`, `x-api-key`, `refresh_token`; (b) raw secret substrings are scrubbed from body text; (c) non-secret fields pass through; (d) `Auth` Debug never prints a `Credential::Value` payload. Also verify `auth.json` mode stays 0600 (existing test) and that `login` masking is asserted in a pty test.

## 10. Risks

- **Output parity**: registry-filtered `models` may still differ from the stock snapshot (stock embeds a different catalog snapshot — `gpt-5.4-fast` variants observed absent from the fetched cache). Mitigate: regenerate `data/models.json` at packaging (models_dev.rs TODO), accept cache-driven output otherwise, and pin oracle comparisons to count-of-connected-providers rather than byte-equality.
- **Config option precedence**: `options.apiKey` vs `provider.key` (env/auth) precedence must mirror `resolveSDK` (`options["apiKey"] === undefined && provider.key`). A mismatch silently sends the wrong credential to a custom provider.
- **OAuth refresh single-flight**: rotating refresh tokens must not be replayed; the refresh helper must be process-singleton and persist before returning, or the next process restarts OAuth (known reference limitation too).
- **Echo hiding portability**: `rpassword` covers Unix/Windows; keep a `-T`/non-TTY guard so CI and piped stdin fail cleanly rather than hang.
- **oc-llm xai fix**: adding `xai` to `PROFILES` is the minimal fix; do not also change route selection (responses-vs-chat) beyond the reference without Agent 06.
- **Dup-code deletion**: deleting `cli/auth.rs`/`cli/models_dev.rs` must land together with the registry-service rewrite to avoid breaking `providers.rs`/`models.rs` mid-wave.

## 11. Merge-order recommendation (Wave 2)

Single PR, provider-domain-scoped, merged before run/serve (Agents 02/12) and before oc-plugin OAuth (Agent 15):
1. **PR 05a (foundation, safe)**: xai profile fix + `PROFILES` test; oc-provider `models_dev::load/refresh` + snapshot fallback; registry-service in oc-cli + delete `cli/auth.rs`/`cli/models_dev.rs`; `models` rewrite; `providers/auth` rewrite (login/login_url/logout via oc-provider, masking). Gates: `cargo build -p oc-cli && cargo test -p oc-cli oc-provider oc-llm`, plus the §8 E2E and §9 redaction tests.
2. **PR 05b (OAuth)**: native xai `AuthHook` (device-code + loopback) + `refresh_oauth` bound to `Auth::Custom`; can move later if Wave 2 scope is tight (SEC-005 is non-blocking).
3. Provider handlers (`oc-server`) consume the registry in Agent 02's server PR; `run --model` resolution consumes it in Agent 12's run PR. Both depend on PR 05a.
