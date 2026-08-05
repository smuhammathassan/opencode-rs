# Plan 04 — Configuration Parity & Runtime Bootstrap (Wave 0 READ-ONLY)

- Agent: 04 | Domain: configuration/environment/JSONC parsing, TOML migration, config-into-runtime wiring
- Repo: opencode-rs @ `fix/audit-remediation` (baseline `90727e19860b8e0c1b0cf6469b696ef3b3efaeb1`)
- Status: READ-ONLY PLAN — no production source modified in this pass
- Inputs: `rust-port-audit/04-configuration-environment.md`, `FINDINGS.json` (CONFIG-001/002/003), `FINDING-STATUS.csv`, `rust-port-audit/artifacts/04/**` (fixtures + diffs), `crates/oc-config/src/**`, `crates/oc-cli/**`, `crates/oc-mcp/src/config.rs`, `crates/oc-project/src/util/config.rs`, reference `packages/opencode/src/config/{config,parse,plugin,variable}.ts`, `packages/core/src/fs-util.ts`, `packages/core/src/config/*.ts`, and the reference oracle `/root/.opencode/bin/opencode` (probed this pass).

## 1. Owned findings

| ID | Sev | Status | Title | Reference | Rust today |
|----|-----|--------|-------|-----------|------------|
| CONFIG-001 | High | CONFIRMED (runtime) | JSONC parser is JSON5-permissive (accepts single-quoted strings, unquoted keys) | reject | accept |
| CONFIG-002 | Medium | CONFIRMED (runtime) | Whitespace-only config file returns defaults; reference errors `ValueExpected` | error | defaults |
| CONFIG-003 | Medium | CONFIRMED (runtime) | Legacy TOML migration schema-validates; unknown keys abort migration (drops provider/model, skips write+unlink) | migrate+keep | abort+drop |
| CONFIG-004 | Medium | CONFIRMED (runtime) | `tools` key removed from resolved config after permission derivation | retained | removed |
| CONFIG-005 | Medium | CONFIRMED (runtime) | Local plugin file URL not canonicalized (`file:///…/./local-plugin`); reference `path.resolve` strips `./` | normalized | `./` kept |
| CONFIG-006 | Medium | CONFIRMED (runtime) | Non-ENOENT config read errors swallowed as defaults (dir-as-config) | hard error | defaults |
| CONFIG-007 | Low | CONFIRMED (static + probed) | `$schema` write-back regex differs: reference `text.replace(/^\s*\{/, …)` strips leading whitespace; Rust `text.find('{')` keeps it. BOM nuance resolved: file reads strip BOM, so the BOM concern is moot | `^\s*\{` | `find('{')` |
| (integration) | — | CONFIRMED | `oc-config` is not wired into any production component; duplicate partial config loaders/mirrors exist in oc-cli (serve/mcp/debug), oc-mcp, oc-project, oc-server | one resolved config | isolated |

Owned-and-carried: `plugin_origins` (reference `debug config` prints it; Rust `InstanceState` holds it separately) and the `Config.update`/`updateGlobal` write-back API (TODO in `lib.rs:36`) — both are required for the MCP/CLI wiring in §6.

## 2. Target state ("done" for this domain)

1. `crates/oc-config` parses opencode.json/.jsonc with a **strict jsonc-parser-equivalent** implementation: byte-exact accept/reject and byte-exact JSONC error blocks (verified against the reference binary). `json5` dependency removed.
2. `load_file` error semantics match the reference: empty string → defaults; `NotFound`/`PermissionDenied` → defaults; any other read error (EISDIR, …) → hard error surfaced by the CLI (exit 1). Whitespace-only files reach the parser and error.
3. Legacy TOML migration is non-validating: keeps unknown keys, merges onto the existing global result, writes `config.json`, unlinks `config`, swallows bad-TOML; side-effect files byte-match the reference.
4. `tools` is retained in the resolved config after permission derivation; plugin file URLs are lexically normalized (no `./`).
5. `$schema` write-back uses `^\s*\{` semantics.
6. The config-loading seam for the composition root is defined (`Context` holds one resolved `InstanceState`), and the duplicate partial loaders/mirrors in oc-cli/oc-mcp/oc-project are documented for replacement by the owning agents (§6).
7. A reusable differential fixture harness (valid + invalid sets) runs the same scenarios against `oc_config` and the reference binary; fixtures are captured to golden tests so CI does not need the oracle.

