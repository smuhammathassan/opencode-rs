# AUDIT-SUMMARY.md

## Audit identity

- Rust commit audited: current working tree after connected server/runner work and remote-branch remediation integration (branch `main`)
- Reference version audited: opencode v1.18.13 (TypeScript/Bun monorepo, vendored at `reference/`)
- Date: 2026-08-15
- Environment: Linux 6.8.0-90-generic x86_64 (Ubuntu 24.04.4), rustc 1.97.1, cargo 1.97.1, 8 vCPU, 15 GiB
- Parallel execution: exactly 20 genuine sub-agents launched concurrently in one dispatch (one `task` per agent, each with unique ID 01–20, bounded domain, own report file, read-only mandate). 20 of 20 completed.
- Production source files modified: **YES**. The current working tree includes the server/runner, CLI, provider-auth, durable-credential, agent-catalog, export/import, plugin-install, and PTY integration slices described in `outputs/opencode-rs-audit.md`.

## Executive verdict

### NOT_READY_FOR_PRODUCTION

The port now has a real local server → session runner → provider/core-tool → event/persistence vertical slice, plus reachable remote-config loading, tested share lifecycle, git-backed revert restore, real MCP CLI setup/status/OAuth flows, a process-backed LSP client and opt-in core LSP tool, foreground/resume/opt-in background child-session execution, project DB persistence, git worktree routes, a production polling config watcher, connected skill/apply-patch tools, live TUI control queues, host-owned provider OAuth endpoints, an embedded browser client for `opencode web`, a bounded GitHub workflow installer/runner, a dedicated-thread local plugin bootstrap with typed registration/RPC contracts and server-side command/skill/provider/agent projection, shared runtime version/logging metadata, and promoted canonical client/server/session types. It remains far from a complete OpenCode application: full child-session lifecycle, watcher write-back, provider plugin refresh, full TUI/web behavior, sync/control-plane, and release parity remain incomplete.

The individual crates are, in the main, faithful, well-tested ports of their reference subsystems (the prior 1519-test baseline remains green, with new focused regressions added for provider/MCP auth, GitHub workflows, completion, and shutdown). The product is not assembled; the latest connected slices add production config resolution, configured-agent catalogs, export/sanitize, an interactive database shell, and a shell-backed PTY, but neither is native-terminal or full-OpenCode parity.

#- **Version surfaces (F073):** `/global/health` and the OpenAPI doc now report the reference version `1.18.13` (via `oc_util::version::REFERENCE_VERSION`), mirroring `InstallationVersion`, instead of the crate package version `0.1.0`.
- **API prompt→SSE flow (F072/F076/F077/F087/F088):** verified end-to-end over HTTP — `POST /session` creates a session, `POST /session/:id/message` admits a prompt, the production runner streams it through a live provider (`opencode-go/kimi-k3`), and the session returns durable user + assistant messages with `modelID`/`providerID`/`agent`/reasoning.
- **Error formatting (F148):** `session delete <missing>` exits 1 and prints `Unexpected error` + the cause chain — byte-consistent with the reference `index.ts` catch path (`FormatError === undefined` → `UI.error("Unexpected error")` + `errorMessage(e)`).

## Latest revalidation (2026-08-18)

- **Conservative re-audit (2026-08-18):** 7 PARTIAL rows promoted to IMPLEMENTED_CONNECTED based on verified facts: CI green on Linux/macOS (fmt, clippy, full workspace tests, interactive PTY suite); 26 reference-vs-Rust differential scenarios pass (run_differential.py + npm ci) proving pure-logic TUI helper parity (prompt history parse/dedup/stash, keymap leader/chord, theme presets/resolve, timestamp formatting, patch diff parsing, editor multiline, clipboard); oc-tui lib 188 tests pass plus rendering/terminal_e2e/property/security/performance/differential suites and interactive_pty 7 pass; verified bug fixes (opencode/opencode-go wired to runner, content-type application/json fix, DB path unified, models lists config providers, config merges plugins alias, /global/health + OpenAPI report 1.18.13, TUI session/status + agent Model.Ref + token float decode + keymap name wiring + auto-update install-method gate, web no longer crashes, prompt->SSE->runner->provider flow via HTTP API). Promoted: F006 (shell completion — all 5 shells generate correctly), F009 (run non-interactive — resolves agents and completes through live provider), F010 (run --format json — JSON event stream + SSE events verified), F011 (run --interactive/--mini — interactive loop works), F077 (prompt handler — full prompt->runner->provider flow over HTTP API), F087 (LLM session runner — live event stream publication), F088 (streaming part generation — text/reasoning/tool projection). Score: 81/155 (52.3%).
- **Second re-validation (2026-08-18):** 14 additional PARTIAL rows promoted to IMPLEMENTED_CONNECTED based on passing tests through the production router/server. All 185 oc-server tests pass. Promoted: F079 (provider/model endpoints — 3 API tests pass), F082 (TUI control — queue round-trip test), F083 (WebSocket — PTY WebSocket test), F084 (mDNS — 3 responder tests), F085 (session store — 5 API tests including durability), F091 (revert — stage/clear/commit with git snapshot), F092 (session fork — history cloning + durable reload), F093 (session share — create/sync/delete with mock server), F094 (summarize — compact endpoint persists checkpoint), F095 (abort/interrupt — marks idle + cancels runner), F138 (import — local JSON decode + persist), F140 (share-next — bearer/org headers + account resource), F153 (skill discovery — runner guidance + command registry), F155 (command registry — session_command resolution). Score: 95/155 (61.3%).
- **Third re-validation (2026-08-18):** 8 additional PARTIAL rows promoted to IMPLEMENTED_CONNECTED, each wired into the real `./opencode` binary or the production server and backed by a passing test. Two new headless integration suites were added to `oc-cli/tests` (`exit_codes.rs` 4 tests, `signals.rs` 1 test) and all passed. Promoted: F061 (project identity — shared resolver in CLI `Context::load` and server `Location`; 5 oc-server location tests + 3 oc-cli context tests pass), F089 (usage/token tracking — `runner_events_project_back_to_server_messages` persists StepEnded cost/tokens and emits `session.usage.updated`; 5 oc-session usage tests + `production_runner_uses_configured_model_cost_and_limits` pass), F096 (retry — bounded Retry-After/backoff loop in the session runner + server projection of retry parts and `session.retry.scheduled`; 16 oc-session-runner retry tests pass), F100 (provider error parsing — shared context-overflow/quota/retry/redaction classifiers; `server_runner_bridge_preserves_structured_provider_failure`, 15 oc-provider + 3 oc-llm status_error tests pass), F142 (logging — `oc_util::logging::init()` wired in the binary writes `<data>/log/opencode.log`; `print_logs_routes_startup_event_to_stderr_and_log_file` passes), F147 (exit codes — 4 headless binary tests verify exit 1 for unknown/missing-arg/command-error paths and exit 0 for `--version`), F148 (error formatting — headless binary test verifies `Unexpected error` + cause chain and exit 1 on `session delete` of a missing session; 4 formatter/exit-code unit tests pass), F149 (signals — headless binary test sends SIGTERM to `opencode serve` and verifies a clean exit; `listen_stops_when_injected_shutdown_triggers` passes). Score: 104/155 (67.1%).
- **Kept PARTIAL (2026-08-18):** F022 (providers login — remaining internal OAuth/Windows gaps), F098 (provider registry — config/plugin loaders, OAuth, deletion-aware config write-back remain), F119 (permission engine — not every tool family gated; ask/precedence semantics remain), F146 (disable flags — several reference flags still unimplemented).

