# Audit Integrity Fix (C1–C10) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fabricated/exit-code-only parity evidence with honest, machine-recomputed evidence and remediate genuine Rust-vs-reference differences, one critical finding (C1…C10) at a time.

**Architecture:** A differential engine executes real vendored reference TS modules (node + committed esbuild loader) and real Rust production functions (cargo example harness), comparing canonical JSON. An interactive PTY suite drives the real `opencode` TUI via `portable-pty`. Generators rebuild all audit inventories from real repo state; a hardened verifier recomputes hashes and comparisons; CI runs everything strictly.

**Tech Stack:** Python 3 (engine/verifier/generators), node + esbuild (reference execution), Rust/cargo (workspace, new dev-dep `portable-pty`), GitHub Actions.

**Conventions:** repo root = `/Users/muhammadhassan/Documents/Codex/2026-08-15/smuhammathassan-opencode-rs-https-github-com/work/opencode-rs` (below shown as `<root>`). Branch `audit-integrity`. Before every commit run `git status --short` — if files changed that you did not edit (concurrent session), stop and reconcile (review/adopt or stash) before committing. Never edit anything under `reference/`.

**Canonical JSON rule (used everywhere):** `json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)` — both sides must emit exactly this.

---

### Task 1 (C1): Real output comparison in differential engine

**Files:**
- Create: `crates/oc-tui/examples/diff_scenarios.rs`
- Modify: `scripts/run_differential.py` (full rewrite of match logic + scenario evals that already use real imports)
- Test: manual tamper-test below

- [ ] **Step 1: Write the Rust scenario harness** — an example binary that runs the SAME production functions the TUI uses and prints one canonical-JSON line per scenario. Registry pattern (complete file skeleton with first scenarios; add remaining per table in Step 2):

```rust
//! Differential scenario harness: executes production oc-tui functions and
//! prints canonical JSON `{"scenario": ..., "result": ...}` per scenario id.
use std::collections::BTreeMap;

fn canon(id: &str, result: serde_json::Value) {
    let mut m = BTreeMap::new();
    m.insert("scenario", serde_json::json!(id));
    m.insert("result", result);
    println!("{}", serde_json::to_string(&m).unwrap());
}

fn main() {
    let id = std::env::args().nth(1).expect("scenario id required");
    match id.as_str() {
        "013-format-duration-under-minute" => canon(&id, serde_json::json!({
            "cases": [
                oc_tui::util::format::format_duration(0),
                oc_tui::util::format::format_duration(1),
                oc_tui::util::format::format_duration(45),
                oc_tui::util::format::format_duration(59),
            ]
        })),
        // ... remaining arms per Step 2 table ...
        _ => { eprintln!("unknown scenario {id}"); std::process::exit(2); }
    }
}
```

- [ ] **Step 2: Enumerate all scenarios (both sides must produce identical canonical JSON):**

