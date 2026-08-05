# Agent 16 — Dependencies, Supply Chain, Licensing, and Build Reproducibility

## Scope

Audit of the opencode-rs Rust port (20 crates under `crates/oc-*`) covering: direct and transitive
dependencies; duplicate versions; yanked crates; known vulnerabilities (manual); git/path deps;
vendored native code (QuickJS provenance); build scripts; proc macros; C/C++ compilation; platform
libraries; TLS stack and certificate validation; crypto provider ownership; feature activation and
unification; dependency provenance; lockfile integrity and reproducibility; offline builds;
checksums; license compatibility against the MIT reference; attribution/notice obligations;
embedded reference content (prompts, model registry, plugin polyfill); dependency-confusion risk;
network access during build; release-profile hardening; panic strategy and debug symbols.

Read-only audit. Tools `cargo-audit` and `cargo-deny` are **not installed** and were **not
installed**; vulnerability checks are therefore manual and marked UNVERIFIED where a live CVE DB was
unavailable. crates.io sparse index was reachable and used for yank/provenance checks.

## Repository areas inspected

- `/root/opencode-rs/Cargo.toml` — workspace manifest, `[workspace.dependencies]`, no `[profile.*]`
- `/root/opencode-rs/Cargo.lock` — 339 packages (319 registry + 20 path)
- `/root/opencode-rs/.gitignore` — line 3 ignores `Cargo.lock`
- `/root/opencode-rs/.cargo/config.toml` — `target-dir = "target"` only
- `crates/oc-plugin/Cargo.toml` — `libquickjs-sys = { version = "0.1" }` (line 13)
- `crates/oc-database/Cargo.toml` — `rusqlite` with `bundled`, `load_extension`, `serialize`
- `crates/oc-provider/src/models_dev.rs` + `crates/oc-provider/data/models.json`
- `crates/oc-plugin/src/polyfill/runtime.js`, `crates/oc-session/assets/prompt/*.txt`,
  `crates/oc-tool/src/prompts/*.txt`
- `~/.cargo/registry/src/*/libquickjs-sys-0.1.0/` (build.rs, embed/quickjs/VERSION, Cargo.toml.orig,
  .cargo_vcs_info.json)
- `reference/LICENSE` and `reference/packages/opencode/src/session/prompt/*.txt` (MIT spec)

## Commands executed

```
cargo tree                     -> rust-port-audit/artifacts/16-cargo-tree.txt
cargo tree -d                  -> rust-port-audit/artifacts/16-cargo-tree-duplicates.txt
cargo metadata --format-version 1 -> rust-port-audit/artifacts/16-cargo-metadata.json
cargo tree --offline -p oc-cli -> exit 0 (deps fully cached; offline resolution OK)
git ls-files | grep Cargo.lock -> 0 files (NOT committed; .gitignore:3)
grep -rn '\[profile' --include=Cargo.toml . -> none
find crates -name build.rs     -> none in the 20 port crates
crates.io sparse index queries -> yank + version-provenance checks for all 319 registry crates
```
Artifacts: `rust-port-audit/artifacts/16-*` (tree, duplicates, metadata, licenses, direct-deps,
yanked-check, libquickjs-sys build.rs, QuickJS VERSION).

## Runtime scenarios attempted

- `cargo tree --offline` — success (cache warm). A fresh offline build on a machine without the
  registry cache would fail because no lockfile and no vendored sources exist.
- Live crates.io yank verification of every locked registry version — 319/319 checked, 0 yanked.
- `cargo-audit`/`cargo-deny` — BLOCKED (not installed; not installed per HARD RULES).

## Architecture or behavior summary

- 339 packages in `Cargo.lock`: 20 workspace path crates + 319 crates.io registry crates; **0 git
  dependencies**; no `[patch]`/`[replace]` sections anywhere. Checksums present for all 319 registry
  packages (`Cargo.lock` entries all carry `checksum =`).
- Native C compilation at build time is pulled in by four registry deps: `libquickjs-sys`
  (QuickJS via `make`), `libsqlite3-sys` (SQLite 3.46.0 via `cc`), `ring` (assembly via `cc`),
  `signal-hook` (via `cc`). The port's own 20 crates have **no** `build.rs` (no workspace build
  scripts, none environment-sensitive).
