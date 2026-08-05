# Plan 20 — Performance Benchmark Methodology & Documentation Accuracy

Agent 20 · Wave 0 (READ-ONLY planning) · Domain: Performance / Release-gate doc accuracy
Date: 2026-08-05 · Branch: `fix/audit-remediation`

## 1. Owned findings

| ID | Severity | Blocker | Summary |
|---|---|---|---|
| PERF-001 | High | YES | Published "140x / 111–131x faster" compares incommensurate work: stock `--version`/`--help` boot Bun/V8 and evaluate the full static module graph (`reference/.../index.ts:3-31`); the Rust path short-circuits at clap parse (`crates/oc-cli/src/main.rs:78-105`) **before the Tokio runtime is built** (`main.rs:107`). Both short-circuit config/plugin/DB (poisoned-config probe: no error, no file access). Ratio is a real *time-to-answer* gap but not an equivalent-work speedup. |
| PERF-002 | Low | NO | Harness noise: "cold-start" is warm (`compare.sh` never drops caches); RSS uses max-not-median (`compare.sh:20`); wall uses truncating integer average (`compare.sh:22`); shell `date`→`/usr/bin/time` fork chain adds ~1–2 ms (large relative to the ~2.8 ms Rust number); `results.txt` lacks n/min/max/stddev/warm-cold split; published stock numbers (981 ms/185 MB) not reproduced (1126–2036 ms / 175–201 MB measured). |
| INFO-001 | Informational | NO | No telemetry in the port (no posthog/sentry/analytics strings, no startup network calls). Positive privacy posture vs reference (which pings GitHub on TUI start). Must be documented, not "fixed". |
| (gate) DOC-ACCURACY | High (release gate) | YES | `CONTEXT.md` goal #1 states "1:1 functional parity" as a *current property* while the executable reaches no domain crate (`run`/`serve`/TUI/session/db all `not_wired`). `benchmarks/README.md` "Parity checks" implies a passing surface check while `parity.sh` actually exits 1 on `--help`. Both documents must be corrected to state intent vs current state. |

Owned evidence base: `rust-port-audit/17-performance-resources.md`, `rust-port-audit/artifacts/17-raw-measurements.txt`, `17-help-diff.txt`, FINDINGS.json PERF-001/PERF-002/INFO-001, `benchmarks/{compare.sh,parity.sh,README.md,results.txt}`.

## 2. Workload-gap analysis (`--version` / `--help`)

**Stock (`reference/packages/opencode/src/index.ts`):**
- yargs short-circuits version/help → no config, plugin, DB, or server init.
- BUT the binary is a Bun-compiled ELF that boots V8/Bun and **statically imports every command module at module scope (lines 3–31)**; the entire bundled JS module graph is evaluated before yargs prints.
- Empirical: ~1.1–2.0 s wall, ~175–201 MB peak RSS for version/help.

**Rust (`crates/oc-cli/src/main.rs`):**
- clap parse → `DisplayVersion`/`DisplayHelp` handled and `exit` **before** the Tokio runtime is built (`main.rs:107`) and before dispatch.
- No config/plugin/DB/server init.
- Empirical: ~2–5 ms wall (~1.1 ms spawn), ~4.7–5.0 MB RSS.

**Conclusion for the plan:** The workload gap is *runtime boot + module-graph evaluation* vs *native arg parse*. Equivalent-work speedup is unproven and currently unmeasurable for `serve`/TUI (features unwired). The honest claim to publish is: **the port eliminates a ~1.5–2 s / ~190 MB per-invocation runtime boot**, not "performs startup Nx faster."

## 3. Corrected benchmark harness design (replaces `compare.sh`)

New: `benchmarks/bench.sh` (invoked via `benchmarks/run.sh`, output to `benchmarks/results/` timestamped dir + `results.txt`).

