# Plan 17 — Remaining CLI Parity, Secondary Commands & Upgrade/Uninstall

Agent 17 · Wave 0 (READ-ONLY planning) · Domain: CLI surface parity / secondary commands / RELEASE-003
Date: 2026-08-05 · Branch: `fix/audit-remediation`
Reference oracle: `/root/.opencode/bin/opencode` (1.18.13). All stream/exit/byte claims below re-verified live against the oracle on 2026-08-05.

---

## 1. Owned findings

| ID | Severity | Blocker | Summary |
|---|---|---|---|
| CLI-004 | Low | NO | Systematic stdout/stderr stream divergence (CLI-012), `ui::error` double space + `ui::println` padding (CLI-013), broken-pipe exit 1 vs 0 (CLI-016), repeated-flag last-wins vs clap-reject (CLI-018), `help` subcommand missing (CLI-014), `--get-yargs-completions` missing (CLI-025), version-short-circuit order (CLI-020), root-help layout (CLI-023). Affects **every** command. |
| RELEASE-003 | Medium | NO | `upgrade` is a fake-success stub (`upgrade_cmd.rs` prints "automatic upgrades are not supported" exit 0), queries the wrong repo (`sst/opencode` vs reference `anomalyco/opencode`), `--method` discarded, no checksum/signature verification, never installs. `uninstall` removes data dirs but never the binary, never cleans shell config, prints no package-manager hints (RELEASE-015). |
| CLI-005 subset | High | YES | All help-listed secondary commands still return `not_wired` (completion, mcp, acp, stats, agent, plugin, github, pr, console, generate, debug, upgrade, uninstall, web, models display). Mandate: every one either works via a real domain service or is honestly scoped in a new `KNOWN-DEVIATIONS.md` — **no command may immediately return `not_wired`**. |
| CLI-010/011/022 (in scope) | Medium/High | — | `debug startup` prints literal `0.000`; `debug info` os format diverges; `debug paths` order wrong; `debug file read/list` lacks the reference path-containment guard (security-relevant). |

Owned evidence: `rust-port-audit/03-cli-compatibility.md`, `COMMAND-COMPATIBILITY.csv` (148 rows), `rust-port-audit/20-packaging-release-operations.md`, FINDINGS.json CLI-004/CLI-005/CLI-007/CLI-010/CLI-011/CLI-012/CLI-013/CLI-014/CLI-016/CLI-017/CLI-018/CLI-020/CLI-021/CLI-022/CLI-023/CLI-024/CLI-025/RELEASE-003/RELEASE-015, `crates/oc-cli/src/cli/**`, `reference/packages/opencode/src/cli/**`.

**Ownership split (assumed, to be confirmed against Agents 12/13/16 plan files):** Agent 12 = run/session/export/import/db-query + LocalClient dispatch seams (CLI-001, SESSION-001, CLI-005 session/data core); Agent 16 = attach/TUI/mini (CLI-003, CLI-015 exit-code contract on the default command); Agent 10 = serve HTTP (CLI-002) + embedded UI; Agent 13 = MCP/ACP wire infrastructure + protocol version (PROTO-001). **This agent owns the secondary-command oc-cli surface** in §2 and the shared CLI-004 contract layer (§3). Any file overlap with CLI-005 is resolved here: `stats.rs`, `agent.rs`, `mcp.rs`, `acp.rs`, `debug.rs`, `plug.rs`, `github.rs`, `console.rs`, `generate.rs`, `completion.rs`, `upgrade_cmd.rs`, `uninstall.rs`, `web.rs`, `models.rs` are 17's; `run/*`, `session.rs`, `export_cmd.rs`, `import_cmd.rs`, `db.rs` are 12's; `attach.rs`/TUI is 16's.

---

## 2. Per-command disposition (from COMMAND-COMPATIBILITY.csv)

Legend: **I** = implement (17), **Dn** = delegate (owner agent), **S** = honestly scope in KNOWN-DEVIATIONS.md.

