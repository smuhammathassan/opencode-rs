# Plan 19 — Release Engineering, Supply Chain, Licensing, Logging, CI

Agent 19 · Domain 20 (release/packaging) + 16 (supply chain) · Wave 0 READ-ONLY planning
Audited commit `e7fc33e` · Baseline HEAD `90727e19` · Reference v1.18.13 · Oracle `/root/.opencode/bin/opencode`

---

## 1. Owned consolidated findings

| Finding | Severity | Blocker | Evidence (report) | State today |
|---|---|---|---|---|
| SUPPLY-002 | High | YES | `.gitignore:3` ignores `Cargo.lock`; `git ls-files` shows 0 | Lockfile untracked |
| SUPPLY-003 | High | YES | No `LICENSE`; verbatim reference content embedded (prompts, polyfill, models.json) with no notice | Legal non-compliance |
| SUPPLY-004 | Medium | NO | No `[profile.*]` in workspace; 8.05 MB unstripped binary | Defaults |
| RELEASE-001 | High | YES | No tracing subscriber; `--print-logs`/`--log-level` set env vars nothing consumes; no `opencode.log` | No diagnostics |
| RELEASE-002 | Medium | NO | `oc-cli/src/lib.rs:11` hardcodes `1.18.13`; crate is `0.1.0`; `oc-util`/`oc-tui` return `CARGO_PKG_VERSION` | Version confusion |
| RELEASE-004 | Medium | NO | No `.github/`, no CI, no packaging, no SBOM/signing | Cannot distribute |
| RELEASE-005 | Medium | NO | Runtime shells to `rg`(download)/`git`/`tar`/`unzip`/`powershell`; offline/air-gap fails | External deps ungoverned |
| RUST-015 / clippy-hygiene | Info | NO | `cargo clippy -D warnings` exit 101, 45 errors (oc-plugin 29, oc-util 19, oc-schema 6) | Lint gate FAIL |
| Attribution-details | Info | NO | `oc-provider/src/models_dev.rs` fetches live URL; QuickJS VERSION unattributed | Provenance gaps |

I also coordinate: version-string consistency (`RELEASE-018`-adjacent, no git commit in version), toolchain pinning (`rust-toolchain` absent), and the offline/external-binary policy that touches `RELEASE-003` (AG-17) and `SUPPLY-001` (AG-15).

## 2. Files to change

Foundation (Wave 1):
- `.gitignore` — remove line 3 `Cargo.lock`
- `Cargo.toml` (root) — add `[profile.release]`; add `[profile.dist]` (inherits release); `rust-version`/MSRV; new workspace deps (`tracing-appender`, `tar`, `flate2`, `zip`)
- `rust-toolchain.toml` — new, pin `channel = "1.97"` (baseline rustc 1.97.1)
- `LICENSE` — new (MIT; upstream + port copyright)
- `NOTICE` — new (attribution manifest for embedded reference content)
- `crates/oc-util/src/logging.rs` — new (tracing subscriber init, reference formatter, log file + stderr layers)
- `crates/oc-util/src/version.rs` — new (shared `REFERENCE_VERSION`/`PORT_VERSION`/`GIT_COMMIT`/`BUILD_PROFILE`)
- `crates/oc-util/src/lib.rs` — export `logging`, `version`; fix `version()` to reference-compat const
- `crates/oc-cli/src/main.rs` — call `oc_util::logging::init()` after parse; use shared version consts
- `crates/oc-cli/src/cli/args.rs` — keep `apply_env()`; feed parsed values into init (or keep env-only, reference-equivalent)
- `crates/oc-cli/src/lib.rs` — replace hardcoded `VERSION` with shared consts (keep `--version` byte-parity)
- `crates/oc-cli/build.rs` — new, emit `GIT_COMMIT` (fallback `GIT_COMMIT` env, else `"unknown"`)
- `crates/oc-tui/src/lib.rs:21` — return shared const (fix 0.1.0 vs 1.18.13 inconsistency)
- Clippy sweep: `oc-plugin/src/js/runtime.rs`, `js/transpile.rs`, `loader.rs`, `npm.rs`, `shared.rs`; `oc-util/src/ripgrep/mod.rs`, `format/formatter.rs`, `util/process.rs`, `util/proxy_env.rs`, `fs_util.rs`, `util/error.rs`, `util/rpc.rs`; `oc-schema/src/session_message.rs`, `v1/session.rs`

