# opencode-rs — Architecture Contract

Rust port of **opencode v1.18.13** (github.com/sst/opencode). Goals, in priority order:

1. **1:1 functional parity** with the reference — same CLI surface, config, storage layout,
   API JSON, part/message formats, plugin behavior.
2. **Minimal memory/CPU footprint** — single binary, in-process QuickJS for JS plugins,
   no Bun/Node runtime.
3. **Stability** — every crate is tested; the workspace compiles with `cargo build`.

## Reference source

`reference/` is a vendored clone of the opencode monorepo at tag `v1.18.13`. **It is read-only
spec material — never edit it.** Navigate it to mirror behavior:

- `reference/packages/schema/src/` — zod data schemas (Part, Message, Session, Agent,
  Command, Connection, Credential, Event, File, LLM, Identifier, Location). The source of
  truth for serialized JSON.
- `reference/packages/opencode/src/` — the CLI app: `cli/`, `session/`, `tool/`, `storage/`,
  `provider/`, `plugin/`, `mcp/`, `lsp/`, `auth/`, `permission/`, `server/`, `command/`,
  `project/`, `sync/`, `acp/`, etc.
- `reference/packages/core/src/` — core engine: `config/`, `database/`, `session/`,
  `tool/`, `plugin/`, `storage`, `event`, `file`, `control-plane/`, `fs-util`, `ripgrep`.
- `reference/packages/llm/src/` — provider wire protocols + tool runtime.
- `reference/packages/server/` — HTTP server (`api.ts`, `routes.ts`, `handlers/`).
- `reference/packages/client/` + `reference/packages/protocol/` — RPC contract.
- `reference/packages/tui/` + `reference/packages/session-ui/` — TUI (port to ratatui).

## Workspace

Cargo workspace at repo root. Crates under `crates/oc-*`. Dependency edges are already
declared in each crate's `Cargo.toml` and in the root `[workspace.dependencies]`.

| Crate | Mirrors | Ownership |
|---|---|---|
| `oc-schema` | `packages/schema` | All shared data types (serde, exact JSON field names & defaults). **THE FOUNDATION.** |
| `oc-util` | `src/util`, `core/fs-util`, `core/ripgrep`, `core/format` | Formatting, env, fs, ripgrep wrapper |
| `oc-config` | `core/config`, `src/config` | opencode.json/.jsonc parse, validate, deep-merge |
| `oc-database` | `core/database` | SQLite schema + migrations |
| `oc-core` | `core` (glue) | Bus/events, background jobs, installation, ide, id, git, project detection |
| `oc-provider` | `src/provider`, `core/credential`, `src/auth` | Model registry, ProviderTransform, auth/keychain |
| `oc-llm` | `packages/llm` | Provider wire protocols, streaming, tool-runtime |
| `oc-plugin` | `core/plugin`, `packages/plugin`, `src/plugin` | QuickJS (rquickjs) host + polyfilled `@opencode-ai/plugin` API |
| `oc-mcp` | `src/mcp` | MCP client (stdio + HTTP/SSE remote) |
| `oc-tool` | `core/tool` **and** `src/tool`, `src/patch`, `src/codemode` | Tool registry + builtin tools |
| `oc-session` | `core/session`, `src/session` (minus runner) | Session types, part generation orchestration |
| `oc-session-runner` | `core/session/runner` | The LLM loop (`llm.ts`), retries, abort |
| `oc-acp` | `src/acp` | Agent Client Protocol |
| `oc-server` | `packages/server`, `src/server` | HTTP API, SSE, WebSocket, CORS, middleware |
| `oc-client` | `packages/client`, `protocol`, `packages/sdk` | Typed RPC client + contract |
| `oc-tui` | `packages/tui`, `session-ui` | ratatui TUI (behavioral + layout parity) |
| `oc-command` | `src/command`, `src/skill`, `src/question` | Slash commands, skills, interactive prompts |
| `oc-project` | `src/project`, `src/worktree`, `src/snapshot` | Project bootstrap, worktrees, snapshots |
| `oc-sync` | `src/sync`, `src/control-plane`, `core/control-plane` | Event-sourcing/replay, remote workspaces |
| `oc-cli` | `src/cli`, `src/index.ts` | Binary entrypoint: run, serve, auth, models, logout, etc. |

Out of scope: `app`, `console`, `web`, `desktop`, `ui`, `slack`, `stats`, `function`,
`httpapi-codegen`.

## Hard rules

1. **Exact serialization.** Rust structs must serialize to JSON identical to the reference's
   zod output. Keep field names verbatim; replicate defaults, `null` vs `absent` semantics,
   and ordering conventions of the reference. Write golden tests from fixtures you derive
   from the reference source (or a running reference instance where feasible).
2. **Read the reference first.** Mirror logic module-by-module. Cite the reference file in
   `/// From reference/packages/...` doc comments.
3. **No Bun/Node.** Everything in-process. JS plugins run on QuickJS via the `quick-js`
   crate (add real crates from `workspace.dependencies` as needed).
4. **Compile green.** Your crate must pass `cargo build -p <crate>` and `cargo test -p <crate>`
   before you finish. Fix the workspace root `Cargo.toml` if you need new deps or features,
   but only add what you use.
5. **Own your crate.** Do NOT edit other crates' source. If you need a type from `oc-schema`
   that doesn't exist yet, define a private local mirror in your crate (documented
   `TODO(integration): promote to oc-schema`) — do not block on other agents.
6. **Edition 2021**, Rust 1.97, idiomatic code, `cargo fmt` style, no `unsafe` unless
   unavoidable (document why). No comments unless they add real value.
7. Keep crates lean: no giant monoliths inside a crate; use `mod` files that mirror the
   reference's file structure.

## Testing

- Unit tests next to code; integration tests in `crates/<crate>/tests/`.
- Golden tests: assert exact JSON serialization against fixtures.
- Verify: `cargo build -p <crate> && cargo test -p <crate>`.

## Workflow for this task

You were dispatched to implement ONE crate in a dedicated git worktree (branch). Commit
your work to that branch as you go. Return a summary of what you implemented, what remains
(TODOs), and the exact `cargo build`/`cargo test` results at the end.
