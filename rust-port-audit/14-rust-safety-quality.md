# Agent 14 — Rust Safety, Correctness, and Code Quality

Audit of the opencode-rs Rust port (20 crates, 702 `.rs` files, ~167,496 LOC) with focus on
unsafe/FFI, panic paths, unwraps, integer handling, blocking-in-async, resource lifetimes,
clippy/fmt/check hygiene, and the explicit-search terms.

- Rust commit audited: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c` (working tree READ-ONLY, clean)
- Toolchain: rustc/cargo 1.97.1, Linux x86_64
- Reference spec consulted: vendored `reference/` (v1.18.13) + bundled `libquickjs-sys` C sources

## Scope

Every crate, with deep dives into:
- `crates/oc-plugin/src/js/runtime.rs` — the only substantial unsafe surface (46/48 unsafe blocks)
- `crates/oc-util/src/util/process.rs` — `libc::kill` (1 block)
- `crates/oc-database/src/sqlite.rs` — `unsafe load_extension` boundary (1 block)
- `crates/oc-core/src/git.rs`, `crates/oc-mcp` (crypto + transports), `crates/oc-session` (usage
  accounting), `crates/oc-llm` (tool runtime, AWS event-stream transport), `crates/oc-tool`
  (tools + sync ripgrep), `crates/oc-sync` (event store).

## Repository areas inspected

- All `unsafe` code in the workspace (verified by full-source grep — 48 blocks, listed in
  `artifacts/agent14-unsafe-lines.txt`).
- Bundled `libquickjs-sys-0.1.0` source: `quickjs.h` tag enum (lines 65–75), `JS_VALUE_HAS_REF_COUNT`
  (line 226), `JS_FreeValue` inline (line ~555), `JS_NewCFunctionData` (quickjs.c:4493),
  `JS_ExecutePendingJob` (quickjs.c:1419), bindgen signatures (`bindings.rs:2874–2880`).
- tokio-1.53.1 `process/mod.rs` for `kill_on_drop` default.
- oc-plugin (loader, bridge, host, npm, js value/transpile), oc-util (process, ripgrep, rpc,
  error, filesystem), oc-database sqlite, oc-core (git, background_job, bus, process),
  oc-mcp (auth, crypto, stdio transport), oc-llm (tool_runtime, route/protocol, transport,
  provider_error), oc-session (session/get_usage, processor, message_updater),
  oc-tool (grep/glob/shell_prompt/registry), oc-schema (event, session_message), oc-sync (store),
  oc-cli (not-yet-wired stubs).

## Commands executed

| Command | Result |
|---|---|
| `cargo check --workspace --all-targets --all-features --message-format short` | **EXIT 0 (PASS)** — 11 warnings, all in test targets (oc-tui, oc-tool, oc-schema, oc-project, oc-plugin, oc-config, oc-command, oc-acp). Log: `artifacts/agent14-cargo-check.log` |
| `cargo clippy --workspace --all-targets --all-features --message-format short -- -D warnings` | **EXIT 101 (FAIL)** — oc-plugin lib (13), oc-plugin lib-test (16), oc-util lib (7), oc-util lib-test (12), oc-schema lib (3), oc-schema lib-test (3). Log: `artifacts/agent14-clippy.log` |
| `cargo fmt --all -- --check` | **EXIT 0 (PASS)**. Log: `artifacts/agent14-fmt.log` (empty) |

### Clippy failures of note
- `oc-plugin/src/js/runtime.rs:32` — `transmute` without type annotations (sound; style).
- `oc-plugin/src/js/runtime.rs:233` — `Arc<Mutex<...>>` wrapping a non-`Send`/`Sync` type
  (defensive; makes `Runtime` `!Send + !Sync` — see RUST-005).
- `oc-util/src/ripgrep/mod.rs:289` — `rows.len() >= input.limit + 1` → clippy suggests
  `> input.limit` (equivalent; also removes a `usize::MAX` debug-overflow edge, RUST-011).
- `oc-util/src/util/rpc.rs:275` — non-binding `let` on a future — inside `#[tokio::test]`; deliberate.
- `oc-schema/src/session_message.rs:340,419` and `v1/session.rs:913`, `oc-plugin/src/loader.rs:300`
  — `large_enum_variant` (304–648 byte enums; perf, not safety).
