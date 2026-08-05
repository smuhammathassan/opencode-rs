# Agent 12 — Plugin System and QuickJS Runtime

**Auditor:** Agent 12 of 20 · **Date:** 2026-08-05 · **Status:** READY_WITH_MINOR_REMEDIATION (test-only runtime; see verdict)

## Scope

Audit of the plugin system in `crates/oc-plugin`: discovery, resolution, npm/git install, load order, lifecycle,
hook registration/invocation, error isolation, async behavior, the QuickJS runtime (create/destroy, limits,
interrupts, GC, leaks), module loading, host functions, capability restrictions, serialization across the host
boundary, reentrancy/deadlocks, panics crossing FFI, and malicious-plugin behavior. Assessed against the
reference (`reference/packages/opencode/src/plugin/*`, `reference/packages/core/src/plugin/*`,
`reference/packages/plugin/*`).

The task also asked to verify the central claim: *"plugins run in-process on a vendored QuickJS (libquickjs-sys),
no Bun/Node subprocess"* — and to assess isolation/memory/security vs the reference.

## Repository areas inspected

- `crates/oc-plugin/` — all source: `lib.rs`, `js/runtime.rs`, `js/value.rs`, `js/transpile.rs`, `js/mod.rs`,
  `host.rs`, `bridge.rs`, `loader.rs`, `shared.rs`, `install.rs`, `meta.rs`, `npm.rs`, `config.rs`, `paths.rs`,
  `jsonc.rs`, `polyfill/mod.rs`, `polyfill/runtime.js`; `tests/integration.rs` + `tests/fixtures/*`; `examples/tr.rs`.
- `reference/packages/opencode/src/plugin/{index,loader,shared,meta,install}.ts` and `reference/packages/opencode/src/cli/cmd/plug.ts`
- `reference/packages/core/src/plugin/host.ts`, `reference/packages/core/src/npm.ts`
- `reference/packages/plugin/src/{index.ts,tool.ts,example.ts,example-workspace.ts,package.json}`
- `crates/oc-cli/src/cli/cmd/plug.rs`, `crates/oc-server/src/pty_environment.rs`, `crates/oc-tool/src/tool/registry.rs`,
  `crates/oc-config/src/load.rs`, Cargo dependency graph, `docs/superpowers/specs/2026-08-05-opencode-rs-design.md`

## Commands executed

- `cargo test -p oc-plugin` (all 52 tests pass: 46 unit + 6 integration; no failures)
- `cargo run -p oc-plugin --example <audit harness>` — six temporary harnesses written to `/tmp/opencode/agent12/`
  and `crates/oc-plugin/examples/*.rs` (deleted afterward), built on the crate's own public API (the only
  reachable plugin path).
- `./target/debug/oc-cli plugin <module>` and `plugin --help` → both return `opencode-rs: not yet implemented`
  (plug command is a `not_wired` stub).

## Runtime scenarios attempted (all STATIC + RUNTIME proof)

| # | Scenario | Result | Proof |
|---|----------|--------|-------|
| R1 | Load real `reference/packages/plugin/src/example.ts` | **FAILS** — `LOAD_ERR Error: failed to read .../src/./index.js` (no `index.js` in reference pkg; fixtures are adapted shims, not reference sources) | runtime |
| R2 | Load fixture `example.ts` (tool) | `LOAD_OK` + `TOOL_OK result="Hello world!"` (also `executes_reference_example_tool`) | runtime |
| R3 | Sandbox probe: `node:fs/promises` read/write/mkdir/rm, `node:os`, `node:process.env`, `fetch`, `node:path` | fs: "not implemented by the host"; `fetch`: "not implemented by the host"; `process.env`: error "value has no property" (env is empty); `os.homedir` → `""`; `path.resolve` works (pure JS) | runtime |
| R4 | Arbitrary file execution via absolute-path import: plugin imports `/tmp/.../evil.js` outside its base dir | **EXECUTED** — `EVIL_RESULT "pwned-no-node"` | runtime |
| R5 | `for(;;){}` in plugin | **HANG** — process never returns (killed at 15 s); no interrupt handler | runtime |
| R6 | `setTimeout(cb, 5000)` | **Fires immediately** — `TIMER_ELAPSED_MS 0`, total load 1.16 ms (delay ignored) | runtime |
| R7 | Hook throws `Error("boom")` | Error isolated, surfaced to Rust as `JsError` (test `triggers_hooks` pattern; runtime exception→string conversion in `js/runtime.rs` `exception()`) | static+runtime |
| R8 | Transpiler probes (`!`, catch annotations, enums, generics, decorators, etc.) | Postfix `!` → `config.directory` (silently corrupts); `catch(e:any)` → passed through → `SyntaxError: expecting ')'`; enums/namespaces silently dropped; `<T>` and `overloads` corrupt output | static+runtime |
| R9 | `node:process.env` / secrets | Empty env, no bridge — plugins cannot read env vars or secrets | static+runtime |