CI/packaging (Wave 5):
- `.github/workflows/ci.yml` — new
- `.github/workflows/release.yml` — new
- `deny.toml` — new (cargo-deny policy)
- `crates/oc-util/src/ripgrep/binary.rs` — checksum-verify pinned download; honor `OPENCODE_NO_DOWNLOAD`
- `crates/oc-util/src/util/archive.rs` — in-process extraction (tar/flate2/zip) replacing subprocesses
- `README`/docs — external-binary requirements + offline/air-gap runbook

## 3. Design

### 3.1 Lockfile & reproducibility (SUPPLY-002, RELEASE-005-build side)
- Un-ignore and commit `Cargo.lock` (339 pkgs: 20 path + 319 registry, all checksummed, 0 yanked, 0 git deps). Binary crate → must commit lockfile.
- Add `rust-toolchain.toml` (`channel = "1.97"`) so CI, `cargo install`, and dev all resolve the same toolchain (audit: MSRV never declared).
- CI gate: `cargo build --workspace --all-targets --all-features --locked` + `cargo build --release --locked`. Add `--locked` to every build/test/clippy invocation.
- Reproducibility CI job: two clean `--release --locked` builds in fresh target dirs, `sha256sum` diff (document `SOURCE_DATE_EPOCH` note; Rust is deterministic for same toolchain/target/env).
- Optional (document tradeoff, do NOT block): `cargo vendor` for air-gapped distribution — 339 crates is large; defer. The committed lockfile + `--locked` covers the reproducibility requirement.
- `Cargo.lock` drift guard: `git diff --exit-code Cargo.lock` after `cargo build` in CI catches silent re-resolution.

### 3.2 LICENSE / NOTICE / attribution (SUPPLY-003, attribution-details)
- `LICENSE` = MIT text, dual copyright block: upstream `Copyright (c) 2025 opencode` (required by MIT for redistributed derivative) + port `Copyright (c) 2026 opencode-rs contributors`.
- `NOTICE` manifest lists every verbatim/derived embedded asset, its reference source path, license, and provenance:
  - `crates/oc-session/assets/prompt/*.txt` == `reference/packages/opencode/src/session/prompt/*.txt`
  - `crates/oc-tool/src/prompts/*.txt` == `reference/packages/opencode/src/tool/prompts/*.txt` (diff-verified for `default.txt`)
  - `crates/oc-plugin/src/polyfill/runtime.js` (1165 lines, derived `@opencode-ai/plugin` API)
  - `crates/oc-provider/data/models.json` (3.5 MB; `models.opencode.ai/api.json` snapshot — add provenance + note the URL is not the vendored reference; see SUPPLY-007/AG-14)
- **Do NOT edit the verbatim asset files.** The audit diff-verifies prompts byte-identical to the reference; adding SPDX headers to `.txt`/`.js` breaks parity goldens. Attribute via `NOTICE` + a `/// From reference/...`/provenance doc-comment on the Rust module that `include_str!`s them (`models_dev.rs:294`, polyfill, prompt loaders).
- CI presence gate: `test -f LICENSE && test -f NOTICE`, plus a small test asserting each embedded asset path listed in `NOTICE` is present (prevents new embeds without attribution).
- `cargo-deny` `licenses` section enforces the permissive-only allowlist (matches audit: MIT/Apache-2.0/ISC/BSD/Unicode-3.0/Zlib/MPL-2.0/CDLA-Permissive; no GPL).