- `oc-util` `type_complexity` (formatter.rs:52, ripgrep/mod.rs:125,160), `derivable_impls`
  (process.rs:25,45), `manual_strip` (proxy_env.rs:84) — style.
- oc-plugin test target: `oc-plugin/src/npm.rs:192` "items after a test module", `transpile.rs`
  collapsible-if/single-match/needless-borrow — style.

## Runtime scenarios attempted

None could be executed: the in-process QuickJS plugin host is not wired into any production
binary (only `oc-plugin/examples/`, tests), and the reference executes on Bun (missing).
All crash/UB claims below are STATIC proof (source + bundled C-source verification), unless
marked RUNTIME.

## Architecture or behavior summary

- The port keeps **all** unsafe inside three files: `oc-plugin/src/js/runtime.rs` (QuickJS FFI),
  `oc-util/src/util/process.rs` (one `libc::kill`), `oc-database/src/sqlite.rs` (one
  `load_extension`). There are **no** `unsafe impl Send/Sync` anywhere and no raw-pointer
  smuggling into safe APIs.
- `Runtime` holds raw `*mut JSRuntime/*mut JSContext` plus an `Arc<Mutex<CallbackRegistry>>`.
  It is deliberately `!Send + !Sync` (clippy warning is the compiler enforcing single-thread use).
- Callback trampoline design is stable-pointer based (verified against bundled
  `JS_NewCFunctionData`, which copies the data JSValue **by value** and `JS_DupValue`s it;
  tagging the data JSValue `TAG_NULL` makes that dup a no-op so QuickJS never frees the Rust
  closure). Drop order (`JS_FreeContext` → `JS_FreeRuntime` → registry Arc drop) prevents
  use-after-free of boxed closures.
- JS is evaluated synchronously; promises are driven by pumping the QuickJS job queue.

## Positive observations

1. `free_value` (runtime.rs:30) exactly mirrors the bundled `JS_FreeValue` semantics:
   `JS_VALUE_HAS_REF_COUNT(v)` ⟺ `tag < 0` for this QuickJS build; `JSRefCountHeader{ref_count: c_int}`
   layout matches bindgen. Manual refcount decrement + `__JS_FreeValue` is correct.
2. All hard-coded `TAG_*` constants (runtime.rs:16–23) match the bundled `quickjs.h` enum
   (JS_TAG_STRING = -7 is correct for this build — verified, not assumed).
3. `pump_jobs` (runtime.rs:279) ignores `JS_ExecutePendingJob` return **correctly**: the bundled
   C returns `int` and `JS_FreeValue`s the result internally (quickjs.c:1419). No leak. (I first
   suspected a leak — cleared by reading the C.)
4. `exec_callback` wraps the callback body in `catch_unwind` so Rust panics inside plugin
   callbacks are converted to a JS exception (except the one path in RUST-001).
5. `get_usage` (oc-session/src/session.rs:229) clamps NaN/±inf/negatives to 0.0 — no overflow
   or NaN in token/cost accounting. Verified.
6. `crypto.rs` (oc-mcp) — hand-rolled SHA-256 passes NIST vectors + RFC 7636 PKCE example;
   `random_bytes` is getrandom-backed (`uuid::Uuid::new_v4()`), acceptable entropy for OAuth
   `state`/verifier. `base64url` padding logic correct.
7. AWS event-stream decoder (oc-llm/src/route/transport.rs:300–460) has proper bounds checks
   (frame-length, header-length, CRC validation) before all slices.
8. `Sqlite::lock` maps poisoning to `Error::Poisoned` (no `unwrap` on poisoned mutex), and
   `load_extension` enable/disable is balanced on all paths.
9. oc-core git service is fully async (`tokio::process`), uses per-repo keyed mutex, and models
   errors as typed tagged unions mirroring the reference.
