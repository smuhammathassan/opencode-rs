# Agent 17 — Performance, Memory, Startup Time, and Benchmark Validity

Auditor: Agent 17. Date: 2026-08-05. Read-only audit of `/root/opencode-rs` (Rust port, commit `e7fc33e`) vs the stock opencode v1.18.13 binary (`/root/.opencode/bin/opencode`, Bun-compiled).

## Scope

Independently reproduce and validate the published performance claims from `benchmarks/README.md` and `benchmarks/results.txt`:
140x faster `--version` cold start, 111–131x `--help`, 46x less RAM, 23x smaller binary. Audited: cold vs warm startup, `--version`, `--help`, subcommand help, real-work commands (`db path`, `debug paths`, `models`), `serve` startup, TUI startup, peak RSS, binary size/stripping, build profile, and the CRITICAL FAIRNESS question of whether the two binaries perform equivalent work on the measured paths. Streaming/DB/large-session/plugin memory is assessed only for verifiability (no provider credentials, features unwired).

## Repository areas inspected

- `crates/oc-cli/src/main.rs` (version/help short-circuit), `crates/oc-cli/src/cli/args.rs`, `crates/oc-cli/src/cli/cmd/mod.rs`, `cmd/serve.rs`, `cmd/attach.rs` (default TUI), `cmd/db.rs`, `cmd/debug.rs`, `cmd/models.rs`, `cmd/run/mod.rs`, `cli/network.rs`
- `crates/oc-server/src/server.rs` (axum listener exists but is NOT used by `serve`)
- `crates/oc-plugin/src/host.rs`, `js/mod.rs` (libquickjs-sys)
- `reference/packages/opencode/src/index.ts` (yargs entry, static imports), `reference/.../cli/cmd/serve.ts`
- `Cargo.toml` (workspace, **no `[profile.release]` section**), `.cargo/config.toml`
- `benchmarks/compare.sh`, `benchmarks/parity.sh`, `benchmarks/results.txt`, `benchmarks/README.md`

## Commands executed

`/usr/bin/time -v`, `date +%s%N`, `python3` (`time.perf_counter`, `/usr/bin/time -v` subprocess capture), `readelf -S`, `size -A`, `file`, `ldd`, `strip` (on a /tmp copy), `script`/pty harness, `sync; echo 3 >/proc/sys/vm/drop_caches`. Full raw data: `rust-port-audit/artifacts/17-raw-measurements.txt`, `17-help-diff.txt`.

## Runtime scenarios attempted

1. `--version` warm (n=12) and cold (drop_caches, n=5) — both binaries
2. `--help`, `run --help`, `models --help` — both binaries
3. Real-work commands: `db path`, `debug paths`, `models list` — both binaries
4. `serve` bind-to-listening + HTTP reachability + peak RSS — both binaries
5. Default TUI under a pty (first-frame time, peak RSS) — both binaries
6. Poisoned-config probe (invalid JSON + throwing plugin in a sandboxed HOME) to detect config/plugin load on `--version`/`--help`

## Architecture or behavior summary

**Stock `--version`/`--help`** (`reference/.../index.ts`): yargs short-circuits these, so **no config, plugin, DB, or server init runs** (empirically proven: poisoned config and throwing plugin in a sandboxed `HOME` cause no error and no file access on either binary). However the stock binary is a Bun-compiled ELF that (a) boots the full V8/Bun runtime and (b) **statically imports every command module at the top of `index.ts` (lines 3–31)**, so the entire bundled JS module graph (all command builders + transitive deps, embedded as `/$bunfs/…` chunks) is evaluated before yargs prints. Empirical cost: ~1.1–2.0 s wall, ~175–201 MB peak RSS for `--version`/`--help`.

**Rust `--version`/`--help`** (`crates/oc-cli/src/main.rs:78-105`): `clap` parses; `DisplayVersion`/`DisplayHelp` are handled and the process exits **before** the Tokio runtime is even built (`main.rs:107`) and before any dispatch. No config/plugin/DB/server init. Cost: ~2–5 ms wall, ~4.7–5.0 MB RSS (of which ~1.1 ms is process-spawn overhead; true exec ≈1.7–3.9 ms).

**`serve`**: stock loads the full server (config, DB, plugins, HTTP/SSE, auth) and answers HTTP (`/health` 200 in ~5.6 s from spawn, peak RSS 345 MB). The Rust `serve` (`crates/oc-cli/src/cli/cmd/serve.rs:40-67`) binds a **bare TCP socket that reads and discards bytes — it never speaks HTTP** (`server_config()` always returns `None`, and `oc_server::server::listen` is never called; the axum router exists in `oc-server` but is not wired into the CLI). An HTTP request to the Rust server hangs forever (25 s timeout observed).

**TUI**: stock starts a full interactive TUI (first pty frame ~2.1 s, peak RSS ~720 MB). The Rust default command (`attach.rs:91-171`) is **not implemented**: with a TTY it errors `the TUI is not yet wired in this build (TODO(integration): oc-tui)`; without a TTY it prints `opencode: starting TUI (requires a TTY)` and exits 0.

