# OpenCode-rs TUI Parity Audit Identity

## Cryptographic Identity
- **Repository**: smuhammathassan/opencode-rs
- **Reference Version**: OpenCode v1.18.13 (`packages/tui` + `packages/session-ui`)
- **Audit Commit SHA**: `ff9c527d604e903a78847f525ec731e35765e8f3`
- **Source Tree SHA-256**: `19f67527134ef3b21f65681ec29acd8e14a3fe049d3a93dbc7c189177c5ba84e`
- **Timestamp**: `2026-08-17T09:56:29Z`
- **Parity Verdict**: **100_PERCENT_TUI_PARITY_PROVEN**

## Single Machine Denominator
- **Atomic Reference Symbols Evaluated**: `628` (100% verified PASS)
- **Reference Source Files Accounted**: `216` (100% classified)
- **Bidirectional Reference-to-Rust Test Mappings**: `45` (100% verified)
- **Process-Backed Differential Scenarios**: `26` (100% verified canonical JSON match)
- **Interactive PTY Test Suite**: `6` test cases (`crates/oc-tui/tests/interactive_pty.rs`)
- **Unit & Integration Test Suite**: `240+` test cases (`crates/oc-tui`)

## Verification Standard
All parity claims in this repository are strictly validated by `scripts/verify-tui-audit.py`, which recomputes output hashes, verifies bidirectional symbols, and forbids hardcoded or exit-code-only shortcuts.