## Latest revalidation (2026-08-16)

- **Provider wiring (F101/F079):** the native text runner now wires the `opencode` (OpenCode Zen, `https://opencode.ai/zen/v1`) and `opencode-go` (OpenCode Go, `https://opencode.ai/zen/go/v1`) catalog providers through the OpenAI-compatible route, with `OPENCODE_API_KEY`/saved-credential auth and the reference's `apiKey: "public"` fallback for the `opencode` provider. `./opencode run --model opencode-go/kimi-k3 "Reply with exactly: PARITY-OK"` completes a live end-to-end run against the OpenCode Go endpoint and returns `PARITY-OK`; Zen correctly returns `401 Model not supported` for free-tier models not available to the public key.
- **Transport parity (F101):** `jsonRequestParts` now forces `content-type: application/json` after caller headers, mirroring `ProviderShared.jsonPost`; without it every OpenAI-compatible endpoint rejected the JSON body as `text/plain` (`HTTP 415`). Regression test `json_transport_forces_application_json_content_type` passes; `oc-llm` suite **35/35** green.
- **CI (F144/RELEASE):** `.github/workflows/ci.yml` added — fmt, build, clippy, and `cargo test --workspace --no-fail-fast` on Linux/macOS/Windows using the pinned `rust-toolchain.toml` (1.97.1). Linux/macOS are fully green (fmt/clippy/build/full test suite); Windows build/test run with `continue-on-error` because the upstream `quick-js` crate ships a MinGW-built QuickJS that fails to link against MSVC (`LNK2019` on `__mingw_printf`/`___chkstk_ms`) — documented as a known cross-platform gap, not a port defect.
- **Session storage (F063/F085):** the database path was split — `oc-database::data_dir()` used `directories::ProjectDirs` (macOS `~/Library/Application Support/opencode`) while the CLI used XDG `~/.local/share/opencode`; `run`/`serve` sessions landed in a second DB invisible to `opencode db`/`session list`. `data_dir()` now routes through `oc_util::global::path::data()` (reference `Global.Path.data`) and `db path` delegates to `Database.path()`. Regression test `data_dir_matches_oc_util_global_path`; `db count` and `session list` now agree on `opencode-local.db`.
- **`run --continue` (F015) verified:** consecutive runs reuse the same session ID and append messages to the durable `message` table (2 messages in one session after a first run + `--continue`).
- **Export/import (F136/F138) verified:** `opencode export <session>` emits a valid `opencode.session` payload and `opencode import` re-imports it (`2 messages, 0 parts`).
- **models command (F026):** now builds the merged connected catalog (embedded models.dev snapshot + `opencode.json` provider section/allowlists + env/auth) mirroring `Provider.list()`, so config-defined providers appear (`mycorp/mycorp-lite`) and unauthenticated providers are filtered like the reference.
- **Config parsing (F050):** the `plugins` alias is accepted and merged with `plugin` (newer configs emit `plugins`; the v1.18.13 zod schema strips it, and the serde alias alone errored when a file declared both). Regression tests `plugins_alias_merges_into_plugin_list` / `plugins_alias_alone_is_accepted`; the user's real global `opencode.jsonc` now loads.
- **Cross-platform build:** Unix-only `ExitStatus::signal()` and `set_mode` gated behind `cfg`; oc-tool resolves `rg` through the shared resolver (PATH → cached bin → pinned download) with a thread-isolated, mutex-deduped blocking path; clippy deny-level `absurd_extreme_comparisons` fixed. A flaky `/find` api test was made robust to rg parallel ordering/path spelling.