## Architecture or behavior summary

- **Central claim (CONFIRMED, mostly):** plugins run in-process on vendored QuickJS. `crates/oc-plugin/Cargo.toml`
  depends on `libquickjs-sys = "0.1"`; `src/js/runtime.rs` wraps `JS_NewRuntime`/`JS_NewContext`/`JS_Eval`/`JS_Call`
  directly. `Runtime::new()` (runtime.rs:218) creates rt+ctx; `Drop` (runtime.rs:445) calls `JS_FreeContext` +
  `JS_FreeRuntime`. **No Bun/Node subprocess.**
- **But the premise "reference spawns a Bun subprocess per plugin" is WRONG.** The reference loads plugins
  in-process via Bun's native `import(row.entry)` (`loader.ts:139`) inside the opencode process (`index.ts` `Plugin.service`).
  The real divergence is the *engine* (Bun/JSC vs QuickJS) and the plugin API surface, not subprocess isolation.
  The vendored spec `CONTEXT.md`/design doc calls the reference a "Bun sidecar", which the reference source does
  not support.
- **Loading model:** config `plugin` entries → `load_external_reported` (loader.rs:398) → per-spec
  `resolve_and_load` → transpile entry → `register_with_resolver`; then `PluginBuilder.build()` creates one
  QuickJS context + `polyfill/runtime.js` and `LoadedPlugin.load()` evals the transpiled entry. All cross-boundary
  data is JSON strings over a single synchronous bridge (`__oc_host_bridge(method, payload)`, bridge.rs:15).
- **Dedup/order:** `deduplicate_plugin_origins` keeps the last origin (matches reference); load order = config
  order, sequential (reference parallelizes with `Promise.all` in loader.ts:214).
- **Isolation model (vs reference):** the reference gives plugins the full in-process Bun/Node environment
  (real `fs`, `process.env`, `Bun.$` shell, real network, no memory/CPU limits) — i.e. **the reference is
  deliberately NOT sandboxed and has NO limits either**. The port's default `PluginHost`/`NoopHost` *blocks* most
  of that (fs/fetch/shell/client all error by default), which is a genuine isolation improvement — but it is
  undermined by the resolver (R4) and by having no runtime limits at all (R5).

## Positive observations

- JS exceptions are captured and surfaced as `JsError::Exception` via `JS_GetException`/`JS_ToString` (runtime.rs:460);
  a throwing hook does not crash the host.
- Rust panics inside host callbacks are contained: `exec_callback` wraps the callback body in
  `catch_unwind` (runtime.rs:487) and converts panics to JS exceptions — panics do not cross the C FFI.
- `Runtime::Drop` frees context and runtime (runtime.rs:445-453); callback trampolines hold a stable pointer into
  an `Arc<Mutex<CallbackRegistry>>` so they survive runtime moves (test `callbacks_survive_runtime_move`).
- `to_value` detects cycles via a visited-set (runtime.rs:665-668) and cuts them as `null` rather than recursing forever.
- Polyfill does not install `eval`/`Function` access to the host environment beyond the bridge; `process.env` is
  empty, `process.argv` empty, `$` shell and `node:fs` blocked by default — secret material is not readable by plugins.
- `exec_callback` runs the callback under `catch_unwind` — no panics cross FFI (see above).
- npm fetch + tarball unpack + git+ clone paths exist with passing tests (`npm.rs`).
- Reference `meta.ts` `Entry` shape is replicated exactly (`meta.rs`); `plugin-meta.json` store implemented.