10. `KillOnDrop` RAII guard in oc-util ripgrep kills the rg child on drop/cancellation.
11. `cargo fmt` clean; workspace check clean; 0 FIXME/HACK/XXX/unimplemented!; `unwrap_or` /
    `unwrap_or_else` preferred over `unwrap` in most conversion paths.

## Findings summary

| ID | Severity | Confidence | Area | One-line |
|----|----------|-----------|------|----------|
| RUST-001 | **High** | CONFIRMED (static) | oc-plugin FFI | `.unwrap()` inside the C trampoline error path panics across `extern "C"` (UB) when an error/exception string contains `\0` |
| RUST-002 | **High** | CONFIRMED (static) | oc-plugin FFI | `to_value_inner` recurses unboundedly on self-referential arrays → stack overflow / process abort |
| RUST-003 | **Medium** | CONFIRMED (static) | oc-plugin FFI | Negative/`Proxy`-spoofed array `length` → `0..2^32` loop → CPU DoS |
| RUST-004 | **Low/Medium** | CONFIRMED (static) | oc-util | `libc::kill` errors only `trace!`-logged; abort/kill failures are silently suppressed |
| RUST-005 | **Low** | CONFIRMED (static) | oc-plugin, oc-core | `.lock().unwrap()` poisoned-mutex panic patterns (Runtime, ModuleResolver, background_job) |
| RUST-006 | **Medium** | CONFIRMED (static) | oc-tool, oc-plugin | Blocking `std::process`/`reqwest::blocking` on the tokio executor (sync tools; latent npm install) |
| RUST-007 | **Medium** | CONFIRMED (static) | oc-mcp | MCP stdio child processes leak on drop (tokio `kill_on_drop=false`); no `Drop` guard on transport |
| RUST-008 | **Medium** | CONFIRMED (static) | oc-plugin FFI | `pump_jobs` unbounded — plugin timer churn hangs the calling thread forever |
| RUST-009 | **Low** | CONFIRMED (static) | oc-plugin | `v2 effect` API is a stub; `tool.metadata` bridge returns `Null` (documented gaps) |
| RUST-010 | **Low** | CONFIRMED (static) | oc-session, oc-tool | `unreachable!()`/`panic!()` invariants on provider-output / template paths; currently unreachable, fragile |
| RUST-011 | **Low** | CONFIRMED (static) | oc-util | `input.limit + 1` debug-overflow only at `usize::MAX`; limits are hard-coded 100 in callers — not reachable |
| RUST-012 | **Low** | CONFIRMED (static) | oc-database | `allow_extension` config flag is never consulted; `load_extension` API ungated (dead-but-exposed today) |
| RUST-013 | **Low** | CONFIRMED (static) | oc-plugin FFI | Lone-surrogate JS strings fail `CStr::to_str()` → value cannot cross FFI (parity gap vs Bun UTF-16) |
| RUST-014 | Info | CONFIRMED | workspace | 1246 `.unwrap()` / 159 `.expect()` in non-test code — most infallible (regex/json!/take on piped handle); only RUST-001 is genuinely triggerable |
| RUST-015 | Info | CONFIRMED | workspace | clippy `-D warnings` fails (oc-plugin/oc-util/oc-schema); fmt + check pass; no MSRV `rust-version` declared |

## Detailed findings

### RUST-001 — Panic unwinds across the C trampoline (`extern "C"`) — UB
- Location: `crates/oc-plugin/src/js/runtime.rs:412-423` (specifically `.unwrap()` at **414**),
  trampoline `unsafe extern "C" fn` at **123-137**.
- Path: `exec_callback` (487–503) returns `Err(JsError)` → wrapper error branch builds
  `JsValue::String(err.to_string())` → `serialize_value` → `make_cstring` fails with
  `JsError::StringWithZeroBytes` if the message contains `'\0'` → **`.unwrap()` panics** inside
  `extern "C"` `trampoline` (invoked from QuickJS C) → with default `panic=unwind` this is UB
  (abort/stack corruption).