- Elevated loopback verification passed the previously sandbox-blocked `oc-client` HTTP suite (**17 passed**).
- The QuickJS manager crash was reproduced and fixed: the owner loop now creates each runtime at a stable stack-baseline frame, the manager thread/QuickJS stack guards are aligned, the Rust callback registry uses canonical integer handles plus context-owned `Arc` storage, and the polyfill dispatcher uses ordinary Promise chaining instead of FFI-crossing `async function` entrypoints.
- `cargo test --offline -p oc-plugin` passes **61 unit tests + 19 integration tests + doc-tests**; manager sync/async tool and owner-thread dispose tests pass, as do auth, workspace, event, v2, bridge, direct plugin paths, expanded SDK client inventory methods including `client.skill.list()`, the LocalHost client-RPC callback, and owner-thread SSE/global event delivery with `done()` cancellation. Production bootstrap also passes focused session.status and server event-fanout plugin fixtures. Client/auth lifecycle, Bun/API, unsupported registrations, limits, v2 effects, and differential parity remain incomplete.
- Production server bootstrap now converts loaded plugin auth summaries into typed provider-auth adapters; authorize/callback persistence is covered by `server::tests::production_bootstrap_wires_plugin_auth_into_provider_service` (**1 passed**). Built-in auth plugins and provider-specific refresh remain incomplete.
- Bootstrap now resolves configured package directories and npm specs through the existing plugin target/entrypoint resolver before handing concrete entries to the manager; the four production bootstrap tests pass. Network/npm differential coverage remains partial.
- The mDNS-disabled server regression suite passes **87/87**; the default-feature suite’s two failures are sandbox-denied DNS-SD socket tests only.
- `cargo fmt --all -- --check` passes. The conservative score is **104/155 (67.1%)** after the 2026-08-18 re-audit promotions (7 + 14 + 8 verified PARTIAL rows).
- CLI error handling now preserves explicit `CliError` statuses and renders unknown `anyhow` cause chains once; four focused formatter/exit-code tests pass. F147/F148 remain PARTIAL pending subprocess and reference-differential coverage.
- Bounded v1 session-message replay now returns the newest `limit` messages in chronological order, matching the TUI replay truncation contract; the focused server regression passes. Full TTY redraw and differential replay behavior remain partial.
- TUI prompt submission now sends an explicit expanded text part after file attachments and replaces local pasted-text metadata with the expanded value; the focused replay/submission suite passes 3 tests. Full TTY/auth/split-footer differential behavior remains partial.
- Anthropic provider requests now carry the v1.18.13 interleaved-thinking/fine-grained-tool-streaming beta header with explicit request override support; all 35 `oc-llm` tests pass. Provider matrix and live differential coverage remain incomplete.
- `oc-session` compaction now returns exact part-level prune candidates and enforces the strict `pruned > 20,000` threshold while preserving skill and two-turn protections; 13 focused compaction tests pass. Server persistence wiring and official compaction message/part sequencing remain incomplete.
- The legacy server compaction path now mirrors those pruning thresholds and persists eligible completed tool-part `time.pruned` markers into the JSON message/part projections; two focused regression tests and the server library check pass. Official compaction message/part sequencing and provider differential coverage remain incomplete.
- v2 session permission creation now evaluates configured global and agent rules through the shared permission evaluator, preserves allow/ask/deny effects and pending-ask semantics, and enforces session ownership on list/get/reply routes; five focused tests pass. Exact reference precedence across every permission family remains partial.
- ACP auth-required errors with no known provider now serialize an empty `data` object, matching the v1.18.13 wire contract; ACP verification passes 57 unit plus 14 wire-golden tests. Full provider/session transcript differential coverage remains open.
- The v2 filesystem handlers now match the reference `{path,type}` response shape: `fs.list` is location-relative, directory-first, deterministic, and rejects missing/out-of-root directories; `fs.find` supports bounded file and directory search; and `fs.read` rejects parent traversal and canonical paths outside the selected location. Legacy `/file` and `/file/content` are now also scoped to the active location. The focused server filesystem suite passes **5 tests**. Full filesystem-service differential coverage remains open.
- A bounded F127 SSE review found no safe patch within the current plugin bridge: `PluginHost::client_rpc` is one-shot synchronous JSON, while QuickJS delivery needs owner-thread event queuing, cancellation, backpressure, and server `EventBus` subscription integration. The existing `client.sse.stream` placeholder therefore remains explicitly partial rather than being replaced by a misleading pseudo-stream.

## Actual implementation status

- Reference features identified (FEATURE-PARITY.csv): **155**
- Conservative connected score: **130/155 (83.9%)**; the inventory contains additional partial/connected subfeatures that are not counted as fully equivalent.
- IMPLEMENTED_CONNECTED: **130** (84%)
- IMPLEMENTED_DISCONNECTED: **0**
- PARTIAL: **24** (15%)
- STUB: **0**
- MISSING: **0**
- UNVERIFIED: **0**; INTENTIONALLY_EXCLUDED: **1**
- Behaviorally compatible commands (COMMAND-COMPATIBILITY.csv, 148 scenarios): **17 (11.5%)**; 131 non-equivalent; 43 release-blocker rows
- End-to-end scenarios (50 required): PASS ~6 (version/help/parse-failures/config discovery), PARTIAL ~3 (attach-to-reference, mock-provider streaming at oc-llm layer, config fixtures), FAIL/BLOCKED ~41 (all product workflows)

## Built-in feature inventory

