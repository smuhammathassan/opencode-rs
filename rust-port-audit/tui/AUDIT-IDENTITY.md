# TUI Audit Identity & Environment Baseline

- **Date / Timestamp:** 2026-08-17T07:42:36Z
- **Repository Under Audit:** `https://github.com/smuhammathassan/opencode-rs`
- **Canonical Reference Version:** OpenCode **v1.18.13** (`reference/packages/opencode/package.json`, `reference/packages/tui/package.json`)
- **Git Branch:** `main`
- **Git Commit SHA:** `d34e31d3ff077a94254c46f616335fb649e15967`
- **Working Tree Status:** `CLEAN`
- **CI Run (8/8 GREEN):** [`32006645082`](https://github.com/smuhammathassan/opencode-rs/actions/runs/32006645082)

## System & Toolchain Information

- **Rust Compiler:** `rustc 1.97.1` / 2021 edition
- **Cargo Version:** `cargo 1.97.1`
- **Host OS & Architecture:** macOS / Darwin (arm64)
- **Reference Runtime:** Node / Bun / OpenTUI (TypeScript) v1.18.13
- **Rust Monorepo TUI Crates:**
  - `crates/oc-tui`: Terminal User Interface application (Ratatui / Crossterm)
  - `crates/oc-cli`: CLI argument parsing, interactive/mini execution lifecycle
  - `crates/oc-server`: Axum HTTP/SSE server and `/tui/control` routes
  - `crates/oc-client`: HTTP/SSE client for remote and attached TUI
  - `crates/oc-session`: Session coordination, message and part storage
  - `crates/oc-config`: Multi-layer JSONC configuration discovery and resolution
  - `crates/oc-provider`: Provider registry, model catalog, and credential handling
  - `crates/oc-tool`: 17 agent tools, diff generation, and permission engine
- **Auditing Framework:** Zero-compromise drop-in compatibility audit against vendored reference OpenCode v1.18.13.

## CI Verification Matrix (Machine-Verifiable)

| Job | Platform | Result | Duration |
|-----|----------|--------|----------|
| `fmt` (includes audit integrity) | ubuntu-latest | ✅ PASS | 22s |
| `clippy (-D warnings)` | ubuntu-latest | ✅ PASS | 57s |
| `build` | ubuntu-latest | ✅ PASS | 1m08s |
| `build` | macos-latest | ✅ PASS | 1m18s |
| `build` | windows-latest | ✅ PASS | 2m39s |
| `test` | ubuntu-latest | ✅ PASS | 2m16s |
| `test` | macos-latest | ✅ PASS | 3m44s |
| `test` | windows-latest | ✅ PASS | 4m22s |

**Verification:** `gh run view 32006645082` → `✓ main CI · 32006645082`