- TLS: `reqwest` → `hyper-rustls` → `rustls 0.23.43` (features: ring, std, tls12; no aws-lc, no
  logging) + `webpki-roots 1.0.9` (Mozilla roots). Crypto provider is **`ring 0.17.14`**
  (briansmith), not aws-lc. No `danger_accept_invalid_certs` / custom verifier anywhere in the
  port source — certificate validation is ON by default.
- `serde_json` is unified with `preserve_order` + `indexmap` (activated in oc-core/Cargo.toml:10,
  oc-llm/Cargo.toml:10, oc-config/Cargo.toml:10) — intentional for reference insertion-order
  parity (commit fd99c06) but it changes serialization ordering of **every** JSON object in the
  binary and HTTP API.
- `tokio` is always used via `tokio = "full"` (workspace dep) — all features are always on.
- Plugin engine: `libquickjs-sys 0.1.0` (registry, from theduke/quickjs-rs, git sha a3f3f06) which
  bundles QuickJS **2019-07-21** and compiles it with `make libquickjs.a` at build time.
- Port embeds verbatim reference content: system prompts
  (`oc-session/assets/prompt/*.txt` == `reference/.../session/prompt/*.txt`, diff-verified for
  `default.txt`), the 3.5 MB model catalog (`oc-provider/data/models.json`), and a plugin polyfill
  (`oc-plugin/src/polyfill/runtime.js`, 1165 lines).

## Positive observations

1. **No yanked crates** — all 319 locked registry versions checked against the live crates.io
   sparse index (310 + 9 short-name entries), zero yanked.
2. **No git dependencies** — the entire graph resolves from crates.io only; no unpinned/unattributable
   revisions, no `[patch]`/`[replace]` shenanigans. `Cargo.lock` has checksums for all registry
   packages.
3. **TLS is sound** — rustls + ring + Mozilla roots, minimal feature set, default cert validation
   intact; no disabled-verification path found in source.
4. **License posture of the dependency set is clean** — all 319 crates are permissive
   (MIT/Apache-2.0/ISC/BSD/Unicode-3.0/Zlib/MPL-2.0/CDLA-Permissive). No GPL/AGPL. Only
   `option-ext` (MPL-2.0) is a weak copyleft (file-level; MIT-compatible combination). `subtle`
   (BSD-3-Clause) and `ring` (Apache-2.0 AND ISC) are standard crypto-adjacent licenses.
5. **Unsafe is confined** — the only significant `unsafe` is in `oc-plugin/src/js/runtime.rs`
   (51 sites, documented QuickJS FFI wrapper) plus 1 in `oc-database/src/sqlite.rs` and 2 in
   `oc-util/src/util/process.rs`. No scattered unsafe.
6. **No build.rs in the port** — 20 workspace crates have no build scripts; nothing environment- or
   network-sensitive runs at build time in the port itself.
7. Lockfile is internally consistent and the workspace builds offline from the warm cache
   (`cargo tree --offline` exit 0).

## Findings summary

| ID | Severity | Confidence | Title |
|----|----------|------------|-------|
| SUPPLY-001 | High | CONFIRMED | QuickJS engine is 7 years old (2019-07-21), unmaintained upstream, executes untrusted plugin JS |
| SUPPLY-002 | High | CONFIRMED | `Cargo.lock` is gitignored/not committed — non-reproducible, unpinned builds |
| SUPPLY-003 | High | CONFIRMED | No LICENSE/attribution in repo; verbatim MIT reference content redistributed without notice |
| SUPPLY-004 | Medium | CONFIRMED | No `[profile.release]` hardening; binary un-stripped, no LTO, panic=unwind |
| SUPPLY-005 | Medium | CONFIRMED | Build requires external C toolchain (`make`, gcc, ar) via libquickjs-sys build script |
| SUPPLY-006 | Medium | CONFIRMED | Feature-activation sprawl: tokio `full`, reqwest blocking+multipart, serde_json preserve_order unification |
| SUPPLY-007 | Medium | CONFIRMED | Embedded model catalog (`models.json`, 3.5 MB) pulled from live URL, not the vendored reference; drift + provenance risk |
| SUPPLY-008 | Low | CONFIRMED | Duplicate versions across the graph (axum 0.7/0.8, base64 0.22/0.23, thiserror 1/2, rustix 0.38/1.1, syn 2/3, …) |
| SUPPLY-009 | Low | CONFIRMED | `serde_yml`/`libyml` 0.0.x are a community fork of the deprecated serde_yaml (different author) |
| SUPPLY-010 | Informational | CONFIRMED | Dead workspace dep `quick-js = "0.1"` declared but unused (oc-plugin uses libquickjs-sys directly) |

