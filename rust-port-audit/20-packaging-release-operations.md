# Agent 20 — Packaging, Installation, Updates, Release Engineering, and Operations

Audit of the opencode-rs Rust port (Cargo workspace, 20 crates) against the vendored reference
(sst/opencode v1.18.13) and the reference executable. Date: 2026-08-05. Evidence files:
`artifacts/20-packaging/runtime-evidence.txt`; differentials in `artifacts/03-cli/run.json`.

## Scope

Distribution and operations readiness: build commands, release profile hardening, binary naming,
version injection, packaging (`cargo install` path), prebuilt binaries/installers, platform
portability (Linux/macOS/Windows/musl), runtime external-binary dependencies, code signing,
SBOM/checksums, reproducibility, `upgrade`/`uninstall` behavior, autoupdate, crash/logging/
diagnostics, telemetry/privacy, proxy handling, CI/release workflows, documentation accuracy,
observability, incident readiness.

## Repository areas inspected

- Root `Cargo.toml`, `.cargo/config.toml`, `.gitignore`, `Cargo.lock` (presence/tracking)
- `crates/oc-cli` (main.rs, lib.rs, version.rs, args.rs, cmd/*, cli/upgrade.rs, cli/paths.rs)
- `crates/oc-util/src/ripgrep/binary.rs`, `util/process.rs`, `util/proxy_env.rs`, `npm.rs`, `shell.rs` (in oc-tool)
- `crates/oc-database/src/path.rs` + Cargo.toml (rusqlite)
- `crates/oc-plugin` (libquickjs-sys, install/npm), `crates/oc-server/src/router.rs`
- `crates/oc-provider/src/models_dev.rs` (embedded catalog)
- `crates/oc-tui/src/app.rs` (version dialog), `oc-util/src/lib.rs`, `oc-llm/src/lib.rs` (version())
- `reference/install` (installer script), `reference/packages/opencode/src/cli/cmd/{upgrade,uninstall}.ts`,
  `reference/packages/opencode/src/cli/upgrade.ts`, `reference/packages/opencode/src/installation/index.ts`,
  `reference/packages/core/src/{observability/logging.ts, shell.ts, global.ts}`, `reference/package.json`
- `benchmarks/` (results.txt, README.md), `CONTEXT.md`, `docs/superpowers/specs/2026-08-05-opencode-rs-design.md`
- Git state (tracked files, Cargo.lock, branches, remote, absence of `.github/`)

## Commands executed

`git ls-files`, `git log`, `git remote -v`, `git branch -a`; `grep -rn` over crates/ and reference/
for version strings, telemetry strings, cfg gates, Command invocations, TODO stubs; `readelf`/`file`
on the release binary; runtime runs of the port and reference binaries (see evidence file):
`--version`, `upgrade 1.18.13`, `upgrade 99.0.0`, `uninstall --help`, `run hello`, default command,
`serve --port 4097`, `web --port 4100`, `session list`, `stats`, `export --sanitize`, `completion bash`,
`agent list`, `debug info`, `debug startup --print-logs --log-level DEBUG`, `debug paths`, `db path`,
`models`, `debug rg files`; log-file size delta check around a `--print-logs` run.

## Runtime scenarios attempted

- `--version` prints `1.18.13` (matches reference). CONFIRMED (RUNTIME).
- `opencode run hello` → **exit 1, "in-process opencode server is not wired"**; reference exit 0 (RUNTIME; corroborated by artifacts/03-cli/run.json).
- Default TUI command → "starting TUI (requires a TTY)" then (in a TTY) "not yet wired" stub. RUNTIME.
- `serve` binds a bare TCP socket that discards bytes; no HTTP API. RUNTIME + STATIC (serve.rs:40-62).
- `upgrade` with target = current → skip; with other target → "automatic upgrades are not supported." No install. RUNTIME.
- `--print-logs --log-level DEBUG` produces zero log output; `~/.local/share/opencode/log/opencode.log`
  (written by the reference binary) unchanged in size after the port run. RUNTIME.
- `session list`, `stats`, `export`, `completion`, `agent list`, `mcp add`, `plug`, `debug rg`, `web` → "not yet wired" stubs. RUNTIME.

## Architecture or behavior summary

The port produces a single `opencode` ELF via `crates/oc-cli/[[bin]]` (8.05 MB, not stripped) using
default cargo release settings (no `[profile.release]`, no LTO/strip/panic=abort). The version is a
**hardcoded constant** `1.18.13` (`oc-cli/src/lib.rs:11`) that collides with the upstream project's
version while the crate version is `0.1.0`. Paths mirror the reference (`~/.local/share/opencode`,
`~/.cache/opencode/bin`, XDG honoring). The CLI surface (help text, flags) closely matches the
reference, but a large fraction of commands are unwired stubs (64 "not yet wired" markers), and the
core LLM loop (run / TUI / attach / mini), the HTTP server under `serve`, and file logging are not
functional. There is no CI, no release pipeline, no installer, no prebuilt binaries, no code signing,
no SBOM, and `Cargo.lock` is gitignored. Runtime behavior relies on external binaries/tools
(`rg`, `git`, `tar`, `unzip`, powershell, shell) with a pin-and-download fallback for ripgrep.
No telemetry phone-home exists in the port (nor hardcoded telemetry in the reference; only env-gated
OTLP). The port additionally omits the reference's startup update check.

## Positive observations

- `opencode --version` = `1.18.13` exactly matches the reference binary (RUNTIME).
- `uninstall` is a real, well-formed implementation (dry-run, force, keep flags; removes data/cache/
  config/state) mirroring the reference's removal list.
- `upgrade` cleanly detects the already-current version and short-circuits; no destructive behavior.
- No telemetry/analytics/posthog/sentry code anywhere in `crates/` (grep). No startup phone-home —
  more private than the reference (reference `cli/tui/worker.ts:61` calls `upgrade()` at startup).
- Cross-platform awareness is present in many subsystems: `cfg(target_os)`/`cfg(unix)`/`cfg(windows)`
  in process/archive/npm/global/ripgrep/browser/config/load/database/path; rusqlite `bundled`,
  crossterm/ratatui, and libquickjs-sys are cross-platform C builds.
- On-disk layout matches the reference (same data/config/cache/state/bin/log paths, same DB path).
- Startup is fast and lightweight (benchmarks/results.txt: 7 ms `--version`, 4 MB RSS vs ~981 ms / ~185 MB reference); 8 MB single binary vs 180 MB reference.

## Findings summary (table)

| ID | Severity | Confidence | Title |
|---|---|---|---|
| RELEASE-001 | Critical | CONFIRMED | `opencode run` headless command non-functional (LocalClient stub) |
| RELEASE-002 | Critical | CONFIRMED | `opencode serve` binds a bare socket, does not serve the HTTP API |
| RELEASE-003 | Critical | CONFIRMED | Default TUI / attach / mini not wired; interactive use impossible |
| RELEASE-004 | High | CONFIRMED | No logging/diagnostics: no tracing subscriber; `--print-logs`/`--log-level` no-ops; no `opencode.log` |
| RELEASE-005 | High | CONFIRMED | `Cargo.lock` gitignored/untracked → non-reproducible builds |
| RELEASE-006 | High | CONFIRMED | Hardcoded version `1.18.13` == upstream version while crate is `0.1.0`; inconsistent `version()` helpers |
| RELEASE-007 | High | CONFIRMED | `upgrade` is a stub; wrong release repo queried; no signatures/checksums/downgrade |
| RELEASE-008 | High | CONFIRMED | 64 "not yet wired" stubs; docs "1:1 functional parity" claim inaccurate |
| RELEASE-009 | Medium | CONFIRMED | No release packaging: no installer (reference has `install`), no prebuilt binaries, no CI, no signing/SBOM |
| RELEASE-010 | Medium | CONFIRMED | Release profile unhardened (no LTO/strip/panic=abort); 8 MB unstripped binary |
| RELEASE-011 | Medium | HIGH | Windows shell fallback missing (no pwsh/cmd.exe path) in `oc-tool/src/shell.rs` |
| RELEASE-012 | Medium | CONFIRMED | Runtime ripgrep auto-download + tar/unzip/powershell dependency; offline environments break grep |
| RELEASE-013 | Medium | CONFIRMED | Models catalog is a static embedded snapshot; runtime refresh (5-min TTL) not ported |
| RELEASE-014 | Low | HIGH | `proxy_env.rs` proxy-from-env port is dead code; relies on reqwest defaults |
| RELEASE-015 | Low | CONFIRMED | `uninstall` does not remove the binary nor print package-manager removal hints (reference does) |
| RELEASE-016 | Low | CONFIRMED | No startup autoupdate check; `autoupdate_disabled`/`always_notify_update` are dead code |
| RELEASE-017 | Low | CONFIRMED | `web`/embedded UI not served; `ui_fallback` always 404 |
| RELEASE-018 | Low | CONFIRMED | No git metadata / commit hash embedded in version output |

## Detailed findings

### RELEASE-001 — `opencode run` headless command non-functional [Critical, CONFIRMED]
STATIC: `crates/oc-cli/src/cli/cmd/run/client.rs:62` `pub struct LocalClient;` and `:64-67` —
`LocalClient::create` returns `Err("the in-process opencode server is not wired yet in this build
(TODO(integration): oc-server)")`. `run/mod.rs:561` calls it. RUNTIME: `opencode run hello` → exit 1
with that error. Differential artifacts/03-cli/run.json: `run hello`, `run --format json hello`,
`run hello world`, `run -- hello` all ref-exit 0 vs rust-exit 1. The primary non-interactive
workflow of the product is non-functional.

### RELEASE-002 — `opencode serve` does not serve the HTTP API [Critical, CONFIRMED]
STATIC: `crates/oc-cli/src/cli/cmd/serve.rs:40-62` — `listen()` binds `tokio::net::TcpListener` and
spawns a loop that reads bytes and discards them; `:38` `TODO(integration): delegate to
oc_server::Server::listen`. `oc-server` (HTTP router, SSE, etc.) is compiled but not wired into the
CLI. RUNTIME: `serve --port 4097` prints "listening" but serves no HTTP endpoints. Any attach/
client/`run --attach` flow over HTTP is impossible.

### RELEASE-003 — TUI / attach / mini not wired [Critical, CONFIRMED]
STATIC: `crates/oc-cli/src/cli/cmd/attach.rs:169` default TUI returns `not_wired("the TUI is not yet
wired in this build (TODO(integration): oc-tui)")` even in a TTY; `:74` attach TUI stub; `:86` mini
stub. RUNTIME: default command prints "starting TUI (requires a TTY)" and exits 0 (non-TTY).
Interactive operation is unavailable.

### RELEASE-004 — No logging / diagnostics [High, CONFIRMED]
STATIC: no `tracing_subscriber`/logger initialization exists anywhere in `crates/` (grep for
`tracing_subscriber`, `.init()`, `Registry::`, env-filter → none). `--print-logs`/`--log-level` only
set env vars (`oc-cli/src/cli/args.rs:103-113`, `OPENCODE_PRINT_LOGS`/`OPENCODE_LOG_LEVEL`) that
nothing consumes. The reference always writes structured logs to
`Global.Path.log/opencode.log` (`reference/packages/core/src/observability/logging.ts` `fileLogger`,
enabled unconditionally in `Logging.loggers()`). RUNTIME: `debug info --print-logs --log-level DEBUG`
emits zero stderr log lines, and `~/.local/share/opencode/log/opencode.log` (9.5 MB, written by the
reference binary) was byte-identical in size before/after the port run. Operational debugging and
incident investigation are impossible.

### RELEASE-005 — Cargo.lock not tracked [High, CONFIRMED]
STATIC: `.gitignore` contains `Cargo.lock`; `git ls-files --error-unmatch Cargo.lock` fails; no
`Cargo.lock` in `git ls-files`. Binary crates should commit Cargo.lock for reproducible builds;
combined with no vendor dir and no offline cache, builds are not reproducible and require crates.io
network access.

### RELEASE-006 — Hardcoded version identical to upstream [High, CONFIRMED]
STATIC: `crates/oc-cli/src/lib.rs:11` `pub const VERSION: &str = "1.18.13"`; printed in
`main.rs:84,102-103` and clap `version = crate::VERSION` (`args.rs:86`). Crate/workspace version is
`0.1.0` (`Cargo.toml`). `oc-util/src/lib.rs:24` and `oc-tui/src/lib.rs:22` return
`env!("CARGO_PKG_VERSION")` (= `0.1.0`); the TUI debug dialog shows `crate::version()`
(`oc-tui/src/app.rs:1223`). RUNTIME: `--version` = `1.18.13` — byte-identical to the official
opencode binary, making the Rust build indistinguishable from upstream in tooling and scripts, while
package managers/cargo see `0.1.0`. No git metadata/commit is embedded.

### RELEASE-007 — `upgrade` is a stub; wrong repo; no verification [High, CONFIRMED]
STATIC: `crates/oc-cli/src/cli/cmd/upgrade_cmd.rs` — after fetching latest it prints "the Rust port is
installed in-process; automatic upgrades are not supported." and exits 0; the `--method` value is
discarded (`:31` `"unknown"` default). `cli/upgrade.rs:36` queries
`https://api.github.com/repos/sst/opencode/releases/latest` — the vendored reference queries
`https://api.github.com/repos/anomalyco/opencode/releases/latest`
(`reference/packages/opencode/src/installation/index.ts:258`) and actually runs the target installers
(curl/npm/brew/choco/scoop, `index.ts:265-309`). No signature/checksum verification anywhere. No
downgrade/rollback. RUNTIME: `upgrade 99.0.0` prints the not-supported message (exit 0).
Also `autoupdate_disabled()`/`always_notify_update()` (`upgrade.rs:56-73`) are dead code — no startup
update check exists (reference `cli/tui/worker.ts:61`).

### RELEASE-008 — Stub surface vs documented parity [High, CONFIRMED]
STATIC: 64 "not yet wired"/`not_wired` occurrences in `oc-cli/src` (17 files; e.g. `export_cmd.rs:11`,
`attach.rs:74,86,123,169`, `run/mod.rs:296`, `agent.rs:33,41`, `db.rs:38,41`, `mcp.rs:55,60,66,73,78,85,92`,
`web.rs:81`, `generate.rs:9`, `plug.rs:19`, `completion.rs:12`, `debug.rs:42-131`). `CONTEXT.md`
priority 1 claims "1:1 functional parity — same CLI surface, config, storage layout, API JSON,
part/message formats, plugin behavior"; the surface parity holds but the functional wiring does not.
RUNTIME: `session list`, `stats`, `export`, `completion bash`, `agent list`, `mcp add`, `plug`,
`debug rg` all error with "not yet wired".

### RELEASE-009 — No distribution/release machinery [Medium, CONFIRMED]
STATIC: no `.github/` directory, no workflow YAML files (find over repo → none), no release/CI
scripts. The reference ships an installer (`reference/install` — downloads a prebuilt asset from
`github.com/anomalyco/opencode/releases`, `--no-modify-path` option, PATH modification). The port has
no installer, no prebuilt artifacts, no code signing/notarization, no checksums/SBOM, no release
provenance. `cargo install --path crates/oc-cli` is the only distribution path (would work — `[[bin]]`
`name = "opencode"` at `oc-cli/Cargo.toml:6-8` with path deps in the workspace — but requires a C
toolchain for `rusqlite` `bundled` and `libquickjs-sys` and installs as a bare binary named `opencode`
that collides with the upstream binary).

### RELEASE-010 — Release profile unhardened [Medium, CONFIRMED]
STATIC: root `Cargo.toml` has no `[profile.release]` (and none in any crate), so cargo defaults apply:
opt-level=3, no LTO, no `strip`, `panic=unwind`, codegen-units=16. RUNTIME: `file` reports the binary
"not stripped", 8,054,560 bytes. No debug-symbol preservation option either way. For a security-
sensitive agent tool, `lto`, `strip = "symbols"` and `panic = "abort"` would be expected hardening.

### RELEASE-011 — Windows shell fallback missing [Medium, HIGH]
STATIC: `crates/oc-tool/src/shell.rs:44-52` — `acceptable()` falls back to `/bin/zsh` (macOS) or
`bash`→`/bin/sh` otherwise; there is no Windows branch. The reference selects `pwsh`/`powershell`/
`gitbash`/`COMSPEC||cmd.exe` on win32 (`reference/packages/core/src/shell.ts` `win()`). On Windows
without a POSIX shell in PATH, the shell tool would attempt `/bin/sh`. Ripgrep download
(`oc-util/src/ripgrep/binary.rs:31-54`) does handle Windows (zip/powershell), so platform support is
partial. UNVERIFIED by runtime (no Windows host).

### RELEASE-012 — Runtime external-binary dependency & auto-download [Medium, CONFIRMED]
STATIC: `oc-util/src/ripgrep/binary.rs:70-115` — resolves system `rg` → cached `bin/rg` → downloads
pinned `ripgrep-15.1.0` from GitHub releases at runtime and writes to the cache dir
(`reqwest::get`, `write_with_dirs`); extraction shells out to `tar` (unix) or `powershell.exe`
(windows). `oc-util/src/util/archive.rs` also uses `unzip`/powershell. Plugin install uses `git`
(`oc-plugin/src/npm.rs:172`). Shell tool runs `bash`/`zsh`. On an offline/air-gapped host, the first
grep without a system `rg` fails; the download is unsigned (HTTPS-only). This is a runtime network +
write behavior worth documenting and gating.

### RELEASE-013 — Static models catalog, no refresh [Medium, CONFIRMED]
STATIC: `crates/oc-provider/src/models_dev.rs:6-15` — reference fetches `https://models.opencode.ai/api.json`
with a 5-minute TTL cache; port embeds a static snapshot `data/models.json`
(`include_str!("../data/models.json")`, `models_dev.rs:294`) and does not port the refresh flow
(`TODO(integration)`). New models never appear; `opencode models --refresh` fetches (cli models_dev)
but the provider registry uses the embedded snapshot. Staleness/behavioral divergence over time.

### RELEASE-014 — Proxy helper is dead code [Low, HIGH]
STATIC: `crates/oc-util/src/util/proxy_env.rs` ports proxy-from-env, but no `get_proxy_for_url`
caller exists (grep → only tests) and no `reqwest::ClientBuilder.proxies(...)` call exists anywhere.
reqwest `ClientBuilder::new()` sets `auto_sys_proxy: true` (reqwest source), so env proxies are
honored by default for most clients — but the port's own ported logic is unused and its behavior is
not verified. UNVERIFIED by runtime (no proxy env test).

### RELEASE-015 — uninstall does not remove binary / no pkg-manager hints [Low, CONFIRMED]
STATIC: `crates/oc-cli/src/cli/cmd/uninstall.rs` — collects data/cache/config/state and
`remove_dir_all`s them; never touches the binary. Reference `uninstall.ts:132-136` additionally
prints `npm uninstall -g opencode-ai`, `brew uninstall opencode`, etc. Minor UX divergence.

### RELEASE-016 — No autoupdate; privacy-positive [Low, CONFIRMED]
STATIC: reference `cli/upgrade.ts:10-57` runs at startup (`worker.ts:61`) and may even auto-upgrade
patch releases; the port implements none of it (functions are dead code). The port performs **no**
startup network calls at all. This is a parity gap but a privacy improvement (no GitHub version
ping on every start).

### RELEASE-017 — web/embedded UI not served [Low, CONFIRMED]
STATIC: `crates/oc-server/src/router.rs:51-56` `ui_fallback` always returns 404
(`TODO(integration): embedded UI assets`); `oc-cli/src/cli/cmd/web.rs:81` prints "(web interface not
yet wired)". Reference proxies to `app.opencode.ai` / serves embedded UI.

### RELEASE-018 — No git metadata in version output [Low, CONFIRMED]
STATIC: no `env!("GIT_*")`, `option_env!`, or `build.rs` anywhere; `--version` is the bare hardcoded
string. No provenance of the exact commit the binary was built from — releasability issue.

## Feature or behavior gaps

- Core LLM loop (run / TUI / attach / mini / serve-HTTP) not wired → product not operational.
- Session/stats/export/import(network)/completion/agent/mcp/plugin/db query/generate/console/debug subcommands stub.
- File + stderr logging absent; reference `opencode.log` not produced.
- Autoupdate/update-notify flow absent; `autoupdate` config silently ignored.
- Runtime models refresh absent (static snapshot).
- Windows default-shell selection gap.

## Test coverage gaps

- No integration tests for `--version`, `upgrade`, `uninstall`, `serve`, `run` end-to-end.
- No tests exercising the CLI wiring seams (LocalClient, serve listen, TUI default path).
- No offline/proxy/no-network test for ripgrep download / models fetch / upgrade latest.
- No cross-compile or CI matrix for macos/win/musl; no `cargo install --path` verification test.
- No test asserting `opencode.log` is written or that `--print-logs` emits output.

## Unverified areas

- Windows/macOS/musl builds (no hosts available) — BLOCKED by missing evidence (mark findings 011, 012 as STATIC only).
- `cargo install --path crates/oc-cli` end-to-end (would require a build; not executed per constraints).
- Live `upgrade` fetch to GitHub (network available but not relied upon); endpoint behavior is STATIC only.
- Proxy env behavior of reqwest default clients in this build.
- The oc-server/oc-tui crates compile and contain functionality that is simply not wired into oc-cli — whether their internals work is other agents' scope; here they are treated as unwired.

## Final domain verdict

**NOT_READY**

The port is not yet distributable or safely operable: the primary run/TUI/serve-HTTP workflows are
unwired stubs, `opencode.log`/`--print-logs` produce no diagnostics, `Cargo.lock` is not committed,
the version string falsely claims upstream `1.18.13`, `upgrade` is a stub with no verification, and
there is no installer/CI/signing/SBOM. Telemetry is clean (no phone-home; more private than the
reference). Uninstall is genuinely functional for data directories.
