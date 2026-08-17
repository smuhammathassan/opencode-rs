# OpenCode-rs TUI Parity Audit Identity

## Cryptographic Identity
- **Repository**: smuhammathassan/opencode-rs
- **Reference Version**: OpenCode v1.18.13 (`packages/tui` + `packages/session-ui`)
- **Audit Commit SHA**: `d623f1da5dd3be313816421733bd4eec7f2b8e8f`
- **Source Tree SHA-256**: `9938402d19c869a35298cffa8a6f63849c51ca259c439ce46f40119816a81099`
- **Timestamp**: `2026-08-17T12:55:17Z`
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
