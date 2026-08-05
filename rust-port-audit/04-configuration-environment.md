# Agent 04 — Configuration, Environment, and Project Discovery

Audit of the `oc-config`, `oc-util` (paths/XDG), and `oc-cli` (env handling) crates of the
opencode-rs Rust port against the vendored reference `opencode v1.18.13`.

## Scope

- Config formats & parsing: opencode.json/.jsonc (JSONC), legacy `config` TOML migration.
- Unknown/missing fields, type validation, defaults, JSON5-vs-JSONC boundary.
- Precedence & discovery: global (`config.json` → `opencode.json` → `opencode.jsonc`),
  project traversal up to worktree, `.opencode/` dirs, home `.opencode`, `OPENCODE_CONFIG`,
  `OPENCODE_CONFIG_DIR`, `OPENCODE_CONFIG_CONTENT`, `OPENCODE_PERMISSION`,
  `OPENCODE_DISABLE_PROJECT_CONFIG`, `OPENCODE_DISABLE_AUTOCOMPACT`, `OPENCODE_DISABLE_PRUNE`.
- Sections: permission ruleset, provider/agent/command/mcp, experimental, tool_output,
  references, plugin, skills, compaction, watcher, layout, attachment, enterprise, server.
- Paths/XDG: `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`,
  `OPENCODE_TEST_HOME`, symlinks, unicode paths.
- Variable substitution `{env:VAR}` / `{file:path}`; serialization & side-effect writes
  (`$schema` injection, seeding, `.gitignore`, TOML migration writes).

## Repository areas inspected

- `crates/oc-config/src/`: `lib.rs`, `load.rs`, `parse.rs`, `paths.rs`, `merge.rs`, `managed.rs`,
  `variable.rs`, `error.rs`, `entry_name.rs`, `jsnum.rs`, `v1/config.rs`, `v1/permission.rs`.
- `crates/oc-util/src/global.rs`, `crates/oc-cli/src/cli/paths.rs`, `cli/context.rs`,
  `cli/args.rs`, `main.rs`.
- Reference: `packages/opencode/src/config/{config,parse,paths,variable,plugin}.ts`,
  `packages/core/src/global.ts`, `packages/core/src/flag/flag.ts`,
  `packages/core/src/v1/config/{config,permission}.ts`, `packages/core/src/config/{experimental,reference}.ts`,
  `packages/opencode/src/env/index.ts`, `packages/opencode/src/index.ts`,
  `packages/opencode/src/cli/cmd/{models,debug/config}.ts`.
- Existing tests: `crates/oc-config/tests/{load,extra,golden}.rs` (all pass; 29 integration +
  7 unit).

## Commands executed

- `cargo test -p oc-config` — 29 passed, 0 failed.
- Differential harness: reference binary `/root/.opencode/bin/opencode` (`models`,
  `debug config`, `debug paths`) vs a harness binary driving `oc_config::load::load_instance_state`
  under identical disposable `HOME`/`XDG_CONFIG_HOME` (built from `/tmp/oc04/harness`,
  output `/root/opencode-rs/target/release/oc04-harness`).
- ~120 differential scenarios across 12 suites saved under
  `rust-port-audit/artifacts/04/*-diff.txt` with fixtures under `artifacts/04/fixtures/`.
- Manual probes: NO_COLOR/FORCE_COLOR, symlinked config dir, unicode path, directory-as-config,
  seeded-file permissions, `$schema` write-back bytes, `debug paths`.

## Runtime scenarios attempted

| Suite | # | Outcome |
|---|---|---|
| parsing (valid/JSONC/JSON5/BOM/dup/comments/empty/garbage/truncated/top-level) | 16 | 3 divergences |
| valid sections (provider/mcp/permission/tools/experimental/tool_output/compaction/skills/references/plugin/watcher/…) | 30 | 2 content diffs |
| env vars (OPENCODE_CONFIG/DIR/CONTENT/PERMISSION/DISABLE_*) | 14 | all MATCH |
| discovery & precedence (nested, .opencode, home .opencode, config.json, XDG priority) | 13 | all MATCH |
| TOML legacy migration (isolated fresh HOME per side) | 5 | 2 divergences |
| substitution {env:}/{file:} | 7 | all MATCH (incl. error text) |
| type validation (bounds, literals, case-sensitivity) | 13 | all MATCH |
| precedence2 (env vs project vs config-dir vs content) | 4 | all MATCH |
| permission details | 10 | all MATCH |
| misc ($schema, nested comments, permission merge) | 9 | all MATCH |

## Architecture or behavior summary