## Positive observations

- `--version` output is byte-identical (`1.18.13\n`, stdout) on both binaries.
- The port genuinely eliminates the ~1–3 s runtime boot and ~185–345 MB runtime footprint on *every* invocation, including real commands (`db path` 1691 ms/274 MB vs 14 ms/5.4 MB; `debug paths` 1062 ms/194 MB vs 7 ms/5.4 MB).
- `db path` returns the identical DB path; `debug paths` output matches (order differs); `models list` errors identically.
- Neither binary loads config/plugins on `--version`/`--help` — the specific "does stock short-circuit too?" fairness question is answered: **both** short-circuit; the workload gap is runtime boot + module-graph evaluation vs native arg parse, not config/plugin load.
- Both binaries are unstripped with no `.debug` sections; the size ratio is therefore not inflated by a stripped-vs-unstripped artifact.

## Findings summary (table)

| Metric | Stock | Rust | Ratio | Fair? | Status |
|---|---|---|---|---|---|
| Binary size | 180,381,824 B (unstripped; embeds V8/Bun runtime + JS bundle) | 8,054,560 B (unstripped; 5.8 MiB stripped copy) | 22.4x (claimed 23x) | FAIR as installed artifact (both self-contained) | REPRODUCED |
| `--version` warm | 1477 ms (1126–2036) | 2.8 ms (2.5–3.4, incl. ~1.1 ms spawn) | ~530x | UNFAIR as "port does same work faster" | REPRODUCED (raw numbers), INVALID (claim semantics) |
| `--version` cold (drop_caches) | 1984 ms | 27 ms | ~72x | UNFAIR (same) | REPRODUCED |
| `--help` | 2915 ms (1960–3424) | 5.0 ms | ~580x | UNFAIR (same) | REPRODUCED (raw), INVALID (semantics) |
| `run --help` | 2790 ms | 3.4 ms | ~820x | UNFAIR (same) | REPRODUCED (raw) |
| `models --help` | ~1044 ms (claimed) | ~8 ms (claimed) | ~131x | UNFAIR (same) | not re-measured; same class |
| peak RSS `--version` | ~186 MB (175–192) | 4.9 MB (4.6–5.0) | ~38–40x (claimed 46x) | UNFAIR (same workload gap) | REPRODUCED directionally |
| peak RSS `--help` | ~200 MB | 4.9 MB | ~41x | UNFAIR (same) | REPRODUCED |
| `serve` startup | ~5.6 s to HTTP 200; 345 MB | never serves HTTP; 5.4 MB | — | INVALID | Rust feature missing |
| TUI startup | ~2.1 s first frame; 720 MB | not implemented | — | INVALID | Rust feature missing |
| `db path` / `debug paths` | 1691 / 1062 ms; 274 / 194 MB | 14 / 7 ms; 5.4 MB | ~120x | output-equivalent; gap still runtime boot | REPRODUCED |

## Detailed findings

### [PERF-01] High — Headline "140x/111–131x faster" claims compare incommensurate workloads. CONFIRMED.
`compare.sh` measures end-to-end time for each binary. The stock pays Bun runtime boot + evaluation of the entire static module graph (`index.ts:3-31`); the Rust binary does clap parse + print only (`main.rs:78-105`, before the Tokio runtime at `main.rs:107`). Neither loads config/plugins (poisoned-config probe: no error, no file access on either). The ratio is real *as a user-visible time-to-answer* metric, but **it is not evidence that the port performs equivalent startup work Nx faster** — the Rust path performs a small fraction of the work. Any claim that "the port is 140x faster at startup" requires an equivalent-work scenario (e.g., config+plugin+DB+server load), which the benchmark does not provide and which, for `serve`/TUI, the port cannot currently provide. Recommendation: reframe claims as "eliminates a ~1.5–2 s / ~190 MB runtime boot per invocation," and add an equivalent-work benchmark.

### [PERF-02] High — `serve` is a TCP drain socket, not a server; server-startup comparison invalid. CONFIRMED.
`crates/oc-cli/src/cli/cmd/serve.rs:40-67` binds a socket and discards all input; `server_config()` returns `None` (`serve.rs:71-72`); `oc_server::server::listen` (a real axum server, `oc-server/src/server.rs:58-127`) is never called. Measured: stock serves HTTP in ~5.6 s; Rust never answers HTTP (25 s timeout). Any "server startup" performance claim would compare real work to a stub.

### [PERF-03] High — Default TUI not implemented; TUI memory/startup claims invalid. CONFIRMED.
`attach.rs:161-170`: with a TTY → `Err(not_wired("the TUI is not yet wired…"))`; without → prints and exits 0. Stock TUI: first frame ~2.1 s, peak RSS 720 MB. No valid TUI comparison is possible.

### [PERF-04] Medium — "cold-start" label is wrong: `compare.sh` never drops caches. CONFIRMED.
`benchmarks/compare.sh:10-25` runs each invocation back-to-back with no cache flush. Published 981 ms for stock `--version` is a **warm** number; true cold (drop_caches) is ~1.98 s (median, n=5). The Rust cold number (27 ms vs 2.8 ms warm) is also affected by paging the 7.7 MB binary. The claimed "140x cold" is actually a warm ratio (~530x) or a cold ratio (~72x) depending on definition; the label does not match the methodology.