### 3.3 Release profile (SUPPLY-004)
```toml
[profile.release]
opt-level = 3
lto = "thin"          # thin: size/perf balance; "fat" reserved for [profile.dist]
codegen-units = 1
panic = "unwind"      # <<< stays unwind until RUST/UX/ASYNC panic paths are merged (see §7)
strip = "symbols"     # strips symtab; keep line tables via debug="line-tables-only" for triage

[profile.dist]        # publish-time maximum: fat LTO
inherits = "release"
lto = "fat"
codegen-units = 1
```
- Land `lto`/`codegen-units`/`strip` in Wave 1 (safe, no behavior change). **`panic = "abort"` is gated** on: RUST-001/002/003 (AG-15) FFI `catch_unwind` + cycle/cap fixes, UX-002 (AG-16) terminal-restore panic hook/signals, ASYNC-002 + RUST-005 (AG-09). Flip to `abort` as a final Wave-5 change after those land; `catch_unwind` in the QuickJS trampoline and `panic=abort` are mutually exclusive, so the trampoline must be proven panic-free or must route through a panic hook before abort is enabled. Document the tension explicitly in the flip commit.
- Baseline binary 8.05 MB → target <4 MB stripped. Re-run `benchmarks` (AG-20) to record size/perf.

### 3.4 Logging subsystem (RELEASE-001)
Mirror `reference/packages/core/src/observability/logging.ts`:
- New `crates/oc-util/src/logging.rs`; root `tracing`/`tracing-subscriber` (env-filter) already in `[workspace.dependencies]`; add `tracing-appender = "0.2"` for non-blocking file writes (reference notes batchWindow=0 causes idle CPU — use a sane non-blocking guard, not 0).
- `init()` reads `OPENCODE_LOG_LEVEL` (DEBUG/INFO/WARN/ERROR; default INFO — reference `minimumLogLevel()`) and `OPENCODE_PRINT_LOGS` (default off).
- Two layers: (1) file layer → `Global.Path.log/opencode.log` (`oc-util/src/global.rs:56` `path::log()` = `data()/log`, append mode like reference `flag:"a"`); (2) stderr layer only when `OPENCODE_PRINT_LOGS=="1"` (reference `loggers()`).
- Formatter matches reference `key=value` shape: `timestamp=... level=... run=<pid/run-id> message=...`, values JSON-quoted when they contain whitespace/`=/`"`/`\`.
- `main.rs`: call `oc_util::logging::init()` immediately after `Cli::try_parse` success and `apply_env()` (before runtime build) so server/run wiring agents (AG-10/11/12) get logging for free. `--version`/`--help` short-circuit stays logging-free (parity + speed).
- Tests: unit (formatter escaping, level filter, run-id) + integration — `opencode --print-logs --log-level DEBUG debug info` → non-empty stderr AND `opencode.log` created/appended; `--log-level ERROR` suppresses INFO; log file path honors XDG/data dir.

### 3.5 Version (RELEASE-002, RELEASE-018, PROTO-002-coordination)
Three orthogonal values, one shared module `crates/oc-util/src/version.rs`:
- `REFERENCE_VERSION = "1.18.13"` — the compat target; `opencode --version` prints exactly this (byte-parity with the oracle preserved — differential tests assert equality).
- `PORT_VERSION` = workspace `0.1.0` (from `env!("CARGO_PKG_VERSION")` — the crate/package version).
- Build metadata: `GIT_COMMIT` (from `oc-cli/build.rs`: `git rev-parse --short HEAD`, overridable via `GIT_COMMIT` env, fallback `"unknown"`), `BUILD_PROFILE` (`env!("PROFILE")`), `INSTALLATION_CHANNEL`.
- Surface: `opencode --version` → `1.18.13` (unchanged); `opencode debug info` and a new hidden `--version --verbose` print `0.1.0 (reference 1.18.13, commit <sha>, release|debug)`. This makes the port distinguishable from upstream in tooling without breaking drop-in scripts.
- Fix inconsistency: `oc-util::version()` (lib.rs:23) and `oc-tui::version()` (lib.rs:21) currently return `CARGO_PKG_VERSION`=`0.1.0` while the binary reports `1.18.13` — route both through the shared module (return `REFERENCE_VERSION` for runtime parity; expose `PORT_VERSION` separately). `oc-cli::VERSION` becomes `oc_util::version::REFERENCE_VERSION` re-export. `upgrade_cmd.rs:33` version-compare keeps using the same const.
- Coordinate with AG-13 (PROTO-002 "share the workspace version") and AG-17 (RELEASE-003 upgrade compares versions) — land the shared module early so they build on it.

