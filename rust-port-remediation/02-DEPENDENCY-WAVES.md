# 02 — Dependency Waves

Compiled by the coordinator from the 20 Wave-0 planning reports (see `AGENT-EXECUTION-LOG.md`).

## Ordering principles

1. **Types before wiring** (Agent 01 first) — canonical `oc-schema` types must exist and all mirrors be deleted before domain crates can import each other.
2. **Composition root before consumers** (Agent 02) — `oc-app` is the structural backbone; every other agent merges into the compiled graph it defines.
3. **Security before exposure** — tool execution, plugins, and the server must NOT become reachable before their security blockers merge (Agent 08 permission gate is the hard prerequisite for tool reachability; SEC-002/003 for server exposure).
4. **Contract changes coalesced** — the runner `LlmClient::stream` BoxStream change (Agents 06/07) is a single migration to avoid churn.
5. **Each wave ends green** — `cargo build --workspace` + `cargo test --workspace` + the wave's newly enabled E2E pass before the next wave begins.

## Wave 0 — Planning (this pass, COMPLETE)

20 read-only agents; all plans delivered to `rust-port-remediation/artifacts/plan-*.md`. No source modified.

## Wave 1 — Canonical foundations (authorized writers: Agents 01, 02, 04, 19, 15, 18-scaffold)

Ordered merge sequence:

| Step | Agent | Deliverable | Gate |
|---|---|---|---|
| 1a | 19 | Commit `Cargo.lock` (+`rust-toolchain.toml` pin, `--locked`); add `LICENSE`/`NOTICE`; `oc-util::version` module; tracing/logging subsystem; `[profile.release]` lto/codegen-units/strip (panic=unwind) | `cargo build --locked`; fmt; license check |
| 1b | 01 | Promote ~35 canonical type groups into `oc-schema`; delete ~90 mirror defs; re-export shims; `TYPE-PROMOTION.csv`; cross-crate serialization tests | schema promotion tests; `cargo test --workspace` |
| 1c | 04 | JSONC parser port (drop json5); loader error semantics; TOML migration; tools-key retention; plugin URL normalization; differential fixtures | CONFIG-001..003 tests green |
| 1d | 02 | `oc-app` composition root skeleton (`AppBuilder::build()` topological order; `AppServices`/`AppRuntime`/`RuntimeContext`; cancellation + shutdown) | `cargo build --workspace`; `cargo test -p oc-app`; real cross-crate `use oc_*` imports exist |
| 1e | 15 | Engine swap `libquickjs-sys` → `rquickjs 0.12` (cc-based build); memory/stack/interrupt/timeout/watchdog; FFI unwind containment; cycle/depth/size caps; module containment; timers + event-hook pumping; transpiler compat | clippy clean on oc-plugin; hostile-plugin corpus terminates; reference fixtures load unmodified |
| 1f | 18 | E2E harness scaffold (`crates/oc-cli/tests/` common infra, mock provider, spawn builders, differential record/replay, provenance tooling); TEST-003/004 fixture fixes | harness smoke; `--ignored`=0 |

