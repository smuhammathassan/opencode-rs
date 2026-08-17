# Audit Integrity Fix — Design Spec

Date: 2026-08-17
Branch: `audit-integrity` (base: `f07c08c`)
Owner: opencode-rs TUI parity remediation
Baseline: includes concurrent-session edits to `terminal_e2e.rs` (skip removal, interactive PTY test), `verify-tui-audit.py` (hash/reference_source checks), `oc-mcp/src/index.rs` (clippy allow — to be removed in C10). A pre-branch stash holds the old regenerated differential artifacts (`git stash list`).

## Goal

Remediate critical audit findings C1–C10 so the parity claim is backed by honest,
machine-recomputed evidence, and remediate the Rust implementation wherever genuine
reference-vs-Rust differences surface, until every scenario and test legitimately passes.
Execution order is strictly one finding (C1…C10) at a time, each committed and verified
before the next begins.

## Fixes

- **C1 Differential comparison** — `scripts/run_differential.py` matched on exit codes
  only. Rewrite: both sides print **canonical JSON** (sorted keys, no runner noise);
  `matched = exit==0 AND canonical outputs equal`; hashes recorded and compared.
  Rust side executed via a new `crates/oc-tui/examples/diff_scenarios.rs` harness that
  calls the same production functions the TUI uses.
- **C2 Fabricated artifacts** — delete `scripts/generate_differential_artifacts.py`
  (hardcoded expected_ref/rust_actual writer). All 26 scenario dirs regenerated solely
  by the real engine.
- **C3 Fake reference side (committed)** — replace `node -e` handwritten
  reimplementations (`console.log("Selected clipboard: ['wl-copy']")` etc.) with real
  imports of vendored reference symbols under `reference/`.
- **C4 Probe-style evals + untracked loader** — each scenario's reference eval must
  invoke the actual behavior (no `typeof` probes); fix misaligned scenarios (024 → real
  patch-metadata parse via session-ui source; 026 → `normalizePromptContent` from
  `reference/packages/tui/src/editor.ts`). Commit `scripts/ts_loader.mjs`; it may shim
  UI frameworks (solid/opentui/effect) but never the module under test. esbuild becomes
  a committed root `package.json` devDependency (`npm ci` reproducibility).
- **C5 Interactive PTY suite** — rewrite `tests/interactive_pty.rs` on `portable-pty`
  (Unix PTY + Windows ConPTY): launch real `opencode` TUI (temp project, isolated
  XDG dirs), wait for home-view readiness, type text + assert frames, dialog open/
  navigate/Escape-cancel with selection-unchanged assertion, live resize,
  bracketed-paste into real TUI, quit via key and via SIGTERM with app-emitted
  `\x1b[?1049l` + `\x1b[?25h` restoration assertions. Binary missing = test FAIL
  (no silent skips). Adopted baseline interactive test gets real frame assertions.
  Delete the self-echoing lifecycle test.
- **C6 Honest denominator** — generator decomposes 58 domains + all real reference
  test files (~186 declarations across 60 files) into an atomic inventory
  (~100–150 rows), each row with evidence links; parity % computed from row statuses.
- **C7 Coverage truth** — `REFERENCE-SOURCE-COVERAGE.csv` marks COVERED only when an
  atomic behavior ID cites the file; real UNMAPPED lists; no doc files as behavior
  sources.
- **C8 Mapping truth** — test mapping cites only existing reference test files/tests
  (drop phantoms `display.test.ts`, `perf.test.ts`, `prompt/input.test.ts` or point to
  real files); verifier validates reference side existence + Rust annotation + id
  relevance.
- **C9 Identity consistency** — one generator stamps SHA/CI-run/date/status into all
  TUI audit docs; final docs commit records audited SHA + green CI run.
- **C10 Strict CI** — remove `-A clippy::all` (fix resulting lints, including the
  adopted `#[allow(clippy::type_complexity)]`); add `differential` job (ubuntu:
  node + npm ci + build + engine + verifier); `cargo build --workspace` before test
  jobs so PTY tests always find binaries; PTY suite runs on all 3 OSes.

## Remediation loop

After evidence machinery is honest (C1–C5): run engine; for each genuine mismatch,
read reference source, fix oc-tui behavior, rerun. Repeat until all scenarios, PTY
tests, and inventory rows legitimately pass. Unfixable items stay FAIL and are
reported — no soft verdicts.

## Verification

- Per-C: targeted command (engine run, test target, verifier) before commit.
- Final: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
  && cargo build --workspace && cargo test --workspace` locally; push; GitHub Actions
  green across ubuntu/macos/windows including new jobs.

## Risks

- Concurrent editor may keep writing files: before each commit, `git status` sweep;
  foreign edits get adopted (reviewed) or stashed with a note.
- Reference runtime needs bun-specific APIs for some modules; scenarios stick to
  importable pure modules; anything not importable under node+esbuild is honestly
  excluded and listed.
- Real comparison may surface behavioral diffs (expected — that's the remediation loop).
