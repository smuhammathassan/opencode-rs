# 00 — Coordinator Scope & Pre-Audit State

## Audit identity

- Rust commit audited: `e7fc33e8359bb064c745761ce8e2f9023ae0ae8c`
- Branch: `main`
- Working tree before audit: **CLEAN** (`git status --short` empty)
- Audit date: 2026-08-05
- Repository: `/root/opencode-rs` (Cargo workspace, 20 crates)

## Reference implementation

- Path: `/root/opencode-rs/reference` (vendored, read-only spec)
- Version: `1.18.13` (from `reference/packages/opencode/package.json`)
- Language/runtime: TypeScript/Bun (bun.lock present; reference binary is a Bun-compiled ELF)
- Reference executable available: `/root/.opencode/bin/opencode` → reports `1.18.13`
- NOTE: The reference is TypeScript/JavaScript on Bun, NOT Go. Adapted reference-side tooling accordingly.

## Environment

- OS: Linux 6.8.0-90-generic x86_64 GNU/Linux (Ubuntu 24.04.4)
- Architecture: x86_64
- rustc: 1.97.1 (8bab26f4f 2026-07-14)
- cargo: 1.97.1 (c980f4866 2026-06-30)
- Reference runtime (bun/node): **MISSING** — reference source cannot be executed directly; behavior must be derived from source + stock binary
- Memory: 15 GiB, 8 vCPUs (AMD EPYC/KVM)

## Available audit tools

- git, cargo/rustc, curl, python3 (assumed), strace absent, etc.

## Missing audit tools (recorded, not installed silently)

| Tool | Status | Reason / gap |
|------|--------|--------------|
| cargo-audit | MISSING | network partial; not installed — dependency vuln scan = manual review of lockfile |
| cargo-deny | MISSING | same |
| cargo-machete / cargo-udeps | MISSING | unused-dep check = manual |
| cargo-outdated | MISSING | version drift = manual |
| cargo-geiger | MISSING | unsafe scan = manual grep |
| cargo-nextest | MISSING | use plain `cargo test` |
| cargo-llvm-cov | MISSING | coverage = inference only |
| hyperfine | MISSING | timing = `/usr/bin/time` + `date +%s%N` |
| valgrind/heaptrack/massif | MISSING | memory = `/usr/bin/time -v` RSS |
| bun / node | MISSING | cannot execute reference source; differential via stock binary only |
| cargo-miri / semver-checks / bloat / fuzz | MISSING | not installed |

## Network

- Partial. `crates.io` reachable for cargo (verified during build). External API fetch uncertain. Audit is designed to work offline.

## Parallel execution model

- Exactly 20 genuine sub-agents launched in parallel (one `task` dispatch per agent in a single message).
- Each agent owns one numbered domain (01–20), writes its own report to `rust-port-audit/NN-domain.md`, works READ-ONLY on production source, and may write only under `rust-port-audit/` or `/tmp`.
- Agents investigate independently; they are instructed not to trust other agents' conclusions, commit reports, or prior summaries.

## Safety rules (audit-wide)

- READ-ONLY on production source (`crates/`, `reference/`, `Cargo.toml`, etc.).
- Allowed writes: `rust-port-audit/**` and OS temp dir only.
- No commits, no pushes, no config/dep/test changes, no mocks replacing real integrations.
- Where a source modification would prove a defect, agents document the experiment instead.

## Ownership of shared artifacts

- Coordinator (post-agent phase) compiles: `FEATURE-PARITY.csv`, `COMMAND-COMPATIBILITY.csv`, `FINDINGS.json`, `TEST-EVIDENCE.md`, `AUDIT-SUMMARY.md`, `RELEASE-GATE.md`.
- Agent 02 owns the initial `FEATURE-PARITY.csv`.
- Agent 03 owns the initial `COMMAND-COMPATIBILITY.csv`.
- Agents save large command outputs under `rust-port-audit/artifacts/`.