## Detailed findings

### SUPPLY-001 — QuickJS engine is 7 years old and unmaintained (High, CONFIRMED)

- `crates/oc-plugin/Cargo.toml:13` pins `libquickjs-sys = { version = "0.1", default-features = true }`;
  semver `0.1` resolves to `libquickjs-sys 0.1.0` (the only 0.1.x). This crate embeds QuickJS
  **2019-07-21** (`artifacts/16-quickjs-VERSION.txt`; header `* Copyright (c) 2017-2019 Fabrice
  Bellard`).
- `build.rs` (artifact 16-libquickjs-sys-build.rs) copies `embed/quickjs` and runs
  `make libquickjs.a` — so the engine that executes third-party plugin JavaScript is a snapshot of
  Bellard's QuickJS from July 2019. Upstream has released numerous security fixes since; the crate
  author's own newer releases (0.2.0–0.10.0, 0.4+ moving to maintained QuickJS-ng) all exist on
  crates.io but are not used. **STATIC proof:** crate metadata + VERSION file + `Cargo.lock`
  (`libquickjs-sys 0.1.0`). Specific CVE numbers are NOT asserted — cargo-audit unavailable
  (UNVERIFIED); the finding rests on 7 years of upstream-version obsolescence of a memory-unsafe C
  JS engine that executes untrusted plugin code (plugins fetched from npm at runtime,
  `oc-plugin/src/npm.rs:29-39`).
- Remediation: move to `libquickjs-sys >= 0.4` (QuickJS-ng) or an actively maintained binding;
  at minimum pin a newer revision and document the engine version in the binary.

### SUPPLY-002 — `Cargo.lock` not committed; builds are not reproducible (High, CONFIRMED)

- `.gitignore:3` ignores `Cargo.lock`; `git ls-files | grep Cargo.lock` → **0 files**. The lockfile
  exists on disk (339 packages) but is untracked.
- Consequences (STATIC): (a) every fresh checkout re-resolves all 319 registry crates to latest
  semver-compatible versions → transitive supply chain can drift silently between builds;
  (b) offline builds on a clean machine are impossible (no lockfile + no vendored/cached sources);
  (c) binary reproducibility cannot be claimed.
- Remediation: commit `Cargo.lock`; consider `.cargo/config.toml` offline/vendored registry or
  `cargo vendor`.

### SUPPLY-003 — License/attribution non-compliance with the MIT reference (High, CONFIRMED)

- Reference is MIT: `reference/LICENSE` — “MIT License, Copyright (c) 2025 opencode”.
- Port: workspace declares `license = "MIT"` (`Cargo.toml:29`) but the repository contains **no
  LICENSE file** and `grep Copyright` across `crates/` finds nothing. MIT requires the copyright
  notice and permission text be included in all copies/substantial portions.
- The port embeds **verbatim** reference content: `oc-session/assets/prompt/*.txt` (diff-verified
  identical header for `default.txt`), `oc-tool/src/prompts/*.txt`, the plugin polyfill
  `oc-plugin/src/polyfill/runtime.js` (derived API surface of `@opencode-ai/plugin`), and the model
  catalog. No provenance header or notice accompanies any of these.
- Verdict: redistribution of this derivative work currently omits the required MIT notice.
- Remediation: add `LICENSE` (reference MIT text) at repo root and attribute the reference in
  crate doc-comments/headers; per-file SPDX headers for embedded prompts/polyfill.