| id | Reference symbol (real import) | Rust production fn | JSON payload |
|---|---|---|---|
| 001-prompt-history-parse | `prompt/history.tsx parsePromptHistory` (mixed valid/corrupt/cap) | `parse_prompt_history` | parsed entries (input/text arrays), len, cap behavior |
| 002-prompt-history-dedup | `prompt/history.tsx isDuplicateEntry` | `is_duplicate_entry` | 3 boolean cases + parts-differ case |
| 003-prompt-paste-placeholders | `prompt/part.ts expandPastedTextPlaceholders` | `expand_text_parts` | expanded strings for 2 cases |
| 004-prompt-stash | `prompt/stash.tsx parsePromptStash + MAX_STASH_ENTRIES` | `parse_prompt_stash`/consts | entries + max |
| 005-keymap-leader | `keymap.tsx LEADER_TOKEN, OPENCODE_BASE_MODE` | `keymap::leader_token()` etc | constants |
| 006-keymap-chord-timeout | `config keymap defaults (keymap.tsx defaults/timeout)` | `KeymapOptions::default` | leader, timeout_ms |
| 007-theme-presets | `theme/index.ts DEFAULT_THEMES` keys | `theme::available_themes` | sorted name list |
| 008-theme-hex | `theme/index.ts` hex parse via RGBA.fromHex path (`DEFAULT_THEMES.dracula` accent values) | `parse_hex_color` | rgb values for 3 hexes |
| 009-theme-light-dark | `theme/index.ts` opencode dark/light resolved colors | `Theme::dark()/light()` | 3 anchor color fields |
| 010-format-duration | `util/format.ts formatDuration` | `format_duration` | 12 boundary cases (0,1,59,60,61,3599,3600,86399,86400,604799,604800,1209600) |
| 011-format-collapse | `util/collapse-tool-output.ts collapseToolOutput` | `collapse_tool_output` | short + long (output, overflow) |
| 012-clipboard-lookup | `clipboard.ts copyCommand` | `copy_command_with_lookup` | 4 env matrices (linux+wl, linux+xclip, linux+xsel, darwin+osascript, none) |
| 013-editor-normalize | `editor.ts normalizePromptContent` | `normalize_prompt_content` | 3 cases ("hello\n", "hello\r\n", "a\nb\n") |
| 014-patch-metadata | `session-ui apply-patch-file.ts patchFile/patchFiles` | `parse_apply_patch_files` | parsed relativePath/additions/deletions |
| 015-toast-lifecycle | `tui` toast store semantics (notifications.test.ts behavior via local ToastStore) | `ToastStore show/prune` | len after show, after prune |
| 016-prompt-interaction | `session-ui machine.ts transitionPromptInputV2` | oc-tui prompt interaction transition fn (if absent → record `"not_implemented"` and let engine FAIL — remediation loop) | 3 transitions |
| 017-logo | `logo.ts logo` | `logo::LOGO` | left rows |
| 018-locale-titlecase | `util/locale.ts titlecase` | oc-tui titlecase (verify existence; else FAIL→remediate) | 3 cases |
| 019-locale-duration | `util/locale.ts duration` | oc-tui equivalent | 3 cases |
| 020-clipboard-wayland | as 012 sub-case (kept for artifact continuity) | same | same shape |
| 021-clipboard-macos | as 012 sub-case | same | same |
| 022-clipboard-x11 | as 012 sub-case | same | same |
| 023-clipboard-none | as 012 sub-case | same | same |
| 024-…—*renamed* 024-patch-metadata-extra | `patchFiles` multi-file case | same | list len + per-file additions |
| 025—*renamed* 025-theme-custom-hex | theme hex round-trip on 3 presets | `Theme::by_name` anchors | hex→rgb map |
| 026—*renamed* 026-editor-multiline | `normalizePromptContent` multiline cases | same | 2 multiline cases |

Old ids whose claimed behavior was never importable from vendored pure modules (grapheme backspace, cursor movement inside OpenTUI textarea, PTY paste as "scenario") move to the honest EXCLUDED list in Task 5 (C4) — they are covered instead by the PTY suite (Task 4) or marked excluded with rationale.

- [ ] **Step 3: Rewrite `scripts/run_differential.py` match core** (full replacement of lines 263-387):

```python
def canonical(obj) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)

# per scenario:
#   ref: node --import <loader> -e <real-import eval printing ONE canonical JSON line>
#   rust: cargo run -q -p oc-tui --example diff_scenarios -- <id>
#   parse BOTH stdout lines as JSON (strip cargo noise: take last line starting with '{')
#   matched = ref_exit == 0 and rust_exit == 0 and ref_obj == rust_obj  # dict equality
#   result.json gains: "reference_output", "rust_output", "outputs_equal": bool,
#                      "reference_output_sha256", "rust_output_sha256" (compared, not just recorded)
```

- [ ] **Step 4: Run and iterate.** `cd <root> && python3 scripts/run_differential.py` — expected: genuine PASSes; genuine FAILs where Rust/reference differ or an eval fails to import. Fix evals until every remaining FAIL is a *behavioral* difference (not an import/format error).

- [ ] **Step 5: Tamper test (proves C1 dead).** Manually edit one rust-frame.txt output line, rerun engine, confirm that scenario FAILs and `result.json.outputs_equal == false`. Revert edit, rerun, PASS.

- [ ] **Step 6: Commit** `git add -A && git commit -m "fix(audit): C1 differential engine compares canonical outputs, not exit codes"`

---

### Task 2 (C2): Delete the fabricator, regenerate all artifacts

**Files:** Delete `scripts/generate_differential_artifacts.py`; regenerate `rust-port-audit/tui/differential/*`; extend `scripts/verify-tui-audit.py`.