| CSV command | Rows | Ref behavior | Disposition | Notes / dependency |
|---|---|---|---|---|
| `completion` | 18–20 | bash script to stdout, exit 0; positional ignored; `completion --help` → root help stderr exit 1 | **I** | Emit byte-identical 932B yargs script (captured). Add hidden `--get-yargs-completions` global (row 21). |
| `help` | 7 | `opencode help` → help to **stdout** exit 0; `help <word>` → logo+root help stderr exit 1 | **I** | Replaces `disable_help_subcommand=true`. |
| `models` | 40–44, 116, 124, 126, 149 | stdout `provider/model` lines; `--verbose` normalized Model JSON; `<provider>` missing → exit 1 `Provider not found`; `--refresh` msg to stderr | **I** | Render source = oc-provider registry view (Agent 09). Catalog divergence (ref lists 8 builtin, port lists 180) → **S** if Agent 09 cannot replicate ref's ProviderV2 validation; `models --verbose` JSON schema parity otherwise **S**. |
| `providers list/ls/logout` | 65–67, 69 | box to **stdout**; glyph `┌ Credentials ~/…`; logout box + `✖ No credentials found` | **I** | Display + logout (auth removal) mine; interactive login flows = Agent 05 (SEC-005). Stream + glyph parity. |
| `mcp` | 52–59, 128 | `add` writes config (side effect) exit 0; `list/ls` status table stdout; `auth list`; `logout`/`debug` | **I** | `add` non-interactive = oc-config jsonc-preserving write (Agent 04) + oc-mcp; `auth`/`debug` status = oc-mcp (Agent 13). Name-less interactive selectors → **S** (no clack-equivalent prompt lib). |
| `acp` | 120 | starts in-process server + stdio NDJSON ACP bridge, runs until stdin ends | **I** | oc-acp `Service` (exists, `oc-acp/src/service.rs`) + in-process server (Agent 10) + stdio framing per `reference/.../acp.ts`. If server unwired by Wave 4 → **S**. |
| `stats` | 73, 145 | OVERVIEW / COST & TOKENS / MODEL USAGE / TOOL USAGE tables to **stdout**; empty DB → zeroed tables exit 0 | **I** | Aggregation driver over oc-database (Agent 03) + usage accounting (Agent 06, LLM-001). Renderer exists (`stats.rs::display_stats`); wire data + stream. Empty-DB zero-table path makes it implementable independently. |
| `agent` | 146–148 | `list` → `name (mode)` + permission JSON stdout; `create` writes `agents/*.md` (non-interactive path) | **I** | list = oc-command agent dir scan; create non-interactive = LLM generate (Agents 07/09) + frontmatter write (oc-command). Interactive create → **S**. |
| `plugin`/`plug` | 84–85, 144 | installs npm module + patches config (side effect) exit 0 | **I** | oc-plugin install/npm (Agent 15) + oc-config patch (Agent 04). |
| `github` | 78–79, 140 | `install` writes GitHub Action workflow; `run` full event-driven agent (Octokit + session/LLM stack) | **I** + **S** | `install` implementable (workflow file write). `run` full agent loop needs session/LLM/share stack (Agents 12/07/10) → **S** with clean error (ref also exits 1 on no-event: `Unsupported event type`). |
| `pr` | 80, 141 | `gh pr checkout` then re-exec TUI with `-s <session>` | **I** + **D16** | Checkout path already works; fix stream/exit (ref `Could not find git repository` clean error, exit 1, no `Unexpected error`). TUI re-exec = Agent 16 (CLI-003); until then **S** note (`(opening the TUI is not yet wired)` already printed). |
| `console` | 91–96, 123 | `logout/switch/orgs/open` non-logged-in → clean messages exit 0; `login` device-code flow | **I** | Non-interactive paths trivially implementable (oc-cli-local state or oc-sync account if Agent 14 provides it). `login` device-code HTTP flow implementable with reqwest; interactive poll/spinner → **S** if no prompt lib. |
| `generate` | 90, 122 | full OpenAPI JSON + `x-codeSamples` to stdout exit 0 | **I** | oc-server `openapi::document()` (exists, `oc-server/src/openapi.rs`) + inject code samples + prettier-style 120-width formatting. |
| `debug` | 97–106, 132–136 | config JSON; lsp; rg; file read/list (containment guard); scrap; skill; snapshot; startup (real ms); agent; v2; info (os `Linux … x64`); paths (home-first); wait | **I** | `startup`/`info`/`paths`/`file` (guard) fully mine. `config` = oc-config (Agent 04); `rg` = oc-util ripgrep; `scrap`/`snapshot` = oc-project; `skill`/`agent` = oc-command; `lsp` = oc-llm/oc-core; `v2` = oc-provider catalog. Deeper subcommands not wired by Wave 4 → **S**. |
| `upgrade` | 70–71, 129 | real install (curl/npm/brew/…) exit 0; `--method bogus` exit 1 | **I** (RELEASE-003) | See §4. No fake success. |
| `uninstall` | 72, 130 | dry-run/force box to stdout; removes dirs; prints `rm "<binary>"` + shell-config cleanup + pkg-manager uninstall hints | **I** (RELEASE-015) | See §4. |
| `web` | 50, 131 | serves web + prints URLs, opens browser, runs forever | **I** + **D10** | Real HTTP/embedded UI = Agent 10 (CLI-002/RELEASE-017). My surface: streams, logo, URL print, browser open. If UI assets not embedded by Wave 4 → **S** (replace `(web interface not yet wired)` with honest note; still bind + print URLs, exit 0 like ref before open fails). |
| `serve` | 45–49, 118 | HTTP server; default port 0→4096; `--port abc` NaN→random (ref) | **D10** | HTTP + port semantics = Agent 10. I fix only shared help-layout/stream parity. |
| `attach` / default TUI | 60–63, 17 | TUI/mini | **D16** | Exit-code contract (cd-fail exit 0, CLI-015) lives here. |
| `run` | 23–39, 117 | LLM session | **D12** | `--replay-limit -1`, repeated-flag parse rows: parse semantics fixed in shared args.rs (§3) — coordinate. |
| `session` | 81–83, 142–143 | list/delete | **D12** | Rows 142–143 are help-layout only (Low, mine). |
| `export` / `import` | 74–76, 138–139 | session JSON | **D12** | `import` `File not found` clean error + exit parity (row 76) = shared error contract (§3). |
| `db` | 86–89, 137 | path works; query/shell | **D12/D03** | `db path` already byte-identical; query = Agent 12 over Agent 03 DB. |
| `--print-logs` | 107 | stderr INFO logs | **D19** | RELEASE-001 (Agent 19) owns logging. |
| invalid-utf8 | 109 | ref accepts invalid UTF-8 arg | **S** | clap `OsString` migration is invasive; Low. Document in KNOWN-DEVIATIONS.md. |
| broken-pipe | 111 | `models \| head -1` exit 0 silent | **I** | §3.3. |