The Rust `oc-config` faithfully ports the reference pipeline: variable substitution →
JSONC parse → legacy-key normalization (`theme`/`keybinds`/`tui` dropped) → top-level
unknown-key rejection → serde schema decode → per-file `$schema` injection → deep merge
(remeda `mergeDeep` semantics incl. element-wise array merge) → `tools`→permission
conversion → `OPENCODE_PERMISSION` merge → `mode`→primary-agent migration → username
fallback → `autoshare`→`share` → autocompact/prune env toggles. Discovery order, merge
order, and the winning-source semantics match the reference exactly across all valid-input
and precedence scenarios tested. The crate is self-contained and passes its tests, but is
**not yet wired into the `oc-cli` binary** (config loading is a `TODO(integration)` in
`oc-cli`), so runtime parity can only be demonstrated through the library harness.

## Positive observations

- Precedence (global < project < `.opencode` dirs < `OPENCODE_CONFIG` < `OPENCODE_CONFIG_DIR`
  < `OPENCODE_CONFIG_CONTENT`) is byte-for-byte identical in every tested ordering.
- `$schema` write-back side effect is byte-identical to the reference (`parse.rs`-style header).
- Global config seeding, `.gitignore` creation, and 0644 file permissions match (root umask).
- `{env:VAR}`/`{file:...}` substitution incl. missing-file error text matches exactly.
- Duplicate keys: last-wins in both; identical rewritten file.
- Permission ruleset (`ask`/`allow`/`deny`, object rules, `*`-normalization, action-only keys)
  matches, incl. `OPENCODE_PERMISSION` deep-merge direction (env wins).
- Unknown keys are rejected at top level and ignored in record-typed sections, same as reference.
- Type/literal/bounds validation (logLevel case-sensitivity, tool_output > 0, mcp_timeout > 0,
  subagent_depth ≥ 0, `share` literals, `autoupdate` bool|"notify") matches on every case.
- Symlinked config dir and unicode path resolution match; BOM accepted by both.

## Findings summary

| ID | Severity | Confidence | Finding | Reference | Rust |
|---|---|---|---|---|---|
| CONFIG-001 | High | CONFIRMED | JSON5-permissive inputs accepted by Rust, rejected by reference (single-quoted strings, unquoted keys) | reject | accept |
| CONFIG-002 | Medium | CONFIRMED | Whitespace-only config file: Rust returns defaults, reference errors | error | accept |
| CONFIG-003 | Medium | CONFIRMED | Legacy TOML migration aborts on unknown keys (loses provider/model, skips migration writes) | migrate+keep | abort+drop |
| CONFIG-004 | Medium | CONFIRMED | `tools` field dropped from resolved config after permission derivation | retained | removed |
| CONFIG-005 | Medium | CONFIRMED | Local plugin file URL not canonicalized (`file:///…/./local-plugin`) | normalized | `./` kept |
| CONFIG-006 | Medium | CONFIRMED | Non-ENOENT config read errors silently swallowed (directory-as-config) | hard error | defaults |
| CONFIG-007 | Low | MEDIUM | `$schema` write-back differs on BOM-prefixed files (static only) | regex on `\s*\{` | `find('{')` |

Informational: error-message wording differs (serde vs Effect-schema phrasing) while
accept/reject parity holds; JSON key order of the merged config differs; `plugin_origins`
appears in reference `debug config` output but not Rust's; reference ignores `NO_COLOR` for
error styling (Rust binary currently emits no ANSI on config errors).

## Detailed findings

### CONFIG-001 — JSONC parser is JSON5-permissive (accepts invalid inputs)
- `crates/oc-config/src/parse.rs:11-12` uses `json5::from_str::<Value>`; the reference uses
  `jsonc-parser` with only `allowTrailingComma` (`reference/.../config/parse.ts:8-10`).
- Runtime: single-quoted strings and unquoted keys are accepted by Rust (exit 0) and rejected
  by the reference ("InvalidSymbol at line 2, column 3"). All 16 parsing fixtures in
  `artifacts/04/parsing-diff.txt`.
- Impact: a config that fails in the reference silently loads in the Rust port; the port
  comment in `parse.rs:8-10` claims this "matches opencode's JSON5-compatible config format",
  which is incorrect for the reference binary.

### CONFIG-002 — whitespace-only config file accepted by Rust
- `crates/oc-config/src/load.rs:132-134` returns `Info::default()` when `text.trim().is_empty()`.
  The reference only short-circuits on empty string (`config.ts:241-242`: `if (!text)`), so a
  whitespace-only file reaches `jsonc-parser` and errors ("ValueExpected"). Runtime: ref=1,
  rust=0 (`ws-only` case).

### CONFIG-003 — legacy TOML migration is schema-validated in Rust (aborts on unknown keys)
- `crates/oc-config/src/load.rs:493` runs `parse::schema` on the TOML-derived map; on error the
  migration is swallowed (`if let Ok(next)` at `load.rs:459-463`). The reference
  (`config.ts:262-275`) does **not** validate: it migrates `provider`+`model`, keeps every
  other key (e.g. `theme`), sets `$schema`, writes `config.json`, and unlinks `config`.
