# AUDIT-SUMMARY.md

## Audit identity

- Rust commit audited: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c` (branch `main`)
- Reference version audited: opencode v1.18.13 (TypeScript/Bun monorepo, vendored at `reference/`)
- Date: 2026-08-05
- Environment: Linux 6.8.0-90-generic x86_64 (Ubuntu 24.04.4), rustc 1.97.1, cargo 1.97.1, 8 vCPU, 15 GiB
- Parallel execution: exactly 20 genuine sub-agents launched concurrently in one dispatch (one `task` per agent, each with unique ID 01–20, bounded domain, own report file, read-only mandate). 20 of 20 completed.
- Production source files modified: **NO**. Only `rust-port-audit/**` (reports, CSVs, JSON, evidence) and OS-temp files were created. Working tree remains clean.

## Executive verdict

### NOT_READY_FOR_PRODUCTION

The port is 20 well-tested, isolated crates that **do not form a working application**. Every primary workflow of the reference — headless `run`, the interactive TUI, `serve`, session management, MCP/ACP, tool execution, plugins, persistence — is either a "not yet wired" stub or an unconnected component. Zero production code imports a sibling crate (`use oc_` = 0 matches), 266 `TODO(integration)` markers remain, and the executable reaches none of the domain crates. The reference's core loop (session → provider → tool → output) has no live path in the Rust binary.

The individual crates are, in the main, faithful, well-tested ports of their reference subsystems (1519 tests pass; DB DDL/migrations, tool schemas, config semantics, LLM wire formats, MCP/ACP codecs show genuine parity). The product is not assembled.

## Actual implementation status

- Reference features identified (FEATURE-PARITY.csv): **155**
- IMPLEMENTED_CONNECTED: **20** (13%)
- IMPLEMENTED_DISCONNECTED: **61** (39%)
- PARTIAL: **32** (21%)
- STUB: **34** (22%)
- MISSING: **6** (4%)
- UNVERIFIED: **1**; INTENTIONALLY_EXCLUDED: **1**
- Behaviorally compatible commands (COMMAND-COMPATIBILITY.csv, 148 scenarios): **17 (11.5%)**; 131 non-equivalent; 43 release-blocker rows
- End-to-end scenarios (50 required): PASS ~6 (version/help/parse-failures/config discovery), PARTIAL ~3 (attach-to-reference, mock-provider streaming at oc-llm layer, config fixtures), FAIL/BLOCKED ~41 (all product workflows)

## Built-in feature inventory

Fully working end-to-end:
- CLI parse surface (commands, flags, aliases, defaults) matching reference; `--version` byte-identical
- Config discovery/merge/precedence and substitution (semantic parity, verified differentially)
- oc-database crate (DDL + 38 migrations, tested, but not wired)
- oc-llm streaming client (mock-verified: unicode-safe SSE parsing, tool-call assembly, usage mapping, retries) — crate level
- oc-tool tool registry + schemas + prompt assets (crate level)
- oc-mcp / oc-acp / oc-client / oc-server codecs and route tables (crate level, hand-written fixtures)
- `run --attach <url>` against a real external server (prints output)

Working with limitations:
- `opencode models` (raw cache dump, not filtered registry)
- `opencode auth list/logout/login` (basic flows, no OAuth; API key echoed)
- `opencode db path`, `debug paths/file`, `upgrade` (fake), `uninstall` (data dirs only)

Present but disconnected (compile + test only, never reached by binary):
- oc-session + oc-session-runner (the agent loop), oc-tool execution, oc-plugin runtime, oc-mcp, oc-acp, oc-server axum router, oc-tui, oc-client, oc-sync, oc-provider registry, oc-project

Stubbed or placeholder (reachable but returns "not yet wired"):
- `run` (local), `serve`, default TUI, `--mini`, `attach`, `session`, `db`, `export`, `import`, `mcp`, `acp`, `stats`, `agent`, `github`, `console`, `plugin`, `completion`, `generate`, many `debug` subcommands — 17 files / 47 call sites

Missing relative to reference:
- Real server (HTTP/SSE/WS), TUI launch, permission enforcement, persistence, local provider calls, session lifecycle, OAuth, logging, install/update pipeline, CI/packaging, LICENSE/attribution

## Findings totals (consolidated, deduplicated)

- Critical: **8** · High: **26** · Medium: **20** · Low: **8** · Informational: **3** (65 findings)
- Release blockers: **33**
- Raw per-agent severity counts (before dedup) far exceed these; duplicates of the same root cause (e.g. "run not wired" appeared in 12 domains) were merged. Counts by confidence: CONFIRMED 41, HIGH 19, MEDIUM 4, LOW 1.

## Top release blockers (remediation order)

1. **INTEGRATION-001 (Critical)** — Zero production cross-crate integration; no workflow reaches a domain crate.
2. **CLI-001 / CLI-002 / CLI-003 (Critical)** — `run`, `serve`, and the TUI are unwired stubs (primary product functions).
3. **SEC-001 (Critical)** — Permission/approval gate is record-only; model tool calls would execute unapproved once wired.
4. **DB-001 + SESSION-001 (Critical)** — Nothing persists; session/export/import commands stubbed.
5. **PROTO-001 (Critical)** — MCP/ACP unreachable; MCP protocol version stale (2025-06-18 vs reference 2025-11-25).
6. **CLI-005 (High)** — 47 "not yet wired" call sites; 88% of CLI scenarios diverge.
7. **TOOLS-001/002, PLUGIN-001/002, RUST-001/002, ASYNC-001/002/003, SSE-001, LLM-001** — latent safety/correctness issues that must be fixed before each subsystem is wired.
8. **SUPPLY-002/003, RELEASE-001** — Cargo.lock untracked, missing LICENSE/attribution, no logging.

## Architecture assessment

The workspace layout mirrors the reference and the declared dependency graph is acyclic with sound foundations (oc-schema, oc-util). But the graph is **vestigial**: no production source imports a sibling crate. Each crate re-implements its declared deps' types via local mirrors (Message×8, SessionInfo×7, ModelRef×7, Entry×7; oc-schema's types referenced only by its own tests), so crates compile "because of duplicate local models," not through canonical shared types. Error models, IDs, and serialization are duplicated per crate. 266 `TODO(integration): promote to oc-schema` markers quantify the remaining unification. The architecture could support end-to-end integration (boundaries are reasonable), but it has not been performed. Recommended target: promote canonical types to oc-schema, delete mirrors, then wire oc-cli → oc-server/oc-session-runner/oc-llm/oc-tool/oc-database/oc-plugin/oc-tui.

## Security assessment

Trust boundaries: local user, malicious repo/config, malicious plugin, malicious MCP server, malicious provider response, remote API clients. Highest-risk findings: SEC-001 (approval gate unenforced — a prompt-injected repo can drive arbitrary commands/file writes with zero approval once the loop is wired), SEC-002 (PTY ticket never validated → auth bypass on the PTY WebSocket), SEC-003 (dropped containment guard on /file/content → arbitrary file read once server reachable), PLUGIN-001/002 (no QuickJS limits; plugins can read/execute arbitrary local JS), RUST-001/002 (FFI unwind and stack overflow from plugin input), UX-001 (terminal escape injection via markdown renderer). Positives: TLS validation on, credentials 0600, secret redaction in LLM error path, loopback default binding, no ReDoS-prone regexes, no telemetry, more private than the reference. Because the server/tool/plugin paths are unreachable today, most security findings are latent (HIGH confidence static, end-to-end exploit UNVERIFIED).

## Compatibility assessment

- CLI: surface parity strong; 11.5% behavioral equivalence; systematic stdout/stderr, broken-pipe, error-format, repeated-flag divergences.
- Config: strong semantic parity (precedence, substitution, side-effect writes); divergences on invalid-input acceptance (json5), whitespace-only files, legacy TOML migration.
- Database: DDL + migrations byte/semantic parity (high quality), but unused by the executable; DB-backed CLI stubbed.
- Protocol: codecs faithful (MCP, ACP, client contract), but unreachable; MCP protocol version stale; fixtures hand-written.
- Provider: registry/transform faithful but dead; `models`/`auth` partial; xai profile panics; Bedrock/Vertex unimplemented.
- Plugin: engine divergence confirmed (in-process QuickJS vs reference in-process Bun import — NOT subprocess isolation as originally claimed); no limits; transpiler corrupts valid TS; test-only.
- Server/TUI: implemented but never mounted/launched.
- Persistence: none.

## Testing assessment

1519 tests / 82 binaries, 0 failures, 0 ignored — independently reproduced. They prove crate-level behavior and genuine reference parity for the well-fixtured crates (database DDL/migrations, prompt assets, config, tool schemas). They do NOT prove the product works: no test invokes the real executable; oc-session/runner test only local mirrors; some goldens are hand-written or contradict the reference (oc-llm cassettes, oc-tool task schema); several high-risk paths are mutation-proof (TUI app 2995 lines with 0 tests); no binary/E2E/differential coverage.

## Performance assessment

The published claims (140× cold start, 46× RAM, 23× binary size) are **directionally real but not fair or accurate as stated**. Verified: binary 22.4× (31× stripped) smaller; peak RSS ~38–40× lower; time-to-answer ~72–820× depending on warm/cold — but the comparison is **unequal work**: stock `--version/--help` boots the full Bun/V8 runtime and module graph (~1.5–2 s, ~190 MB) while the Rust binary short-circuits at clap parse before the runtime builds. Neither path loads config/plugins, so this is real per-invocation runtime-boot savings, not equivalent-work speedup. "Cold-start" was mislabeled (caches never dropped); published stock numbers (981 ms/185 MB) not reproduced (measured 1126–2036 ms, 175–201 MB). `serve` and TUI comparisons are invalid (features absent). Streaming/long-session memory, DB, plugin-init, and SSE-throughput claims are UNVERIFIED (no providers/features).

## Cross-platform assessment

TESTED: Linux x86_64 only. INFERRED (not tested): rusqlite-bundled, crossterm, ratatui, and the quickjs C build are cross-platform by construction; Windows has win32-specific shell fallback gaps; macOS managed-prefs (plutil) paths unimplemented; `/bin/bash`, `git`, `unzip`, `tar`, `rg` (auto-downloaded) are runtime external deps with no offline story. No macOS/Windows build or runtime verification exists.

## Recommended remediation plan

- **Phase 0 (immediate blockers):** SEC-001 permission enforcement; commit Cargo.lock; add LICENSE/attribution; wire logging. (Blocking everything downstream.)
- **Phase 1 (integration completion):** promote canonical types to oc-schema and delete 266 mirrors; wire oc-cli → oc-server (real `serve`), LocalClient for `run`, TUI launch, oc-session/runner over oc-database stores; implement session/export/import/db/mcp/acp commands. Update MCP protocol version and regenerate fixtures from reference capture.
- **Phase 2 (security hardening):** PTY ticket validation, /file containment, QuickJS memory/interrupt limits + module containment, terminal-escape sanitization, terminal-restoration hooks, process-group kill, bounded captures.
- **Phase 3 (compatibility closure):** jsonc-parser-equivalent config parsing; stdout/stderr/exit-code alignment; SSE framing parity; Retry-After/jitter; usage-fallback accounting; xai profile fix.
- **Phase 4 (performance/maintainability):** fair-workload benchmarks; real streaming in the runner; async-runtime hygiene (run_future, blocking-in-async); RunCoordinator lost-wakeup fix; subscriber cleanup.
- **Phase 5 (release engineering):** version injection, CI matrix (Linux/macOS/Windows), release profile (LTO/strip/panic=abort), installers, signed updates, SBOM, docs accuracy.

Dependencies: Phase 0 security must precede wiring tools/plugins; Phase 1 type promotion precedes most integration; Phase 2 precedes exposing server/plugins; performance claims must be re-baselined after Phase 1.