---

## 3. CLI-004 systemic fixes (the output contract)

### 3.1 Stream contract (`ui.rs`, per-command call sites)
Add `ui::println_out`/`print_out` (stdout). Rules derived from live oracle:
- **stdout**: clack `Prompt.intro/log/outro`, `console.log`, `process.stdout.write` → all box/table UIs and data output: `providers list/ls/logout` box, `mcp add/list/auth list/logout/debug` boxes, `stats` tables, `uninstall`/`upgrade` boxes, `models` lines, `agent list`, `session list` table/json, `db` output, `debug paths/info/scrap/skill/v2` output, `generate` JSON, `completion` script, `--get-yargs-completions` words, `help` (bare), `serve` `Warning:`/`listening` lines, `web` URLs.
- **stderr**: `UI.println`/`UI.print` (logo, status spinners, `Models cache refreshed`, console `Not logged in`, account lines), `UI.error` (all `Error:` lines), root/`--help` help text (logo-prefixed via `show()`), `completion --help`.
- Exceptions to keep: stray reset `\e[0m\n` that the ref writes to stderr on empty-box paths (captured in fixtures).

### 3.2 Error/text spacing (`ui.rs:27-54`)
- `error()`: build one concatenated string `{DANGER_BOLD}Error: {NORMAL}{message}` (fixes `Error:  ` double space) — **fixes every error line in every command** (rows 23, 27–33, 43, 62–63, etc.).
- `println`/`print`: keep `join(" ")` (matches ref `message.join(" ")`), but audit call sites that pass split style/text segments where the ref passes one pre-concatenated string (e.g. `models --refresh` ` Models cache refreshed ` padding, rows 41/149). Normalize affected sites to single-string args.
- `empty()`: preserve ref `blank` semantics (only first `empty()` prints).