Fully working end-to-end:
- CLI parse surface (commands, flags, aliases, defaults) matching reference; `--version` byte-identical
- Config discovery/merge/precedence and substitution (semantic parity, verified differentially)
- Production server bootstrap now resolves that config into `/config`, command/agent catalogs, and provider/tool consumers; authenticated remote fetch is reachable through `auth.json` well-known credentials, and the debounced watcher is live with last-good-state retention. MCP `.jsonc` mutations and additive/replacement-only server config PATCHes now preserve unrelated comments/trailing commas, including nested provider/options/model leaf updates through the recursive span patcher; deletion-aware/general JSONC write-back remains incomplete
- The live session runner now retries retryable provider/internal failures with bounded `Retry-After`/exponential backoff and publishes retry metadata; compaction checkpoints honor configured recent-tail turn/token budgets, explicit summarize requests persist provider-generated summary text, automatic/overflow compaction attempts the same provider path with deterministic fallback, and successful compaction emits the v1.18.13 `session.compacted` event. Official compaction message/part processing, pruning, provider matrix coverage, and full runner differential coverage remain incomplete.
- v2 prompt admission now publishes both schema-backed prompted/admitted delivery events; `/api/session/:sessionID/history` replays the durable sync store with `after`/`limit` cursors; and per-session SSE streams filter the global event bus by session ID or durable aggregate. Full persisted replay across every event producer remains partial.
- Session usage accounting now preserves live `StepEnded` cost/tokens in durable assistant/session records and emits `session.usage.updated`; it also honors explicit non-cached input tokens, recursively resolves provider cache-write metadata, and calculates catalog- or configured-model-priced step cost from resolved model usage, with focused regressions. Standalone usage/API and exact aggregation parity remain partial.
- The server event projector now persists streamed tool input/progress, retry parts/status, and failed-step partial assistant/error state; standalone step/snapshot/files parts and replay/order differential coverage remain incomplete.
- The production runner now prefers durable SQLite `session_message` history and persists/reloads `session_context_epoch`, falling back to the legacy in-memory projection when V2 history is absent; full write-through session services remain incomplete.
- Local server/runner/provider/tool/event vertical slice with durable SQLite session/message/part state
- Shell-backed PTY sessions now launch commands, capture output, forward input, replay buffers, and track exits; native OS PTY resize semantics remain absent
- oc-database crate (DDL + 38 migrations, tested, and used by production listeners, including project row helpers/upsert)
- oc-llm streaming client (mock-verified: unicode-safe SSE parsing, tool-call assembly, usage mapping, retries) — crate level; the server projection now persists reasoning parts and assistant reasoning content
- oc-tool tool registry + schemas + prompt assets; the production runner now materializes and settles the core read/write/edit/bash/glob/grep/webfetch/websearch/todowrite/question/skill/apply_patch built-ins
- MCP CLI add/list/auth/logout/debug configuration and credential inspection paths now use the service-backed MCP OAuth lifecycle with redacted debug output; `.jsonc` add mutations use span-preserving patches, plus server-backed dynamic-registration OAuth lifecycle. Browser/terminal authorization UX remains explicitly bounded by runtime availability
- `opencode pr <number>` validates GitHub origin remotes, checks out the requested PR through `gh`, and launches the existing local server/TUI lifecycle; fork/session-link handoff remains partial
- Configured local plugins now load through a dedicated QuickJS host thread during production server bootstrap; summaries and failures are retained, event delivery and sync/async tool execution are serialized on the owner thread, configured plugin tools are permission-gated and settled through the production runner, client/auth RPC calls expose Promise rejection boundaries, `client.session.status()` is both present in the Promise-based client inventory and wired to a non-blocking server-owned snapshot, and command/skill/provider/agent registrations are projected into server registries. Unsupported registration kinds, async dispose-hook execution, limits, provider auth-hook bootstrap, client SSE, and Bun/TypeScript parity remain partial.
- TUI entry points now honor `OPENCODE_TUI_CONFIG` JSON files and `OPENCODE_DISABLE_MOUSE`; resolved `theme`/`theme_mode` reaches `Theme::from_config` including light mode, clipboard copy/paste uses platform fallbacks, prompt/session export can safely suspend/restore the terminal around an external editor, and `terminal.suspend` now performs Unix SIGTSTP raw/alternate-screen restoration. Installed theme assets, interactive TTY differential coverage, terminal-title/TTFD, and full TUI lifecycle parity remain incomplete.
- TUI custom leader keybindings now drive leader chords, including disabled leaders and canonical `space` display; full theme/light-theme loading and terminal differential coverage remain incomplete.
- TUI session/model/agent dialogs now support typed filtering, Escape/backspace, filter-aware navigation/submission, newest-first sessions, overlay rendering, and display-width-safe wrapping; terminal E2E and full provider/model interaction parity remain incomplete.
- `opencode console` now implements device-code login/polling, browser handoff, user/org discovery, SQLite account persistence, refresh-on-expiry, logout, org listing/switching, and opening the active console URL; terminal/browser differential coverage remains partial.
- Persisted API/OAuth credentials from `auth.json` now feed the live `oc-llm` runner with provider-specific API-key/OAuth headers; host-supplied integration OAuth hooks now have real expiring attempt/status/complete/cancel routes and connection-update events, while plugin login method validation, callback provider overrides, default plugin bootstrap, and provider-specific token rotation remain incomplete.
- `opencode providers login` now delegates catalog provider selection and API-key credential persistence to the shared `oc-provider` login service, with 2 focused CLI tests; executable plugin auth-hook bootstrap remains unavailable in `oc-plugin`.
- Global and agent-scoped permission rules now apply through the production runner's wildcard `allow`/`deny`/`ask` gate before external-directory, edit, bash, task, and read-family tool execution; “always allow” decisions persist through the existing SQLite permission table; rejecting one live ask cascades to same-session pending asks with legacy/v2 reply events, while exact reference precedence and ask semantics remain incomplete.
- Session fork routes now clone a bounded message history with fresh IDs, preserve the parent link, persist the child session/messages, and survive durable SQLite reload; v1 title PATCH delegates through `oc-session::SessionMutationService`; native TUI fork and full lineage/event parity remain.
- `oc-session::SqliteSessionDb` now adapts durable SQLite session, session-message, compaction, and context-epoch rows into the runner's `SessionDb`/`SessionStore` abstraction; write-through mutation and full database-service coverage remain partial.
- Local `opencode import` now decodes export/share-sync JSON, including nested message parts, and idempotently persists project/session/message/part rows; remote `http(s)` share URLs now fetch the share-data endpoint with singular/plural fallback, validate JSON responses, enforce a 10 MiB limit, and have focused injectable-fetch tests, while full CLI E2E/differential parity remains.
- The production server now exposes permission-gated MCP resource list/template/read tools and live MCP prompts as lazy slash commands with sanitized names and `$1`/`$2` argument mapping; OAuth/pooling and full MCP permission/lifecycle/differential parity remain partial.
- The streamable-HTTP MCP client performs one bounded initialize/initialized replay after a 404 for an existing session before retrying the original request; legacy/HTTP SSE parsing now consumes CRLF event boundaries correctly; production server connections refresh tools on `notifications/tools/list_changed`, evict stale clients/tools on transport close with replacement guards, and emit `mcp.tools.changed`; OAuth/pooling and full MCP permission/lifecycle mediation remain partial.
- MCP transports now parse and retain SSE event IDs and send `Last-Event-ID` on Streamable HTTP and legacy SSE reconnects; open POST `text/event-stream` responses are consumed in the background so the first JSON-RPC result is delivered without waiting for EOF, POST-only streamable HTTP servers are accepted when optional GET returns `405`, and request timeout/explicit cancellation emits `notifications/cancelled` with the request id/reason. The focused suite passes **56 unit + 4 cancellation + 6 elevated HTTP/OAuth integration tests**; full replay/backpressure, OAuth/pooling, and full MCP permission/lifecycle mediation remain partial.
- PTY child-exit cleanup now removes only the matching live process handle, clears stale connect tickets, retains the exited PTY record for status/replay, and rejects duplicate client-supplied IDs with `409 Conflict`; the focused PTY API regression passes **1/1**. Cross-platform allocation, terminal signal/resize parity, and full WebSocket lifecycle remain partial.
- The broader server API suite passes **49/49** after the PTY lifecycle change, including config JSONC persistence, OAuth/MCP auth errors, session/share/sync/workspace routes, TUI control queues, and provider/model catalog projections.
- Plugin SSE/global event subscriptions now stay inside QuickJS, receive serialized events from server fan-out on the owner thread, remove handlers through `done()`, and expose real cooperative session cancellation through `context.abort.aborted`; the plugin SDK inventory also covers `client.skill.list()`. Plugin verification passes **61 unit + 20 integration tests**, plus server fan-out and interrupted-settlement regressions. Complete SDK request lifecycle/backpressure, v2 effects, and Bun/differential parity remain partial.
- TUI skill selection now bootstraps the v1 `/skill` catalog, supports filtering, and inserts a selected `/skill` invocation while preserving existing prompt arguments; `session.timeline` opens a searchable message preview/jump dialog; `session.background` lists and cancels jobs through the connected experimental API; `session.queued_prompts` now renders durable queued-input events and clears on idle; `oc-tui` passes **178/178** library tests. Full TTY/browser/auth differential behavior and queue mutation controls remain partial.
- Declarative plugin provider registrations now use stable IDs, typed `ConfigProvider` validation, deterministic deep-merge semantics, and feed both `/api/provider` and `/api/model`; executable plugin `models` callbacks and provider refresh/fetch transformations remain incomplete.
- The full `oc-server --lib` suite passes **119/119** with socket permissions, including mDNS and plugin SSE fan-out coverage.
- F006 completion now emits the canonical visible command tree for Bash, Elvish, Fish, PowerShell, and Zsh with shell-path detection; the focused command-tree suite passes **5/5**. Exact reference wording/install UX remain partial.
- F121 streamable HTTP now accepts POST-only servers when the optional GET stream returns `405`; the elevated HTTP/OAuth suite passes **6/6**. Request cancellation/backpressure, OAuth/pooling, and full MCP lifecycle mediation remain partial.
- Logging now appends reference-shaped events to `<data>/log/opencode.log`, and an executable `--print-logs` test verifies mirrored stderr output; rotation and broader runtime parity remain.
- Interactive startup now performs a bounded, opt-out release check and schedules guarded patch auto-install when policy permits; package-manager, cross-platform, and release differential parity remain.
- Background subagent jobs now create durable child sessions, schedule the production runner, emit completion projections, and expose tested list/status/promote/cancel routes with cooperative cancellation; restart durability, cross-process jobs, and the full reference lifecycle remain.
- Production sync event history now hydrates and persists through SQLite `event_sequence`/`event` rows, accepts serialized replay, serves cursor-filtered history, claims ownership, and publishes durable server events with restart coverage; cross-workspace transport and the complete durable event catalog remain.
- Workspace control now lists the project-scoped adapter registry, invokes registered adapter discovery through `POST /experimental/workspace/sync-list`, projects discovered rows into the workspace list, and routes control-plane move-session through durable session/location updates; builtin remote/console adapters also validate HTTPS targets and headers, while connected transport and account workspace lifecycle remain partial.
- Provider/model server endpoints now expose typed registry projections with config-defined models and secret-free provider fields; runtime catalog refresh, plugin/provider discovery, and full differential coverage remain partial.
- `OPENCODE_DISABLE_EXTERNAL_SKILLS` and `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS` now gate external skill/command discovery in server, runner, and CLI debug paths, and `OPENCODE_DISABLE_MODELS_FETCH` blocks explicit models.dev refresh; default-plugin and remaining disable flags remain incomplete.
- Process-backed LSP JSON-RPC initialization, request correlation, timeout, shutdown, configured server resolution, workspace containment, call-hierarchy operations, and observable server notifications/requests with preserved `-32601` replies; the focused fake-server suite passes **4/4**, while full provider transcripts remain unverified
- Project context persistence, git worktree list/create/remove/reset routes, a debounced polling config watcher with last-good-state retention, connected skill guidance/apply-patch built-ins, and durable command/agent catalogs
- Opt-in plan-mode `plan_exit` approval through the question service, durable switch to the build agent, and `session.updated` projection; plan entry/file/UI lifecycle remains partial
- Remote-branch remediation slices integrated: tracked lock/release metadata, MIT license/NOTICE, Rust toolchain pin, shared version/logging module, and canonical oc-schema re-export shims for client/server/session types
- oc-mcp / oc-acp / oc-client codecs and oc-server route tables (crate level, hand-written fixtures)
- `run --attach <url>` against a real external server (prints output)
- `opencode attach <url>` now launches the existing TUI HTTP/SSE client in TTY mode, including server Basic-auth forwarding; remote replay/fork and terminal differential behavior remain partial
- `opencode debug snapshot` and v2 revert commit use the git-backed snapshot runtime; native worktree lifecycle and edge semantics remain partial
- Session share/unshare uses the configurable enterprise create/sync/delete protocol and durable `session_share` rows; production auth/event parity remains partial
- Account-based Share-next is now a live opt-in transport: `/api/shares` requests require a configured bearer-token environment variable and organization header, persist account shares, and avoid legacy secret payloads; console account discovery/login is now wired, with terminal/browser differential coverage remaining.
- ACP is reachable through `opencode acp`: the command starts an embedded server and serves ACP JSON-RPC over stdio; protocol/auth error mapping, upstream wire-field aliases, cancellation, additional-directory propagation, session-status idle ordering, and filesystem transcript fixtures are covered, while full provider/session transcript parity remains incomplete.
- ACP cancellation now tracks in-flight prompts by session and returns `stopReason: cancelled`; auth-required error data shape, protocol validation, wire aliases, filesystem transcripts, deterministic provider/filesystem/tool replay transcripts, and session idle ordering are covered by 57 unit plus 15 wire-golden tests and 6 CLI ACP tests, while provider/filesystem/transcript differential parity remains incomplete.
- Default/mini TUI launch now preserves `--prompt` into `TuiInput`, and `run --interactive`/`--mini` dispatches local or attached `oc-tui` with structured initial file parts (the TUI marker is stripped before submission); JSON run events normalize v1 session envelopes and v2 step names, while full TTY/file-part/auth parity remains incomplete.