### SUPPLY-004 — No release-profile hardening (Medium, CONFIRMED)

- `grep -rn '\[profile' --include=Cargo.toml` across the workspace → none (root `Cargo.toml` ends
  at line 51). Release uses Cargo defaults: `opt-level=3`, `codegen-units=16`, `lto=false`,
  `panic=unwind`, `strip="none"`.
- STATIC proof: `file target/release/opencode` → “not stripped”; 8,054,560 bytes. Given the project’s
  stated goal (small single binary, commit e7fc33e “23x smaller binary”), `lto="thin"|"fat"`,
  `codegen-units=1`, `strip=true` and a deliberate `panic` strategy are missing wins and currently
  vary the shipped artifact.
- Debug symbols are absent (default release `debug=0`) — good; panic strategy is default unwind
  (no abort; unwind is acceptable for a CLI but increases code size).

### SUPPLY-005 — Build depends on external C toolchain via make subprocess (Medium, CONFIRMED)

- `libquickjs-sys` build.rs: `copy_dir::copy_dir("./embed/quickjs", $OUT_DIR/quickjs)` then
  `std::process::Command::new("make").arg("libquickjs.a")` (artifact 16-libquickjs-sys-build.rs) —
  requires `make`, `ar`, and a C compiler on the build machine; `make`/`gcc` confirmed present here.
- `libsqlite3-sys 0.30.1` compiles bundled SQLite 3.46.0 via `cc` (feature `bundled` active;
  `oc-database/Cargo.toml`), `ring` compiles asm via `cc`, `signal-hook` via `cc`. None of these
  require network, but the build is not hermetic across machines/toolchains. Note the crate-level
  build scripts run with the full environment (standard risk; no secrets are supplied by this repo).

### SUPPLY-006 — Feature-activation sprawl (Medium, CONFIRMED)

- `tokio = { version = "1", features = ["full"] }` (`Cargo.toml:39`) → every crate gets the full
  tokio surface (io-util, net, fs, process, signal, rt-multi-thread, time, sync, macros…).
  Resolved features confirm the full set.
- `reqwest` activates `json`, `multipart`, `stream`, `rustls-tls`, plus `blocking` from
  `oc-plugin/Cargo.toml:12` — blocking + async client both in the binary.
- `serde_json` is unified with `preserve_order`+`indexmap`+`raw_value` (features confirmed in
  metadata) → every `serde_json::to_string` in the process is insertion-ordered, including HTTP API
  payloads. Intentional for parity (commit fd99c06) but a global behavioral/feature decision that
  deserves documentation and review.
- Duplicate-version sprawl is separately tracked (SUPPLY-008).

### SUPPLY-007 — Embedded model catalog provenance/drift (Medium, CONFIRMED)

- `crates/oc-provider/src/models_dev.rs:1-16` documents that the reference fetches
  `https://models.opencode.ai/api.json` at runtime, and that the port embeds a snapshot as
  `crates/oc-provider/data/models.json` (3.5 MB, 180 providers) “regenerated from the same URL”.
  This is **not** a copy of the vendored reference; a `TODO(integration)` notes it must be
  regenerated at release. Risks: (a) catalog drift vs the reference build snapshot; (b) the data’s
  license/provenance is unattributed (no notice); (c) the runtime refresh + TTL + flock flow is not
  ported, so the embedded snapshot can go stale in a long-running install.

### SUPPLY-008 — Duplicate versions (Low, CONFIRMED)

From `16-cargo-tree-duplicates.txt` (runtime-relevant duplicates; this is a workspace of 20 crates
so some duplication is expected):
- `axum 0.7.9` (oc-server prod) vs `axum 0.8.9` (oc-client dev-dep) — a prod/dev split that builds
  two full axum stacks into test binaries; prod ships 0.7.9.