## Findings summary

| ID | Severity | Confidence | Title |
|----|----------|-----------|-------|
| PLUGIN-001 | High | CONFIRMED | No memory limit, no interrupt handler, no watchdog → plugin can hang/starve the whole process |
| PLUGIN-002 | High | CONFIRMED | Resolver reads & executes arbitrary local JS via absolute-path `import` (no containment; R4) |
| PLUGIN-003 | Medium | CONFIRMED | `event` hooks are silently dropped (microtask never pumped) |
| PLUGIN-004 | Medium | CONFIRMED | `setTimeout`/`setInterval` ignore delays; timers + real-timer I/O break timing semantics |
| PLUGIN-005 | Medium | CONFIRMED | `fetch` polyfill does NOT do real HTTP (host default errors); `node:fs` blocked by default |
| PLUGIN-006 | Medium | CONFIRMED | Transpiler corrupts valid TS/JS (postfix `!`, catch annotations, generic arrows, overloads, decorators); silently strips enums/namespaces |
| PLUGIN-007 | Medium | CONFIRMED | npm install doesn't resolve dependency trees; naive `^`/`~` range pick can install a wrong version |
| PLUGIN-008 | Medium | CONFIRMED | Real reference plugin sources don't load (missing `./index.js`); fixtures are adapted shims; `@opencode-ai/plugin` real API not loadable |
| PLUGIN-009 | Medium | HIGH | No production caller: `opencode plugin` is a stub, oc-server/oc-session/oc-tool only `TODO(integration)` — plugin runtime is TEST-ONLY today |
| PLUGIN-010 | Low | CONFIRMED | v2/effect API is a passthrough stub; `auth`/`provider` hooks and legacy v1 surface are no-ops |
| PLUGIN-011 | Low | CONFIRMED | `process.env` empty and `node:process.argv` empty (blocks credential-based plugins) |
| PLUGIN-012 | Low | HIGH | npm entry containment check from reference (`resolvePackageFile`) not ported |
| PLUGIN-013 | Low | CONFIRMED | `OPENCODE_VERSION` hardcoded "1.18.13" (lib.rs:57) rather than workspace metadata |
| PLUGIN-014 | Informational | HIGH | "reference spawns Bun subprocess" claim is false; reference loads plugins in-process via `import()` |
| PLUGIN-015 | Informational | CONFIRMED | `set_global`/object-readback path relies on `Object.keys` (enumerable own props only); non-enumerable/inherited props don't round-trip |
| PLUGIN-016 | Informational | HIGH | `Runtime` not `Sync`/`Send`; `Mutex` poisoning on `callbacks.lock().unwrap()` (host.rs:188, runtime.rs:427) and `async_call` `serde_json::from_str(...).unwrap()` panic paths |

## Detailed findings

### PLUGIN-001 — No memory limit, no interrupt handler, no watchdog (High, CONFIRMED)

`Runtime::new` (js/runtime.rs:218-242) calls `JS_NewRuntime()`/`JS_NewContext()` and never calls
`JS_SetMemoryLimit` or `JS_SetInterruptHandler` — confirmed by grep across the crate (no matches). There is no
timeout or preemption anywhere in the host (host.rs, bridge.rs). RUNTIME: a plugin containing `for(;;){}` hangs
the process until external kill (R5, killed at 15 s). A hook awaiting an unresolvable promise makes
`pump_jobs` (runtime.rs:279-288) loop forever at 100% CPU, because `JS_ExecutePendingJob` returns without
progress and `JS_IsJobPending` stays true. A plugin that leaks memory is unbounded. Reference: Bun in-process
also has no QuickJS-level limit, but JS is JIT-compiled and event-loop based, so a hung plugin blocks the event
loop rather than spinning a busy-wait in the same way; neither has a hard wall-clock kill, so this is
environmental rather than parity-breaking, but for a single-binary process this is a DoS vector.

### PLUGIN-002 — Arbitrary local file read/execute via absolute-path import (High, CONFIRMED)

- `ModuleResolver::resolve_path` (loader.rs:67-78): an absolute spec returns the path unchanged; the base
  directory is NOT enforced (no containment, unlike reference `resolvePackageFile`, shared.ts:89-97).
