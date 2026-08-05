# 00 — Remediation Baseline

## Repository state (pre-change)

- **Audited commit**: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c`
- **Current HEAD (baseline)**: `90727e19860b8e0c1b0cf6469b696ef3b3efaeb1`
  - HEAD supersedes the audited commit by adding `rust-port-audit/**` (audit artifacts only). No production source changed between audit and baseline.
- **Branch**: `main` (will create `fix/audit-remediation`)
- **Remote**: `origin` = https://github.com/smuhammathassan/opencode-rs.git
- **Working tree**: CLEAN (0 changes) before remediation begins
- **Reference spec**: vendored opencode v1.18.13 (TypeScript/Bun) at `reference/` — read-only
- **Reference binary (oracle)**: `/root/.opencode/bin/opencode` (reports 1.18.13)

## Audit inputs (authoritative, preserved unchanged)

- `rust-port-audit/AUDIT-SUMMARY.md` — verdict NOT_READY_FOR_PRODUCTION
- `rust-port-audit/RELEASE-GATE.md` — 6/25 gates PASS; 11 FAIL; 2 PARTIAL; 5 NOT_TESTED
- `rust-port-audit/FINDINGS.json` — 65 consolidated findings (8 Critical, 26 High, 20 Medium, 8 Low, 3 Informational); 33 release blockers
- `rust-port-audit/FEATURE-PARITY.csv` — 155 rows; 20 IMPLEMENTED_CONNECTED, 61 DISCONNECTED, 32 PARTIAL, 34 STUB, 6 MISSING
- `rust-port-audit/COMMAND-COMPATIBILITY.csv` — 148 rows; 11.5% equivalent
- `rust-port-audit/TEST-EVIDENCE.md` — 1519 tests pass; zero binary/E2E coverage
- `rust-port-audit/01-architecture-modularity.md` … `20-packaging-release-operations.md`

## Root causes (from audit)

1. **Zero production cross-crate integration** (INTEGRATION-001): `use oc_*` = 0 in production source; 266 `TODO(integration)` markers; every crate re-implements its declared deps via local mirror types.
2. **No composition root**: `opencode run`/`serve`/TUI/session/export/import/db/mcp/acp all return "not yet wired"; executable reaches no domain crate.
3. **Security primitives absent**: permission gate record-only (SEC-001); PTY ticket unvalidated (SEC-002); file containment dropped (SEC-003).
4. **Latent safety defects** in plugin (FFI/limits/containment), tools (OOM/symlink/process-group), async (lost-wakeup/nested runtime/buffered streaming).
5. **Process/engineering gaps**: Cargo.lock untracked, no LICENSE, no logging, no CI, unfair benchmarks, no binary E2E.

## Execution model

- 20 named workstreams (Agents 01–20), one per audit domain / remediation area.
- **Wave 0 (this phase)**: concurrent READ-ONLY planning — every agent produces an ownership/impact plan. No source modified.
- Waves 1–5: implementation in controlled dependency order with security-first merge gating (see `02-DEPENDENCY-WAVES.md`).
- All remediation evidence under `rust-port-remediation/**`; `rust-port-audit/**` unchanged.

## Baseline quality checks (recorded, not yet run at Wave 0)

- `cargo test --workspace`: 1519 passed (baseline)
- `cargo fmt --check`: PASS
- `cargo clippy -- -D warnings`: FAIL (45 errors)
- `cargo build --workspace`: PASS