### 3.3 Broken pipe (`main.rs`)
- `#[cfg(unix)]` ignore SIGPIPE at `main()` start (libc `signal(SIGPIPE, SIG_IGN)`).
- Top-level error path (`cmd/mod.rs:60-75`) and all `writeln!`/`println!` data paths: map `io::ErrorKind::BrokenPipe` → silent exit 0. Reference `models | head -1` = 0.

### 3.4 Repeated flags (`args.rs`, coordinate with 12/16)
- Add `#[arg(overrides_with_self = true)]` to scalar options where yargs is last-wins: run `--model/-m`, `--format`, `--session/-s`, `--password/-p`, `--username/-u`, `--dir`, `--agent`, `--variant`, `--port`, tui `--model/-m`, `--session/-s`, `--prompt`, `--agent`, global `--log-level`. Vec options (`-f`, mcp `--env/--header`) already append — matches yargs array semantics. Rows 35/36 then proceed to message validation like ref.

### 3.5 Version short-circuit (`main.rs`)
- Pre-scan argv (before `--`) for `-v`/`--version` → print `1.18.13` exit 0 **before** clap validation (fixes row 11 `--log-level BOGUS --version`). Preserves `run -v`/`run --version` (rows 37–38).

### 3.6 `help` subcommand + `completion --help` (`args.rs`, `main.rs`)
- Re-enable a `help` command variant: bare → root help to **stdout** exit 0 (row 7); with any arg → logo + root help to **stderr** exit 1. `completion --help` → same stderr-root-help exit 1 (row 20).

### 3.7 Shell completion protocol (`completion.rs`, `main.rs`)
- `completion` (and `completion bash|zsh|fish`, positionals ignored): emit the exact captured bash script (`###-begin-opencode-completions-###` … `###-end-opencode-completions-###`) to stdout, exit 0.
- Hidden global `--get-yargs-completions`: when present, consume the yargs completion args (`<current> <opencode> <word...>`) and print the word list to stdout, exit 0: top-level `completion acp mcp $0 attach run debug providers agent upgrade uninstall serve web models stats export import github pr session plugin db` (byte-verified, incl. `$0`); per-subcommand words for `mcp add list auth logout debug`, `debug config lsp rg file scrap skill snapshot startup agent v2 info paths wait`, `providers list login logout`, `session list delete`, `db $0 path`, `agent create list`, etc.

### 3.8 Root/subcommand help layout (CLI-023, Low)
- New `cli/help_text.rs` producing yargs-style layout: `Commands:` list with aliases (`[aliases: auth]`, `[aliases: plug]`) and `[default]` marker on `opencode [project]`, then `Positionals:` / `Options:`. Remove the leaked `GlobalArgs` doc-comment from the clap description. Root + per-subcommand `--help` goldens against captured `03-cli/*.json`.

---

## 4. RELEASE-003 — upgrade & uninstall

### 4.1 `upgrade` (`cli/cmd/upgrade_cmd.rs`, `cli/upgrade.rs`)
- **Remove fake success.** Paths:
  1. `target == current` → keep skip message, exit 0 (matches ref).
  2. `--method bogus` → clap choice error exit 1 (already matches).
  3. Real upgrade: detect method via `Installation::method()` (binary at `~/.cache/opencode/bin` → `curl`; else env/config → npm/pnpm/bun/brew/choco/scoop). Implement `curl` method: fetch release manifest from the **port's configured repo** (`OPENCODE_UPGRADE_REPO`, default = port origin, NOT `sst/opencode`), download the matching asset, **verify SHA-256 checksum** published alongside the release (ref has none — port adds it; document as a favorable deviation in KNOWN-DEVIATIONS.md), atomic-replace the binary, exit 0. Package-manager methods shell out to the documented command and surface stderr on failure (mirror `reference/.../upgrade.ts` error handling).
  4. No releases / no checksum / unreachable repo: print an honest error (`No verifiable releases for opencode-rs channel …`), **exit 1** — never a printed "Done" + exit 0 (RELEASE-003 fake-success).
- Fix `fetch_latest()` upstream constant (currently `sst/opencode`, `cli/upgrade.rs:43`) to the configurable port repo; keep `User-Agent`.