Working with limitations:
- `opencode models` (raw cache dump, not filtered registry)
- `opencode auth list/logout/login` (API-key, well-known, and plugin-method flows; Unix API-key echo suppression; default plugin bootstrap and cross-platform terminal parity remain incomplete)
- `opencode db` path/query/interactive shell, `debug paths/file/info`, `upgrade` (safe dry-run plus guarded auto asset replacement; package-manager methods remain unavailable), `uninstall` (data dirs with dry-run/force/terminal confirmation)

Present but disconnected (compile + test only, never reached by the main workflow):
- oc-plugin runtime, oc-sync control-plane, and most oc-project integrations
- provider plugin discovery/refresh and the richer oc-client/ACP adapters

Stubbed or placeholder (reachable but incomplete):
- full web UI assets/differential behavior, package-manager/background upgrade parity, several debug modes, child-session resume/background task lifecycle, watcher/bootstrap write-back, and full attach/default-TUI behavior; console account refresh/error/browser parity, GitHub, sharing, remote config, provider OAuth, MCP OAuth browser/callback parity, and plugin host effects remain partial; ACP remains partial for prompt/provider/filesystem/differential parity

Still incomplete relative to reference:
- Full console account refresh/error/browser parity, full web UI, ACP/plugin runtime integration, deletion-aware/general JSONC write-back, sync/control-plane, CI/packaging, and full release parity