- `resolve` bridge (bridge.rs:44-51) serves any resolved spec to the polyfill's `__oc_require` (runtime.js:112-117),
  which evaluates it via `new Function`. RUNTIME R4: a plugin importing `/tmp/opencode/agent12/malicious/evil.js`
  (outside its base dir) executed the file and set a global (`EVIL_RESULT "pwned-no-node"`).
- Impact: any plugin (or dependency of a plugin) can execute arbitrary local `.js` (and `.ts`, transpiled) files
  anywhere on disk and read JSON-parseable data through module exports — despite the design intent that in-process
  plugins are capability-restricted. Currently unreachable in production (PLUGIN-009) but reachable as soon as the
  plugin path is wired.

### PLUGIN-003 — `event` hooks silently dropped (Medium, CONFIRMED)

`__oc_event` (polyfill/runtime.js:769-779) schedules each hook as `Promise.resolve().then(...)` microtask.
`LoadedPlugin::event` (host.rs:124-129) uses `call_function` and never pumps the job queue, so the microtask
never runs unless a later hook happens to pump. STATIC proof: no `pump_jobs` call after `event()`; contrast with
`trigger`/`dispose` which use `async_call` → `pump_jobs`. Integration test `triggers_hooks` calls
`plugin.event(...)` but asserts nothing observable — the event hook body is never verified.

### PLUGIN-004 — Timers ignore wall-clock delays (Medium, CONFIRMED)

polyfill/runtime.js:202-213: `setTimeout`/`setInterval` schedule via `Promise.resolve().then(fn)`, delay ignored.
RUNTIME R6: `setTimeout(cb, 5000)` fired with `TIMER_ELAPSED_MS 0`, load completed in 1.16 ms. A plugin awaiting
a genuinely delayed timer will only progress during a later `pump_jobs` and will otherwise deadlock the host.
Documented in lib.rs:17-19 ("known limitation") and runtime.js comments, but real plugins relying on
retry/backoff/polling break.

### PLUGIN-005 — `fetch` is a stub, `node:fs` blocked by default (Medium, CONFIRMED)

`__oc_fetch` (runtime.js:219-230) bridges to `host.fetch`; the default `PluginHost::fetch` returns
`Err("fetch is not implemented by the host")` (host.rs:37-39). RUNTIME R3: `fetch` failed with that exact
message. So the polyfilled fetch does NOT do real HTTP unless a host provides it. `node:fs/promises` methods
similarly all fail with "fs.<m> is not implemented by the host" (host.rs:49-51; confirmed in R3). The only
non-blocked side channel is the resolver (PLUGIN-002). Reference: real `Bun.$` + real `fs` + real network.

### PLUGIN-006 — Transpiler corrupts valid TypeScript/JS (Medium, CONFIRMED)

`js/transpile.rs` is a hand-written lexer+heuristic stripper, not a real TS/ESM compiler. RUNTIME+RUNTIME R8:
- `catch (e: any)` is passed through → `SyntaxError: expecting ')'` at load (fixture-style plugins avoid it).
- Postfix `!` is dropped in exactly the position that matters (`config.directory!` → `config.directory`), so
  non-null-assertions silently change semantics.
- `<T>(x:T)=>x` generic arrows pass through as invalid JS.
- Overloaded function declarations corrupt (`function f(a);`).
- `enum`/`namespace`/`declare`/`interface` bodies are silently erased without error — silent behavior change.
- Decorators `@dec` pass through and `render_export` mangles `export default class`.
Only the constructs exercised by `tests/fixtures/*` and transpile unit tests are known-good; the "known
limitations" note in lib.rs:19-23 understates this. Real plugins (e.g. the reference sources) do not reliably
survive this transform (PLUGIN-008).

### PLUGIN-007 — npm install lacks dependency resolution and real semver ranges (Medium, CONFIRMED)

`npm::add` (npm.rs:104-154) fetches registry metadata and unpacks the single tarball; it does not install
transitive `node_modules` (the reference uses `@npmcli/arborist`, core/src/npm.ts). `pick_version` (npm.rs:52-83)
implements `^`/`~` by naive `version.starts_with(base)` prefix matching — can select the wrong version (e.g.
`^1` picks `1.10.0`; `~1.2` picks `1.20`). Plugins with dependency trees (nearly all real ones, e.g.
`@opencode-ai/plugin` deps) will not resolve their imports.