- Reachable from: any plugin that throws `new Error("\u0000")` (or any bridge call whose error
  string contains NUL), and any Rust callback that returns a string containing NUL (e.g. file
  content read via `from_utf8_lossy`). Plugin boundary is exactly where third-party code runs.
- The reference runs plugins in a Bun subprocess, so a misbehaving plugin cannot crash the main
  process; this port removes that isolation. The surrounding `catch_unwind` does **not** cover
  this panic (it wraps only `callback.call`, not the post-call serialization).
- Fix: replace `.unwrap()` at runtime.rs:414 with a `match`/`unwrap_or_else` that falls back to a
  NUL-free sentinel message (or `JsValue::Null`).

### RUST-002 — Unbounded recursion on cyclic arrays → stack overflow
- Location: `crates/oc-plugin/src/js/runtime.rs:639-663` (`to_value_inner`, array branch).
- The `visited` cycle set (666) is only consulted for non-array objects; arrays are converted
  before any cycle check. A plugin returning `const a = []; a.push(a); a` from a tool/trigger
  recurses forever → stack overflow → process abort. STATIC proof (no RUNTIME run).
- Fix: insert the array identity into `visited` too, or depth-limit the traversal.

### RUST-003 — Proxy/negative `length` → near-infinite loop
- Location: `crates/oc-plugin/src/js/runtime.rs:643-652`. `len` is read from a JS `length`
  property and cast `len as usize` (i32 → usize). A negative value (reachable via a `Proxy`
  `get` trap or a hostile getter) becomes up to 4,294,967,295 → the `for index in 0..len as usize`
  loop issues that many `JS_GetPropertyUint32` calls — CPU DoS. Plugin-triggerable.
- Fix: reject `len < 0` (and cap `len`).

### RUST-004 — Kill-path error suppression
- Location: `crates/oc-util/src/util/process.rs:230-238` (`kill_pid`), 245-259 (`attach_abort`).
- `libc::kill(pid as i32, signal)` — `pid as i32` cast is safe on Linux (pid_t is i32). But `-1`
  results are only `tracing::trace!`-logged; the abort flow never learns the process is gone and
  still `sleep`s `timeout_ms` before SIGKILL. Matches reference semantics loosely, but a failed
  kill is invisible. Note: `attach_abort` spawns an unbounded detached task per spawn.

### RUST-005 — Poisoned-mutex `unwrap()` patterns
- `runtime.rs:427` `self.callbacks.lock().unwrap()`; `loader.rs:53,107,114` cache locks;
  `oc-core/src/background_job.rs:123,133,142,184,198,236,253`. All convert a poisoned lock into a
  panic cascade. Practical risk is low (locks are held only for non-panicking map ops, never
  across `.await` in background_job — verified), and tokio swallows panics in spawned tasks, but
  `Runtime`/`ModuleResolver` locks would panic the calling thread on poisoning.

### RUST-006 — Blocking calls on the tokio executor
- oc-tool tools are **synchronous** handlers wrapped via `sync_execute`
  (`oc-tool/src/tool/tool.rs:25-32`) and awaited inside the async LLM loop
  (`oc-llm/src/tool_runtime.rs:62-77`). `oc-tool/src/ripgrep.rs:119-124` runs
  `std::process::Command::new("rg").output()` on the executor thread (whole-repo scans), and
  grep/glob/write/edit do blocking fs/process work. Stalls SSE/other tasks while a tool runs.
  Medium (systemic design choice, matches "no async everywhere" port).
- `oc-plugin/src/npm.rs:117` (`reqwest::blocking::Client`) and `:172` (`git clone` via
  `std::process::Command`). Today `npm::add` is only reachable from examples/tests (plugin host
  not wired — `cargo grep` finds no production caller), so this is a **latent** block-on-install
  that must not be called from an async context after integration.

