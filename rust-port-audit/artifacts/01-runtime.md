# Agent 01 — Runtime scenarios (black-box)

Binary: /root/opencode-rs/target/release/opencode (1.18.13, 8,054,560 bytes)
Reference oracle: /root/.opencode/bin/opencode (1.18.13)

## 1. `opencode run "say hi"` (Rust)
`HOME=/tmp/oc-audit-run timeout 15 opencode run "say hi"`
Output:
```
Error:  the in-process opencode server is not wired yet in this build (TODO(integration): oc-server)
Try `opencode run --attach <url>` to connect to a running opencode server.
```
=> PRIMARY command unusable without `--attach`. LocalClient::create returns Err (oc-cli/src/cli/cmd/run/client.rs:64).

## 2. `opencode serve --port 19999` (Rust)
Prints "opencode server listening on http://127.0.0.1:19999" but:
- GET /health -> empty
- GET /session -> empty
- GET /config -> empty
- GET / -> http 000 (connection failed/reset)
=> binds a bare TCP socket, serves nothing (oc-cli/src/cli/cmd/serve.rs:37).

## 3. Reference `serve --port 20001` (oracle)
- GET /health -> 200, full web app HTML (OpenCode SPA)
- GET /session -> `[]`
=> reference server actually serves HTTP + API. Rust port does not.

## 4. `opencode session list` (Rust)
`Error:  session listing is not yet wired in this build (TODO(integration): oc-database/oc-session)`

## 5. `opencode acp` (Rust)
Binds TCP socket and blocks in `std::future::pending()` forever; no ACP protocol logic
(oc-cli/src/cli/cmd/acp.rs:16).

## 6. `opencode db path` (Rust)
Works: prints /root/.local/share/opencode/opencode.db (local impl).

## 7. `opencode models` (Rust)
"models database is empty; run `opencode models --refresh` to fetch it" (local models_dev).

## 8. `opencode` (no args, non-tty) (Rust)
"opencode: starting TUI (requires a TTY)" — no TUI linked (oc-tui NOT a dep of oc-cli).

## 9. Compile status
cargo check -p oc-session-runner -> OK; cargo check -p oc-server -p oc-acp -p oc-tui -> OK.
Workspace compiles; release binary links.
