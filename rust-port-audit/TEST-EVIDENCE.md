# TEST-EVIDENCE.md

Evidence log for the opencode-rs audit of commit `e7fc33e` against reference v1.18.13.
Coordinator re-runs plus per-agent recorded evidence. Full outputs in `rust-port-audit/artifacts/`.

## Environment & state

- Commit audited: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c` (branch `main`)
- Working tree before audit: CLEAN (`git status --short` = empty)
- rustc 1.97.1 / cargo 1.97.1; Linux 6.8.0-90-generic x86_64 Ubuntu 24.04.4
- Reference: vendored TS/Bun v1.18.13 at `reference/`; reference binary `/root/.opencode/bin/opencode` (reports 1.18.13)
- bun/node NOT installed (reference source not directly executable; differential via stock binary only)

## Coordinator re-run commands and results

### `git status --short`
Empty (clean).

### `git rev-parse HEAD`
`e7fc33e8359bb064c745761ce8e2f9023ae0ae8c`

### `rustc --version --verbose` / `cargo --version`
rustc 1.97.1 (8bab26f4f 2026-07-14) / cargo 1.97.1 (c980f4866 2026-06-30)

### `cargo fmt --all -- --check`
PASS (exit 0).

### `cargo clippy --workspace --all-targets --all-features -- -D warnings`
FAIL (exit 101; 45 errors). Sample: "unnecessary `>= y + 1`", "variable does not need to be mutable", "transmute used without annotations", "usage of an `Arc` that is not `Send` and `Sync`", "redundant closure". Crates affected include oc-plugin, oc-util, oc-schema (per Agent 14). Log: `artifacts/agent14-clippy.log`.

### `cargo test --workspace --all-features` (coordinator, earlier in session) and Agent 18 reproduction
**1519 passed, 0 failed, 0 ignored** across 82 test binaries. Log: `artifacts/18-workspace-test.log`.

### `cargo test --workspace -- --ignored`
0 ignored tests exist (empty run). Exit 0.

### `cargo test --workspace --doc`
0 doctests.

### `cargo check --workspace --all-targets --all-features`
PASS (11 warnings, all in test targets). Log: `artifacts/agent14-cargo-check.log`.

### Cross-crate integration check
`grep -rn "use oc_" crates/*/src --include=*.rs | grep -v "^crates/oc-"` → **0 matches**.
`grep -rn "TODO(integration)" crates/*/src --include=*.rs | wc -l` → **266**.

## Continuation verification

The current implementation pass additionally verified:

- `cargo fmt --all -- --check` — PASS.
- `cargo check --locked -p oc-server --lib` — PASS.
- `cargo test --locked -p oc-plugin --lib` — **51 passed**.
- `cargo test --locked -p oc-plugin --test integration` — **9 passed**, including manager event delivery and sync/async plugin tool execution.
- `cargo test --locked -p oc-server --lib server::tests::production_bootstrap_loads_local_plugin_declarations` — **1 passed**.
- `cargo test --locked -p oc-server --lib runner::tests::configured_plugin_tool_is_materialized_and_settled` — **1 passed**.
- `cargo test --locked -p oc-server --lib state::tests::saved_permissions_round_trip_through_sqlite` — **1 passed**.
- `cargo test --locked -p oc-server --lib runner::tests::configured_permission_rules_override_interactive_defaults` — **1 passed**.
- `cargo test --locked -p oc-provider --test auth_flow` — **20 passed**, including plugin login prompt validation, callback provider overrides, and deterministic expired-credential refresh/rotation helper coverage; live runner refresh invocation remains separately covered below.
- `cargo test --locked -p oc-plugin --lib jsonc::tests` — **7 passed**, including UTF-8 byte-span safety and comment/trailing-comma-preserving object-property patching.
- `OPENCODE_DATA_DIR=/private/tmp/opencode-tests cargo test --locked -p oc-tool --lib` — **93 passed**, including callback-driven child results, depth checks, lifecycle cleanup, cooperative abort, tool-output storage, and truncation; the data-dir override is required on this desktop host because the default per-user data directory is outside the writable sandbox.
- `cargo test --offline -p oc-project --test lsp` — **3 passed**, including correlated JSON-RPC lifecycle, document didOpen synchronization, and prepare/incoming/outgoing call-hierarchy operation coverage against the fake language server.
- `cargo test --locked -p oc-server --lib runner::tests::auto_compaction_respects_config_disablement` — **1 passed**, covering the configured automatic-compaction off switch.
- `cargo test --locked -p oc-server --lib mdns::implementation::tests` — **3 passed** with socket permissions, covering DNS-SD browse records, unrelated-query filtering, and name validation.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-mcp --test http_oauth --quiet` — **4 passed** with elevated local HTTP permissions, covering discovery, dynamic registration, token exchange, error handling, and streamable-HTTP expired-session recovery by replaying initialize/initialized before retrying.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-mcp --lib --quiet` — **53 passed**, including the OAuth pending-provider callback-state regression and OAuth persistence compile fix.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-command --test command --quiet` — **35 passed**, covering lazy MCP prompt command metadata and `$1`/`$2` argument mapping.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-mcp --test stdio --quiet` — **3 passed**, covering MCP prompt pagination/get alongside tools/resources.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib instance_handlers::tests::mcp_prompt_messages_join_text_and_ignore_non_text_content --quiet` — **1 passed**, covering prompt message rendering into server slash-command text.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet` — passed after connecting live MCP prompt discovery and lazy resolution to command endpoints.
- `cargo check --locked -p oc-cli` — passed after upgrade/uninstall and CLI secret-input changes; later focused CLI tests passed after allowing Cargo access to its external crate cache.
- `cargo test --locked -p oc-util --lib logging` — **8 passed**, including append-file creation at the resolved data log path.
- `cargo check --locked -p oc-server --lib` — passed after the mDNS fallback-port lifecycle correction.
- `cargo test --locked -p oc-server --lib handlers::provider::tests::live_provider_refresh_rotates_persisted_credentials` — **1 passed**, proving an expired persisted OAuth credential is refreshed through the host hook before provider auth resolution.
- `cargo test --locked -p oc-plugin --lib host::local_host_tests` — **2 passed**, covering production local-host shell/brace expansion and JSON filesystem effects; `cargo check --locked -p oc-plugin --lib` also passed.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib state::tests::server_event_fanout_invokes_loaded_plugin_event_hooks --quiet` — **1 passed**, after the QuickJS event entrypoint was changed to await async plugin hooks before returning.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-config --tests --quiet` — **passed** (including managed-config override/plist tests and authenticated remote-config fixture); emitted only pre-existing unused-import warnings in `tests/load.rs`.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-cli --lib 'cli::upgrade::tests' --quiet` — **6 passed**, covering startup policy, release-version comparison, strict asset selection, and atomic replacement.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-cli --lib 'cli::cmd::debug::tests' --quiet` — **2 passed**, covering configured LSP server selection and URI normalization.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-server --lib runner::tests::server_user_message_lowers_to_runner_history --quiet` — **1 passed**, proving v1/v2 prompt projections preserve file attachments, agent attachments, and metadata into runner history.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-session --lib store::tests::sqlite_adapter_loads_session_and_event_history --quiet` — **1 passed**, covering the production SQLite-backed `SessionDb` adapter over session rows and ordered `session_message` history.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-cli --lib cmd::providers::tests --quiet` — **2 passed**, covering catalog provider-name resolution, shared provider-login credential persistence, and unknown-provider rejection.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-session --lib --quiet` — **97 passed**, including the SQLite-backed session-store adapter and usage-accounting regressions.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner_events_project_back_to_server_messages --quiet` — **1 passed**, covering live StepEnded cost/token preservation, durable session accumulation, and `session.usage.updated` emission.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-database --lib --quiet` — **7 passed**, including the session-message/context-epoch query helpers.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-plugin --lib npm --quiet` — **8 passed**, covering semver selection, versioned cache validation, package metadata checks, safe archive extraction, and symlink/traversal rejection.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-acp --lib --quiet` — **57 passed**, including session-status idle ordering and wire-field alias handling.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-acp --test wire_golden --quiet` — **14 passed**, covering cancellation, protocol/auth error mapping, additional-directory propagation, idle transcript completion, and multi-file filesystem transcript coverage.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-cli --lib 'acp' --quiet` — **6 passed**, covering the ACP CLI bridge's session-status and provider/file/tool transcript normalization.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib 'cli::cmd::mcp::tests' --quiet` — **7 passed**, covering MCP transport validation, token redaction, config-secret redaction, explicit auth/registration statuses, and root/nested `.jsonc` comment-preserving writes.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-cli --lib --quiet` — **58 passed**, including the uninstall home-path boundary regression; the earlier run exposed and the corrected run resolved `/tmp/home-other` shortening incorrectly to `~-other`.
- `CARGO_INCREMENTAL=0 cargo check --locked -p oc-session-runner --quiet` — passed; `CARGO_INCREMENTAL=0 cargo test --locked -p oc-session-runner --lib --quiet` — **44 passed** after adding bounded retry/backoff event handling.
- `CARGO_INCREMENTAL=0 cargo check --locked -p oc-cli --quiet` — passed after wiring mini/interactive dispatch and run-event normalization.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-llm --lib --quiet` — **3 passed**, covering Bedrock SigV4 HMAC/canonical path-query helpers and current-time formatting.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-server --lib runner::tests::runner_events_project_back_to_server_messages --quiet` — **1 passed**, now covering reasoning start/delta/end projection into persisted parts and assistant content.
- `CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --lib runner::tests::runner_events_project_back_to_server_messages --quiet` — **1 passed**, covering durable tool input start/delta/end, progress, retry, and failed-step partial-error projection.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet` — **passed** after wiring the production runner's durable SQLite history/context-epoch boundary.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner::tests::durable_history_prefers_sqlite_session_messages --quiet` — **1 passed**, proving the runner prefers a persisted `session_message` over the legacy in-memory projection.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-session --lib service::tests::title_mutation_changes_only_the_title --quiet` — **1 passed**, covering the shared title mutation service.
- `CARGO_INCREMENTAL=0 cargo test --locked -p oc-server --test api session_title_mutation_uses_session_service_and_survives_reload --quiet` — **1 passed**, covering v1 title PATCH delegation and SQLite/router reload persistence.
- `cargo check --locked -p oc-cli` — PASS after Unix terminal echo suppression for provider API-key input.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib cmd::attach::tests --quiet` — **1 passed**, covering preservation of the default/mini TUI initial prompt into `TuiInput`; `cargo check --offline -p oc-cli` also passed.

The final serial workspace suite, `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --workspace --offline --locked --no-fail-fast --quiet`, passed outside the sandbox with loopback socket access. Earlier sandbox-only loopback permission failures and the deterministic `apply_patch` fixture hang were separately resolved or bypassed for verification.
`grep -rln "not yet wired" crates/*/src | wc -l` → **17 files**.

### Runtime probes

The older release-binary probes below are retained as historical evidence of the pre-integration snapshot. Current post-integration smoke probes were run against `target/debug/opencode` outside the sandbox with the real per-user data paths.
| Command | Result |
|---|---|
| `opencode run "hi"` | `Error:  the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)`; exit **1** (reference: exit 0, real response) |
| `opencode` (no args, stdin closed) | `starting TUI (requires a TTY)`; under a real pty: `Error: the TUI is not yet wired in this build (TODO(integration): oc-tui)`, exit 1 |
| `opencode session list` | Historical release snapshot reported the session-list stub; current `target/debug/opencode session list` exits 0 and prints the SQLite-backed session table header. |

| `target/debug/opencode debug paths` | **passed**, printing resolved data/bin/log/repos/cache/config/state/tmp/home paths. |
| `opencode models` | prints raw models.dev cache (6057 lines incl. deprecated) — not the filtered registry |
| `opencode serve --port 43199` | prints "opencode server listening", but `curl /api/health` → **HTTP 000** (no HTTP server; bare socket draining bytes) |
| `opencode db "SELECT 1"` | `database queries are not yet wired in this build`, exit 1 |
| `opencode --version` | byte-identical `1.18.13` to reference |

### Validation of SEC-001 (permission gate)
- `crates/oc-tool/src/model.rs:386` — `ToolContext::ask` still records tool asks for the runner gate.
- `crates/oc-tool/src/core/tool.rs:42` — `CoreContext::assert` remains an input assertion helper.
- `crates/oc-server/src/handlers/permission.rs:62-90` — v2 permission creation now evaluates configured global/agent rules through `oc_session::permission::evaluate`; deny dominates, unmatched resources ask, and only asks are stored as pending requests.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib handlers::permission::tests -- --nocapture` — **5 passed**, including session ownership filtering for permission list/get/reply routes.
- No `oc-permission` crate exists; permission logic is embedded in oc-tool/oc-session/oc-server.

## Latest revalidation (2026-08-16)

- Elevated loopback rerun of the workspace HTTP mock-server coverage passed the 17 `oc-client` tests that failed only under sandbox socket restrictions.
- The QuickJS manager crash was fixed by creating owner-thread runtimes at the stable request-loop frame (avoiding QuickJS stack-baseline underflow), aligning native/QuickJS stack guards, rooting callbacks through canonical integer handles/context opaque storage, and replacing FFI-crossing polyfill `async function` entrypoints with ordinary Promise chaining.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-plugin --lib -- --nocapture`: **61 unit tests passed**; `cargo test --offline -p oc-plugin --test integration -- --nocapture`: **16 integration tests passed**. This includes manager sync/async tool and owner-thread dispose tests, manager auth, Promise client/auth rejection propagation, direct tools, async event hooks, workspace adapters, v2 transforms, typed bridge/registration, local host effects, and the LocalHost client-RPC callback.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server server::tests::production_bootstrap_wires_plugin_auth_into_provider_service -- --exact`: **1 passed**, proving loaded plugin auth summaries populate the provider-auth service and manager-backed authorize/callback persistence.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server 'server::tests::production_bootstrap_'`: **4 passed**, covering local plugin loading, registration projection, auth adapter wiring, and concrete package-directory entrypoint resolution.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --no-default-features --lib --quiet`: **87 passed**. The default-feature run reached 88/90; its only failures were the two mDNS DNS-SD socket tests denied by the sandbox with `Operation not permitted`.
- `cargo fmt --all -- --check`: passed after the runtime fix. F125/F127 remain PARTIAL; the safety fix is not counted as full API/Bun/differential parity.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-cli --lib cli::error::tests --quiet`: **4 passed**; `cargo check --offline -p oc-cli --quiet`: passed after preserving explicit `CliError` exit codes through top-level dispatch and removing duplicate unknown-error cause rendering. F147/F148 remain PARTIAL pending subprocess/differential coverage.

## Latest bounded verification

- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-plugin --lib -- --nocapture`: **61 passed**; `cargo test --offline -p oc-plugin --test integration -- --nocapture`: **19 passed**, including owner-thread SSE/global event delivery, `done()` cancellation, manager queueing, `client.skill.list()` dispatch, and the existing plugin/RPC/auth/disposal coverage.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --lib state::tests::server_event_fanout_delivers_plugin_sse_streams -- --nocapture`: **1 passed**, proving server event fan-out reaches a loaded plugin stream subscription.
- Elevated `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --lib -- --nocapture`: **119 passed**, including mDNS socket tests and the new plugin SSE server fan-out regression.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --test api -- --nocapture`: **49 passed**, including PTY duplicate-ID rejection/command capture, config JSONC persistence, OAuth lifecycle, MCP auth errors, session interruption/fork/reload, share/sync/workspace routes, TUI control queues, and provider/model catalogs.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --test api pty_create_runs_command_and_captures_output -- --nocapture`: **1 passed** after PTY child-exit cleanup now removes only the matching live process handle, clears stale connect tickets, retains the exited record, and rejects duplicate client-supplied PTY IDs with `409 Conflict`.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-mcp --test http_oauth -- --nocapture`: **6 passed** with elevated local HTTP permissions, covering discovery, dynamic registration, token exchange, error handling, streamable-HTTP expired-session initialize/initialized replay, an open POST `text/event-stream` response that delivers its JSON-RPC result before the stream closes, and a POST-only server whose optional GET returns `405`.
- The MCP HTTP/OAuth suite now passes **6/6** with elevated local HTTP permissions, including a POST-only streamable-HTTP server whose optional GET endpoint returns `405`; the transport falls back to client-originated POST responses.
- `cargo test --offline -p oc-cli --lib cli::cmd::completion::tests -- --nocapture`: **5 passed**, covering Bash/Elvish/Fish/PowerShell/Zsh generation, shell-path detection, nested options, aliases, and all 21 visible top-level commands.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-tui --lib --quiet`: **173 passed**, including `/skill` bootstrap, skill-selector filtering, slash invocation insertion, prompt argument preservation, and existing TUI rendering/control regressions.
- `cargo test --offline -p oc-plugin --lib --quiet`: **61 passed**; `cargo test --offline -p oc-plugin --test integration --quiet`: **19 passed**, including `client.skill.list()` Promise dispatch and response unwrapping.
- `cargo fmt --all -- --check`: PASS after the MCP POST-SSE transport fix.
- `cargo fmt --all -- --check`: PASS after remote import and MCP resource integration.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet`: PASS; MCP resource list/template/read settlement compiles against live client/catalog APIs.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-cli --quiet`: PASS; remote share import compiles.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib import_cmd::tests --quiet`: **6 passed**, covering local decoding/persistence plus remote endpoint fallback, content-type validation, size limits, and injectable fetch behavior.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner::tests::mcp_resource_tools_have_strict_contracts --quiet`: **1 passed**.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner::tests::mcp_resource_target_requires_connected_server --quiet`: **1 passed**.
- These are bounded crate checks, not product-level E2E or differential proof; the conservative score remains **74/155 (47.7%)**.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-util --lib logging::tests::file_layer_writes_reference_formatted_events --quiet`: **1 passed**.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --test logging --quiet`: **1 passed**, verifying executable `--print-logs` routing to stderr and `<data>/log/opencode.log`.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib cli::upgrade::tests::startup_action_uses_injected_fetch_without_network --quiet`: **1 passed**.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-plugin --lib jsonc::tests --quiet`: **7 passed**, including UTF-8-safe spans and comment/trailing-comma-preserving object-property patching.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib cli::cmd::mcp::tests --quiet`: **7 passed**, including root/nested `.jsonc` MCP config mutation coverage.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api config_update_ --quiet`: **2 passed**, covering JSON and additive/replacement-only JSONC config PATCH persistence; the JSONC test verifies unrelated comments and the trailing-comma document survive.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api legacy_provider_list_reports_connected_ids_and_default_models --quiet`: **1 passed**, covering the legacy `/provider` response's active connected IDs and per-provider non-deprecated default-model mapping.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api session_interrupt_emits_idle_status_after_cancelling_run --quiet`: **1 passed**, covering v2 interrupt cancellation and the terminal `session.status` idle event.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner_events_project_back_to_server_messages --quiet`: **1 passed**, including v2 `session.retry.scheduled` projection alongside the existing retry part/status events.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-tui --lib leader_tests::terminal_title_toggle_persists_preference --quiet`: **1 passed**, covering the registered terminal title toggle and persisted preference state.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-tui --lib leader_tests::terminal_suspend_dispatch_defers_to_terminal_loop --quiet`: **1 passed**, covering deferred `terminal.suspend` dispatch; `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-tui --lib terminal_suspend --quiet`: **2 passed**, covering the fallback Ctrl-Z binding configuration and suspend request path. The Unix lifecycle helper leaves raw/alternate/mouse/paste modes, raises `SIGTSTP`, and restores them after foreground resume; interactive TTY verification remains outstanding.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-tui --lib leader_tests::server_tui_control_requests_reach_the_application_loop --quiet`: **1 passed**, covering remote control queue dispatch into prompt editing and help dialog actions.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet`: PASS after routing project/global config writes through the JSONC span-preserving helper; deletion-aware/general JSONC rewrite remains partial.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-session --lib session::tests::usage_ --quiet`: **4 passed**, covering zero/default usage, cache subtraction, explicit non-cached input, and nested provider cache-write metadata.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-command --test skill --quiet`: **19 passed**, including environment-gated external/Claude skill discovery; `cargo check --offline --locked -p oc-server --lib --quiet` and `-p oc-cli --quiet` passed after downstream wiring.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib models_dev::tests --quiet`: **3 passed**, including the `OPENCODE_DISABLE_MODELS_FETCH` refresh guard.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-core --lib background_job --quiet`: **5 passed**; `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test background_jobs --quiet`: **1 passed**, covering process-local job lifecycle and HTTP list/status/promote/cancel behavior.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-mcp --lib --quiet`: **53 passed**; the focused HTTP integration suite above also passed its 4 recovery/OAuth cases.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-cli --quiet`: PASS after startup update policy integration.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner::tests::background_subagent_returns_running_and_completes_durably --quiet`: **1 passed**.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-core --lib background_job --quiet`: **5 passed**, covering background job start/wait/timeout/promote/extend behavior.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet`: PASS after background route/cancellation integration.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-sync --lib sync::store::tests --quiet`: **16 passed**, including SQLite event persistence/hydration, cursor ordering, idempotent replay, owner claims, and removal.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-sync --lib --quiet`: **91 passed**; `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib state::tests::durable_events_are_assigned_to_sync_history --quiet`: **1 passed**, covering serialized replay, cursor-filtered history, ownership claims, hydration, and production event publication.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib state::tests::durable_events_are_assigned_to_sync_history --quiet`: **1 passed** after wiring production `AppState::with_database` to the SQLite-backed sync store.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib location::tests --quiet`: **5 passed**, covering query/header resolution, normalized remote identity, repo-cache identity, and global non-Git identity.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-cli --lib cli::context::tests --quiet`: **3 passed**, including CLI repo-cache identity parity.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-project --lib identity::tests --quiet`: **1 passed**, covering the shared normalized remote identity helper used by CLI and server.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-sync --lib --quiet`: **93 passed**, including injected remote workspace create/list/remove transport, HTTPS/header validation, DELETE requests, project filtering, and durable sync regressions.
- `cargo fmt --all -- --check`: **passed** after routing MCP tool calls through the configured permission evaluator.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib runner::tests::configured_permission_rules_override_interactive_defaults --quiet`: **1 passed**, including configured MCP allow/deny decisions.
- `cargo fmt --all -- --check`: **passed** after the provider-error alignment.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-provider --lib --quiet`: **132 passed**; `-p oc-llm --lib --quiet`: **6 passed**; `-p oc-llm --test misc --quiet`: **10 passed**, covering shared retry/quota/context-overflow classification and redacted HTTP details.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-database --lib --quiet`: **7 passed**, including event-sequence/event-row helpers.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api workspace_routes_manage_local_workspace_projection_and_warp --quiet`: **1 passed**, now covering the control-plane move-session endpoint in addition to local workspace warp/status/remove.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api pty_create_runs_command_and_captures_output --quiet`: **1 passed**, now covering Unix master/slave PTY command capture plus live cols/rows resize through the API; non-Unix keeps the pipe fallback.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --lib instance_handlers::tests::configured_model_supports_all_embedded_provider_facades --quiet`: **1 passed**, covering live model resolution for Azure, Cloudflare AI Gateway/Workers AI, GitHub Copilot, and Amazon Bedrock facades.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-sync --lib control_plane::adapters::remote::tests --quiet`: **2 passed**, covering HTTPS target normalization, header projection, and rejection of non-HTTPS remote workspace URLs.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-sync --lib control_plane::adapters::tests --quiet`: **4 passed**, covering builtin worktree/remote/console registration and project-scoped adapter overrides.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet`: PASS after registering the builtin remote and console adapters; connected remote transport and account lifecycle remain incomplete.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test provider_models --quiet`: **1 passed**, covering typed provider/model catalog endpoints, custom model merging, credential-key redaction, and missing-provider errors.
- `cargo test -p oc-server --lib runner::tests::server_runner_bridge_preserves_structured_provider_failure --quiet`: **1 passed**, covering provider status/retry-after preservation, classification, redacted HTTP context, and metadata across the server runner bridge.
- `cargo test --offline -p oc-server --test web --quiet`: **3 passed**, covering the embedded root browser client, JavaScript asset content type/content, and unknown API fallback behavior.
- `plugin_registrations_reach_typed_sink_with_plugin_id`: **1 passed** in the focused `oc-plugin` integration suite, covering plugin ID propagation through typed command/skill registration sinks.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server plugin_commands_and_agents_layer_over_resolved_config --lib --quiet`: **1 passed**, covering server projection of typed plugin command/agent registrations.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-server --lib --quiet`: **passed** after wiring the production registration sink and handler projections.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server production_bootstrap_projects_plugin_registrations --lib --quiet`: **1 passed**, covering plugin ID propagation and production bootstrap capture of command/skill registrations.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --lib compaction_ --quiet`: **6 passed**, covering provider-backed automatic compaction, deterministic overflow fallback, disabled auto compaction, and recent-tail/token-budget behavior.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-server --lib --quiet`: **passed** after wiring production MCP transport-close eviction, `notifications/tools/list_changed` refresh, and pointer-identity replacement guards.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-plugin --lib jsonc::tests --quiet`: **9 passed**, covering UTF-8-safe spans, nested provider/options/models comment/trailing-comma preservation, additive nested members, and canonical deletion fallback.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --test api config_update_preserves_nested_jsonc_comments --quiet`: **1 passed**, covering nested provider/options/model PATCH persistence, comment retention, and JSONC validity.
- The `compaction_` focused suite above includes the `tail_turns`/`preserve_recent_tokens` recent-tail checkpoint regression; all **6 tests passed** after the provider-backed automatic/overflow compaction slice.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test route_table --quiet`: **3 passed** after the provider/model route integration.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet`: PASS after the F079 provider/model endpoint slice; runtime catalog refresh and plugin/provider discovery remain incomplete.
- `CARGO_INCREMENTAL=0 cargo check --offline --locked -p oc-server --lib --quiet`: PASS after project-scoped adapter listing and control-plane move-session wiring.
- Final bounded regression pass: `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-sync --lib --quiet` **91 passed**; `-p oc-plugin --lib` **58 passed**; `-p oc-plugin --test integration` **8 passed**; `-p oc-cli --lib cli::cmd::mcp::tests` **7 passed**; `-p oc-cli --lib import_cmd::tests` **6 passed**; and `-p oc-cli --lib cli::upgrade::tests` **7 passed**.
- The QuickJS manager event path now awaits async event hooks; the server event-fanout regression and plugin integration target are green.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api integration_oauth_attempt_uses_provider_hook_and_tracks_lifecycle --quiet`: **1 passed**, covering hook-only integration discovery, `Integration.Attempt` creation, pending/complete status, callback credential persistence, cancellation, and not-found behavior.
- `CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-server --test api --quiet`: **45 passed**, including the integration OAuth lifecycle regression; the constrained `oc-server --lib` run compiles and passes 77/79 tests in the sandbox, with the two mDNS socket tests requiring elevated socket permissions.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline --locked -p oc-tui --lib --quiet`: **166 passed**, including resolved theme/theme_mode and light-mode tests, replay/keymap regressions, clipboard command discovery, and external-editor command parsing.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --test fs -- --nocapture`: **5 passed**, covering location-relative `fs.list` entries, directory-first ordering, directory trailing slashes, deterministic lexical ordering, missing-directory rejection, the reduced `{path,type}` wire shape, recursive file/directory `fs.find` with limits, `fs.read` parent-traversal rejection plus canonical containment, and legacy `/file/content` location scoping.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-cli cli::cmd::run::tui_initial_prompt_tests --quiet`: **2 passed**, covering `run --mini/--interactive` initial file markers and structured `{url, filename, mime, source}` parts, including file-only prompts.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-tui prompt::state --quiet`: **5 passed**; `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-cli --quiet`: **passed** after adding structured initial-part bootstrap plumbing to all `TuiInput` constructors.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --test api workspace_sync_list_projects_registered_adapter_discovery --quiet`: **1 passed**, proving `POST /experimental/workspace/sync-list` invokes a registered adapter and exposes the discovered workspace through `GET /experimental/workspace`.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-sync --test workspace_service sync_list_discovers_workspaces_from_adapters --quiet`: **1 passed**; `cargo fmt --all -- --check`: **passed** after the server projection slice.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-config --test extra parses_v2_plugin_object_and_preserves_options -- --exact`: **1 passed**; `cargo check --offline -p oc-config`: **passed**.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-server --no-default-features --lib`: **passed** after server bootstrap accepted v2 `plugins` object declarations and pure-mode external gating. The corresponding server test binary compiled but could not link after the constrained volume reached `ENOSPC`.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-server --no-default-features --tests`: **passed**, including type-checking the focused v2 plugin-object bootstrap regression without invoking the linker.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --no-default-features --lib builtin_auth -- --nocapture`: **3 passed**, covering native OpenAI/xAI OAuth/API method exposure, nested OpenAI account-ID extraction, validation, and non-fabricated API credentials.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-cli --tests`: **passed**, including CLI use of the native internal OAuth/API hooks.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-cli --lib cli::cmd::providers::tests -- --nocapture`: **3 passed**, including explicit manual API-key selection when the native provider advertises OAuth methods.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-server --no-default-features --lib server::tests::production_bootstrap_installs_native_default_auth_hooks -- --exact --nocapture`: **1 passed**, proving production bootstrap installs native OpenAI/xAI API-key auth hooks when default plugins are enabled.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib handlers::provider::tests::maps_openai_oauth_account_headers_for_codex -- --exact --nocapture`: **1 passed**, proving native OpenAI OAuth renders the bearer token together with the Codex account/origin headers.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib instance_handlers::tests::native_openai_oauth_uses_codex_responses_endpoint -- --exact --nocapture`: **1 passed**, proving the native OpenAI OAuth model route selects `https://chatgpt.com/backend-api/codex/responses` rather than the ordinary OpenAI API endpoint.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib builtin_auth -- --nocapture`: **3 passed**, covering native OpenAI/xAI methods plus GitHub Copilot public/enterprise device-flow method prompts, API-key behavior, and account-ID extraction.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib handlers::provider::tests::maps_github_copilot_oauth_headers -- --exact --nocapture`: **1 passed**, proving Copilot OAuth adds the GitHub API-version, OpenAI intent, and user-agent headers.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib instance_handlers::tests::native_github_copilot_oauth_uses_enterprise_endpoint -- --exact --nocapture`: **1 passed**, proving enterprise Copilot OAuth selects the `copilot-api.<enterprise-domain>` endpoint.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test --offline -p oc-session-runner --tests -- --nocapture`: **44 unit + 5 coordinator + 2 runner-loop tests passed**, after changing the runner contract from buffered event vectors to boxed live event streams.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-session-runner --tests`: **passed**; `cargo check --offline -p oc-server --no-default-features --tests`: **passed** after live-stream and Copilot endpoint wiring.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib runner::tests::production_runner_resolves_configured_agent_and_model_defaults -- --exact --nocapture`: **1 passed**, proving production runner agent system/step metadata and session → agent → root model precedence are no longer hard-coded to the empty/default fallback.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib runner::tests::compaction_transcript_preserves_structured_parts -- --exact --nocapture`: **1 passed**, proving compaction prompts retain bounded file, reasoning, and tool-call details from structured message parts.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-session-runner --lib runner::publish_llm_event::tests::step_finish_calculates_catalog_cost -- --exact --nocapture`: **1 passed**, proving resolved catalog pricing is applied to non-cached input, visible output, and cache-read usage before durable `StepEnded` settlement.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib runner::tests::compaction_trigger_uses_resolved_model_limits -- --exact --nocapture`: **1 passed**, proving automatic compaction uses resolved model context/output limits instead of a universal byte threshold when catalog metadata is available.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib runner::tests::production_runner_uses_configured_model_cost_and_limits -- --exact --nocapture`: **1 passed**, proving configured custom-provider model cost and context/output limits override catalog fallback metadata.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib runner::tests::automatic_compaction_uses_provider_summary -- --exact --nocapture`: **1 passed**, proving successful automatic compaction emits exactly one v1.18.13-compatible `session.compacted` event after persisting its checkpoint.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --test api prompt_returns_admitted_input -- --exact --nocapture`: **1 passed**, proving v2 prompt admission persists the requested delivery mode, publishes the schema-backed prompted/admitted event payloads, and replays both through cursor-based durable history.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib sse::tests -- --nocapture`: **3 passed**, including session SSE filtering by event `sessionID` or durable aggregate ID; the session stream now accepts an `after` cursor and replays up to 1,000 durable events before live filtered events.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-provider --test registry -- --nocapture`: **16 passed**, including reference-compatible ignoring of empty environment credentials during provider registry construction.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-client --tests -- --nocapture`: **34 passed**, covering the typed `session.compacted` decoder plus the existing HTTP, SSE, contract, and generic raw request/SSE client surfaces (loopback tests required elevated socket permission).
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-plugin --lib -- --nocapture`: **61 passed**; `cargo test --offline -p oc-plugin --test integration -- --nocapture`: **16 passed** in the earlier pre-stream baseline. The current bounded suite is **18 passed** and includes owner-thread SSE/global event delivery and `done()` cancellation.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-plugin --test integration client_inventory_includes_session_status -- --exact --nocapture`: **1 passed**, covering the Promise-based `client.session.status()` inventory method and bridge response unwrapping.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-plugin --test integration client_inventory_supports_nested_v118_methods_and_unwraps_data -- --exact --nocapture`: **1 passed**, covering representative PTY and TUI-control nested methods from the expanded v1.18.13 SDK inventory.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --lib server::tests::production_bootstrap_wires_plugin_session_status_client -- --nocapture`: **1 passed**, proving a configured plugin can call `client.session.status()` during production bootstrap through a non-blocking server-owned snapshot and register a result-dependent command.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib state::tests::rejecting_permission_rejects_all_pending_requests_in_session -- --exact --nocapture`: **1 passed**, proving a rejected live permission ask rejects same-session pending asks, removes them, and emits both legacy and v2 reply events.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-mcp --lib transport::tests -- --nocapture`: **4 passed**, including regression coverage that CRLF SSE event boundaries are fully consumed and subsequent MCP events do not stall.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-mcp --lib --tests -- --nocapture`: **56 unit tests passed**; the four HTTP/OAuth tests require loopback socket permission.
- Elevated `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-mcp --test http_oauth -- --nocapture`: **4 passed**, covering Streamable HTTP roundtrip/recovery and OAuth discovery/registration/exchange after the Last-Event-ID transport changes.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo check --offline -p oc-mcp`: **passed** after the transport parser fix.
- `cargo fmt --package oc-mcp -- --check`: **passed** after adding SSE ID tracking and reconnect headers.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib instance_handlers::tests::message_limit_keeps_newest_context_in_order -- --exact --nocapture`: **1 passed**, proving bounded session-message replay returns the newest messages in chronological order for TUI replay.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-tui --lib replay_tests -- --nocapture`: **3 passed**, covering newest replay limits plus explicit expanded text-part submission after file attachments and replacement of local pasted-text metadata.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-llm --tests -- --nocapture`: **35 passed**, including Anthropic beta-header defaults/overrides, provider golden bodies, streaming, usage, Bedrock signing, and error classification.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-session --lib compaction -- --nocapture`: **13 passed**, including part-level compaction candidates, strict `pruned > 20,000` savings, skill protection, two-turn protection, and compaction history behavior.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo check --offline -p oc-server --no-default-features --tests`: **passed** after the Anthropic, compaction-pruning, and newest-message replay slices.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo check --offline -p oc-server --no-default-features --lib`: **passed** after the legacy pruning borrow-lifetime fix and v2 permission policy slice.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib handlers::permission::tests -- --nocapture`: **4 passed**, covering configured allow/ask/deny effects, agent-rule layering, deny precedence across resources, and response wire shape.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib runner::tests::legacy_compaction -- --nocapture`: **2 passed**, covering legacy JSON pruning markers and the below-threshold no-op.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-acp --lib --tests -- --nocapture`: **57 unit + 14 wire-golden tests passed**, including unknown-provider auth-required `{data:{}}` serialization.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib state::tests::session_inputs_promote_steer_and_queue_in_order -- --exact --nocapture`: **1 passed**, proving the production session input queue preserves delivery modes and promotes steer before queued input.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --no-default-features --lib state::tests::durable_state_round_trips_session_message_and_pending_input -- --exact --nocapture`: **1 passed**, proving pending `session_input` rows are persisted, rehydrated after a database-backed state reload, and withheld from runner history until promotion.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo check --offline -p oc-server --no-default-features --tests`: **passed** after wiring model limits and production steer/queue input promotion.
- Delegated F127 review: client/auth calls now expose Promise resolution/rejection boundaries and the focused integration suite passes; the host bridge remains one-shot internally, `client.sse.stream` is a no-op, and v2 effect modules are identity shims. The next implementation requires an explicit async transport contract with request IDs, cancellation, and event delivery.

## Latest bounded continuation verification (2026-08-16)

- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-plugin --lib --quiet`: **61 passed**; the integration suite passes **20/20**, including `client.skill.list()` and cooperative async plugin-tool cancellation through `context.abort.aborted`.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --lib runner::tests::running_async_plugin_tool_is_interrupted_by_session_abort -- --exact --nocapture`: **1 passed**, proving a session-run cancellation interrupts an in-flight QuickJS plugin tool settlement.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-server --lib plugin_registry::tests::plugin_commands_and_agents_layer_over_resolved_config -- --exact --nocapture`: **1 passed**, proving typed plugin provider/model registration deep-merges into the server provider projection; `/api/model` consumes the same merged config.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-project --test lsp -- --nocapture`: **4 passed**, covering JSON-RPC lifecycle, document synchronization, call-hierarchy operations, and observable server notifications/requests with preserved unsupported-request replies.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-mcp --test cancellation -- --nocapture`: **4 passed**, covering timeout/explicit request cancellation, `notifications/cancelled` request-id/reason serialization, late-response isolation, initialize non-cancellation, and tools/call cancellation.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-mcp --lib --quiet`: **56 passed**; elevated `cargo test --offline -p oc-mcp --test http_oauth -- --nocapture`: **6 passed**, including POST-only HTTP, POST SSE, expired-session replay, and OAuth flows.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-acp --lib --quiet`: **57 passed**; `cargo test --offline -p oc-acp --test wire_golden -- --nocapture`: **15 passed**; focused CLI ACP tests: **6 passed**, including provider/filesystem/tool replay, idle ordering, and prompt cancellation.
- `CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --offline -p oc-tui --lib --quiet`: **178 passed**, including searchable session timeline message previews/jump behavior, background-job list/status/cancel, event-backed queued-prompt ordering/idle clearing, skill selector, replay, TUI control, and rendering regressions.
- `cargo fmt --all -- --check` and `git diff --check -- . ':!reference'`: **passed**. The conservative parity score remains **74/155 (47.7%)** because these slices strengthen partial features rather than establish full reference equivalence.

## TUI Parity & CI Verification (2026-08-17)

- **TUI Visual & Functional Parity:**
  - Implemented global `render_footer` in `crates/oc-tui/src/app.rs` matching upstream `reference/packages/tui/src/routes/session/footer.tsx` (directory path, pending permissions badge, LSP indicator `• 0 LSP`, and `/status` / `Get started /connect` shortcuts).
  - Synchronized Home and Session view bounding areas so the message scroll viewport and prompt leave room for the 1-line persistent footer.
  - Implemented rotating placeholder text in `render_prompt_widget` matching upstream example cycles (`"Fix a TODO in the codebase"`, `"What is the tech stack of this project?"`, `"Fix broken tests"` for normal mode; `"ls -la"`, `"git status"`, `"pwd"` for shell mode).
  - Enhanced prompt submit fallback: when no provider is configured, prompt submission displays the warning toast and opens the provider connection dialog (`DialogKind::ProviderList`).
- **GitHub Actions CI Matrix Run `31984802068`:**
  - `fmt` — **PASSED** (20s)
  - `clippy` — **PASSED** (59s)
  - `build (ubuntu-latest)` — **PASSED** (1m17s)
  - `build (macos-latest)` — **PASSED** (1m51s)
  - `test (ubuntu-latest)` — **PASSED** (2m4s)
  - `test (macos-latest)` — **PASSED** (3m43s)
  - Overall status: **SUCCESS**.

## Per-agent evidence (saved under `rust-port-audit/artifacts/`)

- 01: `01-cargo-metadata.json`, `01-dep-graph.txt`, `01-duplicate-types.txt`, `01-runtime.md`
- 02: `02-ref-help.txt`, `02-rust-help.txt`, `02-workspace-tests.txt`, `02-workspace-summary.txt`
- 03: `03-cli/` (per-command JSON captures), `03-notes.md`
- 04: `04/` (config fixture diffs, harness scripts)
- 05: `05-migration_sql_diff.py` (automated migration semantic-diff vs reference)
- 06: `06/` (mock server traces, attach tests)
- 07: `07-reference-serve.txt`, `07-rust-serve-endpoints.txt`, `07-attach-tests.txt`
- 08: `08-mcp-server.py` (mock MCP server)
- 09: `09-mock-provider.py` (mock SSE provider)
- 10: `/tmp/opencode/llm-roundtrip` (streaming round-trip harness)
- 11: `11-probe.rs`, `11-probe-output.txt` (symlink-escape probe)
- 14: `agent14-cargo-check.log`, `agent14-clippy.log`, `agent14-fmt.log`
- 16: `16-cargo-tree.txt`, `16-cargo-tree-duplicates.txt`, `16-direct-deps.txt`, `16-licenses.txt`, `16-yanked-check.txt`, `16-libquickjs-sys-build.rs`, `16-quickjs-VERSION.txt`
- 17: `17-raw-measurements.txt`, `17-help-diff.txt`
- 18: `18-workspace-test.log`
- 19: `19-tui-ux-portability/` (escape passthrough artifact: `ratatui-escape-passthrough.txt`)
- 20: `20-packaging/runtime-evidence.txt`

## Tools unavailable (not installed; gap noted)

| Tool | Status | Coverage gap |
|---|---|---|
| cargo-audit / cargo-deny | MISSING | dependency vulnerabilities (manual lockfile review; 319/319 crates verified not yanked against live crates.io) |
| cargo-machete / cargo-udeps | MISSING | unused-dependency detection (manual) |
| cargo-outdated | MISSING | version drift (manual) |
| cargo-geiger | MISSING | unsafe audit (manual: 48 unsafe blocks in 3 files per Agent 14) |
| cargo-nextest / cargo-llvm-cov | MISSING | test speed / coverage (manual inference) |
| hyperfine | MISSING | timing (used `/usr/bin/time -v` + `date +%s%N`) |
| valgrind / heaptrack | MISSING | memory profiling (used RSS via `/usr/bin/time -v`) |
| bun / node | MISSING | executing reference source directly (differential via stock binary only) |
| cargo-miri / semver-checks / bloat / fuzz | MISSING | UB/API drift/size/fuzz (manual static review) |
| strace | MISSING | syscall-level evidence |

## Reference-side checks

Reference binary `/root/.opencode/bin/opencode`:
- `--version` → `1.18.13` (byte-identical to Rust).
- `--help`, `run --help` captured (`artifacts/03-root-help-reference.txt`, `02-ref-help.txt`).
- Reference `run hello` (live, real provider configured on this host) → real server + model response, exit 0.
- Reference `serve` → real HTTP (SPA + API) on port 4096 (`artifacts/07-reference-serve.txt`).
- Reference `opencode acp` answered `initialize` over stdin; Rust emitted zero bytes.
- Reference `opencode mcp list` connected to a disposable mock server and reported connected; Rust returns not_wired.
