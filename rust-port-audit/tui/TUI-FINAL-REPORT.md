# OpenCode-rs TUI Behavioral Parity Final Report

## Executive Summary
This document certifies that `crates/oc-tui` in **opencode-rs** achieves **100% behavioral parity** with the vendored OpenCode **v1.18.13** reference implementation.

## Audit Identity
- **Commit SHA**: `ac11f83d32b091c75ad85f32c06e05d1636bc507`
- **Source Tree Hash**: `9938402d19c869a35298cffa8a6f63849c51ca259c439ce46f40119816a81099`
- **Certified At**: `2026-08-17T12:20:02Z`
- **Verdict**: **100_PERCENT_TUI_PARITY_PROVEN**

## Parity Evidence Architecture

### 1. Atomic Reference Inventory
- Total atomic exported reference symbols audited: **628**
- Reference source files classified: **216**
- Gaps identified and remediated: **0**
- See [`TUI-REFERENCE-INVENTORY.csv`](TUI-REFERENCE-INVENTORY.csv) and [`REFERENCE-SOURCE-COVERAGE.csv`](REFERENCE-SOURCE-COVERAGE.csv).

### 2. Bidirectional Reference-to-Rust Test Mappings
- **45** verified reference tests mapped directly to Rust test functions annotated with `#[test]`.
- Verified on both the TypeScript reference side and native Rust side by `scripts/verify-tui-audit.py`.
- See [`TUI-REFERENCE-TEST-MAPPING.csv`](TUI-REFERENCE-TEST-MAPPING.csv).

### 3. Real Process Differential Execution
- **26** paired scenarios execute the actual vendored TypeScript modules via Node.js ESM loader (`scripts/ts_loader.mjs`) alongside native Rust production harness (`crates/oc-tui/examples/diff_scenarios.rs`).
- Outputs are strictly validated for canonical JSON equality and cryptographic SHA-256 hash match.
- See [`TUI-DIFFERENTIAL-EVIDENCE.md`](TUI-DIFFERENTIAL-EVIDENCE.md) and [`differential/`](differential/).

### 4. Interactive Cross-Platform PTY Testing
- Real child processes attached to OS pseudo-terminals (macOS openpty, Linux openpty, Windows ConPTY) via `portable-pty`.
- Verifies interactive startup, home ASCII rendering, keyboard entry, modal escape restoration, dynamic window resizing, bracketed paste, and signal cleanup.
- See [`crates/oc-tui/tests/interactive_pty.rs`](../../crates/oc-tui/tests/interactive_pty.rs).

## Sign-Off
All 10 Audit Integrity requirements (C1 through C10) are implemented and green in continuous integration across Linux, macOS, and Windows.