### PLUGIN-008 — Reference plugin sources don't load; fixtures are shims (Medium, CONFIRMED)

RUNTIME R1: loading the actual `reference/packages/plugin/src/example.ts` fails because it imports
`./index.js`/`./tool.js` which do not exist in that package (only `.ts`). `tests/fixtures/{index.js,tool.js}` are
manually written shims (see fixture comments) to make the *adapted* fixture pass. `reference/packages/plugin`
exports are source `.ts` files (`package.json` `exports` → `./src/*.ts`) that import `zod`, `effect`,
`@opencode-ai/sdk`; the polyfill provides only a small `zod` subset and no `effect`/`@opencode-ai/sdk`. Real
reference-compatible plugins are therefore not portable without heavy editing.

### PLUGIN-009 — Plugin runtime not reachable from the production executable (Medium, HIGH)

`cargo run --bin oc-cli plugin <m>` returns `opencode-rs: not yet implemented` (`crates/oc-cli/src/cli/cmd/plug.rs`
is a `not_wired` stub; `oc-cli/Cargo.toml` depends on `oc-plugin` but `plug.rs` only mentions it in a comment).
oc-server/oc-session/oc-session-runner/oc-tool depend on `oc-plugin` in `Cargo.toml` but no production source
calls `PluginBuilder`/`LoadedPlugin`/`PluginHost`/`load_external` (grep across all crates finds only
`oc-plugin` self-references and `TODO(integration)` comments). The `Plugin.service` equivalent
(`PluginBuilder.build()` + `load`) exists only in `crates/oc-plugin/tests` and examples. **Plugins are TEST-ONLY
today**; findings 001/002 are latent.

### PLUGIN-010 — v2/effect stub, `auth`/`provider` hooks absent (Low, CONFIRMED)

`opencode/plugin/v2/effect*` and `v2/promise` `define()` are passthroughs (runtime.js:886-907). `__oc_pick_server`
(runtime.js:644-662) never handles `auth`/`provider` hook objects; `__oc_hooks_summary` (runtime.js:710-726)
registers only functions + `tool`. A plugin exporting `auth` or `provider` hooks registers nothing.

### PLUGIN-011 — No environment/secret access (Low, CONFIRMED)

`node:process` (runtime.js:1039-1048) exposes `env: {}`, `argv: []`; `node:os.homedir()` → `""` (R3). Plugins
cannot read env vars or credentials (secure, but breaks real credential-based plugins and diverges from the
reference `process.env`).

### PLUGIN-012 — npm entry containment check not ported (Low, HIGH)

Reference `resolvePackageFile` (shared.ts:89-97) throws if an npm `exports` entry resolves outside the package
dir. Port `resolve_export_path` (shared.rs:84-91) does no containment. Combined with PLUGIN-002 this is the same
class of arbitrary-read; separate as an upstream-parity gap.

### PLUGIN-013 — Hardcoded version (Low, CONFIRMED)

`OPENCODE_VERSION` is `"1.18.13"` hardcoded (lib.rs:57) with a TODO; compatibility gate (shared.rs:233-262)
compares against it.

### PLUGIN-014 — "Bun subprocess" premise is false (Informational, HIGH)

Reference `loader.ts:139` uses `await import(row.entry)` in-process; no `spawn`/`child_process` anywhere in
`reference/packages/opencode/src/plugin/*`. The port is not *more* in-process than the reference; it replaces
the engine (Bun/JSC → QuickJS) and sandboxes by default (which the reference does not). Design docs
(`docs/superpowers/specs/2026-08-05-opencode-rs-design.md`) and CONTEXT.md propagate the incorrect
"sidecar/subprocess" framing.

### PLUGIN-015 — Object readback limited to enumerable own props (Informational, CONFIRMED)

`to_value_inner`/`object_keys` (runtime.rs:694-718) read via `Object.keys` — non-enumerable, inherited, getter,
and symbol-keyed properties are lost crossing the boundary (output mutation round-trips only JSON-able data).
Reference passes JS objects by reference in-process.

### PLUGIN-016 — Panic paths in host plumbing (Informational, HIGH)