### RUST-007 — MCP child-process leak on drop
- `oc-mcp/src/transport/stdio.rs:158-169` has an explicit `close()` that kills+waits, but there is
  **no `Drop` impl** on `StdioTransport`/`Client`, and tokio's `Child` defaults to
  `kill_on_drop: false` (verified in tokio-1.53.1 `src/process/mod.rs:1099`). Any drop without
  `close()` (abnormal exit, runtime shutdown, `start()` failure after spawn at stdio.rs:73-87)
  orphans the MCP server process. The manager's `close_all` covers normal shutdown only.

### RUST-008 — `pump_jobs` unbounded loop
- `runtime.rs:279-288`: `while JS_IsJobPending { JS_ExecutePendingJob }`. A plugin that schedules
  a repeating timer/`queueMicrotask` that re-enqueues work makes this loop never terminate,
  hanging the calling thread (and any tokio task it runs on). Reference (Bun) event loop is
  non-blocking, so this is a divergence + DoS. Documented timers limitation, but the hang is
  unbounded.

### RUST-009 — Documented stubs (classified: documented-temporary)
- `oc-plugin/src/lib.rs:23` v2 effect API stub; `bridge.rs:82` `tool.metadata` → `Null`;
  `oc-cli` "not yet wired" commands (attach/export/db/agent/run --mini); `oc-sync`
  `StubDeps` (test/control-plane helper). All carry `TODO(integration)` (266 total) — consistent
  with CONTEXT.md workflow. No FIXME/HACK/XXX anywhere. These are honest, loud stubs.

### RUST-010 — Invariant panics on provider/template paths
- `oc-llm/src/llm.rs:214` `unreachable!()` after a `generateObject` event-type guard (defensive;
  fires only if a provider emits an unexpected event after the guard — a panic instead of a
  graceful error, LOW).
- `oc-tool/src/tool/shell_prompt.rs:70` `render_prompt` panics on `${key}` not in `values` —
  verified the static `prompts/shell.txt` template keys are all supplied today, so currently
  unreachable, but any template/values drift turns a config error into a session crash; the
  reference renders `undefined` instead of crashing.
- `oc-schema/src/event.rs:65,87` duplicate-durable-definition panics at registration time
  (startup invariant, matches reference throw-on-duplicate).

### RUST-011 — Integer/overflow audit (all clear or unreachable)
- `get_usage` (oc-session/src/session.rs:229-…) f64 with NaN/neg clamping — verified.
- `oc-util/src/ripgrep/mod.rs:289` `limit + 1` debug-overflow needs `limit == usize::MAX`;
  all production callers hard-code `limit: 100` (oc-tool grep/glob) — not reachable; clippy
  already flags it.
- `oc-database` `value_to_json`/`json_to_sqlite`: non-finite f64 → `Null`/`Text`, no panics.
- AWS event-stream `u32`→`usize` casts bounded by buffer-length checks (RUST-… positive above).
- `process.rs:298` `status.code().unwrap_or(if signal…)` — no cast panic.

### RUST-012 — `allow_extension` guard dead
- `crates/oc-database/src/sqlite.rs:342-355` — `load_extension` (the only `unsafe` block besides
  process.rs/runtime.rs) never consults `Config.allow_extension` (default `false`). The unsafe
  block itself is sound: enable/disable balanced, serialized by the connection mutex, same trust
  boundary as the reference. But the config flag is unenforced and `load_extension` has no
  production caller today — dead-but-exposed.

### RUST-013 — FFI string encoding gap
- `runtime.rs:627-637` converts JS strings via `JS_ToCStringLen` + `CStr::to_str`. QuickJS emits
  invalid UTF-8 (WTF-8-style) for lone surrogates, so such strings fail `to_str()` →
  `JsError::InvalidString` and cannot cross into Rust. Parity gap vs Bun (UTF-16). The main
  bridge is JSON-string typed, so impact is limited to direct `call_function` args/returns with
  lone surrogates.