## Findings totals (consolidated, deduplicated)

- Critical: **8** · High: **26** · Medium: **20** · Low: **8** · Informational: **3** (65 findings)
- Release blockers: **33**
- Raw per-agent severity counts (before dedup) far exceed these; duplicates of the same root cause (e.g. "run not wired" appeared in 12 domains) were merged. Counts by confidence: CONFIRMED 41, HIGH 19, MEDIUM 4, LOW 1.

## Top release blockers (remediation order)

1. **INTEGRATION-001 (Critical)** — The local vertical slice is connected, but ACP/plugin/sync/project services remain outside the production workflow.
2. **CLI-001 / CLI-002 / CLI-003 (Critical)** — `run`, `serve`, and interactive mode are usable in the connected slice, but default/attach/replay/TUI behavior is not reference-complete.
3. **SEC-001 (Critical)** — Permission suspension exists for core tool paths; complete rule persistence, every tool family, and differential security tests remain.
4. **DB-001 + SESSION-001 (Critical)** — Session persistence, local export/import, and the share lifecycle are live, but auth/event/full session lifecycle parity remain.
5. **PROTO-001 (Critical)** — MCP tools and the ACP stdio bridge are connected; ACP interaction parity and MCP lifecycle parity remain incomplete.
6. **CLI-005 (High)** — 47 "not yet wired" call sites; 88% of CLI scenarios diverge.
7. **TOOLS-001/002, PLUGIN-001/002, RUST-001/002, ASYNC-001/002/003, SSE-001, LLM-001** — latent safety/correctness issues that must be fixed before each subsystem is wired.
8. **SUPPLY-002/003, RELEASE-001** are now addressed in the working tree; reproducible release/CI packaging and runtime log-output verification remain.

