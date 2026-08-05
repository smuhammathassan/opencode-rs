# Plan 15 — Plugin Runtime: maintained engine, safety primitives, containment, compat (Agent 15)

**Domain:** 12-plugin / 13-security / 14-rust / 16-supply · **Branch:** `fix/audit-remediation` · **Phase:** Wave 0 (READ-ONLY plan)
**Owner:** Agent 15 · **Depends on:** Agent 02 (composition root; production plugin service), Agent 08 (permission/ask write path), Agent 18 (reference-capture fixtures for compat goldens). Blocked-from: nothing.

---

## 1. Owned consolidated findings

| ID | Severity | Verdict | Evidence (audit + this pass) |
|---|---|---|---|
| PLUGIN-001 | High (blocker) | **Confirmed** | `Runtime::new` (js/runtime.rs:218-242) never calls `JS_SetMemoryLimit`/`JS_SetInterruptHandler`; grep across crate = 0 matches. RUNTIME R5: `for(;;){}` hangs process. `pump_jobs` (runtime.rs:279-288) loops forever on an unresolvable promise. |
| PLUGIN-002 | High (blocker) | **Confirmed** | `ModuleResolver::resolve_path` (loader.rs:67-78) returns absolute specs unchanged with no containment; `resolve` bridge (bridge.rs:44-51) + `__oc_require`/`new Function` (runtime.js:112-117). RUNTIME R4: `/tmp/.../evil.js` executed. Reference has `resolvePackageFile` containment (shared.ts:89-97) — port dropped it (PLUGIN-012). |
| PLUGIN-003 | Medium | **Confirmed** | `__oc_event` schedules hooks as `Promise.resolve().then(...)` (runtime.js:769-779); `LoadedPlugin::event` (host.rs:124-129) uses `call_function` without `pump_jobs`, so the microtask never runs. Test `triggers_hooks` asserts nothing observable. |
| PLUGIN-004 | Medium | **Confirmed** | `setTimeout`/`setInterval` ignore delay (runtime.js:202-213). RUNTIME R6: `setTimeout(cb,5000)` fired at 0 ms. Real timers + real-timer I/O deadlock the host. |
| PLUGIN-006 | Medium | **Confirmed** | Hand-written `transpile.rs` corrupts valid TS/JS: postfix `!` dropped in `config.directory!` position, `catch(e:any)` → `SyntaxError`, generic arrows/overloads corrupt, `enum`/`namespace` silently erased, decorators pass through. RUNTIME R8. |
| PLUGIN-008 | Medium | **Confirmed** | RUNTIME R1: real `reference/packages/plugin/src/example.ts` fails to load (`./index.js` missing — package exports point at `.ts` sources that import `zod`, `effect`, `@opencode-ai/sdk`). `tests/fixtures/*` are adapted shims, not reference sources. |
| PLUGIN-004(rev) | Medium | **Confirmed** | Plugin host TEST-ONLY: `opencode plugin` is a `not_wired` stub; oc-server/oc-session/oc-tool have `TODO(integration)` only. All 52 tests + examples are the only callers. 001/002 latent today. |
| RUST-001 | High (blocker) | **Confirmed** | `runtime.rs:414` `.unwrap()` in the `extern "C"` trampoline error path — panic unwinds across C (UB) when an exception string contains `\0`. |
| RUST-002 | High (blocker) | **Confirmed** | `to_value_inner` array branch (runtime.rs:641-663) recurses before the `visited` cycle check (only non-array objects checked, :665-668) → `a.push(a)` stack overflow / abort. |
| RUST-003 | Medium | **Confirmed** | `len as usize` from a spoofable `length` property (runtime.rs:652) → up to `0..2^32` property reads via Proxy trap → CPU DoS. |
| SUPPLY-001 | High (blocker) | **Confirmed** | `libquickjs-sys = "0.1"` (Cargo.toml:13) → QuickJS **2019-07-21** (audit artifact 16-quickjs-VERSION.txt; copyright `2017-2019`). Upstream (Bellard) + QuickJS-ng resumed maintenance; this snapshot has 7 years of unpatched C engine. Builds via `make libquickjs.a` subprocess (build.rs) → SUPPLY-005. |
| SUPPLY-010 | Info | **Confirmed** | Dead workspace dep `quick-js = "0.1"` (root Cargo.toml:51) — remove or wire when switching bindings. |