### RUST-014 — unwrap census
- 1246 `.unwrap()` + 159 `.expect()` matched outside obvious test markers (over-counts
  `#[cfg(test)]` bodies). Reviewed hot spots: oc-mcp/auth.rs, oc-sync/store.rs, oc-core/bus.rs,
  oc-util fs_util/filesystem.rs, oc-session-runner/publish_llm_event.rs — all test-only.
  Production unwraps are overwhelmingly (a) `Regex::new` on static patterns
  (executor.rs:27,244,267 — note :244/:267 recompile the regex on **every** 429/content-filter
  error, a small perf smell), (b) `json!().as_object().unwrap()` (variants.rs — infallible),
  (c) `.take().expect("stdout pipe")` on guaranteed-piped handles (process.rs:338-339,
  ripgrep:258,266). Genuinely triggerable panics: RUST-001 only.
- `provider_error.rs:8-22` — 10 `Regex::new(...).unwrap()` at module init (infallible, but do
  them once — they are `Lazy`).

### RUST-015 — Build hygiene
- check: PASS (11 warnings, all test targets). fmt: PASS. clippy `-D warnings`: **FAIL** on
  oc-plugin (13 lib), oc-util (7 lib), oc-schema (3 lib) + test targets. Details in
  `artifacts/agent14-clippy.log`. No `rust-version` field → no MSRV claim; `edition = "2021"`.
- Feature-flag/platform: `#[cfg(windows)]` present (CREATE_NO_WINDOW, taskkill path); unverified
  on this Linux host (see Unverified).

## Feature or behavior gaps

1. Plugin crash isolation lost: reference sandboxes plugins in a Bun subprocess; this port runs
   them in-process, so RUST-001/RUST-002 are crashes/UB of the whole process that the reference
   would contain.
2. Plugin host not wired into any production path (266 `TODO(integration)`); `npm::add` blocking
   IO and the FFI defects are latent until integration.
3. Timers ignore wall-clock (documented) and `pump_jobs` can hang (RUST-008).
4. v2 effect API stub; `tool.metadata` no-op; built-in auth plugins not ported (documented).
5. Lone-surrogate JS strings cannot cross the FFI (RUST-013).
6. MCP server processes leak on non-clean shutdown (RUST-007).

## Test coverage gaps

- No test drives a NUL byte through the callback error path (would catch RUST-001).
- No test returns a cyclic array / self-referential structure to Rust (RUST-002).
- No test with `Proxy`-spoofed `length` (RUST-003).
- No test for plugin timer churn against `pump_jobs` (RUST-008).
- No `miri`/sanitizer pass over the FFI (tooling absent); no multi-thread test asserting
  `Runtime` single-thread confinement (enforced by types only).
- `cargo test` was not run by this agent (shared target dir + time); workspace test log exists
  from agent 18.

## Unverified areas

- Windows `#[cfg(windows)]` paths (taskkill, CREATE_NO_WINDOW) — BLOCKED (Linux host).
- Actual behavior of QuickJS GC/refcounts under adversarial plugins — would need miri or a live
  harness (BLOCKED_BY_MISSING_EVIDENCE; static C-source review done).
- oc-tui, oc-acp, oc-command, oc-server internals audited only via grep-level for the listed
  categories, not line-by-line.
- 16 oc-plugin + 12 oc-util test-target clippy errors not individually enumerated.

## Final domain verdict

**READY_WITH_MINOR_REMEDIATION**

Rationale: the workspace compiles clean (`cargo check` PASS), `cargo fmt` PASS, unsafe is
contained to 48 blocks in 3 files with a sound, documented ownership design, and the core crates
(git, database, session, LLM, schema, MCP crypto) are well-formed with correct error handling and
no reachable panic paths I could find. However, the QuickJS FFI layer in `oc-plugin` contains two
plugin-reachable crash/UB defects (RUST-001 panic-across-`extern "C"`, RUST-002 cyclic-array stack
overflow) and one CPU-DoS (RUST-003), clippy `-D warnings` fails on three crates, and MCP
subprocesses leak on drop. **Before the plugin host is wired into the production binary, RUST-001,
RUST-002, and RUST-003 must be fixed and oc-plugin should pass clippy.** The plugin crate itself
should be treated as NOT_READY until then.
