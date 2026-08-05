# Agent 03 — CLI compatibility evidence notes

All differential runs: reference=/root/.opencode/bin/opencode (1.18.13), rust=/root/opencode-rs/target/release/opencode (1.18.13).
Common env: HOME=/tmp/opencode/agent03/home TERM=dumb NO_COLOR=1 CI=1 OPENCODE_DISABLE_AUTOUPDATE=1.
Raw scenario captures: rust-port-audit/artifacts/03-cli/*.json (core, cmd, cmd2, run, debug, misc, models2).

## Signal tests (python, default SIGINT disposition, debug wait)
- SIGINT: ref exit=-2, rust exit=-2 (identical)
- SIGTERM: ref exit=-15, rust exit=-15 (identical)

## Broken pipe (models | head -1)
- ref exit=0, stderr EMPTY
- rust exit=1, stderr "Error:  Unexpected error\n\nBroken pipe (os error 32)"

## Invalid UTF-8 arg (`opencode run <0xff 0xfe 0x80 'hello'>`)
- ref: accepts, runs LLM session, exit 0
- rust: "error: invalid UTF-8 was detected in one or more arguments", exit 1

## Unicode arg (`opencode run 'héllo wörld 日本語 🎉'`)
- ref: streams JSON events, exit 0; rust: not-wired error

## NO_COLOR
- Both binaries IGNORE NO_COLOR; ANSI escapes present in stderr error text in both cases.

## stdin piping (`echo 'piped hello' | opencode run`)
- ref: uses piped input as message, runs LLM, exit 0
- rust: not-wired error exit 1

## Default command (no args, non-TTY)
- ref: launches real TUI (ANSI frames to stdout), runs until timeout (exit 124 via timeout)
- rust: "opencode: starting TUI (requires a TTY)" to stderr, exit 0 immediately

## --print-logs on `models`
- ref: INFO log lines to stderr (timestamp/level/run/message)
- rust: flag accepted, no log output at all

## TTY providers login (pty)
- ref: interactive "Select provider" (only opencode listed)
- rust: numbered provider list (180 providers from models cache)

## Models data divergence
- rust fetch of https://models.opencode.ai/api.json succeeds (HTTP 200 via curl 0.22s) and writes ~/.cache/opencode/models.json (180 providers).
- reference fetch also writes the same 180-provider cache but `models` lists only 8 builtin opencode free models (big-pickle, deepseek-v4-flash-free, ...).
- Even with OPENCODE_MODELS_PATH pointing at a valid 180-provider cache, ref lists only the 8 builtins; a minimal malformed entry ("zorp" without limit.context) makes ref die "undefined is not an object (evaluating 'Z.limit.context')" => ref schema-validates catalog; current api.json fails v1.18.13 validation; port lists raw entries without validation.
- `models --verbose` JSON shape: ref emits normalized Model (providerID/api/capabilities/cost.cache{read,write}); rust emits raw api.json model (description/release_date/cost.cache_read, no providerID/api).
- `models anthropic`: ref "Provider not found: anthropic" exit 1; rust exit 0 lists anthropic models.

## Side effects observed (reference only)
- `mcp add myserver --url ...` wrote /tmp/opencode/agent03/home/.config/opencode/opencode.jsonc (mcp block).
- `plugin somepkg` wrote /root/.opencode/opencode.json (REMOVED after test; /root/.opencode/node_modules etc pre-dated test).
- `upgrade 1.2.3` attempted curl install (network) exit 0; rust refuses with "automatic upgrades are not supported".

## Port defaults
- ref serve/web default port resolves to 4096 (server.ts:120-121 retries 4096 then 0); rust binds port 0 -> OS-assigned random port.
- `serve --port abc`: ref parses NaN -> random port, serves; rust rejects exit 1.
- `serve --port` (missing): ref serves on default; rust "a value is required" exit 1.