## 3. Files to change (ownership map)

Owned by this agent (modify):
- `crates/oc-config/src/parse.rs` — replace `json5` with the strict port; keep the `jsonc(text, source) -> Result<Value, ConfigError>` signature and the `--- JSONC Input ---` error formatting (§4/§5).
- `crates/oc-config/src/jsonc.rs` (**new**) — port of `jsonc-parser` 3.3.1 scanner + `parse` (§4). MIT-attribution header (SUPPLY-003).
- `crates/oc-config/src/load.rs` — CONFIG-002/003/004/005/006/007 fixes (§5).
- `crates/oc-config/src/lib.rs` — re-export `Config::update_global` seam once added; no API break otherwise.
- `crates/oc-config/Cargo.toml` — remove `json5` (used nowhere else in the workspace); root `Cargo.toml` `[workspace.dependencies]` entry for `json5` removed if unclaimed.
- Tests: `crates/oc-config/tests/diff.rs` (**new**, differential fixture set, §7), extend `tests/extra.rs` (plugin URL normalization, tools retention), `tests/load.rs` (ws-only, dir-as-config, TOML unknown-key), `tests/golden.rs` (captured JSONC error-text goldens).

Coordination only (other agents own; see §8):
- `crates/oc-cli/src/cli/context.rs` — add resolved-config seam (Agent 02 composition root).
- `crates/oc-cli/src/cli/network.rs` / `cmd/serve.rs` — drop the `ServerConfig` mirror, use `Info.server` (Agent 02/12).
- `crates/oc-cli/src/cli/cmd/debug.rs` — `debug config` prints resolved config + `plugin_origins` (Agent 12; uses my harness output as goldens).
- `crates/oc-cli/src/cli/cmd/mcp.rs` — `mcp add` write-back via new `Config.update_global` (Agent 13).
- `crates/oc-mcp/src/config.rs` — promote mirror → `oc_config::v1::mcp::Value` (Agent 13).
- `crates/oc-project/src/util/config.rs` — replace mirror → `Info.snapshot` (Agent 02).
- `crates/oc-server/src/instance_handlers.rs` — `/api/config` returns the merged view (Agent 02).
- `crates/oc-command/src/command/mod.rs` — consume `oc_config::load::load_commands` (Agent 12).

## 4. JSONC replacement approach — in-crate rewrite (recommended), crate fallback

### 4.1 Decision: port `jsonc-parser` 3.3.1 into `crates/oc-config/src/jsonc.rs`

The reference uses `jsonc-parser` **3.3.1** (Microsoft, MIT) with `{ allowTrailingComma: true }` (`reference/packages/opencode/src/config/parse.ts:10`). I fetched the exact 3.3.1 `scanner` + `parse` sources this pass; the port surface is small and self-contained:

- **Scanner** (`impl/scanner.js`, ~200 LOC): token kinds (OpenBrace/CloseBrace/OpenBracket/CloseBracket/Comma/Colon, `null`/`true`/`false`, StringLiteral, NumericLiteral, LineComment/BlockComment/LineBreak/Trivia, Unknown, EOF), `ScanError` set (`UnexpectedEndOfComment/String/Number`, `InvalidUnicode/EscapeCharacter/Character`), whitespace = space+tab only, numbers `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?` with the documented quirks (a leading zero like `01` ends the number after `0`; `1.`/`1e` set `UnexpectedEndOfNumber`).
- **Parser** (`impl/parser.js` `visit`/`parse`, ~250 LOC): the exact `ParseErrorCode`→error emission incl. `handleError` skip-until recovery, `needsComma`/`isFirstElement` state, `currentProperty`/`previousParents` object construction, and the top-level EOF → `ValueExpected` / non-EOF → `EndOfFileExpected` logic. This is required — the reference prints **all** errors joined with `\n`, and the per-error offset/order (e.g. `01` → `CommaExpected`+`PropertyNameExpected`+`ValueExpected`) only matches if the full state machine is ported, not just "first error".
- Object construction: duplicate keys last-wins with **original insertion position** — matches both JS object literals and `IndexMap` insert semantics (confirmed by audit `05-dupkeys` MATCH). Build `serde_json::Value` directly; no visitor layer needed.
- Numbers: token → `f64` via `str::parse::<f64>()` (JS `Number()` equivalent for the token forms the scanner produces). `serde_json::Number::from_f64` fails for non-finite → record `InvalidNumberFormat` (see §5.5 known edge).