- `base64 0.22.1` (14 crates) vs `0.23.1` (oc-cli only).
- `thiserror 1.0.69` (tungstenite) vs `2.0.19` (everything else).
- `rustix 0.38.44` (crossterm) vs `1.1.4` (tempfile/xattr); `linux-raw-sys 0.4.15` vs `0.12.1`.
- `syn 2.0.119` vs `3.0.3` (both active); `hashbrown 0.14.5/0.15.5/0.17.1`;
  `getrandom 0.2.17`/`0.4.3`; `unicode-width 0.1.14`/`0.2.0`; `matchit 0.7.3`/`0.8.4`.
All are legitimate transitive pins; none indicate a supply-chain problem, but `axum` and `base64`
could be unified with small effort.

### SUPPLY-009 — serde_yml/libyml are a fork, not the canonical serde_yaml (Low, CONFIRMED)

- `serde_yml 0.0.12` (github.com/sebastienrousseau/serde_yml) + `libyml 0.0.5` are a community
  continuation of the deprecated `serde_yaml` (dtolnay). Both are MIT/Apache-2.0. Early `0.0.x`
  versions of a fresh fork should be reviewed on upgrade (API/perf churn, low release maturity).
  Not a defect in itself; a provenance note.

### SUPPLY-010 — Dead workspace dependency (Informational, CONFIRMED)

- `Cargo.toml:51` declares `quick-js = "0.1"` in `[workspace.dependencies]` but no crate depends on
  it (not present in `cargo tree`). `quick-js` is a separate, unmaintained wrapper around the same
  `libquickjs-sys` — the declaration invites confusion about the actual engine binding
  (`libquickjs-sys` used directly in `oc-plugin`). Recommend removing or wiring it.

## Feature or behavior gaps

- No committed lockfile / no vendor dir / no `--offline` policy → supply-chain drift + clean-machine
  offline builds fail (SUPPLY-002).
- No `[profile.release]` — hardening decisions (LTO/strip/panic) are absent (SUPPLY-004).
- Runtime model-catalog refresh (fetch + TTL cache) is not ported (SUPPLY-007).
- No `cargo deny`/`cargo audit` policy, no `.cargo` registry restrictions, no toolchain pin file
  (`rust-toolchain` absent — MSRV 1.97 from the environment, unspecified in repo).

## Test coverage gaps

- No dependency/security CI (cargo-deny `advisories`/`bans`/`sources`, cargo-audit) — cannot be
  confirmed from the repo.
- No license-compliance test that would catch the missing attribution/notice files.
- No offline/reproducible-build test (e.g., `cargo build --offline --locked` in CI).
- No golden test pinning the embedded `models.json` to a reference snapshot (drift risk).
- QuickJS engine version is not asserted anywhere (no test that fails when the engine is upgraded,
  which is good for upgrades but means the obsolete engine is silently shipped).

## Unverified areas

- Known-CVE list for QuickJS 2019-07-21 and other locked versions: **UNVERIFIED** — cargo-audit /
  cargo-deny not installed (per HARD RULES); manual memory-based CVE knowledge deliberately not
  asserted. The finding stands on version obsolescence + upstream unmaintained status, not on a
  CVE DB lookup.
- Whether a truly clean-machine build succeeds (no cache): **UNVERIFIED** here; inferred to fail for
  offline builds because no lockfile/vendor exists.
- models.opencode.ai data license (attribution terms of the model catalog): **UNVERIFIED** (URL not
  fetched).
- Windows/macOS build behavior of `libquickjs-sys` (`make`) and other platform libraries: not
  exercised (Linux only).

## Final domain verdict

**READY_WITH_MINOR_REMEDIATION**

Build is functional and dependency hygiene is generally strong (0 yanked, 0 git deps, clean TLS,
permissive-only licenses, confined unsafe, no workspace build scripts). However, three High items
must be remediated before this port is distributed as a public artifact: (1) commit `Cargo.lock`
and enable `--locked`/offline builds; (2) add the MIT `LICENSE` and reference attribution for the
verbatim embedded content (prompts, polyfill, model catalog); (3) replace or pin a current QuickJS
binding — shipping a 2019 JS engine that executes untrusted third-party plugin code is a
security-relevant supply-chain liability. With those plus the release-profile and model-catalog
remediation, the domain is clean.