### 3.6 CI matrix & quality gates (RELEASE-004, clippy-hygiene)
`.github/workflows/ci.yml`, triggered on PR + push to `main`/`fix/*`:
- Matrix: `ubuntu-latest` (x86_64 Linux), `macos-13` (x86_64) + `macos-14` (aarch64), `windows-latest` (x86_64). Optional stretch: aarch64 Linux via cross/zigbuild — defer (tooling not installed in audit).
- Steps per job: checkout → `rust-toolchain.toml` (1.97) → `Swatinem/rust-cache` → `cargo fmt --all -- --check` → `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → `cargo test --workspace --all-features --locked` → `cargo build --release --locked` → binary smoke (`opencode --version`, `opencode help`, `opencode debug paths`).
- Supply-chain job (Linux): `cargo-deny check` (advisories/bans/licenses/sources; `deny.toml`: sources=only crates-io, licenses allowlist from audit, duplicate-ban warn) + `cargo-audit` (rustsec advisory DB). Also the reproducibility job (two clean `--locked` builds, sha256 diff) + `Cargo.lock` drift guard.
- E2E/differential (coordinated with AG-18 harness): CI runs the binary-level suite against the oracle where a runner can host it; at minimum version/help/logging surface assertions live here.

### 3.7 Release pipeline, artifacts, SBOM, provenance (RELEASE-004)
`.github/workflows/release.yml` on tag `v0.1.x`:
- Build release artifacts per target (Linux x86_64, macOS arm64+x86_64, Windows x86_64): single stripped binary (`profile.dist`).
- Package per-platform tarballs/zip; generate `SHA256SUMS`.
- SBOM: emit `cargo metadata --format-version 1 --locked` snapshot as the machine SBOM artifact + cargo-deny license report (document that no SPDX generator is installed in the audit env; keep to locked-metadata + deny report, upgrade later).
- Provenance/signing: `actions/attest-build-provenance` (SLSA provenance); macOS notarization + Windows `signtool` + optional GPG on SHA256SUMS documented as **BLOCKED without code-signing certs** — record as follow-up, don't block binary distribution.
- GitHub release body auto-generated; `Cargo.lock` + `NOTICE`/`LICENSE` attached.
- Installer: port the reference `install` script (`reference/install`) for the port's own release asset URL; defer real end-to-end install until AG-17 wires `upgrade`/`uninstall` (RELEASE-003).

### 3.8 Offline / external-binary policy (RELEASE-005)
- Required external binaries today: `rg` (system or pinned-download 15.1.0), `git` (plugin install `oc-plugin/src/npm.rs:172`), `tar`/`unzip`/`powershell` (extraction), `bash`/`zsh` (shell tool).
- Policy:
  1. Prefer system binaries when present (already true for `rg`; `git`/shell always system).
  2. In-process extraction: replace `tar`/`unzip`/`powershell.exe` subprocesses with `tar` + `flate2` + `zip` crates in `oc-util/src/util/archive.rs` and `ripgrep/binary.rs::extract` — removes 3 external deps, fixes Windows parity (RELEASE-011-adjacent). All three are common, permissive, MSRV-clean.
  3. Pinned ripgrep download: verify SHA-256 against a checked-in pinned manifest before extraction; honor `OPENCODE_NO_DOWNLOAD` (env) — on air-gap/offline with no system `rg`, fail with a clear message listing fallbacks instead of a raw network error.
  4. Document in README: required external binaries per platform + air-gap runbook. Surface in `opencode debug info`/`doctor`.
  5. Coordinate with AG-17 (RELEASE-003 upgrade/install shares download/verify logic) and AG-15 (SUPPLY-001 QuickJS build needs `make`/gcc at build time — document as a build-time, not runtime, requirement; hermeticity improvement is AG-15's).

## 4. Clippy-remediation strategy — resolve, not suppress

Rule: real fix first; `#[allow]` only when the lint is semantically wrong for an intentional design, each with a justifying comment. Catalog from `artifacts/agent14-clippy.log`:

| Lint / error | Location | Fix |
|---|---|---|
| `manual_...`/needless range | `oc-util/src/ripgrep/mod.rs:289` | `rows.len() > input.limit` (also removes debug-overflow edge, RUST-011) |
| `unused_mut` | `oc-plugin/src/js/runtime.rs:806` | drop `mut` |
| `missing_transmute_annotations` | `runtime.rs:32` | `transmute::<*mut c_void, *mut JSRefCountHeader>` (sound; just annotations) |
| non-Send Arc | `runtime.rs:233` | deliberate `!Send` design (RUST-005); `#[allow(non_send_fields_in_send_ty)]` + comment — do not wrap, do not fake Send |
| `redundant_closure` | `runtime.rs:634` | use `JsError::InvalidString` variant directly |
| `collapsible_if` / `collapsible_match` / `single_match` / identical-blocks | `js/transpile.rs:464,556,767,774,793,814,862`; `shared.rs:105` | mechanical refactors preserving behavior; add unit coverage where behavior is subtle |
| `needless_borrow` | `transpile.rs:1130`; `npm.rs:270` | remove `&` |
| `result_large_err`/`large_enum_variant` | `oc-plugin/src/loader.rs:300`; `oc-schema/src/session_message.rs:340,419`; `v1/session.rs:913` | `Box` the large variants (serde-transparent; no serialization change, no parity break) |
| `type_complexity` | `oc-util/format/formatter.rs:52`; `ripgrep/mod.rs:125,160` | named type aliases |
| `derivable_impls` | `util/process.rs:25,45` | derive (`Default`) instead of manual impls |
| `manual_strip_prefix` | `util/proxy_env.rs:84` | `strip_prefix` |
| `assert_eq!(bool)` | `fs_util.rs:344` | `assert!` |
| `io_other_error` | `util/error.rs:419` | `io::Error::other` |
| `field_reassign_with_default` | `util/process.rs:568` | `..Default::default()` in the struct literal |
| `let_underscore_future` | `util/rpc.rs:275` | inside test — restructure to a binding + explicit drop, or local `#[allow]` (test-only, deliberate); prefer real restructure |
| `useless_vec` | `util/process.rs:553` | `&[...]` slice |
| `items_after_test_module` | `oc-plugin/src/npm.rs:192` | move tests into `#[cfg(test)] mod tests` at file end |

Boundary coordination: the sweep touches oc-plugin (owned by AG-15), oc-util (AG-09/AG-05), oc-schema (AG-01). Ship as a single workspace-wide cleanup commit ordered AFTER Wave-1 foundation and BEFORE the CI clippy gate is enforced, so `-D warnings` stays green. Any agent landing new code must run clippy green per the CI gate.

## 5. Test list

Unit:
- Logging: formatter key=value escaping; level filtering; run-id; append-mode file writer.
- Version: `REFERENCE_VERSION`/`PORT_VERSION`/`GIT_COMMIT` fallback; consistency across `oc-cli`/`oc-util`/`oc-tui`.
- Archive: tar.gz + zip extraction in-process (replaces subprocess tests); ripgrep pinned-manifest checksum mismatch → error + no write.
- Clippy regressions: one test per refactor where behavior was subtle (identical-blocks, collapsible-if).

Integration (binary-level, new — RELEASE-001/002/004):
- `opencode --version` == `1.18.13` (byte-parity); `debug info` contains port version + commit + profile.
- `opencode --print-logs --log-level DEBUG debug info` → non-empty stderr + `opencode.log` created/grown; `--log-level ERROR` suppresses INFO.
- `cargo build --locked` and `cargo build --release --locked` succeed (CI).
- Reproducibility: two clean release builds → identical sha256 (CI job).
- External-binary: with `OPENCODE_NO_DOWNLOAD` and no system `rg`, grep tool fails with the documented message (no network call).

CI/static:
- `cargo fmt --check`, `cargo clippy -D warnings` (all targets/features), `cargo test --workspace --all-features`, `cargo-deny check`, `cargo audit`, license/NOTICE presence + asset-coverage check.

## 6. Dependencies on other agents