### 4.2 `uninstall` (`cli/cmd/uninstall.rs`)
- Add to the dry-run summary: `✓ Binary: <path>` and `✓ Shell PATH in <shellConfig>` and package-manager hint line (rows 72, 130 stdout-box parity).
- Implement: method detection; shell-config file detection + `# opencode`/PATH cleanup (`reference/.../uninstall.ts:235-315`); after removing data dirs, print `To finish removing the binary, run: rm "<binary>"` (+ `rmdir "<binDir>" 2>/dev/null` when the dir is under `.opencode`); for pkg-manager methods print (and with `--force` run) the uninstall command, surfacing failure hints. Non-interactive confirmation: currently `--force`/`--dry-run` both print hints; keep but match ref prompt behavior for the missing-force case (ref `prompts.confirm` interactive; honest-scope to a printed `? Are you sure…` + exit 0 like today, documented).

---

## 5. Files to change

| File | Change |
|---|---|
| `crates/oc-cli/src/main.rs` | SIGPIPE ignore; argv `-v/--version` pre-scan; `help` routing; `--get-yargs-completions` dispatch; version const stays (RELEASE-002 = Agent 19). |
| `crates/oc-cli/src/cli/args.rs` | `overrides_with_self` on scalars; `Help` command variant; hidden `--get-yargs-completions` global; drop leaked doc-comment from `GlobalArgs`. |
| `crates/oc-cli/src/cli/ui.rs` | `error()` single-concat; add stdout helpers; call-site spacing audit; `empty()` blank semantics. |
| `crates/oc-cli/src/cli/cmd/mod.rs` | BrokenPipe→0 in top-level error path. |
| `crates/oc-cli/src/cli/help_text.rs` (new) | yargs-layout help renderer. |
| `crates/oc-cli/src/cli/cmd/completion.rs` | bash script + `--get-yargs-completions` word lists. |
| `crates/oc-cli/src/cli/cmd/upgrade_cmd.rs`, `cli/upgrade.rs` | RELEASE-003 (§4.1). |
| `crates/oc-cli/src/cli/cmd/uninstall.rs` | RELEASE-015 (§4.2). |
| `crates/oc-cli/src/cli/cmd/models.rs`, `providers.rs` | stream/glyph parity; provider lookup exit 1; registry view hook. |
| `crates/oc-cli/src/cli/cmd/mcp.rs` | add/list/auth-list/logout/debug over oc-config + oc-mcp; honest-scope selectors. |
| `crates/oc-cli/src/cli/cmd/acp.rs` | stdio bridge over oc-acp Service (+ server). |
| `crates/oc-cli/src/cli/cmd/stats.rs` | aggregation driver + stdout tables. |
| `crates/oc-cli/src/cli/cmd/agent.rs`, `plug.rs`, `github.rs`, `console.rs`, `generate.rs`, `pr.rs`, `debug.rs`, `web.rs` | per disposition §2. |
| `crates/oc-cli/src/cli/cmd/serve.rs`, `run/*`, `session.rs`, `export_cmd.rs`, `import_cmd.rs`, `db.rs`, `attach.rs` | **not touched** (12/10/16/03); only shared `args.rs`/`ui.rs` fixes apply. |
| `rust-port-remediation/artifacts/KNOWN-DEVIATIONS.md` (new) | honest scopes: invalid-UTF-8, models catalog divergence, interactive selectors/login flows, github run, embedded web UI, acp-before-server, checksum-stronger-than-ref upgrade. |
| `crates/oc-cli/Cargo.toml` | add `libc` (SIGPIPE), `sha2` (checksums); optional `dialoguer` if interactive prompts approved. |

---

## 6. Output-parity checklist (applies to every changed command)

- [ ] Exit codes: all rows match reference (0/1; never exit 1 where ref exits 0 on handler-level returns).
- [ ] Streams: box/table/data → stdout; logo/status/errors → stderr; per-command map in §3.1.
- [ ] `Error: ` single space everywhere (no `Error:  `); no `Unexpected error` where ref prints a clean message (`pr`, `import`, `version`-as-path rows 4/8/22/76/80).
- [ ] No extra space padding from `ui::println` joins (rows 41/50/84/149).
- [ ] Broken pipe (`| head -1`) → exit 0, silent.
- [ ] Repeated scalar flags → last-wins (rows 35/36).
- [ ] `-v/--version` short-circuits before value validation (row 11).
- [ ] `help` (stdout/exit0), `help <word>` (stderr/exit1), `completion --help` (stderr/exit1).
- [ ] `--get-yargs-completions` word lists byte-match (incl. `$0`).
- [ ] `debug paths` order home-first; `debug info` os = `Linux <release> x64`; `debug startup` real ms.
- [ ] `debug file read/list` rejects out-of-project paths (CLI-022): `Path escapes the location`-equivalent, exit 1.
- [ ] `upgrade` never fakes success; `uninstall` prints binary + shell-config + pkg hints.
- [ ] NO command prints "not yet wired"; every S-scoped path prints a specific, honest message and is listed in KNOWN-DEVIATIONS.md.