Public API unchanged: `parse::jsonc(text, source)` returns `Result<Value, ConfigError>`; error text/format code in `error.rs` already matches the reference block and stays untouched. Also port the `printParseErrorCode` name table (exact strings: `InvalidSymbol`, `InvalidNumberFormat`, `PropertyNameExpected`, `ValueExpected`, `ColonExpected`, `CommaExpected`, `CloseBraceExpected`, `CloseBracketExpected`, `EndOfFileExpected`, `InvalidCommentToken`, `UnexpectedEndOfComment`, `UnexpectedEndOfString`, `UnexpectedEndOfNumber`, `InvalidUnicode`, `InvalidEscapeCharacter`, `InvalidCharacter`).

**Why rewrite over the dprint crate**: the `jsonc-parser` crate (dprint, 0.33.1) enforces the same accept/reject grammar, but its `ParseErrorKind` enum and single-error surface do **not** reproduce the reference's `printParseErrorCode` names or multi-error accumulation. The project's #1 goal is 1:1 parity and `debug config`/error output is user-visible. The port is ~450 LOC, removes a dependency, and keeps "own your crate" hygiene. It also yields the `modify`/`applyEdits` machinery we can port later for `Config.update` (JSONC `patchJsonc`).

**Fallback** (only if the team formally deprioritizes error-text parity): adopt the dprint crate with a mapping layer and accept accept/reject-only parity. Not recommended — the fixtures in §7 prove text parity is cheap once the port exists.

**BOM handling (new, verified this pass)**: Node/Bun `readFile(utf8)` strips a leading UTF-8 BOM, so the reference accepts BOM-prefixed config *files* but rejects BOM in `OPENCODE_CONFIG_CONTENT` (probe: exit 1, `InvalidSymbol`). Match by stripping one leading `\u{FEFF}` inside `load_file` only, never in `load_config`/content. This keeps the audit's `04-bom` fixture passing after the strict parser lands.

## 5. Exact error semantics to match (all runtime-verified this pass)

### 5.1 JSONC parse errors — error block format
`parse.ts` builds the message exactly as Rust `error.rs` already formats it (`Config file at {path} is not valid JSON(C)` + `: ` + `\n--- JSONC Input ---\n{text}\n--- Errors ---\n{issues}\n--- End ---`). Per-error line:

```
{printParseErrorCode(error)} at line {line}, column {column}
   Line {line}: {problemLine}
{pad(column + 9)}^
```
where `line = text.substring(0, offset).split("\n").length`, `column = lastLine.length + 1`, and offset is the **token start offset**. Errors joined by `\n`; `problemLine` omitted when the offset's line is empty. (CLI `Error:` prefix/ANSI styling is Agent 17's `cli/error.ts` concern; my domain guarantees the message body.)

### 5.2 Probe-derived accept/reject table (differential fixtures, §7)
| Input | Reference behavior |
|---|---|
| `'x'` single-quote string | InvalidSymbol @quote + ValueExpected |
| `{model: "x"}` unquoted key | InvalidSymbol + PropertyNameExpected + ValueExpected |
| `01` leading zero | CommaExpected + PropertyNameExpected + ValueExpected (token boundary, not a number error) |
| `+1` | InvalidSymbol + ValueExpected |
| `0x10` | InvalidSymbol |
| `NaN` / `undefined` | InvalidSymbol + ValueExpected |
| `1_000` | InvalidSymbol |
| `.5` | InvalidSymbol + ValueExpected |
| `1.` / `1e` | UnexpectedEndOfNumber |
| `1 2` (two root values) | EndOfFileExpected @second token |
| whitespace-only | ValueExpected @EOF |
| comments + trailing commas (`allowTrailingComma`) | accepted |
| duplicate keys | last-wins, accepted |
| `{"model": 1e999}` | parse OK (JS stores Infinity) → schema error `Expected string \| undefined, got Infinity model` |
| BOM file | accepted (BOM stripped at read) |
| BOM in `OPENCODE_CONFIG_CONTENT` | rejected InvalidSymbol |

