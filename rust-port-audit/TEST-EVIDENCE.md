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
`grep -rln "not yet wired" crates/*/src | wc -l` → **17 files**.

### Runtime probes (coordinator, against `target/release/opencode`)
| Command | Result |
|---|---|
| `opencode run "hi"` | `Error:  the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)`; exit **1** (reference: exit 0, real response) |
| `opencode` (no args, stdin closed) | `starting TUI (requires a TTY)`; under a real pty: `Error: the TUI is not yet wired in this build (TODO(integration): oc-tui)`, exit 1 |
| `opencode session list` | `Error:  session listing is not yet wired in this build (TODO(integration): oc-database/oc-session)`, exit 1 |
| `opencode models` | prints raw models.dev cache (6057 lines incl. deprecated) — not the filtered registry |
| `opencode serve --port 43199` | prints "opencode server listening", but `curl /api/health` → **HTTP 000** (no HTTP server; bare socket draining bytes) |
| `opencode db "SELECT 1"` | `database queries are not yet wired in this build`, exit 1 |
| `opencode --version` | byte-identical `1.18.13` to reference |

### Validation of SEC-001 (permission gate)
- `crates/oc-tool/src/model.rs:386` — `ToolContext::ask` pushes to `self.asks` and returns `Ok` (no enforcement).
- `crates/oc-tool/src/core/tool.rs:42` — `CoreContext::assert` records only.
- `crates/oc-server/src/handlers/permission.rs:62` — returns `effect: "allow"` unconditionally.
- No `oc-permission` crate exists; permission logic is embedded in oc-tool/oc-server.

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