Wave 1 exit gate: full workspace build+tests green; clippy `-D warnings` passing (after 19's clippy sweep); `Cargo.lock` committed.

## Wave 2 — Domain implementations and security controls (parallel where ownership disjoint)

| Agent | Scope | Notes |
|---|---|---|
| 03 | DB-backed stores (durable event, credential, project dir, SessionDb, saved-permission), channel-aware path, JSON-mode serialization, restart recovery | wire into oc-app seam; `spawn_blocking` for rusqlite |
| 05 | RegistryService, models command rewrite, auth masking + OAuth-capable login, xai fix, snapshot fallback, custom-provider E2E | PR 05a before 05b (OAuth) |
| 06 | Retry (jitter bounds + HTTP-date Retry-After) → accounting fallbacks → `LlmClient::stream` BoxStream trait change | stream trait change coordinated with 07 |
| 07 | RunCoordinator lost-wakeup fix; bus/keyed-mutex cleanup; then runner incremental loop over the stream | coordinator fixes land first (no deps) |
| 08 | Permission service (V1 + V2 engines, ask/suspend/reply, deny-no-side-effects, persistence, fail-closed) | **HARD GATE before tool reachability** |
| 09 | Containment (canonicalize), streaming bounded reads, bounded shell output + spill, process-group lifecycle, async rg, `run_future` rework, panic→errors, permission-gate integration | parallel with 08 (independent); consumes 08 |
| 10 | oc-server-only hardening (unmounted): PTY ticket store, file containment, SSE framing, overflow surface, `spawn_blocking` | server NOT exposed |
| 11 | oc-client hardening + missing v1/v2 endpoints + `HttpExecutor`/`RouterExecutor`/`ReconnectingSseStream`; SSE-001 fix | in-crate green |
| 13 | MCP version 2025-11-25 + clientInfo version; buffer/channel bounds; child kill-on-drop; reconnect backoff; session-expiry re-init; CLI mcp services | crate-local + CLI, no server |
| 14 | ACP dispatcher + params validator + stdio transport (additive in-crate modules) | no wiring yet |
| 15 | Production plugin wiring (gated: clippy clean, hostile corpus pass, permission-backed ask) | merges only after 08 |
| 18 | Enable E2E scenario groups D (session) with 03; E (tools/permission) with 08/09; F (plugin) with 15; G (MCP) with 13; H (ACP) with 14 | per-agent regression tests |

Wave 2 exit gate: all security blockers (SEC-001/002/003, PLUGIN-001/002, RUST-001/002/003, TOOLS-002/003/004) `FIXED_STATIC_ONLY` minimum, `FIXED_INTEGRATION_TESTED` preferred; no dangerous subsystem exposed without its gate.

## Wave 3 — Session runner and server composition

| Agent | Scope |
|---|---|
| 07 | Runner wiring: real LLM adapter (06 stream), real tool settlement (09+08), persistent event publishing, interrupt→cancellation, recovery sweep |
| 02 | Mount runtime into `oc-server`; `LocalClient` over `RouterExecutor` |
| 10 | **Gated server mount**: `serve.rs` → `oc_server::server::listen` (port-0→4096), graceful shutdown, signals; SSE parity goldens |
| 11 | Consumer cut-over: CLI RunClient / TUI HttpSdkClient / ACP adapter → oc-client; attach works against Rust server |
| 03 | Server-backed persistence live (restart-recovery test green) |

Wave 3 exit gate: `run --attach` and local `run` reach the real stack; serve serves HTTP; restart persistence passes; SEC gate confirmed not bypassed.

## Wave 4 — CLI and TUI

| Agent | Scope |
|---|---|
| 12 | Local `run`, `session`, `db`, `export`/`import` over runtime services; output/exit parity |
| 16 | Default TUI launch, attach, mini, piped-stdin prompt, sanitization, terminal restoration, keybind wiring (after 10/11) |
| 17 | CLI-004 contract layer (error spacing, stream map, broken pipe, repeated flags) coordinated with 12/16; completion/help/debug/generate/uninstall/upgrade/pr |
| 18 | Enable E2E scenarios A/B/C/I/J/K/L/M |

Wave 4 exit gate: no help-listed command returns `not_wired`; E2E groups pass; TUI first-frame + real session.

## Wave 5 — Compatibility, release, and proof

| Agent | Scope |
|---|---|
| 18 | Full differential suite (captured fixtures), mutation-targeted tests, `E2E-RESULTS.md`, `DIFFERENTIAL-RESULTS.md` |
| 20 | `bench.sh` re-baseline on integrated product (equivalent-work scenarios S3–S10); `BENCHMARK-RESULTS.md`; docs corrections |
| 19 | CI matrix (Linux/macOS-Intel/macOS-ARM/Windows: fmt→clippy→test→release→smoke; deny/audit; reproducibility; SBOM/signing hooks); packaging; offline policy; panic=abort after panic paths verified; `RELEASE-GATE-FINAL.md` inputs |
| 17 | mcp-auth/acp/stats-aggregation/agent/plugin/console-login/github-install/web; `KNOWN-DEVIATIONS.md` |
| 10 | Cross-platform + CORS/auth/SSE re-verification |

Wave 5 exit gate: `RELEASE-GATE-FINAL.md` all PASS with runtime evidence; performance claims reproducible; docs accurate.

## Cross-cutting gates (coordinator-enforced)

- **Tool reachability gate**: Agent 08 (permission) + Agent 09 (safety) merged before any merge makes tool execution reachable.
- **Server exposure gate**: SEC-002 + SEC-003 + SEC-001 merged before `serve` mount.
- **Plugin production gate**: rquickjs + limits + containment + clippy clean + permission-backed ask before plugins load in production.
- **TUI gate**: sanitization + restoration before default-TUI launch.
- **Every wave**: `cargo fmt --check`, `cargo build --workspace`, `cargo test --workspace`, clippy, one newly-enabled E2E; then update `FINDING-STATUS.csv` and record the integrated commit hash.