- **panic=abort gating**: RUST-001/002/003 (AG-15), UX-002 (AG-16), ASYNC-002 + RUST-005 (AG-09). Keep `panic=unwind` until these merge; flip at Wave 5.
- **AG-15** (SUPPLY-001): shares the external-toolchain/build-hermeticity story; QuickJS upgrade will change `Cargo.lock` (fine, after lockfile is committed). Clippy sweep on oc-plugin must land without conflicting with AG-15's runtime.rs work — sequence sweep first.
- **AG-17** (RELEASE-003 upgrade/uninstall): consumes my shared version consts and the offline/download-verify policy; upgrade compares `REFERENCE_VERSION`/`PORT_VERSION`.
- **AG-13** (PROTO-002): version injection — coordinate the shared `oc-util::version` module early.
- **AG-10/11/12** (server/run wiring): consume the tracing init in `main.rs` so their runtime logs flow to `opencode.log`; they must call through the wired dispatch after my init lands.
- **AG-01** (ARCH/TEST-002): oc-schema enum boxing (clippy `large_enum_variant`) must not change serialization — coordinate the Box refactor with AG-01's schema ownership.
- **AG-18** (TEST-001 binary harness): my version/logging surfaces are part of the E2E assertions; land foundation before/with the harness.
- **AG-20** (PERF): release-profile change affects benchmark numbers — coordinate re-measure.

## 7. Risks

- `panic=abort` vs FFI `catch_unwind`: mutually exclusive with RUST-001's remediation. Must prove the trampoline panic-free or route through a panic hook; this is the #1 sequencing risk for the profile change.
- Shared `opencode.log` path with the reference binary: both append to `~/.local/share/opencode/log/opencode.log`; interleaving when both run. Keep parity (same path); document. Unbounded growth (reference has no rotation) — accept for parity, note follow-up.
- Version-string change risk: differential tests assert `--version` byte-equality; keep `--version` = `1.18.13`, expose metadata only in `debug info`/verbose.
- Editing verbatim embedded assets breaks prompt parity goldens — do NOT touch asset files; attribute via NOTICE/doc-comments.
- New deps (tracing-appender, tar, flate2, zip) enlarge supply surface — counter to SUPPLY goals; all four are ubiquitous/permissive; keep minimal and locked.
- Build-time `git` in `build.rs` fails under `cargo install`/vendored builds → env override + `"unknown"` fallback.
- Cross-compile aarch64 Linux + macOS/Windows E2E not verifiable on this host (audit BLOCKED) — CI matrix is the vehicle; expect iteration.
- macOS/Windows code signing blocked without certs — documented follow-up, not a blocker for unsigned distribution.
- Clippy sweep touches 3 agent-owned crates → merge conflicts; sequence as one dedicated commit, then enforce the gate.
- `serde_json preserve_order` (SUPPLY-006, intentional) is orthogonal but must be documented in the release notes — not changed here.

## 8. Merge-order recommendation

- **Wave 1 (foundations, small, low-risk, unblocks others):**
  1. Commit `Cargo.lock` + remove from `.gitignore` + `rust-toolchain.toml` (SUPPLY-002).
  2. `LICENSE` + `NOTICE` + attribution doc-comments + CI presence gate (SUPPLY-003).
  3. Logging subsystem in oc-util + init in `main.rs` (RELEASE-001).
  4. Shared `oc-util::version` module + build.rs commit hash (RELEASE-002/018).
  5. `[profile.release]` with `lto=thin`/`codegen-units=1`/`strip` — **panic stays `unwind`** (SUPPLY-004 partial).
  6. Clippy sweep (resolve-not-suppress) so the Lint gate flips PASS.
  Merge order within wave: lockfile+toolchain → LICENSE/NOTICE → version module → logging (log init can depend on version consts for run-id if desired, order flexible) → profile → clippy sweep.
- **Waves 2–4:** dependents (AG-10/11/12 wiring, AG-13/17 version+upgrade) consume logging/version/offline policy; panic-path fixes land (AG-15/16/09).
- **Wave 5 (CI/packaging, final):** full CI matrix (fmt/clippy/test/E2E/audit/deny/reproducibility) → `deny.toml` + SBOM/provenance → release pipeline + artifacts/checksums → in-process archive extraction + ripgrep checksum/`OPENCODE_NO_DOWNLOAD` policy → flip `panic = "abort"` LAST, after panic-path fixes verified, then re-measure with AG-20.

Wave 1 is self-contained and mergeable independently; nothing in CI/packaging (Wave 5) should block Wave 1's reproducibility/legal/logging wins.