- [ ] **Step 1:** `git rm scripts/generate_differential_artifacts.py`
- [ ] **Step 2:** `rm -rf rust-port-audit/tui/differential && python3 scripts/run_differential.py` (all dirs now solely engine-produced)
- [ ] **Step 3:** Verifier recomputation — replace `verify_differential_scenarios()` result.json check with: recompute sha256 of parsed `reference_output`/`rust_output` strings and require equality with recorded hashes; require `outputs_equal is True`; require `"Executed Command:"` lines match `scenario.json` commands; require `reference_command` contains `--import` loader path (real reference execution) and `rust_command` contains `diff_scenarios`.
- [ ] **Step 4:** Tamper test 2: edit a `result.json` `status` to `"PASS"` while outputs differ → verifier must FAIL. `python3 scripts/verify-tui-audit.py` (expect exit 1 + error). Revert.
- [ ] **Step 5:** Commit: `"fix(audit): C2 remove hardcoded artifact fabricator; verifier recomputes hashes+equality"`

---

### Task 3 (C3): No handwritten reference implementations, anywhere

**Files:** `scripts/verify-tui-audit.py` (+ regenerate if needed)

- [ ] **Step 1:** Verifier gate: scan every `reference-frame.txt` "Executed Command" — FAIL if it matches `node -e` without `--import`, or if the eval string contains a `function` definition (handwritten impl) — only `import ... from "reference/..."` + calls allowed.
- [ ] **Step 2:** `grep -rl "console.log(\"Selected clipboard" rust-port-audit/` → must be empty after Task 1-2 regeneration; verifier enforces permanently.
- [ ] **Step 3:** Run verifier (PASS), commit: `"fix(audit): C3 verifier bans handwritten node -e reference implementations"`

---

### Task 4 (C4): Scenario alignment, committed loader, honest exclusions

**Files:** `scripts/ts_loader.mjs` (already committed), `package.json` (new), `scripts/run_differential.py` (eval table), `rust-port-audit/tui/TUI-DIFFERENTIAL-EVIDENCE.md` (EXCLUDED section)

- [ ] **Step 1:** `<root>/package.json`: `{"private":true,"devDependencies":{"esbuild":"^0.25.0"}}` (pin whatever `npm ls esbuild` resolves locally, `npm install --package-lock-only` to commit lockfile). Verify `npm ci && node --import <loader> -e '<smoke import>'` works from clean temp clone dir copy.
- [ ] **Step 2:** Audit each scenario eval against its table name (no `typeof` probes, no logo/titlecase stand-ins). The renamed 013-editor-normalize/014-patch-metadata ids replace the old misaligned 024/025/026; scenario.json gains `"behavior": "<one-line real behavior>"`.
- [ ] **Step 3:** EXCLUDED list in TUI-DIFFERENTIAL-EVIDENCE.md: grapheme-segmentation editor primitives, OpenTUI textarea internals, renderer-level behaviors — reason: `@opentui/*` runtime not vendored (bun workspace, mocked by loader); coverage responsibility moved to PTY suite (Task 5) where observable.
- [ ] **Step 4:** Commit: `"fix(audit): C4 real-symbol scenario evals, pinned esbuild, honest exclusion list"`

---

### Task 5 (C5): Real interactive PTY suite

**Files:** `crates/oc-tui/Cargo.toml` (`[dev-dependencies] portable-pty = "0.8"`), Create `crates/oc-tui/tests/interactive_pty.rs`; Modify `crates/oc-tui/tests/terminal_e2e.rs` (delete self-echo lifecycle test + adopted weak interactive test)

- [ ] **Step 1:** Add dep; `cargo build -p oc-tui --tests` compiles.
- [ ] **Step 2:** Shared harness in `interactive_pty.rs`:

```rust
struct TuiSession { reader: Box<dyn std::io::Read + Send>, writer: Box<dyn std::io::Write + Send>, child: Box<dyn portable_pty::Child + Send + Wait> }
fn launch(cols: u16, rows: u16) -> TuiSession {
    // portable_pty::native_pty_system().openpty(PtySize{..})
    // cmd: find_opencode_binary() (panic if missing), args: [], env:
    //   OPENCODE_CONFIG_DIR=<tmp>, XDG_DATA_HOME=<tmp>/data, XDG_STATE_HOME=<tmp>/state,
    //   XDG_CONFIG_HOME=<tmp>/config, HOME=<tmp>, NO_COLOR=1, TERM=xterm-256color
    // cwd: fresh temp project dir (git init + one file)
}
fn wait_frame(s: &mut TuiSession, needle: &str, timeout: std::time::Duration) -> Vec<u8> // poll-read until output contains needle
```