Additionally owned from report 12: **transpiler/TS corruption** (PLUGIN-006) and **reference-plugin compatibility** (PLUGIN-008, R1 failure), including the requirement that reference fixtures load *unmodified*.

---

## 2. Files to change (oc-plugin only, unless noted)

| File | Change |
|---|---|
| `Cargo.toml` | Replace `libquickjs-sys` with maintained binding (see §3). Add runtime crate deps: `crossbeam-channel` or `tokio` time for timer wheel; `rustc-demangle`/`backtrace` not needed. Drop nothing else (reqwest blocking stays until npm async work, owned by install domain). |
| `src/js/runtime.rs` | **Rewrite on the safe binding.** Delete the hand-rolled unsafe FFI (`free_value`, `build_closure_trampoline`, `Owned`, `OwnedObject`, `serialize_value`, `to_value`). Keep the public API (`Runtime::new/eval/eval_json/global/set_global_json/call_function/call_json/add_callback/install_bridge/pump_jobs`) so `host.rs`/`bridge.rs` change minimally. Add safety primitives (§4): limits in `new()`, deadline interrupt, bounded pump, cycle/size/depth caps in value conversion. |
| `src/js/mod.rs` | Re-export from new runtime; delete stale `libquickjs-sys` references in doc comments. |
| `src/js/value.rs` | Keep `JsValue`/`JsError`; add `Error::Timeout`, `Error::MemoryLimit`, `Error::DepthLimit` variants and size-capped conversion helpers. |
| `src/js/transpile.rs` | Fix corruption classes (§6.2) or swap to a real TS stripper (decision in §6.2). |
| `src/loader.rs` | `ModuleResolver::resolve_path` (+`candidates`, `load`, `preload_imports`) gains containment (§5): base-root check, npm `exports` containment (port `resolvePackageFile`), symlink resolution, size cap on read source. |
| `src/bridge.rs` | `resolve` returns containment errors; `read` method (used by `__oc_eval_module_path`) must also be contained; add `timer` bridge method group. |
| `src/host.rs` | `LoadedPlugin::event`/`trigger`/`dispose` route through the new pump with deadline + microtask flushing (§4.3); add `TimerBridge` interface + `Watchdog` handle. |
| `src/polyfill/runtime.js` | Real timer implementation (§4.3); `__oc_require` containment-aligned error paths; keep the module registry and API surface; fix `__oc_event` to be driven by host flush; add optional `@opencode-ai/plugin` real-surface loading (§6.3). |
| `src/lib.rs` | `OPENCODE_VERSION` from workspace metadata (PLUGIN-013, owned implicitly); doc updates. |
| `src/npm.rs` | (owner: install domain) — unchanged here except: module resolution of installed packages must stay inside the plugin's `node_modules` root (containment root set at resolve time). |
| `tests/integration.rs`, `tests/fixtures/*` | Add safety/containment/compat tests (§7); fixtures remain, but the *reference-loads-unmodified* test uses `reference/packages/plugin/src/*.ts` directly (read-only path). |
| `Cargo.toml` (root) | Remove dead `quick-js` workspace dep (SUPPLY-010). Commit `Cargo.lock` is Agent 19's SUPPLY-002 — I only touch dep lines. |

---

## 3. Engine decision: **rquickjs 0.12.2** (safe binding over maintained QuickJS)

### Recommendation
Adopt **`rquickjs = { version = "0.12", features = ["loader", "futures"] }`** as the engine. Verified against live crates.io + downloaded crate source (2026-08-05):