### 5.3 CONFIG-002 — whitespace-only
`load.rs:132` `text.trim().is_empty()` → change to `text.is_empty()`. Reference short-circuits only on `if (!text)` (`config.ts:242`).

### 5.4 CONFIG-006 — non-ENOENT read errors
`load.rs:128-131`: map read errors like `readFileStringSafe` (`fs-util.ts:62-67`) + `.pipe(Effect.orDie)` (`config.ts:185`):
- `NotFound` → `Ok(Info::default())`
- `PermissionDenied` → `Ok(Info::default())` (reference swallows EACCES too)
- everything else (EISDIR, EINVAL, …) → `Err(ConfigError::Io { path, error })`. Reference renders this as the Effect defect `Unexpected error\nBadResource: FileSystem.readFile (path)` (probed); exact CLI text is Agent 17's, but the error must **propagate** so the CLI exits 1 — never `Ok(default)`.

### 5.5 CONFIG-003 — TOML migration (non-validating)
Rewrite `migrate_legacy_toml` to mirror `config.ts:262-276` exactly:
1. `toml::from_str` — on error, swallow (`.catch(() => {})`): return `Ok(current)` untouched, no write, `config` left in place (audit `toml-bad-syntax` MATCH).
2. Build `Value::Object(map)` from the TOML (keeps **all** keys — no `parse::schema` validation, no `normalize_loaded_config`).
3. `map.remove("provider")`/`map.remove("model")`; if **both** were strings, `map["model"] = "{provider}/{model}"`; set `map["$schema"] = "https://opencode.ai/config.json"`.
4. `merged = merge_deep(to_value(current), map)` (rest keys override, matching `result = mergeConfig(result, rest)`).
5. Write `config.json` as `serde_json::to_string_pretty(merged)` (byte parity with reference, no trailing newline — audit `toml-clean` MATCH), then unlink `config`.
6. Return `from_value::<Info>(merged)` — unknown legacy keys (`theme`) are dropped at the `Info` boundary in memory (transient: only visible in the migrating process's `debug config`; the reference keeps them because its `Info` is a plain object). Side-effect files byte-match; document this single residual in-memory divergence (matches audit `toml-with-unknown-key` DIFF-CONTENT, which we shrink to just the in-memory `theme` key).

### 5.6 CONFIG-004 — `tools` retention
`load.rs:353-354`: `result.tools.take()` → iterate `result.tools.as_ref()` without removing. Reference `config.ts:553-564` only reads. Verified: reference `debug config` emits both `tools` and derived `permission` (`tools:{"bash":true,"edit":false}` → `permission:{"bash":"allow","edit":"deny"}`).

### 5.7 CONFIG-005 — plugin URL normalization
`load.rs:682`: `path_to_file_url(&base.join(specifier))` keeps `./`. Replace with a Node-`path.resolve(base, spec)`-equivalent lexical normalize (resolve `.`/`..`, drop trailing `/`, keep absolute spec as-is). `path.resolve` is lexical — do **not** canonicalize symlinks. Also mirror `pathToFileURL(...).href` percent-encoding for `%`/`?`/`#` and non-ASCII bytes (Node encodes; probe `file:///tmp/…/local-plugin` had no `./`). Existing `extra.rs` tests keep passing (they assert `ends_with("/plugin.ts")`); add a no-`./`-segment assertion.

### 5.8 CONFIG-007 — `$schema` write-back
`load.rs:167-180`: replace `text.find('{')` with `^\s*\{` semantics over the JS `\s` set (space, tab, LF, CR, VT, FF, NBSP, Unicode Zs, `\uFEFF`): strip the leading whitespace run and the first `{`, emit `{\n  "$schema": "<schema>",` + rest (leading whitespace is **dropped**, matching the probed reference write-back of `\n\n{\n  "model": "x"}` → `{\n  "$schema":…`).

### 5.9 Known edge — non-finite numbers
Reference stores `Infinity` (JS) and fails at schema decode (`got Infinity model`). `serde_json` cannot represent it. Choose: record `InvalidNumberFormat` at parse (JsonError) → exit parity preserved, error *category* differs from the reference in this pathological case (`1e999` in a config). Document as acceptable text divergence; add the fixture so the divergence is pinned, not silent.

## 6. Config-into-runtime wiring points

The crate is a tested island (audit §architecture). Reference single-source-of-truth: `Config.loadInstanceState` → `Config.get()` consumed by every service. Rust analog and seams:

1. **Composition root (Agent 02)** — `Context` (`oc-cli/src/cli/context.rs`) gains a lazy `config: InstanceState` populated via `load_instance_state(LoadOptions { directory, worktree, env, username })`; `load_global` caching and per-instance `InstanceState` semantics live in oc-cli's bootstrap. Every command reads one resolved `Info`.
2. **serve/network (Agent 02/12)** — `serve.rs:71 server_config()` returns `None`; replace `cli/network.rs` `ServerConfig` mirror with `Info.server` (`hostname`, `port`, `mdns`, `mdns_domain`, `cors`).
3. **debug config (Agent 12)** — print resolved `Info` **plus** `plugin_origins` (reference does; probe confirmed). The §7 harness output doubles as the golden.
4. **MCP (Agent 13)** — `oc-mcp/src/config.rs` mirror → `oc_config::v1::mcp::Value`; `mcp add`/`mcp list` write-back needs the new `Config.update_global` API (JSONC `patchJsonc` port — defer to the same wave, §9). The `mcp` section is loaded by `Info.mcp` today; nothing spawns servers yet.
5. **Provider (Agent 05)** — registry input `{ provider, disabled_providers, enabled_providers }` from `Info` (registry.rs already takes these as slices). Config fixes do not change `Info`'s shape, so provider wiring is unblocked.
6. **Permission gate (Agent 08, SEC-001)** — consumes `Info.permission` (incl. the `tools`-derived rules §5.6) and `OPENCODE_PERMISSION` merge.
7. **oc-project/oc-server/oc-command mirrors** — `oc-project/src/util/config.rs`, `oc-server/src/instance_handlers.rs:24`, `oc-command/src/command/mod.rs:245` all TODO-replace with oc-config APIs (Agent 02/12).
8. **`Config.update`/`updateGlobal`** (new oc-config API, `lib.rs:36` TODO) — JSONC `patchJsonc` (`modify`+`applyEdits`) for `.jsonc`, `parse+merge+JSON.stringify` for `.json`; needed by MCP add and the desktop-style config editors. Deferred to a follow-up PR within this agent's domain.

## 7. Differential fixture design

Reuse the audit harness: `/root/opencode-rs/target/release/oc04-harness` (src at `/tmp/oc04/harness`) drives `load_instance_state` with disposable `HOME`; oracle is `/root/.opencode/bin/opencode` (`debug config`). Formalize as `crates/oc-config/tests/diff.rs` + `rust-port-remediation/artifacts/config-diff/` runner script.

- **Runner contract**: per case, build isolated `HOME` (seed files), run reference (`HOME=… OPENCODE_CONFIG=… debug config`, capture exit + stdout + stderr) and Rust harness; compare (a) exit code, (b) resolved-config JSON semantically, (c) side-effect file bytes (`$schema` write-back, migrated `config.json`, `config` unlink, `.gitignore`), (d) stderr JSONC error block byte-for-byte, (e) schema-`Invalid` errors: exit code only (serde vs Effect wording is a documented informational divergence).
- **Valid set** (must parse): `01-valid`, `05-dupkeys`, `06-comments`, `07-dupkeys-comma`, `10-dup-mixed`, BOM-file, all ~30 valid-section fixtures from `artifacts/04/valid-diff.txt` (provider/mcp/permission/tools/experimental/tool_output/compaction/skills/references/plugin/watcher/lsp/formatter/server/…), substitution set, precedence set, `tools-map`, `plugin-section`, TOML `toml-clean`/`toml-nested-tables`.
- **Invalid set** (must reject with exact codes): `02-singlequote`, `03-unquoted`, `ws-only`, `garbage`, `truncated`, `09-comment-only`, `top-{bool,array,null}`, plus new §5.2 cases (`01`, `+1`, `0x10`, `NaN`, `1_000`, `.5`, `1.`, `1 2`, `1e999`, BOM-content, dir-as-config, `OPENCODE_CONFIG`-missing vs -directory).
- **Error-text goldens**: capture the reference stderr for each invalid case into `crates/oc-config/tests/fixtures/*.err` and assert the Rust message body matches — this keeps error parity enforced in CI without the oracle.
- **Priority/triage**: run the full 12 audit suites (re-running `artifacts/04/*-diff.txt`) plus the new cases; all previously-MATCH suites must stay MATCH after the strict parser lands (only CONFIG-001/002/003/004/005/006 diverge today).

## 8. Dependencies on other agents

- **Agent 02 (composition root)** — owns bootstrap/`Context` and server mount. Needs my `load_instance_state` seam (§6.1); I keep `LoadOptions`/`InstanceState` API stable and provide `Info.server` access. Order: my config-PR lands first so the composition root loads a correct config.
- **Agent 05 (provider)** — consumes `Info.{provider, disabled_providers, enabled_providers, model, small_model}`. My strict-parser + `tools`/TOML changes do not alter `Info`'s serde shape; provider fixtures must stay green. Coordinate the `provider-section` and `models` differential fixtures (shared §7 set).
- **Agent 13 (protocol/MCP/ACP)** — needs the `mcp` section (`Info.mcp`) to spawn MCP servers and `Config.update_global` for `mcp add`; also PROTO-001's `oc-mcp` mirror promotion. Order: my `update_global`/mirror-promotion PR before MCP wiring.
- Secondary: Agent 08 (permission gate reads `Info.permission` + §5.6 rules), Agent 12 (CLI `debug config`/serve consume the seam), Agent 17 (CLI error rendering consumes `ConfigError::format`), Agent 18 (shared differential harness — my §7 set is the config slice), Agent 19 (SUPPLY-003 attribution header on the jsonc port).

## 9. Risks

- **Multi-error accumulation order**: the reference prints all JSONC errors; a naive "first error" port would silently diverge. Mitigation: port the full `visit` state machine and pin with §7 invalid-set goldens.
- **BOM asymmetry** (file stripped vs content rejected): easy to over-apply. Mitigation: strip only in `load_file`; add both fixtures.
- **Infinity/`1e999`**: Rust cannot represent JS `Infinity`; parse-time rejection preserves exit parity but differs in error category. Documented edge, pinned by fixture.
- **TOML in-memory `theme`**: residual in-memory divergence in the migrating process only; side-effect files byte-match. Keep the write at Value level.
- **plugin URL percent-encoding**: Node `pathToFileURL` encodes `%? #`/non-ASCII; a lexical-only fix diverges on exotic paths. Add encoding when unicode-path fixtures land; low risk (ASCII demo path verified).
- **Schema-`Invalid` text divergence** (serde vs Effect): accepted informational; exit-code-only parity enforced.
- **Port quality**: ~450 LOC hand-port from minified-ish ESM; must be reviewed against the fetched 3.3.1 source. Compile + full suite (`cargo test -p oc-config`) is the gate; previously-MATCH differential suites must stay MATCH.
- **json5 removal**: verified `json5` is referenced only by `oc-config`; remove the workspace dep entry with the rewrite to avoid an orphan.

## 10. Merge-order recommendation (Wave 1 foundations)

Wave 1 (foundations, before integration/composition work): this domain is a hard prerequisite for Agents 02/05/08/12/13 because every consumer needs a *correct* single resolved config.

1. **PR 04a (parser + loader parity, atomic)**: `jsonc.rs` port + drop `json5`; CONFIG-002/006 load-error semantics; CONFIG-003 TOML rewrite; CONFIG-004 tools retention; CONFIG-005 URL normalization; CONFIG-007 write-back; BOM handling. Gates: `cargo build -p oc-config && cargo test -p oc-config`, plus the full §7 differential set vs the oracle (all suites that MATCH today must remain MATCH; CONFIG-001/002/003/004/005/006 flip to MATCH).
2. **PR 04b (fixtures/goldens, can fold into 04a)**: `tests/diff.rs` + captured `.err` goldens so parity is CI-enforced without the oracle; contributes the config slice to Agent 18's shared harness.
3. **PR 04c (wiring seam + `Config.update_global`)**: define the `Context` config seam consumed by Agent 02; port JSONC `modify`/`applyEdits` for `updateGlobal` (MCP add, Agent 13). Merge before Agent 02/13's wiring PRs, after 04a/04b.

Merge order relative to others: **04a → 04b → (02 composition, 05 provider, 13 MCP wiring)**. Do not merge 02/05/13 consumers before 04a — they would bootstrap against the permissive parser and swallow-error loader. CONFIG-001 is the sole release blocker in this domain; 04a clears it.
