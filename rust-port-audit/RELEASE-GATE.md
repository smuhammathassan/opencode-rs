# RELEASE-GATE.md

Strict release checklist for opencode-rs (commit `e7fc33e`) against reference v1.18.13.
Status values: PASS / FAIL / PARTIAL / BLOCKED / NOT_TESTED / NOT_APPLICABLE.

| Gate | Required condition | Current status | Evidence | Blocking finding IDs | Owner area | Retest command |
|---|---|---|---|---|---|---|
| Build | `cargo build --workspace --all-features` succeeds | **PASS** | Coordinator + agent 14 runs, 0 errors | — | Build | `cargo build --workspace --all-features` |
| Formatting | `cargo fmt --all -- --check` clean | **PASS** | exit 0 | — | Quality | `cargo fmt --all -- --check` |
| Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean | **FAIL** | exit 101, 45 errors (oc-plugin/oc-util/oc-schema) | RUST-004 | Quality | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Unit tests | `cargo test --workspace` green | **PASS** | 1519 passed, 0 failed | — | Testing | `cargo test --workspace` |
| Integration tests | crate integration tests green | **PASS** (crate-level only) | per-crate suites pass | TEST-001 | Testing | `cargo test --workspace --all-features` |
| End-to-end tests | binary-level E2E passes | **FAIL** | No test invokes the binary; `run`/`serve`/TUI fail | INTEGRATION-001, CLI-001/2/3, TEST-001 | Testing | `opencode run "hi"` |
| CLI parity | ≥95% command scenarios equivalent | **FAIL** | 11.5% (17/148) equivalent | CLI-005, CLI-004 | CLI | `benchmarks/parity.sh` |
| Config parity | accept/reject same inputs as reference | **PARTIAL** | strong semantics; json5 over-permissive | CONFIG-001, CONFIG-002 | Config | Agent 04 fixture harness |
| Database compatibility | schema/migrations match; executable uses DB | **PARTIAL** | DDL/migrations parity; never opened by binary | DB-001 | Database | `opencode session list` |
| Protocol compatibility | MCP/ACP/SSE wire parity vs reference | **FAIL** | unreachable; stale MCP version; SSE framing differs | PROTO-001, SSE-002 | Protocol | `opencode mcp list`, `opencode acp` |
| Provider functionality | provider request end-to-end | **FAIL** | zero HTTP from binary | CLI-001, PROVIDER-001 | Provider | mock-provider `run` |
| Tool safety | tools reachable + approval enforced | **FAIL** | unreachable; approval record-only; OOM/symlink/process-group issues | SEC-001, TOOLS-001/2/3/4 | Tools | tool-loop E2E |
| Plugin isolation | plugins limited/contained, reached by binary | **FAIL** | no limits; arbitrary JS read; test-only | PLUGIN-001/2, RUST-001/2/3 | Plugin | `opencode plugin` |
| Server functionality | `serve` serves HTTP/SSE/WS | **FAIL** | bare TCP socket, HTTP 000 | CLI-002, SERVER-01 | Server | `curl /api/health` |
| TUI functionality | `opencode` launches working TUI | **FAIL** | "TUI is not yet wired" | CLI-003, UX-001/2 | TUI | pty launch |
| Security | threat model findings closed | **FAIL** | SEC-001 critical open; PTY/file/escape | SEC-001/2/3, UX-001 | Security | per-finding tests |
| Dependency vulnerabilities | cargo-audit clean | **NOT_TESTED** | tool unavailable; 319/319 not yanked (manual) | SUPPLY-001 | Supply | `cargo audit` |
| Licensing | LICENSE + attribution present | **FAIL** | no LICENSE file; embedded reference content | SUPPLY-003 | Legal | `test -f LICENSE` |
| Performance claim validity | claims fair + reproducible | **FAIL** | unequal-work benchmark; mislabeled cold | PERF-001, PERF-002 | Perf | fair benchmark |
| Linux packaging | installable release artifact | **NOT_TESTED** | no installer/packaging | RELEASE-004 | Release | packaging pipeline |
| macOS packaging | build + notarization | **NOT_TESTED** | never built on macOS | RELEASE-004 | Release | CI mac job |
| Windows packaging | build + installer | **NOT_TESTED** | never built on Windows; shell gaps | RELEASE-004 | Release | CI win job |
| Upgrade and rollback | signed, working upgrade | **FAIL** | upgrade stub, no signatures, wrong upstream | RELEASE-003 | Release | upgrade E2E |
| Documentation accuracy | docs match behavior | **FAIL** | CONTEXT.md/spec claim "1:1 functional parity"; reality: unwired | INTEGRATION-001 | Docs | doc-vs-runtime review |

## Overall gate

**BLOCKED.** 6 of 25 gates PASS; 2 PARTIAL; 11 FAIL; 5 NOT_TESTED; 0 NOT_APPLICABLE.
The product cannot be released until the integration phase (Phase 1) and the security prerequisites (Phase 0/2) are complete.