- **Maintained.** rquickjs 0.12.2 published **2026-07-27** (9 days old); 41 versions since 2020; `rquickjs-core`/`rquickjs-sys` track it. The bundled QuickJS carries **`Copyright (c) 2018-2026 Fabrice Bellard`** — the engine inside is actively maintained, not the 2019-07-21 snapshot (SUPPLY-001 fixed). rust-version 1.87 ≤ our 1.97.
- **Safety primitives exist (verified in source):**
  - `Runtime::set_memory_limit(usize)` → `JS_SetMemoryLimit` (rquickjs-core/src/runtime/raw.rs:246; base.rs:123).
  - `Runtime::set_max_stack_size(usize)` → `JS_SetMaxStackSize` (raw.rs:256; base.rs:132).
  - `Runtime::set_interrupt_handler(Option<InterruptHandler>)` → `JS_SetInterruptHandler` (raw.rs:390-422; base.rs:89). Handler is a `FnMut() -> bool`; returning `false` makes QuickJS throw a catchable JS exception. This is the watchdog/interrupt primitive for PLUGIN-001.
  - `Context::execute_pending_job` + `Runtime::is_job_pending` (ctx.rs:404; raw.rs:205-209) — drop-in for the current `pump_jobs`.
  - `CatchResultExt` (result.rs:616) — converts Rust `Result`/**panics** into JS exceptions at the binding boundary. The `extern "C"` trampoline is owned by the binding and is panic-safe, which **eliminates RUST-001 at the source** rather than patching it.
- **API fit.** It is the high-level successor to the same QuickJS engine, so the current `__oc_require`/`__oc_define`/polyfill JS layer (runtime.js) carries over ~unchanged; only the Rust wrapper (runtime.rs) is rewritten. The `Loader` trait (loader.rs:96) gives us a real ESM module loader option (§6.3). `Runtime`/`Context` remain `!Send + !Sync` — matches the existing single-thread confinement design.
- **Build implications.** rquickjs-sys compiles the bundled QuickJS via the **`cc` crate** (`build.rs:153`), not a `make` subprocess — this **removes the `make`/`ar` subprocess dependency (SUPPLY-005)**. It still requires a C compiler, which the workspace already needs (bundled SQLite, `ring`). No network at build on Linux (the WASI-SDK auto-download is `target_os=="wasi"`-only). Binary footprint: +~1.5–2 MB static lib vs the 2019 snapshot; acceptable vs CONTEXT.md goal (still ≪ Bun/JSC).

### Alternatives evaluated (rejected)
| Option | Rationale for rejection |
|---|---|
| `quickjs-ng` via `quickjs-rusty`/`libquickjs-ng-sys 0.12` | Actively maintained fork; valid. Rejected for **binding maturity**: `libquickjs-ng-sys`/`quickjs-rusty` are younger and less battle-tested than rquickjs, and would still require the same runtime.rs rewrite with fewer safety helpers. Revisit if rquickjs stops tracking upstream. |
| `boa_engine 0.21.1` | Pure Rust, zero C toolchain. Rejected: from-scratch engine with known gaps (async/iterator/Intl edge semantics), lower perf, and a **larger behavioral divergence** from the QuickJS-shaped polyfill; memory-limit/interrupt APIs are newer and less proven. Would force a rewrite of runtime.js semantics, not just runtime.rs. |
| `deno_core` (rusty_v8) | Rejected on footprint/hermeticity: pulls a prebuilt V8 (~100 MB) or a massive V8 build; contradicts the "minimal single binary, in-process" contract; `import()`-compat gain doesn't justify the dependency mass. |
| Keep `libquickjs-sys 0.1` + patch | Rejected: the crate is unmaintained (SUPPLY-001); we'd be hand-maintaining FFI for a 7-year-old engine and still hand-writing the trampoline where RUST-001 lives. |

### How much of `js/runtime.rs` must change
~100%. The public method surface is preserved; every interned FFI call (`JS_NewRuntime`, `JS_Eval`, `JS_Call`, `JS_GetPropertyStr`, `JS_NewCFunctionData`, ...) is replaced by rquickjs's safe `Context`/`Ctx`/`Value`/`Function`/`Module` APIs. `host.rs` and `bridge.rs` change only where new timeout/containment/timer plumbing is threaded in (§4, §5). `value.rs` stays.

---

## 4. Safety-primitive design

### 4.1 Runtime limits (PLUGIN-001)
Set once in `Runtime::new()` on the rquickjs `Runtime` (not per-context):
- `set_memory_limit(limit)` — configurable, default **64 MiB** per plugin context (reference has none, but in-process needs a bound; 64 MiB covers real plugin/effect-type workloads; make it an env knob `OPENCODE_PLUGIN_MEM_LIMIT` for parity overrides).
- `set_max_stack_size(1 MiB)` — QuickJS's own JS-stack bound (default 256 KiB is too tight for zod-schema recursion; 1 MiB is safe; `raw.rs` clamps >16 MiB to 0).
- **Interrupt handler + deadline watchdog:** install `AtomicU64` deadline (µs) + `AtomicU64` instruction counter. Handler: if counter > step budget (e.g. 100k ops per check) or `now ≥ deadline`, return `false` → QuickJS throws `InternalError`/`Error` → surfaced as `JsError::Timeout`. Every host entrypoint (`trigger`, `config`, `event`, `execute_tool`, `load`, `dispose`) arms a deadline derived from the `Watchdog` config (default **5 s** per JS call, 30 s per plugin load); disarm on return.
- **Wall-clock watchdog on the Rust side:** every pump loop runs with an outer `Instant` deadline; on expiry the context is torn down (drop `Runtime`) and the plugin is marked failed, so even a C-level hang (e.g. pathological GC) cannot wedge the host thread.

### 4.2 Value-boundary hardening (RUST-002, RUST-003)
Rebuild `to_value`/`serialize_value` on rquickjs's safe `Value` in `value.rs`/new runtime:
- **Cycle detection:** one `HashSet<identity>` consulted for *both* arrays and objects, inserted before descending and removed after (or kept for the whole conversion when read-only). `const a=[];a.push(a)` → cycle cut as `null` (matches current object behavior). Regression: `self_referential_array_converts_without_overflow`.
- **Size caps:** array `length` read via the safe property API; reject `len < 0`, reject non-integer, cap **64 k elements / 1 M nodes total / 64 depth** for a single boundary conversion; over-limit → `JsError::DepthLimit`/`Internal` (a plugin returning a 2^32-length Proxy gets an error, not a loop).
- **NUL/lone-surrogate strings:** rquickjs converts JS strings losslessly to Rust `String`; replace the `CString`/`CStr` path (`make_cstring`, runtime.rs:41-43) so `\0` never reaches a C boundary — fixes RUST-001's *input* half (its *output* half is fixed by the binding's panic-safe trampoline).

### 4.3 Event-loop / microtask pumping + real timers (PLUGIN-003, PLUGIN-004, RUST-008)
Replace `pump_jobs`'s bare `while JS_IsJobPending` with a **bounded, deadline-aware driver**:
1. `drive_until_idle(deadline)`: loop `execute_pending_job()` with (a) the §4.1 deadline and (b) a **max-iterations / total-time cap**; if jobs never drain (RUST-008), return `JsError::Timeout` instead of hanging.
2. **Host-queued microtask flush:** `LoadedPlugin::event` (and any future host→JS push) calls `call_function("__oc_event", ...)` *then* `drive_until_idle` — the `Promise.resolve().then(hook)` microtask now runs and its result/exception is captured. Add an observable assertion to `triggers_hooks` (event hook sets a flag the test reads back).
3. **Timer bridge:** polyfill `setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` register into a Rust-side **timer wheel** (priority queue keyed by deadline) instead of `Promise.resolve().then`. `drive_until_idle` first fires due timers via `call_function` on a small helper (`__oc_fire_timer(id)`), then pumps microtasks, then recomputes the next deadline — yielding genuine wall-clock semantics for retry/backoff/polling plugins without a native async runtime. `setImmediate`/`queueMicrotask` map to the next microtask flush.
4. Timer IDs are validated against the wheel (clearTimeout of an unknown id is a no-op); the wheel is per-`LoadedPlugin` and dropped on context teardown.

### 4.4 FFI unwind containment (RUST-001)
Falls out of §3/§4.2: no hand-written `extern "C"` trampoline remains in our code; rquickjs catches Rust panics (including in registered closures) via `CatchResultExt` and converts them to JS exceptions. Belt-and-braces: wrap every Rust callback body in `catch_unwind` (keep the existing `exec_callback` shape, runtime.rs:487-503) so a panic in host-side `PluginHost` code surfaces as `JsError` instead of unwinding. **No `.unwrap()` on poisonable locks or on `make_cstring` in any bridge path** (replace `runtime.rs:414`, `host.rs:188`, `host.rs:197` with fallible paths).

---

## 5. Containment design (PLUGIN-002, PLUGIN-012)

Containment is enforced in the **Rust resolver** (single choke point — the bridge's `resolve` and `read` methods), not in JS:

1. **Approved roots per plugin:** `{ plugin dir }` and `{ plugin dir }/node_modules` (recursive), plus (for npm plugins) the installed package dir. For file plugins the root is the *declared file's* directory. Absolute specs are only allowed if they canonicalize inside an approved root.
2. **Canonicalization:** `fs::canonicalize` the resolved path (and its parents) **before** reading; reject any path that escapes via `..`, symlink, or bind-mount outside the root. This matches the reference `Filesystem.contains` behavior (`resolvePackageFile`, shared.ts:89-97) and closes the lexical-`..` and symlink classes.
3. **`file://` and npm `exports`:** port `resolvePackageFile` exactly — an npm `exports`/`main` that resolves outside the package dir throws `Plugin <spec> resolved <kind> entry outside plugin directory` (PLUGIN-012 parity).
4. **Reject policy:** any spec that would escape returns a bridge error (`Err`) → JS throws `Error("Cannot resolve module ... outside plugin directory")`. **No fallback to eval.** The `__oc_require` path (runtime.js:112-117) gets an explicit guard that trusts only bridge results whose `kind` is `inline` from a contained resolve.
5. **Host functions stay capability-scoped** (PLUGIN-005 posture): default `PluginHost` methods (`fs`/`fetch`/`shell_exec`/`client`) continue to error unless the integrating application implements them — containment fixes the one current side channel (R4). Document that `fs`/`fetch`/`shell` remain **deny-by-default** until Agent 02's host wires a scoped client.
6. **Preload/static-import scan** (`preload_imports`, loader.rs:443-462) runs the same canonicalization — it currently pre-reads any path `static_import_specs` names, another escape vector.

Tests: `path_escape_import_rejected` (absolute import outside root → error), `symlink_escape_rejected`, `npm_entry_outside_dir_rejected`, `sibling_import_allowed` (within-root relative import still works), `bare_spec_node_modules_only`.

---

## 6. Compat design — reference plugins load unmodified (PLUGIN-006, PLUGIN-008)

### 6.1 Goal
Load the **actual** `reference/packages/plugin/src/example.ts` and `example-workspace.ts` (and the real `@opencode-ai/plugin` surface) without adapted shims. R1 currently fails at `./index.js` (missing) + `zod`/`effect`/`@opencode-ai/sdk` imports.

### 6.2 Transpiler decision (PLUGIN-006)
Primary: **replace the hand-written `transpile.rs` with a maintained Rust TS-transpiler.** Candidate: **`oxc`** (`oxc_parser` + `oxc_transformer` TypeScript strip / module transform) — pure Rust, actively maintained, fast, and already used widely. This fixes the whole corruption class (postfix `!`, `catch(e:any)`, generic arrows, overloads, decorators, `enum`/`namespace` handling) with a real parser instead of patching lexer heuristics one-by-one. Evaluate build-size/compile-time impact first (adds a few deps); if it blows the "lean crate" budget, **fallback**: fix the specific corruption classes in `transpile.rs` (postfix-`!` context, `catch` annotation erase, generic-arrow/overload handling) with targeted tests, and keep the heuristic approach documented as a known subset.

Either way: `import type` / type-only erasure stays; the `__oc_require`/`__oc_define` ESM transform stays (it mirrors what the loader expects), but the transform must be **spec-complete for the constructs the reference examples use**.

### 6.3 API surface + module resolution (PLUGIN-008)
- **`@opencode-ai/plugin` real surface:** keep the polyfilled module registry for `opencode/plugin`, `opencode/plugin/tool`, etc., but make `@opencode-ai/plugin` (and subpaths `tool`, `v2/*`) resolve to a **complete** surface: `ToolContext`, `ToolResult`, `Hooks` (all 20+ hook names incl. `auth`, `provider`), `PluginInput` (`client`, `project`, `directory`, `worktree`, `experimental_workspace`, `serverUrl`, `$`), `Config`, types. `tool()` returns input; `tool.schema = z` stays. The type-only imports are erased by the transpiler, so runtime surface completeness is what matters.
- **`zod`:** polyfill stays but must cover the zod surface the real examples use (`z.object().shape`, `describe`, `optional`, `array`, `enum`, `union`, `literal`, refinements no-ops). Keep as an in-process shim (installing real zod needs its dist + dependency tree — PLUGIN-007, install domain).
- **`effect` / `@opencode-ai/sdk`:** cannot load in-process without a Node/Bun toolchain and are out of scope for the *v1* fixture path (they are types-only for the reference examples — `import type` erases). For plugins that genuinely import `effect` at runtime, the v2 surface must reject loudly (existing stub behavior documented), not corrupt.
- **Module resolution for `./index.js` → `.ts`:** the resolver's extension fallback (loader.rs:82-104) already tries `.ts` after `.js`; verify the real example's `./index.js` resolves to `src/index.ts` (Bun does this natively). Add the reverse mapping (spec `.js`, file `.ts`) explicitly if needed.

### 6.4 Compat test matrix
1. `reference_loads_unmodified`: transpile+load `reference/packages/plugin/src/example.ts` **as-is** (no shim edits), assert `tool` hook registered and `mytool` executes `Hello world!`.
2. Same for `example-workspace.ts` (workspace adapter register + configure/target).
3. `reference_index_surface`: `import { tool, ... } from "opencode/plugin"` and `@opencode-ai/plugin` paths resolve; `ToolContext`-shaped `ask`/`metadata` bridge works.
4. Transpiler corpus: postfix `!`, `catch(e:any)`, `enum`, generics, overloads, decorators, `satisfies`, `import type` — golden round-trips.
5. Zod golden: `tool.schema.string().describe("foo")` → expected JSON schema (byte-exact vs fixture).
6. Differential vs reference oracle (Agent 18 fixture capture): tool schema JSON for a representative plugin must match the reference binary's output.

---

## 7. Production-wiring gate (PLUGIN-004)

Plugins **must not** reach production until the safety+containment gates pass. Define the gate as a single feature flag + checklist:

- **Gate condition (all must hold):**
  1. `cargo test -p oc-plugin` green with new safety/containment/compat tests (incl. §7 tests below).
  2. `cargo clippy -p oc-plugin -- -D warnings` clean (oc-plugin currently fails clippy; RUST-015).
  3. `cargo build -p oc-plugin` green; workspace `cargo build` green.
  4. A fuzz/probe run: hostile plugin corpus (infinite loop, memory blowup, self-referential array, Proxy length, path escape, NUL exception) all terminate with `JsError`, no process crash, no hang.
- **Wiring contract with Agent 02:** Agent 15 delivers `LoadedPlugin` + a `PluginManager` facade (`load_all_from_origins`, per-plugin `LoadedPlugin`, hook dispatch with timeouts). Agent 02's composition root calls it in the server/CLI startup path **only after** the gate passes; until then the production `opencode plugin` stub stays stubbed (safe default). No production `PluginHost` impl with `fs`/`shell`/`fetch`/`client` grants is enabled until (a) gate passes and (b) Agent 08's permission/ask path exists to gate `tool.ask`/`permission.ask` from plugins (SEC-001 is the owning blocker).

---

## 8. Dependencies on agents 02 / 08 / 18

- **Agent 02 (composition root / architecture):** consumes my `PluginManager` facade; provides `PluginHost` impl backed by the real client/server (currently `NoopHost` only); owns the startup ordering (config plugin_origins → plugin load → server). I provide the facade + gate; Agent 02 wires it and cannot enable it until my gate passes (§7). I need Agent 02's decision on the `PluginHost.client`/`fs` scoping (which methods get real impls vs. remain deny-by-default).
- **Agent 08 (permission / wire):** plugin `tool.ask` + `permission.ask` bridges must call the real allow/ask/deny service. I keep the bridge call-shape stable (`tool.ask` → `host.tool_ask`) so Agent 08 can implement the backend; do not merge production `tool_ask` auto-allow behavior (host.rs:67-69 default) into a wired host until SEC-001 lands.
- **Agent 18 (testing/reference capture):** differential goldens for the compat matrix (§6.4.6) and hostile-plugin fixtures captured from the reference oracle; coordinate on the fixture-provenance cleanup (TEST-003).
- **Agent 19 (supply chain):** SUPPLY-002 (`Cargo.lock` commit) and SUPPLY-003 (LICENSE) are preconditions for shipping the new engine dep; SUPPLY-004 (release profile) is a hardening follow-on. Coordinate version-pin review for rquickjs.

---

## 9. Risks

| Risk | Mitigation |
|---|---|
| rquickjs API churn vs the 2019-engine internals (module registry, job pumping, `globalThis` polyfill no longer needed) | rquickjs bundles *newer* QuickJS with `globalThis`/Promise/`Object.keys` natives — the polyfill's `globalThis` shim (runtime.rs:237-240) and some shims become no-ops but stay harmless. Keep the JS layer's contract (bridge methods) stable; feature-gate removals. |
| `effect`/`@opencode-ai/sdk` runtime imports still impossible in-process | Out of scope for v1 parity (types-only in reference examples); document loudly; do not regress the promise-based v2 surface. |
| oxc dependency weight vs "lean crate" rule | Measure; fallback to targeted `transpile.rs` fixes with regression tests. |
| Timer wheel + deadline pump changes observable timing (tests that relied on "fires immediately") | Update affected tests to assert *real* timing (R6-style test asserts `≥ delay`); keep delay-0 timers immediate. |
| Memory-limit false positives on real plugins (64 MiB too small) | Make limit configurable (env knob + `PluginManager` option); tune from compat corpus before gate. |
| Engine upgrade changes JS semantics (string encoding, `Date`, regex) vs the 2019 engine | Run the existing 52-test suite + compat matrix against the new engine as the first migration step; diff any behavioral deltas into known-limitations docs. |
| `Runtime`/`Context` `!Send+!Sync` restricts wiring (Agent 02 wants async) | Keep single-thread-per-plugin confinement (matches reference event-loop model); expose a per-plugin thread/task pinning contract in the facade. |

---

## 10. Merge-order recommendation (Wave 1 foundation)

The plugin engine is **foundational infrastructure** — everything plugin-facing depends on it, and no production exposure may precede it. Recommended order:

1. **Wave 1 (this agent, oc-plugin only):**
   - **1a. Engine swap** (`runtime.rs` → rquickjs, value conversion rewrite, cycle/size/depth caps). Ship with the existing test suite green + new RUST-001/002/003 regressions. *Unblocks every later item; no behavior change yet.*
   - **1b. Safety primitives** (memory/stack limits, interrupt + deadline watchdog, bounded pump, real timer wheel, event microtask flush). PLUGIN-001/003/004 + RUST-008 closed.
   - **1c. Containment** (resolver canonicalization, approved roots, npm-entry parity). PLUGIN-002/012 closed.
   - **1d. Compat** (transpiler fix + real `@opencode-ai/plugin` surface + reference fixtures unmodified). PLUGIN-006/008 closed. Gate §7 evaluable.
2. **Wave 2+ (with Agent 02/08/18):** production wiring behind the gate (§7) — `PluginManager` facade adopted by Agent 02's composition root; `tool.ask`/`permission.ask` backed by Agent 08's real permission service; differential goldens from Agent 18. `opencode plugin` un-stubbed only here.
3. **Wave 3:** install-domain hardening (npm dependency trees, real semver — PLUGIN-007) and remaining v2/effect surface.

**Rationale:** 1a–1d are self-contained in `oc-plugin` (no cross-crate merge risk), reduce the crate from "latently dangerous" to "safe + isolated + compatible," and are the *precondition* the audit's NOT_READY verdict names for any plugin exposure. Merge 1a and 1b as separate commits (engine swap is mechanically large; limits are small and testable). Do **not** co-merge production wiring with the engine swap — the gate (§7) is the hard boundary.

---

*Evidence: static inspection of `crates/oc-plugin/src/js/runtime.rs`, `loader.rs`, `host.rs`, `bridge.rs`, `npm.rs`, `polyfill/runtime.js`, `Cargo.toml`; reference `packages/opencode/src/plugin/{index,loader,shared}.ts`, `packages/core/src/plugin/host.ts`, `packages/plugin/src/{index,tool,example}.ts`, `package.json`; live crates.io metadata + downloaded `rquickjs`/`rquickjs-core`/`rquickjs-sys` 0.12.2 sources (APIs and build path verified). No production source modified.*