### 3.1 Measurement engine
- **Timing:** Python3 driver (like the audit's) using `time.perf_counter()` around a direct `subprocess.run` of each binary — eliminates the `sh`→`date`→`/usr/bin/time` fork-chain overhead. Hyperfine is *not* assumed present (verified: absent); a small Python harness is the portable choice.
- **RSS:** `/usr/bin/time -v` per-run, captured per invocation, not aggregated inline.
- **Warm/cold split:** explicit flag `--cold` / `--warm`. Cold = `sync; echo 3 >/proc/sys/vm/drop_caches` before each run **requires root** — verified available in this environment. Non-root runs fall back to warm-only and label as such (never call it "cold").
- **N:** default 8 warm, 5 cold per scenario (matches audit, sufficient for the ~300 ms stock stddev).
- **No `|| true`:** each scenario must complete its defined work or the run **fails the harness** (see §5). Suppressed failures are how the `serve`/TUI stubs got benchmarked.

### 3.2 Stats to report (per scenario, per binary)
- wall: **median** (primary), plus mean, stddev, min, max, p90, p95, **N** (never truncating integer average).
- RSS: **peak RSS median** (primary), plus mean/min/max; separately **steady-state RSS** where the scenario is long-lived (`serve`, TUI): sample RSS at t+5 s and t+30 s, report both.
- Workload-gap column: for every scenario, state whether both binaries perform equivalent work (YES/PARTIAL/NO + one-line note). NO/PARTIAL rows are flagged in output.

### 3.3 Output format
```
## bench run 2026-08-05T…  mode=warm N=8 machine=… profile=release (assert: lto=… strip=…)
### scenario: serve-to-first-HTTP   equivalent_work=YES
bin        median   mean   stddev  min     max     p90     p95     peakRSS_med  steadyRSS_5s  steadyRSS_30s
stock      5632 ms   …      …       …       …       …       …       345 MB       341 MB        344 MB
rust        512 ms   …      …       …       …       …       …        18 MB        15 MB         16 MB
ratio       11.0x    …                                                                   
verdict: PASS (both served /api/health 200 within timeout)
```

## 4. Equivalent-work scenario list

Gate: **a scenario may only run when both binaries complete the same defined unit of work.** Do not benchmark missing features (see §6 dependencies).

| # | Scenario | Defined unit of work (must complete on both sides) | Equivalent? | Status today |
|---|---|---|---|---|
| S1 | `--version` | process exits 0, prints `1.18.13` | Partial — same answer, different init work | Both OK (reproduced) |
| S2 | `--help` (root) | process exits, help on stderr (byte-diff allowed, structural) | Partial — same as S1 | Both OK; help text differs (CLI-004, agent 17) |
| S3 | config load | load resolved `opencode.json` (+ env overrides) from a **fixture HOME**, then exit 0 | YES (defined via `debug paths`/config dump) | Rust `Context::load` runs but config resolution is `oc_config`-side; **PARTIAL until wired** |
| S4 | provider registry load | `models`/`providers` command lists the registry, exits 0 | YES | `models list` errors identically today (audit); **PARTIAL until real provider wiring** |
| S5 | DB init + `db path` | create/open the SQLite DB, print path, exit 0 | YES (DB-open work) | Rust `db path` works; `db query`/`session list` are `not_wired` → keep scenario to `db path` only |
| S6 | server-to-first-HTTP | spawn `serve --port 0` (or fixed), poll `GET /api/health` until 200 or timeout (25 s), then kill | YES | **BLOCKED** — Rust `serve` is a bare TCP drain socket (`serve.rs:40-67`); `oc_server::Server::listen` (`oc-server/src/server.rs:58-127`) exists but is not called (CLI-002) |
| S7 | local session round-trip (mock provider) | with a mock OpenAI-compatible SSE provider (reuse audit `09-mock-provider.py`), `opencode run "hi"` completes a round-trip and writes the session | YES | **BLOCKED** — `run` reaches no domain crate; requires CLI-001/SESSION-001/LLM wiring |
| S8 | plugin load | load a fixture plugin, run `plugin list`, exit 0 | YES (QuickJS init + host) | **BLOCKED** — plugin host (`oc-plugin/src/host.rs`) not exercised by any CLI path (PLUGIN-004) |
| S9 | MCP init | `mcp list` with a fixture stdio MCP server config, exit 0 | YES | **BLOCKED** — MCP not wired into CLI/server (PROTO-001) |
| S10 | TUI first frame | under a pty, default `opencode` renders first frame (< 10 s), then kill | YES | **BLOCKED** — `attach.rs:161-170` returns `not_wired`; `oc-tui::app::run_async` exists (`oc-tui/src/lib.rs:19`) but is not called (CLI-003) |
| S11 | binary size | artifact bytes (both unstripped as installed) | YES | Both reproducible; ratio 22.4x (claimed 23x), fair as installed footprint (PERF-08) |
| S12 | real-command boot (no-equivalent-work control) | `db path`, `debug paths`, `models list` | PARTIAL — same answer, different init | Reproduces; report as "runtime-boot elimination," not speedup |

**S1/S2/S12** stay as *time-to-answer* metrics labeled with the workload caveat. **S3–S10** are the equivalent-work scenarios; only these may support speedup claims. **S6, S7, S8, S9, S10 are the deliverables** that unlock the honest headline comparison once wired.

## 5. Fail-fast rules

The harness exits non-zero (and does not append a PASS row) when:
1. Any scenario command fails, times out, or exits non-zero on **either** binary — no `|| true` anywhere. A "benchmark" of a stub that doesn't do the work is a harness failure, not a data point.
2. S6: the probe never receives HTTP 200 within the timeout → FAIL (not "record wall timeout as a number").
3. S7: the mock-provider log shows no request from the Rust side → FAIL.
4. S10: no first pty frame within timeout → FAIL.
5. Workload-gap check: scenario declares `equivalent_work=YES` but the harness cannot observe completion of the defined unit of work on both sides → harness aborts before writing stats.
6. Build-profile mismatch: harness asserts binary mtime is newer than `target/release` build and prints the build profile (see §7); it warns (not fails) on stale binaries.

`parity.sh` is kept but its exit code is honored: `bench/run.sh` treats a `PARITY DIFF` as a warning, not the benchmark result; parity is tracked separately (CLI-004/CLI-005).

## 6. Dependencies on other agents (do not benchmark missing features)

| Scenario | Requires (owner agent) | Wave |
|---|---|---|
| S6 server-to-first-HTTP | CLI-002 wire `oc_server::Server::listen` into `serve` (Agents 10/12) | Wave 4 |
| S7 session round-trip | CLI-001 LocalClient + SESSION-001 + LLM wiring + TOOLS-001 runner + DB-001 (Agents 3,6,7,12,13) | Wave 4 |
| S8 plugin load | PLUGIN-004 host wiring (Agent 15) | Wave 4 |
| S9 MCP init | PROTO-001 wiring (Agent 13) | Wave 4 |
| S10 TUI first frame | CLI-003 TUI launch (Agent 16) | Wave 4 |
| S3/S4/S5 | config/DB/provider wiring (Agents 3,4,12) | Waves 2–3 |
| release profile for all | SUPPLY-004 `[profile.release]` lto/strip/panic (Agent 19) | Wave 2 — **changes all absolute numbers; re-baseline required** |

The benchmark harness is authored in Wave 0/1 but **re-baselined in Wave 5** (after functional integration), per §8.

## 7. Exact binaries / build profiles to benchmark

- Stock oracle: `/root/.opencode/bin/opencode` (180,381,824 B, unstripped, Bun-compiled, ELF x86-64). Reports `1.18.13`. Immutable reference — do not rebuild.
- Rust: `/root/opencode-rs/target/release/opencode` (8,054,560 B, unstripped). Built from commit `fd99c06` (audit commit `e7fc33e` touched only `benchmarks/`); binary mtime 2026-08-05 08:31 matches.
- **Profile today:** root `Cargo.toml` has **no `[profile.release]`** → cargo defaults (opt-level 3, codegen-units 16, panic=unwind, no LTO, no strip). `SUPPLY-004` (Agent 19) will add `lto=true, strip=true, panic=abort`; **all memory/size numbers must be re-measured after that lands.**
- Harness must print the profile used and assert reproducibility (record `git rev-parse HEAD`, binary size, `readelf -S` presence of `.debug`).

## 8. Documentation updates

### 8.1 `benchmarks/README.md` (rewrite)
- Replace "Results — 2026-08-05" table with the corrected-workload framing:
  - Keep: binary size (fair as installed artifact), time-to-answer numbers.
  - Add: a **workload-gap note** — stock pays runtime boot + full module-graph eval on every invocation; Rust `--version`/`--help` short-circuit before the runtime builds; equivalent-work scenarios listed in §4 of this plan with their status.
  - Reframe headline: "the port eliminates a ~1.5–2 s / ~190 MB runtime boot per invocation" instead of "140x faster."
  - State that `serve`/TUI comparisons are absent until those features are wired (no numbers until then).
- Parity section: mark `parity.sh` `--help` diff as a known, tracked divergence (reference `opencode completion` vs Rust `completion`, clap doc-comment leak), not a passing check.

### 8.2 `CONTEXT.md` (accuracy, release gate "Documentation accuracy")
- Goal #1 "**1:1 functional parity**" must be reframed as **aspiration/design target**, with an explicit "Current status" line pointing at the release gate: executable wiring incomplete (see RELEASE-GATE.md, INTEGRATION-001). Do not describe the current tree as achieving parity.
- Add a short "Benchmarks" note: current claims are time-to-answer; equivalent-work numbers pending integration (link `benchmarks/README.md`).
- Add INFO-001 privacy posture line: "No telemetry; the port makes no startup network calls (reference pings GitHub releases on TUI start)."

### 8.3 `benchmarks/results.txt`
- Appended format per §3.3 with mode/N/min/max/stddev; superseded runs archived under `benchmarks/results/`.

## 9. Risks

| Risk | Mitigation |
|---|---|
| Re-baselining in Wave 5 lands late and blocks release-gate sign-off | Harness is CI-runnable standalone; only *equivalent-work* rows require Wave 4 wiring; time-to-answer rows valid now |
| Stale binaries measured (post-SUPPLY-004 profile change) | Harness asserts binary mtime/HEAD/profile and warns on mismatch |
| Root requirement for cold runs limits portability | Cold optional; warm-only runs labeled as such; never mislabel warm as cold |
| `parity.sh` exit 1 misread as benchmark failure | `bench/run.sh` treats parity as separate tracked signal, not benchmark result |
| Feature wiring slips past the fail-fast gate | Every scenario must complete defined work on both sides or harness FAILs — this is the guard against re-publishing stub benchmarks |
| Stock binary is the only oracle (180 MB, Bun) | It is immutable reference; all comparisons anchor to it; record its bytes |

## 10. Merge-order recommendation

1. **Wave 0/1:** merge this plan + the harness scaffolding (`benchmarks/bench.sh`, `run.sh`, Python driver, output format) — no production source touched. Merge order within the release: **after** Agent 19's SUPPLY-004 profile change is *approved* but before the profile actually lands would waste a baseline; so: author harness now, re-baseline after SUPPLY-004.
2. **Waves 2–4:** functional integration (Agents 3/4/6/7/10/12/13/15/16) — nothing here benchmarks until wiring lands.
3. **Wave 5 (final, before release gate):** re-run the harness against the integrated build; only then may equivalent-work rows populate `results.txt` and the performance release gate ("claims fair + reproducible") flip from FAIL to PASS.
4. Documentation corrections (CONTEXT.md goal #1, benchmarks/README.md) land with the re-baselined results in Wave 5, so docs never claim parity the binary lacks.

Primary gate to reopen: **Release-gate "Performance claim validity"** (currently FAIL) + **"Documentation accuracy"** (currently FAIL). Success = equivalent-work rows green for S6–S10, warm/cold split reported, no `|| true`, docs stating current-state honestly.