### [PERF-05] Medium — Timing/RSS methodology noise. CONFIRMED.
- Wall time via `date +%s%N` around a full `sh`→`date`→`/usr/bin/time`→binary fork chain adds ~1–2 ms of overhead, a large fraction of the ~7 ms measured for the Rust binary (true exec ≈2–4 ms). The published "7 ms"/"9 ms" are therefore upper bounds; the claimed ratio is conservative in the Rust direction.
- Peak RSS uses the **max across runs** (`compare.sh:20`), not median/mean, and wall uses truncating integer averaging (`compare.sh:22`). `results.txt` lacks n, min/max/stddev, and warm/cold distinction.
- Published stock numbers (~981 ms, 185 MB) were not reproduced exactly (I measured 1126–2036 ms, 175–201 MB); direction and magnitude reproduce, exact ratio does not (machine load/cache dependent).

### [PERF-06] Medium — README overstates CLI parity; `parity.sh` actually exits 1. CONFIRMED.
`benchmarks/parity.sh` reports `PARITY DIFF top-level help` and exits 1; `--help` output differs (102 diff lines; e.g., stock `opencode completion …` vs rust `completion …`, and clap leaks a struct doc-comment into rust help). `benchmarks/README.md` "Parity checks" implies a passing surface check. Only `--version` is byte-identical.

### [PERF-07] Low — Build profile is cargo defaults; no tuning evidence. CONFIRMED.
Root `Cargo.toml` has **no `[profile.release]`**; the binary was built with default release (opt-level 3, no LTO, codegen-units 16, panic=unwind, no strip). Binary mtime 2026-08-05 08:31 from source at `fd99c06` (the audit commit `e7fc33e` only touched `benchmarks/`), so the binary matches audited code. Exact `cargo build` invocation: UNVERIFIED (no log found); consistent with `cargo build --release`/`-p oc-cli`.

### [PERF-08] Informational — Binary size claim is fair as installed footprint. CONFIRMED.
Both binaries unstripped, 0 `.debug` sections. Stock: `.text` 57.5 MB, `.rodata` 33.9 MB, embeds V8/Bun runtime + bundled JS (`/$bunfs/…`), self-contained (runs in sandboxed HOME). Rust: 8,054,560 B; stripped copy → 5.8 MiB (31x smaller). "23x smaller" is REPRODUCED as an artifact-size claim; it is a consequence of embedding a JS engine + bundle, not a code-size comparison.

### [PERF-09] Informational — No benchmark coverage exists for most in-scope performance areas. CONFIRMED.
No benchmarks for: streaming memory, long-session memory, DB performance, large sessions, plugin startup, QuickJS init, SSE/protocol throughput, tool-output handling, allocation/clone hotspots, serialization overhead. Several of these are unimplemented in the port (`session list` → `not_wired` in `db.rs`/`session`; DB queries → `not_wired`; TUI → `not_wired`; plugin host exists via libquickjs-sys but is not exercised by any CLI path and has no tests).

## Feature or behavior gaps

- `serve` (HTTP) not implemented (bare socket); TUI not implemented; `session list`, `db <query>` not wired.
- Plugins/QuickJS never initialized in any benchmarked path; no live LLM path exists to measure streaming.

## Test coverage gaps

- No automated benchmark harness with median/stddev/warm-cold split; no regression guard.
- No perf tests for serve/TUI/session/DB (features absent).
- `parity.sh` documents a known-different `--help` as a hard failure while README implies success.

## Unverified areas

- **Streaming memory / long-session memory / peak session RSS**: UNVERIFIED — requires live provider calls; no credentials.
- **DB performance / large-session performance**: BLOCKED — Rust `session`/DB features not wired (`not_wired`), cannot be measured.
- **Plugin startup / QuickJS init / SSE throughput / serialization overhead / clone hotspots**: UNVERIFIED — no benchmarks; plugin runtime not exercised end-to-end.
- **Exact build command**: UNVERIFIED (no log; binary consistent with default `cargo build --release`).

## Final domain verdict

**NOT_READY** — for the *performance claims as published*. The raw measurements reproduce the direction and approximate magnitude of the port's advantage (runtime-boot elimination is real and large: ~1.5–2 s / ~190 MB saved on every invocation), and the 22.4x binary-size and ~38–40x RSS numbers reproduce. But the headline "140x / 111–131x faster" figures are not valid evidence of equivalent-work speedup (the Rust path does a fraction of the work), the "cold-start" label is factually wrong (warm), the exact ratios are not reproduced (my warm `--version` ratio is ~530x, cold ~72x), and two benchmarked subsystems (`serve`, TUI) are unimplemented, so their comparisons are invalid. Remediation before these claims may stand: (1) benchmark equivalent-work scenarios; (2) fix `serve`/TUI; (3) drop caches for cold, report median/min/max/stddev; (4) correct the parity framing.