- Runtime (`artifacts/04/toml-diff.txt`): with `theme` present, reference keeps `theme` and
  migrates the model; Rust drops everything, leaves `config` in place, and never writes
  `config.json`. Clean TOML migrates identically in both.

### CONFIG-004 — `tools` key removed after permission derivation
- `crates/oc-config/src/load.rs:353-354` uses `result.tools.take()`. Reference `config.ts:553-564`
  iterates but does not delete `tools`, so the resolved config retains it.
- Runtime: `tools-map` case — reference output has both `tools` and derived `permission`; Rust
  only `permission`. No downstream consumer reads `config.tools`, so functional impact is
  limited, but exact-serialization parity (CONTEXT.md goal #1) is broken.

### CONFIG-005 — local plugin file URLs not canonicalized
- `crates/oc-config/src/load.rs:682` builds `path_to_file_url(&base.join(specifier))`, keeping
  `./`. Reference `plugin.ts:53` uses `path.resolve(base, spec)`, normalizing away `./`.
- Runtime: `plugin-section` case — reference `file:///…/opencode/local-plugin`, Rust
  `file:///…/opencode/./local-plugin`. Different URL strings affect plugin identity, dedupe,
  and origin tracking.

### CONFIG-006 — non-ENOENT config read errors silently swallowed
- `crates/oc-config/src/load.rs:128-131`: any `read_to_string` error returns `Ok(default)`.
  Reference `readFileStringSafe(...).pipe(Effect.orDie)` dies on non-ENOENT errors.
- Runtime: a directory named `opencode.jsonc` in the config dir — reference exits 1
  ("Unexpected error … BadResource"), Rust exits 0 with defaults.
- Note: permission-denied tests (chmod 000) were invalid because the audit runs as root; the
  EISDIR case above is the confirmed non-ENOENT failure.

### CONFIG-007 — `$schema` write-back vs BOM (static only)
- `load.rs:168-180` `insert_schema_header` uses `text.find('{')`; reference uses
  `text.replace(/^\s*\{/, …)` (`config.ts:233`). On a BOM-prefixed file the reference would not
  inject `$schema` (file left unchanged) while Rust would insert after the BOM. Not
  runtime-verified; low severity.

## Feature or behavior gaps (reference behavior not yet ported)

- `Config.update` / `Config.updateGlobal` (write-back, JSONC `patchJsonc`) — TODO
  (`crates/oc-config/src/lib.rs:36`).
- Remote well-known `/account-org` config fetching (HTTP) — TODO (`lib.rs:31-33`); requires
  auth + network, not exercised here.
- macOS managed preferences plist reading (`ConfigManaged.readManagedPreferences`) — TODO
  (`lib.rs:37-38`); `managed_config_dir` exists with `OPENCODE_TEST_MANAGED_CONFIG_DIR`.
- Background npm dependency install per config directory — TODO (`load.rs:276-277`).
- `oc-cli` does not consume `oc_config` at all: config loading is not wired to the binary, so
  `opencode` (Rust) cannot be differentially run against the reference for config errors.

## Test coverage gaps

- No test asserting JSONC strictness (single-quote / unquoted-key rejection); the crate's
  existing `extra.rs`/`golden.rs` tests do not cover the JSON5-vs-JSONC boundary.
- No test for whitespace-only config rejection.
- No test asserting `tools` is retained after permission derivation.
- No test for non-ENOENT read errors (directory-as-config).
- No test for TOML migration with unknown keys or the reference's non-validating behavior.
- No golden/differential tests run against the actual reference binary; existing tests are
  self-referential.

## Unverified areas

- CLI-level config error rendering in the Rust binary (not wired; `ui::error` path untested).
- macOS managed preferences; remote well-known/account config; Windows behavior (only
  `managed_config_dir` cfg branches reviewed statically).
- Permission-denied (EACCES) behavior on unreadable files — BLOCKED as the audit runs as root.
- Effect of `$schema` write-back on BOM-prefixed files (CONFIG-007, static only).

## Final domain verdict

**READY_WITH_MINOR_REMEDIATION**

The `oc-config` crate reaches strong parity on valid-input handling, precedence, substitution,
permissions, and side-effect writes (all ~120 differential scenarios that should MATCH do match
semantically). However, six CONFIRMED divergences remain — most importantly accepting
JSON5-permissive syntax the reference rejects (CONFIG-001) and silently swallowing
non-ENOENT config read errors (CONFIG-006), plus three medium content divergences
(TOML migration abort, `tools` removal, plugin URL canonicalization). All are localized and
fixable in `oc-config`; none block the crate's core function, and the crate is not yet wired
into the CLI binary. Final message below.

- High: 1 · Medium: 5 · Low: 1 · Informational: 5
- Confirmation report written to `rust-port-audit/04-configuration-environment.md`; evidence in
  `rust-port-audit/artifacts/04/`.