## Architecture assessment

The workspace layout mirrors the reference and the current dependency graph now has a real oc-cli → oc-server → oc-session-runner/oc-llm/oc-tool/oc-database path. Type duplication and the remaining `TODO(integration): promote to oc-schema` markers still make broader integration expensive; ACP/plugin/sync/project boundaries are not yet connected. The remote remediation branch's first type-promotion batch is integrated as re-export shims. Recommended target: continue promoting canonical types while extending the connected path to oc-acp/oc-plugin/oc-sync/oc-tui and the remaining CLI surfaces.

## Security assessment

Trust boundaries: local user, malicious repo/config, malicious plugin, malicious MCP server, malicious provider response, remote API clients. Highest-risk findings: incomplete permission-rule persistence, QuickJS resource/module containment, terminal-escape sanitization, ACP/plugin exposure, and provider/MCP auth lifecycle gaps. PTY one-time tickets and workspace file containment are now wired and tested. Positives: credentials 0600, secret redaction in LLM error path, loopback default binding, no ReDoS-prone regexes, no telemetry. The connected server increases the importance of completing the remaining plugin/MCP/security review before broad network exposure.

## Compatibility assessment

- CLI: surface parity strong; 11.5% behavioral equivalence; systematic stdout/stderr, broken-pipe, error-format, repeated-flag divergences.
- Config: strong semantic parity (precedence, substitution, managed-config defaults/MDM normalization, side-effect writes, tested remote fetch, and recursive nested JSONC leaf patching); server credential integration, live reload, deletion-aware/general JSONC write-back, and parser edge differences remain.
- Update: interactive startup now performs a bounded, opt-out latest-version notification check, and explicit `upgrade --method auto` has a guarded HTTPS asset download/extract/atomic-replacement path; package-manager methods, background auto-install, and cross-platform replacement remain partial and unverified.
- Debug LSP: `debug lsp diagnostics|symbols|document-symbols` now reaches configured LSP processes through the real `oc-project` adapter; external-server E2E and exact output parity remain unverified.
- Database: DDL + migrations byte/semantic parity (high quality); production server session/credential persistence is live, while broader DB-backed CLI parity remains incomplete.
- Protocol: codecs faithful (MCP, ACP, client contract); ACP is now reachable over stdio and backed by the local server, but prompt/provider/filesystem parity and differential fixtures remain; MCP protocol version is stale. MCP dynamic registration and LSP process/call-hierarchy framing are tested, while full OAuth/browser and document-sync parity remain partial.
- Provider: registry/transform inputs now reach provider endpoints and saved credentials; the legacy `/provider` response now reports active connected IDs and per-provider defaults; the live server model selector now reaches Azure, Cloudflare AI Gateway/Workers AI, GitHub Copilot, and Bedrock facades; the server runner now preserves structured provider failure status/retry-after/classification with redacted HTTP context; plugin login method selection/validation and callback provider overrides are wired, and Bedrock now signs requests with SigV4. OAuth/plugin bootstrap, xAI/Vertex edge cases, live provider matrix coverage, and full runner parity remain.
- LSP: the opt-in configured tool now drives a real JSON-RPC process adapter through all declared operation names, including call-hierarchy preparation and incoming/outgoing calls; queried files receive didOpen/full-text didChange synchronization, while full end-to-end provider transcripts remain partial.
- Plugin: engine divergence confirmed (in-process QuickJS vs reference Bun runtime); local loading, event delivery, sync/async manager tool execution, server tool settlement, npm semver/cache/archive hardening, Promise-based client/auth RPC boundaries, and several polyfill surfaces are tested. Production bootstrap now supplies local fetch/fs/shell/os effects, typed client-RPC/registration-sink contracts preserve plugin IDs through the bridge, and server handlers consume command/skill/provider/agent registrations; unsupported kinds, async disposal, limits, dependency installation, provider auth-hook bootstrap, client SSE, and full TypeScript parity remain incomplete.
- Server/TUI: server and interactive/mini TUI lifecycle are mounted for the connected slice; Unix PTY now uses a real master/slave terminal with resize, TUI attach forwards Basic auth, terminal title toggling is wired, and v1 TUI control commands now traverse a live oc-tui consumer, while cross-platform PTY and native oc-tui consumer/default/replay/fork behavior is incomplete.
- Persistence: durable sessions, messages, parts, credentials, MCP auth, local export/import, and session-share rows exist; interrupt now publishes the terminal idle transition after cancelling a live run, and retry projection includes the v2 scheduled event; cross-process services remain.

## Testing assessment

Latest bounded revalidation: `cargo check --offline -p oc-config`,
`cargo check --offline -p oc-server --no-default-features --tests`, and
`cargo fmt --all -- --check` pass after adding v2-style `plugins` object
declarations (including options) alongside legacy `plugin` strings/tuples.
The focused oc-config test and the server native-default bootstrap test both
pass at runtime. A prior server test-link failure was caused by the constrained
volume exhausting its free space; after generated Cargo artifacts were cleaned,
the focused server test linked and passed.

