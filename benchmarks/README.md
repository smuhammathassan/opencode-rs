# opencode-rs benchmarks

Differential CPU/RAM comparison: `opencode-rs` (release) vs the stock `opencode`
v1.18.13 binary. Run: `./compare.sh` (results appended to `results.txt`).

## Results — 2026-08-05 (8 runs avg, `target/release/opencode`)

| Scenario | Stock opencode | opencode-rs | Speedup | RSS |
|---|---|---|---|---|
| Binary size | 180 MB | 7.7 MB | 23x | — |
| `--version` cold start | 981 ms | 7 ms | 140x | 185 → 4 MB |
| `--help` | 1003 ms | 9 ms | 111x | 197 → 4 MB |
| `run --help` | 1128 ms | 10 ms | 113x | 193 → 4 MB |
| `models --help` | 1044 ms | 8 ms | 131x | 186 → 4 MB |

## Parity checks

- `cargo test --workspace`: **1519 tests pass** across 20 crates — golden JSON
  serialization tests against the reference v1.18.13 source (config, schema,
  database DDL, tool schemas, SSE frames, JSON-RPC, ACP wire, MCP, CLI routes).
- `./parity.sh` — diffs CLI surface (`--version`, `--help` output) between the
  two binaries for structure parity.

## Notes

- The JS plugin runtime runs in-process via a vendored QuickJS (no Bun/Node
  process), which is the main source of the memory/CPU win.
- Real LLM streaming parity is covered by unit tests (request bodies, SSE
  parsing); end-to-end live calls require provider credentials.