---

## 7. Test list (differential per command, via Agent 18 TEST-001 harness)

Harness pattern: run oracle and Rust binary with identical isolated `HOME/XDG_*/OPENCODE_MODELS_PATH=…` env; assert stdout bytes, stderr bytes (ANSI normalized where ref-only), exit code.

| Command | Differential cases (assert stdout/stderr/exit) |
|---|---|
| completion | `completion`, `completion bash`, `completion --help` (exit 1), `--get-yargs-completions "" opencode ""`, `"mcp " opencode mcp`, `"debug "`, `"session "`, `"db "`, `"mo" opencode mo` |
| help | `help`, `help run`, `help mcp` |
| models | `models` (vs registry view), `models --verbose opencode`, `models anthropic` (exit 1), `models bogus-provider` (exit 1), `models --refresh` spacing |
| providers | `providers list`, `providers ls`, `providers logout` (no creds), `auth list` |
| mcp | `mcp list`/`ls` (fixture config), `mcp add name --url …` (assert config side effect + success line), `mcp add` validation errors, `mcp auth list`, `mcp logout srv`, `mcp debug srv` |
| acp | `acp --help` (exit 0), short-lived bridge: `printf '{"jsonrpc":"2.0",…}' | opencode acp` framing smoke |
| stats | `stats` empty DB (zero tables, exit 0), `stats` with seeded DB (Agent 03 fixture), `stats --days 30 --tools 5 --models 3` |
| agent | `agent list`, `agent create --path --description --mode --permissions` (file side effect), interactive-create S-path |
| plugin | `plugin somepkg` against fixture npm registry (Agent 15), `plug` alias, empty module exit 1 |
| github | `github` (help exit 1), `github install` (workflow file side effect), `github run` (no event → clean error exit 1) |
| pr | `pr 123` outside repo (clean error, exit 1), inside repo with mock `gh` |
| console | `console logout` (Not logged in, exit 0), `switch`, `orgs`, `open`, `console` bare (help exit 1), `console login` device-code (mock endpoint) |
| generate | `generate` → valid OpenAPI JSON + `x-codeSamples`, exit 0 |
| debug | `config`, `info`, `paths` (order), `startup` (real ms), `file read` (in-project ok / out-of-project exit 1), `file list`, `scrap`, `skill`, `v2`, `wait` (signal equivalence) |
| upgrade | `upgrade` current-version skip exit 0, `upgrade 99.0.0` (no fake success; honest exit 1 without releases), `--method bogus` exit 1, checksum-mismatch rejection, offline repo failure |
| uninstall | `uninstall --dry-run --force -c -d` stdout box parity, real run removes dirs + prints binary rm + shell cleanup, keep flags honored |
| web/serve | `web --help`, `serve --help` layout; `web` binds + prints URLs exit 0 (UI assets S-gated); `serve` HTTP = Agent 10 test |
| CLI-004 | broken-pipe `models | head -1`, repeated `-m -m`/`--format --format`, `--log-level BOGUS --version`, invalid-utf8 (S), every `Error:` single-space assertion |

All fixtures captured from the oracle into `rust-port-remediation/artifacts/17-*` at implementation time (Wave 4) — follow TEST-003 fixture-provenance rule (Agent 18).

---

## 8. Dependencies on other agents