- [ ] **Step 3:** Tests (each: no skip — missing binary/pty panics):
  1. `tui_launches_renders_home_and_quits_cleanly` — wait for prompt/home marker (logo block char `█` or `Ask`); send `q`/Ctrl+C; assert child exits ≤5s AND captured bytes contain `\x1b[?1049l` and `\x1b[?25h` (app-emitted restore).
  2. `tui_typing_appears_in_prompt` — type `hello world`; assert frame contains `hello world`.
  3. `tui_dialog_escape_restores_state` — open model dialog (`ctrl+x` then `m` or the bound key from `keybind.rs`), assert dialog marker; Escape; assert dialog gone and typed prompt text still present.
  4. `tui_resize_keeps_responsive` — resize to 100x30 then 60x20 via `pty.master.resize`… use `portable_pty::MasterPty::resize`; after each, type + assert echo within 2s.
  5. `tui_bracketed_paste_into_prompt` — `\x1b[200~pasted line\x1b[201~`; assert `pasted line` in frame.
  6. `tui_sigterm_exits_and_restores` — SIGTERM via `child.kill()` on unix (use `libc::kill(pid, SIGTERM)`); assert exit + restore sequence in drain read.
- [ ] **Step 4:** Delete from `terminal_e2e.rs`: `real_pty_child_process_lifecycle_and_teardown` (self-echo) and adopted `real_pty_spawns_interactive_tui_and_handles_input_and_exit` (superseded; its no-assertion read loop would pass on a blank screen).
- [ ] **Step 5:** `cargo test -p oc-tui --test interactive_pty -- --test-threads=1` → all PASS (unix locally; Windows validated in CI Task 10).
- [ ] **Step 6:** Commit: `"fix(audit): C5 real interactive TUI PTY suite with frame assertions; drop self-echo lifecycle test"`

---

### Task 6 (C6): Honest atomic denominator

**Files:** Create `scripts/generate_atomic_inventory.py`; regenerate `rust-port-audit/tui/TUI-REFERENCE-INVENTORY.csv`; Modify `rust-port-audit/tui/TUI-FEATURE-PARITY.csv` generation

- [ ] **Step 1:** Generator: (a) parse all `test(...)`/`it(...)` names from `reference/packages/tui/test/**` + `reference/packages/session-ui/src/**/*.test.ts(x)` (regex `^\s*(?:test|it)\(["'](.+?)["']`); (b) each becomes an atomic row `id, area (parent dir), behavior=test name, evidence=<differential scenario id | rust test path::fn | PTY test name>`; (c) 58 domain rows from TUI-FEATURE-PARITY.csv become `domain` grouping column only, not denominator; (d) `Status` computed: PASS iff evidence exists in repo (grep the cited test/case) else FAIL; parity line `PASS x / TOTAL y = z%` written to CSV footer + `TUI-FINAL-REPORT.md` regeneration input.
- [ ] **Step 2:** Unmapped evidence rows (reference tests with no Rust evidence) are LISTED, not hidden — drives remediation loop; loop until each is either evidenced or explicitly N/A'd with reason column.
- [ ] **Step 3:** Run, inspect output, commit: `"fix(audit): C6 atomic inventory generated from real reference tests; honest parity computation"`

---

### Task 7 (C7): Truthful source coverage

**Files:** Modify `scripts/generate_source_coverage.py`; regenerate `REFERENCE-SOURCE-COVERAGE.csv`, `UNMAPPED-REFERENCE-FILES.txt`

- [ ] **Step 1:** `COVERED` only when file path appears in some atomic row's evidence/reference-file column; else `LISTED`. Config/docs/tests get `Category` ≠ BEHAVIOR_SOURCE and never count toward parity denominators. Drop AGENTS.md-style rows from behavior mapping.
- [ ] **Step 2:** UNMAPPED-REFERENCE-FILES.txt lists every BEHAVIOR_SOURCE file with zero atomic rows — target: empty or each entry annotated with rationale (e.g. UI-shell re-implemented in PTY suite).
- [ ] **Step 3:** Commit: `"fix(audit): C7 source coverage reflects actual evidence links"`

---

### Task 8 (C8): Truthful reference-test mapping + verifier reference-side checks

**Files:** Regenerate `TUI-REFERENCE-TEST-MAPPING.csv`; Modify `scripts/verify-tui-audit.py`