`self.callbacks.lock().unwrap()` (runtime.rs:427) and `host.callbacks.lock().unwrap()` (same),
`async_call` `serde_json::from_str(&s).unwrap_or(...)` (host.rs:197) and `unwrap_or(Value::String(s))`;
`self.callbacks.lock().unwrap().push(pair)` will panic if a callback panics while holding the mutex (the closure
runs under `catch_unwind` at runtime.rs:487, so a panic inside the callback unwinds through the mutex guard and
poisons it). `LoadedPlugin`/`Runtime` are not `Send`/`Sync`; a caller can still move the raw-pointer-holding
`Runtime` between threads, and `Arc<dyn PluginHost>` is `Sync` — cross-thread use is UB-by-construction if the
embedder misuses it.

## Feature or behavior gaps

- No plugin runtime reachable from the executable (plug install; config `plugin` list never loaded by oc-server/oc-session).
- No memory/CPU/time limits or interrupt handlers (PLUGIN-001).
- Timers without delays; no event loop; `event` hook microtasks dropped (PLUGIN-003/004).
- `fetch` and `node:fs` not implemented by any host; no real `Bun.$` shell; no process spawning from plugins.
- No npm dependency tree install; naive semver ranges.
- v2/effect + auth/provider hooks stubbed; legacy v1 hooks reduced to a few names.
- Built-in auth plugins (codex, copilot, modal, …) not ported (documented).
- No containment for npm entrypoints or module resolution.
- No golden/serialization tests against a live reference for plugin-meta.json or hook payloads.

## Test coverage gaps

- All 52 tests run against `NoopHost`; no test exercises a host that implements `fs`/`fetch`/`client`/`shell`.
- `event` hook body is never asserted (test `triggers_hooks` calls it but checks nothing).
- No test for a plugin that hangs/loops, an unfulfilled promise, or a memory limit.
- No test loads the actual reference package sources (R1 fails); fixtures are adapted.
- No test for absolute-path imports escaping the plugin directory (PLUGIN-002) or npm entry containment (PLUGIN-012).
- No test for transpiler corruption (catch annotations, postfix `!`, generics, overloads).
- No multi-plugin concurrency/isolation test (reference loads plugins in parallel; port sequential).
- No test for `Runtime` drop/free leaks; no valgrind/sanitizer run.

## Unverified areas (BLOCKED / not provable here)

- **Reference "Bun subprocess" black-box comparison:** not run (reference executable is a compiled Bun binary;
  only source inspected — PLUGIN-014 is source-based). Marked HIGH confidence from source, not runtime.
- **Actual memory-leak behavior on repeated plugin load/dispose:** no memory profiling available; only static
  `Drop` inspection. UNVERIFIED.
- **GC behavior** (mark-and-sweep frequency) and cross-context leaks: not measurable statically. UNVERIFIED.
- **Whether a real-world npm plugin (e.g. opencode-* ecosystem) loads end-to-end:** requires network install +
  dependency tree the port does not install; not attempted (network-restricted environment). BLOCKED.
- **QuickJS engine version/features of `libquickjs-sys` 0.1** beyond observed behaviors. BLOCKED (no crate source
  inspection beyond FFI surface).

## Final domain verdict

**READY_WITH_MINOR_REMEDIATION** — *for the crate as a standalone tested component*. The plugin runtime works and
its 52 tests pass; error isolation, panic containment, context teardown, and the default-capability lockdown are
solid. However, **the plugin host is not reached by the production executable** (plug install and config-driven
plugin loading are unwired), so as a product feature it is TEST-ONLY, and there are **no memory or CPU limits**
(no `JS_SetMemoryLimit`, no `JS_SetInterruptHandler`, no watchdog) — a plugin can hang the process forever, and
an absolute-path `import` can read/execute arbitrary local `.js` files. Before the plugin path is wired into
oc-server/oc-cli these two High items (001, 002), the silent `event`-hook drop (003), timer semantics (004), and
the transpiler's corruption of real TS (006) should be remediated; otherwise the runtime should be considered
NOT_READY for production use.

---
*Evidence style: STATIC = code inspection with file:line; RUNTIME = executed harness (scenarios R1–R9 above). No production source modified; temporary example harnesses removed after use.*
