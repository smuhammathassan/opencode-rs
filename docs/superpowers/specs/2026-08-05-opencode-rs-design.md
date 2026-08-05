# opencode-rs: Rust port of opencode v1.18.13

Date: 2026-08-05
Status: Approved by user; under implementation

## Goal

1:1 functional parity port of the opencode CLI (v1.18.13) to Rust, optimized for memory and
CPU. Single binary, in-process QuickJS plugin runtime, no Bun/Node. 20 parallel build agents,
each in its own git worktree, integrated at the end.

## Design decisions (brainstormed + independently reviewed)

- **Approach**: module-mirroring Cargo workspace. Each TS subsystem becomes an isolated crate
  (`crates/oc-*`). Review verdict: APPROVE WITH CHANGES — incorporated.
- **Reference**: vendored `reference/` = opencode `v1.18.13`, read-only spec.
- **Plugin runtime**: QuickJS in-process (`quick-js` crate) hosting real JS plugins, polyfilling
  `@opencode-ai/plugin`. Documented divergence: reference spawns a Bun sidecar; ours is in-process
  for the memory/CPU goal. Verified by plugin API contract tests.
- **Verification**: golden tests (exact JSON vs reference-derived fixtures) + differential harness
  (`oc-parity`) comparing stock binary vs `opencode-rs` on config parse, storage files, API JSON.
  CPU/RAM benchmarks under `benchmarks/`.
- **Delivery**: 20 parallel agents, one crate each, in separate git worktrees; branches merged at
  the end; repo at `github.com/smuhammathassan/opencode-rs`.

## Out of scope

`app`, `console`, `web`, `desktop`, `ui`, `slack`, `stats`, `function`, `httpapi-codegen`.

## Crates

See `CONTEXT.md` for the crate table, ownership, and hard rules.

## Definition of done

- `cargo build --workspace` compiles; `cargo test --workspace` passes.
- Differential harness runs against the stock `opencode` binary on a fixture set.
- Benchmarks document CPU/RAM vs stock.