- [ ] **Step 1:** Mapping rows only where BOTH sides exist: reference file+test name verified by grep in reference tree; Rust fn verified with `#[test]`; `Test type` must be truthful (`Unit`, not `Differential` — differential evidence lives in scenario dirs). Phantom rows (`display.test.ts`, `perf.test.ts`, `prompt/input.test.ts`) removed; `runtime.test.tsx` rows remap to the real tests (`abbreviates paths…`, `provides focused immutable runtime inputs`) with honest Rust counterparts (add `abbreviate_home` unit test if missing).
- [ ] **Step 2:** Verifier: for each mapping row, grep reference file for the test name — error if absent. Also error if `Evidence` column contains `Machine-verified in CI` without a concrete artifact path (ban marketing strings).
- [ ] **Step 3:** Commit: `"fix(audit): C8 mapping verified against real reference tests; verifier checks both sides"`

---

### Task 9 (C9): Single audit identity

**Files:** Create `scripts/stamp_audit_identity.py`; regenerate `AUDIT-IDENTITY.md`, `TUI-FINAL-REPORT.md`, `TUI-CROSS-PLATFORM.md`, `TUI-TEST-MATRIX.md`, `TUI-RELEASE-GATE.md`

- [ ] **Step 1:** One script writes all docs from live state: `git rev-parse HEAD`, `git status --porcelain` (must be clean → docs generated then committed in a second commit), computed parity from C6 CSVs, test counts via `cargo test -p oc-tui -- --list | wc -l` style enumeration, differential counts from result.json files. No hardcoded run IDs; CI run recorded post-push as final commit (run `gh run list` after push).
- [ ] **Step 2:** Every doc footer: `generated-by: scripts/stamp_audit_identity.py @ <sha>`; verifier (from C8) cross-checks all docs cite the same SHA.
- [ ] **Step 3:** Commit: `"fix(audit): C9 all identity docs generated from one live-state stamper"`

---

### Task 10 (C10): Strict CI + full validation

**Files:** `.github/workflows/ci.yml`; lint fixes across `crates/`; remove adopted `#[allow(clippy::type_complexity)]` (oc-mcp/src/index.rs:513) by fixing the underlying type

- [ ] **Step 1:** Local: `cargo clippy --workspace --all-targets -- -D warnings` (no allow-all). Fix every firing lint properly (no new `#[allow]` without justification comment).
- [ ] **Step 2:** New workflow (replace clippy line; add jobs):

```yaml
  clippy:
    runs-on: ubuntu-latest
    steps: [checkout, setup-rust(+clippy)]
      - run: cargo clippy --workspace --all-targets -- -D warnings
  differential:
    runs-on: ubuntu-latest
    steps: [checkout, setup-rust, setup-node@v4 (node 22)]
      - run: npm ci
      - run: cargo build --workspace
      - run: python3 scripts/run_differential.py
      - run: python3 scripts/verify-tui-audit.py
  test: (matrix os ×3)
    steps: [checkout, setup-rust]
      - run: cargo build --workspace        # ensures PTY tests find binaries
      - run: cargo test --workspace --no-fail-fast
```

- [ ] **Step 3:** Local full gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace && cargo test -p oc-tui -- --test-threads=1 && cargo test --workspace` → all green.
- [ ] **Step 4:** Push branch, open nothing yet: `git push -u origin audit-integrity`; `gh run watch` → all jobs green (incl. windows PTY + differential). Record run id; re-stamp docs (C9 script) with run id; final commit; push.
- [ ] **Step 5:** Commit sequence: `"fix(ci): C10 strict clippy, differential job, build-before-test, PTY on 3 OS"` then `"docs(audit): record final verified CI run <id>"`

---

## Remediation loop (after Task 5, alongside Tasks 6–9)

For every genuine FAIL surfaced (engine mismatch, PTY assertion, unevidenced atomic row): read the reference source of that behavior → fix oc-tui implementation to match → rerun scenario/test → green. Genuinely unfixable/uniformable items stay FAIL with rationale in final report — no soft verdicts, denominator never shrinks to pass.

## Definition of Done

All 10 tasks committed on `audit-integrity`; CI green on pushed branch including new `differential` job and 3-OS PTY tests; `verify-tui-audit.py` passes AND its two tamper-tests fail correctly; every `result.json` has `outputs_equal: true` backed by recomputable hashes; atomic inventory parity number is machine-computed; docs all cite one SHA + one CI run.