| Agent | Deliverable I depend on | My need | Wave |
|---|---|---|---|
| 03 | `Database::open` wiring, DB fixtures, DB-002 JSON columns | stats aggregation, db-adjacent reads | 3–4 |
| 04 | jsonc-parser-exact config parse/load; config write/merge | mcp add config write, debug config, plugin config patch | 3 |
| 05 | auth UX/login flows (SEC-005) | providers login; console/device auth reuse | 3–4 |
| 07 | runner/tool registry/permission wiring (TOOLS-001) | agent create LLM gen, github run | 4 |
| 09 (provider registry) | oc-provider registry view + models catalog behavior (CLI-011 root cause) | models render parity; debug v2 | 3–4 |
| 10 | serve HTTP (CLI-002), embedded UI (RELEASE-017), port 0→4096 | web real serving; acp in-process server; generate openapi completeness | 4 |
| 13 | MCP/ACP wire + protocol version (PROTO-001) | mcp/acp command wiring; shared word-list/route surface | 4 |
| 14 | oc-sync/oc-command/oc-project wiring per its plan (account service, skills, project listing) | console login, debug scrap/skill/snapshot, agent list | 4 |
| 15 | plugin runtime security fixes + install/npm (PLUGIN-001/002/004, SUPPLY-001) | plugin command | 4 |
| 16 | TUI/attach launch (CLI-003) + exit-code contract (CLI-015) | pr TUI re-exec, `version`-as-path exit 0 | 4 |
| 19 | logging (RELEASE-001), version injection (RELEASE-002), release pipeline/repo (RELEASE-004), SUPPLY-004 | --print-logs; upgrade upstream target + signatures; --version string | 2–4 |
| 18 | binary differential harness (TEST-001) | run the §7 suite | 4 |

---

## 9. Risks

| Risk | Mitigation |
|---|---|
| Ownership overlap with Agent 12's CLI-005 on secondary command files | This plan claims the enumerated `cmd/*.rs`; reconcile against 12/13/16 plans before Wave 4; keep dispatch (`cmd/mod.rs`) routing owned by 12. |
| `args.rs` shared edits (overrides_with_self, version pre-scan) conflict with 12/16 parse work | Land §3.4–3.5 as one atomic commit coordinated with 12/16; differential tests pin behavior. |
| No real release repo for `upgrade` (RELEASE-004 not landed) → cannot do a genuine curl install | Env-configurable `OPENCODE_UPGRADE_REPO` + mandatory checksum + honest exit 1 when no verifiable release; ship with Agent 19's pipeline. |
| Models catalog divergence (CLI-011) root cause unresolved by Agent 09 | Honest-scope `models`/`--verbose` divergence; KNOWN-DEVIATIONS entry; keep exit codes/streams exact. |
| Interactive prompt flows (mcp/auth/console login) have no prompt library | Default to honest-scoped clean errors; add `dialoguer` only if coordinator approves a prompt lib. |
| Web/embedded-UI and acp blocked on server wiring (Agent 10/13) | Scope both with the "binds + prints URLs / clean error, exit 0" honest path until server lands. |
| SIGPIPE ignore leaks into long-running `serve`/`web` | Ignoring SIGPIPE is safe for servers; only writer paths change; verify serve still runs. |
| Help-layout parity (CLI-023) is large text work | Low severity; implement root + top subcommands, gate remainder as Low-scope. |

---

## 10. Merge-order recommendation

1. **Wave 4 (before/with 12/16):** §3 CLI-004 contract layer as one coordinated commit — `ui.rs` error/spacing + streams, SIGPIPE/broken-pipe, version pre-scan, repeated-flag args, `help` subcommand, completion + `--get-yargs-completions`. This unblocks differential fixtures for every other command. Also land: `debug startup/info/paths/file-guard`, `generate` (oc-server openapi), `uninstall`, `upgrade` honest paths, `pr` error/stream fixes, `KNOWN-DEVIATIONS.md` skeleton.
2. **Wave 4, parallel:** `models`/`providers` display parity, `mcp` add/list/status (non-interactive), `stats` empty-DB path.
3. **Wave 5 (after 03/04/05/09/10/13/14/15 land):** `mcp` auth/debug status, `acp` bridge, `stats` real aggregation, `agent`, `plugin`, `console` device login, `github install`, `debug` deep subcommands, `web` real serving, full differential suite (§7) green.
4. **Wave 5 end:** refresh `KNOWN-DEVIATIONS.md` to final state; reopen release gate "CLI command compatibility" (03-cli) + RELEASE-003/015 rows. Last merge before Agent 18's binary differential gate.

Primary gates to reopen: **03-cli command compatibility** (11.5% equivalent today) and **RELEASE-003/RELEASE-015** upgrade/uninstall rows. Success = every help-listed command works via a real service or is honestly scoped; CSV rows owned here flip Equivalent=True at the differential gate.