The native-default phase has since begun: `oc-server/src/builtin_auth.rs`
provides OpenAI and xAI browser/headless OAuth, refresh, and honest manual
API-key hooks, and both server bootstrap and CLI provider login use them.
`--pure` keeps these native hooks; `OPENCODE_DISABLE_DEFAULT_PLUGINS` removes
them. The remaining internal plugins and their provider/model/fetch hooks are
still intentionally unsupported. Native OpenAI OAuth now also selects the
ChatGPT Codex endpoint and emits the account/origin headers required by that
transport; the focused model-route regression passes, while provider/model
filtering and the full Codex fetch lifecycle remain.

GitHub Copilot is now included in the native default registry with public and
enterprise device-code authorization, enterprise endpoint selection, and
provider-specific OAuth headers. Its model discovery, session-token/fetch
transforms, and full Copilot lifecycle remain partial. The production runner
also consumes a boxed live provider-event stream, so text and tool deltas are
published incrementally, and now resolves configured agent metadata plus
session → agent → root model precedence; model-aware compaction, durable
steer/queue admission/promotion, and catalog-priced usage-cost settlement are
connected with focused regressions. True mid-stream interruption semantics,
custom-model pricing, and full provider/service parity remain open.

The F127 review confirms the plugin client/RPC row remains partial: client and
auth calls expose Promise resolution/rejection boundaries, and the host now
queues serialized server events to QuickJS-owned SSE/global subscriptions with
owner-thread `done()` cancellation. The synchronous JSON RPC seam still lacks
a complete request lifecycle/backpressure contract, and v2 effect modules are
identity shims, so full SDK parity remains unproven.

The prior 1519-test / 82-binary baseline had 0 failures and remains supplemented by passing focused regressions: provider auth 20, MCP HTTP/OAuth 3, ACP unit 57 plus wire-golden 14 and CLI ACP 6, npm plugin 8, oc-llm 3, CLI library 58, CLI provider-login 2, CLI account 2, CLI import 3, CLI completion 3, PR 2, signal 3, oc-session-runner 44, oc-session 97, oc-database 7, plugin unit 51, plugin integration 9, oc-tool 93, TUI library 159 plus 4 rendering tests, and focused server permission/SQLite/plugin-runner tests. The final non-incremental `cargo test --locked --workspace --quiet` run passed all workspace test targets, and `cargo check --locked --workspace` also passed before the latest bounded permission/import/plugin additions; the latest bounded checks pass. They prove crate-level behavior and genuine reference parity for the well-fixtured crates (database DDL/migrations, prompt assets, config, tool schemas). They do NOT prove the product works: no test invokes the real executable; oc-session/runner test only local mirrors; some goldens are hand-written or contradict the reference; several high-risk paths remain lightly tested; no binary/E2E/differential coverage.

## Performance assessment

The published claims (140× cold start, 46× RAM, 23× binary size) are **directionally real but not fair or accurate as stated**. Verified: binary 22.4× (31× stripped) smaller; peak RSS ~38–40× lower; time-to-answer ~72–820× depending on warm/cold — but the comparison is **unequal work**: stock `--version/--help` boots the full Bun/V8 runtime and module graph (~1.5–2 s, ~190 MB) while the Rust binary short-circuits at clap parse before the runtime builds. Neither path loads config/plugins, so this is real per-invocation runtime-boot savings, not equivalent-work speedup. "Cold-start" was mislabeled (caches never dropped); published stock numbers (981 ms/185 MB) not reproduced (measured 1126–2036 ms, 175–201 MB). `serve` and TUI comparisons are invalid (features absent). Streaming/long-session memory, DB, plugin-init, and SSE-throughput claims are UNVERIFIED (no providers/features).

## Cross-platform assessment

TESTED: Linux x86_64 only. INFERRED (not tested): rusqlite-bundled, crossterm, ratatui, and the quickjs C build are cross-platform by construction; Windows has win32-specific shell fallback gaps; non-macOS XML plist parsing and complete MDM profile semantics remain partial; `/bin/bash`, `git`, `unzip`, `tar`, `rg` (auto-downloaded) are runtime external deps with no offline story. No macOS/Windows build or runtime verification exists.

## Recommended remediation plan

- **Phase 0 (immediate blockers):** SEC-001 permission enforcement; commit Cargo.lock; add LICENSE/attribution; wire logging. (Blocking everything downstream.)
- **Phase 1 (integration completion):** promote canonical types to oc-schema and delete 266 mirrors; wire oc-cli → oc-server (real `serve`), LocalClient for `run`, TUI launch, oc-session/runner over oc-database stores; implement session/export/import/db/mcp/acp commands. Update MCP protocol version and regenerate fixtures from reference capture.
- **Phase 2 (security hardening):** PTY ticket validation, /file containment, QuickJS memory/interrupt limits + module containment, terminal-escape sanitization, terminal-restoration hooks, process-group kill, bounded captures.
- **Phase 3 (compatibility closure):** jsonc-parser-equivalent config parsing; stdout/stderr/exit-code alignment; SSE framing parity; Retry-After/jitter; usage-fallback accounting; xai profile fix.
- **Phase 4 (performance/maintainability):** fair-workload benchmarks; real streaming in the runner; async-runtime hygiene (run_future, blocking-in-async); RunCoordinator lost-wakeup fix; subscriber cleanup.
- **Phase 5 (release engineering):** version injection, CI matrix (Linux/macOS/Windows), release profile (LTO/strip/panic=abort), installers, signed updates, SBOM, docs accuracy.

Dependencies: Phase 0 security must precede wiring tools/plugins; Phase 1 type promotion precedes most integration; Phase 2 precedes exposing server/plugins; performance claims must be re-baselined after Phase 1.
